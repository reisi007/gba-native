use super::super::{AudioEngine, AudioSettings, PsgMixer};
use crate::bus::Bus;
use crate::cart::Cartridge;

pub(super) fn restart_bit_does_not_act_as_a_square_channel_enable() {
    for (route, envelope_offset, frequency_offset) in
        [(0x0107u16, 0x02, 0x04), (0x0207, 0x08, 0x0C)]
    {
        let mut bus = Bus::new(Cartridge::from_bytes(vec![0; 0x200]).expect("cart"));
        bus.write16(0x0400_0080, route)
            .expect("route square channel");
        bus.write16(0x0400_0084, 0x0080).expect("enable sound");
        bus.write16(0x0400_0060 + envelope_offset, 0xF080)
            .expect("set square envelope");
        let mut engine = AudioEngine::new(AudioSettings::new(100, false));

        engine
            .advance_bus(&mut bus, 171)
            .expect("observe inactive channel");
        let before_trigger = engine.samples().len();
        assert_eq!(engine.samples()[before_trigger - 1], 0);

        bus.write16(0x0400_0060 + frequency_offset, 0x8000 | 1750)
            .expect("trigger square channel");
        engine.advance_bus(&mut bus, 171).expect("observe trigger");
        let triggered_sample = engine.samples()[before_trigger];
        assert_ne!(triggered_sample, 0, "trigger should activate the channel");

        bus.write16(0x0400_0060 + frequency_offset, 1750)
            .expect("write frequency without restart");
        engine
            .advance_bus(&mut bus, 171)
            .expect("render after non-trigger write");
        assert_ne!(
            engine.samples()[before_trigger + 1],
            0,
            "a non-trigger write must not stop an active channel"
        );

        bus.reset_group6_sound();
        bus.write16(0x0400_0080, route)
            .expect("restore channel routing");
        bus.write16(0x0400_0084, 0x0080)
            .expect("restore sound enable");
        engine
            .advance_bus(&mut bus, 171)
            .expect("observe sound reset");
        assert_eq!(engine.samples().last(), Some(&0));
    }

    for (route, envelope_offset, frequency_offset) in
        [(0x0107u16, 0x02, 0x04), (0x0207, 0x08, 0x0C)]
    {
        let mut mixer = PsgMixer::new();
        let mut unrouted = [0; 0x48];
        unrouted[envelope_offset..envelope_offset + 2].copy_from_slice(&0xF080u16.to_le_bytes());
        unrouted[frequency_offset..frequency_offset + 2]
            .copy_from_slice(&(0x8000u16 | 1750).to_le_bytes());
        unrouted[0x20..0x22].copy_from_slice(&0x0007u16.to_le_bytes());
        unrouted[0x24] = 0x80;
        assert_eq!(mixer.frame(&unrouted).1, 0);

        unrouted[frequency_offset..frequency_offset + 2].copy_from_slice(&1750u16.to_le_bytes());
        unrouted[0x20..0x22].copy_from_slice(&route.to_le_bytes());
        let (sample, active) = mixer.frame(&unrouted);
        assert_eq!(active, 1);
        assert_ne!(sample, 0.0, "standalone trigger latches before routing");
    }
}
