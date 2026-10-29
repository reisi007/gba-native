//! Interrupt controller, DISPSTAT/VCOUNT and the host-driven frame boundary.
//!
//! **Design choice, not a timing claim.** This prototype has no cycle counter,
//! no waitstates and no prefetch. A frame is a fixed guest-instruction budget
//! (`FRAME_INSTRUCTIONS`, derived from the documented 280896 cycles per frame
//! divided by a nominal three cycles per instruction) spread over the
//! documented 228 scanlines. Each line owns [`line_instructions`] guest
//! instructions and the 228 line budgets add up to exactly
//! `FRAME_INSTRUCTIONS`, so no remainder is dropped.
//!
//! `VCOUNT` therefore reports an instruction-count line in `0..=227` and walks
//! every documented line before wrapping from 227 to 0. It remains an
//! approximation: one guest instruction is not three hardware cycles, the line
//! slices model neither drawing nor HBlank periods, and no instruction observes
//! a mid-line split. The DISPSTAT VBlank flag covers lines `160..=226` and line
//! 227 clears it, per GBATEK `#lcd-io-interrupts-and-status` ("set in line
//! 160..226; not 227"). The VBlank IRQ request latches once when line 160 is
//! entered. HBlank, VCOUNT-match IRQs, scanline/cycle timing and BIOS execution
//! remain absent.

use super::Bus;

pub(super) const DISPSTAT_ADDR: u32 = 0x0400_0004;
pub(super) const VCOUNT_ADDR: u32 = 0x0400_0006;
pub(super) const IE_ADDR: u32 = 0x0400_0200;
pub(super) const IF_ADDR: u32 = 0x0400_0202;
pub(super) const IME_ADDR: u32 = 0x0400_0208;

/// 280896 cycles per frame at a nominal three cycles per instruction.
///
/// The total is retained unchanged by the per-line model; only its
/// distribution over the documented 228 lines is new.
pub const FRAME_INSTRUCTIONS: u64 = 280_896 / 3;

/// Documented scanlines per frame (`VCOUNT` range `0..=227`).
pub const FRAME_LINES: u8 = 228;

/// First documented VBlank line; lines `160..=226` set the DISPSTAT flag.
pub const VBLANK_FIRST_LINE: u8 = 160;

/// Last line with the DISPSTAT VBlank flag set; hidden line 227 clears it.
pub const VBLANK_LAST_LINE: u8 = 226;

/// Guest instructions a frame assigns to one documented line.
///
/// Every line owns `FRAME_INSTRUCTIONS / FRAME_LINES` instructions plus one for
/// each of the `FRAME_INSTRUCTIONS % FRAME_LINES` remainder instructions. The
/// extra instructions are spread evenly by remainder accumulation, so the
/// per-line values differ by at most one and their sum is exactly
/// [`FRAME_INSTRUCTIONS`]. This is an instruction-count slice, not scanline or
/// cycle timing.
///
/// # Panics
///
/// Panics when `line >= FRAME_LINES`, because only the 228 documented lines have
/// a budget.
pub const fn line_instructions(line: u8) -> u64 {
    assert!(line < FRAME_LINES, "line must be inside the frame");
    let lines = FRAME_LINES as u64;
    let base = FRAME_INSTRUCTIONS / lines;
    let remainder = FRAME_INSTRUCTIONS % lines;
    let line = line as u64;
    // `(line + 1) * remainder / lines - line * remainder / lines` is 0 or 1 and
    // places the remainder instructions evenly across the frame.
    base + (line + 1) * remainder / lines - line * remainder / lines
}

/// Guest instructions from the start of a frame until `line` is entered.
///
/// `instructions_until_line(0)` is `0` and `instructions_until_line(228)` is
/// [`FRAME_INSTRUCTIONS`].
///
/// # Panics
///
/// Panics when `line > FRAME_LINES`.
pub const fn instructions_until_line(line: u8) -> u64 {
    assert!(
        line <= FRAME_LINES,
        "line must not pass the end of the frame"
    );
    let lines = FRAME_LINES as u64;
    let base = FRAME_INSTRUCTIONS / lines;
    let remainder = FRAME_INSTRUCTIONS % lines;
    line as u64 * base + line as u64 * remainder / lines
}

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
    /// Writable DISPSTAT bits only; the VBlank flag is derived from the line.
    dispstat: u16,
    /// Current modelled `VCOUNT` line in `0..FRAME_LINES`.
    line: u8,
    /// Guest instructions left on the current line; model-internal.
    line_remaining: u64,
}

