//! Independent pixel oracles for the static Mode 0 BG0 text-background subset.

use gba_native_core::bus::{
    BG0CNT_ADDR, BG0HOFS_ADDR, BG0VOFS_ADDR, Bus, DISPCNT_ADDR, PALETTE_BASE, VRAM_BASE,
};
use gba_native_core::cart::Cartridge;
use gba_native_core::display;
use gba_native_core::error::Error;

const BG0_ON: u16 = 0x0100;

fn bus() -> Bus {
    Bus::new(Cartridge::from_bytes(vec![0; 0x200]).expect("synthetic cart"))
}

fn set_mode0(bus: &mut Bus, bgcnt: u16) {
    bus.write16(BG0CNT_ADDR, bgcnt).expect("BG0CNT");
    bus.write16(DISPCNT_ADDR, BG0_ON).expect("Mode 0 BG0");
}

fn write_vram(bus: &mut Bus, offset: usize, bytes: &[u8]) {
    assert_eq!(offset % 2, 0);
    assert_eq!(bytes.len() % 2, 0);
    for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        bus.write16(
            VRAM_BASE + offset as u32 + (index * 2) as u32,
            u16::from_le_bytes([pair[0], pair[1]]),
        )
        .expect("VRAM halfword");
    }
}

fn put_map_entry(bus: &mut Bus, offset: usize, entry: u16) {
    bus.write16(VRAM_BASE + offset as u32, entry)
        .expect("BG map entry");
}

fn put_palette(bus: &mut Bus, index: usize, color: u16) {
    bus.write16(PALETTE_BASE + (index * 2) as u32, color)
        .expect("palette entry");
}

fn set_uniform_4bpp_tile(bus: &mut Bus, tile: usize, color: u8) {
    write_vram(bus, tile * 32, &[(color | (color << 4)); 32]);
}

#[test]
fn mode0_4bpp_uses_palette_bank_transparency_and_backdrop() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x001F);
    put_palette(&mut bus, 49, 0x03E0);
    put_palette(&mut bus, 50, 0x7C00);
    // Tile 1 begins with transparent, bank-color 2, bank-color 1, transparent.
    write_vram(&mut bus, 0x4000 + 32, &[0x20, 0x01]);
    put_map_entry(&mut bus, 0x0800, 1 | (3 << 12));
    // Priority 3 is accepted/ignored; character and screen bases select block 1.
    set_mode0(&mut bus, 0x0107);

    let image = display::snapshot(&bus).expect("Mode 0 snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
    assert_eq!(image.pixel(1, 0), 0x0000_00FF);
    assert_eq!(image.pixel(2, 0), 0x0000_FF00);
    assert_eq!(image.pixel(3, 0), 0x00FF_0000);
}

#[test]
fn mode0_8bpp_uses_the_shared_palette_and_transparent_zero() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7C00);
    put_palette(&mut bus, 7, 0x001F);
    put_palette(&mut bus, 200, 0x03E0);
    write_vram(&mut bus, 64, &[7, 0, 200, 0]);
    put_map_entry(&mut bus, 0, 1);
    set_mode0(&mut bus, 0x0080);

    let image = display::snapshot(&bus).expect("Mode 0 snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
    assert_eq!(image.pixel(1, 0), 0x0000_00FF);
    assert_eq!(image.pixel(2, 0), 0x0000_FF00);
}

#[test]
fn mode0_map_entry_flips_tile_pixels_horizontally_and_vertically() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    for index in 1..=28 {
        put_palette(&mut bus, index, index as u16);
    }
    let mut tile = [0; 64];
    for x in 0..8 {
        tile[7 * 8 + x] = (x + 1) as u8;
        tile[x] = (x + 21) as u8;
    }
    write_vram(&mut bus, 64, &tile);
    put_map_entry(&mut bus, 0, 1 | 0x0400 | 0x0800);
    set_mode0(&mut bus, 0x0080);

    let image = display::snapshot(&bus).expect("Mode 0 snapshot");
    assert_eq!(image.pixel(0, 0), display::rgb555_to_rgb888(8));
    assert_eq!(image.pixel(7, 0), display::rgb555_to_rgb888(1));
    assert_eq!(image.pixel(0, 7), display::rgb555_to_rgb888(28));
}

#[test]
fn mode0_sizes_select_the_documented_map_quadrants() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    for tile in 1..=4 {
        put_palette(&mut bus, tile, [0x001F, 0x03E0, 0x7C00, 0x4210][tile - 1]);
        set_uniform_4bpp_tile(&mut bus, tile, tile as u8);
    }
    put_map_entry(&mut bus, 0x0000, 1);
    put_map_entry(&mut bus, 0x0800, 2);
    put_map_entry(&mut bus, 0x1000, 3);
    put_map_entry(&mut bus, 0x1800, 4);

    for (size, scroll_x, scroll_y, tile) in [
        (0, 0, 0, 1),
        (1, 256, 0, 2),
        (2, 0, 256, 2),
        (3, 0, 256, 3),
        (3, 256, 256, 4),
    ] {
        set_mode0(&mut bus, size << 14);
        bus.write16(BG0HOFS_ADDR, scroll_x)
            .expect("horizontal scroll");
        bus.write16(BG0VOFS_ADDR, scroll_y)
            .expect("vertical scroll");
        let image = display::snapshot(&bus).expect("size snapshot");
        let expected = display::rgb555_to_rgb888([0, 0x001F, 0x03E0, 0x7C00, 0x4210][tile]);
        assert_eq!(image.pixel(0, 0), expected, "size {size}");
    }
}

