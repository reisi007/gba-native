//! Independent pixel oracles for bounded semi-transparent OBJ composition.

use gba_native_core::bus::{
    BG0CNT_ADDR, BG1CNT_ADDR, BLDALPHA_ADDR, BLDCNT_ADDR, BLDY_ADDR, Bus, DISPCNT_ADDR, OAM_BASE,
    PALETTE_BASE, VRAM_BASE, WIN0H_ADDR, WIN0V_ADDR, WININ_ADDR,
};
use gba_native_core::cart::Cartridge;
use gba_native_core::display;

const OBJ_ON: u16 = 0x1040;
const BG0_ON: u16 = 0x0100;
const BG1_ON: u16 = 0x0200;

fn bus() -> Bus {
    Bus::new(Cartridge::from_bytes(vec![0; 0x200]).expect("synthetic cart"))
}

fn palette(bus: &mut Bus, index: usize, color: u16) {
    bus.write16(PALETTE_BASE + (index * 2) as u32, color)
        .expect("palette");
}

fn tile4(bus: &mut Bus, offset: usize, value: u8) {
    for byte in 0..32 {
        bus.write8(VRAM_BASE + offset as u32 + byte, value)
            .expect("tile byte");
    }
}

fn semi_obj(bus: &mut Bus, tile: u16) {
    bus.write16(OAM_BASE, 0x0400).expect("semi OBJ Attr0");
    bus.write16(OAM_BASE + 2, 0).expect("OBJ Attr1");
    bus.write16(OAM_BASE + 4, tile).expect("OBJ Attr2");
}

#[test]
fn window_effect_bit_gates_semi_obj_alpha_composition() {
    let mut bus = bus();
    palette(&mut bus, 0, 0x03E0); // green backdrop
    palette(&mut bus, 0x111, 0x001F); // red OBJ palette bank 1
    tile4(&mut bus, 0x1_0000, 0x11);
    semi_obj(&mut bus, 0x1000); // palette bank 1
    bus.write16(WIN0H_ADDR, 0x00F0).expect("full-width window");
    bus.write16(WIN0V_ADDR, 0x00A0).expect("full-height window");
    bus.write16(WININ_ADDR, 0x0010)
        .expect("OBJ visible, effects disabled");
    bus.write16(BLDCNT_ADDR, 0x2050)
        .expect("OBJ first/backdrop second");
    bus.write16(BLDALPHA_ADDR, 0x0808)
        .expect("equal alpha coefficients");
    bus.write16(DISPCNT_ADDR, OBJ_ON | 0x2000)
        .expect("OBJ and Window 0");

    let disabled = display::snapshot(&bus).expect("effects-disabled snapshot");
    assert_eq!(disabled.pixel(0, 0), 0x00FF_0000);

    bus.write16(WININ_ADDR, 0x0030)
        .expect("enable window effects");
    let enabled = display::snapshot(&bus).expect("effects-enabled snapshot");
    assert_eq!(enabled.pixel(0, 0), 0x007F_7F00);
}

#[test]
fn semi_obj_alpha_overrides_every_bldcnt_mode_when_the_next_target_qualifies() {
    for mode in 0..=3u16 {
        let mut bus = bus();
        palette(&mut bus, 0, 0);
        palette(&mut bus, 1, 0x03E0); // green immediate BG0
        palette(&mut bus, 0x111, 0x001F); // red semi-transparent OBJ
        tile4(&mut bus, 32, 0x11); // BG0 tile 1
        tile4(&mut bus, 0x1_0000, 0x11); // OBJ tile 0
        bus.write16(VRAM_BASE, 1).expect("BG0 map entry");
        bus.write16(BG0CNT_ADDR, 0).expect("BG0 priority");
        semi_obj(&mut bus, 0x1000);
        // Select BG0 only as a second target; omit OBJ from the first-target
        // mask. BLDY would fully brighten in modes 2/3 if alpha did not win.
        bus.write16(BLDCNT_ADDR, (mode << 6) | 0x0100)
            .expect("mode-specific BLDCNT");
        bus.write16(BLDALPHA_ADDR, 0x0808)
            .expect("equal alpha coefficients");
        bus.write16(BLDY_ADDR, 16).expect("full brightness amount");
        bus.write16(DISPCNT_ADDR, OBJ_ON | BG0_ON)
            .expect("OBJ and BG0 enable");

        let image = display::snapshot(&bus).expect("semi-OBJ alpha snapshot");
        assert_eq!(
            image.pixel(0, 0),
            0x007F_7F00,
            "semi-OBJ alpha must override BLDCNT mode {mode}"
        );
    }
}

