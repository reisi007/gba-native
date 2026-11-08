use std::path::Path;

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("canonical repository root")
}

#[test]
fn frontend_manifests_and_cutover_gates_are_isolated_from_root_graph() {
    let root = repo_root();
    let desktop_manifest =
        std::fs::read_to_string(root.join("apps/desktop/Cargo.toml")).expect("desktop manifest");
    assert!(
        desktop_manifest.contains(
            "gba-native = { package = \"gba-native-core\", path = \"../../crates/core\" }"
        )
    );
    assert!(!desktop_manifest.contains("gba-native-aot"));

    let desktop_lock =
        std::fs::read_to_string(root.join("apps/desktop/Cargo.lock")).expect("desktop lock");
    assert!(desktop_lock.contains("name = \"gba-native-core\""));
    assert!(!desktop_lock.contains("name = \"gba-native-aot\""));
    assert!(!desktop_lock.contains("name = \"gba-native\"\n"));
    assert!(desktop_lock.contains("name = \"pixels\"\nversion = \"0.17.2\""));
    assert!(desktop_lock.contains("name = \"winit\"\nversion = \"0.30.13\""));
    assert!(desktop_lock.contains("name = \"wgpu\"\nversion = \"29.0.4\""));

    let web_manifest =
        std::fs::read_to_string(root.join("apps/web/Cargo.toml")).expect("browser manifest");
    assert!(web_manifest.contains("name = \"gba-native-web\""));
    assert!(
        web_manifest.contains(
            "gba-native = { package = \"gba-native-core\", path = \"../../crates/core\" }"
        )
    );
    assert!(!web_manifest.contains("gba-native-aot"));
    assert!(!web_manifest.contains("package = \"gba-native\""));
    let web_lock = std::fs::read_to_string(root.join("apps/web/Cargo.lock")).expect("browser lock");
    assert!(web_lock.contains("name = \"gba-native-core\""));
    assert!(web_lock.contains("name = \"gba-native-web\""));
    assert!(!web_lock.contains("name = \"gba-native-aot\"\n"));
    assert!(!web_lock.contains("name = \"gba-native\"\n"));

    let rune_manifest =
        std::fs::read_to_string(root.join("apps/rune/Cargo.toml")).expect("Rune manifest");
    assert!(rune_manifest.contains("name = \"gba-native-rune\""));
    assert!(
        rune_manifest.contains(
            "gba-native = { package = \"gba-native-core\", path = \"../../crates/core\" }"
        )
    );
    assert!(!rune_manifest.contains("gba-native-aot"));
    assert!(!rune_manifest.contains("package = \"gba-native\""));
    let rune_lock = std::fs::read_to_string(root.join("apps/rune/Cargo.lock")).expect("Rune lock");
    assert!(rune_lock.contains("name = \"gba-native-core\""));
    assert!(rune_lock.contains("name = \"gba-native-rune\""));
    assert!(!rune_lock.contains("name = \"gba-native-aot\"\n"));
    assert!(!rune_lock.contains("name = \"gba-native\"\n"));

    let native_gate =
        std::fs::read_to_string(root.join("scripts/frontend.sh")).expect("canonical native gate");
    assert!(native_gate.contains("exec bash \"$ROOT/scripts/rune.sh\""));
    let rune_gate = std::fs::read_to_string(root.join("scripts/rune.sh")).expect("Rune host gate");
    assert!(rune_gate.contains("cargo fmt --manifest-path apps/rune/Cargo.toml --all --check"));
    let desktop_reference = std::fs::read_to_string(root.join("scripts/frontend-reference.sh"))
        .expect("legacy desktop reference gate");
    assert!(desktop_reference.contains("apps/desktop/Cargo.toml"));

    let browser_gate =
        std::fs::read_to_string(root.join("scripts/web.sh")).expect("canonical browser gate");
    assert!(browser_gate.contains("scripts/rune-browser.sh"));
    assert!(browser_gate.contains("scripts/rune-web-screenshots.sh"));
    assert!(browser_gate.contains("partial evidence only"));
    let browser_reference = std::fs::read_to_string(root.join("scripts/web-reference.sh"))
        .expect("legacy browser reference gate");
    assert!(browser_reference.contains("apps/web/Cargo.toml"));

    let rune_workflow =
        std::fs::read_to_string(root.join(".github/workflows/rune.yml")).expect("Rune workflow");
    assert!(rune_workflow.contains("run: bash scripts/frontend.sh"));
    assert!(rune_workflow.contains("run: bash scripts/web.sh --ci"));
    assert!(rune_workflow.contains("scripts/rune-ios-simulator.sh"));
    let frontend_reference_workflow =
        std::fs::read_to_string(root.join(".github/workflows/frontend.yml"))
            .expect("desktop reference workflow");
    assert!(frontend_reference_workflow.contains("scripts/frontend-reference.sh"));
    let web_reference_workflow = std::fs::read_to_string(root.join(".github/workflows/web.yml"))
        .expect("browser reference workflow");
    assert!(web_reference_workflow.contains("scripts/web-reference.sh --ci"));
}
