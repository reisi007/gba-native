//! Synthetic save-tester ROM.
//!
//! The guest writes a small magic and a counter to byte-wide SRAM at
//! `0x0E000000`, then paints the whole Mode 3 frame. A fresh save shows the
//! green-ish colour and a loaded save shows the blue colour after incrementing
//! the counter. The host can persist and reload the backup image between runs.
//!
//! This is our own fixture. It contains no retail save data and is not a
//! save-device compatibility claim.

use super::{
    Cartridge, Image, arm_add_imm, arm_branch, arm_cmp_imm, arm_ldr_literal, arm_ldrb, arm_mov_imm,
    arm_mov_imm_rot, arm_mov_reg, arm_strb, arm_strh, arm_strh_post, arm_subs_imm, finish,
    rgb555_to_rgb888, write_arm, write_word,
};

/// Fixture image length.
pub const SAVE_TESTER_LEN: usize = 0x1000;
/// Title stored in the synthetic header.
pub const SAVE_TESTER_TITLE: &str = "SAVETEST";
/// Instruction budget used by the tests and the native gate.
pub const SAVE_TESTER_BUDGET: u64 = 5_000_000;
/// Byte offset of the signature inside the save window.
pub const SAVE_TESTER_MAGIC: [u8; 4] = *b"GSAV";
/// Counter byte offset.
pub const SAVE_TESTER_COUNTER: usize = 4;
/// Fresh-save frame colour (RGB555).
pub const SAVE_TESTER_FRESH_COLOR: u16 = 0x00E0;
/// Loaded-save frame colour (RGB555).
pub const SAVE_TESTER_LOADED_COLOR: u16 = 0x001F;

const SAVE_BASE: u32 = 0x0E00_0000;
const VRAM: u32 = 0x0600_0000;
const WIDTH: u32 = 240;
const HEIGHT: u32 = 160;

pub fn save_tester_cartridge() -> Cartridge {
    let mut rom = vec![0u8; SAVE_TESTER_LEN];
    write_arm(&mut rom, 0x000, arm_branch(0xE, 0x000, 0x110));

    // Mode 3 + BG2.
    write_arm(&mut rom, 0x110, arm_mov_imm_rot(3, 0x04, 8));
    write_arm(&mut rom, 0x114, arm_mov_imm(5, 3));
    write_arm(&mut rom, 0x118, 0xE385_5B01); // orr r5, r5, #0x400
    write_arm(&mut rom, 0x11C, arm_strh(5, 3, 0));

    // r4 = save window.
    write_arm(&mut rom, 0x120, arm_ldr_literal(4, 0x120, 0x198));
    write_arm(&mut rom, 0x124, arm_ldrb(5, 4, 0));
    write_arm(
        &mut rom,
        0x128,
        arm_cmp_imm(5, u32::from(SAVE_TESTER_MAGIC[0])),
    );
    write_arm(&mut rom, 0x12C, arm_branch(0x1, 0x12C, 0x144));

    // Existing save: increment the counter and paint blue.
    write_arm(&mut rom, 0x130, arm_ldrb(5, 4, SAVE_TESTER_COUNTER as u32));
    write_arm(&mut rom, 0x134, arm_add_imm(5, 5, 1));
    write_arm(&mut rom, 0x138, arm_strb(5, 4, SAVE_TESTER_COUNTER as u32));
    write_arm(
        &mut rom,
        0x13C,
        arm_mov_imm(1, u32::from(SAVE_TESTER_LOADED_COLOR as u8)),
    );
    write_arm(&mut rom, 0x140, arm_branch(0xE, 0x140, 0x170));

    // Fresh save: write the magic and counter, paint green-ish.
    for (index, byte) in SAVE_TESTER_MAGIC.iter().enumerate() {
        let offset = 0x144 + index as u32 * 8;
        write_arm(&mut rom, offset as usize, arm_mov_imm(5, u32::from(*byte)));
        write_arm(&mut rom, offset as usize + 4, arm_strb(5, 4, index as u32));
    }
    write_arm(&mut rom, 0x164, arm_mov_imm(5, 1));
    write_arm(&mut rom, 0x168, arm_strb(5, 4, SAVE_TESTER_COUNTER as u32));
    write_arm(
        &mut rom,
        0x16C,
        arm_mov_imm(1, u32::from(SAVE_TESTER_FRESH_COLOR as u8)),
    );

    // Fill the Mode 3 frame.
    write_arm(&mut rom, 0x170, arm_ldr_literal(3, 0x170, 0x19C));
    write_arm(&mut rom, 0x174, arm_mov_imm(6, WIDTH));
    write_arm(&mut rom, 0x178, arm_mov_imm(7, HEIGHT));
    write_arm(&mut rom, 0x17C, arm_mov_reg(0, 6));
    write_arm(&mut rom, 0x180, arm_strh_post(1, 3, 2));
    write_arm(&mut rom, 0x184, arm_subs_imm(0, 0, 1));
    write_arm(&mut rom, 0x188, arm_branch(0x1, 0x188, 0x180));
    write_arm(&mut rom, 0x18C, arm_subs_imm(7, 7, 1));
    write_arm(&mut rom, 0x190, arm_branch(0x1, 0x190, 0x17C));
    write_arm(&mut rom, 0x194, arm_branch(0xE, 0x194, 0x194));

    write_word(&mut rom, 0x198, SAVE_BASE);
    write_word(&mut rom, 0x19C, VRAM);
    finish(rom, SAVE_TESTER_TITLE.as_bytes(), b"GBAS", b"00")
}

