//! Bounded ROM-specific wasm32 AOT module build.
//!
//! The generated crate is a fresh cdylib that depends only on the sibling
//! std-only `gba-native-core` package plus the exact `wasm-bindgen` pin. It is
//! compiled offline for `wasm32-unknown-unknown` with its own target directory
//! and then post-processed by the repository-owned wasm-bindgen CLI into an
//! importable JS glue module plus `_bg.wasm`. Nothing here executes guest code:
//! the produced module is a browser-callable execution artifact, not an HTML/JS
//! application bundle.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::wasm_tool::{WASM_BINDGEN_VERSION, wasm_bindgen_cli};
use gba_native::error::{Error, Result};

/// The only supported WASM build target. Other targets keep using the native
/// executable path (or fail explicitly for other wasm32/wasm64 triples).
pub const WASM_TARGET: &str = "wasm32-unknown-unknown";

const CRATE_NAME: &str = "gba-native-wasm-module";
const LIB_NAME: &str = "gba_native_wasm_module";

/// Reject a WASM target that this bounded build path cannot produce.
///
/// The native branch keeps its existing behavior for every non-WASM triple.
pub fn validate_build_target(target: &str) -> Result<()> {
    if target == WASM_TARGET {
        return Ok(());
    }
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("wasm32") || lower.starts_with("wasm64") {
        return Err(Error::Args(format!(
            "unsupported WASM target {target:?}: only {WASM_TARGET} is supported"
        )));
    }
    Ok(())
}

/// A built, ROM-bound WASM execution module and its JS glue.
pub struct WasmModule {
    js: PathBuf,
    wasm: PathBuf,
}

impl WasmModule {
    /// Build the generated module in a fresh `dir` using the pinned CLI.
    pub fn build(source: &str, dir: &Path) -> Result<Self> {
        let bindgen = wasm_bindgen_cli()?;
        Self::build_with_bindgen(source, dir, &bindgen)
    }

    /// Build with an already resolved wasm-bindgen CLI, so the caller can verify
    /// tool availability before creating its output workspace.
    ///
    /// `dir` must not exist. It is created exclusively with `create_dir`, its
    /// `src` child likewise, and both generated files are created with
    /// `create_new`, so a pre-existing directory, file or symbolic link is
    /// refused rather than reused or followed. The crate owns its target
    /// directory inside `dir`; the caller owns only `dir` and may remove it.
    pub fn build_with_bindgen(source: &str, dir: &Path, bindgen: &Path) -> Result<Self> {
        let target_dir = prepare_crate(source, dir)?;
        let output = Command::new(cargo())
            .arg("build")
            .arg("--quiet")
            .arg("--offline")
            .arg("--release")
            .arg("--target")
            .arg(WASM_TARGET)
            .arg("--manifest-path")
            .arg(dir.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()?;
        if !output.status.success() {
            return Err(Error::Aot(format!(
                "generated WASM module release build for {WASM_TARGET} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let compiled = target_dir
            .join(WASM_TARGET)
            .join("release")
            .join(format!("{LIB_NAME}.wasm"));
        if !compiled.is_file() {
            return Err(Error::Aot(format!(
                "cargo finished but no WASM module was produced at {}",
                compiled.display()
            )));
        }

        let pkg = dir.join("pkg");
        let bindings = Command::new(bindgen)
            .arg("--target")
            .arg("web")
            .arg("--no-typescript")
            .arg("--out-dir")
            .arg(&pkg)
            .arg("--out-name")
            .arg(LIB_NAME)
            .arg(&compiled)
            .output()
            .map_err(|error| {
                Error::Aot(format!(
                    "cannot execute the pinned wasm-bindgen CLI {}: {error}; run scripts/bootstrap-web-tools.sh",
                    bindgen.display()
                ))
            })?;
        if !bindings.status.success() {
            return Err(Error::Aot(format!(
                "wasm-bindgen {WASM_BINDGEN_VERSION} web bindings failed with {:?}: {}",
                bindings.status.code(),
                String::from_utf8_lossy(&bindings.stderr).trim()
            )));
        }

        let wasm = pkg.join(format!("{LIB_NAME}_bg.wasm"));
        let js = pkg.join(format!("{LIB_NAME}.js"));
        for path in [&wasm, &js] {
            let metadata = std::fs::metadata(path).map_err(|error| {
                Error::Aot(format!(
                    "wasm-bindgen did not produce the expected {}: {error}",
                    path.display()
                ))
            })?;
            if !metadata.is_file() || metadata.len() == 0 {
                return Err(Error::Aot(format!(
                    "wasm-bindgen produced an empty or non-file {}",
                    path.display()
                )));
            }
        }
        Ok(Self { js, wasm })
    }

    /// Generated JS glue module (`import init, { run_rom } from ...`).
    pub fn js(&self) -> &Path {
        &self.js
    }

    /// Generated `_bg.wasm` module processed by wasm-bindgen.
    pub fn wasm(&self) -> &Path {
        &self.wasm
    }

    /// Module bytes, read only for metadata hashing.
    pub fn module_bytes(&self) -> Result<Vec<u8>> {
        Ok(std::fs::read(&self.wasm)?)
    }

    /// JS glue bytes, read only for metadata hashing.
    pub fn glue_bytes(&self) -> Result<Vec<u8>> {
        Ok(std::fs::read(&self.js)?)
    }
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Create the fresh generated cdylib crate and return its owned Cargo target
/// directory.
fn prepare_crate(source: &str, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir(dir).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::Aot(format!(
                "WASM module directory {} already exists; build into a fresh directory",
                dir.display()
            ))
        } else {
            Error::Io(error.to_string())
        }
    })?;
    verify_real_directory(dir, "WASM module")?;

    let src = dir.join("src");
    std::fs::create_dir(&src)?;
    verify_real_directory(&src, "WASM module source")?;

    // Same dependency boundary as the native harness: only the sibling
    // std-only core package (renamed to the `gba_native` namespace), never this
    // host-side AOT crate or the root compatibility facade. wasm-bindgen is an
    // exact pin resolved offline from the local Cargo cache.
    let core_dir = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../core"))
        .map_err(|error| Error::Aot(format!("cannot resolve the sibling core package: {error}")))?;
    let escaped = core_dir
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let manifest = format!(
        "[package]\nname = \"{CRATE_NAME}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [lib]\ncrate-type = [\"cdylib\"]\npath = \"src/lib.rs\"\n\n\
         [dependencies]\ngba-native = {{ package = \"gba-native-core\", path = \"{escaped}\" }}\n\
         wasm-bindgen = \"={WASM_BINDGEN_VERSION}\"\n\n[workspace]\n"
    );
    write_new(&dir.join("Cargo.toml"), manifest.as_bytes())?;
    write_new(&src.join("lib.rs"), source.as_bytes())?;
    Ok(dir.join("target"))
}

fn verify_real_directory(path: &Path, label: &str) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::Aot(format!(
            "refusing non-directory or symbolic-link {label} path {}",
            path.display()
        )));
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}
