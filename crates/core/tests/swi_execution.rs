//! Synthetic guest SWIs with hand-derived exception frames and service results.

use gba_native_core::bios::BiosError;
use gba_native_core::bus::{Bus, DISPCNT_ADDR, EWRAM_BASE, IWRAM_BASE, IWRAM_LEN};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::{Cpu, FLAG_C, FLAG_F, FLAG_I, FLAG_N, FLAG_V, FLAG_Z, Mode};
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::{PSR_FIELD_C, Psr};

const ENTRY: u32 = 0x0800_0000;
const SVC_FRAME: u32 = 0x0300_7FD0;

fn fixture(words: &[u32]) -> Bus {
    let mut rom = vec![0u8; 0x200];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    Bus::new(Cartridge::from_bytes(rom).expect("synthetic cart"))
}

fn thumb_fixture(halfword: u16) -> Bus {
    let mut rom = vec![0u8; 0x200];
    rom[..2].copy_from_slice(&halfword.to_le_bytes());
    Bus::new(Cartridge::from_bytes(rom).expect("synthetic Thumb cart"))
}

fn arm_swi(service: u8) -> u32 {
    0xEF00_0000 | (u32::from(service) << 16)
}

fn set_mode(cpu: &mut Cpu, mode: Mode) {
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, mode.bits())
        .expect("valid mode switch");
}

fn prepared_cpu() -> Cpu {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu
}

#[test]
fn arm_div_enters_svc_frames_and_returns_with_full_cpsr() {
    let mut bus = fixture(&[arm_swi(0x06)]);
    let mut cpu = prepared_cpu();
    cpu.set_reg(0, (-1234i32) as u32);
    cpu.set_reg(1, 10);
    cpu.set_reg(11, 0x1122_3344);
    cpu.set_reg(12, 0x5566_7788);
    cpu.set_reg(14, 0xAABB_CCDD);
    cpu.set_nzcv(true, false, true, true);
    let expected_cpsr = cpu.cpsr();

    assert_eq!(
        interpret::run_budget(&mut cpu, &mut bus, 1).expect("SWI"),
        1
    );

    assert_eq!(cpu.pc(), ENTRY + 4);
    assert_eq!(cpu.cpsr(), expected_cpsr);
    assert_eq!(cpu.mode(), Mode::System);
    assert!(!cpu.thumb());
    assert_eq!(cpu.reg(0), (-123i32) as u32);
    assert_eq!(cpu.reg(1), (-4i32) as u32);
    assert_eq!(cpu.reg(3), 123);
    assert_eq!(cpu.reg(11), 0x1122_3344);
    assert_eq!(cpu.reg(12), 0x5566_7788);
    assert_eq!(cpu.reg(14), 0xAABB_CCDD);
    assert_eq!(bus.read32(SVC_FRAME).expect("saved SPSR"), expected_cpsr);
    assert_eq!(bus.read32(SVC_FRAME + 4).expect("saved r11"), 0x1122_3344);
    assert_eq!(bus.read32(SVC_FRAME + 8).expect("saved r12"), 0x5566_7788);
    assert_eq!(bus.read32(SVC_FRAME + 12).expect("saved LR"), ENTRY + 4);

    set_mode(&mut cpu, Mode::Svc);
    assert_eq!(cpu.reg(13), 0x0300_7FE0);
    assert_eq!(cpu.reg(14), ENTRY + 4);
    assert_eq!(
        cpu.read_psr(Psr::Spsr).expect("restored SPSR"),
        expected_cpsr
    );
}

#[test]
fn thumb_sqrt_uses_low_comment_byte_and_restores_thumb_flags() {
    let mut bus = thumb_fixture(0xDF08);
    let mut cpu = prepared_cpu();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(true);
    cpu.set_reg(0, 4_294_836_225);
    cpu.set_nzcv(true, true, false, true);
    let expected_cpsr = cpu.cpsr();

    interpret::step(&mut cpu, &mut bus).expect("Thumb SWI");

    assert_eq!(cpu.pc(), ENTRY + 2);
    assert_eq!(cpu.cpsr(), expected_cpsr);
    assert_eq!(cpu.mode(), Mode::System);
    assert!(cpu.thumb());
    assert_eq!(cpu.reg(0), 65_535);
    assert_eq!(bus.read32(SVC_FRAME).expect("saved SPSR"), expected_cpsr);
    assert_eq!(bus.read32(SVC_FRAME + 12).expect("saved LR"), ENTRY + 2);
}

