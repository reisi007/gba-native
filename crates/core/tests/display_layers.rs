//! Pixel oracles for Mode 0 BG1-BG3/OBJ and Mode 1/2 affine/window/blend layers.

use gba_native_core::bus::{
    BG0CNT_ADDR, BG1CNT_ADDR, BG2CNT_ADDR, BG2PA_ADDR, BG2PB_ADDR, BG2PC_ADDR, BG2PD_ADDR,
    BG2X_ADDR, BG2Y_ADDR, BLDALPHA_ADDR, BLDCNT_ADDR, BLDY_ADDR, Bus, DISPCNT_ADDR, MOSAIC_ADDR,
    OAM_BASE, PALETTE_BASE, VRAM_BASE, WIN0H_ADDR, WIN0V_ADDR, WIN1H_ADDR, WIN1V_ADDR, WININ_ADDR,
    WINOUT_ADDR,
};
use gba_native_core::cart::Cartridge;
use gba_native_core::display;

const BG1_ON: u16 = 0x0200;
const BG2_ON: u16 = 0x0400;
const OBJ_ON: u16 = 0x1000;
const OBJ_1D: u16 = 0x0040;
const WINDOW0: u16 = 0x2000;
const WINDOW1: u16 = 0x4000;
const OBJ_WINDOW: u16 = 0x8000;

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
fn mode1_text_and_affine_bg_mosaic_sample_screen_group_origins() {
    let mut bus = bus();
    for index in 1..=64 {
        put_palette(&mut bus, index, index as u16);
    }
    for x in 0..4 {
        let low = (x * 2 + 1) as u8;
        let high = (x * 2 + 2) as u8;
        bus.write16(
            VRAM_BASE + 64 + (x * 2) as u32,
            u16::from_le_bytes([low, high]),
        )
        .expect("BG1 tile pixels");
    }
    bus.write16(VRAM_BASE, 1).expect("BG1 map");
    bus.write16(BG1CNT_ADDR, 0x00C0)
        .expect("8bpp BG1 with mosaic");
    bus.write16(MOSAIC_ADDR, 0x0012).expect("3x2 BG mosaic");
    bus.write16(DISPCNT_ADDR, 0x0001 | BG1_ON)
        .expect("Mode 1 BG1");

    let image = display::snapshot(&bus).expect("Mode 1 text mosaic");
    assert_eq!(image.pixel(0, 0), display::rgb555_to_rgb888(1));
    assert_eq!(image.pixel(2, 0), display::rgb555_to_rgb888(1));
    assert_eq!(image.pixel(3, 0), display::rgb555_to_rgb888(4));

    for y in 0..8 {
        for x in (0..8).step_by(2) {
            let low = (1 + y * 8 + x) as u8;
            let high = low + 1;
            bus.write16(
                VRAM_BASE + 64 + (y * 8 + x) as u32,
                u16::from_le_bytes([low, high]),
            )
            .expect("affine BG tile pixels");
        }
    }
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
    bus.write16(BG2CNT_ADDR, 0x00C0)
        .expect("8bpp affine BG2 with mosaic");
    bus.write16(DISPCNT_ADDR, 0x0001 | BG2_ON)
        .expect("Mode 1 affine BG2");
    let image = display::snapshot(&bus).expect("Mode 1 affine BG mosaic");
    assert_eq!(image.pixel(0, 0), display::rgb555_to_rgb888(1));
    assert_eq!(image.pixel(2, 0), display::rgb555_to_rgb888(1));
    assert_eq!(image.pixel(3, 0), display::rgb555_to_rgb888(4));
    assert_eq!(image.pixel(1, 1), display::rgb555_to_rgb888(1));
    assert_eq!(image.pixel(2, 2), display::rgb555_to_rgb888(17));
    assert_eq!(image.pixel(3, 2), display::rgb555_to_rgb888(20));
}