impl Default for IrqState {
    fn default() -> Self {
        Self {
            ie: 0,
            pending: 0,
            ime: 0,
            dispstat: 0,
            // A fresh bus starts on the first active line, not on a boundary,
            // with that line's full instruction budget still ahead.
            line: 0,
            line_remaining: line_instructions(0),
        }
    }
}

impl IrqState {
    fn in_vblank(&self) -> bool {
        (VBLANK_FIRST_LINE..=VBLANK_LAST_LINE).contains(&self.line)
    }

    pub(super) fn read16(&self, addr: u32) -> Option<u16> {
        Some(match addr {
            DISPSTAT_ADDR => {
                let flag = if self.in_vblank() {
                    DISPSTAT_VBLANK_FLAG
                } else {
                    0
                };
                self.dispstat | flag
            }
            VCOUNT_ADDR => u16::from(self.line),
            IE_ADDR => self.ie,
            IF_ADDR => self.pending,
            IME_ADDR => self.ime,
            _ => return None,
        })
    }

    /// Byte lanes of the modelled `VCOUNT` halfword only.
    ///
    /// GBATEK `#gba-io-map` and `#lcd-io-interrupts-and-status` describe I/O as
    /// 8/16/32-bit accessible and `VCOUNT` (`0x04000006`) as a readable 16-bit
    /// register whose bits 0..7 are the current scanline and bits 8..15 zero.
    /// This model reports line `0..=227`, so the high lane is always zero. The
    /// path handles only `VCOUNT`: byte-addressed sound-register reads predate
    /// this change and are unchanged, every other I/O address keeps the
    /// existing register-specific dispatch, and otherwise-unmodelled reads
    /// remain explicit unsupported-width errors.
    pub(super) fn read8(&self, addr: u32) -> Option<u8> {
        let vcount = self.read16(VCOUNT_ADDR)?;
        Some(match addr {
            VCOUNT_ADDR => vcount as u8,
            addr if addr == VCOUNT_ADDR + 1 => (vcount >> 8) as u8,
            _ => return None,
        })
    }

