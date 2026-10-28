//! Deliberately small memory bus.
//!
//! Implements mirrored EWRAM/IWRAM, palette, VRAM, OAM, three ROM windows,
//! selected I/O register blocks, bounded DMA/Sound FIFO behavior, and a
//! host-driven frame/IRQ model.
//! Unmodelled I/O reads and open-bus behavior remain explicit errors; unused
//! I/O writes are ignored and recorded. This is not a cycle-accurate bus.

use std::collections::BTreeSet;

mod access;
mod dma;
mod io;
mod irq;
mod sound;
mod timer;
pub use io::{
    KEY_A, KEY_B, KEY_DOWN, KEY_L, KEY_LEFT, KEY_MASK, KEY_R, KEY_RIGHT, KEY_SELECT, KEY_START,
    KEY_UP,
};
pub use sound::{FIFO_A_ADDR, FIFO_B_ADDR, SoundFifo};

use crate::cart::{Cartridge, ROM_BASE};
use crate::error::{Error, Result};
use crate::sha256;

pub const BIOS_BASE: u32 = 0x0000_0000;
pub const BIOS_LEN: usize = 16 * 1024;
pub const EWRAM_BASE: u32 = 0x0200_0000;
pub const EWRAM_LEN: usize = 256 * 1024;
pub const IWRAM_BASE: u32 = 0x0300_0000;
pub const IWRAM_LEN: usize = 32 * 1024;
pub const IO_BASE: u32 = 0x0400_0000;
pub const PALETTE_BASE: u32 = 0x0500_0000;
pub const PALETTE_LEN: usize = 1024;
pub const VRAM_BASE: u32 = 0x0600_0000;
pub const VRAM_LEN: usize = 96 * 1024;
pub const OAM_BASE: u32 = 0x0700_0000;
pub const OAM_LEN: usize = 1024;
pub const DISPCNT_ADDR: u32 = 0x0400_0000;
pub const BG0CNT_ADDR: u32 = 0x0400_0008;
pub const BG0HOFS_ADDR: u32 = 0x0400_0010;
pub const BG0VOFS_ADDR: u32 = 0x0400_0012;
pub const WAITCNT_ADDR: u32 = 0x0400_0204;
pub const KEYINPUT_ADDR: u32 = 0x0400_0130;
pub const KEYINPUT_RELEASED: u16 = 0x03FF;

/// Host-selected policy for **data** reads from the 16 KiB BIOS region.
///
/// This is not guest state and is deliberately excluded from
/// [`Bus::state_digest`]. It never makes the BIOS executable and never permits
/// writes. See [`Bus::with_synthetic_bios_zero_fill`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BiosDataPolicy {
    /// Fail closed: every BIOS data read returns [`Error::BiosAccess`].
    Strict,
    /// **Experimental, non-hardware-faithful assumption:** BIOS data reads
    /// return `0` for every width. This is a deterministic synthetic fallback
    /// for bounded progress experiments, not authentic BIOS data, instruction
    /// bytes or hardware behavior.
    SyntheticZeroFill,
}

impl BiosDataPolicy {
    /// Stable label for CLI output and snapshot reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::SyntheticZeroFill => "synthetic-zero-fill",
        }
    }
}

pub struct Bus {
    cart: Cartridge,
    ewram: Vec<u8>,
    iwram: Vec<u8>,
    palette: Vec<u8>,
    vram: Vec<u8>,
    oam: Vec<u8>,
    io: io::IoState,
    pressed_keys: u16,
    bios_data_policy: BiosDataPolicy,
    ignored_io_writes: Vec<u32>,
    ignored_io_write_addresses: BTreeSet<u32>,
    ignored_io_write_count: u64,
    ignored_io_writes_truncated: bool,
}

impl Bus {
    /// Fail-closed construction: BIOS data reads are explicit errors.
    pub fn new(cart: Cartridge) -> Self {
        Self::with_bios_data_policy(cart, BiosDataPolicy::Strict)
    }

    /// Explicit opt-in construction for the synthetic BIOS zero-fill policy.
    ///
    /// Under this policy, `read8`/`read16`/`read32` at
    /// `0x00000000..=0x00003FFF` return `0` after the usual alignment checks.
    /// BIOS fetches still return [`Error::NonExecutable`] and BIOS writes still
    /// return [`Error::BiosAccess`]. This is an experimental assumption, not a
    /// BIOS image, opcode source or hardware-faithful behavior.
    pub fn with_synthetic_bios_zero_fill(cart: Cartridge) -> Self {
        Self::with_bios_data_policy(cart, BiosDataPolicy::SyntheticZeroFill)
    }

    pub fn with_bios_data_policy(cart: Cartridge, policy: BiosDataPolicy) -> Self {
        Self {
            cart,
            ewram: vec![0; EWRAM_LEN],
            iwram: vec![0; IWRAM_LEN],
            palette: vec![0; PALETTE_LEN],
            vram: vec![0; VRAM_LEN],
            oam: vec![0; OAM_LEN],
            io: io::IoState::default(),
            pressed_keys: 0,
            bios_data_policy: policy,
            ignored_io_writes: Vec::new(),
            ignored_io_write_addresses: BTreeSet::new(),
            ignored_io_write_count: 0,
            ignored_io_writes_truncated: false,
        }
    }

