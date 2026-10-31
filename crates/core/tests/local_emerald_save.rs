//! Local-only retail save persistence proof.
//!
//! Ignored and environment-gated. It reads a user-supplied Emerald ROM
//! read-only, drives the observed intro/name/menu flow to an in-game save,
//! persists the flash image through `SaveLibrary`, and verifies the image can be
//! loaded back into a fresh bus. Evidence is written only under `output/`.
//! No retail ROM, BIOS or save bytes are committed or redistributed.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gba_native_core::bus::{BackupPolicy, BiosWritePolicy, Bus, RomWritePolicy};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::input::{InputEvent, InputScript, run_scripted};
use gba_native_core::save::SaveLibrary;

const BUDGET: u64 = 7_600_000_000;

fn script() -> InputScript {
    let mut events = Vec::new();
    let mut push = |at: u64, keys: u16, hold: u64| {
        events.push(InputEvent { at, keys, hold });
    };
    push(460_000_000, 1 << 3, 20_000_000); // START at the title
    for at in (700_000_000..=1_700_000_000).step_by(50_000_000) {
        push(at, 1 << 0, 12_000_000); // A through the intro
    }
    // Name keyboard: first, second and third characters as observed.
    let naming: &[(u64, u16)] = &[
        (1_750_000_000, 1 << 0),
        (1_800_000_000, 1 << 7),
        (1_850_000_000, 1 << 7),
        (1_900_000_000, 1 << 4),
        (1_950_000_000, 1 << 4),
        (2_000_000_000, 1 << 0),
        (2_050_000_000, 1 << 6),
        (2_100_000_000, 1 << 6),
        (2_150_000_000, 1 << 4),
        (2_200_000_000, 1 << 4),
        (2_250_000_000, 1 << 4),
        (2_300_000_000, 1 << 4),
        (2_350_000_000, 1 << 4),
        (2_400_000_000, 1 << 0),
        (2_450_000_000, 1 << 3),
        (2_500_000_000, 1 << 0),
    ];
    for &(at, keys) in naming {
        push(at, keys, 5_000_000);
    }
    // Dismiss the remaining intro dialogue, walk out of the truck, then open
    // START -> SAVE. The observed menu needs the bag closed (B) before the two
    // navigation presses reach SICHERN.
    for at in (2_550_000_000..=4_470_000_000).step_by(30_000_000) {
        push(at, 1 << 0, 5_000_000);
    }
    push(4_500_000_000, 1 << 7, 200_000_000); // walk down out of the truck
    push(4_750_000_000, 1 << 3, 5_000_000); // START menu
    for index in 0..5u64 {
        push(4_950_000_000 + index * 50_000_000, 1 << 7, 10_000_000);
    }
    for at in [5_300_000_000, 5_400_000_000, 5_500_000_000, 5_600_000_000] {
        push(at, 1 << 0, 5_000_000);
    }
    push(5_850_000_000, 1 << 7, 10_000_000);
    push(5_900_000_000, 1 << 7, 10_000_000);
    for at in [6_000_000_000, 6_100_000_000, 6_200_000_000, 6_300_000_000] {
        push(at, 1 << 0, 5_000_000);
    }
    push(6_600_000_000, 1 << 1, 5_000_000); // B: close the bag
    push(6_700_000_000, 1 << 7, 5_000_000);
    push(6_800_000_000, 1 << 7, 5_000_000);
    for at in [7_000_000_000, 7_100_000_000, 7_200_000_000] {
        push(at, 1 << 0, 5_000_000);
    }
    InputScript::from_events(events)
}

fn evidence_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("output")
        .join(format!(
            "local-emerald-save-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
}

#[test]
#[ignore = "local-only: set GBA_NATIVE_LOCAL_ROM to a read-only retail ROM"]
fn emerald_save_round_trips_through_the_host_library() {
    let Some(rom) = std::env::var_os("GBA_NATIVE_LOCAL_ROM") else {
        eprintln!(
            "skipped: set GBA_NATIVE_LOCAL_ROM to a read-only ROM path (the ROM is never copied)"
        );
        return;
    };
    let path = PathBuf::from(rom);
    let cart = Cartridge::from_path(&path).expect("read-only ROM");
    let digest = cart.sha256_hex();
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    let mut bus = Bus::with_synthetic_bios_zero_fill(cart.clone())
        .with_bios_write_policy(BiosWritePolicy::RomWritesDropped)
        .with_rom_write_policy(RomWritePolicy::Dropped)
        .with_backup_policy(BackupPolicy::Flash128KVolatile)
        .with_decompression_policy(gba_native_core::bios::DecompressionPolicy::DocumentedFormats)
        .with_affine_policy(gba_native_core::bios::AffinePolicy::Documented);
    let used = run_scripted(&mut cpu, &mut bus, BUDGET, &script()).expect("scripted run");
    assert_eq!(used, BUDGET);

    let image = bus.backup_image().to_vec();
    let non_erased = image.iter().filter(|byte| **byte != 0xFF).count();
    let save_sha = gba_native_core::sha256::hex(&gba_native_core::sha256::digest(&image));
    let out = evidence_dir();
    std::fs::create_dir_all(&out).expect("evidence dir");

    assert!(
        non_erased > 4096,
        "Emerald did not write a meaningful flash image (only {non_erased} non-0xFF bytes)"
    );

    let library = SaveLibrary::new(out.join("saves"));
    library
        .store(&digest, "emerald-proof", &image)
        .expect("persist image");
    let stored = library
        .load(&digest, "emerald-proof")
        .expect("reload image");
    assert_eq!(stored, image, "reloaded image differs");

    let mut reloaded = Bus::with_synthetic_bios_zero_fill(cart)
        .with_bios_write_policy(BiosWritePolicy::RomWritesDropped)
        .with_rom_write_policy(RomWritePolicy::Dropped)
        .with_backup_policy(BackupPolicy::Flash128KVolatile);
    reloaded.load_backup(&stored).expect("load backup");
    assert_eq!(reloaded.backup_image(), image);

    std::fs::write(
        out.join("report.txt"),
        format!(
            "capture_kind=local_emerald_save\n\
             rom_sha256={digest}\n\
             rom_bytes={}\n\
             budget={BUDGET}\n\
             final_pc=0x{:08X}\n\
             save_bytes={}\n\
             save_non_erased_bytes={non_erased}\n\
             save_sha256={save_sha}\n\
             reloaded=ok\n\
             note=local-only read-only retail observation; not a compatibility or distribution claim\n",
            std::fs::metadata(&path).expect("ROM metadata").len(),
            cpu.pc(),
            image.len()
        ),
    )
    .expect("report");
    println!(
        "emerald save proof: non_erased={non_erased} sha256={save_sha} evidence={}",
        out.display()
    );
}