#[test]
fn thumb_df06_dispatches_div_and_sqrt_guest_boundaries_are_exact() {
    let mut bus = thumb_fixture(0xDF06);
    let mut cpu = prepared_cpu();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(true);
    cpu.set_reg(0, (-1234i32) as u32);
    cpu.set_reg(1, 10);
    interpret::step(&mut cpu, &mut bus).expect("Thumb Div SWI");
    assert_eq!(cpu.reg(0), (-123i32) as u32);
    assert_eq!(cpu.reg(1), (-4i32) as u32);
    assert_eq!(cpu.reg(3), 123);
    assert_eq!(cpu.pc(), ENTRY + 2);

    let mut sqrt_bus = fixture(&[arm_swi(0x08)]);
    let mut sqrt_cpu = prepared_cpu();
    for (input, expected) in [
        (0, 0),
        (1, 1),
        (2, 1),
        (3, 1),
        (4, 2),
        (u32::MAX - 1, 65_535),
        (u32::MAX, 65_535),
    ] {
        sqrt_cpu.set_pc(ENTRY);
        sqrt_cpu.set_reg(0, input);
        interpret::step(&mut sqrt_cpu, &mut sqrt_bus).expect("guest Sqrt SWI");
        assert_eq!(sqrt_cpu.reg(0), expected, "sqrt({input})");
    }
}

#[test]
fn conditional_false_unsupported_swi_only_advances_by_arm_width() {
    let mut bus = fixture(&[0x0F00_0006]); // EQ SWI; service field is 00h.
    let mut cpu = Cpu::direct_entry();
    let before = cpu.clone();
    let bus_before = bus.state_digest();

    interpret::step(&mut cpu, &mut bus).expect("false condition is a no-op");

    assert_eq!(cpu.pc(), ENTRY + 4);
    assert_eq!(bus.state_digest(), bus_before);
    cpu.set_pc(before.pc());
    assert_eq!(cpu.state_digest(), before.state_digest());
}

#[test]
fn unknown_service_and_precondition_errors_leave_cpu_and_bus_unchanged() {
    let mut bus = fixture(&[0xEF00_0006]); // ARM comment[23:16] is 00h.
    let mut cpu = prepared_cpu();
    let before = (cpu.state_digest(), bus.state_digest());
    assert_eq!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedService(0)))
    );
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);

    let mut bus = fixture(&[arm_swi(0x06)]);
    cpu.set_pc(ENTRY);
    cpu.set_reg(0, 5);
    cpu.set_reg(1, 0);
    let before = (cpu.state_digest(), bus.state_digest());
    assert_eq!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::Bios(BiosError::ZeroDivisor))
    );
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);
}

#[test]
fn user_and_system_origins_return_to_their_exact_modes() {
    for mode in [Mode::User, Mode::System] {
        let mut bus = fixture(&[arm_swi(0x08)]);
        let mut cpu = prepared_cpu();
        set_mode(&mut cpu, mode);
        cpu.set_reg(0, 81);
        let flags = cpu.cpsr() & (FLAG_N | FLAG_Z | FLAG_C | FLAG_V | FLAG_I | FLAG_F);

        interpret::step(&mut cpu, &mut bus).expect("Sqrt SWI");

        assert_eq!(cpu.mode(), mode);
        assert_eq!(cpu.cpsr() & !0x1F, flags);
        assert_eq!(cpu.reg(0), 9);
        assert_eq!(cpu.pc(), ENTRY + 4);
    }
}

