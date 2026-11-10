//! Exhaustive Thumb decoder coverage against the ARMv4T instruction formats.
//!
//! This checks the documented 16-bit encoding space, including the prototype's
//! explicit low/low MOV/CMP design extension. It is decoder coverage, not
//! hardware-conformance evidence.

use gba_native_core::decode::decode_thumb;
use gba_native_core::error::Error;

#[test]
fn every_thumb_halfword_is_implemented_or_explicitly_rejected() {
    let addr = 0x0800_0000;
    let mut accepted = 0usize;
    let mut rejected = 0usize;

    for raw in 0..=u16::MAX {
        let expected = expected_supported(raw);
        match decode_thumb(addr, u32::from(raw)) {
            Ok(instruction) => {
                assert!(expected, "unexpectedly decoded Thumb encoding {raw:04X}");
                assert_eq!(instruction.addr, addr, "encoding {raw:04X}");
                assert!(instruction.thumb, "encoding {raw:04X}");
                assert_eq!(instruction.raw, u32::from(raw), "encoding {raw:04X}");
                accepted += 1;
            }
            Err(Error::UnsupportedInstruction {
                addr: error_addr,
                thumb,
                raw: error_raw,
                ..
            }) => {
                assert!(!expected, "unexpectedly rejected Thumb encoding {raw:04X}");
                assert_eq!(error_addr, addr, "encoding {raw:04X}");
                assert!(thumb, "encoding {raw:04X}");
                assert_eq!(error_raw, u32::from(raw), "encoding {raw:04X}");
                rejected += 1;
            }
            Err(error) => panic!("unexpected error for Thumb encoding {raw:04X}: {error}"),
        }
    }

    assert_eq!(accepted + rejected, 1 << 16);
    assert!(accepted > 0 && rejected > 0);
}

fn expected_supported(raw: u16) -> bool {
    match raw {
        0x0000..=0x43FF => true,
        0x4400..=0x47FF => match (raw >> 8) & 0x3 {
            0 => raw & 0x00C0 != 0, // low/low ADD is reserved
            3 => raw & 0x0087 == 0, // BX requires bit 7 and unused destination bits clear
            _ => true,
        },
        0x4800..=0xAFFF => true,
        0xB000..=0xB0FF => true, // ADD/SUB SP, #imm
        0xB100..=0xBFFF => {
            let push_or_pop = raw & 0x0600 == 0x0400;
            let has_register = raw & 0x00FF != 0;
            let has_lr_or_pc = raw & 0x0100 != 0;
            push_or_pop && (has_register || has_lr_or_pc)
        }
        0xC000..=0xDDFF | 0xDF00..=0xE7FF | 0xF000..=0xFFFF => true,
        // Undefined conditional branch, ARMv5 BLX second half, and other gaps.
        0xDE00..=0xDEFF | 0xE800..=0xEFFF => false,
    }
}
