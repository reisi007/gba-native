//! Typed intermediate representation decoded from ARM/Thumb instruction words.
//!
//! The IR contains only operations that this prototype actually executes. The
//! AOT emitter consumes exactly these operations; there is no raw-opcode
//! passthrough.

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

/// A data-processing source operand. `Imm` carries the documented ARM
/// shifter-carry result (if any) so MOVS/CMPS flag behavior stays accurate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    Reg(u8),
    Imm { value: u32, carry: Option<bool> },
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
    /// Conditional or unconditional direct branch. The target is already
    /// aligned and relative to the documented pipeline offset.
    B {
        target: u32,
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
        matches!(self.op, Op::B { .. } | Op::Bx { .. })
    }

    pub fn writes_register(self, register: u8) -> bool {
        match self.op {
            Op::Mov { rd, .. } | Op::Add { rd, .. } | Op::Sub { rd, .. } => rd == register,
            Op::LdrLiteral { rd, .. } | Op::LdrHalf { rd, .. } => rd == register,
            Op::Cmp { .. } | Op::B { .. } | Op::Bx { .. } | Op::StrHalf { .. } => false,
        }
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
