//! End-to-end reference execution of the synthetic guest and the fallback
//! fixture. No commercial ROM is used.

use gba_native::demo;
use gba_native::display::{self, rgb555_to_rgb888};
use gba_native::interpret;
use gba_native::render::{self, Backend};

#[test]
fn demo_guest_draws_the_complete_expected_bitmap() {
    let (cpu, bus) = demo::run_demo_reference().expect("demo reference run");
    assert_eq!(cpu.pc(), 0x0800_0124);
    assert!(!cpu.thumb());
    assert_eq!(cpu.cpsr(), 0x6000_00DF);
    assert_eq!(cpu.reg(0), 0x0601_2C00);
    assert_eq!(cpu.reg(1), 240 * 160);
    assert_eq!(cpu.reg(2), 240 * 160);
    assert_eq!(cpu.reg(3), 240 * 160 - 1);

    let image = display::snapshot(&bus).expect("image");
    let expected = demo::expected_demo_image();
    assert_eq!(image, expected);
    for y in 0..160 {
        for x in 0..240 {
            let index = y * 240 + x;
            assert_eq!(
                image.pixel(x, y),
                rgb555_to_rgb888(index as u16),
                "pixel {x},{y}"
            );
        }
    }

    let scaled = render::scale(&image, 480, 320, Backend::Nearest).expect("scale");
    assert_eq!((scaled.width, scaled.height), (480, 320));
    assert_eq!(scaled.pixel(479, 319), image.pixel(239, 159));
    let ppm = render::to_ppm(&scaled);
    assert!(ppm.starts_with(b"P6\n480 320\n255\n"));
    assert_eq!(ppm.len(), "P6\n480 320\n255\n".len() + 480 * 320 * 3);
}

#[test]
fn fallback_fixture_takes_the_dynamic_bx_path() {
    let (cpu, bus) = demo::run_fallback_reference().expect("fallback reference run");
    assert_eq!(cpu.pc(), 0x0800_0134);
    assert_eq!(cpu.reg(2), 1);
    assert_eq!(cpu.reg(4), 0x55);
    assert_eq!(cpu.flags(), (false, true, true, false));
    let image = display::snapshot(&bus).expect("image");
    assert!(image.pixels.iter().all(|pixel| *pixel == 0));
}

#[test]
fn reference_runner_is_exactly_budgeted() {
    let cart = demo::demo_cartridge();
    let mut cpu = gba_native::cpu::Cpu::direct_entry();
    cpu.set_pc(demo::ENTRY);
    cpu.set_thumb(false);
    let mut bus = gba_native::bus::Bus::new(cart);
    let used = interpret::run_budget(&mut cpu, &mut bus, demo::DEMO_BUDGET).expect("run");
    assert_eq!(used, demo::DEMO_BUDGET);
    assert_eq!(cpu.pc(), 0x0800_0124);
}
