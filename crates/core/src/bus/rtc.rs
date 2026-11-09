//! Host-controlled S-3511 RTC snapshot and bounded three-wire GPIO protocol.
//!
//! This models selected documented commands only. Time changes only through the
//! host setter; nominal guest cycles do not advance it.

use crate::error::{Error, Result};

const RESET_DATE: [u8; 7] = [0, 1, 1, 0, 0, 0, 0];

/// Host-supplied RTC snapshot. Weekday is 0..=6 and hours use 24-hour time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RtcDateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RtcState {
    /// Year, month, day, weekday, hour, minute and second; BCD, hour is 24-hour.
    date: [u8; 7],
    reset_time: bool,
    control: u8,
    active: bool,
    last_pins: u8,
    reading: bool,
    read_started: bool,
    done: bool,
    bit: u8,
    bytes: Vec<u8>,
    expected: u8,
    command: u8,
    command_bits: u8,
}

impl Default for RtcState {
    fn default() -> Self {
        Self {
            date: RESET_DATE,
            reset_time: true,
            control: 0,
            active: false,
            last_pins: 0,
            reading: false,
            read_started: false,
            done: false,
            bit: 0,
            bytes: Vec::new(),
            expected: 0,
            command: 0,
            command_bits: 0,
        }
    }
}

impl RtcState {
    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(super) fn reading_sio(&self) -> bool {
        self.active && self.reading && self.read_started
    }

    pub(super) fn sio_level(&self) -> bool {
        self.bytes
            .get(usize::from(self.bit / 8))
            .is_some_and(|byte| byte & (1 << (self.bit % 8)) != 0)
    }

    pub(super) fn set_datetime(&mut self, datetime: RtcDateTime) -> Result<()> {
        let RtcDateTime {
            year,
            month,
            day,
            weekday,
            hour,
            minute,
            second,
        } = datetime;
        if !(2000..=2099).contains(&year)
            || month == 0
            || month > 12
            || day == 0
            || day > days_in_month(year, month)
            || weekday > 6
            || hour > 23
            || minute > 59
            || second > 59
        {
            return Err(Error::InvalidRtcDate);
        }
        self.date = [
            bcd((year - 2000) as u8),
            bcd(month),
            bcd(day),
            weekday,
            bcd(hour),
            bcd(minute),
            bcd(second),
        ];
        self.reset_time = false;
        Ok(())
    }

    /// Process a pin update atomically: malformed commands or data do not
    /// partially update the RTC state.
    pub(super) fn pins(&mut self, pins: u8) -> Result<()> {
        let mut next = self.clone();
        next.apply_pins(pins & 0x07)?;
        *self = next;
        Ok(())
    }

    fn apply_pins(&mut self, pins: u8) -> Result<()> {
        let old = self.last_pins;
        self.last_pins = pins;

        let selected = pins & 0x04 != 0;
        let was_selected = old & 0x04 != 0;
        if selected && !was_selected {
            self.begin_transaction();
            return Ok(());
        }
        if !selected && was_selected {
            self.end_transaction();
            return Ok(());
        }
        if !self.active || self.done {
            return Ok(());
        }

        let rising = old & 1 == 0 && pins & 1 != 0;
        let falling = old & 1 != 0 && pins & 1 == 0;

        if self.reading {
            if falling {
                // Data bit zero is presented by the first falling edge after
                // the command; each later rising edge consumes the sampled bit.
                self.read_started = true;
            } else if rising && self.read_started {
                self.bit += 1;
                if usize::from(self.bit) == usize::from(self.expected) * 8 {
                    self.reading = false;
                    self.done = true;
                }
            }
            return Ok(());
        }

        if !rising {
            return Ok(());
        }

        if self.command_bits < 8 {
            self.command |= ((pins >> 1) & 1) << self.command_bits;
            self.command_bits += 1;
            if self.command_bits == 8 {
                self.begin_command()?;
            }
            return Ok(());
        }

        if self.expected == 0 {
            self.done = true;
            return Ok(());
        }

        if self.bit.is_multiple_of(8) {
            self.bytes.push(0);
        }
        let byte_index = self.bytes.len() - 1;
        if pins & 0x02 != 0 {
            self.bytes[byte_index] |= 1 << (self.bit % 8);
        }
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            if self.bytes.len() == usize::from(self.expected) {
                self.commit_write()?;
            }
        }
        Ok(())
    }

    fn begin_transaction(&mut self) {
        self.active = true;
        self.reading = false;
        self.read_started = false;
        self.done = false;
        self.command = 0;
        self.command_bits = 0;
        self.expected = 0;
        self.bit = 0;
        self.bytes.clear();
    }

    fn end_transaction(&mut self) {
        self.active = false;
        self.reading = false;
        self.read_started = false;
        self.done = false;
        self.bit = 0;
        self.bytes.clear();
        self.expected = 0;
        self.command = 0;
        self.command_bits = 0;
    }

    fn begin_command(&mut self) -> Result<()> {
        let (command, read) = decode_command(self.command)?;
        self.command = command;
        self.reading = read;
        self.read_started = false;
        self.bit = 0;
        self.bytes.clear();

        match command {
            0 => {
                // GBA force-reset strobes on either read or write access.
                self.date = RESET_DATE;
                self.reset_time = true;
                self.control = 0;
                self.reading = false;
                self.done = true;
            }
            1 => self.prepare_register(1, read, &[0xff]),
            2 => {
                self.expected = 7;
                if read {
                    self.bytes = datetime_wire(&self.date, self.control, self.reset_time).to_vec();
                }
            }
            3 => return Err(Error::RtcForceIrqUnsupported),
            4 => self.prepare_register(1, read, &[self.control]),
            5 => self.prepare_register(3, read, &[0xff, 0xff, 0xff]),
            6 => {
                self.expected = 3;
                if read {
                    let wire = datetime_wire(&self.date, self.control, self.reset_time);
                    self.bytes.extend_from_slice(&wire[4..]);
                }
            }
            7 => self.prepare_register(1, read, &[0xff]),
            _ => unreachable!("command decoder returns a three-bit ID"),
        }
        Ok(())
    }

    fn prepare_register(&mut self, size: u8, read: bool, value: &[u8]) {
        self.expected = size;
        if read {
            self.bytes.extend_from_slice(value);
        }
    }

    fn commit_write(&mut self) -> Result<()> {
        match self.command {
            1 | 5 | 7 => {} // GBA documents these registers as constant FFh.
            2 => {
                self.date = parse_datetime(&self.bytes, self.control)?;
                self.reset_time = false;
            }
            4 => self.control = self.bytes[0] & 0x6a,
            6 => {
                let mut wire = datetime_wire(&self.date, self.control, self.reset_time);
                wire[4..].copy_from_slice(&self.bytes);
                let parsed = parse_datetime(&wire, self.control)?;
                self.date[4..].copy_from_slice(&parsed[4..]);
                self.reset_time = false;
            }
            _ => return Err(Error::InvalidRtcCommand(self.command)),
        }
        self.bytes.clear();
        self.done = true;
        Ok(())
    }

    pub(super) fn append_digest(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.date);
        out.push(self.reset_time as u8);
        out.push(self.control);
        out.extend_from_slice(&[
            self.active as u8,
            self.last_pins,
            self.reading as u8,
            self.read_started as u8,
            self.done as u8,
            self.bit,
            self.expected,
            self.command,
            self.command_bits,
        ]);
        out.extend_from_slice(&(self.bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(&self.bytes);
    }
}

