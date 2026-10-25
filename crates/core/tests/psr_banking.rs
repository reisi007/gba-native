//! Independent ARM7TDMI banked-register and SPSR tests.
//!
//! The expected sharing rules are written from the documented register table:
//! r0-r7 shared, r8-r12 shared except in FIQ, r13/r14 banked per exception mode
//! with USER/SYSTEM sharing, and one separate SPSR per exception mode.

use gba_native_core::cpu::{Cpu, Mode, USER_STACK};
use gba_native_core::error::Error;
use gba_native_core::ir::{PSR_FIELD_C, PSR_FIELD_F, Psr};

fn switch(cpu: &mut Cpu, mode: Mode) {
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, mode.bits())
        .expect("valid privileged mode switch");
    assert_eq!(cpu.mode(), mode);
}

fn set_bank(cpu: &mut Cpu, base: u32) {
    for index in 8..=12u8 {
        cpu.set_reg(index, base + index as u32);
    }
    cpu.set_reg(13, base + 13);
    cpu.set_reg(14, base + 14);
}

fn assert_bank(cpu: &Cpu, base: u32) {
    for index in 8..=12u8 {
        assert_eq!(cpu.reg(index), base + index as u32, "r{index}");
    }
    assert_eq!(cpu.reg(13), base + 13);
    assert_eq!(cpu.reg(14), base + 14);
}

fn set_sp_lr(cpu: &mut Cpu, base: u32) {
    cpu.set_reg(13, base + 13);
    cpu.set_reg(14, base + 14);
}

#[test]
fn all_exception_modes_have_independent_sp_and_lr_banks() {
    let mut cpu = Cpu::direct_entry();
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.reg(13), USER_STACK);
    cpu.set_reg(0, 0x1111_1111);
    set_bank(&mut cpu, 0x8000_0000);
    let system_hi = [
        cpu.reg(8),
        cpu.reg(9),
        cpu.reg(10),
        cpu.reg(11),
        cpu.reg(12),
    ];

    switch(&mut cpu, Mode::Fiq);
    // FIQ has its own high registers and its own SP/LR bank.
    assert_eq!([cpu.reg(8), cpu.reg(9)], [0, 0]);
    assert_eq!((cpu.reg(13), cpu.reg(14)), (0, 0));
    set_bank(&mut cpu, 0xF000_0000);

    for (index, mode) in [Mode::Irq, Mode::Svc, Mode::Abt, Mode::Und]
        .into_iter()
        .enumerate()
    {
        switch(&mut cpu, mode);
        assert_eq!(
            [
                cpu.reg(8),
                cpu.reg(9),
                cpu.reg(10),
                cpu.reg(11),
                cpu.reg(12)
            ],
            system_hi,
            "{:?} shares r8-r12 outside FIQ",
            mode
        );
        assert_eq!((cpu.reg(13), cpu.reg(14)), (0, 0), "{mode:?} fresh bank");
        assert_eq!(cpu.reg(0), 0x1111_1111, "r0-r7 are shared");
        set_sp_lr(&mut cpu, 0x8100_0000 + (index as u32) * 0x0100_0000);
    }

    switch(&mut cpu, Mode::System);
    assert_bank(&cpu, 0x8000_0000);
    assert_eq!(cpu.reg(0), 0x1111_1111);

    switch(&mut cpu, Mode::Fiq);
    assert_bank(&cpu, 0xF000_0000);

    for (index, mode) in [Mode::Irq, Mode::Svc, Mode::Abt, Mode::Und]
        .into_iter()
        .enumerate()
    {
        switch(&mut cpu, mode);
        let base = 0x8100_0000 + (index as u32) * 0x0100_0000;
        assert_eq!((cpu.reg(13), cpu.reg(14)), (base + 13, base + 14));
        assert_eq!(
            [
                cpu.reg(8),
                cpu.reg(9),
                cpu.reg(10),
                cpu.reg(11),
                cpu.reg(12)
            ],
            system_hi,
            "{mode:?} still shares r8-r12"
        );
    }
}

#[test]
fn non_fiq_modes_share_high_registers_with_user_and_system() {
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(8, 0xAAAA_AAAA);
    cpu.set_reg(9, 0xAAAA_AAA9);

    switch(&mut cpu, Mode::Fiq);
    assert_eq!((cpu.reg(8), cpu.reg(9)), (0, 0));
    cpu.set_reg(8, 0xBBBB_BBBB);
    cpu.set_reg(9, 0xBBBB_BBB9);

    switch(&mut cpu, Mode::Irq);
    assert_eq!((cpu.reg(8), cpu.reg(9)), (0xAAAA_AAAA, 0xAAAA_AAA9));
    cpu.set_reg(8, 0xCCCC_CCCC);
    cpu.set_reg(9, 0xCCCC_CCC9);

    switch(&mut cpu, Mode::Fiq);
    assert_eq!((cpu.reg(8), cpu.reg(9)), (0xBBBB_BBBB, 0xBBBB_BBB9));

    switch(&mut cpu, Mode::System);
    assert_eq!((cpu.reg(8), cpu.reg(9)), (0xCCCC_CCCC, 0xCCCC_CCC9));

    switch(&mut cpu, Mode::Svc);
    assert_eq!((cpu.reg(8), cpu.reg(9)), (0xCCCC_CCCC, 0xCCCC_CCC9));
}

