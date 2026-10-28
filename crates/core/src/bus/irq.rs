//! Interrupt controller, DISPSTAT/VCOUNT and the host-driven frame boundary.
//!
//! **Design choice, not a timing claim.** This prototype has no cycle counter,
//! so a frame is a fixed number of guest instructions derived from the
//! documented 280896 cycles per frame divided by a nominal three cycles per
//! instruction. A frame has exactly two phases: active and VBlank, with VBlank
//! occupying one tenth of it. There are no 228 scanlines, no HBlank, no
//! waitstates and no prefetch. `VCOUNT` reports the documented VBlank start line
//! during the VBlank phase and zero otherwise, which is a two-phase
//! approximation and not scanline timing.

use super::Bus;

pub(super) const DISPSTAT_ADDR: u32 = 0x0400_0004;
pub(super) const VCOUNT_ADDR: u32 = 0x0400_0006;
pub(super) const IE_ADDR: u32 = 0x0400_0200;
pub(super) const IF_ADDR: u32 = 0x0400_0202;
pub(super) const IME_ADDR: u32 = 0x0400_0208;

/// 280896 cycles per frame at a nominal three cycles per instruction.
pub const FRAME_INSTRUCTIONS: u64 = 280_896 / 3;
const VBLANK_INSTRUCTIONS: u64 = FRAME_INSTRUCTIONS / 10;
/// First line of the documented VBlank period.
const VBLANK_LINE: u8 = 160;

const DISPSTAT_VBLANK_FLAG: u16 = 1 << 0;
const DISPSTAT_WRITABLE: u16 = 0b0011_1000;
const IF_VBLANK: u16 = 1 << 0;
const IE_MASK: u16 = 0x3FFF;
const IME_ENABLE: u16 = 1;

#[derive(Clone, Copy)]
pub(super) struct IrqState {
    ie: u16,
    pending: u16,
    ime: u16,
    /// Writable DISPSTAT bits only; the VBlank flag is derived from the phase.
    dispstat: u16,
    in_vblank: bool,
    phase_remaining: u64,
}

impl Default for IrqState {
    fn default() -> Self {
        Self {
            ie: 0,
            pending: 0,
            ime: 0,
            dispstat: 0,
            in_vblank: false,
            // A fresh bus starts in the active phase, not on a boundary.
            phase_remaining: FRAME_INSTRUCTIONS - VBLANK_INSTRUCTIONS,
        }
    }
}

impl IrqState {
    pub(super) fn read16(&self, addr: u32) -> Option<u16> {
        Some(match addr {
            DISPSTAT_ADDR => {
                let flag = if self.in_vblank {
                    DISPSTAT_VBLANK_FLAG
                } else {
                    0
                };
                self.dispstat | flag
            }
            VCOUNT_ADDR => u16::from(if self.in_vblank { VBLANK_LINE } else { 0 }),
            IE_ADDR => self.ie,
            IF_ADDR => self.pending,
            IME_ADDR => self.ime,
            _ => return None,
        })
    }

    pub(super) fn write16(&mut self, addr: u32, value: u16) -> Option<()> {
        match addr {
            DISPSTAT_ADDR => self.dispstat = value & DISPSTAT_WRITABLE,
            IE_ADDR => self.ie = value & IE_MASK,
            // Documented write-one-to-clear semantics.
            IF_ADDR => self.pending &= !value,
            IME_ADDR => self.ime = value & IME_ENABLE,
            // VCOUNT is read-only on hardware.
            VCOUNT_ADDR => return None,
            _ => return None,
        }
        Some(())
    }

