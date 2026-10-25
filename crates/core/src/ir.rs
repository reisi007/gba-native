//! Typed intermediate representation decoded from ARM/Thumb instruction words.
//!
//! The IR contains only operations that this prototype actually executes. The
//! AOT emitter consumes exactly these operations; there is no raw-opcode
//! passthrough. Operand shifts are explicit (`Shift`), data-processing
//! operations other than the original MOV/ADD/SUB/CMP subset use the typed
//! `AluOpcode` family, and the two halves of a Thumb long branch are separate
//! instructions so an instruction budget can stop between them.

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Eq,
    Ne,
    Cs,
    Cc,
    Mi,
    Pl,
    Vs,
    Vc,
    Hi,
    Ls,
    Ge,
    Lt,
    Gt,
    Le,
    Al,
}

impl Cond {
    /// ARM condition field 0..=14. Field value 15 is the reserved/unconditional
    /// space on ARMv4T and is rejected rather than mapped to `Al`.
    pub fn from_bits(bits: u32) -> Option<Self> {
        Some(match bits {
            0x0 => Cond::Eq,
            0x1 => Cond::Ne,
            0x2 => Cond::Cs,
            0x3 => Cond::Cc,
            0x4 => Cond::Mi,
            0x5 => Cond::Pl,
            0x6 => Cond::Vs,
            0x7 => Cond::Vc,
            0x8 => Cond::Hi,
            0x9 => Cond::Ls,
            0xA => Cond::Ge,
            0xB => Cond::Lt,
            0xC => Cond::Gt,
            0xD => Cond::Le,
            0xE => Cond::Al,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Cond::Eq => "Eq",
            Cond::Ne => "Ne",
            Cond::Cs => "Cs",
            Cond::Cc => "Cc",
            Cond::Mi => "Mi",
            Cond::Pl => "Pl",
            Cond::Vs => "Vs",
            Cond::Vc => "Vc",
            Cond::Hi => "Hi",
            Cond::Ls => "Ls",
            Cond::Ge => "Ge",
            Cond::Lt => "Lt",
            Cond::Gt => "Gt",
            Cond::Le => "Le",
            Cond::Al => "Al",
        }
    }
}

/// ARM/Thumb barrel-shifter operation. `Ror` with an immediate amount of zero is
/// the documented `RRX` form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShiftKind {
    Lsl,
    Lsr,
    Asr,
    Ror,
}

impl ShiftKind {
    pub fn name(self) -> &'static str {
        match self {
            ShiftKind::Lsl => "LSL",
            ShiftKind::Lsr => "LSR",
            ShiftKind::Asr => "ASR",
            ShiftKind::Ror => "ROR",
        }
    }

    pub fn rust_name(self) -> &'static str {
        match self {
            ShiftKind::Lsl => "Lsl",
            ShiftKind::Lsr => "Lsr",
            ShiftKind::Asr => "Asr",
            ShiftKind::Ror => "Ror",
        }
    }
}

/// A barrel-shift operand. `Imm` keeps the encoded 5-bit amount (0 has the
/// documented special meanings); `Reg` names the register whose low eight bits
/// are the runtime shift amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shift {
    Imm { kind: ShiftKind, amount: u8 },
    Reg { kind: ShiftKind, rm: u8 },
}

impl Shift {
    pub fn kind(self) -> ShiftKind {
        match self {
            Shift::Imm { kind, .. } | Shift::Reg { kind, .. } => kind,
        }
    }

    pub fn is_register_controlled(self) -> bool {
        matches!(self, Shift::Reg { .. })
    }
}

/// A data-processing source operand. `Imm` carries the documented ARM
/// shifter-carry result (if any) so MOVS/CMPS flag behavior stays accurate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    Reg(u8),
    Imm { value: u32, carry: Option<bool> },
    RegShift { rm: u8, shift: Shift },
}

impl Src {
    pub fn uses_register_shift(self) -> bool {
        matches!(
            self,
            Src::RegShift {
                shift: Shift::Reg { .. },
                ..
            }
        )
    }
}

