//! Pixel oracles for Mode 0 BG1-BG3/OBJ and Mode 1 affine/window/blend layers.

use gba_native_core::bus::{
    BG0CNT_ADDR, BG2CNT_ADDR, BG2PA_ADDR, BG2PB_ADDR, BG2PC_ADDR, BG2PD_ADDR, BG2X_ADDR, BG2Y_ADDR,
    BLDALPHA_ADDR, BLDCNT_ADDR, BLDY_ADDR, Bus, DISPCNT_ADDR, OAM_BASE, PALETTE_BASE, VRAM_BASE,
    WIN0H_ADDR, WIN0V_ADDR, WININ_ADDR, WINOUT_ADDR,
};
use gba_native_core::cart::Cartridge;
use gba_native_core::display;

const BG1_ON: u16 = 0x0200;
const BG2_ON: u16 = 0x0400;
const OBJ_ON: u16 = 0x1000;
const OBJ_1D: u16 = 0x0040;
const WINDOW0: u16 = 0x2000;

fn bus() -> Bus {
    Bus::new(Cartridge::from_bytes(vec![0; 0x200]).expect("synthetic cart"))
}

fn put_palette(bus: &mut Bus, index: usize, color: u16) {
    bus.write16(PALETTE_BASE + (index * 2) as u32, color)
        .expect("palette");
}

fn put_obj_palette(bus: &mut Bus, index: usize, color: u16) {
    bus.write16(PALETTE_BASE + 0x200 + (index * 2) as u32, color)
        .expect("OBJ palette");
}

fn put_vram8(bus: &mut Bus, offset: usize, value: u8) {
    bus.write8(VRAM_BASE + offset as u32, value)
        .expect("VRAM byte");
}

fn put_tile_4bpp(bus: &mut Bus, tile: usize, color: u8) {
    for index in 0..32 {
        put_vram8(bus, tile * 32 + index, color | (color << 4));
    }
}

fn put_tile_8bpp(bus: &mut Bus, tile: usize, color: u8) {
    for index in 0..64 {
        put_vram8(bus, tile * 64 + index, color);
    }
}

#[test]
fn mode0_bg1_renders_independently_of_bg0() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    put_palette(&mut bus, 5, 0x001F);
    put_tile_4bpp(&mut bus, 1, 5);
    bus.write16(VRAM_BASE, 1).expect("BG1 map");
    bus.write16(BG0CNT_ADDR + 2, 0).expect("BG1CNT");
    bus.write16(DISPCNT_ADDR, BG1_ON).expect("Mode 0 BG1");
    let image = display::snapshot(&bus).expect("BG1 snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
}

#[test]
fn mode1_bg2_affine_identity_samples_the_map() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    put_palette(&mut bus, 7, 0x03E0);
    put_vram8(&mut bus, 0, 1); // affine map entry
    put_tile_8bpp(&mut bus, 1, 7);
    bus.write16(BG2CNT_ADDR, 0x0080).expect("BG2CNT");
    for (addr, value) in [
        (BG2PA_ADDR, 0x0100),
        (BG2PB_ADDR, 0),
        (BG2PC_ADDR, 0),
        (BG2PD_ADDR, 0x0100),
    ] {
        bus.write16(addr, value).expect("BG2 matrix");
    }
    bus.write32(BG2X_ADDR, 0).expect("BG2X");
    bus.write32(BG2Y_ADDR, 0).expect("BG2Y");
    bus.write16(DISPCNT_ADDR, 0x0001 | BG2_ON)
        .expect("Mode 1 BG2");
    let image = display::snapshot(&bus).expect("affine snapshot");
    assert_eq!(image.pixel(0, 0), 0x0000_FF00);
}

#[test]
fn mode0_normal_and_affine_obj_sample_the_tile_and_palette() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    put_obj_palette(&mut bus, 3, 0x001F);
    put_vram8(&mut bus, 0x1_0000 + 32, 0x33);
    bus.write16(OAM_BASE, 0x0000).expect("attr0");
    bus.write16(OAM_BASE + 2, 0x0000).expect("attr1");
    bus.write16(OAM_BASE + 4, 1).expect("attr2");
    bus.write16(DISPCNT_ADDR, OBJ_ON | OBJ_1D).expect("OBJ");
    let image = display::snapshot(&bus).expect("OBJ snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);

    bus.write16(OAM_BASE, 0x0100).expect("affine attr0");
    for (addr, value) in [
        (OAM_BASE + 6, 0x0100),
        (OAM_BASE + 14, 0),
        (OAM_BASE + 22, 0),
        (OAM_BASE + 30, 0x0100),
    ] {
        bus.write16(addr, value).expect("affine parameter");
    }
    let image = display::snapshot(&bus).expect("affine OBJ snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
}

#[test]
fn window0_clips_layers_and_uses_the_backdrop_outside() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7C00);
    put_palette(&mut bus, 1, 0x001F);
    put_tile_4bpp(&mut bus, 1, 1);
    bus.write16(VRAM_BASE, 1).expect("BG0 map");
    bus.write16(BG0CNT_ADDR, 0).expect("BG0CNT");
    bus.write16(WIN0H_ADDR, 0x0008).expect("WIN0H");
    bus.write16(WIN0V_ADDR, 0x0008).expect("WIN0V");
    bus.write16(WININ_ADDR, 0x3F).expect("WININ");
    bus.write16(WINOUT_ADDR, 0).expect("WINOUT");
    bus.write16(DISPCNT_ADDR, 0x0100 | WINDOW0)
        .expect("windowed BG0");
    let image = display::snapshot(&bus).expect("window snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
    assert_eq!(image.pixel(10, 0), 0x0000_00FF);
}

