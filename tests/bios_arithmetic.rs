//! Independent golden tests for the BIOS HLE arithmetic services.
//!
//! Expected values come from the documented interface and hand-derived
//! arithmetic, not from the implementation under test.

use gba_native::bios::{self, BiosError};
use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::error::Error;

fn fresh() -> (Cpu, Bus) {
    let cart = Cartridge::from_bytes(vec![0u8; 0x200]).expect("test cart");
    (Cpu::direct_entry(), Bus::new(cart))
}

fn signed(value: i32) -> u32 {
    value as u32
}

#[test]
fn div_matches_documented_example_and_preserves_other_state() {
    let (mut cpu, mut bus) = fresh();
    cpu.set_reg(0, signed(-1234));
    cpu.set_reg(1, 10);
    cpu.set_reg(2, 0xDEAD_BEEF);
    cpu.set_reg(4, 0x1122_3344);
    cpu.set_reg(12, 0x5566_7788);
    let cpsr = cpu.cpsr();
    let pc = cpu.pc();
    let bus_digest = bus.state_digest();

    bios::call(bios::SERVICE_DIV, &mut cpu, &mut bus).expect("div");

    assert_eq!(cpu.reg(0), signed(-123));
    assert_eq!(cpu.reg(1), signed(-4));
    assert_eq!(cpu.reg(3), 123);
    assert_eq!(cpu.reg(2), 0xDEAD_BEEF);
    assert_eq!(cpu.reg(4), 0x1122_3344);
    assert_eq!(cpu.reg(12), 0x5566_7788);
    assert_eq!(cpu.cpsr(), cpsr);
    assert_eq!(cpu.pc(), pc);
    assert_eq!(bus.state_digest(), bus_digest);
}

#[test]
fn div_arm_reverses_the_documented_parameter_order() {
    let (mut cpu, mut bus) = fresh();
    cpu.set_reg(0, 10); // denominator
    cpu.set_reg(1, signed(-1234)); // numerator
    cpu.set_reg(2, 0xA5A5_5A5A);

    bios::call(bios::SERVICE_DIV_ARM, &mut cpu, &mut bus).expect("divarm");

    assert_eq!(cpu.reg(0), signed(-123));
    assert_eq!(cpu.reg(1), signed(-4));
    assert_eq!(cpu.reg(3), 123);
    assert_eq!(cpu.reg(2), 0xA5A5_5A5A);
}

#[test]
fn div_signed_matrix_truncates_toward_zero() {
    let cases = [
        (7, 3, 2, 1, 2),
        (-7, 3, -2, -1, 2),
        (7, -3, -2, 1, 2),
        (-7, -3, 2, -1, 2),
        (1, 2, 0, 1, 0),
        (-1, 2, 0, -1, 0),
        (-1, -2, 0, -1, 0),
        (2, 1, 2, 0, 2),
        (i32::MAX, 1, i32::MAX, 0, i32::MAX as u32),
        (i32::MAX, -1, -i32::MAX, 0, i32::MAX as u32),
        (i32::MIN, 1, i32::MIN, 0, 2_147_483_648),
        (0, 5, 0, 0, 0),
        (0, -5, 0, 0, 0),
    ];
    let mut cpu = Cpu::direct_entry();
    let mut bus = fresh().1;
    for (numerator, denominator, quotient, remainder, magnitude) in cases {
        cpu.set_reg(0, signed(numerator));
        cpu.set_reg(1, signed(denominator));
        cpu.set_reg(2, 0xFFFF_FFFF);
        cpu.set_reg(3, 0xFFFF_FFFF);
        bios::call(bios::SERVICE_DIV, &mut cpu, &mut bus)
            .unwrap_or_else(|error| panic!("{numerator}/{denominator}: {error}"));
        assert_eq!(cpu.reg(0), signed(quotient), "{numerator}/{denominator}");
        assert_eq!(cpu.reg(1), signed(remainder), "{numerator}/{denominator}");
        assert_eq!(cpu.reg(3), magnitude, "{numerator}/{denominator}");
        assert_eq!(cpu.reg(2), 0xFFFF_FFFF);
    }
}

