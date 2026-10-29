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
fn dispstat_keeps_writable_bits_and_derives_the_flags() {
    let mut bus = bus();
    bus.write16(DISPSTAT_ADDR, 0xFFFF).expect("dispstat");
    // Bits 3..5 and the V-Count setting in bits 8..15 are writable; both
    // read-only flags are derived. LYC=255 does not match line 0, so only
    // the V-Blank-related bits are absent as well.
    assert_eq!(bus.read16(DISPSTAT_ADDR).expect("dispstat"), 0xFF38);
}

#[test]
fn vcount_setting_sets_the_match_flag_and_requests_irq() {
    let mut bus = bus();
    // LYC = 5 with the V-Counter IRQ enabled and no other enable bits.
    bus.write16(DISPSTAT_ADDR, 0x0520).expect("dispstat");
    assert_eq!(bus.read16(IF_ADDR), Ok(0));
    // Walk to line 5: the flag and the request appear exactly there.
    bus.tick_frames(instructions_until_line(5));
    assert_eq!(bus.read16(VCOUNT_ADDR), Ok(5));
    assert_eq!(
        bus.read16(DISPSTAT_ADDR).expect("dispstat") & 0x0004,
        0x0004
    );
    assert_eq!(bus.read16(IF_ADDR), Ok(0x0004));
    // One more line clears the derived flag again.
    bus.tick_frames(line_instructions(5));
    assert_eq!(bus.read16(VCOUNT_ADDR), Ok(6));
    assert_eq!(bus.read16(DISPSTAT_ADDR).expect("dispstat") & 0x0004, 0);
}

#[test]
fn writing_a_matching_vcount_setting_requests_immediately() {
    let mut bus = bus();
    // Line 0 is current, so writing LYC = 0 with the IRQ enabled requests
    // the documented V-Counter interrupt without waiting for a new line.
    bus.write16(DISPSTAT_ADDR, 0x0020).expect("dispstat");
    assert_eq!(bus.read16(IF_ADDR), Ok(0x0004));
    // Without the enable bit there is no request, only the read-only flag.
    let mut clean = self::bus();
    clean.write16(DISPSTAT_ADDR, 0x0000).expect("dispstat");
    assert_eq!(clean.read16(IF_ADDR), Ok(0));
    assert_eq!(
        clean.read16(DISPSTAT_ADDR).expect("dispstat") & 0x0004,
        0x0004
    );
}
