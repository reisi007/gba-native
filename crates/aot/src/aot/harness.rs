use std::io::Write as _;
use std::path::{Path, PathBuf};

use std::process::Command;

use super::report::NativeReport;
use super::target_build::{executable_suffix, validate_target};
use gba_native::error::{Error, Result};

pub struct NativeHarness {
    exe: PathBuf,
}

impl NativeHarness {
    /// Build a generated harness crate in `dir`.
    ///
    /// `dir` must not exist. It is created exclusively with `create_dir`, its
    /// `src` child likewise, and both files are created with `create_new`, so
    /// a pre-existing directory, file or symbolic link is refused rather than
    /// reused or followed. The caller owns `dir` and may remove only that
    /// directory; no recursive cleanup or replacement is offered here.
    ///
    /// An inherited `CARGO_BUILD_TARGET` is honored rather than overridden: the
    /// nested `cargo build` keeps that target and the returned executable is
    /// resolved from its target-specific directory
    /// (`<target-dir>/<triple>/debug/`). Without the variable the host-native
    /// directory (`<target-dir>/debug/`) is used. Target support is decided by
    /// the actual `cargo`/`rustc` toolchain.
    pub fn build(source: &str, dir: &Path) -> Result<Self> {
        let target = inherited_build_target()?;
        let target_dir = prepare_crate(source, dir)?;
        let output = Command::new(cargo())
            .arg("build")
            .arg("--quiet")
            .arg("--offline")
            .arg("--manifest-path")
            .arg(dir.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()?;
        if !output.status.success() {
            return Err(Error::Native(format!(
                "generated harness build failed for target {}: {}",
                target.as_deref().unwrap_or("host"),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let exe = harness_executable(&target_dir, "debug", target.as_deref());
        if !exe.is_file() {
            return Err(Error::Native(format!(
                "cargo finished but no harness executable was produced at {}; this build path supports executable targets only",
                exe.display()
            )));
        }
        Ok(Self { exe })
    }

    /// Build the generated harness crate in release mode for one explicit Cargo
    /// target triple.
    ///
    /// The caller supplies a fresh `dir` with the same ownership rules as
    /// [`Self::build`]. Cargo uses an own target directory inside `dir`, so the
    /// build is offline and does not take the parent Cargo target lock. Target
    /// support is decided by the actual `cargo`/`rustc` toolchain; this method
    /// makes no cross-compilation or platform-support claim. The returned path
    /// is the target-specific release executable, which is not executed here.
    pub fn build_for_target(source: &str, dir: &Path, target: &str) -> Result<Self> {
        validate_target(target)
            .map_err(|reason| Error::Native(format!("invalid target {target:?}: {reason}")))?;
        let target_dir = prepare_crate(source, dir)?;
        let output = Command::new(cargo())
            .arg("build")
            .arg("--quiet")
            .arg("--offline")
            .arg("--release")
            .arg("--target")
            .arg(target)
            .arg("--manifest-path")
            .arg(dir.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()?;
        if !output.status.success() {
            return Err(Error::Native(format!(
                "generated harness release build for target {target} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let exe = harness_executable(&target_dir, "release", Some(target));
        if !exe.is_file() {
            return Err(Error::Native(format!(
                "cargo finished but no target executable was produced at {}; this build path supports executable targets only",
                exe.display()
            )));
        }
        Ok(Self { exe })
    }

    /// Path to the built harness executable, for diagnostics and no-argument
    /// argument-contract tests.
    pub fn executable(&self) -> &Path {
        &self.exe
    }

    pub fn run(&self, rom: &Path) -> Result<NativeReport> {
        let output = Command::new(&self.exe).arg("--rom").arg(rom).output()?;
        if !output.status.success() {
            return Err(Error::Native(format!(
                "exit {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        NativeReport::parse(&String::from_utf8_lossy(&output.stdout))
    }
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// The Cargo build target inherited from the caller's environment, if any.
///
/// Cargo treats an empty `CARGO_BUILD_TARGET` as "no target", so only a
/// non-empty value selects a target-specific layout. A value that this crate
/// would not accept as one target triple is an explicit error rather than a
/// silent host build.
fn inherited_build_target() -> Result<Option<String>> {
    let Some(raw) = std::env::var_os("CARGO_BUILD_TARGET") else {
        return Ok(None);
    };
    let Some(value) = raw.to_str() else {
        return Err(Error::Native(
            "inherited CARGO_BUILD_TARGET is not valid UTF-8".to_string(),
        ));
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    validate_target(value).map_err(|reason| {
        Error::Native(format!(
            "invalid inherited CARGO_BUILD_TARGET {value:?}: {reason}"
        ))
    })?;
    Ok(Some(value.to_string()))
}

/// Resolve the harness executable Cargo wrote into `target_dir` for `profile`.
///
/// With an explicit `target`, Cargo nests the profile directory under the
/// target triple; without one it uses the host layout. The executable-name
/// suffix follows the same distinction: the host suffix for a host build and
/// the bounded per-target policy for an explicit triple.
pub(super) fn harness_executable(
    target_dir: &Path,
    profile: &str,
    target: Option<&str>,
) -> PathBuf {
    let mut dir = target_dir.to_path_buf();
    if let Some(target) = target {
        dir.push(target);
    }
    let suffix = match target {
        Some(target) => executable_suffix(target),
        None => std::env::consts::EXE_SUFFIX,
    };
    dir.join(profile)
        .join(format!("gba-native-aot-harness{suffix}"))
}

/// Create the fresh generated crate and return its owned Cargo target
/// directory. This is the shared ownership boundary for both build APIs.
fn prepare_crate(source: &str, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir(dir).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::Native(format!(
                "harness directory {} already exists; build into a fresh directory",
                dir.display()
            ))
        } else {
            Error::Io(error.to_string())
        }
    })?;
    let metadata = std::fs::symlink_metadata(dir)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::Native(format!(
            "refusing non-directory or symbolic-link harness path {}",
            dir.display()
        )));
    }

    let src = dir.join("src");
    std::fs::create_dir(&src)?;
    let src_metadata = std::fs::symlink_metadata(&src)?;
    if src_metadata.file_type().is_symlink() || !src_metadata.is_dir() {
        return Err(Error::Native(format!(
            "refusing non-directory or symbolic-link harness source path {}",
            src.display()
        )));
    }

    // The AOT crate lives in `crates/aot`; the harness must depend only on
    // the sibling std-only core package, never on this host-side crate or
    // the root compatibility facade. The generated source keeps using the
    // `gba_native` namespace, so the dependency is renamed to `gba-native`
    // while selecting `gba-native-core` by package name.
    let core_dir = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../core"))
        .map_err(|error| {
            Error::Native(format!("cannot resolve the sibling core package: {error}"))
        })?;
    let manifest_string = core_dir
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let manifest = format!(
        "[package]\nname = \"gba-native-aot-harness\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\ngba-native = {{ package = \"gba-native-core\", path = \"{manifest_string}\" }}\n\n[workspace]\n"
    );
    write_new(&dir.join("Cargo.toml"), manifest.as_bytes())?;
    write_new(&src.join("main.rs"), source.as_bytes())?;
    Ok(dir.join("target"))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}