#[test]
fn user_and_system_share_the_same_sp_lr_bank() {
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(13, 0x0300_7E00);
    cpu.set_reg(14, 0x5000_000E);
    switch(&mut cpu, Mode::Irq);
    cpu.set_reg(13, 0x0300_7000);
    cpu.set_reg(14, 0x6000_000E);

    switch(&mut cpu, Mode::User);
    assert_eq!((cpu.reg(13), cpu.reg(14)), (0x0300_7E00, 0x5000_000E));
    cpu.set_reg(13, 0x0300_7D00);

    // USER cannot switch itself back, so inspect the shared bank from a fresh
    // CPU and verify the distinct IRQ bank remains separate.
    assert_eq!(cpu.mode(), Mode::User);
    let mut other = Cpu::direct_entry();
    other.set_reg(13, 0x0300_7D00);
    switch(&mut other, Mode::System);
    assert_eq!(other.reg(13), 0x0300_7D00);
    switch(&mut other, Mode::Irq);
    assert_ne!(other.reg(13), 0x0300_7D00);
}

#[test]
fn spsr_is_separate_per_exception_mode_and_does_not_change_cpsr() {
    let mut cpu = Cpu::direct_entry();
    let modes = [Mode::Fiq, Mode::Irq, Mode::Svc, Mode::Abt, Mode::Und];
    for (index, mode) in modes.into_iter().enumerate() {
        switch(&mut cpu, mode);
        let cpsr_before = cpu.cpsr();
        let value = ((index as u32 + 1) << 28) | 0x0000_0010;
        cpu.write_psr(Psr::Spsr, PSR_FIELD_F | PSR_FIELD_C, value)
            .expect("write SPSR");
        assert_eq!(cpu.read_psr(Psr::Spsr), Ok(value), "{mode:?}");
        assert_eq!(cpu.cpsr(), cpsr_before, "SPSR write must not touch CPSR");
    }

    for (index, mode) in modes.into_iter().enumerate() {
        switch(&mut cpu, mode);
        let value = ((index as u32 + 1) << 28) | 0x0000_0010;
        assert_eq!(cpu.read_psr(Psr::Spsr), Ok(value), "{mode:?} isolated");
    }

    // SPSR field writes merge like CPSR field writes, without switching mode.
    switch(&mut cpu, Mode::Irq);
    cpu.write_psr(Psr::Spsr, PSR_FIELD_F, 0xF000_0000)
        .expect("SPSR flags");
    assert_eq!(cpu.read_psr(Psr::Spsr), Ok(0xF000_0010));
    cpu.write_psr(Psr::Spsr, PSR_FIELD_C, 0x0000_0013)
        .expect("SPSR control");
    assert_eq!(cpu.read_psr(Psr::Spsr), Ok(0xF000_0013));
    assert_eq!(cpu.mode(), Mode::Irq);
    cpu.write_psr(Psr::Spsr, PSR_FIELD_F, 0)
        .expect("clear SPSR flags");
    assert_eq!(cpu.read_psr(Psr::Spsr), Ok(0x0000_0013));

    // SPSR reserved bits 27-24 and 15-8 are preserved like CPSR's.
    cpu.write_psr(Psr::Spsr, 0b1111, 0xFFFF_FFDF)
        .expect("SPSR all fields");
    assert_eq!(cpu.read_psr(Psr::Spsr), Ok(0xF000_00DF));
}

#[test]
fn psr_field_masks_are_the_documented_arm7tdmi_bytes() {
    let mut cpu = Cpu::direct_entry();
    // Direct entry is SYSTEM 0xDF; reserved CPSR bits 27-24 and 15-8 stay zero.
    cpu.set_nzcv(true, true, true, true);
    assert_eq!(cpu.cpsr(), 0xF000_00DF);
    // T is already 0; reserved bits in the value must not reach CPSR.
    cpu.write_psr(Psr::Cpsr, 0b1111, 0xFFFF_FFDF)
        .expect("all fields");
    assert_eq!(cpu.cpsr(), 0xF000_00DF);
    assert_eq!(cpu.mode(), Mode::System);

    // Reserved s/x fields alone cannot mutate anything.
    let before = cpu.clone();
    cpu.write_psr(Psr::Cpsr, 0b0110, 0xFFFF_FFFF)
        .expect("reserved fields ignored");
    assert_eq!(cpu, before);
}

#[test]
fn spsr_restriction_errors_leave_state_untouched() {
    let mut cpu = Cpu::direct_entry();
    let before = cpu.clone();
    assert!(matches!(cpu.read_psr(Psr::Spsr), Err(Error::Psr(_))));
    assert!(matches!(
        cpu.write_psr(Psr::Spsr, PSR_FIELD_F, 0),
        Err(Error::Psr(_))
    ));
    assert_eq!(cpu, before);
}
