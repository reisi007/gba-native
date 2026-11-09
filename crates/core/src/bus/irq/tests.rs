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
fn hblank_flag_and_irq_follow_exact_boundaries_on_all_lines() {
    for line in [0, VBLANK_FIRST_LINE, FRAME_LINES - 1] {
        let mut line_bus = bus();
        line_bus.tick_cycles(u32::from(line) * CYCLES_PER_LINE);
        line_bus
            .write16(DISPSTAT_ADDR, 1 << 4)
            .expect("HBlank IRQ enable");
        line_bus.tick_cycles(CYCLES_TO_HBLANK - 1);
        assert_eq!(line_bus.read16(DISPSTAT_ADDR).unwrap() & 2, 0);
        assert_eq!(line_bus.read16(IF_ADDR).unwrap() & IF_HBLANK, 0);
        line_bus.tick_cycles(1);
        assert_eq!(line_bus.read16(DISPSTAT_ADDR).unwrap() & 2, 2);
        assert_eq!(line_bus.read16(IF_ADDR).unwrap() & IF_HBLANK, IF_HBLANK);
        line_bus.write16(IF_ADDR, IF_HBLANK).unwrap();
        line_bus.tick_cycles(CYCLES_PER_LINE - CYCLES_TO_HBLANK);
        assert_eq!(line_bus.read16(DISPSTAT_ADDR).unwrap() & 2, 0);
        assert_eq!(line_bus.read16(IF_ADDR).unwrap() & IF_HBLANK, 0);
    }
}

#[test]
fn hblank_flag_is_derived_enable_is_writable_and_batched_ticks_match() {
    let mut batched = bus();
    let mut split = bus();
    for candidate in [&mut batched, &mut split] {
        candidate.write16(DISPSTAT_ADDR, 0xFFFF).unwrap();
        assert_eq!(candidate.read16(DISPSTAT_ADDR).unwrap() & 0x0012, 0x0010);
    }
    batched.tick_cycles(CYCLES_TO_HBLANK + CYCLES_PER_LINE + 7);
    split.tick_cycles(CYCLES_TO_HBLANK);
    split.tick_cycles(CYCLES_PER_LINE);
    split.tick_cycles(7);
    assert_eq!(batched.state_digest(), split.state_digest());
    assert_eq!(batched.read16(DISPSTAT_ADDR), split.read16(DISPSTAT_ADDR));
    assert_eq!(batched.read16(IF_ADDR), split.read16(IF_ADDR));
    assert_eq!(batched.read16(IF_ADDR).unwrap() & IF_HBLANK, IF_HBLANK);
}

