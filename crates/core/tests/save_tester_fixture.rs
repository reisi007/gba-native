//! Committed synthetic save-tester fixture.
//!
//! Offline and dependency-free. The guest writes a magic and counter to SRAM;
//! the first run must show the fresh frame and the second run, after loading the
//! persisted backup image, must show the loaded frame with the counter advanced.

use std::path::PathBuf;

use gba_native_core::bus::{BackupPolicy, Bus};
use gba_native_core::cpu::Cpu;
use gba_native_core::demo;
use gba_native_core::display;
use gba_native_core::interpret;
use gba_native_core::save::SaveLibrary;

const ROM_SHA256: &str = "f8dbe41f4dcd1e134759a23cce988b4a05f1185f102b3153d69240600e76c4e3";
const FRESH_IMAGE_SHA256: &str = "53ff58d4cb667ee14a36ec6d6723fbfa623a7ddc489c4a11551254cdeaad8755";
const LOADED_IMAGE_SHA256: &str =
    "259d7e7302a5d04611c276395f3f9a5901e7c65061b4fb0e6b9b6f786905434c";
const DIGEST: &str = "f8dbe41f4dcd1e134759a23cce988b4a05f1185f102b3153d69240600e76c4e3";

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/roms/synthetic/save-tester.gba")
}

fn run(cart: gba_native_core::cart::Cartridge) -> (Bus, gba_native_core::display::Image) {
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(0x0800_0000);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart).with_backup_policy(BackupPolicy::Sram32KVolatile);
    let used =
        interpret::run_budget(&mut cpu, &mut bus, demo::SAVE_TESTER_BUDGET).expect("reference run");
    assert_eq!(used, demo::SAVE_TESTER_BUDGET);
    let image = display::snapshot(&bus).expect("snapshot");
    (bus, image)
}

#[test]
fn committed_rom_matches_the_generator_and_digest() {
    let committed = std::fs::read(fixture_path()).expect("committed save ROM");
    assert_eq!(committed, demo::save_tester_cartridge().bytes());
    assert_eq!(demo::save_tester_cartridge().sha256_hex(), ROM_SHA256);
    let (_, image) = run(demo::save_tester_cartridge());
    assert_eq!(image.sha256_hex(), FRESH_IMAGE_SHA256);
    assert_eq!(image, demo::expected_save_tester_image(false));
}

#[test]
fn persisted_save_changes_the_next_run_and_keeps_multiple_slots() {
    let root = std::env::temp_dir().join(format!(
        "gba-native-save-fixture-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let library = SaveLibrary::new(&root);
    let (bus, image) = run(demo::save_tester_cartridge());
    assert_eq!(image.sha256_hex(), FRESH_IMAGE_SHA256);
    let saved = bus.backup_image().to_vec();
    library.store(DIGEST, "first", &saved).expect("store first");
    library
        .store(DIGEST, "second", &saved)
        .expect("store second");
    assert_eq!(library.list(DIGEST).unwrap(), ["first", "second"]);
    assert_eq!(
        library.default_slot(DIGEST).unwrap().as_deref(),
        Some("first")
    );
    library.set_default(DIGEST, "second").expect("use second");
    assert_eq!(
        library.default_slot(DIGEST).unwrap().as_deref(),
        Some("second")
    );

    let mut reloaded =
        Bus::new(demo::save_tester_cartridge()).with_backup_policy(BackupPolicy::Sram32KVolatile);
    reloaded
        .load_backup(&library.load(DIGEST, "second").expect("load second"))
        .expect("load backup");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(0x0800_0000);
    cpu.set_thumb(false);
    let used = interpret::run_budget(&mut cpu, &mut reloaded, demo::SAVE_TESTER_BUDGET)
        .expect("second run");
    assert_eq!(used, demo::SAVE_TESTER_BUDGET);
    let image = display::snapshot(&reloaded).expect("snapshot");
    assert_eq!(image.sha256_hex(), LOADED_IMAGE_SHA256);
    assert_eq!(image, demo::expected_save_tester_image(true));
    std::fs::remove_dir_all(root).unwrap();
}
