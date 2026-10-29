//! Synthetic coverage for the documented GBA IRQ dispatcher frame and return.

use gba_native_core::bus::{Bus, VBLANK_FIRST_LINE, instructions_until_line};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::{Cpu, FLAG_F, FLAG_I, FLAG_V, IRQ_STACK, Mode};
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::{PSR_FIELD_C, Psr};

const ENTRY: u32 = 0x0800_0000;
const HANDLER: u32 = 0x0800_0100;
const HANDLER_POINTER: u32 = 0x0300_7FFC;
const VCOUNT_ADDR: u32 = 0x0400_0006;
const IE_ADDR: u32 = 0x0400_0200;
const IF_ADDR: u32 = 0x0400_0202;
const IME_ADDR: u32 = 0x0400_0208;

fn irq_rom() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    // Thumb B . is the interrupted instruction; the logo area stays zero.
    rom[..2].copy_from_slice(&0xE7FEu16.to_le_bytes());

    // The synthetic ARM handler acknowledges VBlank, clobbers every register
    // saved by the documented dispatcher, then returns through BX LR.
    let handler: [u32; 9] = [
        0xE59F_001C, // LDR r0,[pc,#0x1C] -> IF register literal at HANDLER+0x24
        0xE3A0_1001, // MOV r1,#1
        0xE1C0_10B0, // STRH r1,[r0]
        0xE3A0_0000, // MOV r0,#0
        0xE3A0_1000, // MOV r1,#0
        0xE3A0_2000, // MOV r2,#0
        0xE3A0_3000, // MOV r3,#0
        0xE3A0_C000, // MOV r12,#0
        0xE12F_FF1E, // BX lr
    ];
    for (index, word) in handler.iter().enumerate() {
        let offset = HANDLER as usize - ENTRY as usize + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    let literal = HANDLER as usize - ENTRY as usize + 0x24;
    rom[literal..literal + 4].copy_from_slice(&IF_ADDR.to_le_bytes());
    Cartridge::from_bytes(rom).expect("synthetic IRQ cartridge")
}

fn pending_bus(handler: Option<u32>) -> Bus {
    let mut bus = Bus::new(irq_rom());
    if let Some(handler) = handler {
        bus.write32(HANDLER_POINTER, handler)
            .expect("write handler pointer");
    }
    bus.write16(IE_ADDR, 1).expect("enable VBlank IRQ");
    bus.write16(IME_ADDR, 1).expect("enable IRQ master switch");
    let active = instructions_until_line(VBLANK_FIRST_LINE);
    bus.tick_frames(active);
    assert_eq!(bus.read16(VCOUNT_ADDR).expect("vcount"), 160);
    assert_eq!(bus.read16(IF_ADDR).expect("IF"), 1);
    bus
}

#[test]
fn vblank_request_waits_for_the_documented_line_160() {
    let mut bus = Bus::new(irq_rom());
    bus.write16(IE_ADDR, 1).expect("enable VBlank IRQ");
    bus.write16(IME_ADDR, 1).expect("enable IRQ master switch");
    let boundary = instructions_until_line(VBLANK_FIRST_LINE);

    bus.tick_frames(boundary - 1);
    assert_eq!(bus.read16(VCOUNT_ADDR).expect("vcount"), 159);
    assert_eq!(
        bus.read16(IF_ADDR).expect("IF"),
        0,
        "no request on line 159"
    );
    assert!(!bus.irq_pending(), "nothing to deliver before line 160");

    bus.tick_frame();
    assert_eq!(bus.read16(VCOUNT_ADDR).expect("vcount"), 160);
    assert_eq!(bus.read16(IF_ADDR).expect("IF"), 1);
    assert!(bus.irq_pending(), "the line-160 request is deliverable");
}

fn interrupted_thumb_cpu() -> Cpu {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(ENTRY);
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, cpu.cpsr() & !FLAG_I)
        .expect("unmask IRQ");
    cpu.set_thumb(true);
    cpu.set_nzcv(true, true, false, true);
    for (index, value) in [0x1010_1010, 0x2121_2121, 0x3232_3232, 0x4343_4343]
        .into_iter()
        .enumerate()
    {
        cpu.set_reg(index as u8, value);
    }
    cpu.set_reg(12, 0xCCCC_CCCC);
    cpu.set_reg(14, 0xDEAD_BEEF);
    cpu
}