/// Expected frame for a fresh or loaded save.
#[must_use]
pub fn expected_save_tester_image(loaded: bool) -> Image {
    let color = if loaded {
        SAVE_TESTER_LOADED_COLOR
    } else {
        SAVE_TESTER_FRESH_COLOR
    };
    let pixel = rgb555_to_rgb888(color);
    Image {
        width: WIDTH,
        height: HEIGHT,
        pixels: vec![pixel; (WIDTH * HEIGHT) as usize],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{BackupPolicy, Bus};
    use crate::cpu::Cpu;
    use crate::demo::ROM_BASE;
    use crate::display;
    use crate::interpret;
    use crate::save::SaveLibrary;

    fn run(bus: &mut Bus) -> Image {
        let mut cpu = Cpu::direct_entry();
        cpu.set_pc(ROM_BASE);
        cpu.set_thumb(false);
        let used = interpret::run_budget(&mut cpu, bus, SAVE_TESTER_BUDGET).expect("run");
        assert_eq!(used, SAVE_TESTER_BUDGET);
        display::snapshot(bus).expect("snapshot")
    }

    #[test]
    fn fresh_run_writes_the_magic_and_counter() {
        let mut bus =
            Bus::new(save_tester_cartridge()).with_backup_policy(BackupPolicy::Sram32KVolatile);
        let image = run(&mut bus);
        assert_eq!(image, expected_save_tester_image(false));
        assert_eq!(&bus.backup_image()[..4], &SAVE_TESTER_MAGIC);
        assert_eq!(bus.backup_image()[SAVE_TESTER_COUNTER], 1);
    }

    #[test]
    fn a_loaded_save_changes_the_frame_and_increments_the_counter() {
        let mut first =
            Bus::new(save_tester_cartridge()).with_backup_policy(BackupPolicy::Sram32KVolatile);
        let _ = run(&mut first);
        let image = first.backup_image().to_vec();

        let mut second =
            Bus::new(save_tester_cartridge()).with_backup_policy(BackupPolicy::Sram32KVolatile);
        second.load_backup(&image).expect("load backup");
        let frame = run(&mut second);
        assert_eq!(frame, expected_save_tester_image(true));
        assert_eq!(second.backup_image()[SAVE_TESTER_COUNTER], 2);
    }

    #[test]
    fn the_save_library_round_trips_the_backup_image() {
        let root = std::env::temp_dir().join(format!(
            "gba-native-save-tester-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let library = SaveLibrary::new(&root);
        let cart = save_tester_cartridge();
        let mut bus = Bus::new(cart.clone()).with_backup_policy(BackupPolicy::Sram32KVolatile);
        let _ = run(&mut bus);
        library
            .store(&cart.sha256_hex(), "default", bus.backup_image())
            .expect("persist");
        let stored = library.load(&cart.sha256_hex(), "default").expect("load");
        assert_eq!(stored, bus.backup_image());

        let mut reloaded = Bus::new(cart.clone()).with_backup_policy(BackupPolicy::Sram32KVolatile);
        reloaded.load_backup(&stored).expect("load backup");
        let _ = run(&mut reloaded);
        assert_eq!(reloaded.backup_image()[SAVE_TESTER_COUNTER], 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_images_are_rejected() {
        let mut bus = Bus::new(save_tester_cartridge());
        assert!(bus.load_backup(&[]).is_err());
        assert!(bus.load_backup(&vec![0; 128 * 1024 + 1]).is_err());
    }
}
