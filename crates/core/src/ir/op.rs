//! Typed instruction operations and their register/control-flow metadata.

use super::{AluOpcode, MemOffset, MemWidth, Psr, PsrSrc, Src};

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
    /// ARM `MUL`/`MLA` and Thumb `MUL`: `rd = rm * rs` plus the optional
    /// `accumulate` register. Only the low 32 bits of the product are kept, so
    /// signed and unsigned multiplication coincide. With `set_flags` the
    /// documented N/Z flags follow the result; C is architecturally
    /// unpredictable ("destroyed") on ARMv4 and V is unaffected, so this
    /// prototype preserves the previous C bit as a stated Design choice.
    Mul {
        rd: u8,
        rm: u8,
        rs: u8,
        accumulate: Option<u8>,
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
    /// ARM/Thumb software interrupt comment. ARM keeps all 24 comment bits;
    /// Thumb keeps the encoded low byte, and service selection happens later.
    Swi {
        comment: u32,
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
    /// Single data transfer with an explicit pre/post index, sign, writeback and
    /// (for post-indexing) the ARM `T` force-user bit. The bus is a flat checked
    /// GBA memory map with no MMU, so `T` does not change CPU mode or privilege.
    MemSingle {
        load: bool,
        width: MemWidth,
        signed: bool,
        rd: u8,
        rn: u8,
        offset: MemOffset,
        add: bool,
        pre_index: bool,
        writeback: bool,
        user_access: bool,
    },
    /// ARM `LDM`/`STM` with IA/IB/DA/DB addressing. `regs` is the non-empty
    /// register bitmap; `S=1` user-bank/SPSR transfers are rejected before IR.
    BlockTransfer {
        load: bool,
        rn: u8,
        regs: u16,
        pre_index: bool,
        add: bool,
        writeback: bool,
    },
    /// Thumb `PUSH {Rlist}{,LR}` (full descending, SP base).
    Push {
        regs: u8,
        lr: bool,
    },
    /// Thumb `POP {Rlist}{,PC}` (full descending, SP base). `pc` keeps Thumb
    /// state and forces the loaded address to halfword alignment.
    Pop {
        regs: u8,
        pc: bool,
    },
    /// Thumb `LDMIA Rb!,{Rlist}` / `STMIA Rb!,{Rlist}` for low registers.
    ThumbBlock {
        load: bool,
        rn: u8,
        regs: u8,
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
            Op::B { .. } | Op::Bl { .. } | Op::BlLow { .. } | Op::Bx { .. } | Op::Swi { .. } => {
                true
            }
            // A word load into R15 changes control flow; narrow loads into PC
            // and stores are rejected before this point. LDM/Thumb POP with PC
            // in the list also end the straight-line block.
            Op::MemSingle {
                load: true,
                width: MemWidth::Word,
                rd: 15,
                ..
            } => true,
            Op::BlockTransfer {
                load: true, regs, ..
            } => regs & (1 << 15) != 0,
            Op::Pop { pc: true, .. } => true,
            Op::BlHigh { .. }
            | Op::Cmp { .. }
            | Op::Mul { .. }
            | Op::LdrLiteral { .. }
            | Op::LdrHalf { .. }
            | Op::StrHalf { .. }
            | Op::MemSingle { .. }
            | Op::BlockTransfer { .. }
            | Op::Push { .. }
            | Op::Pop { .. }
            | Op::ThumbBlock { .. }
            | Op::Mrs { .. }
            | Op::Msr { .. } => false,
        }
    }

    pub fn writes_register(self, register: u8) -> bool {
        match self {
            Op::Mov { rd, .. } | Op::Add { rd, .. } | Op::Sub { rd, .. } => rd == register,
            Op::Alu { rd, .. } => rd == Some(register),
            Op::Mul { rd, .. } => rd == register,
            Op::BlHigh { .. } | Op::Bl { .. } => register == 14,
            Op::LdrLiteral { rd, .. } | Op::LdrHalf { rd, .. } | Op::Mrs { rd, .. } => {
                rd == register
            }
            Op::BlLow { .. } => register == 14 || register == 15,
            Op::MemSingle {
                load,
                rd,
                rn,
                writeback,
                ..
            } => (load && rd == register) || (writeback && rn == register),
            Op::BlockTransfer {
                load,
                rn,
                regs,
                writeback,
                ..
            } => (load && regs & (1 << register) != 0) || (writeback && rn == register),
            // PUSH only reads its register list and writes SP. POP writes its
            // low list, optionally PC, and always updates SP.
            Op::Push { .. } => register == 13,
            Op::Pop { regs, pc, .. } => {
                register == 13
                    || (register < 8 && regs & (1 << register) != 0)
                    || (register == 15 && pc)
            }
            // Thumb LDMIA/STMIA always updates the base; it may also load the
            // listed low registers. A loaded base wins over the increment.
            Op::ThumbBlock { load, rn, regs } => {
                register == rn || (load && register < 8 && regs & (1 << register) != 0)
            }
            // An HLE service may alter any documented result register and
            // memory. Invalidate every cached register constant conservatively.
            Op::Swi { .. } => true,
            Op::Cmp { .. } | Op::B { .. } | Op::Bx { .. } | Op::StrHalf { .. } | Op::Msr { .. } => {
                false
            }
        }
    }
}
