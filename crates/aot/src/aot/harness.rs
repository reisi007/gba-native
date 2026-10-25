use std::io::Write as _;
use std::path::{Path, PathBuf};

use std::process::Command;

use super::report::NativeReport;
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
    pub fn build(source: &str, dir: &Path) -> Result<Self> {
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

        let target_dir = dir.join("target");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .arg("build")
            .arg("--quiet")
            .arg("--offline")
            .arg("--manifest-path")
            .arg(dir.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()?;
        if !output.status.success() {
            return Err(Error::Native(format!(
                "generated harness build failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let exe = target_dir.join("debug").join(format!(
            "gba-native-aot-harness{}",
            std::env::consts::EXE_SUFFIX
        ));
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

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}
