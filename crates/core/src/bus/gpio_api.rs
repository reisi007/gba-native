use super::{Bus, GpioPolicy, RtcDateTime};
use crate::error::{Error, Result};

impl Bus {
    /// Opts in to GPIO at 0x080000C4..=0x080000C9. `DocumentedStorage` models
    /// only registers/input; `DocumentedRtc` separately enables the RTC subset.
    #[must_use]
    pub fn with_gpio_policy(mut self, policy: GpioPolicy) -> Self {
        self.gpio_policy = policy;
        self
    }

    pub fn gpio_policy(&self) -> GpioPolicy {
        self.gpio_policy
    }

    /// Injects the externally supplied levels of GPIO pins 0..3. No clock,
    /// protocol, or automatic sampling is implied.
    pub fn set_gpio_input(&mut self, input: u8) {
        self.gpio_input = input & 0x0F;
    }

    /// Returns the low four data bits, including output latch levels, injected
    /// input levels and (for RTC reads on input-configured SIO) the RTC level.
    pub fn gpio_data(&self) -> u8 {
        let mut value = ((self.gpio_data & self.gpio_direction)
            | (self.gpio_input & !self.gpio_direction))
            & 0x0F;
        if self.gpio_policy == GpioPolicy::DocumentedRtc
            && self.gpio_direction & 0x02 == 0
            && self.rtc.reading_sio()
        {
            value = (value & !2) | (u8::from(self.rtc.sio_level()) << 1);
        }
        value
    }

    /// Sets a host-supplied snapshot. No wall clock or guest-cycle advancement
    /// is implied; `datetime` uses 24-hour time.
    pub fn set_rtc_datetime(&mut self, datetime: RtcDateTime) -> Result<()> {
        if self.gpio_policy != GpioPolicy::DocumentedRtc {
            return Err(Error::RtcPolicyRequired);
        }
        self.rtc.set_datetime(datetime)
    }
}