    pub(super) fn write16(&mut self, addr: u32, value: u16) -> Option<()> {
        match addr {
            // The V-Count setting (bits 8..15), its match flag (bit 2) and the
            // V-Counter IRQ are not modelled, so only bits 3..5 are retained.
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

    /// Advance one guest instruction and enter the next line when due.
    pub(super) fn tick(&mut self) {
        self.line_remaining -= 1;
        if self.line_remaining > 0 {
            return;
        }
        self.line = if self.line + 1 == FRAME_LINES {
            0
        } else {
            self.line + 1
        };
        self.line_remaining = line_instructions(self.line);
        if self.line == VBLANK_FIRST_LINE {
            self.pending |= IF_VBLANK;
        }
    }

    /// Latch requests from another modelled source, e.g. timer overflow.
    ///
    /// The bits keep the documented write-one-to-clear `IF` semantics; this
    /// only raises requests and never clears them.
    pub(super) fn latch_requests(&mut self, requests: u16) {
        self.pending |= requests & IE_MASK;
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
        // The current line is guest-visible through VCOUNT/DISPSTAT and is
        // hashed; the instruction counter that produces it is model-internal.
        bytes.push(self.line);
    }
}

impl Bus {
    /// Advance the frame and timer models by one guest instruction.
    pub fn tick_frame(&mut self) {
        self.io.irq_mut().tick();
        self.tick_timers();
    }

    /// Advance the frame and timer models by `count` guest instructions.
    pub fn tick_frames(&mut self, count: u64) {
        for _ in 0..count {
            self.tick_frame();
        }
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
    use crate::error::Error;

    fn bus() -> Bus {
        Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"))
    }

    fn vcount(bus: &Bus) -> u16 {
        bus.read16(VCOUNT_ADDR).expect("vcount")
    }

    fn vblank_flag(bus: &Bus) -> u16 {
        bus.read16(DISPSTAT_ADDR).expect("dispstat") & DISPSTAT_VBLANK_FLAG
    }

    fn vblank_request(bus: &Bus) -> u16 {
        bus.read16(IF_ADDR).expect("if") & IF_VBLANK
    }

    #[test]
    fn line_budgets_cover_the_frame_budget_exactly() {
        // Total frame budget is retained, not re-derived from the line count.
        assert_eq!(FRAME_INSTRUCTIONS, 93_632);
        assert_eq!(FRAME_LINES, 228);
        assert_eq!(VBLANK_FIRST_LINE, 160);
        assert_eq!(VBLANK_LAST_LINE, 226);
        assert_eq!(VBLANK_LAST_LINE + 1, FRAME_LINES - 1);

        let lines = u64::from(FRAME_LINES);
        let base = FRAME_INSTRUCTIONS / lines;
        let remainder = FRAME_INSTRUCTIONS % lines;
        assert_eq!((base, remainder), (410, 152));

        let mut sum = 0;
        let mut larger = 0;
        for line in 0..FRAME_LINES {
            let count = line_instructions(line);
            assert!(
                count == base || count == base + 1,
                "line {line} owns {count} instructions"
            );
            if count == base + 1 {
                larger += 1;
            }
            // The closed-form cumulative mapping agrees with each line budget.
            assert_eq!(
                instructions_until_line(line + 1) - instructions_until_line(line),
                count,
                "cumulative mapping for line {line}"
            );
            sum += count;
        }
        assert_eq!(sum, FRAME_INSTRUCTIONS, "no remainder may be lost");
        assert_eq!(larger, remainder, "every remainder instruction is placed");
        assert_eq!(instructions_until_line(0), 0);
        assert_eq!(instructions_until_line(FRAME_LINES), FRAME_INSTRUCTIONS);
    }

    #[test]
    fn vcount_walks_every_line_once_per_frame_and_wraps_to_zero() {
        let mut bus = bus();
        assert_eq!(vcount(&bus), 0, "a fresh bus starts on line 0");

        let mut ticked = 0;
        for line in 0..FRAME_LINES {
            bus.tick_frames(instructions_until_line(line) - ticked);
            ticked = instructions_until_line(line);
            assert_eq!(vcount(&bus), u16::from(line), "first tick of line {line}");

            let last = instructions_until_line(line + 1) - 1;
            bus.tick_frames(last - ticked);
            ticked = last;
            assert_eq!(vcount(&bus), u16::from(line), "last tick of line {line}");
        }
        assert_eq!(ticked, FRAME_INSTRUCTIONS - 1);
        let before_wrap = bus.state_digest();
        bus.tick_frame();
        assert_eq!(vcount(&bus), 0, "line 227 wraps to line 0");
        assert_ne!(before_wrap, bus.state_digest(), "line state is hashed");

        // The next frame repeats the same deterministic line sequence.
        bus.tick_frames(instructions_until_line(VBLANK_FIRST_LINE));
        assert_eq!(vcount(&bus), u16::from(VBLANK_FIRST_LINE));
        assert_eq!(vblank_flag(&bus), DISPSTAT_VBLANK_FLAG);
        assert_eq!(vblank_request(&bus), IF_VBLANK);
        bus.tick_frames(FRAME_INSTRUCTIONS - instructions_until_line(VBLANK_FIRST_LINE));
        assert_eq!(vcount(&bus), 0, "a whole frame ends on line 0");
    }

    #[test]
    fn frame_boundary_sets_the_vblank_flag_and_request() {
        let mut bus = bus();
        let first_vblank = instructions_until_line(VBLANK_FIRST_LINE);
        bus.tick_frames(first_vblank - 1);
        assert_eq!(vcount(&bus), u16::from(VBLANK_FIRST_LINE) - 1);
        assert_eq!(vblank_flag(&bus), 0);
        assert_eq!(vblank_request(&bus), 0);

        bus.tick_frame();
        assert_eq!(vcount(&bus), u16::from(VBLANK_FIRST_LINE));
        assert_eq!(vblank_flag(&bus), DISPSTAT_VBLANK_FLAG);
        assert_eq!(vblank_request(&bus), IF_VBLANK);

        // Acknowledging the request clears IF but not the flag.
        bus.write16(IF_ADDR, IF_VBLANK).expect("acknowledge");
        bus.tick_frames(instructions_until_line(VBLANK_LAST_LINE) - first_vblank);
        assert_eq!(vcount(&bus), u16::from(VBLANK_LAST_LINE));
        assert_eq!(vblank_flag(&bus), DISPSTAT_VBLANK_FLAG);
        assert_eq!(vblank_request(&bus), 0, "one request per frame");

        // Hidden line 227 clears the VBlank flag; the wrap raises nothing.
        bus.tick_frames(
            instructions_until_line(FRAME_LINES - 1) - instructions_until_line(VBLANK_LAST_LINE),
        );
        assert_eq!(vcount(&bus), u16::from(FRAME_LINES - 1));
        assert_eq!(vblank_flag(&bus), 0, "line 227 is not VBlank");
        assert_eq!(vblank_request(&bus), 0);

        bus.tick_frames(FRAME_INSTRUCTIONS - instructions_until_line(FRAME_LINES - 1));
        assert_eq!(vcount(&bus), 0);
        assert_eq!(vblank_flag(&bus), 0);
        assert_eq!(vblank_request(&bus), 0, "the wrap raises no request");

        bus.tick_frames(instructions_until_line(VBLANK_FIRST_LINE));
        assert_eq!(vcount(&bus), u16::from(VBLANK_FIRST_LINE));
        assert_eq!(vblank_request(&bus), IF_VBLANK, "next frame requests again");
    }

    #[test]
    fn irq_requires_ime_ie_and_a_pending_bit() {
        let mut bus = bus();
        bus.write16(IE_ADDR, 1).expect("ie");
        assert!(!bus.irq_pending(), "IME still disabled");
        bus.write16(IME_ADDR, 1).expect("ime");
        assert!(!bus.irq_pending(), "nothing pending yet");
        bus.tick_frames(instructions_until_line(VBLANK_FIRST_LINE));
        assert!(bus.irq_pending());
        bus.write16(IF_ADDR, 1).expect("clear");
        assert!(!bus.irq_pending(), "write-one-to-clear");
    }

    #[test]
    fn batch_and_single_instruction_ticks_stay_equivalent() {
        let first_vblank = instructions_until_line(VBLANK_FIRST_LINE);
        let counts = [
            0,
            1,
            line_instructions(0),
            first_vblank - 1,
            first_vblank,
            first_vblank + 1,
            instructions_until_line(VBLANK_LAST_LINE + 1),
            FRAME_INSTRUCTIONS - 1,
            FRAME_INSTRUCTIONS,
            FRAME_INSTRUCTIONS + first_vblank,
        ];
        for count in counts {
            let mut batched = bus();
            batched.tick_frames(count);
            let mut single = bus();
            for _ in 0..count {
                single.tick_frame();
            }
            assert_eq!(
                batched.state_digest(),
                single.state_digest(),
                "batch of {count} instructions"
            );
            for addr in [DISPSTAT_ADDR, VCOUNT_ADDR, IF_ADDR] {
                assert_eq!(
                    batched.read16(addr),
                    single.read16(addr),
                    "register {addr:#010x} after {count} instructions"
                );
            }
        }
    }

    #[test]
    fn vcount_byte_lanes_track_every_modelled_line() {
        let probe = bus();
        assert_eq!(probe.read8(VCOUNT_ADDR).expect("line 0 low lane"), 0);
        assert_eq!(probe.read8(VCOUNT_ADDR + 1).expect("line 0 high lane"), 0);
        assert_eq!(probe.read16(VCOUNT_ADDR).expect("vcount"), 0);

        for line in [VBLANK_FIRST_LINE, VBLANK_LAST_LINE, FRAME_LINES - 1] {
            let mut bus = bus();
            bus.tick_frames(instructions_until_line(line));
            assert_eq!(
                bus.read8(VCOUNT_ADDR).expect("low lane"),
                line,
                "low byte of line {line}"
            );
            assert_eq!(
                bus.read8(VCOUNT_ADDR + 1).expect("high lane"),
                0,
                "line {line} never reaches the high byte"
            );
            assert_eq!(bus.read16(VCOUNT_ADDR).expect("vcount"), u16::from(line));
        }

        // The byte path covers only the two lanes above. These selected
        // addresses have no byte path, so they remain explicit width errors
        // (unlike pre-existing sound-register byte reads).
        let bus = bus();
        for addr in [DISPSTAT_ADDR, DISPSTAT_ADDR + 1, VCOUNT_ADDR + 2, IF_ADDR] {
            assert!(
                matches!(
                    bus.read8(addr),
                    Err(Error::UnsupportedIoRead { width: 8, .. })
                ),
                "{addr:#010x} must stay an unsupported 8-bit I/O read"
            );
        }
    }

    #[test]
    fn dispstat_keeps_writable_bits_and_derives_the_flag() {
        let mut bus = bus();
        bus.write16(DISPSTAT_ADDR, 0xFFFF).expect("dispstat");
        // Only the documented enable bits 3..5 are stored; the V-Count setting
        // (bits 8..15) and the V-Counter match flag stay unmodelled.
        assert_eq!(bus.read16(DISPSTAT_ADDR).expect("dispstat"), 0x0038);
    }
}