    /// Advance `count` guest instructions, e.g. a whole emitted block.
    pub(super) fn advance(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Advance one guest instruction and cross the frame boundary when due.
    pub(super) fn tick(&mut self) {
        self.phase_remaining -= 1;
        if self.phase_remaining > 0 {
            return;
        }
        self.in_vblank = !self.in_vblank;
        self.phase_remaining = if self.in_vblank {
            VBLANK_INSTRUCTIONS
        } else {
            FRAME_INSTRUCTIONS - VBLANK_INSTRUCTIONS
        };
        if self.in_vblank {
            self.pending |= IF_VBLANK;
        }
    }

    pub(super) fn irq_pending(&self) -> bool {
        self.ime & IME_ENABLE != 0 && self.ie & self.pending != 0
    }

    pub(super) fn reset_group7(&mut self) {
        self.ie = 0;
        self.pending = 0;
        self.ime = 0;
        self.dispstat = 0;
    }

    pub(super) fn append_digest(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.ie.to_le_bytes());
        bytes.extend_from_slice(&self.pending.to_le_bytes());
        bytes.extend_from_slice(&self.ime.to_le_bytes());
        bytes.extend_from_slice(&self.dispstat.to_le_bytes());
        // The VBlank phase is guest-visible through DISPSTAT/VCOUNT and is
        // hashed; the instruction counter that produces it is model-internal.
        bytes.push(u8::from(self.in_vblank));
    }
}

impl Bus {
    /// Advance the frame model by one guest instruction.
    pub fn tick_frame(&mut self) {
        self.io.irq_mut().tick();
    }

    /// Advance the frame model by `count` guest instructions.
    pub fn tick_frames(&mut self, count: u64) {
        self.io.irq_mut().advance(count);
    }

    /// True when an enabled interrupt is pending at the controller.
    pub fn irq_pending(&self) -> bool {
        self.io.irq().irq_pending()
    }

    /// The documented IRQ handler pointer at `0x03007FFC`, when the guest wrote
    /// one. Without it there is nowhere to dispatch, so delivery stays off.
    pub fn irq_handler(&self) -> Option<u32> {
        let bytes: [u8; 4] = self.iwram.get(0x7FFC..0x8000)?.try_into().ok()?;
        let handler = u32::from_le_bytes(bytes);
        (handler != 0).then_some(handler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::cart::Cartridge;

    fn bus() -> Bus {
        Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"))
    }

    #[test]
    fn frame_boundary_sets_the_vblank_flag_and_request() {
        let mut bus = bus();
        assert_eq!(bus.read16(DISPSTAT_ADDR).expect("dispstat") & 1, 0);
        assert_eq!(bus.read16(VCOUNT_ADDR).expect("vcount"), 0);
        bus.tick_frames(FRAME_INSTRUCTIONS - VBLANK_INSTRUCTIONS);
        assert_eq!(bus.read16(DISPSTAT_ADDR).expect("dispstat") & 1, 1);
        assert_eq!(bus.read16(VCOUNT_ADDR).expect("vcount"), 160);
        assert_eq!(bus.read16(IF_ADDR).expect("if") & 1, 1);
    }

    #[test]
    fn irq_requires_ime_ie_and_a_pending_bit() {
        let mut bus = bus();
        bus.write16(IE_ADDR, 1).expect("ie");
        assert!(!bus.irq_pending(), "IME still disabled");
        bus.write16(IME_ADDR, 1).expect("ime");
        assert!(!bus.irq_pending(), "nothing pending yet");
        bus.tick_frames(FRAME_INSTRUCTIONS - VBLANK_INSTRUCTIONS);
        assert!(bus.irq_pending());
        bus.write16(IF_ADDR, 1).expect("clear");
        assert!(!bus.irq_pending(), "write-one-to-clear");
    }

    #[test]
    fn dispstat_keeps_writable_bits_and_derives_the_flag() {
        let mut bus = bus();
        bus.write16(DISPSTAT_ADDR, 0xFFFF).expect("dispstat");
        // Only the documented enable bits 3..5 are stored.
        assert_eq!(bus.read16(DISPSTAT_ADDR).expect("dispstat"), 0x0038);
    }
}
