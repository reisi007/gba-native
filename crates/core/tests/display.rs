//! Display snapshot tests: complete mode-3 image, page selection, bounds,
//! blank/backdrop behavior and explicit rejection of unsupported PPU features.

use gba_native_core::bus::{BG2CNT_ADDR, Bus, DISPCNT_ADDR, OAM_BASE, PALETTE_BASE, VRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::display::{self, HEIGHT, WIDTH, rgb555_to_rgb888};
use gba_native_core::error::Error;

fn bus() -> Bus {
    Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"))
}

#[test]
fn mode3_full_image_matches_rgb555_conversion() {
    let mut bus = bus();
    bus.write16(DISPCNT_ADDR, 0x0403).expect("dispcnt");
    for index in 0..WIDTH * HEIGHT {
        bus.write16(VRAM_BASE + 2 * index, index as u16)
            .expect("pixel");
    }
    let image = display::snapshot(&bus).expect("snapshot");
    assert_eq!((image.width, image.height), (WIDTH, HEIGHT));
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let index = y * WIDTH + x;
            assert_eq!(image.pixel(x, y), rgb555_to_rgb888(index as u16));
        }
    }
    bus.write16(VRAM_BASE, 0xFFFF).expect("unused bit");
    let image = display::snapshot(&bus).expect("snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_FFFF);

    bus.write16(PALETTE_BASE + 0x202, 0x001F)
        .expect("OBJ red palette entry");
    bus.write16(OAM_BASE, 0).expect("normal OBJ");
    bus.write16(OAM_BASE + 2, 0).expect("OBJ origin");
    bus.write16(OAM_BASE + 4, 0).expect("OBJ tile and priority");
    for entry in 1..128 {
        bus.write16(OAM_BASE + entry * 8, 0x0200)
            .expect("disable unused OBJ");
    }
    for offset in (0..32).step_by(2) {
        bus.write16(VRAM_BASE + 0x1_4000 + offset, 0x1111)
            .expect("bitmap-mode OBJ tile");
    }
    bus.write16(BG2CNT_ADDR, 2).expect("BG2 priority");
    bus.write16(OAM_BASE + 4, 0x0800)
        .expect("OBJ priority tied with BG2");
    bus.write16(DISPCNT_ADDR, 0x1403)
        .expect("Mode 3 BG2 and OBJ");
    let image = display::snapshot(&bus).expect("Mode 3 OBJ overlay");
    assert_eq!(image.pixel(0, 0), rgb555_to_rgb888(0x001F));
    assert_eq!(image.pixel(8, 0), rgb555_to_rgb888(8));

    bus.write16(OAM_BASE + 4, 0x0C00)
        .expect("OBJ priority behind BG2");
    let image = display::snapshot(&bus).expect("BG2 wins priority");
    assert_eq!(image.pixel(0, 0), rgb555_to_rgb888(0xFFFF));

    bus.write16(OAM_BASE + 4, 0).expect("restore OBJ priority");
    bus.write16(DISPCNT_ADDR, 0x1003)
        .expect("Mode 3 OBJ without BG2");
    let image = display::snapshot(&bus).expect("OBJ over backdrop");
    assert_eq!(image.pixel(0, 0), rgb555_to_rgb888(0x001F));
    assert_eq!(image.pixel(8, 0), 0);

    bus.write16(OAM_BASE + 4, 0x0200)
        .expect("OBJ tile starts at bitmap OBJ end");
    assert!(matches!(
        display::snapshot(&bus),
        Err(Error::BgVramOutOfRange { addr, width: 1 })
            if addr == VRAM_BASE + 0x1_8000
    ));
}

#[test]
fn forced_blank_is_white_and_disabled_bg2_uses_backdrop() {
    let mut bus = bus();
    bus.write16(PALETTE_BASE, 0x001F).expect("palette");
    bus.write16(DISPCNT_ADDR, 0x0080).expect("blank");
    let image = display::snapshot(&bus).expect("blank");
    assert!(image.pixels.iter().all(|pixel| *pixel == 0x00FF_FFFF));

    bus.write16(DISPCNT_ADDR, 0x0003).expect("bg2 off");
    let image = display::snapshot(&bus).expect("backdrop");
    assert!(image.pixels.iter().all(|pixel| *pixel == 0x00FF_0000));
}

#[test]
fn mode4_selects_palette_index_and_page() {
    let mut bus = bus();
    bus.write16(PALETTE_BASE + 2, 0x03E0).expect("palette 1");
    bus.write16(PALETTE_BASE + 4, 0x7C00).expect("palette 2");
    bus.write16(VRAM_BASE, 0x0001).expect("frame 0");
    bus.write16(VRAM_BASE + 0xA000, 0x0002).expect("frame 1");

    bus.write16(DISPCNT_ADDR, 0x0404).expect("mode 4 page 0");
    let image = display::snapshot(&bus).expect("page 0");
    assert_eq!(image.pixel(0, 0), 0x0000_FF00);
    assert_eq!(image.pixel(1, 0), 0);

    bus.write16(DISPCNT_ADDR, 0x0414).expect("mode 4 page 1");
    let image = display::snapshot(&bus).expect("page 1");
    assert_eq!(image.pixel(0, 0), 0x0000_00FF);
}

#[test]
fn mode5_is_160x128_with_backdrop_outside() {
    let mut bus = bus();
    bus.write16(PALETTE_BASE, 0x03E0).expect("backdrop green");
    bus.write16(VRAM_BASE, 0x001F).expect("red pixel");
    bus.write16(VRAM_BASE + 0xA000, 0x7C00)
        .expect("blue page 1");
    bus.write16(DISPCNT_ADDR, 0x0405).expect("mode 5 page 0");
    let image = display::snapshot(&bus).expect("page 0");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
    assert_eq!(image.pixel(159, 127), 0);
    assert_eq!(image.pixel(160, 0), 0x0000_FF00);
    assert_eq!(image.pixel(0, 128), 0x0000_FF00);

    bus.write16(DISPCNT_ADDR, 0x0415).expect("mode 5 page 1");
    let image = display::snapshot(&bus).expect("page 1");
    assert_eq!(image.pixel(0, 0), 0x0000_00FF);
}

#[test]
fn unsupported_modes_and_layers_are_explicit_errors() {
    let mut bus = bus();
    for dispcnt in [0x0102, 0x0202, 0x000A] {
        bus.write16(DISPCNT_ADDR, dispcnt)
            .expect("unsupported Mode 2 layer/control");
        assert!(matches!(
            display::snapshot(&bus),
            Err(Error::UnsupportedPpuFeature { .. })
        ));
    }
    bus.write16(DISPCNT_ADDR, 0x0006).expect("mode 6");
    assert!(matches!(
        display::snapshot(&bus),
        Err(Error::UnsupportedDisplayMode { mode: 6 })
    ));
    for dispcnt in [0x1404, 0x1414, 0x1405] {
        bus.write16(DISPCNT_ADDR, dispcnt)
            .expect("OBJ remains unsupported in Mode 4/5");
        assert!(matches!(
            display::snapshot(&bus),
            Err(Error::UnsupportedPpuFeature { .. })
        ));
    }
    bus.write16(DISPCNT_ADDR, 0x0503).expect("BG0 layer");
    assert!(matches!(
        display::snapshot(&bus),
        Err(Error::UnsupportedPpuFeature { .. })
    ));
}
