use super::*;

#[test]
fn envelope_trigger_is_a_write_event_and_retriggers_coalesce_to_latest() {
    let mut bus = Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"));
    bus.write16(0x0400_0064, 0x0000)
        .expect("non-trigger register write");
    assert_eq!(bus.sound_envelope_triggers()[0].sequence, 0);
    // Channel 1 envelope: initial 14, increase, period 2.
    bus.write16(0x0400_0062, 0xEA00).expect("envelope");
    bus.write8(0x0400_0065, 0x80).expect("byte trigger");
    let first = bus.sound_envelope_triggers()[0];
    assert_eq!(
        (
            first.sequence,
            first.initial_volume,
            first.increase,
            first.period
        ),
        (1, 14, true, 2)
    );
    // Identical trigger write is another event; intervening envelope changes are
    // captured at the write, not reconstructed from the later register snapshot.
    bus.write16(0x0400_0062, 0x3100).expect("new envelope");
    assert_eq!(bus.sound_envelope_triggers()[0].initial_volume, 14);
    bus.write16(0x0400_0064, 0x8000).expect("repeat trigger");
    let latest = bus.sound_envelope_triggers()[0];
    assert_eq!(
        (
            latest.sequence,
            latest.initial_volume,
            latest.increase,
            latest.period
        ),
        (2, 3, false, 1)
    );
    assert_ne!(
        bus.state_digest(),
        {
            let mut twin = Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"));
            twin.write16(0x0400_0062, 0x3100)
                .expect("same final registers");
            twin.write16(0x0400_0064, 0x8000).expect("one trigger");
            twin.state_digest()
        },
        "captured repeated-trigger state affects future audio"
    );
}

#[test]
fn envelope_byte_halfword_word_writes_cover_channels_one_two_and_four() {
    let mut bus = Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"));
    bus.write16(0x0400_0062, 0xF100)
        .expect("channel 1 envelope");
    bus.write16(0x0400_0064, 0x8000)
        .expect("channel 1 halfword trigger");
    bus.write16(0x0400_0068, 0x4800)
        .expect("channel 2 envelope");
    bus.write8(0x0400_006D, 0x80)
        .expect("channel 2 byte trigger");
    bus.write16(0x0400_0078, 0xC800)
        .expect("channel 4 envelope");
    bus.write32(0x0400_007C, 0x0000_8000)
        .expect("channel 4 word trigger");
    let events = bus.sound_envelope_triggers();
    assert_eq!((events[0].sequence, events[0].initial_volume), (1, 15));
    assert_eq!((events[1].sequence, events[1].initial_volume), (1, 4));
    assert_eq!((events[2].sequence, events[2].initial_volume), (1, 12));
}

#[test]
fn envelope_no_sample_observation_and_reset_are_independent_per_engine() {
    let mut bus = Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"));
    bus.write16(0x0400_0062, 0xF100).expect("envelope");
    bus.write16(0x0400_0064, 0x8000).expect("trigger");
    let mut first = AudioEngine::new(AudioSettings::new(100, false));
    let mut second = AudioEngine::new(AudioSettings::new(100, false));
    first
        .advance_instruction_bus(&mut bus)
        .expect("first observes");
    assert!(
        first.samples().is_empty(),
        "instruction is short of a sample"
    );
    assert_eq!(first.psg.envelopes[0].expect("trigger latched").volume, 15);
    second
        .advance_instruction_bus(&mut bus)
        .expect("second observes independently");
    assert_eq!(second.psg.envelopes[0].expect("trigger latched").volume, 15);
    bus.reset_group6_sound();
    first
        .advance_instruction_bus(&mut bus)
        .expect("observe reset");
    assert!(first.psg.envelopes[0].is_none());
}

#[test]
fn bus_backed_default_master_disabled_stays_silent_while_latching_trigger() {
    let mut bus = Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"));
    bus.write16(0x0400_0062, 0xF100).expect("envelope");
    bus.write16(0x0400_0064, 0x8000).expect("trigger");
    let mut engine = AudioEngine::new(AudioSettings::default());
    engine.advance_bus(&mut bus, 171).expect("advance");
    assert_eq!(engine.samples(), &[0]);
    assert_eq!(engine.psg.envelopes[0].unwrap().volume, 15);
}

