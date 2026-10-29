//! Deliberately small memory bus.
//!
//! Implements mirrored EWRAM/IWRAM, palette, VRAM, OAM, three ROM windows,
//! selected I/O register blocks, bounded DMA/Sound FIFO behavior, and a
//! host-driven instruction-count line/frame/IRQ model (`VCOUNT` `0..=227`).
//! Unmodelled I/O reads and open-bus behavior remain explicit errors; unused
//! I/O writes are ignored and recorded. This is not a cycle-accurate bus.

use std::collections::BTreeSet;

mod access;
mod backup;
mod dma;
mod io;
mod irq;
mod sound;
mod timer;
pub use backup::{BackupPolicy, SAVE_BASE, SAVE_LEN, SAVE_LEN_64K, SAVE_WINDOW_LEN};
pub use io::{
    KEY_A, KEY_B, KEY_DOWN, KEY_L, KEY_LEFT, KEY_MASK, KEY_R, KEY_RIGHT, KEY_SELECT, KEY_START,
    KEY_UP,
};
pub use irq::{
    FRAME_INSTRUCTIONS, FRAME_LINES, VBLANK_FIRST_LINE, VBLANK_LAST_LINE, instructions_until_line,
    line_instructions,
};
pub use sound::{FIFO_A_ADDR, FIFO_B_ADDR, SoundFifo};

use crate::bios::{AffinePolicy, DecompressionPolicy};
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

/// Serial I/O data and control registers, modelled as documented storage only.
pub const SIODATA32_L_ADDR: u32 = 0x0400_0120;
pub const SIODATA32_H_ADDR: u32 = 0x0400_0122;
pub const SIOCNT_ADDR: u32 = 0x0400_0128;
pub const SIODATA8_ADDR: u32 = 0x0400_012A;
pub const RCNT_ADDR: u32 = 0x0400_0134;

/// Nominal hardware cycles attributed to one retired guest instruction.
///
/// The instruction-count frame and timer models share this mapping. It is a
/// deterministic approximation, not measured CPU timing: instruction lengths,
/// waitstates and bus arbitration are not modelled.
pub const INSTRUCTION_CYCLES: u32 = 3;

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

/// Host-selected policy for **writes** into the 16 KiB BIOS region.
///
/// The BIOS is mask ROM on hardware, so a store into `0x0000_0000..=0x0000_3FFF`
/// has no effect instead of raising a CPU-visible error. The strict policy is the
/// default and reports such an access as [`Error::BiosAccess`]; the explicit
/// opt-in models the documented "no effect" behavior instead of silently
/// dropping the store.
///
/// This is host configuration, not guest state, and is excluded from
/// [`Bus::state_digest`]. It never makes BIOS data readable or executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BiosWritePolicy {
    /// Fail closed: every write into the BIOS range returns [`Error::BiosAccess`].
    Strict,
    /// **Experimental opt-in:** BIOS stores complete with no effect, matching
    /// mask-ROM semantics. No BIOS byte is stored, fetched or synthesized.
    RomWritesDropped,
}

impl BiosWritePolicy {
    /// Stable label for CLI output and snapshot reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::RomWritesDropped => "rom-writes-dropped",
        }
    }
}

/// Host-selected policy for stores into the Game Pak ROM window.
///
/// A ROM-only cartridge cannot be written; a flash cartridge would need a
/// command engine that this prototype does not model. The strict policy is the
/// default and reports [`Error::RomWrite`]; the explicit opt-in models the
/// documented no-effect store of a ROM-only cartridge and leaves the save/flash
/// region (`0x0E00_0000..=0x0E00_FFFF`) an explicit error.
///
/// This is host configuration, not guest state, and is excluded from
/// [`Bus::state_digest`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RomWritePolicy {
    /// Fail closed: every Game Pak ROM store returns [`Error::RomWrite`].
    Strict,
    /// **Experimental opt-in:** ROM-only cartridge stores complete with no
    /// effect. No flash command engine is modelled.
    Dropped,
}

