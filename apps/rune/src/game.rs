//! Bounded live reference-interpreter session for an externally supplied ROM.

use gba_native::cart::Cartridge;
use gba_native::interpret;
use gba_native::{
    audio::{AudioEngine, AudioSettings},
    bus::Bus,
    cpu::Cpu,
    display,
    frontend::{image::RgbaImage, ui},
};
#[cfg(rom_bound)]
#[allow(
    clippy::all,
    dead_code,
    unused_imports,
    unused_variables,
    unreachable_code
)]
mod compiled_program {
    include!(concat!(env!("OUT_DIR"), "/rom_program.rs"));
}

pub const INSTRUCTIONS_PER_POLL: u64 = 100_000;

/// Explicit host-selected device policies for a bound game session.
///
/// Every field is an opt-in documented in `docs/clean-room/gba-contract.md`;
/// `Default` is the strict fail-closed bus. A one-game build selects its policy
/// visibly in the UI instead of silently relaxing the core defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionPolicy {
    /// `Bus::with_synthetic_bios_zero_fill`: experimental zero data reads.
    pub bios_zero_fill: bool,
    /// Documented LZ77/RL/diff decompression SWIs.
    pub decompression: bool,
    /// Documented `BgAffineSet`/`ObjAffineSet` SWIs.
    pub affine: bool,
    /// Mask-ROM "store has no effect" handling for BIOS-area writes.
    pub bios_rom_writes: bool,
    /// Mask-ROM "store has no effect" handling for ROM writes.
    pub rom_writes_dropped: bool,
    /// Volatile 128 KiB flash backup device.
    pub flash_backup: bool,
    /// Documented GPIO real-time-clock subset seeded from host time.
    pub rtc: bool,
}

impl SessionPolicy {
    /// The documented opt-ins a one-game build requests, shown in its UI.
    #[must_use]
    pub const fn documented_opt_ins() -> Self {
        Self {
            bios_zero_fill: true,
            decompression: true,
            affine: true,
            bios_rom_writes: true,
            rom_writes_dropped: true,
            flash_backup: true,
            rtc: true,
        }
    }

    #[must_use]
    pub fn describe(self) -> String {
        let mut names = Vec::new();
        if self.bios_zero_fill {
            names.push("synthetic BIOS zero-fill");
        }
        if self.decompression {
            names.push("documented decompression HLE");
        }
        if self.affine {
            names.push("documented affine HLE");
        }
        if self.bios_rom_writes {
            names.push("BIOS writes ignored");
        }
        if self.rom_writes_dropped {
            names.push("ROM writes ignored");
        }
        if self.flash_backup {
            names.push("volatile 128 KiB flash");
        }
        if self.rtc {
            names.push("host RTC snapshot (interpreter-only)");
        }
        if names.is_empty() {
            "strict bus policy".to_owned()
        } else {
            format!("strict defaults plus {}", names.join(", "))
        }
    }
}

pub struct GameSession {
    cpu: Cpu,
    bus: Bus,
    /// Persistent mixer state: resetting it per poll restarts PSG phases and
    /// Direct Sound FIFOs and produces audible clicks and gaps.
    audio: Option<AudioEngine>,
    image: RgbaImage,
    title: String,
    digest: String,
    game_code: String,
    rom_len: usize,
    policy: SessionPolicy,
    instructions: u64,
    running: bool,
    error: Option<String>,
}

impl GameSession {
    #[cfg(any(test, target_arch = "wasm32", rom_embedded))]
    pub fn from_bytes(bytes: Vec<u8>, settings: AudioSettings) -> Result<Self, String> {
        Self::from_bytes_with_policy(bytes, settings, SessionPolicy::default())
    }