#[test]
fn svc_origin_is_rejected_before_mutating_any_state() {
    let mut bus = fixture(&[arm_swi(0x08)]);
    let mut cpu = prepared_cpu();
    set_mode(&mut cpu, Mode::Svc);
    let before = (cpu.state_digest(), bus.state_digest());

    assert_eq!(interpret::step(&mut cpu, &mut bus), Err(Error::SwiNesting));
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);
}

#[test]
fn invalid_svc_stack_is_rejected_without_frame_or_service_effects() {
    let mut bus = fixture(&[arm_swi(0x08)]);
    let mut cpu = prepared_cpu();
    set_mode(&mut cpu, Mode::Svc);
    cpu.set_reg(13, 0x0400_0000);
    set_mode(&mut cpu, Mode::System);
    let before = (cpu.state_digest(), bus.state_digest());

    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::SwiStack { .. })
    ));
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);
}

#[test]
fn all_six_documented_services_are_guest_reachable() {
    for service in [0x06, 0x07, 0x08, 0x0B, 0x0C, 0x01] {
        let mut bus = fixture(&[arm_swi(service)]);
        let mut cpu = prepared_cpu();
        match service {
            0x06 => {
                cpu.set_reg(0, (-1234i32) as u32);
                cpu.set_reg(1, 10);
            }
            0x07 => {
                cpu.set_reg(0, 10);
                cpu.set_reg(1, (-1234i32) as u32);
            }
            0x08 => cpu.set_reg(0, 81),
            0x0B => {
                cpu.set_reg(0, EWRAM_BASE);
                cpu.set_reg(1, EWRAM_BASE + 0x20);
                cpu.set_reg(2, (1 << 26) | 1);
                bus.write32(EWRAM_BASE, 0x1234_5678).expect("source");
            }
            0x0C => {
                cpu.set_reg(0, EWRAM_BASE + 0x40);
                cpu.set_reg(1, EWRAM_BASE + 0x80);
                cpu.set_reg(2, 1);
                for index in 0..8 {
                    bus.write32(EWRAM_BASE + 0x40 + index * 4, 0xA500_0000 + index)
                        .expect("fast source");
                }
            }
            0x01 => {
                cpu.set_reg(0, 0x02);
                bus.write32(IWRAM_BASE, 0xFFFF_FFFF).expect("reset start");
                bus.write32(IWRAM_BASE + IWRAM_LEN as u32 - 512, 0xAABB_CCDD)
                    .expect("reset preserved tail");
            }
            _ => unreachable!(),
        }
        interpret::step(&mut cpu, &mut bus).expect("guest SWI service");
        assert_eq!(cpu.pc(), ENTRY + 4, "service {service:02X}");
        match service {
            0x06 | 0x07 => {
                assert_eq!(cpu.reg(0), (-123i32) as u32);
                assert_eq!(cpu.reg(1), (-4i32) as u32);
                assert_eq!(cpu.reg(3), 123);
            }
            0x08 => assert_eq!(cpu.reg(0), 9),
            0x0B => assert_eq!(bus.read32(EWRAM_BASE + 0x20).expect("copy"), 0x1234_5678),
            0x0C => assert_eq!(
                bus.read32(EWRAM_BASE + 0x9C).expect("rounded end"),
                0xA500_0007
            ),
            0x01 => {
                assert_eq!(bus.read32(IWRAM_BASE).expect("cleared"), 0);
                assert_eq!(
                    bus.read32(IWRAM_BASE + IWRAM_LEN as u32 - 512)
                        .expect("tail"),
                    0xAABB_CCDD
                );
                assert_eq!(bus.read16(DISPCNT_ADDR).expect("forced blank"), 0x0080);
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn invalid_memory_service_request_fails_before_svc_frame() {
    let mut bus = fixture(&[arm_swi(0x0B)]);
    let mut cpu = prepared_cpu();
    cpu.set_reg(0, EWRAM_BASE);
    cpu.set_reg(1, DISPCNT_ADDR);
    cpu.set_reg(2, (1 << 26) | 1);
    let before = (cpu.state_digest(), bus.state_digest());

    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedDestination { .. }))
    ));
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);
}
