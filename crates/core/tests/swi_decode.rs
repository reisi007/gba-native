//! Encoding contracts for ARM and Thumb SWIs; raw comments stay typed data.

use gba_native_core::decode::{decode_arm, decode_thumb};
use gba_native_core::error::Error;
use gba_native_core::ir::Op;

#[test]
fn arm_swi_keeps_all_comment_bits_and_conditional_field() {
    let arm_div = decode_arm(0x0800_0100, 0xEF06_0000).expect("ARM SWI");
    assert_eq!(arm_div.cond, gba_native_core::ir::Cond::Al);
    assert_eq!(
        arm_div.op,
        Op::Swi {
            comment: 0x0006_0000
        }
    );

    // GBA ARM selects the service from comment[23:16], not comment[7:0].
    let service_zero = decode_arm(0x0800_0100, 0xEF00_0006).expect("ARM SWI");
    assert_eq!(
        service_zero.op,
        Op::Swi {
            comment: 0x0000_0006
        }
    );
    assert!(service_zero.is_control_flow());
    assert!((0..16).all(|register| service_zero.writes_register(register)));

    let conditional = decode_arm(0x0800_0100, 0x0F06_0000).expect("conditional SWI");
    assert_eq!(conditional.cond, gba_native_core::ir::Cond::Eq);
    assert_eq!(
        conditional.op,
        Op::Swi {
            comment: 0x0006_0000
        }
    );
}

#[test]
fn thumb_df_is_swi_but_de_and_cond_f_arm_space_remain_rejected() {
    assert_eq!(
        decode_thumb(0x0800_0100, 0xDF06).expect("Thumb SWI").op,
        Op::Swi { comment: 6 }
    );
    assert!(matches!(
        decode_thumb(0x0800_0100, 0xDE06),
        Err(Error::UnsupportedInstruction { .. })
    ));
    assert!(matches!(
        decode_arm(0x0800_0100, 0xFF06_0000),
        Err(Error::UnsupportedInstruction { .. })
    ));
}

#[test]
fn coprocessor_instruction_space_does_not_alias_to_swi() {
    assert!(matches!(
        decode_arm(0x0800_0100, 0xEE00_0010),
        Err(Error::UnsupportedInstruction { .. })
    ));
}