    pub fn bios_data_policy(&self) -> BiosDataPolicy {
        self.bios_data_policy
    }

    /// Sets the host-injected GBA key mask, where one means pressed.
    ///
    /// KEYINPUT reads expose this state active-low. Bits outside [`KEY_MASK`]
    /// are ignored. This does not provide frame-synchronous sampling or a
    /// timing guarantee.
    pub fn set_pressed_keys(&mut self, pressed: u16) {
        self.pressed_keys = pressed & KEY_MASK;
    }

    pub fn cart(&self) -> &Cartridge {
        &self.cart
    }

    pub fn dispcnt(&self) -> u16 {
        self.io.dispcnt()
    }

    pub(crate) fn bg0cnt(&self) -> u16 {
        self.io.bg0cnt()
    }

    pub(crate) fn bg0_scroll(&self) -> (u16, u16) {
        self.io.bg0_scroll()
    }

    /// Instruction fetch. Only ROM and work RAM are executable by design.
    /// [`BiosDataPolicy`] affects data reads only; BIOS fetches stay
    /// non-executable under every policy.
    pub fn fetch16(&self, addr: u32) -> Result<u16> {
        if !addr.is_multiple_of(2) {
            return Err(Error::UnalignedFetch { addr, thumb: true });
        }
        if is_rom(addr)
            || access::mirror_offset(addr, EWRAM_BASE, EWRAM_LEN).is_some()
            || access::mirror_offset(addr, IWRAM_BASE, IWRAM_LEN).is_some()
        {
            return self.read16(addr);
        }
        Err(Error::NonExecutable { addr })
    }

    pub fn fetch32(&self, addr: u32) -> Result<u32> {
        if !addr.is_multiple_of(4) {
            return Err(Error::UnalignedFetch { addr, thumb: false });
        }
        if is_rom(addr)
            || access::mirror_offset(addr, EWRAM_BASE, EWRAM_LEN).is_some()
            || access::mirror_offset(addr, IWRAM_BASE, IWRAM_LEN).is_some()
        {
            return self.read32(addr);
        }
        Err(Error::NonExecutable { addr })
    }

    /// SWI 01h group 7 over the modelled "other I/O" set. DISPCNT is
    /// deliberately absent: the service forces blank separately.
    pub fn reset_group7_io(&mut self) {
        self.io.reset_group7();
    }

    /// SWI 01h group 6 over the modelled sound-register storage.
    pub fn reset_group6_sound(&mut self) {
        self.io.reset_group6();
    }

    pub fn state_digest(&self) -> [u8; 32] {
        let mut bytes = Vec::with_capacity(
            self.ewram.len()
                + self.iwram.len()
                + self.palette.len()
                + self.vram.len()
                + self.oam.len()
                + 10,
        );
        bytes.extend_from_slice(&self.ewram);
        bytes.extend_from_slice(&self.iwram);
        bytes.extend_from_slice(&self.palette);
        bytes.extend_from_slice(&self.vram);
        bytes.extend_from_slice(&self.oam);
        self.io.append_digest(&mut bytes);
        bytes.extend_from_slice(&(KEYINPUT_RELEASED & !self.pressed_keys).to_le_bytes());
        sha256::digest(&bytes)
    }

    pub fn vram_digest(&self) -> [u8; 32] {
        sha256::digest(&self.vram)
    }

    pub(crate) fn is_work_ram_range(addr: u32, len: usize) -> bool {
        let Some(last) = len
            .checked_sub(1)
            .and_then(|size| u32::try_from(size).ok())
            .and_then(|size| addr.checked_add(size))
        else {
            return false;
        };
        [(EWRAM_BASE, EWRAM_LEN), (IWRAM_BASE, IWRAM_LEN)]
            .iter()
            .any(|(base, size)| {
                access::mirror_offset(addr, *base, *size).is_some()
                    && access::mirror_offset(last, *base, *size).is_some()
            })
    }
}

fn is_rom(addr: u32) -> bool {
    (ROM_BASE..=0x0DFF_FFFF).contains(&addr)
}

fn is_bios(addr: u32) -> bool {
    (BIOS_BASE..BIOS_BASE + BIOS_LEN as u32).contains(&addr)
}

fn offset_in(addr: u32, base: u32, len: usize) -> Option<usize> {
    addr.checked_sub(base)
        .map(|offset| offset as usize)
        .filter(|offset| *offset < len)
}

fn require_halfword(addr: u32) -> Result<()> {
    if addr.is_multiple_of(2) {
        Ok(())
    } else {
        Err(Error::UnalignedHalfword { addr })
    }
}

fn require_word(addr: u32) -> Result<()> {
    if addr.is_multiple_of(4) {
        Ok(())
    } else {
        Err(Error::UnalignedWord { addr })
    }
}

#[cfg(test)]
mod tests;