    #[cfg(any(test, target_arch = "wasm32", rom_embedded))]
    pub fn from_bytes_with_policy(
        bytes: Vec<u8>,
        settings: AudioSettings,
        policy: SessionPolicy,
    ) -> Result<Self, String> {
        let cart = Cartridge::from_bytes(bytes).map_err(|error| error.to_string())?;
        Self::new(cart, settings, policy)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_path(path: &std::path::Path, settings: AudioSettings) -> Result<Self, String> {
        Self::from_path_with_policy(path, settings, SessionPolicy::default())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_path_with_policy(
        path: &std::path::Path,
        settings: AudioSettings,
        policy: SessionPolicy,
    ) -> Result<Self, String> {
        let cart = Cartridge::from_path(path).map_err(|error| error.to_string())?;
        Self::new(cart, settings, policy)
    }

    fn new(
        cart: Cartridge,
        settings: AudioSettings,
        policy: SessionPolicy,
    ) -> Result<Self, String> {
        let title = cart.header().title.clone();
        let game_code = String::from_utf8_lossy(&cart.header().game_code).into_owned();
        let digest = cart.sha256_hex();
        let rom_len = cart.len();
        let mut cpu = Cpu::direct_entry();
        cpu.initialize_bios_stacks();
        let mut bus = if policy.bios_zero_fill {
            Bus::with_synthetic_bios_zero_fill(cart)
        } else {
            Bus::new(cart)
        };
        if policy.decompression {
            bus = bus.with_decompression_policy(
                gba_native::bios::DecompressionPolicy::DocumentedFormats,
            );
        }
        if policy.affine {
            bus = bus.with_affine_policy(gba_native::bios::AffinePolicy::Documented);
        }
        if policy.bios_rom_writes {
            bus = bus.with_bios_write_policy(gba_native::bus::BiosWritePolicy::RomWritesDropped);
        }
        if policy.rom_writes_dropped {
            bus = bus.with_rom_write_policy(gba_native::bus::RomWritePolicy::Dropped);
        }
        if policy.flash_backup {
            bus = bus.with_backup_policy(gba_native::bus::BackupPolicy::Flash128KVolatile);
        }
        if policy.rtc {
            bus = bus.with_gpio_policy(gba_native::bus::GpioPolicy::DocumentedRtc);
            if let Some(datetime) = host_datetime() {
                // A dead RTC cell makes the game fall back to its dry-battery
                // path; seeding host time keeps that path off. It is a host
                // snapshot, not hardware RTC timing.
                let _ = bus.set_rtc_datetime(datetime);
            }
        }
        let image = snapshot(&bus, 0)?;
        Ok(Self {
            cpu,
            bus,
            audio: Some(AudioEngine::new(settings)),
            image,
            title,
            digest,
            game_code,
            rom_len,
            policy,
            instructions: 0,
            running: true,
            error: None,
        })
    }

    /// Guest instructions for one displayed frame.
    ///
    /// The core attributes three nominal cycles per instruction, so realtime
    /// playback needs `elapsed × CPU_HZ / 3` instructions. At least one guest
    /// frame is always executed; a slow host frame is capped instead of stalling
    /// the window, and audio keeps its backlog.
    #[must_use]
    pub fn poll_budget(elapsed: f64) -> u64 {
        let elapsed = if elapsed.is_finite() && elapsed > 0.0 {
            elapsed
        } else {
            0.0
        };
        let wanted = (elapsed * gba_native::input::GBA_CPU_HZ as f64 / 3.0) as u64;
        if wanted == 0 {
            // First poll: run exactly one guest frame.
            return INSTRUCTIONS_PER_POLL;
        }
        wanted.min(1_000_000)
    }

    pub fn advance_poll(
        &mut self,
        pressed: u16,
        settings: AudioSettings,
        budget: u64,
    ) -> Result<(), String> {
        if !self.running || self.error.is_some() {
            return Ok(());
        }
        if let Some(engine) = self.audio.as_mut() {
            engine.set_settings(settings);
        }
        self.bus.set_pressed_keys(pressed);
        // Generated blocks are only valid under the ordinary immutable-ROM
        // mapping; a GPIO-enabled bus (host RTC) is reference-only by contract.
        #[cfg(all(rom_bound, not(test)))]
        let use_program = self.bus.gpio_policy() == gba_native::bus::GpioPolicy::Strict;
        #[cfg(all(rom_bound, test))]
        let use_program = false;
        #[cfg(rom_bound)]
        if use_program {
            // The generated runner applies a script mask per instruction. One
            // held interval across this poll keeps the live host buttons down
            // for the whole slice instead of dropping them.
            let script =
                gba_native::input::InputScript::from_events(vec![gba_native::input::InputEvent {
                    at: 0,
                    keys: pressed,
                    hold: budget,
                }]);
            let mut audio = self.audio.take();
            let report = compiled_program::run_program(
                &mut self.cpu,
                &mut self.bus,
                budget,
                &script,
                &mut audio,
            );
            self.audio = audio;
            let report = report.map_err(|error| self.stop_with(error.to_string()))?;
            self.instructions = self
                .instructions
                .saturating_add(budget.saturating_sub(report.budget_remaining));
        }
        #[cfg(not(rom_bound))]
        let interpreter_only = true;
        #[cfg(rom_bound)]
        let interpreter_only = !use_program;
        if interpreter_only {
            for _ in 0..budget {
                interpret::deliver_pending_irq(&mut self.cpu, &mut self.bus)
                    .and_then(|_| interpret::step(&mut self.cpu, &mut self.bus))
                    .map_err(|error| self.stop_with(error.to_string()))?;
                self.bus.tick_frame();
                self.instructions = self.instructions.saturating_add(1);
                if let Some(engine) = self.audio.as_mut() {
                    engine
                        .advance_instruction_bus(&mut self.bus)
                        .map_err(|error| self.stop_with(error.to_string()))?;
                }
            }
        }
        self.image =
            snapshot(&self.bus, self.instructions).map_err(|error| self.stop_with(error))?;
        Ok(())
    }

    fn stop_with(&mut self, error: String) -> String {
        self.running = false;
        let context = format!(
            "{error} (pc=0x{:08X}, thumb={}, after {} guest instructions)",
            self.cpu.pc(),
            self.cpu.thumb(),
            self.instructions
        );
        self.error = Some(context.clone());
        context
    }

    pub fn presentation(&self, pressed: u16, fullscreen: bool, touch_controls: bool) -> RgbaImage {
        if crate::rom_binding::BOUND_ROM.is_some() {
            // A one-game build is a handheld: the shell stays visible in every
            // window mode so the on-screen pad and buttons never disappear.
            ui::game_shell(&self.image, pressed, true)
        } else if fullscreen {
            ui::fullscreen(&self.image)
        } else {
            ui::shell_with_controls(&self.image, pressed, touch_controls)
        }
    }

    pub const fn running(&self) -> bool {
        self.running
    }

    pub fn toggle_running(&mut self) {
        if self.error.is_none() {
            self.running = !self.running;
        }
    }

    pub const fn instructions(&self) -> u64 {
        self.instructions
    }

    pub const fn policy(&self) -> SessionPolicy {
        self.policy
    }

    pub fn image_digest(&self) -> &str {
        self.image.rgb_sha256()
    }

    pub const fn error(&self) -> Option<&String> {
        self.error.as_ref()
    }

    pub fn identity(&self) -> String {
        format!(
            "{} ({}) · {} bytes · SHA-256 {}",
            self.title, self.game_code, self.rom_len, self.digest
        )
    }

    pub fn verify_binding(&self, binding: crate::rom_binding::RomBinding) -> Result<(), String> {
        if self.rom_len == binding.len
            && self.digest == binding.sha256
            && self.game_code == binding.game_code
        {
            return Ok(());
        }
        Err(format!(
            "ROM identity mismatch: expected {} ({}) · {} bytes · SHA-256 {}, got {}",
            binding.title,
            binding.game_code,
            binding.len,
            binding.sha256,
            self.identity()
        ))
    }

    pub fn take_audio_samples(&mut self) -> Vec<i16> {
        self.audio
            .as_mut()
            .map(AudioEngine::take_samples)
            .unwrap_or_default()
    }
}

/// Host wall clock in Unix milliseconds, or `None` when the host provides none.
#[cfg(not(target_arch = "wasm32"))]
fn host_unix_millis() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
}

#[cfg(target_arch = "wasm32")]
fn host_unix_millis() -> Option<u64> {
    #[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function runeNowMillis() { return Date.now(); }
"#)]
    extern "C" {
        #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeNowMillis)]
        fn now_millis() -> f64;
    }
    let millis = now_millis();
    (millis.is_finite() && millis >= 0.0).then_some(millis as u64)
}

/// Convert a Unix timestamp to a UTC calendar snapshot for the cartridge RTC.
///
/// Weekday uses 0 = Sunday. This is a host snapshot; guest cycles do not advance
/// it, matching the core's documented model.
fn utc_from_unix_seconds(seconds: u64) -> gba_native::bus::RtcDateTime {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let of_day = seconds % 86_400;
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = if month <= 2 { year + 1 } else { year };
    gba_native::bus::RtcDateTime {
        year: u16::try_from(year.clamp(0, 9999)).unwrap_or(2000),
        month,
        day,
        weekday: u8::try_from((days + 4).rem_euclid(7)).unwrap_or(0),
        hour: (of_day / 3_600) as u8,
        minute: (of_day / 60 % 60) as u8,
        second: (of_day % 60) as u8,
    }
}

fn host_datetime() -> Option<gba_native::bus::RtcDateTime> {
    host_unix_millis().map(|millis| utc_from_unix_seconds(millis / 1_000))
}

fn snapshot(bus: &Bus, instructions: u64) -> Result<RgbaImage, String> {
    let image = display::snapshot(bus).map_err(|error| error.to_string())?;
    let rgba = gba_native::frontend::image::convert_rgb888_to_rgba(
        image.width,
        image.height,
        &image.pixels,
    )
    .map_err(|error| error.to_string())?;
    RgbaImage::from_rgba(image.width, image.height, rgba, instructions)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loop_rom() -> Vec<u8> {
        let mut rom = vec![0; 192];
        rom[..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        rom
    }

    #[test]
    fn rom_identity_and_length_reject_the_wrong_cartridge() {
        let session = GameSession::from_bytes(loop_rom(), AudioSettings::default())
            .expect("ROM with minimum bounded image size");
        assert!(session.identity().contains("192 bytes"));
        let binding = crate::rom_binding::RomBinding {
            title: "EXPECTED",
            game_code: "BPED",
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            len: 256,
            file_name: "expected.gba",
        };
        let error = session
            .verify_binding(binding)
            .expect_err("identity must differ");
        assert!(error.contains("ROM identity mismatch"));
    }

    #[test]
    fn unix_seconds_convert_to_a_calendar_rtc_snapshot() {
        let epoch = utc_from_unix_seconds(0);
        assert_eq!(
            (epoch.year, epoch.month, epoch.day, epoch.weekday),
            (1970, 1, 1, 4),
            "1970-01-01 was a Thursday with 0 = Sunday"
        );
        let leap = utc_from_unix_seconds(951_782_400); // 2000-02-29 00:00:00 UTC
        assert_eq!((leap.year, leap.month, leap.day), (2000, 2, 29));
        let late = utc_from_unix_seconds(1_791_331_199); // 2026-10-06 23:59:59 UTC
        assert_eq!(
            (
                late.year,
                late.month,
                late.day,
                late.hour,
                late.minute,
                late.second
            ),
            (2026, 10, 6, 23, 59, 59)
        );
    }

    #[test]
    fn malformed_rom_is_rejected_before_a_session_is_created() {
        assert!(GameSession::from_bytes(vec![0; 191], AudioSettings::default()).is_err());
    }
}
