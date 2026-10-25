use gba_native::cart::Cartridge;
use gba_native::decode::{decode_arm, decode_thumb};
use gba_native::error::{Error, Result};
use gba_native::ir::IrInstr;
use gba_native::sha256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_blocks: usize,
    pub max_instructions: usize,
    pub max_block_instructions: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_blocks: 512,
            max_instructions: 8192,
            max_block_instructions: 64,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AotBlock {
    pub addr: u32,
    pub thumb: bool,
    pub instructions: Vec<IrInstr>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiscoveryStats {
    pub blocks: usize,
    pub instructions: usize,
    pub resolved_bx: usize,
    pub dynamic_bx: usize,
    /// ARM `BL` callees followed during discovery.
    pub bl_calls: usize,
    /// Return continuations followed after a call (ARM `BL` or the second
    /// Thumb long-branch half).
    pub bl_returns: usize,
    /// Thumb long-branch second halves whose callee is a dynamic LR target.
    pub dynamic_bl: usize,
    /// Non-branch blocks terminated by a PC write (e.g. ALU/MOV, word LDR,
    /// LDM, or POP to R15).
    pub pc_writes: usize,
    pub decode_stops: usize,
    pub external_targets: usize,
    /// Number of blocks cut at `max_block_instructions` and resumed from the
    /// continuation address instead of aborting discovery.
    pub block_limit_splits: usize,
    /// A global `max_blocks`/`max_instructions` limit stopped discovery.
    pub limit_reached: bool,
}

#[derive(Clone, Debug)]
pub struct AotArtifact {
    pub rom_sha256: [u8; 32],
    pub rom_len: usize,
    pub entry: u32,
    pub entry_thumb: bool,
    pub blocks: Vec<AotBlock>,
    pub stats: DiscoveryStats,
}

impl AotArtifact {
    pub fn rom_sha256_hex(&self) -> String {
        sha256::hex(&self.rom_sha256)
    }

    pub fn verify(&self, cart: &Cartridge) -> Result<()> {
        if self.rom_len != cart.len() || self.rom_sha256 != cart.sha256() {
            return Err(Error::Aot(
                "artifact digest/size does not match the supplied cartridge".to_string(),
            ));
        }
        if cart.rom_offset(self.entry).is_none() {
            return Err(Error::Aot(
                "artifact entry is not inside the immutable ROM".to_string(),
            ));
        }
        if (self.entry_thumb && !self.entry.is_multiple_of(2))
            || (!self.entry_thumb && !self.entry.is_multiple_of(4))
        {
            return Err(Error::Aot(
                "artifact entry is not aligned for its instruction state".to_string(),
            ));
        }
        for block in &self.blocks {
            if block.instructions.is_empty() {
                return Err(Error::Aot(format!(
                    "artifact block at 0x{:08X} has no instructions",
                    block.addr
                )));
            }
            if cart.rom_offset(block.addr).is_none() {
                return Err(Error::Aot(format!(
                    "artifact block at 0x{:08X} is not inside the immutable ROM",
                    block.addr
                )));
            }
            let mut expected_addr = block.addr;
            for instruction in &block.instructions {
                if cart.rom_offset(instruction.addr).is_none() {
                    return Err(Error::Aot(format!(
                        "artifact instruction at 0x{:08X} is not inside the immutable ROM",
                        instruction.addr
                    )));
                }
                if instruction.addr != expected_addr || instruction.thumb != block.thumb {
                    return Err(Error::Aot(format!(
                        "artifact block at 0x{:08X} has a non-contiguous instruction stream",
                        block.addr
                    )));
                }
                expected_addr = instruction.next_addr();
                let current = decode_rom(cart, instruction.addr, instruction.thumb)?;
                if current != *instruction {
                    return Err(Error::Aot(format!(
                        "artifact instruction at 0x{:08X} no longer matches ROM bytes",
                        instruction.addr
                    )));
                }
            }
        }
        Ok(())
    }
}

pub(super) fn decode_rom(cart: &Cartridge, addr: u32, thumb: bool) -> Result<IrInstr> {
    if thumb {
        let raw = cart
            .read_u16(addr)
            .ok_or_else(|| Error::Aot(format!("Thumb decode past end of ROM at 0x{addr:08X}")))?;
        decode_thumb(addr, raw as u32)
    } else {
        let raw = cart
            .read_u32(addr)
            .ok_or_else(|| Error::Aot(format!("ARM decode past end of ROM at 0x{addr:08X}")))?;
        decode_arm(addr, raw)
    }
}
