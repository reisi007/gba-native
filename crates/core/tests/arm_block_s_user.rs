//! Independent vectors for ARM block transfers using the User register bank.
use gba_native_core::bus::{Bus, EWRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::{Cpu, FLAG_C, FLAG_N, FLAG_T, Mode, USER_STACK};
use gba_native_core::decode::decode_arm;
use gba_native_core::interpret;
use gba_native_core::ir::{PSR_FIELD_C, PSR_FIELD_F, Psr};

#[allow(clippy::too_many_arguments)]
fn block(cond: u32, p: u32, u: u32, s: u32, w: u32, l: u32, rn: u32, regs: u32) -> u32 {
    (cond << 28)
        | 0x0800_0000
        | (p << 24)
        | (u << 23)
        | (s << 22)
        | (w << 21)
        | (l << 20)
        | (rn << 16)
        | (regs & 0xffff)
}

fn setup(raw: u32) -> (Cpu, Bus) {
    let mut bus = Bus::new(Cartridge::from_bytes(vec![0; 0x200]).unwrap());
    bus.write32(EWRAM_BASE, raw).unwrap();
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(false);
    (cpu, bus)
}

fn mode(cpu: &mut Cpu, mode: Mode) {
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, mode.bits()).unwrap();
}

#[test]
fn fiq_stm_user_bank_stores_user_r8_through_lr() {
    let (mut cpu, mut bus) = setup(block(0xe, 0, 1, 1, 0, 0, 0, 0x7f00));
    let base = EWRAM_BASE + 0x100;
    cpu.set_reg(0, base);
    mode(&mut cpu, Mode::System);
    for (i, value) in (0x81..=0x87).enumerate() {
        cpu.set_reg(8 + i as u8, value);
    }
    mode(&mut cpu, Mode::Fiq);
    for (i, value) in (0xf1..=0xf7).enumerate() {
        cpu.set_reg(8 + i as u8, value);
    }
    interpret::step(&mut cpu, &mut bus).unwrap();
    for i in 0..7 {
        assert_eq!(bus.read32(base + i * 4), Ok(0x81 + i));
    }
    assert_eq!(cpu.reg(8), 0xf1);
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
}

#[test]
fn fiq_ldm_user_bank_changes_user_registers_only() {
    let (mut cpu, mut bus) = setup(block(0xe, 0, 1, 1, 0, 1, 0, 0x7f00));
    let base = EWRAM_BASE + 0x100;
    cpu.set_reg(0, base);
    for i in 0..7 {
        bus.write32(base + i * 4, 0x5100 + i).unwrap();
    }
    mode(&mut cpu, Mode::Fiq);
    for i in 0..7 {
        cpu.set_reg(8 + i as u8, 0xf100 + i);
    }
    let flags = cpu.flags();
    interpret::step(&mut cpu, &mut bus).unwrap();
    assert_eq!(cpu.flags(), flags);
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
    mode(&mut cpu, Mode::System);
    for i in 0..7 {
        assert_eq!(cpu.reg(8 + i as u8), 0x5100 + i);
    }
    mode(&mut cpu, Mode::Fiq);
    for i in 0..7 {
        assert_eq!(cpu.reg(8 + i as u8), 0xf100 + i);
    }
}

#[test]
fn ldm_user_bank_pc_restores_svc_spsr_after_loading_current_bank() {
    let raw = block(0xe, 0, 1, 1, 0, 1, 0, 0xe002);
    let instruction = decode_arm(EWRAM_BASE, raw).unwrap();
    assert!(instruction.is_control_flow());
    assert!(instruction.writes_cpsr_control());
    let (mut cpu, mut bus) = setup(raw);
    let base = EWRAM_BASE + 0x100;
    cpu.set_reg(0, base);
    bus.write32(base, 0xabcdef01).unwrap();
    bus.write32(base + 4, 0x0300_7fd0).unwrap();
    bus.write32(base + 8, 0x1234_5678).unwrap();
    bus.write32(base + 12, 0x0800_1237).unwrap();
    mode(&mut cpu, Mode::Svc);
    cpu.set_reg(13, 0x0300_7fc0);
    cpu.set_reg(14, 0x8765_4321);
    cpu.write_psr(Psr::Spsr, PSR_FIELD_C, Mode::System.bits())
        .unwrap();
    cpu.write_psr(Psr::Spsr, PSR_FIELD_F, FLAG_N | FLAG_C)
        .unwrap();
    interpret::step(&mut cpu, &mut bus).unwrap();
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.flags(), (true, false, true, false));
    assert_eq!(cpu.pc(), 0x0800_1234);
    assert_eq!(cpu.reg(1), 0xabcdef01);
    assert_eq!(cpu.reg(13), USER_STACK, "the loaded SVC SP stayed banked");
    mode(&mut cpu, Mode::Svc);
    assert_eq!(cpu.reg(13), 0x0300_7fd0);
    assert_eq!(cpu.reg(14), 0x1234_5678);
}

