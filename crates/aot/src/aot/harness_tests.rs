//! Focused path-resolution tests for the generated native harness.
//!
//! These cover the two Cargo layouts without mutating process environment, which
//! is why the resolver takes the inherited target as a parameter.

use std::path::{Path, PathBuf};

use super::harness::harness_executable;

fn normalized(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn host_layout_resolves_below_the_profile_directory_only() {
    let target_dir = Path::new("owned-target");
    let exe = harness_executable(target_dir, "debug", None);
    assert_eq!(
        exe,
        PathBuf::from("owned-target").join("debug").join(format!(
            "gba-native-aot-harness{}",
            std::env::consts::EXE_SUFFIX
        ))
    );
    assert!(!normalized(&exe).contains("//"));
}

#[test]
fn explicit_target_layout_nests_the_triple_before_the_profile() {
    let exe = harness_executable(
        Path::new("owned-target"),
        "debug",
        Some("aarch64-unknown-linux-gnu"),
    );
    assert_eq!(
        normalized(&exe),
        "owned-target/aarch64-unknown-linux-gnu/debug/gba-native-aot-harness"
    );

    let release = harness_executable(
        Path::new("owned-target"),
        "release",
        Some("aarch64-unknown-linux-gnu"),
    );
    assert_eq!(
        normalized(&release),
        "owned-target/aarch64-unknown-linux-gnu/release/gba-native-aot-harness"
    );
}

#[test]
fn explicit_target_uses_the_bounded_executable_suffix() {
    let windows = harness_executable(
        Path::new("owned-target"),
        "debug",
        Some("x86_64-pc-windows-msvc"),
    );
    assert!(normalized(&windows).ends_with("debug/gba-native-aot-harness.exe"));

    let darwin = harness_executable(
        Path::new("owned-target"),
        "release",
        Some("x86_64-apple-darwin"),
    );
    assert!(normalized(&darwin).ends_with("release/gba-native-aot-harness"));
}
