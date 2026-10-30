use gba_native::demo;

use super::target_build::ToolchainFacts;
use super::wasm_build::{WASM_TARGET, validate_build_target};
use super::wasm_meta::{wasm_artifact_metadata, wasm_build_metadata, wasm_readme};
use super::wasm_tool::{WASM_BINDGEN_VERSION, resolve_wasm_bindgen, verify_wasm_bindgen_version};
use super::{Limits, discover, emit_wasm_module};

fn fixture() -> (gba_native::cart::Cartridge, super::AotArtifact, String) {
    let cart = demo::demo_cartridge();
    let artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    let source = emit_wasm_module(&cart, &artifact, 424_242).expect("emit wasm module");
    (cart, artifact, source)
}

#[test]
fn wasm_module_embeds_binding_constants_without_rom_bytes() {
    let (_cart, artifact, source) = fixture();
    assert!(source.contains("#[wasm_bindgen]"));
    assert!(source.contains("pub fn run_rom(bytes: Vec<u8>)"));
    assert!(source.contains(&format!(
        "const ROM_SHA256: &str = \"{}\";",
        artifact.rom_sha256_hex()
    )));
    assert!(source.contains(&format!("const ROM_LEN: usize = {};", artifact.rom_len)));
    assert!(source.contains("const BUDGET: u64 = 424242;"));
    for marker in ["ROM_BYTES", "include_bytes!", "GBA-NATIVE", "@ROM_SHA256@"] {
        assert!(
            !source.contains(marker),
            "WASM module must not contain {marker:?}"
        );
    }
    // The target-neutral program section stays unchanged and usable.
    assert!(source.contains(
        "pub fn run_program(cpu: &mut Cpu, bus: &mut Bus, budget: u64, input: &gba_native::input::InputScript, audio: &mut Option<gba_native::audio::AudioEngine>)"
    ));
    assert!(source.contains("interpret::step(cpu, bus)?;"));
}

#[test]
fn wasm_driver_rejects_before_constructing_cartridge_cpu_or_bus() {
    let (_cart, _artifact, source) = fixture();
    let length_check = source.find("bytes.len() != ROM_LEN").expect("length check");
    let digest_check = source.find("actual != ROM_SHA256").expect("digest check");
    let verify_call = source
        .find("verify_binding(&bytes)?;")
        .expect("binding call in execute");
    let cartridge = source
        .find("Cartridge::from_bytes")
        .expect("cartridge construction");
    let bus = source.find("Bus::new(cart)").expect("bus construction");
    let cpu = source
        .find("Cpu::direct_entry()")
        .expect("cpu construction");
    assert!(
        length_check < digest_check && verify_call < cartridge && cartridge < bus && bus < cpu,
        "binding checks must precede all guest state construction"
    );
}

#[test]
fn wasm_module_has_no_host_process_or_output_boundary() {
    let (_cart, _artifact, source) = fixture();
    for marker in [
        "fn main(",
        "std::fs",
        "std::process",
        "std::env",
        "std::path",
        "println!",
        "eprintln!",
        "--rom",
        "Command::new",
    ] {
        assert!(
            !source.contains(marker),
            "WASM module must not contain host boundary {marker:?}"
        );
    }
    // The only exported entry point takes caller-supplied bytes and reports
    // through a return value or a JS error.
    assert!(source.contains("-> std::result::Result<String, JsValue>"));
    assert!(source.contains("JsValue::from_str"));
}

#[test]
fn wasm_artifact_metadata_is_ordered_and_hash_based() {
    let (cart, artifact, source) = fixture();
    let module = b"\0asm synthetic module fixture".to_vec();
    let glue = b"export function run_rom() {}".to_vec();
    let text = wasm_artifact_metadata(
        &artifact,
        424_242,
        &source,
        &module,
        &glue,
        WASM_BINDGEN_VERSION,
    );
    let expected_keys = [
        "schema",
        "artifact_kind",
        "target",
        "rom_sha256",
        "rom_bytes",
        "entry",
        "entry_state",
        "budget",
        "blocks",
        "translated_instructions",
        "resolved_bx",
        "dynamic_bx",
        "bl_calls",
        "bl_returns",
        "dynamic_bl",
        "pc_writes",
        "swis",
        "decode_stops",
        "external_targets",
        "block_limit_splits",
        "limit_reached",
        "program_source_sha256",
        "js_glue_sha256",
        "module_sha256",
        "wasm_bindgen_version",
        "embedded_rom_bytes",
        "requires_external_rom",
    ];
    let keys = text
        .lines()
        .map(|line| line.split_once('=').expect("key=value").0)
        .collect::<Vec<_>>();
    assert_eq!(keys, expected_keys, "WASM metadata key order changed");

    let fields = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("missing {key}"))
    };
    assert_eq!(fields("artifact_kind"), "wasm-module");
    assert_eq!(fields("target"), WASM_TARGET);
    assert_eq!(fields("rom_sha256"), cart.sha256_hex());
    assert_eq!(fields("rom_bytes"), cart.len().to_string());
    assert_eq!(fields("entry"), "0x08000000");
    assert_eq!(fields("entry_state"), "arm");
    assert_eq!(fields("budget"), "424242");
    assert_eq!(fields("blocks"), "4");
    assert_eq!(fields("translated_instructions"), "17");
    assert_eq!(fields("wasm_bindgen_version"), WASM_BINDGEN_VERSION);
    assert_eq!(fields("embedded_rom_bytes"), "0");
    assert_eq!(fields("requires_external_rom"), "true");
    assert_eq!(
        fields("program_source_sha256"),
        gba_native::sha256::hex(&gba_native::sha256::digest(source.as_bytes()))
    );
    assert_eq!(
        fields("js_glue_sha256"),
        gba_native::sha256::hex(&gba_native::sha256::digest(&glue))
    );
    assert_eq!(
        fields("module_sha256"),
        gba_native::sha256::hex(&gba_native::sha256::digest(&module))
    );
    // No executable claim and no absolute path or timestamp in metadata.
    assert!(!text.contains("executable_sha256"));
    assert!(!text.contains('/'));
    assert!(!text.contains(&std::env::temp_dir().to_string_lossy().into_owned()));
}

