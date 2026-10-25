//! SWI 06h/07h/08h: signed division and unsigned integer square root.
//!
//! Derived from the public GBATEK service descriptions. The division result is
//! computed completely before any register is written, so a zero divisor or the
//! unrepresentable `i32::MIN / -1` case leaves the register file untouched.

use crate::bios::BiosError;
use crate::cpu::Cpu;
use crate::error::{Error, Result};

/// SWI 06h `Div`: numerator r0, denominator r1.
pub(super) fn div(cpu: &mut Cpu) -> Result<()> {
    divide(cpu, cpu.reg(0), cpu.reg(1))
}

/// SWI 07h `DivArm`: numerator r1, denominator r0, same results as `Div`.
pub(super) fn div_arm(cpu: &mut Cpu) -> Result<()> {
    divide(cpu, cpu.reg(1), cpu.reg(0))
}

fn divide(cpu: &mut Cpu, numerator: u32, denominator: u32) -> Result<()> {
    validate_div(numerator, denominator)?;
    let numerator = numerator as i32;
    let denominator = denominator as i32;
    // ARM's documented signed divide truncates toward zero and the remainder
    // follows the dividend's sign. Rust's checked i32 operators use the same
    // convention and report the one unrepresentable case instead of panicking.
    let quotient = numerator
        .checked_div(denominator)
        .expect("validated division");
    let remainder = numerator
        .checked_rem(denominator)
        .expect("validated remainder");
    cpu.set_reg(0, quotient as u32);
    cpu.set_reg(1, remainder as u32);
    // r3 is the unsigned magnitude of the quotient, including for negatives.
    cpu.set_reg(3, quotient.unsigned_abs());
    Ok(())
}

pub(super) fn validate_div(numerator: u32, denominator: u32) -> Result<()> {
    let numerator = numerator as i32;
    let denominator = denominator as i32;
    if denominator == 0 {
        return Err(Error::Bios(BiosError::ZeroDivisor));
    }
    numerator
        .checked_div(denominator)
        .ok_or(Error::Bios(BiosError::DivisionOverflow))?;
    numerator
        .checked_rem(denominator)
        .ok_or(Error::Bios(BiosError::DivisionOverflow))?;
    Ok(())
}

/// SWI 08h `Sqrt`: r0 is an unsigned 32-bit input, result is the floor of the
/// unsigned integer square root in r0 (0..=65535).
pub(super) fn sqrt(cpu: &mut Cpu) -> Result<()> {
    let value = cpu.reg(0);
    cpu.set_reg(0, u32::from(integer_sqrt(value)));
    Ok(())
}

/// Bit-exact floor square root using a restoring binary algorithm. All
/// intermediate values fit in `u64`; no floating point is used.
fn integer_sqrt(value: u32) -> u16 {
    let mut remainder = u64::from(value);
    let mut root = 0u64;
    let mut bit = 1u64 << 30;
    while bit > remainder {
        bit >>= 2;
    }
    while bit != 0 {
        if remainder >= root + bit {
            remainder -= root + bit;
            root = (root >> 1) + bit;
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    root as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_sqrt_known_values() {
        for (value, expected) in [
            (0, 0),
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 2),
            (15, 3),
            (16, 4),
            (17, 4),
            (65_535, 255),
            (65_536, 256),
            (4_294_836_225, 65_535),
            (u32::MAX, 65_535),
        ] {
            assert_eq!(integer_sqrt(value), expected, "sqrt({value})");
        }
    }

    #[test]
    fn integer_sqrt_property_in_64_bit() {
        for value in (0..=131_072u32).chain([4_294_836_224, 4_294_836_225, 4_294_836_226, u32::MAX])
        {
            let root = u64::from(integer_sqrt(value));
            let value = u64::from(value);
            assert!(root * root <= value, "root too large for {value}");
            assert!(
                (root + 1) * (root + 1) > value,
                "root too small for {value}"
            );
        }
    }
}