/// Program status register selected by an ARM MRS/MSR encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Psr {
    Cpsr,
    Spsr,
}

impl Psr {
    pub fn name(self) -> &'static str {
        match self {
            Psr::Cpsr => "CPSR",
            Psr::Spsr => "SPSR",
        }
    }

    /// Rust variant spelling used by generated native source.
    pub fn rust_name(self) -> &'static str {
        match self {
            Psr::Cpsr => "Cpsr",
            Psr::Spsr => "Spsr",
        }
    }
}

/// One MSR field-mask bit from GBATEK `#arm-opcodes-psr-transfer-mrs-msr`.
/// `s` and `x` name reserved ARM7TDMI fields and are preserved on a write.
pub const PSR_FIELD_F: u8 = 1 << 3;
pub const PSR_FIELD_S: u8 = 1 << 2;
pub const PSR_FIELD_X: u8 = 1 << 1;
pub const PSR_FIELD_C: u8 = 1;

/// MSR source. Immediate operands are already expanded (`imm8 ROR 2*rot`);
/// carry-out is irrelevant because PSR transfers never update NZCV themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsrSrc {
    Reg(u8),
    Imm(u32),
}

/// Data-processing operations that are not represented by the original
/// `Mov`/`Add`/`Sub`/`Cmp` variants. The opcode and operand are explicit typed
/// data, never a decoded raw instruction word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AluOpcode {
    And,
    Eor,
    Rsb,
    Adc,
    Sbc,
    Rsc,
    Tst,
    Teq,
    Cmn,
    Orr,
    Bic,
    Mvn,
}

