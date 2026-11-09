//! Synthetic pixel oracles for the bounded static Mode 2 affine BG subset.

use gba_native_core::bus::{
    BG2CNT_ADDR, BG2PA_ADDR, BG2PB_ADDR, BG2PC_ADDR, BG2PD_ADDR, BG2X_ADDR, BG2Y_ADDR, BG3CNT_ADDR,
    BG3PA_ADDR, BG3PB_ADDR, BG3PC_ADDR, BG3PD_ADDR, BG3X_ADDR, BG3Y_ADDR, Bus, DISPCNT_ADDR,
    PALETTE_BASE, VRAM_BASE,
};
use gba_native_core::cart::Cartridge;
use gba_native_core::display;

const BG2_ON: u16 = 0x0400;
const BG3_ON: u16 = 0x0800;

fn bus() -> Bus {
    Bus::new(Cartridge::from_bytes(vec![0; 0x200]).expect("synthetic cart"))
}

fn put_palette(bus: &mut Bus, index: usize, color: u16) {
    bus.write16(PALETTE_BASE + (index * 2) as u32, color)
        .expect("palette");
}

fn put_vram8(bus: &mut Bus, offset: usize, value: u8) {
    bus.write8(VRAM_BASE + offset as u32, value)
        .expect("VRAM byte");
}

fn put_tile_8bpp(bus: &mut Bus, tile: usize, color: u8) {
    for index in 0..64 {
        put_vram8(bus, tile * 64 + index, color);
    }
}

fn set_identity(bus: &mut Bus, matrix: [u32; 6]) {
    let [pa, pb, pc, pd, x, y] = matrix;
    for (addr, value) in [(pa, 0x0100), (pb, 0), (pc, 0), (pd, 0x0100)] {
        bus.write16(addr, value).expect("affine matrix");
    }
    bus.write32(x, 0).expect("reference X");
    bus.write32(y, 0).expect("reference Y");
}

#[test]
fn mode2_bg3_uses_its_own_reference_and_character_map_bases() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    put_palette(&mut bus, 9, 0x03E0);
    put_palette(&mut bus, 10, 0x001F);
    put_palette(&mut bus, 11, 0x7C00);
    put_vram8(&mut bus, 0x800, 2); // BG3 map, distinct screen base
    put_tile_8bpp(&mut bus, 0x4000 / 64 + 2, 9); // BG3 character base 1
    for (x, index) in [9, 10, 11].into_iter().enumerate() {
        put_vram8(&mut bus, 0x4000 + 2 * 64 + x, index);
    }
    bus.write16(BG3CNT_ADDR, 0x0184)
        .expect("BG3CNT bases and 8bpp");
    set_identity(
        &mut bus,
        [
            BG3PA_ADDR, BG3PB_ADDR, BG3PC_ADDR, BG3PD_ADDR, BG3X_ADDR, BG3Y_ADDR,
        ],
    );
    bus.write32(BG3X_ADDR, 0x0000_0100)
        .expect("BG3 reference X");
    bus.write16(DISPCNT_ADDR, 0x0002 | BG3_ON)
        .expect("Mode 2 BG3");
    let image = display::snapshot(&bus).expect("BG3 snapshot");
    assert_eq!(image.pixel(0, 0), display::rgb555_to_rgb888(0x001F));
    assert_eq!(image.pixel(1, 0), display::rgb555_to_rgb888(0x7C00));
}

#[test]
fn mode2_bg2_and_bg3_compose_by_priority_and_transparency() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    put_palette(&mut bus, 5, 0x001F);
    put_palette(&mut bus, 6, 0x7C00);
    put_vram8(&mut bus, 0, 1);
    put_tile_8bpp(&mut bus, 1, 5);
    put_vram8(&mut bus, 0x800, 2);
    put_tile_8bpp(&mut bus, 0x4000 / 64 + 2, 6);
    bus.write16(BG2CNT_ADDR, 0x0080 | 1)
        .expect("BG2 priority 1");
    bus.write16(BG3CNT_ADDR, 0x0184).expect("BG3 priority 0");
    set_identity(
        &mut bus,
        [
            BG2PA_ADDR, BG2PB_ADDR, BG2PC_ADDR, BG2PD_ADDR, BG2X_ADDR, BG2Y_ADDR,
        ],
    );
    set_identity(
        &mut bus,
        [
            BG3PA_ADDR, BG3PB_ADDR, BG3PC_ADDR, BG3PD_ADDR, BG3X_ADDR, BG3Y_ADDR,
        ],
    );
    bus.write16(DISPCNT_ADDR, 0x0002 | BG2_ON | BG3_ON)
        .expect("Mode 2 BG2/BG3");
    let image = display::snapshot(&bus).expect("concurrent affine snapshot");
    assert_eq!(image.pixel(0, 0), display::rgb555_to_rgb888(0x7C00));
    put_vram8(&mut bus, 0x4000 + 2 * 64, 0); // transparent BG3 texel
    let image = display::snapshot(&bus).expect("transparent BG3 fallback");
    assert_eq!(image.pixel(0, 0), display::rgb555_to_rgb888(0x001F));
}