#[test]
fn alpha_and_brightness_blending_follow_bldcnt() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x001F); // backdrop blue
    put_palette(&mut bus, 1, 0x7C00); // BG0 blue
    put_tile_4bpp(&mut bus, 1, 1);
    bus.write16(VRAM_BASE, 1).expect("BG0 map");
    bus.write16(BG0CNT_ADDR, 0).expect("BG0CNT");
    bus.write16(DISPCNT_ADDR, 0x0100).expect("BG0");

    bus.write16(BLDCNT_ADDR, 0x2041).expect("alpha BLDCNT");
    bus.write16(BLDALPHA_ADDR, 0x0808).expect("BLDALPHA");
    bus.write16(BLDY_ADDR, 0).expect("BLDY");
    let image = display::snapshot(&bus).expect("alpha snapshot");
    assert_eq!(image.pixel(0, 0), 0x007F_007F);

    bus.write16(BLDCNT_ADDR, 0x0081).expect("brightness BLDCNT");
    bus.write16(BLDY_ADDR, 8).expect("BLDY");
    let image = display::snapshot(&bus).expect("brightness snapshot");
    assert_eq!(image.pixel(0, 0), 0x007F_7FFF);

    // BLDY is documented as 0..16; larger values clamp instead of underflowing.
    bus.write16(BLDCNT_ADDR, 0x00C1).expect("darkness BLDCNT");
    bus.write16(BLDY_ADDR, 31).expect("large BLDY");
    let image = display::snapshot(&bus).expect("darkness snapshot");
    assert_eq!(image.pixel(0, 0), 0);

    // EVA/EVB 31 clamp to 16/16: blue BG0 over red backdrop -> magenta.
    bus.write16(BLDCNT_ADDR, 0x2041).expect("alpha BLDCNT");
    bus.write16(BLDALPHA_ADDR, 0x1F1F).expect("large EVA/EVB");
    bus.write16(BLDY_ADDR, 0).expect("BLDY");
    let image = display::snapshot(&bus).expect("clamped alpha snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_00FF);
}

#[test]
fn mode0_8bpp_obj_uses_32_byte_tile_units() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    put_obj_palette(&mut bus, 5, 0x001F);
    put_obj_palette(&mut bus, 6, 0x03E0);
    // 1D 8bpp tiles are numbered in 32-byte units: tile 2 is at +0x40,
    // tile 4 at +0x80.
    put_vram8(&mut bus, 0x1_0040, 5);
    put_vram8(&mut bus, 0x1_0080, 6);
    bus.write16(OAM_BASE, 0x2000).expect("8bpp attr0");
    bus.write16(OAM_BASE + 2, 0x4000).expect("16x16 attr1");
    bus.write16(OAM_BASE + 4, 2).expect("tile 2 attr2");
    bus.write16(DISPCNT_ADDR, OBJ_ON | OBJ_1D).expect("OBJ");
    let image = display::snapshot(&bus).expect("8bpp OBJ snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
    assert_eq!(image.pixel(8, 0), 0x0000_FF00);
}

#[test]
fn obj_semi_transparent_and_double_size_are_explicit_errors() {
    let mut bus = bus();
    bus.write16(OAM_BASE, 0x0400)
        .expect("semi-transparent mode");
    bus.write16(DISPCNT_ADDR, OBJ_ON | OBJ_1D).expect("OBJ");
    assert!(matches!(
        display::snapshot(&bus),
        Err(gba_native_core::error::Error::UnsupportedPpuFeature { .. })
    ));

    bus.write16(OAM_BASE, 0x0300).expect("double-size affine");
    assert!(matches!(
        display::snapshot(&bus),
        Err(gba_native_core::error::Error::UnsupportedPpuFeature { .. })
    ));
}

#[test]
fn alpha_blending_requires_the_immediately_next_layer_to_be_a_second_target() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0);
    put_palette(&mut bus, 1, 0x001F); // BG0 red
    put_palette(&mut bus, 2, 0x03E0); // BG1 green
    put_palette(&mut bus, 3, 0x7C00); // BG2 blue
    put_tile_4bpp(&mut bus, 1, 1);
    put_tile_4bpp(&mut bus, 2, 2);
    put_tile_4bpp(&mut bus, 3, 3);
    bus.write16(VRAM_BASE, 1).expect("BG0 map");
    bus.write16(VRAM_BASE + 0x800, 2).expect("BG1 map");
    bus.write16(VRAM_BASE + 0x1000, 3).expect("BG2 map");
    bus.write16(BG0CNT_ADDR, 0).expect("BG0CNT");
    bus.write16(BG0CNT_ADDR + 2, 0x0100).expect("BG1CNT");
    bus.write16(BG2CNT_ADDR, 0x0200).expect("BG2CNT");
    bus.write16(DISPCNT_ADDR, 0x0100 | 0x0200 | 0x0400)
        .expect("three text BGs");
    bus.write16(BLDALPHA_ADDR, 0x0808).expect("BLDALPHA");

    // BG1 is the next lower layer but not a second target: no blending, even
    // though BG2 further below is a second target.
    bus.write16(BLDCNT_ADDR, 0x0441).expect("second BG2");
    let image = display::snapshot(&bus).expect("non-adjacent alpha");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);

    // Make BG1 the second target: BG0/BG1 are now adjacent and blend.
    bus.write16(BLDCNT_ADDR, 0x0241).expect("second BG1");
    let image = display::snapshot(&bus).expect("adjacent alpha");
    assert_eq!(image.pixel(0, 0), 0x007F_7F00);
}