fn set_mode(cpu: &mut Cpu, mode: Mode) {
    let control = (cpu.cpsr() & !0x1F) | mode.bits();
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, control)
        .expect("switch banked mode");
}

#[test]
fn documented_irq_frame_dispatches_and_restores_interrupted_thumb_context() {
    let mut bus = pending_bus(Some(HANDLER));
    let mut cpu = interrupted_thumb_cpu();
    let interrupted_cpsr = cpu.cpsr();
    let saved_registers = [cpu.reg(0), cpu.reg(1), cpu.reg(2), cpu.reg(3), cpu.reg(12)];

    assert!(interpret::deliver_pending_irq(&mut cpu, &mut bus).expect("IRQ delivery"));
    assert_eq!(cpu.mode(), Mode::Irq);
    assert!(!cpu.thumb(), "user IRQ handlers are entered in ARM state");
    assert_eq!(cpu.pc(), HANDLER);
    assert_eq!(cpu.cpsr() & FLAG_I, FLAG_I);
    assert_eq!(
        cpu.cpsr() & (0xF000_0000 | FLAG_F),
        interrupted_cpsr & (0xF000_0000 | FLAG_F),
        "NZCV and F are preserved on IRQ entry"
    );
    assert_eq!(cpu.cpsr() & FLAG_V, interrupted_cpsr & FLAG_V);
    assert_eq!(cpu.reg(14), 0x0000_0138, "modeled BIOS continuation");
    assert_eq!(cpu.reg(13), IRQ_STACK - 24);
    assert_eq!(cpu.read_psr(Psr::Spsr).expect("SPSR_irq"), interrupted_cpsr);
    for (index, value) in [
        saved_registers[0],
        saved_registers[1],
        saved_registers[2],
        saved_registers[3],
        saved_registers[4],
        ENTRY + 4,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            bus.read32(IRQ_STACK - 24 + index as u32 * 4)
                .expect("documented dispatcher frame"),
            value,
            "frame word {index}"
        );
    }

    for _ in 0..9 {
        interpret::step(&mut cpu, &mut bus).expect("guest IRQ handler instruction");
    }
    assert_eq!(
        cpu.pc(),
        0x0000_0138,
        "BX LR reaches the modeled continuation"
    );
    assert_eq!(bus.read16(IF_ADDR).expect("acknowledged IF"), 0);
    assert!(matches!(
        bus.fetch32(0x0000_0138),
        Err(Error::NonExecutable { addr: 0x0000_0138 })
    ));

    // One modeled BIOS epilogue operation restores the frame and performs the
    // documented exception-return semantics; it does not fetch BIOS bytes.
    interpret::step(&mut cpu, &mut bus).expect("modeled IRQ dispatcher return");
    assert_eq!(cpu.mode(), Mode::System);
    assert!(cpu.thumb());
    assert_eq!(cpu.cpsr(), interrupted_cpsr);
    assert_eq!(cpu.pc(), ENTRY, "resume the interrupted Thumb instruction");
    assert_eq!(cpu.reg(13), 0x0300_7F00);
    assert_eq!(
        cpu.reg(14),
        0xDEAD_BEEF,
        "the interrupted LR bank is untouched"
    );
    for (index, value) in saved_registers[..4].iter().enumerate() {
        assert_eq!(cpu.reg(index as u8), *value, "restored r{index}");
    }
    assert_eq!(cpu.reg(12), saved_registers[4]);
    assert!(!bus.irq_pending());

    set_mode(&mut cpu, Mode::Irq);
    assert_eq!(cpu.reg(13), IRQ_STACK, "IRQ stack pointer is restored");
    set_mode(&mut cpu, Mode::System);
    assert_eq!(cpu.cpsr(), interrupted_cpsr);

    interpret::step(&mut cpu, &mut bus).expect("resume the same Thumb instruction");
    assert_eq!(cpu.pc(), ENTRY);
    assert!(cpu.thumb());
}