#[test]
fn mode0_normal_affine_and_mosaic_obj_sample_tiles_and_palette() {
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

    for index in 1..=64 {
        put_obj_palette(&mut bus, index, index as u16);
    }
    for y in 0..8 {
        for x in (0..8).step_by(2) {
            let low = (1 + y * 8 + x) as u8;
            let high = low + 1;
            bus.write16(
                VRAM_BASE + 0x1_0000 + 32 + (y * 8 + x) as u32,
                u16::from_le_bytes([low, high]),
            )
            .expect("mosaic OBJ tile pixels");
        }
    }
    bus.write16(OAM_BASE, 0x3001)
        .expect("8bpp mosaic OBJ at 1,1");
    bus.write16(OAM_BASE + 2, 1).expect("OBJ origin");
    bus.write16(OAM_BASE + 4, 1).expect("OBJ tile");
    bus.write16(MOSAIC_ADDR, 0x1200).expect("3x2 OBJ mosaic");
    let image = display::snapshot(&bus).expect("OBJ mosaic snapshot");
    assert_eq!(image.pixel(1, 1), display::rgb555_to_rgb888(1));
    assert_eq!(image.pixel(2, 1), display::rgb555_to_rgb888(1));
    assert_eq!(image.pixel(3, 1), display::rgb555_to_rgb888(3));
    assert_eq!(image.pixel(1, 2), display::rgb555_to_rgb888(9));
    assert_eq!(image.pixel(2, 3), display::rgb555_to_rgb888(9));
    assert_eq!(image.pixel(3, 3), display::rgb555_to_rgb888(11));

    put_obj_palette(&mut bus, 3, 0x001F);
    put_vram8(&mut bus, 0x1_0000 + 32, 0x33);
    bus.write16(OAM_BASE, 0x0100).expect("affine attr0");
    bus.write16(OAM_BASE + 2, 0).expect("affine OBJ origin");
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

    bus.write16(OAM_BASE, 0x3100).expect("affine mosaic OBJ");
    bus.write16(MOSAIC_ADDR, 0x1101).expect("2x2 OBJ mosaic");
    let image = display::snapshot(&bus).expect("affine OBJ mosaic snapshot");
    assert_eq!(image.pixel(0, 0), display::rgb555_to_rgb888(0x33));
    assert_eq!(image.pixel(1, 0), display::rgb555_to_rgb888(0x33));
    assert_eq!(image.pixel(2, 0), display::rgb555_to_rgb888(0x001F));
    assert_eq!(image.pixel(0, 1), display::rgb555_to_rgb888(0x33));
    assert_eq!(image.pixel(2, 2), display::rgb555_to_rgb888(19));

    bus.write16(OAM_BASE + 6, 0x0200)
        .expect("affine OBJ horizontal scale");
    bus.write16(MOSAIC_ADDR, 0x0300).expect("4x1 OBJ mosaic");
    let image = display::snapshot(&bus).expect("affine OBJ clipped mosaic group");
    assert_eq!(image.pixel(0, 0), display::rgb555_to_rgb888(0x33));
    assert_eq!(image.pixel(3, 0), display::rgb555_to_rgb888(0x33));
    assert_eq!(image.pixel(4, 0), display::rgb555_to_rgb888(5));
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

    // Window 0 wins its overlap with Window 1; Window 1 wins over OBJ window.
    // Distinct BG palette indices make each selected mask observable.
    put_palette(&mut bus, 0, 0x7C00);
    put_palette(&mut bus, 2, 0x03E0);
    put_palette(&mut bus, 3, 0x7FFF);
    put_tile_4bpp(&mut bus, 2, 2);
    put_tile_4bpp(&mut bus, 3, 3);
    bus.write16(VRAM_BASE, 0x0001).expect("BG tile 1");
    bus.write16(VRAM_BASE + 2, 0x0002).expect("BG tile 2");
    bus.write16(VRAM_BASE + 4, 0x0003).expect("BG tile 3");
    bus.write16(WIN0H_ADDR, 0x0408).expect("WIN0H");
    bus.write16(WIN1H_ADDR, 0x040C).expect("WIN1H");
    bus.write16(WIN0V_ADDR, 0x0008).expect("WIN0V");
    bus.write16(WIN1V_ADDR, 0x0008).expect("WIN1V");
    bus.write16(WININ_ADDR, 0x0001)
        .expect("Window 0 BG0, Window 1 none");
    bus.write16(WINOUT_ADDR, 0x0101)
        .expect("outside and OBJ window BG0");
    put_tile_4bpp(&mut bus, 0x0800, 1);
    bus.write16(OAM_BASE, 0x0800).expect("OBJ window mode");
    bus.write16(OAM_BASE + 2, 8).expect("OBJ window position");
    bus.write16(OAM_BASE + 4, 0).expect("OBJ window tile");
    bus.write16(
        DISPCNT_ADDR,
        0x0100 | OBJ_ON | WINDOW0 | WINDOW1 | OBJ_WINDOW,
    )
    .expect("all windows");
    let image = display::snapshot(&bus).expect("combined window snapshot");
    assert_eq!(image.pixel(2, 0), 0x00FF_0000); // outside
    assert_eq!(image.pixel(6, 0), 0x00FF_0000); // WIN0 > WIN1
    assert_eq!(image.pixel(10, 0), 0x0000_00FF); // WIN1 > OBJ window; backdrop
    assert_eq!(image.pixel(14, 0), 0x0000_FF00); // OBJ-window-only mask

    // Window masks independently control whether color effects are applied.
    bus.write16(WININ_ADDR, 0x0101)
        .expect("Window 0/1 BG0 masks");
    bus.write16(WINOUT_ADDR, 0x0121)
        .expect("outside effects enabled");
    bus.write16(BLDCNT_ADDR, 0x0081)
        .expect("brightness-up on BG0");
    bus.write16(BLDY_ADDR, 16).expect("maximum brightness");
    let image = display::snapshot(&bus).expect("window effect masks");
    assert_eq!(image.pixel(2, 0), 0x00FF_FFFF); // outside effects enabled
    assert_eq!(image.pixel(6, 0), 0x00FF_0000); // Window 0 effects disabled
    assert_eq!(image.pixel(10, 0), 0x0000_FF00); // Window 1 effects disabled
    assert_eq!(image.pixel(14, 0), 0x0000_FF00); // OBJ-window effects disabled

    // Window 1 is shared by the Mode 1 text/affine compositor as well.
    bus.write16(BG0CNT_ADDR, 0).expect("Mode 1 BG0CNT");
    bus.write16(WININ_ADDR, 0x0100).expect("Window 1 BG0 only");
    bus.write16(WINOUT_ADDR, 0).expect("outside disabled");
    bus.write16(DISPCNT_ADDR, 0x0001 | 0x0100 | WINDOW1)
        .expect("Mode 1 Window 1");
    let image = display::snapshot(&bus).expect("Mode 1 Window 1 snapshot");
    assert_eq!(image.pixel(2, 0), 0x0000_00FF); // outside backdrop
    assert_eq!(image.pixel(6, 0), 0x00FF_0000); // Window 1 BG0

    // With no BG/OBJ enabled, windows still gate effects on the backdrop.
    bus.write16(WININ_ADDR, 0)
        .expect("Window 1 effects disabled");
    bus.write16(WINOUT_ADDR, 0x0020)
        .expect("outside backdrop effects enabled");
    bus.write16(BLDCNT_ADDR, 0x00A0)
        .expect("brightness-up on backdrop");
    bus.write16(DISPCNT_ADDR, WINDOW1)
        .expect("Mode 0 Window 1 backdrop");
    let image = display::snapshot(&bus).expect("windowed backdrop effects");
    assert_eq!(image.pixel(2, 0), 0x00FF_FFFF); // outside effect enabled
    assert_eq!(image.pixel(6, 0), 0x0000_00FF); // Window 1 effect disabled
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
fn mode0_obj_uses_32_byte_tile_units_in_1d_and_2d() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x7FFF);
    put_obj_palette(&mut bus, 5, 0x001F);
    put_obj_palette(&mut bus, 6, 0x03E0);
    put_obj_palette(&mut bus, 7, 0x7C00);
    put_obj_palette(&mut bus, 8, 0x7FFF);
    // The 2x2 tiles are distinct in both layouts. 1D uses indices 2,4,6,8;
    // 2D uses indices 2,4,34,36 (8bpp still advances in 32-byte units).
    for (tile, color) in [(2, 5), (4, 6), (6, 7), (8, 8), (34, 8), (36, 7)] {
        put_vram8(&mut bus, 0x1_0000 + tile * 32, color);
    }
    bus.write16(OAM_BASE, 0x2000).expect("8bpp attr0");
    bus.write16(OAM_BASE + 2, 0x4000).expect("16x16 attr1");
    bus.write16(OAM_BASE + 4, 2).expect("tile 2 attr2");
    bus.write16(DISPCNT_ADDR, OBJ_ON | OBJ_1D).expect("OBJ");
    let image = display::snapshot(&bus).expect("8bpp OBJ snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
    assert_eq!(image.pixel(8, 0), 0x0000_FF00);
    assert_eq!(image.pixel(0, 8), 0x0000_00FF);
    assert_eq!(image.pixel(8, 8), 0x00FF_FFFF);

    bus.write16(DISPCNT_ADDR, OBJ_ON).expect("2D OBJ");
    let image = display::snapshot(&bus).expect("2D 8bpp OBJ snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
    assert_eq!(image.pixel(8, 0), 0x0000_FF00);
    assert_eq!(image.pixel(0, 8), 0x00FF_FFFF);
    assert_eq!(image.pixel(8, 8), 0x0000_00FF);

    for (index, color) in [(1, 0x001F), (2, 0x03E0), (3, 0x7C00), (4, 0x7FFF)] {
        put_obj_palette(&mut bus, index, color);
    }
    for (tile, color_index) in [(1, 1), (2, 2), (33, 3), (34, 4)] {
        put_vram8(&mut bus, 0x1_0000 + tile * 32, color_index);
    }
    bus.write16(OAM_BASE, 0).expect("4bpp OBJ");
    bus.write16(OAM_BASE + 4, 1).expect("2D 4bpp tile base");
    let image = display::snapshot(&bus).expect("2D 4bpp OBJ snapshot");
    assert_eq!(image.pixel(0, 0), display::rgb555_to_rgb888(0x001F));
    assert_eq!(image.pixel(8, 0), display::rgb555_to_rgb888(0x03E0));
    assert_eq!(image.pixel(0, 8), display::rgb555_to_rgb888(0x7C00));
    assert_eq!(image.pixel(8, 8), display::rgb555_to_rgb888(0x7FFF));
}

#[test]
fn obj_semi_transparent_mode_without_eligible_alpha_target_keeps_brightness_fallback() {
    let mut bus = bus();
    bus.write16(OAM_BASE, 0x0400)
        .expect("semi-transparent mode");
    bus.write16(DISPCNT_ADDR, OBJ_ON | OBJ_1D).expect("OBJ");
    put_obj_palette(&mut bus, 1, 0x001F);
    put_vram8(&mut bus, 0x1_0000, 1);
    bus.write16(BLDCNT_ADDR, 0x0090)
        .expect("OBJ brightness target");
    bus.write16(BLDY_ADDR, 16).expect("full brighten");
    let image = display::snapshot(&bus).expect("semi-transparent OBJ snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_FFFF);

    bus.write16(BLDCNT_ADDR, 0x00D0)
        .expect("OBJ darkness target");
    let image = display::snapshot(&bus).expect("mode-3 brightness fallback");
    assert_eq!(image.pixel(0, 0), 0x0000_0000);
}

#[test]
fn semi_transparent_obj_forces_alpha_first_target_and_blends_backdrop() {
    let mut bus = bus();
    put_palette(&mut bus, 0, 0x03E0); // green backdrop
    put_obj_palette(&mut bus, 1, 0x001F); // red OBJ
    for offset in 0..32 {
        put_vram8(&mut bus, 0x1_0000 + offset, 0x11);
    }
    bus.write16(OAM_BASE, 0x0400)
        .expect("semi-transparent Attr0");
    bus.write16(BLDCNT_ADDR, 0x2050)
        .expect("backdrop second target");
    bus.write16(BLDALPHA_ADDR, 0x0808)
        .expect("equal alpha coefficients");
    bus.write16(BLDY_ADDR, 16)
        .expect("brightness fallback amount");
    bus.write16(DISPCNT_ADDR, OBJ_ON | OBJ_1D)
        .expect("OBJ enable");

    let image = display::snapshot(&bus).expect("semi-transparent OBJ snapshot");
    assert_eq!(image.pixel(0, 0), 0x007F_7F00);
}

#[test]
fn reserved_obj_mode_three_remains_an_error() {
    let mut bus = bus();
    bus.write16(OAM_BASE, 0x0C00).expect("reserved OBJ mode");
    bus.write16(DISPCNT_ADDR, OBJ_ON | OBJ_1D)
        .expect("OBJ enable");
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