#[test]
fn mode0_scroll_uses_low_nine_bits_and_wraps_both_axes() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    put_palette(&mut bus, 1, 0x001F);
    put_palette(&mut bus, 2, 0x03E0);
    set_uniform_4bpp_tile(&mut bus, 1, 1);
    set_uniform_4bpp_tile(&mut bus, 2, 2);
    put_map_entry(&mut bus, (31 * 32 + 31) * 2, 2);
    put_map_entry(&mut bus, (31 * 32) * 2, 1);
    bus.write16(BG0HOFS_ADDR, 0xFFFF)
        .expect("horizontal scroll");
    bus.write16(BG0VOFS_ADDR, 0xFFFF).expect("vertical scroll");
    set_mode0(&mut bus, 0);

    let image = display::snapshot(&bus).expect("wrapped snapshot");
    assert_eq!(image.pixel(0, 0), 0x0000_FF00);
    assert_eq!(image.pixel(1, 0), 0x00FF_0000);
}

#[test]
fn mode0_disabled_bg_uses_backdrop_and_forced_blank_is_white() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x03E0);
    put_palette(&mut bus, 1, 0x001F);
    set_uniform_4bpp_tile(&mut bus, 1, 1);
    put_map_entry(&mut bus, 0, 1);

    bus.write16(DISPCNT_ADDR, 0)
        .expect("Mode 0 with BG0 disabled");
    let image = display::snapshot(&bus).expect("disabled BG0 snapshot");
    assert_eq!(image.pixel(0, 0), 0x0000_FF00);

    bus.write16(DISPCNT_ADDR, BG0_ON | 0x0080)
        .expect("forced blank");
    let image = display::snapshot(&bus).expect("forced-blank snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_FFFF);

    for bgcnt in [0x0040, 0x0010, 0x2000] {
        bus.write16(BG0CNT_ADDR, bgcnt).expect("unsupported BG0CNT");
        assert!(matches!(
            display::snapshot(&bus),
            Err(Error::UnsupportedBgControl { .. })
        ));
    }
}

#[test]
fn mode0_rejects_mosaic_reserved_bgcnt_and_unimplemented_layers() {
    let mut bus = bus();
    for (bgcnt, reason) in [
        (0x0040, "mosaic"),
        (0x0010, "reserved"),
        (0x2000, "reserved"),
    ] {
        set_mode0(&mut bus, bgcnt);
        assert!(matches!(
            display::snapshot(&bus),
            Err(Error::UnsupportedBgControl { reason: actual, .. }) if actual.contains(reason)
        ));
    }
    set_mode0(&mut bus, 0);
    for dispcnt in [BG0_ON | 0x0008, BG0_ON | 0x4000] {
        bus.write16(DISPCNT_ADDR, dispcnt)
            .expect("unsupported layer");
        assert!(matches!(
            display::snapshot(&bus),
            Err(Error::UnsupportedPpuFeature { .. })
        ));
    }
    // BG1-BG3 and OBJ are now implemented; with no data they fall through to
    // the backdrop instead of being rejected as unknown layers.
    for layer in [0x0200, 0x0400, 0x0800, 0x1040] {
        bus.write16(DISPCNT_ADDR, BG0_ON | layer)
            .expect("implemented layer");
        display::snapshot(&bus).expect("implemented layer snapshot");
    }
    bus.write16(DISPCNT_ADDR, 2).expect("unsupported mode");
    assert!(matches!(
        display::snapshot(&bus),
        Err(Error::UnsupportedDisplayMode { mode: 2 })
    ));
}

#[test]
fn mode0_rejects_map_and_tile_addresses_beyond_bg_vram() {
    let mut bus = bus();
    bus.write16(BG0HOFS_ADDR, 256).expect("horizontal scroll");
    bus.write16(BG0VOFS_ADDR, 256).expect("vertical scroll");
    set_mode0(&mut bus, (3 << 14) | (31 << 8));
    assert!(matches!(
        display::snapshot(&bus),
        Err(Error::BgVramOutOfRange { addr, width: 2 }) if addr == VRAM_BASE + 0x11000
    ));

    put_map_entry(&mut bus, 0, 256);
    bus.write16(BG0HOFS_ADDR, 0).expect("horizontal scroll");
    bus.write16(BG0VOFS_ADDR, 0).expect("vertical scroll");
    set_mode0(&mut bus, 0x008C);
    assert!(matches!(
        display::snapshot(&bus),
        Err(Error::BgVramOutOfRange { addr, width: 1 }) if addr == VRAM_BASE + 0x10000
    ));
}