#[test]
fn nominal_instruction_helpers_round_cycle_boundaries_across_frame() {
    // Compatibility instruction count is derived from the documented cycle total.
    assert_eq!(FRAME_CYCLES, 280_896);
    assert_eq!(CYCLES_PER_LINE * u32::from(FRAME_LINES), FRAME_CYCLES);
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
        // The rounded cumulative mapping agrees with each per-line delta.
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
fn cycle_scheduler_uses_exact_boundaries_carries_overrun_and_wraps_frame() {
    let mut scheduled = bus();
    assert_eq!((CYCLES_PER_LINE, FRAME_CYCLES), (1232, 280_896));
    scheduled.tick_cycles(CYCLES_PER_LINE - 1);
    assert_eq!(vcount(&scheduled), 0);
    scheduled.tick_cycles(1);
    assert_eq!(vcount(&scheduled), 1);

    // Phase is zero on line 1; 1231 cycles plus one three-cycle instruction
    // crosses the next edge with two cycles carried into line 2.
    scheduled.tick_cycles(CYCLES_PER_LINE - 1);
    scheduled.tick_cycles(3);
    assert_eq!(vcount(&scheduled), 2);
    // Two overrun cycles are already in line 2: only 1230 more reach line 3.
    scheduled.tick_cycles(CYCLES_PER_LINE - 3);
    assert_eq!(vcount(&scheduled), 2);
    scheduled.tick_cycles(1);
    assert_eq!(vcount(&scheduled), 3);

    let mut exact = bus();
    exact.tick_cycles(FRAME_CYCLES - 1);
    assert_eq!(vcount(&exact), u16::from(FRAME_LINES - 1));
    exact.tick_cycles(1);
    assert_eq!(vcount(&exact), 0);
    assert_eq!(exact.read16(IF_ADDR), Ok(IF_VBLANK));
}

#[test]
fn arbitrary_cycle_batches_equal_single_cycle_advancement() {
    let mut batched = bus();
    let mut single = bus();
    batched.tick_cycles(8_765);
    for _ in 0..8_765 {
        single.tick_cycles(1);
    }
    assert_eq!(batched.state_digest(), single.state_digest());
    assert_eq!(vcount(&batched), vcount(&single));

    let mut phase_one = bus();
    phase_one.tick_cycles(1);
    let mut phase_two = bus();
    phase_two.tick_cycles(2);
    assert_eq!(vcount(&phase_one), vcount(&phase_two));
    assert_ne!(
        phase_one.state_digest(),
        phase_two.state_digest(),
        "different LCD phases must remain distinguishable"
    );
}

#[test]
fn coincident_lcd_and_timer_events_latch_all_irq_requests() {
    let mut bus = bus();
    bus.tick_cycles(u32::from(VBLANK_FIRST_LINE - 1) * CYCLES_PER_LINE + CYCLES_PER_LINE - 1);
    assert_eq!(vcount(&bus), u16::from(VBLANK_FIRST_LINE - 1));
    bus.write16(DISPSTAT_ADDR, 0xA020)
        .expect("enable VCOUNT IRQ for line 160");
    assert_eq!(bus.read16(IF_ADDR), Ok(0));

    // Timer 0 starts at 0xFFFF with F/1 and its IRQ enabled, so its first
    // supplied cycle overflows on the same cycle that enters VBlank.
    bus.write16(0x0400_0100, u16::MAX).expect("TM0 reload");
    bus.write16(0x0400_0102, 0x00C0)
        .expect("TM0 start with IRQ");
    assert_eq!(bus.read16(0x0400_0100), Ok(u16::MAX));
    assert_eq!(bus.read16(0x0400_0102), Ok(0x00C0));
    bus.tick_cycles(1);

    assert_eq!(vcount(&bus), u16::from(VBLANK_FIRST_LINE));
    assert_eq!(
        bus.read16(DISPSTAT_ADDR).expect("DISPSTAT") & 0x0005,
        0x0005,
        "VBlank and matching VCOUNT flags become visible together"
    );
    assert_eq!(
        bus.read16(IF_ADDR),
        Ok(IF_VBLANK | IF_VCOUNT | (1 << 3)),
        "VBlank, VCOUNT and timer requests are all retained"
    );
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
fn vcount_match_bit_is_read_only_and_acknowledges_through_if() {
    let mut bus = bus();
    // Of the read-only bits 0..2 the value sets only bit 2; bit 8 sets LYC = 1,
    // so the stored value is 0x0100 with no enable bit. Line 0 does not match
    // and the derived flag stays clear.
    bus.write16(DISPSTAT_ADDR, 0x0104).expect("dispstat");
    assert_eq!(bus.read16(DISPSTAT_ADDR).expect("dispstat") & 0x0004, 0);
    assert_eq!(
        bus.read16(DISPSTAT_ADDR).expect("dispstat") & 0xFF00,
        0x0100
    );
    // LYC = 0 plus the enable bit raises the documented request on line 0.
    bus.write16(DISPSTAT_ADDR, 0x0020).expect("dispstat");
    assert_eq!(bus.read16(IF_ADDR), Ok(0x0004));
    // Write-one-to-clear acknowledges the request; the match flag is derived.
    bus.write16(IF_ADDR, 0x0004).expect("ack");
    assert_eq!(bus.read16(IF_ADDR), Ok(0));
    assert_eq!(
        bus.read16(DISPSTAT_ADDR).expect("dispstat") & 0x0004,
        0x0004
    );
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
