//! Pinned wasm-bindgen CLI discovery.
//!
//! The repository owns its browser tooling under the ignored `target/tools`
//! tree (see `scripts/bootstrap-web-tools.sh`). This module never falls back to
//! a global binary and never installs anything: a missing or mismatched CLI is
//! an explicit failure with the command needed to fix it.

use std::path::{Path, PathBuf};
use std::process::Command;

use gba_native::error::{Error, Result};

/// Exact wasm-bindgen CLI pin shared with `scripts/bootstrap-web-tools.sh` and
/// the generated module manifest.
pub const WASM_BINDGEN_VERSION: &str = "0.2.129";

const BINDGEN_IN_ROOT: &str = "target/tools/bin/wasm-bindgen";
const BOOTSTRAP_HINT: &str = "run scripts/bootstrap-web-tools.sh to install the pinned tool";

/// Resolve and verify the repository-owned wasm-bindgen CLI.
///
/// The repository root is taken from this crate's compiled-in location, so the
/// check works from any working directory. The reported version must be exactly
/// [`WASM_BINDGEN_VERSION`]; nothing is downloaded or installed here.
pub fn wasm_bindgen_cli() -> Result<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    resolve_wasm_bindgen(&root)
}

/// Resolve `target/tools/bin/wasm-bindgen` below an explicit repository root and
/// verify its version. Split out so tests can cover missing and mismatched
/// tools without the real installation.
pub fn resolve_wasm_bindgen(repository_root: &Path) -> Result<PathBuf> {
    let path = repository_root.join(BINDGEN_IN_ROOT);
    if !path.is_file() {
        return Err(Error::Aot(format!(
            "pinned wasm-bindgen CLI {WASM_BINDGEN_VERSION} is missing at {}; {BOOTSTRAP_HINT}",
            path.display()
        )));
    }
    let output = Command::new(&path)
        .arg("--version")
        .output()
        .map_err(|error| {
            Error::Aot(format!(
                "cannot execute the pinned wasm-bindgen CLI {}: {error}; {BOOTSTRAP_HINT}",
                path.display()
            ))
        })?;
    let reported = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() {
        return Err(Error::Aot(format!(
            "pinned wasm-bindgen CLI {} failed --version with {:?}; {BOOTSTRAP_HINT}",
            path.display(),
            output.status.code()
        )));
    }
    verify_wasm_bindgen_version(&reported)?;
    Ok(path)
}

/// Require the exact `wasm-bindgen <version>` banner.
pub fn verify_wasm_bindgen_version(reported: &str) -> Result<()> {
    let expected = format!("wasm-bindgen {WASM_BINDGEN_VERSION}");
    if reported != expected {
        return Err(Error::Aot(format!(
            "pinned wasm-bindgen CLI mismatch: expected {expected:?}, got {reported:?}; {BOOTSTRAP_HINT}"
        )));
    }
    Ok(())
}
