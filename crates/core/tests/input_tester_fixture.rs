//! Committed synthetic input-tester fixture.
//!
//! This test is offline and dependency-free. It regenerates the fixture from
//! source, checks the committed digest and expected frame, then runs the
//! reference interpreter with the committed input script. A retail ROM is not
//! involved.

use std::path::PathBuf;

use gba_native_core::bus::Bus;
use gba_native_core::cpu::Cpu;
use gba_native_core::demo;
use gba_native_core::display;
use gba_native_core::input::{InputScript, run_scripted};

const ROM_SHA256: &str = "8a7667d43d9ec0333a5a7c0a032837919e101ca1c1893bdf5f95f51b27555539";
const EXPECTED_FILE_SHA256: &str =
    "2824c8e9f509c1294c555af3318cec4d3fd4a37aa964f60e9ecc974721da941a";
const EXPECTED_IMAGE_SHA256: &str =
    "5cbbd998a2bc5a026ebac07d91a0f058b769cb05dd14c09e30d52888ed80db8b";
const BUDGET: u64 = 2_000_000;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/roms/synthetic")
}

#[test]
fn committed_rom_matches_the_generator_and_digest() {
    let committed = std::fs::read(fixture_dir().join("input-tester.gba")).expect("committed ROM");
    let generated = demo::input_tester_cartridge();
    assert_eq!(committed, generated.bytes(), "regenerate the fixture");
    assert_eq!(generated.sha256_hex(), ROM_SHA256);
    assert_eq!(
        gba_native_core::sha256::hex(&gba_native_core::sha256::digest(&committed)),
        ROM_SHA256
    );
}

#[test]
fn committed_expected_frame_matches_the_independent_image_function() {
    let ppm = std::fs::read(fixture_dir().join("input-tester.expected.ppm")).expect("expected PPM");
    assert_eq!(
        gba_native_core::sha256::hex(&gba_native_core::sha256::digest(&ppm)),
        EXPECTED_FILE_SHA256
    );
    let expected = demo::expected_input_tester_image(demo::INPUT_TESTER_SAMPLE_MASK);
    assert_eq!(ppm, gba_native_core::render::to_ppm(&expected));
    assert_eq!(expected.sha256_hex(), EXPECTED_IMAGE_SHA256);
}

#[test]
fn scripted_reference_run_matches_the_expected_frame() {
    let script_text =
        std::fs::read_to_string(fixture_dir().join("input-tester.input")).expect("input script");
    let script = InputScript::parse(&script_text).expect("valid input script");
    let cart = demo::input_tester_cartridge();
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(demo::INPUT_TESTER_ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart);
    let used = run_scripted(&mut cpu, &mut bus, BUDGET, &script).expect("reference run");
    assert_eq!(used, BUDGET);
    let image = display::snapshot(&bus).expect("snapshot");
    assert_eq!(
        image,
        demo::expected_input_tester_image(demo::INPUT_TESTER_SAMPLE_MASK)
    );
    assert_eq!(image.sha256_hex(), EXPECTED_IMAGE_SHA256);
}
