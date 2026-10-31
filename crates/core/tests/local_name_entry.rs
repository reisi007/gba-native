//! Local-only retail name-entry regression.
//!
//! This test is deliberately `#[ignore]`d and environment-gated: it needs a
//! user-supplied, read-only retail ROM that must never be committed, copied or
//! redistributed. It is evidence for this prototype only, not a compatibility,
//! playability or distribution claim.
//!
//! Run it manually on a host that has the ROM:
//!
//! ```text
//! GBA_NATIVE_LOCAL_ROM=/path/to/read-only.gba \
//!   cargo test -p gba-native-core --test local_name_entry -- --ignored --nocapture
//! ```
//!
//! The scripted sequence was derived from the observed title/new-game flow:
//! START at 460,000,000 instructions, periodic A presses through the intro,
//! then directional navigation on the name keyboard. The final frame shows the
//! confirmation dialog with the entered name. The fixed digest is a local
//! golden tied to this renderer build and is not a hardware or retail claim.

use std::path::PathBuf;

use gba_native_core::bus::Bus;
use gba_native_core::cpu::Cpu;
use gba_native_core::display;
use gba_native_core::input::{InputEvent, InputScript, run_scripted};
use gba_native_core::render;

const BUDGET: u64 = 2_525_000_000;
const EXPECTED_PC: u32 = 0x0800_08C8;
const EXPECTED_DISPCNT: u16 = 0x1340;
/// Local golden for the confirmation dialog in this renderer build.
const EXPECTED_IMAGE_SHA256: &str =
    "558ae1e3c9fb682f2031525cc5783c2445e3ca47ae0b39bef6f5b3255c1785f8";

fn script() -> InputScript {
    let mut events = Vec::new();
    events.push(InputEvent {
        at: 460_000_000,
        keys: 1 << 3, // START
        hold: 20_000_000,
    });
    for at in (700_000_000..=1_700_000_000).step_by(50_000_000) {
        events.push(InputEvent {
            at,
            keys: 1 << 0, // A
            hold: 12_000_000,
        });
    }
    let steps: &[(u64, u16)] = &[
        (1_750_000_000, 1 << 0), // A: first character
        (1_800_000_000, 1 << 7), // DOWN
        (1_850_000_000, 1 << 7), // DOWN
        (1_900_000_000, 1 << 4), // RIGHT
        (1_950_000_000, 1 << 4), // RIGHT
        (2_000_000_000, 1 << 0), // A: second character
        (2_050_000_000, 1 << 6), // UP
        (2_100_000_000, 1 << 6), // UP
        (2_150_000_000, 1 << 4), // RIGHT
        (2_200_000_000, 1 << 4), // RIGHT
        (2_250_000_000, 1 << 4), // RIGHT
        (2_300_000_000, 1 << 4), // RIGHT
        (2_350_000_000, 1 << 4), // RIGHT
        (2_400_000_000, 1 << 0), // A: third character
        (2_450_000_000, 1 << 3), // START: confirm
        (2_500_000_000, 1 << 0), // A: dismiss/confirm dialog
    ];
    for &(at, keys) in steps {
        events.push(InputEvent {
            at,
            keys,
            hold: 5_000_000,
        });
    }
    InputScript::from_events(events)
}

fn output_dir() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.join("output").join(format!(
        "local-name-entry-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0)
    ))
}

#[test]
#[ignore = "local-only: set GBA_NATIVE_LOCAL_ROM to a read-only retail ROM"]
fn enters_and_confirms_a_player_name() {
    let Some(rom) = std::env::var_os("GBA_NATIVE_LOCAL_ROM") else {
        eprintln!(
            "skipped: set GBA_NATIVE_LOCAL_ROM to a read-only ROM path (the ROM is never copied)"
        );
        return;
    };
    let path = PathBuf::from(rom);
    let cart = gba_native_core::cart::Cartridge::from_path(&path).expect("read-only ROM");
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    let mut bus = Bus::with_synthetic_bios_zero_fill(cart)
        .with_bios_write_policy(gba_native_core::bus::BiosWritePolicy::RomWritesDropped)
        .with_rom_write_policy(gba_native_core::bus::RomWritePolicy::Dropped)
        .with_backup_policy(gba_native_core::bus::BackupPolicy::Flash128KVolatile)
        .with_decompression_policy(gba_native_core::bios::DecompressionPolicy::DocumentedFormats)
        .with_affine_policy(gba_native_core::bios::AffinePolicy::Documented);
    let used = run_scripted(&mut cpu, &mut bus, BUDGET, &script()).expect("scripted run");
    assert_eq!(used, BUDGET);
    assert_eq!(cpu.pc(), EXPECTED_PC, "unexpected final PC");
    assert_eq!(
        bus.read16(gba_native_core::bus::DISPCNT_ADDR)
            .expect("DISPCNT"),
        EXPECTED_DISPCNT
    );

    let image = display::snapshot(&bus).expect("final frame");
    let out = output_dir();
    std::fs::create_dir_all(&out).expect("output dir");
    std::fs::write(out.join("screen.ppm"), render::to_ppm(&image)).expect("snapshot");
    std::fs::write(
        out.join("report.txt"),
        format!(
            "capture_kind=local_retail_name_entry\n\
             rom_sha256={}\n\
             rom_bytes={}\n\
             budget={BUDGET}\n\
             final_pc=0x{:08X}\n\
             final_dispcnt=0x{EXPECTED_DISPCNT:04X}\n\
             image_sha256={}\n\
             note=local-only read-only retail observation; not a compatibility, playability or distribution claim\n",
            bus.cart().sha256_hex(),
            bus.cart().len(),
            cpu.pc(),
            image.sha256_hex()
        ),
    )
    .expect("report");

    assert_eq!(
        image.sha256_hex(),
        EXPECTED_IMAGE_SHA256,
        "local name-entry frame changed; inspect {}",
        out.join("screen.ppm").display()
    );
    eprintln!(
        "local name entry confirmed: pc=0x{:08X} image={} evidence={}",
        cpu.pc(),
        image.sha256_hex(),
        out.display()
    );
}
