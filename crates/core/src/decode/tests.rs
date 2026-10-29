use crate::ir::{AluOpcode, Cond, Op, Psr, PsrSrc, Src};

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

    // BIC immediate is a data-processing instruction, not STRH r0,[r1].
    assert_eq!(
        decode_arm(0x0800_0008, 0xE3C1_00B0)
            .expect("BIC immediate")
            .op,
        Op::Alu {
            opcode: AluOpcode::Bic,
            rd: Some(0),
            rn: 1,
            rhs: Src::Imm {
                value: 0xB0,
                carry: None
            },
            set_flags: false
        }
    );

    // A malformed CMP (Rd != 0) must be rejected, not decoded as STRH r1,[r0].
    assert!(decode_arm(0x0800_000C, 0xE350_10B4).is_err());
}

#[test]
fn rejects_arm_multiply_and_block_space() {
    // UMULL shares the low-nibble 1001b shape and stays unsupported.
    assert!(decode_arm(0, 0xE080_0091).is_err());
    // A plain STMDB sp!,{lr} is now implemented; the still-unsupported S=1
    // user-bank form and an empty list stay explicit errors.
    assert!(matches!(
        decode_arm(0, 0xE92D_4000).expect("STMDB sp!,{lr}").op,
        Op::BlockTransfer {
            load: false,
            rn: 13,
            regs: 0x4000,
            pre_index: true,
            add: false,
            writeback: true,
        }
    ));
    assert!(decode_arm(0, 0xE96D_4000).is_err());
    assert!(decode_arm(0, 0xE92D_0000).is_err());
    // SWP shares the multiply/PSR decode region but has its own typed operation.
    assert_eq!(
        decode_arm(0, 0xE100_1092).expect("SWP r1,r2,[r0]").op,
        Op::Swap {
            rd: 1,
            rm: 2,
            rn: 0,
            byte: false,
        }
    );
}

#[test]
fn decodes_psr_transfer_forms_and_rejects_malformed_fixed_bits() {
    assert_eq!(
        decode_arm(0, 0xE10F_0000).expect("MRS r0,CPSR").op,
        Op::Mrs {
            rd: 0,
            psr: Psr::Cpsr
        }
    );
    assert_eq!(
        decode_arm(0, 0xE14F_2000).expect("MRS r2,SPSR").op,
        Op::Mrs {
            rd: 2,
            psr: Psr::Spsr
        }
    );
    assert_eq!(
        decode_arm(0, 0xE129_F000).expect("MSR CPSR_fc,r0").op,
        Op::Msr {
            psr: Psr::Cpsr,
            fields: 0b1001,
            src: PsrSrc::Reg(0)
        }
    );
    assert_eq!(
        decode_arm(0, 0xE321_F01F).expect("MSR CPSR_f,#0x1F").op,
        Op::Msr {
            psr: Psr::Cpsr,
            fields: 0b0001,
            src: PsrSrc::Imm(0x1F)
        }
    );
    // Fixed/unused fields are validation, not aliases for other instructions.
    for raw in [
        0xE10F_F000, // MRS PC
        0xE10F_0001, // MRS low bits nonzero
        0xE10E_0000, // valid MRS shape, fixed Rn != 1111 (must error as MRS)
        0xE12F_0000, // MSR fixed Rd-field != 1111
        0xE129_F010, // register MSR bits 11-4 nonzero
        0xE129_F00F, // MSR from PC
    ] {
        assert!(decode_arm(0, raw).is_err(), "raw 0x{raw:08X} must fail");
    }
    let error = decode_arm(0, 0xE10E_0000).expect_err("MRS fixed Rn");
    assert!(format!("{error}").contains("malformed ARM MRS"), "{error}");
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
