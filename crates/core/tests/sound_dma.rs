//! Bounded, host-triggered Sound FIFO DMA regressions.
//!
//! Fixtures use synthetic RAM and register writes only; no ROM or BIOS image is
//! involved. No timer, sample clock, audio output or automatic DMA event exists.

use gba_native_core::bus::{Bus, EWRAM_BASE, FIFO_A_ADDR, FIFO_B_ADDR, SoundFifo};
use gba_native_core::cart::Cartridge;
use gba_native_core::error::Error;

const DMA_BASE: u32 = 0x0400_00B0;
const DMA_STRIDE: u32 = 12;
const ENABLE_SPECIAL_REPEAT_HALF_FIXED: u16 = 0xB240;

fn fresh() -> Bus {
    Bus::new(Cartridge::from_bytes(vec![0; 0x200]).expect("test cart"))
}

fn configure_dma(bus: &mut Bus, channel: u32, source: u32, destination: u32, count: u16) {
    let base = DMA_BASE + channel * DMA_STRIDE;
    bus.write32(base, source).expect("source registers");
    bus.write32(base + 4, destination)
        .expect("destination registers");
    bus.write16(base + 8, count).expect("count");
}

#[test]
fn special_enable_arms_without_reading_zero_and_bad_destination_fails_on_request() {
    let mut bus = fresh();
    let dma2 = DMA_BASE + 2 * DMA_STRIDE;
    bus.write32(dma2, 0).expect("zero source");
    bus.write32(dma2 + 4, 0).expect("zero destination");
    bus.write16(dma2 + 8, 0).expect("zero count");

    // The observed guest value: enable + special + repeat + 32-bit mode.
    bus.write16(dma2 + 10, 0xB600)
        .expect("arm DMA2 without an immediate transfer");
    assert_eq!(bus.read16(dma2 + 10), Ok(0xB600));
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 0);
    assert_eq!(bus.sound_fifo_len(SoundFifo::B), 0);

    assert!(matches!(
        bus.request_sound_fifo_dma(SoundFifo::A),
        Err(Error::UnsupportedDma { .. })
    ));
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 0);
}

#[test]
fn explicit_requests_transfer_four_words_and_advance_only_the_hidden_source_cursor() {
    let mut bus = fresh();
    let source = EWRAM_BASE + 0x100;
    let dma1 = DMA_BASE + DMA_STRIDE;
    for index in 0..8u32 {
        bus.write32(source + index * 4, 0x0302_0100 + index * 0x0404_0404)
            .expect("seed source words");
    }
    // DMA1 feeds FIFO B here: no fixed DMA1/A versus DMA2/B pairing is assumed.
    // CNT_L=1 and halfword width are deliberately ignored in special sound mode.
    configure_dma(&mut bus, 1, source, FIFO_B_ADDR, 1);
    bus.write16(dma1 + 10, ENABLE_SPECIAL_REPEAT_HALF_FIXED)
        .expect("arm DMA1 for FIFO B");
    let armed_digest = bus.state_digest();
    let visible_source = bus.read32(dma1).expect("SAD");
    let visible_count = bus.read16(dma1 + 8).expect("CNT_L");
    let visible_control = bus.read16(dma1 + 10).expect("CNT_H");
    // The visible DAD latch may be rewritten, but the active transfer retains
    // the destination captured on the enable edge.
    bus.write32(dma1 + 4, FIFO_A_ADDR)
        .expect("rewrite visible DAD while enabled");

    bus.request_sound_fifo_dma(SoundFifo::A)
        .expect("unmatched FIFO request is idle");
    assert_eq!(bus.sound_fifo_len(SoundFifo::B), 0);
    bus.request_sound_fifo_dma(SoundFifo::B)
        .expect("first explicit FIFO request");
    bus.request_sound_fifo_dma(SoundFifo::B)
        .expect("second explicit FIFO request");
    assert_eq!(bus.sound_fifo_len(SoundFifo::B), 32);
    assert_eq!(bus.read32(dma1), Ok(visible_source));
    assert_eq!(bus.read32(dma1 + 4), Ok(FIFO_A_ADDR));
    assert_eq!(bus.read16(dma1 + 8), Ok(visible_count));
    assert_eq!(bus.read16(dma1 + 10), Ok(visible_control));
    assert!(matches!(
        bus.request_sound_fifo_dma(SoundFifo::B),
        Err(Error::SoundFifoFull {
            addr: FIFO_B_ADDR,
            needed: 16,
            available: 0,
        })
    ));
    assert_eq!(bus.sound_fifo_len(SoundFifo::B), 32);

    for expected in 0..32u8 {
        assert_eq!(
            bus.pop_sound_fifo_sample(SoundFifo::B),
            Some(expected as i8),
            "sample byte {expected}"
        );
    }
    assert_eq!(bus.pop_sound_fifo_sample(SoundFifo::B), None);
    assert_ne!(
        bus.state_digest(),
        armed_digest,
        "the advanced internal DMA source cursor is guest-relevant model state"
    );
}

