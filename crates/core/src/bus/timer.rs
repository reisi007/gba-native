//! Timer registers `0x04000100..0x0400010F`: instruction-budget-driven counters.
//!
//! **Design choice, not a timing claim.** GBATEK `#gba-timers` describes four
//! incrementing 16-bit timers, a selectable prescaler (`F/1`, `F/64`, `F/256`,
//! `F/1024`), count-up cascading from the previous timer, overflow reload and an
//! optional overflow IRQ. This model advances every enabled timer from the same
//! nominal clock as the instruction-count frame model: one retired guest
//! instruction contributes [`INSTRUCTION_CYCLES`] cycles. A normal timer
//! increments once every selected number of those cycles; a cascaded timer
//! ignores its prescaler and increments once per previous-timer overflow.
//!
//! GBATEK `#4000100h---tm0cnt_l---timer-0-counterreload-rw` says a write to
//! `TMxCNT_L` updates the reload value without changing the current counter, a
//! read returns the current (or frozen) counter, and the reload value is copied
//! into the counter on overflow or on a `0 -> 1` start edge in `TMxCNT_H`.
//! That documented behavior is implemented. A 32-bit reload+control write is
//! composed low halfword first, so a simultaneously written reload is the value
//! used by the start edge.
//!
//! **Explicit limits.** This is not cycle or prescaler-accurate hardware timing:
//! one guest instruction is not three hardware cycles, instruction lengths,
//! waitstates and bus arbitration are absent, and a counter read observes only
//! completed instruction ticks. Timer overflow can latch the matching `IF` bit
//! (bits 3..6) for the existing IME/IE dispatcher, but timers never drive Sound
//! FIFO sample consumption, Sound DMA, VCOUNT, HBlank or any other peripheral.
//! Timer 0's documented `Not used` count-up bit is accepted and ignored, then
//! reads back as zero; Timer 1..3 use it for count-up. Reserved control bits are
//! stored as zero. Reset through RegisterRamReset group 7 returns all four
//! timers to their stopped, zeroed default state.

use super::{Bus, INSTRUCTION_CYCLES};

pub(super) const TIMER_BASE: u32 = 0x0400_0100;
pub(super) const TIMER_LEN: usize = 16;
const TIMER_COUNT: usize = 4;
const TIMER_STRIDE: u32 = 4;

const CONTROL_PRESCALER: u16 = 0x0003;
const CONTROL_COUNT_UP: u16 = 0x0004;
const CONTROL_IRQ: u16 = 0x0040;
const CONTROL_START: u16 = 0x0080;
const CONTROL_WRITABLE: u16 = 0x00C7;
const TIMER0_CONTROL_WRITABLE: u16 = 0x00C3;
const PRESCALERS: [u32; 4] = [1, 64, 256, 1024];
const TIMER_IRQ_FIRST_BIT: u16 = 3;

#[derive(Clone, Copy, Default)]
struct Timer {
    reload: u16,
    counter: u16,
    control: u16,
    /// Nominal cycles accumulated toward the next prescaler increment.
    prescaler_cycles: u32,
}

impl Timer {
    fn running(self) -> bool {
        self.control & CONTROL_START != 0
    }

    fn prescaler(self) -> u32 {
        PRESCALERS[usize::from(self.control & CONTROL_PRESCALER)]
    }

