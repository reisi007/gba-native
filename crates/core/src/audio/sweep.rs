//! Channel 1 host-sample-quantized frequency sweep approximation.

const SAMPLES_PER_SWEEP_UNIT: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrequencySweep {
    frequency: u16,
    observed_frequency: u16,
    period: u8,
    shift: u8,
    decrease: bool,
    elapsed: u32,
    enabled: bool,
}

impl FrequencySweep {
    pub(super) fn new(control: u8, frequency: u16) -> Self {
        let period = (control >> 4) & 7;
        Self {
            frequency: frequency & 0x07ff,
            observed_frequency: frequency & 0x07ff,
            period,
            shift: control & 7,
            decrease: control & 8 != 0,
            elapsed: 0,
            enabled: true,
        }
    }

    pub(super) const fn frequency(self) -> u16 {
        self.frequency
    }

    /// Track register snapshots without disturbing accumulated sweep time unless
    /// the raw frequency actually changed. Same-value writes are unobservable.
    pub(super) fn observe_registers(&mut self, control: u8, frequency: u16) {
        let frequency = frequency & 0x07ff;
        if frequency != self.observed_frequency {
            self.frequency = frequency;
            self.observed_frequency = frequency;
        }
        self.period = (control >> 4) & 7;
        self.shift = control & 7;
        self.decrease = control & 8 != 0;
    }

    /// Advance one host sample; overflow disables the modeled sweep/channel.
    pub(super) fn advance(&mut self) -> bool {
        if !self.enabled {
            return self.enabled;
        }
        if self.period == 0 {
            return true;
        }
        self.elapsed += 1;
        if self.elapsed < u32::from(self.period) * SAMPLES_PER_SWEEP_UNIT {
            return true;
        }
        self.elapsed = 0;
        let delta = self.frequency >> self.shift;
        let next = if self.decrease {
            self.frequency.saturating_sub(delta)
        } else {
            self.frequency.saturating_add(delta)
        };
        if next > 0x07ff {
            self.enabled = false;
            return false;
        }
        self.frequency = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::FrequencySweep;
    use crate::audio::{PsgMixer, SAMPLE_RATE};
    use crate::bus::Bus;
    use crate::cart::Cartridge;

    const SOUND_BASE: u32 = 0x0400_0060;

    #[test]
    fn increase_and_decrease_use_raw_frequency_integer_formula() {
        let mut increase = FrequencySweep::new(0x11, 1000);
        for _ in 0..256 {
            assert!(increase.advance());
        }
        assert_eq!(increase.frequency(), 1500);

        let mut decrease = FrequencySweep::new(0x19, 1000);
        for _ in 0..256 {
            assert!(decrease.advance());
        }
        assert_eq!(decrease.frequency(), 500);
    }

    #[test]
    fn zero_shift_uses_the_full_frequency_as_the_delta() {
        let mut increase = FrequencySweep::new(0x10, 1000);
        for _ in 0..256 {
            assert!(increase.advance());
        }
        assert_eq!(increase.frequency(), 2000);

        let mut decrease = FrequencySweep::new(0x18, 1000);
        for _ in 0..256 {
            assert!(decrease.advance());
        }
        assert_eq!(decrease.frequency(), 0);
    }

    #[test]
    fn update_occurs_at_exact_sample_boundary() {
        let mut sweep = FrequencySweep::new(0x11, 1000);
        for _ in 0..255 {
            sweep.advance();
        }
        assert_eq!(sweep.frequency(), 1000);
        sweep.advance();
        assert_eq!(sweep.frequency(), 1500);
    }

    #[test]
    fn zero_period_disables_sweep() {
        let mut sweep = FrequencySweep::new(0x01, 1000);
        for _ in 0..1024 {
            assert!(sweep.advance());
        }
        assert_eq!(sweep.frequency(), 1000);
    }

    #[test]
    fn increase_overflow_disables_channel_without_wrapping() {
        let mut sweep = FrequencySweep::new(0x11, 1800);
        for _ in 0..256 {
            if !sweep.advance() {
                assert_eq!(sweep.frequency(), 1800);
                return;
            }
        }
        panic!("overflow should disable the modeled channel");
    }

    #[test]
    fn frequency_register_change_rebases_pitch_on_the_next_sample() {
        let mut regs = [0u8; 0x48];
        regs[0] = 0x01; // No sweep period.
        regs[0x02..0x04].copy_from_slice(&0xF080u16.to_le_bytes());
        regs[0x04..0x06].copy_from_slice(&(0x8000u16 | 1750).to_le_bytes());
        regs[0x20..0x22].copy_from_slice(&0x0107u16.to_le_bytes());
        regs[0x24] = 0x80;

        let mut mixer = PsgMixer::new();
        mixer.frame(&regs); // Observe trigger and establish sweep state.
        let old_phase = mixer.phases[0];
        regs[0x04..0x06].copy_from_slice(&(0x8000u16 | 1700).to_le_bytes());
        mixer.frame(&regs);
        let expected_step = 131_072.0 / f32::from(2048u16 - 1700) / SAMPLE_RATE as f32;
        assert!((mixer.phases[0] - old_phase - expected_step).abs() < 0.00001);
    }

    #[test]
    fn bus_trigger_initializes_sweep_and_sound_reset_clears_it() {
        let mut bus = Bus::new(Cartridge::from_bytes(vec![0; 0x200]).expect("cart"));
        bus.write16(SOUND_BASE, 0x11).expect("NR10");
        bus.write16(SOUND_BASE + 2, 0xF080).expect("NR11/NR12");
        bus.write16(SOUND_BASE + 0x20, 0x0107).expect("SOUNDCNT_L");
        bus.write16(SOUND_BASE + 0x24, 0x0080)
            .expect("enable sound");
        bus.write16(SOUND_BASE + 4, 0x8000 | 1000)
            .expect("trigger Channel 1");

        let mut mixer = PsgMixer::new();
        mixer.observe_bus(&bus);
        assert_eq!(mixer.sweep.map(FrequencySweep::frequency), Some(1000));

        bus.reset_group6_sound();
        mixer.observe_bus(&bus);
        assert!(mixer.sweep.is_none());
        assert!(!mixer.square_active[0]);
    }
}
