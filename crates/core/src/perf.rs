//! Host performance statistics for bounded guest runs.
//!
//! The comparison target is the documented GBA CPU clock (16.777216 MHz). One
//! retired guest instruction is *not* one hardware cycle: instruction lengths,
//! waitstates, DMA and bus arbitration are not modelled. The real-time factor
//! is therefore a stated approximation of "how much guest work per wall-clock
//! second", not a cycle-accurate speed claim.

use std::time::Duration;

/// Documented GBA CPU clock in Hz.
pub const GBA_CPU_HZ: u64 = 16_777_216;

/// Wall-clock measurement of one bounded run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfStats {
    pub instructions: u64,
    pub irq_returns: u64,
    pub fallbacks: u64,
    pub elapsed: Duration,
}

impl PerfStats {
    #[must_use]
    pub const fn new(
        instructions: u64,
        irq_returns: u64,
        fallbacks: u64,
        elapsed: Duration,
    ) -> Self {
        Self {
            instructions,
            irq_returns,
            fallbacks,
            elapsed,
        }
    }

    #[must_use]
    pub fn elapsed_millis(self) -> f64 {
        self.elapsed.as_secs_f64() * 1_000.0
    }

    /// Retired guest instructions per wall-clock second.
    #[must_use]
    pub fn instructions_per_second(self) -> f64 {
        let seconds = self.elapsed.as_secs_f64();
        if seconds <= 0.0 {
            return 0.0;
        }
        self.instructions as f64 / seconds
    }

    /// Guest instructions per second divided by the documented CPU clock.
    #[must_use]
    pub fn realtime_factor(self) -> f64 {
        self.instructions_per_second() / GBA_CPU_HZ as f64
    }

    #[must_use]
    pub fn is_faster_than_realtime(self) -> bool {
        self.realtime_factor() >= 1.0
    }

    /// Stable ordered report lines shared by the CLI and the AOT harness.
    #[must_use]
    pub fn report_lines(self) -> Vec<String> {
        vec![
            format!("perf_elapsed_millis={:.3}", self.elapsed_millis()),
            format!(
                "perf_guest_instructions_per_second={:.0}",
                self.instructions_per_second()
            ),
            format!("perf_realtime_factor={:.4}", self.realtime_factor()),
            format!(
                "perf_faster_than_realtime={}",
                if self.is_faster_than_realtime() {
                    "true"
                } else {
                    "false"
                }
            ),
            format!(
                "perf_reference=gba_cpu_hz={GBA_CPU_HZ}; one retired instruction is not one hardware cycle; no cycle, waitstate or frame timing claim"
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_factor_uses_the_documented_clock() {
        let stats = PerfStats::new(GBA_CPU_HZ, 0, 0, Duration::from_secs(1));
        assert!((stats.instructions_per_second() - GBA_CPU_HZ as f64).abs() < 0.5);
        assert!((stats.realtime_factor() - 1.0).abs() < 1e-9);
        assert!(stats.is_faster_than_realtime());

        let twice = PerfStats::new(GBA_CPU_HZ * 2, 0, 0, Duration::from_secs(1));
        assert!((twice.realtime_factor() - 2.0).abs() < 1e-9);

        let half = PerfStats::new(GBA_CPU_HZ / 2, 0, 0, Duration::from_secs(1));
        assert!(!half.is_faster_than_realtime());
    }

    #[test]
    fn zero_elapsed_is_reported_as_zero_not_infinite() {
        let stats = PerfStats::new(100, 0, 0, Duration::ZERO);
        assert_eq!(stats.instructions_per_second(), 0.0);
        assert_eq!(stats.realtime_factor(), 0.0);
        assert!(!stats.is_faster_than_realtime());
    }

    #[test]
    fn report_is_ordered_and_complete() {
        let lines = PerfStats::new(16_777_216, 3, 4, Duration::from_millis(500)).report_lines();
        assert_eq!(lines.len(), 5);
        assert!(lines[0].starts_with("perf_elapsed_millis=500.000"));
        assert!(lines[1].starts_with("perf_guest_instructions_per_second=33554432"));
        assert!(lines[2].starts_with("perf_realtime_factor=2.0000"));
        assert_eq!(lines[3], "perf_faster_than_realtime=true");
        assert!(lines[4].contains("not one hardware cycle"));
    }
}