#[test]
fn wasm_build_metadata_reuses_toolchain_facts_and_pins_bindgen() {
    let facts = ToolchainFacts {
        cargo: "1.99.1".to_string(),
        rustc: "1.99.1".to_string(),
        host: "aarch64-apple-darwin".to_string(),
    };
    let text = wasm_build_metadata(&facts, WASM_BINDGEN_VERSION);
    let expected_keys = [
        "schema",
        "target",
        "profile",
        "build_offline",
        "cargo_version",
        "rustc_version",
        "rustc_host",
        "wasm_bindgen_version",
    ];
    let keys = text
        .lines()
        .map(|line| line.split_once('=').expect("key=value").0)
        .collect::<Vec<_>>();
    assert_eq!(keys, expected_keys, "WASM build metadata key order changed");
    assert!(text.contains(&format!("target={WASM_TARGET}")));
    assert!(text.contains("profile=release"));
    assert!(text.contains("build_offline=true"));
    assert!(text.contains("cargo_version=1.99.1"));
    assert!(text.contains("rustc_version=1.99.1"));
    assert!(text.contains("rustc_host=aarch64-apple-darwin"));
    assert!(text.contains(&format!("wasm_bindgen_version={WASM_BINDGEN_VERSION}")));
}

#[test]
fn wasm_readme_documents_the_callable_module_and_external_binding() {
    let (cart, artifact, _source) = fixture();
    let readme = wasm_readme(&artifact, 424_242);
    let hash = cart.sha256_hex();
    for marker in [
        "run_rom(bytes)",
        "Uint8Array",
        "input.files[0].arrayBuffer()",
        "browser-local `File`",
        "leaves the page",
        "SHA-256",
        hash.as_str(),
        "424242",
        "caller supplies the image at call time",
        "no browser UI",
        "not an\nHTML/JS application bundle",
    ] {
        assert!(readme.contains(marker), "README is missing {marker:?}");
    }
    assert!(
        !readme.contains("fetch("),
        "README must not imply fetching or hosting a ROM"
    );
    assert!(!readme.contains("/Users/"));
}

#[test]
fn only_the_supported_wasm_target_is_accepted() {
    validate_build_target("wasm32-unknown-unknown").expect("supported wasm target");
    validate_build_target("aarch64-apple-darwin").expect("native targets keep their path");
    validate_build_target("x86_64-pc-windows-msvc").expect("native targets keep their path");
    for rejected in [
        "wasm32-wasip1",
        "wasm32-unknown-emscripten",
        "wasm64-unknown",
    ] {
        let error = validate_build_target(rejected).expect_err("unsupported wasm target");
        let message = format!("{error}");
        assert!(message.contains(rejected), "{message}");
        assert!(message.contains(WASM_TARGET), "{message}");
    }
}

#[test]
fn wasm_bindgen_version_must_match_the_pin_exactly() {
    verify_wasm_bindgen_version(&format!("wasm-bindgen {WASM_BINDGEN_VERSION}"))
        .expect("pinned banner");
    for rejected in [
        "wasm-bindgen 0.2.128".to_string(),
        "wasm-bindgen 0.2.1290".to_string(),
        format!("wasm-bindgen {WASM_BINDGEN_VERSION} extra"),
        String::new(),
    ] {
        let error = verify_wasm_bindgen_version(&rejected).expect_err("mismatched banner");
        assert!(format!("{error}").contains("bootstrap-web-tools.sh"));
    }
}

#[test]
fn missing_pinned_wasm_bindgen_fails_with_a_bootstrap_instruction() {
    let root = std::env::temp_dir().join(format!(
        "gba-native-missing-wasm-tool-{}",
        std::process::id()
    ));
    assert!(!root.exists(), "test root must not exist");
    let error = resolve_wasm_bindgen(&root).expect_err("missing tool must fail");
    let message = format!("{error}");
    assert!(message.contains("bootstrap-web-tools.sh"), "{message}");
    assert!(message.contains(WASM_BINDGEN_VERSION), "{message}");
    assert!(
        message.contains("target") && message.contains("wasm-bindgen"),
        "{message}"
    );
}
