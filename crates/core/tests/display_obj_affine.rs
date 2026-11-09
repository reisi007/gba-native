//! Independent pixel oracles for double-size affine OBJ snapshots.

use gba_native_core::bus::{Bus, DISPCNT_ADDR, OAM_BASE, PALETTE_BASE, VRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::display;

const OBJ: u16 = 0x1040;

fn setup(attr0: u16, matrix: [i16; 4], mosaic: u16) -> Bus {
    let mut bus = Bus::new(Cartridge::from_bytes(vec![0; 0x200]).expect("fixture"));
    bus.write16(PALETTE_BASE, 0x7FFF).expect("backdrop");
    bus.write16(PALETTE_BASE + 0x200 + 2, 0x001F)
        .expect("OBJ color");
    // 8x8 source tile, all pixels index 1.
    for offset in 0..32 {
        bus.write8(VRAM_BASE + 0x1_0000 + offset, 0x11)
            .expect("tile");
    }
    bus.write16(OAM_BASE, attr0).expect("attr0");
    bus.write16(OAM_BASE + 2, 0).expect("attr1");
    bus.write16(OAM_BASE + 4, 0).expect("attr2");
    for (index, value) in matrix.into_iter().enumerate() {
        bus.write16(OAM_BASE + 6 + index as u32 * 8, value as u16)
            .expect("matrix");
    }
    bus.write16(DISPCNT_ADDR, OBJ).expect("Mode 0 OBJ");
    bus.write16(0x0400_004C, mosaic).expect("MOSAIC");
    bus
}

#[test]
fn identity_double_size_centers_source_and_clips_twice_the_dimensions() {
    let bus = setup(0x0300, [0x100, 0, 0, 0x100], 0);
    let image = display::snapshot(&bus).expect("snapshot");
    let red = display::rgb555_to_rgb888(0x001F);
    let white = display::rgb555_to_rgb888(0x7FFF);
    assert_eq!(image.pixel(0, 0), red);
    assert_eq!(image.pixel(3, 3), red);
    assert_eq!(image.pixel(4, 4), red);
    assert_eq!(image.pixel(7, 7), red);
    assert_eq!(image.pixel(8, 8), white);
}

#[test]
fn double_size_keeps_the_existing_offscreen_y_rejection() {
    // The expanded canvas would reach scanline 156, but Y=160 remains offscreen.
    let bus = setup(0x03A0, [0x0100, 0, 0, 0x0080], 0);
    let image = display::snapshot(&bus).expect("snapshot");
    let white = display::rgb555_to_rgb888(0x7FFF);
    assert_eq!(image.pixel(0, 156), white);
}

#[test]
fn expanded_affine_mapping_and_mosaic_use_canvas_bounds() {
    // A half-scale inverse transform samples the source across the expanded
    // canvas; 4x1 screen-aligned mosaic groups retain each group's sampled color.
    let mut bus = setup(0x1300, [0x0080, 0, 0, 0x0100], 0x0300);
    bus.write16(PALETTE_BASE + 0x200 + 4, 0x03E0)
        .expect("second OBJ color");
    for y in 0..8 {
        for (x, value) in [0x11, 0x11, 0x22, 0x22].into_iter().enumerate() {
            bus.write8(VRAM_BASE + 0x1_0000 + y * 4 + x as u32, value)
                .expect("two-color tile");
        }
    }
    let image = display::snapshot(&bus).expect("snapshot");
    let red = display::rgb555_to_rgb888(0x001F);
    let green = display::rgb555_to_rgb888(0x03E0);
    assert_eq!(image.pixel(0, 3), red);
    assert_eq!(image.pixel(3, 3), red);
    assert_eq!(image.pixel(4, 3), green);
    assert_eq!(image.pixel(7, 3), green);
    assert_eq!(image.pixel(8, 3), green);
    assert_eq!(image.pixel(11, 3), green);
    assert_eq!(image.pixel(12, 3), display::rgb555_to_rgb888(0x7FFF));
}