#[test]
fn division_errors_are_named_and_leave_state_untouched() {
    let (mut cpu, mut bus) = fresh();
    let before = (cpu.state_digest(), bus.state_digest());
    cpu.set_reg(0, 5);
    cpu.set_reg(1, 0);
    cpu.set_reg(2, 0x1234_5678);
    cpu.set_reg(3, 0x9ABC_DEF0);
    let armed = (cpu.state_digest(), bus.state_digest());

    assert_eq!(
        bios::call(bios::SERVICE_DIV, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::ZeroDivisor))
    );
    assert_eq!((cpu.state_digest(), bus.state_digest()), armed);

    // DivArm takes the denominator from r0.
    cpu.set_reg(0, 0);
    cpu.set_reg(1, 5);
    let armed = (cpu.state_digest(), bus.state_digest());
    assert_eq!(
        bios::call(bios::SERVICE_DIV_ARM, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::ZeroDivisor))
    );
    assert_eq!((cpu.state_digest(), bus.state_digest()), armed);

    // The one signed quotient outside the i32 range is an explicit error.
    cpu.set_reg(0, signed(i32::MIN));
    cpu.set_reg(1, signed(-1));
    let armed = (cpu.state_digest(), bus.state_digest());
    assert_eq!(
        bios::call(bios::SERVICE_DIV, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::DivisionOverflow))
    );
    assert_eq!((cpu.state_digest(), bus.state_digest()), armed);

    cpu.set_reg(0, signed(-1));
    cpu.set_reg(1, signed(i32::MIN));
    let armed = (cpu.state_digest(), bus.state_digest());
    assert_eq!(
        bios::call(bios::SERVICE_DIV_ARM, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::DivisionOverflow))
    );
    assert_eq!((cpu.state_digest(), bus.state_digest()), armed);

    // The untouched baseline, for contrast with the snapshots above.
    assert_ne!(before, (cpu.state_digest(), bus.state_digest()));
}

#[test]
fn sqrt_returns_documented_integer_floor() {
    let cases = [
        (0u32, 0u32),
        (1, 1),
        (2, 1),
        (3, 1),
        (4, 2),
        (15, 3),
        (16, 4),
        (17, 4),
        (65_534, 255),
        (65_535, 255),
        (65_536, 256),
        (4_294_836_224, 65_534),
        (4_294_836_225, 65_535),
        (4_294_836_226, 65_535),
        (u32::MAX, 65_535),
    ];
    let (mut cpu, mut bus) = fresh();
    cpu.set_reg(1, 0x1111_1111);
    cpu.set_reg(2, 0x2222_2222);
    cpu.set_reg(3, 0x3333_3333);
    cpu.set_reg(4, 0x4444_4444);
    let cpsr = cpu.cpsr();
    let pc = cpu.pc();
    let bus_digest = bus.state_digest();
    for (value, expected) in cases {
        cpu.set_reg(0, value);
        bios::call(bios::SERVICE_SQRT, &mut cpu, &mut bus).expect("sqrt");
        assert_eq!(cpu.reg(0), expected, "sqrt({value})");
        assert_eq!(cpu.reg(1), 0x1111_1111);
        assert_eq!(cpu.reg(2), 0x2222_2222);
        assert_eq!(cpu.reg(3), 0x3333_3333);
        assert_eq!(cpu.reg(4), 0x4444_4444);
        assert_eq!(cpu.cpsr(), cpsr);
        assert_eq!(cpu.pc(), pc);
    }
    assert_eq!(bus.state_digest(), bus_digest);
}

#[test]
fn sqrt_property_root_squared_le_value_below_next_square() {
    let (mut cpu, mut bus) = fresh();
    for value in (0..=131_072u32).chain([4_294_836_224, 4_294_836_225, 4_294_836_226, u32::MAX]) {
        cpu.set_reg(0, value);
        bios::call(bios::SERVICE_SQRT, &mut cpu, &mut bus).expect("sqrt");
        let root = u64::from(cpu.reg(0));
        let wide = u64::from(value);
        assert!(root * root <= wide, "sqrt({value})={root} is too large");
        assert!(
            (root + 1) * (root + 1) > wide,
            "sqrt({value})={root} is too small"
        );
    }
}