#[test]
fn ldm_user_bank_pc_applies_writeback_before_switching_from_svc() {
    let (mut cpu, mut bus) = setup(block(0xe, 0, 1, 1, 1, 1, 13, 0x8000));
    let base = EWRAM_BASE + 0x100;
    mode(&mut cpu, Mode::Svc);
    cpu.set_reg(13, base);
    cpu.write_psr(Psr::Spsr, PSR_FIELD_C, Mode::System.bits())
        .unwrap();
    bus.write32(base, 0x0800_1234).unwrap();
    interpret::step(&mut cpu, &mut bus).unwrap();
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.reg(13), USER_STACK);
    mode(&mut cpu, Mode::Svc);
    assert_eq!(cpu.reg(13), base + 4);
}

#[test]
fn ldm_user_bank_pc_restores_saved_thumb_state_and_alignment() {
    let (mut cpu, mut bus) = setup(0);
    cpu.initialize_bios_stacks();
    cpu.set_pc(EWRAM_BASE + 0x20);
    cpu.set_thumb(true);
    cpu.set_nzcv(true, false, true, false);
    let saved_flags = cpu.flags();
    bus.write16(EWRAM_BASE + 0x20, 0xdf08).unwrap(); // Thumb SWI #Sqrt
    interpret::step(&mut cpu, &mut bus).unwrap();
    assert_eq!(cpu.mode(), Mode::System);
    assert!(cpu.thumb());

    let base = EWRAM_BASE + 0x100;
    bus.write32(EWRAM_BASE + 0x40, block(0xe, 0, 1, 1, 0, 1, 0, 0x8000))
        .unwrap();
    bus.write32(base, 0x0800_1237).unwrap();
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, Mode::Svc.bits() | FLAG_T)
        .unwrap();
    cpu.set_thumb(false);
    cpu.set_reg(0, base);
    cpu.set_pc(EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).unwrap();

    assert_eq!(cpu.mode(), Mode::System);
    assert!(cpu.thumb());
    assert_eq!(cpu.flags(), saved_flags);
    assert_eq!(cpu.pc(), 0x0800_1236);
}

#[test]
fn unavailable_spsr_fails_before_cpu_or_bus_mutation() {
    let (mut cpu, mut bus) = setup(block(0xe, 0, 1, 1, 0, 1, 0, 0x8000));
    let base = EWRAM_BASE + 0x100;
    cpu.set_reg(0, base);
    bus.write32(base, 0x1234).unwrap();
    let before_cpu = cpu.clone();
    let before_bus = bus.state_digest();
    let result = interpret::step(&mut cpu, &mut bus);
    assert!(
        result.is_err(),
        "S=1 LDM PC without SPSR must fail: {result:?}"
    );
    assert_eq!(cpu, before_cpu);
    assert_eq!(bus.state_digest(), before_bus);

    let (mut cpu, mut bus) = setup(block(0xe, 0, 1, 1, 0, 1, 0, 0x8000));
    mode(&mut cpu, Mode::Svc); // direct-entry SPSR_svc contains invalid mode 0
    cpu.set_reg(0, base);
    bus.write32(base, 0x0800_5678).unwrap();
    let before_cpu = cpu.clone();
    let before_bus = bus.state_digest();
    assert!(interpret::step(&mut cpu, &mut bus).is_err());
    assert_eq!(cpu, before_cpu);
    assert_eq!(bus.state_digest(), before_bus);
}

#[test]
fn decode_rejects_user_bank_writeback_and_false_conditional_return_is_inert() {
    assert!(decode_arm(0, block(0xe, 0, 1, 1, 1, 1, 0, 2)).is_err());
    assert!(decode_arm(0, block(0xe, 0, 1, 1, 1, 0, 0, 1 << 8)).is_err());
    let (mut cpu, mut bus) = setup(block(0, 0, 1, 1, 0, 1, 0, 0x8000));
    cpu.set_reg(0, EWRAM_BASE + 0x100);
    cpu.set_nzcv(false, false, false, false);
    let before_cpu = cpu.clone();
    let before_bus = bus.state_digest();
    interpret::step(&mut cpu, &mut bus).unwrap();
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
    assert_eq!(cpu.mode(), before_cpu.mode());
    assert_eq!(bus.state_digest(), before_bus);
}