#[test]
fn fifo_word_writes_enforce_capacity_and_sample_pop_wraps_the_queue() {
    let mut bus = fresh();
    let empty_digest = bus.state_digest();
    assert!(matches!(
        bus.read32(FIFO_A_ADDR),
        Err(Error::UnsupportedIoRead { .. })
    ));
    for index in 0..8u32 {
        bus.write32(FIFO_A_ADDR, 0x0302_0100 + index * 0x0404_0404)
            .expect("fill 32-byte FIFO");
    }
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 32);
    assert_ne!(bus.state_digest(), empty_digest);
    assert!(matches!(
        bus.write32(FIFO_A_ADDR, 0xFFFF_FFFF),
        Err(Error::SoundFifoFull {
            addr: FIFO_A_ADDR,
            needed: 4,
            available: 0,
        })
    ));

    for expected in 0..4u8 {
        assert_eq!(
            bus.pop_sound_fifo_sample(SoundFifo::A),
            Some(expected as i8)
        );
    }
    bus.write32(FIFO_A_ADDR, 0xA3A2_A1A0)
        .expect("write through ring wrap");
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 32);
    for expected in 4..32u8 {
        assert_eq!(
            bus.pop_sound_fifo_sample(SoundFifo::A),
            Some(expected as i8)
        );
    }
    for expected in [0xA0_u8, 0xA1, 0xA2, 0xA3] {
        assert_eq!(
            bus.pop_sound_fifo_sample(SoundFifo::A),
            Some(expected as i8)
        );
    }
    assert_eq!(bus.state_digest(), empty_digest);
}

#[test]
fn unsupported_sound_dma_shapes_fail_before_source_access() {
    let mut bus = fresh();
    let dma1 = DMA_BASE + DMA_STRIDE;
    configure_dma(&mut bus, 1, 0, FIFO_A_ADDR, 1);

    for control in [
        0xB040, // no repeat
        0xB2C0, // decrementing source
        0xB220, // incrementing FIFO destination
    ] {
        bus.write16(dma1 + 10, control)
            .expect("arm supported timing");
        assert!(matches!(
            bus.request_sound_fifo_dma(SoundFifo::A),
            Err(Error::UnsupportedDma { .. })
        ));
        assert_eq!(bus.sound_fifo_len(SoundFifo::A), 0);
        bus.write16(dma1 + 10, 0).expect("disable channel");
    }

    configure_dma(&mut bus, 1, 0, FIFO_A_ADDR, 1);
    configure_dma(&mut bus, 2, 0, FIFO_A_ADDR, 1);
    bus.write16(dma1 + 10, ENABLE_SPECIAL_REPEAT_HALF_FIXED)
        .expect("arm DMA1");
    bus.write16(dma1 + DMA_STRIDE + 10, ENABLE_SPECIAL_REPEAT_HALF_FIXED)
        .expect("arm DMA2");
    assert!(matches!(
        bus.request_sound_fifo_dma(SoundFifo::A),
        Err(Error::UnsupportedDma { .. })
    ));
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 0);
}

#[test]
fn special_video_or_dma0_timing_is_not_run_as_an_immediate_transfer() {
    let mut bus = fresh();
    for channel in [0, 3] {
        let base = DMA_BASE + channel * DMA_STRIDE;
        bus.write32(base, 0).expect("source zero");
        bus.write32(base + 4, EWRAM_BASE).expect("destination RAM");
        bus.write16(base + 8, 1).expect("count");
        assert!(matches!(
            bus.write16(base + 10, 0xB400),
            Err(Error::UnsupportedDma { .. })
        ));
        assert_eq!(bus.read32(EWRAM_BASE), Ok(0));
    }
}

#[test]
fn sound_control_fifo_reset_bits_clear_only_the_selected_fifo() {
    let mut bus = fresh();
    bus.write32(FIFO_A_ADDR, 0x0403_0201).expect("FIFO A");
    bus.write32(FIFO_B_ADDR, 0x0807_0605).expect("FIFO B");

    bus.write16(0x0400_0082, 1 << 11).expect("reset FIFO A");
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 0);
    assert_eq!(bus.sound_fifo_len(SoundFifo::B), 4);
    bus.write16(0x0400_0082, 1 << 15).expect("reset FIFO B");
    assert_eq!(bus.sound_fifo_len(SoundFifo::B), 0);
}

#[test]
fn group_seven_reset_disarms_dma_but_preserves_queued_sound_data() {
    let mut bus = fresh();
    let source = EWRAM_BASE + 0x200;
    let dma2 = DMA_BASE + 2 * DMA_STRIDE;
    for index in 0..4u32 {
        bus.write32(source + index * 4, index + 1)
            .expect("seed source");
    }
    configure_dma(&mut bus, 2, source, FIFO_A_ADDR, 4);
    bus.write16(dma2 + 10, ENABLE_SPECIAL_REPEAT_HALF_FIXED)
        .expect("arm DMA2");
    bus.request_sound_fifo_dma(SoundFifo::A)
        .expect("first burst");
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 16);

    bus.reset_group7_io();
    bus.request_sound_fifo_dma(SoundFifo::A)
        .expect("reset removed the armed DMA channel");
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 16);
    assert_eq!(bus.read16(dma2 + 10), Ok(0));
}