#[test]
fn envelope_steps_saturate_in_both_directions_and_period_zero_is_static() {
    let mut mixer = PsgMixer::new();
    let events = |sequence, initial_volume, increase, period| {
        let event = crate::bus::EnvelopeTrigger {
            sequence,
            triggered: true,
            initial_volume,
            increase,
            period,
        };
        [event; 3]
    };
    mixer.observe_envelopes(events(1, 15, true, 1));
    mixer.advance_envelopes(512);
    assert_eq!(mixer.envelopes[0].unwrap().volume, 15);
    mixer.observe_envelopes(events(2, 0, false, 1));
    mixer.advance_envelopes(512);
    assert_eq!(mixer.envelopes[0].unwrap().volume, 0);
    mixer.observe_envelopes(events(3, 8, false, 1));
    mixer.advance_envelopes(512);
    assert_eq!(mixer.envelopes[0].unwrap().volume, 7);
    mixer.observe_envelopes(events(4, 8, true, 0));
    mixer.advance_envelopes(10_000);
    assert_eq!(mixer.envelopes[0].unwrap().volume, 8);
}

#[test]
fn envelope_batch_observation_uses_latest_trigger_only() {
    let mut bus = Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"));
    bus.write16(0x0400_0062, 0x2100).expect("first params");
    bus.write16(0x0400_0064, 0x8000).expect("first trigger");
    bus.write16(0x0400_0062, 0xC100).expect("second params");
    bus.write16(0x0400_0064, 0x8000).expect("second trigger");
    let mut engine = AudioEngine::new(AudioSettings::new(100, false));
    engine
        .advance_bus(&mut bus, 171)
        .expect("batch observes once");
    assert_eq!(engine.psg.envelopes[0].unwrap().volume, 12);
    assert!(!engine.psg.envelopes[0].unwrap().increase);
}

#[test]
fn envelope_advances_after_the_boundary_sample_not_before_the_batch() {
    let make_bus = |period: u8| {
        let mut bus = Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"));
        bus.write16(0x0400_0062, 0x1900 | (u16::from(period) << 8))
            .expect("envelope");
        bus.write16(0x0400_0064, 0x8000).expect("trigger");
        bus.write16(0x0400_0064, 0x8000 | 1700)
            .expect("low square pitch");
        bus.write16(0x0400_0080, 0x0107).expect("PSG volume");
        bus.write16(0x0400_0084, 0x0080).expect("master enable");
        bus
    };
    // period 1 is exactly 512 rendered samples.
    let mut stepping = AudioEngine::new(AudioSettings::new(100, false));
    let mut bus = make_bus(1);
    stepping
        .advance_bus(&mut bus, 87_211)
        .expect("render first 511 samples");
    assert_eq!(stepping.samples().len(), 511);
    assert_eq!(stepping.psg.envelopes[0].unwrap().volume, 1);
    stepping
        .advance_bus(&mut bus, 171)
        .expect("render boundary sample");
    assert_eq!(stepping.samples().len(), 512);
    assert_eq!(stepping.psg.envelopes[0].unwrap().volume, 2);
}

#[test]
fn envelope_sample_output_is_chunk_equivalent_for_same_observation() {
    let make_bus = || {
        let mut bus = Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"));
        bus.write16(0x0400_0062, 0xF100).expect("envelope");
        bus.write16(0x0400_0064, 0x8000).expect("trigger");
        bus.write16(0x0400_0064, 0x8000).expect("repeat");
        bus.write16(0x0400_0062, 0xF080).expect("square duty");
        bus.write16(0x0400_0064, 0x8000 | 1750)
            .expect("square pitch");
        bus.write16(0x0400_0080, 0x0107).expect("PSG volume");
        bus.write16(0x0400_0084, 0x0080).expect("master enable");
        bus
    };
    let mut batched = make_bus();
    let mut stepped = make_bus();
    let mut batch_engine = AudioEngine::new(AudioSettings::new(100, false));
    let mut step_engine = AudioEngine::new(AudioSettings::new(100, false));
    batch_engine.advance_bus(&mut batched, 4096).expect("batch");
    for _ in 0..4096 {
        step_engine
            .advance_instruction_bus(&mut stepped)
            .expect("instruction");
    }
    assert_eq!(batch_engine.samples(), step_engine.samples());
}