#[test]
fn immediate_non_target_blocks_farther_alpha_target_and_obj_priority_is_preserved() {
    let mut bus = bus();
    palette(&mut bus, 0, 0);
    palette(&mut bus, 1, 0x03E0); // immediate BG0 blocker
    palette(&mut bus, 2, 0x7C00); // farther BG1 second target
    palette(&mut bus, 0x111, 0x001F); // semi OBJ red
    tile4(&mut bus, 0x1_0000, 0x11);
    tile4(&mut bus, 32, 0x11);
    tile4(&mut bus, 2 * 32, 0x22);
    bus.write16(VRAM_BASE, 1).expect("BG0 map");
    bus.write16(VRAM_BASE + 0x800, 2).expect("BG1 map");
    bus.write16(BG0CNT_ADDR, 0x0001)
        .expect("BG0 lower than OBJ");
    bus.write16(BG1CNT_ADDR, 0x0202).expect("BG1 behind BG0");
    semi_obj(&mut bus, 0x1000);
    bus.write16(BLDCNT_ADDR, 0x0240)
        .expect("BG1 only second target");
    bus.write16(BLDALPHA_ADDR, 0x0808)
        .expect("equal alpha coefficients");
    bus.write16(DISPCNT_ADDR, OBJ_ON | BG0_ON | BG1_ON)
        .expect("OBJ and two BGs");

    let image = display::snapshot(&bus).expect("non-adjacent target snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_0000);
}

#[test]
fn transparent_semi_obj_and_ordinary_obj_keep_normal_visibility_and_priority() {
    let mut bus = bus();
    palette(&mut bus, 0, 0x03E0); // green backdrop
    palette(&mut bus, 0x111, 0x001F); // red OBJ palette bank 1
    palette(&mut bus, 0x112, 0x7C00); // blue OBJ palette bank 1
    tile4(&mut bus, 0x1_0000, 0x00); // transparent texel
    tile4(&mut bus, 0x1_0020, 0x22);
    tile4(&mut bus, 0x1_0040, 0x22);
    bus.write16(BLDCNT_ADDR, 0x2040)
        .expect("backdrop second target");
    bus.write16(BLDALPHA_ADDR, 0x0808)
        .expect("equal alpha coefficients");
    bus.write16(DISPCNT_ADDR, OBJ_ON).expect("OBJ enable");

    semi_obj(&mut bus, 0x1000);
    let transparent = display::snapshot(&bus).expect("transparent OBJ snapshot");
    assert_eq!(transparent.pixel(0, 0), 0x0000_FF00);

    // Ordinary OBJ mode does not acquire forced alpha from BLDCNT mode 1.
    bus.write16(OAM_BASE, 0x0000).expect("ordinary OBJ mode");
    bus.write16(OAM_BASE + 4, 0x1000)
        .expect("red OBJ tile/palette");
    bus.write16(VRAM_BASE + 0x1_0000, 0x1111)
        .expect("opaque red texel");
    let ordinary = display::snapshot(&bus).expect("ordinary OBJ snapshot");
    assert_eq!(ordinary.pixel(0, 0), 0x00FF_0000);

    // A later OAM entry with higher priority wins over an earlier lower-priority OBJ.
    bus.write16(OAM_BASE + 4, 0x1400).expect("red priority 1");
    bus.write16(OAM_BASE + 8, 0x0000).expect("second OBJ Attr0");
    bus.write16(OAM_BASE + 10, 0).expect("second OBJ Attr1");
    bus.write16(OAM_BASE + 12, 0x1001).expect("blue priority 0");
    let priority = display::snapshot(&bus).expect("OBJ priority snapshot");
    assert_eq!(priority.pixel(0, 0), 0x0000_00FF);
}