fn decode_command(raw: u8) -> Result<(u8, bool)> {
    if raw & 0x0f == 0x06 {
        Ok(((raw >> 4) & 0x07, raw & 0x80 != 0))
    } else if raw >> 4 == 0x06 {
        Ok((reverse_three((raw >> 1) & 0x07), raw & 1 != 0))
    } else {
        Err(Error::InvalidRtcCommand(raw))
    }
}

fn reverse_three(value: u8) -> u8 {
    ((value & 1) << 2) | (value & 2) | ((value & 4) >> 2)
}

fn datetime_wire(date: &[u8; 7], control: u8, reset_time: bool) -> [u8; 7] {
    let mut wire = *date;
    if !reset_time {
        wire[4] = encode_hour(unbcd(date[4]).expect("stored hour is valid BCD"), control);
    }
    wire
}

fn parse_datetime(wire: &[u8], control: u8) -> Result<[u8; 7]> {
    if wire.len() != 7 {
        return Err(Error::InvalidRtcDate);
    }
    let year = unbcd(wire[0]).ok_or(Error::InvalidRtcBcd)?;
    let month = unbcd(wire[1]).ok_or(Error::InvalidRtcBcd)?;
    let day = unbcd(wire[2]).ok_or(Error::InvalidRtcBcd)?;
    let weekday = wire[3];
    let minute = unbcd(wire[5]).ok_or(Error::InvalidRtcBcd)?;
    let second = unbcd(wire[6]).ok_or(Error::InvalidRtcBcd)?;
    if weekday > 6
        || month == 0
        || month > 12
        || day == 0
        || day > days_in_month(2000 + u16::from(year), month)
        || minute > 59
        || second > 59
    {
        return Err(Error::InvalidRtcDate);
    }
    let hour = decode_hour(wire[4], control)?;
    Ok([
        wire[0],
        wire[1],
        wire[2],
        weekday,
        bcd(hour),
        wire[5],
        wire[6],
    ])
}

fn decode_hour(value: u8, control: u8) -> Result<u8> {
    let hour = unbcd(value & 0x7f).ok_or(Error::InvalidRtcBcd)?;
    if control & 0x40 != 0 {
        if hour > 23 {
            return Err(Error::InvalidRtcDate);
        }
        Ok(hour)
    } else {
        if hour > 11 {
            return Err(Error::InvalidRtcDate);
        }
        let pm = value & 0x80 != 0;
        Ok(hour + if pm { 12 } else { 0 })
    }
}

fn encode_hour(hour: u8, control: u8) -> u8 {
    let hour_bcd = if control & 0x40 != 0 {
        bcd(hour)
    } else {
        bcd(hour % 12)
    };
    hour_bcd | if hour >= 12 { 0x80 } else { 0 }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(4) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn bcd(value: u8) -> u8 {
    (value / 10) << 4 | (value % 10)
}

fn unbcd(value: u8) -> Option<u8> {
    if value >> 4 > 9 || value & 0x0f > 9 {
        None
    } else {
        Some((value >> 4) * 10 + (value & 0x0f))
    }
}