    fn count_up(self) -> bool {
        self.control & CONTROL_COUNT_UP != 0
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct TimerState {
    timers: [Timer; TIMER_COUNT],
}

impl TimerState {
    fn offset(addr: u32) -> Option<usize> {
        let offset = addr.checked_sub(TIMER_BASE)?;
        (offset < TIMER_LEN as u32).then_some(offset as usize)
    }

    fn index_and_lane(offset: usize) -> (usize, usize) {
        (
            offset / TIMER_STRIDE as usize,
            offset % TIMER_STRIDE as usize,
        )
    }

    pub(super) fn read16(&self, addr: u32) -> Option<u16> {
        let offset = Self::offset(addr)?;
        let (index, lane) = Self::index_and_lane(offset);
        let timer = self.timers[index];
        match lane {
            0 => Some(timer.counter),
            2 => Some(timer.control),
            _ => None,
        }
    }

    /// Write one timer halfword.
    ///
    /// `TMxCNT_L` updates only the reload latch. `TMxCNT_H` applies a documented
    /// `0 -> 1` start edge (reload the counter) and freezes the counter on
    /// disable. Timer 0's documented unused count-up bit is ignored so it reads
    /// back as zero.
    pub(super) fn write16(&mut self, addr: u32, value: u16) -> Option<()> {
        let offset = Self::offset(addr)?;
        let (index, lane) = Self::index_and_lane(offset);
        let timer = &mut self.timers[index];
        if lane == 0 {
            timer.reload = value;
            return Some(());
        }
        if lane != 2 {
            return None;
        }
        let writable = if index == 0 {
            TIMER0_CONTROL_WRITABLE
        } else {
            CONTROL_WRITABLE
        };
        let previous = timer.control;
        let control = value & writable;
        let was_running = previous & CONTROL_START != 0;
        let running = control & CONTROL_START != 0;
        timer.control = control;
        if running && !was_running {
            // Documented start edge: the current counter takes the reload value.
            timer.counter = timer.reload;
            timer.prescaler_cycles = 0;
        } else if !running {
            // Disable freezes the counter and abandons any partial prescaler
            // phase; a later enable reloads the counter anyway.
            timer.prescaler_cycles = 0;
        } else if (previous ^ control) & (CONTROL_PRESCALER | CONTROL_COUNT_UP) != 0 {
            // A running timer keeps its counter when only the clock source
            // changes, but a partial phase from the old divisor cannot carry
            // over to the new one.
            timer.prescaler_cycles = 0;
        }
        Some(())
    }

    /// Advance the timers by nominal hardware cycles.
    ///
    /// Returns the latched timer IRQ request bits (`IF` bits 3..6).
    pub(super) fn advance_cycles(&mut self, cycles: u32) -> u16 {
        let mut requests = 0u16;
        for _ in 0..cycles {
            let mut previous_overflow = false;
            for index in 0..TIMER_COUNT {
                let timer = &mut self.timers[index];
                if !timer.running() {
                    timer.prescaler_cycles = 0;
                    previous_overflow = false;
                    continue;
                }
                let increment = if index > 0 && timer.count_up() {
                    previous_overflow
                } else {
                    timer.prescaler_cycles += 1;
                    if timer.prescaler_cycles >= timer.prescaler() {
                        timer.prescaler_cycles = 0;
                        true
                    } else {
                        false
                    }
                };
                if !increment {
                    previous_overflow = false;
                    continue;
                }
                let (next, overflow) = timer.counter.overflowing_add(1);
                timer.counter = if overflow { timer.reload } else { next };
                if overflow && timer.control & CONTROL_IRQ != 0 {
                    requests |= 1 << (index as u16 + TIMER_IRQ_FIRST_BIT);
                }
                previous_overflow = overflow;
            }
        }
        requests
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn append_digest(&self, bytes: &mut Vec<u8>) {
        for timer in &self.timers {
            bytes.extend_from_slice(&timer.reload.to_le_bytes());
            bytes.extend_from_slice(&timer.counter.to_le_bytes());
            bytes.extend_from_slice(&timer.control.to_le_bytes());
            bytes.extend_from_slice(&timer.prescaler_cycles.to_le_bytes());
        }
    }
}

impl Bus {
    pub(super) fn timer_read16(&self, addr: u32) -> Option<u16> {
        self.io.timer().read16(addr)
    }

    pub(super) fn timer_write16(&mut self, addr: u32, value: u16) -> Option<()> {
        self.io.timer_mut().write16(addr, value)
    }

    /// Advance all timers by one retired instruction's nominal cycles and latch
    /// any overflow request in `IF`.
    pub(super) fn tick_timers(&mut self) {
        let requests = self.io.timer_mut().advance_cycles(INSTRUCTION_CYCLES);
        self.io.irq_mut().latch_requests(requests);
    }
}
