use crate::ir::{Cond, Op, Src};

use crate::decode::{decode_arm, decode_thumb};

#[test]
fn decodes_minimal_arm_subset() {
    let mov = decode_arm(0x0800_0000, 0xE3A0_0000).expect("mov");
    assert_eq!(
        mov.op,
        Op::Mov {
            rd: 0,
            src: Src::Imm {
                value: 0,
                carry: None
            },
            set_flags: false
        }
    );
    let add = decode_arm(0x0800_0004, 0xE280_0002).expect("add");
    assert_eq!(
        add.op,
        Op::Add {
            rd: 0,
            rn: 0,
            rhs: Src::Imm {
                value: 2,
                carry: None
            },
            set_flags: false
        }
    );
    let cmp = decode_arm(0x0800_0008, 0xE152_0001).expect("cmp");
    assert_eq!(
        cmp.op,
        Op::Cmp {
            rn: 2,
            rhs: Src::Reg(1)
        }
    );
}

#[test]
fn arm_immediate_payload_is_never_reinterpreted_as_extra_transfer() {
    let cmp = decode_arm(0x0800_0000, 0xE350_00B0).expect("CMP immediate");
    assert_eq!(
        cmp.op,
        Op::Cmp {
            rn: 0,
            rhs: Src::Imm {
                value: 0xB0,
                carry: None
            }
        }
    );

    let mov = decode_arm(0x0800_0004, 0xE3A0_0009).expect("MOV immediate");
    assert_eq!(
        mov.op,
        Op::Mov {
            rd: 0,
            src: Src::Imm {
                value: 9,
                carry: None
            },
            set_flags: false
        }
    );

    // BIC immediate: unsupported, but specifically not STRH r0,[r1].
    assert!(decode_arm(0x0800_0008, 0xE3C1_00B0).is_err());

    // A malformed CMP (Rd != 0) must be rejected, not decoded as STRH r1,[r0].
    assert!(decode_arm(0x0800_000C, 0xE350_10B4).is_err());
}

#[test]
fn rejects_arm_multiply_and_psr_like_space() {
    assert!(decode_arm(0, 0xE000_0091).is_err());
    assert!(decode_arm(0, 0xE10F_0000).is_err());
    assert!(decode_arm(0, 0xE92D_4000).is_err());
}

#[test]
fn decodes_thumb_loop_instructions() {
    assert!(matches!(
        decode_thumb(0x28, 0x1C13).expect("mov/add").op,
        Op::Add {
            rd: 3,
            rn: 2,
            rhs: Src::Imm { value: 0, .. },
            set_flags: true
        }
    ));
    assert!(matches!(
        decode_thumb(0x2A, 0x8003).expect("strh").op,
        Op::StrHalf {
            rd: 3,
            rn: 0,
            offset: 0
        }
    ));
    assert!(matches!(
        decode_thumb(0x30, 0x428A).expect("cmp").op,
        Op::Cmp {
            rn: 2,
            rhs: Src::Reg(1)
        }
    ));
    let branch = decode_thumb(0x32, 0xD1F9).expect("bne");
    assert_eq!(branch.cond, Cond::Ne);
    assert_eq!(branch.op, Op::B { target: 0x28 });
    assert_eq!(
        decode_thumb(0x34, 0x4770).expect("bx").op,
        Op::Bx { rm: 14 }
    );
}