impl RomWritePolicy {
    /// Stable label for CLI output and snapshot reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Dropped => "dropped",
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
    bios_write_policy: BiosWritePolicy,
    rom_write_policy: RomWritePolicy,
    backup_policy: BackupPolicy,
    decompression_policy: DecompressionPolicy,
    affine_policy: AffinePolicy,
    backup: Vec<u8>,
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
            bios_write_policy: BiosWritePolicy::Strict,
            rom_write_policy: RomWritePolicy::Strict,
            backup_policy: BackupPolicy::Strict,
            decompression_policy: DecompressionPolicy::Strict,
            affine_policy: AffinePolicy::Strict,
            // Fixed 64 KiB erased image: the selected policy only gates the
            // accessible capacity, so the guest-state digest is policy-independent.
            backup: vec![0xFF; SAVE_LEN_64K],
            ignored_io_writes: Vec::new(),
            ignored_io_write_addresses: BTreeSet::new(),
            ignored_io_write_count: 0,
            ignored_io_writes_truncated: false,
        }
    }

    pub fn bios_data_policy(&self) -> BiosDataPolicy {
        self.bios_data_policy
    }

    /// Selects the explicit opt-in for documented mask-ROM write semantics.
    ///
    /// Under [`BiosWritePolicy::RomWritesDropped`] a store into the BIOS range
    /// completes without effect. This does not make the BIOS readable, stored or
    /// executable and does not change any other address range.
    #[must_use]
    pub fn with_bios_write_policy(mut self, policy: BiosWritePolicy) -> Self {
        self.bios_write_policy = policy;
        self
    }

    pub fn bios_write_policy(&self) -> BiosWritePolicy {
        self.bios_write_policy
    }

    /// Selects the explicit opt-in for ROM-only cartridge store semantics.
    ///
    /// Under [`RomWritePolicy::Dropped`] a store into the Game Pak ROM window
    /// completes without effect. Nothing is stored, and the save/flash region
    /// keeps its own explicit error.
    #[must_use]
    pub fn with_rom_write_policy(mut self, policy: RomWritePolicy) -> Self {
        self.rom_write_policy = policy;
        self
    }

    pub fn rom_write_policy(&self) -> RomWritePolicy {
        self.rom_write_policy
    }

    /// Selects the explicit opt-in for a volatile backup device.
    ///
    /// Under [`BackupPolicy::Sram32KVolatile`] the backup window is backed by
    /// 32 KiB of erased SRAM that lives only in memory; under
    /// [`BackupPolicy::Sram64KVolatile`] it is backed by the full 64 KiB
    /// documented window. No file is written and no flash command engine
    /// exists. The backing store stays the fixed 64 KiB erased image; the
    /// policy only gates the accessible capacity.
    #[must_use]
    pub fn with_backup_policy(mut self, policy: BackupPolicy) -> Self {
        self.backup_policy = policy;
        self
    }

    pub fn backup_policy(&self) -> BackupPolicy {
        self.backup_policy
    }

    /// Selects the explicit opt-in for documented decompression HLE.
    ///
    /// Under [`DecompressionPolicy::DocumentedFormats`] SWI `11h/12h/14h/15h/
    /// 16h/17h/18h` implement only the public LZ77, run-length and diff-filter
    /// formats. The default [`DecompressionPolicy::Strict`] keeps them explicit
    /// unsupported-service errors. This is host configuration, not guest state,
    /// and is excluded from [`Bus::state_digest`].
    #[must_use]
    pub fn with_decompression_policy(mut self, policy: DecompressionPolicy) -> Self {
        self.decompression_policy = policy;
        self
    }

    pub fn decompression_policy(&self) -> DecompressionPolicy {
        self.decompression_policy
    }

    /// Selects the explicit opt-in for the documented `ObjAffineSet` service.
    ///
    /// Under [`AffinePolicy::Documented`] SWI `0Fh` implements only the public
    /// affine interface and calculation described in `bios::affine`. The
    /// default [`AffinePolicy::Strict`] keeps `0Fh` an explicit
    /// unsupported-service error. This is host configuration, not guest state,
    /// and is excluded from [`Bus::state_digest`].
    #[must_use]
    pub fn with_affine_policy(mut self, policy: AffinePolicy) -> Self {
        self.affine_policy = policy;
        self
    }

    pub fn affine_policy(&self) -> AffinePolicy {
        self.affine_policy
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

    /// SWI 01h group 5 over the modelled serial-register storage.
    pub fn reset_group5_sio(&mut self) {
        self.io.reset_group5();
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
                + self.backup.len()
                + 10,
        );
        bytes.extend_from_slice(&self.ewram);
        bytes.extend_from_slice(&self.iwram);
        bytes.extend_from_slice(&self.palette);
        bytes.extend_from_slice(&self.vram);
        bytes.extend_from_slice(&self.oam);
        self.io.append_digest(&mut bytes);
        // The backup bytes are guest state only under the SRAM opt-in; under
        // the strict default they are the constant erased image. Hashing them
        // unconditionally keeps the digest independent of the host policy.
        bytes.extend_from_slice(&self.backup);
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