impl AluOpcode {
    pub fn name(self) -> &'static str {
        match self {
            AluOpcode::And => "AND",
            AluOpcode::Eor => "EOR",
            AluOpcode::Rsb => "RSB",
            AluOpcode::Adc => "ADC",
            AluOpcode::Sbc => "SBC",
            AluOpcode::Rsc => "RSC",
            AluOpcode::Tst => "TST",
            AluOpcode::Teq => "TEQ",
            AluOpcode::Cmn => "CMN",
            AluOpcode::Orr => "ORR",
            AluOpcode::Bic => "BIC",
            AluOpcode::Mvn => "MVN",
        }
    }

    pub fn rust_name(self) -> &'static str {
        match self {
            AluOpcode::And => "And",
            AluOpcode::Eor => "Eor",
            AluOpcode::Rsb => "Rsb",
            AluOpcode::Adc => "Adc",
            AluOpcode::Sbc => "Sbc",
            AluOpcode::Rsc => "Rsc",
            AluOpcode::Tst => "Tst",
            AluOpcode::Teq => "Teq",
            AluOpcode::Cmn => "Cmn",
            AluOpcode::Orr => "Orr",
            AluOpcode::Bic => "Bic",
            AluOpcode::Mvn => "Mvn",
        }
    }

    /// Test operations update flags but never write `Rd`.
    pub fn is_test(self) -> bool {
        matches!(self, AluOpcode::Tst | AluOpcode::Teq | AluOpcode::Cmn)
    }

    pub fn is_logical(self) -> bool {
        matches!(
            self,
            AluOpcode::And
                | AluOpcode::Eor
                | AluOpcode::Tst
                | AluOpcode::Teq
                | AluOpcode::Orr
                | AluOpcode::Bic
                | AluOpcode::Mvn
        )
    }

    pub fn is_arithmetic(self) -> bool {
        matches!(
            self,
            AluOpcode::Rsb | AluOpcode::Adc | AluOpcode::Sbc | AluOpcode::Rsc | AluOpcode::Cmn
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Mov {
        rd: u8,
        src: Src,
        set_flags: bool,
    },
    Add {
        rd: u8,
        rn: u8,
        rhs: Src,
        set_flags: bool,
    },
    Sub {
        rd: u8,
        rn: u8,
        rhs: Src,
        set_flags: bool,
    },
    Cmp {
        rn: u8,
        rhs: Src,
    },
    /// The remaining ARM data-processing operations, plus the register forms
    /// shared with Thumb. `rd` is `None` for TST/TEQ/CMN.
    Alu {
        opcode: AluOpcode,
        rd: Option<u8>,
        rn: u8,
        rhs: Src,
        set_flags: bool,
    },
    /// Conditional or unconditional direct branch. The target is already
    /// aligned and relative to the documented pipeline offset.
    B {
        target: u32,
    },
    /// ARM `BL`: stores the return address in LR and branches to the callee.
    Bl {
        target: u32,
    },
    /// First half of a Thumb long branch: only LR is updated. It never branches
    /// by itself, so discovery and the instruction budget treat it as one
    /// ordinary instruction.
    BlHigh {
        lr: u32,
    },
    /// Second half of a Thumb long branch: the target is computed from the
    /// current LR, LR is set to the return address, and control transfers.
    BlLow {
        offset: u32,
    },
    Bx {
        rm: u8,
    },
    LdrLiteral {
        rd: u8,
        addr: u32,
    },
    LdrHalf {
        rd: u8,
        rn: u8,
        offset: i32,
    },
    StrHalf {
        rd: u8,
        rn: u8,
        offset: i32,
    },
    /// ARM `MRS Rd, CPSR|SPSR_<current mode>`.
    Mrs {
        rd: u8,
        psr: Psr,
    },
    /// ARM `MSR CPSR|SPSR_<current mode>{_fields}, Rm|#imm`.
    Msr {
        psr: Psr,
        fields: u8,
        src: PsrSrc,
    },
}

impl Op {
    /// A direct branch, an exchange, a long branch half, or any ALU/MOV write
    /// to R15 ends the current straight-line block.
    pub fn writes_pc(self) -> bool {
        match self {
            Op::Mov { rd, .. } | Op::Add { rd, .. } | Op::Sub { rd, .. } => rd == 15,
            Op::Alu { rd, .. } => rd == Some(15),
            Op::B { .. } | Op::Bl { .. } | Op::BlLow { .. } | Op::Bx { .. } => true,
            Op::BlHigh { .. }
            | Op::Cmp { .. }
            | Op::LdrLiteral { .. }
            | Op::LdrHalf { .. }
            | Op::StrHalf { .. }
            | Op::Mrs { .. }
            | Op::Msr { .. } => false,
        }
    }

    pub fn writes_register(self, register: u8) -> bool {
        match self {
            Op::Mov { rd, .. } | Op::Add { rd, .. } | Op::Sub { rd, .. } => rd == register,
            Op::Alu { rd, .. } => rd == Some(register),
            Op::BlHigh { .. } | Op::Bl { .. } => register == 14,
            Op::LdrLiteral { rd, .. } | Op::LdrHalf { rd, .. } | Op::Mrs { rd, .. } => {
                rd == register
            }
            Op::BlLow { .. } => register == 14 || register == 15,
            Op::Cmp { .. } | Op::B { .. } | Op::Bx { .. } | Op::StrHalf { .. } | Op::Msr { .. } => {
                false
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrInstr {
    pub addr: u32,
    pub thumb: bool,
    pub raw: u32,
    pub cond: Cond,
    pub op: Op,
}

impl IrInstr {
    pub fn width(self) -> u32 {
        if self.thumb { 2 } else { 4 }
    }

    pub fn next_addr(self) -> u32 {
        self.addr.wrapping_add(self.width())
    }

    pub fn is_control_flow(self) -> bool {
        self.op.writes_pc()
    }

    pub fn writes_register(self, register: u8) -> bool {
        self.op.writes_register(register)
    }

    /// A control-field write to the current CPSR can change the active register
    /// bank. Discovery must not carry cached r8-r14 values across it.
    pub fn writes_cpsr_control(self) -> bool {
        matches!(
            self.op,
            Op::Msr {
                psr: Psr::Cpsr,
                fields,
                ..
            } if fields & PSR_FIELD_C != 0
        )
    }
}

pub fn unsupported(addr: u32, thumb: bool, raw: u32, reason: &'static str) -> Error {
    Error::UnsupportedInstruction {
        addr,
        thumb,
        raw,
        reason,
    }
}

pub type DecodeResult = Result<IrInstr>;