#[test]
fn missing_handler_and_masked_or_nested_irq_stay_pending() {
    let mut bus = pending_bus(None);
    let mut cpu = interrupted_thumb_cpu();
    let before = (cpu.state_digest(), bus.state_digest());
    assert!(!interpret::deliver_pending_irq(&mut cpu, &mut bus).expect("no handler"));
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);
    assert!(bus.irq_pending());

    bus = pending_bus(Some(HANDLER));
    cpu = interrupted_thumb_cpu();
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, cpu.cpsr() | FLAG_I)
        .expect("mask IRQ");
    let before = (cpu.state_digest(), bus.state_digest());
    assert!(!interpret::deliver_pending_irq(&mut cpu, &mut bus).expect("masked IRQ"));
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);
    assert!(bus.irq_pending());

    cpu = interrupted_thumb_cpu();
    set_mode(&mut cpu, Mode::Irq);
    let before = (cpu.state_digest(), bus.state_digest());
    assert!(!interpret::deliver_pending_irq(&mut cpu, &mut bus).expect("no IRQ nesting"));
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);
    assert!(bus.irq_pending());
}

#[test]
fn invalid_handler_and_irq_stack_fail_before_cpu_or_bus_mutation() {
    for handler in [
        HANDLER + 1,
        0x0400_0000, // I/O is not executable
        0x0000_0020, // BIOS is not executable
        0x0100_0000, // unmapped
        0x0800_0200, // one byte past this synthetic ROM
    ] {
        let mut bus = pending_bus(Some(handler));
        let mut cpu = interrupted_thumb_cpu();
        let before = (cpu.state_digest(), bus.state_digest());
        assert!(matches!(
            interpret::deliver_pending_irq(&mut cpu, &mut bus),
            Err(Error::IrqHandler { addr, .. }) if addr == handler
        ));
        assert_eq!((cpu.state_digest(), bus.state_digest()), before);
    }

    for stack in [0x0300_7FA1, 0x0300_0010] {
        let mut bus = pending_bus(Some(HANDLER));
        let mut cpu = interrupted_thumb_cpu();
        set_mode(&mut cpu, Mode::Irq);
        cpu.set_reg(13, stack);
        set_mode(&mut cpu, Mode::System);
        let before = (cpu.state_digest(), bus.state_digest());
        assert!(matches!(
            interpret::deliver_pending_irq(&mut cpu, &mut bus),
            Err(Error::IrqStack { sp, .. }) if sp == stack
        ));
        assert_eq!((cpu.state_digest(), bus.state_digest()), before);
    }
}

#[test]
fn irq_continuation_is_not_fabricated_outside_irq_mode() {
    let mut bus = Bus::new(irq_rom());
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(0x0000_0138);
    assert_eq!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::NonExecutable { addr: 0x0000_0138 })
    );

    set_mode(&mut cpu, Mode::Irq);
    cpu.set_thumb(true);
    assert_eq!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::NonExecutable { addr: 0x0000_0138 })
    );
}

#[test]
fn irq_dispatch_preserves_fiq_banked_r12_and_saves_the_shared_r12() {
    const FIQ_R12: u32 = 0xF1F1_F1F1;
    const SHARED_R12: u32 = 0xCCCC_CCCC;
    let mut bus = pending_bus(Some(HANDLER));
    let mut cpu = interrupted_thumb_cpu();
    set_mode(&mut cpu, Mode::Fiq);
    cpu.set_reg(12, FIQ_R12);
    cpu.set_thumb(false);

    assert!(interpret::deliver_pending_irq(&mut cpu, &mut bus).expect("IRQ from FIQ"));
    assert_eq!(cpu.reg(12), SHARED_R12, "IRQ observes the shared R12 bank");
    assert_eq!(
        bus.read32(IRQ_STACK - 8).expect("saved shared R12"),
        SHARED_R12
    );
    for _ in 0..9 {
        interpret::step(&mut cpu, &mut bus).expect("guest IRQ handler instruction");
    }
    interpret::step(&mut cpu, &mut bus).expect("modeled IRQ return");
    assert_eq!(cpu.mode(), Mode::Fiq);
    assert_eq!(cpu.reg(12), FIQ_R12, "FIQ's bank is not overwritten");

    set_mode(&mut cpu, Mode::System);
    assert_eq!(
        cpu.reg(12),
        SHARED_R12,
        "shared R12 was restored separately"
    );
}
