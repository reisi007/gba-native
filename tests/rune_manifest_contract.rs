use std::path::Path;

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("canonical repository root")
}

#[test]
fn rune_manifest_and_canonical_gates_are_isolated_from_root_graph() {
    let root = repo_root();
    let root_manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("root manifest");
    assert!(root_manifest.contains("exclude = [\"apps/android\", \"apps/rune\"]"));
    assert!(!root.join("apps/desktop").exists());
    assert!(!root.join("apps/web").exists());

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

    let browser_tools =
        std::fs::read_to_string(root.join("tools/browser/package.json")).expect("browser tools");
    assert!(browser_tools.contains("\"@playwright/test\": \"1.64.1\""));
    assert!(!browser_tools.contains("tailwind"));
    assert!(!browser_tools.contains("daisyui"));
    let browser_lock = std::fs::read_to_string(root.join("tools/browser/package-lock.json"))
        .expect("browser tools lock");
    assert_eq!(browser_lock.matches("\"node_modules/").count(), 3);
    assert!(browser_lock.contains("\"node_modules/@playwright/test\""));
    assert!(!browser_lock.contains("tailwind"));
    assert!(!browser_lock.contains("daisyui"));

    let native_gate =
        std::fs::read_to_string(root.join("scripts/frontend.sh")).expect("native gate");
    assert!(native_gate.contains("exec bash \"$ROOT/scripts/rune.sh\""));
    let rune_gate = std::fs::read_to_string(root.join("scripts/rune.sh")).expect("Rune host gate");
    assert!(rune_gate.contains("cargo fmt --manifest-path apps/rune/Cargo.toml --all --check"));

    let browser_gate = std::fs::read_to_string(root.join("scripts/web.sh")).expect("browser gate");
    assert!(browser_gate.contains("scripts/rune-browser.sh"));
    assert!(browser_gate.contains("scripts/rune-web-screenshots.sh"));
    assert!(browser_gate.contains("partial evidence only"));

    let rune_workflow =
        std::fs::read_to_string(root.join(".github/workflows/rune.yml")).expect("Rune workflow");
    assert!(rune_workflow.contains("run: bash scripts/frontend.sh"));
    assert!(rune_workflow.contains("run: bash scripts/web.sh --ci"));
    assert!(rune_workflow.contains("scripts/rune-ios-simulator.sh"));
    assert!(!root.join(".github/workflows/frontend.yml").exists());
    assert!(!root.join(".github/workflows/web.yml").exists());
}
