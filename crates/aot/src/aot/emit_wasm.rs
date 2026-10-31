//! Emission of the ROM-bound WASM execution module.
//!
//! The module is the target-neutral program section plus a wasm-bindgen driver
//! whose only boundary is a caller-supplied `Uint8Array`. The generated source
//! embeds the expected SHA-256 and ROM length, never ROM bytes, and performs no
//! filesystem, process, environment or stdout work. It is a browser-callable
//! execution module, not an HTML/JS application bundle.

use super::artifact::AotArtifact;
use super::emit_program::emit_program_module;
use super::policy::{HarnessPolicy, bus_setup_source};
use gba_native::cart::Cartridge;
use gba_native::error::Result;

/// Compose the target-neutral program section with the WASM driver.
///
/// The budget is fixed at build time and travels inside the module; the caller
/// supplies only the external ROM bytes at call time.
pub fn emit_wasm_module(cart: &Cartridge, artifact: &AotArtifact, budget: u64) -> Result<String> {
    emit_wasm_module_with_policy(cart, artifact, budget, HarnessPolicy::default())
}

/// Compose the WASM driver with explicit synthetic device policies.
///
/// The policy statements are baked into the generated module exactly like the
/// native harness; the default remains the fail-closed strict policy.
pub fn emit_wasm_module_with_policy(
    cart: &Cartridge,
    artifact: &AotArtifact,
    budget: u64,
    policy: HarnessPolicy,
) -> Result<String> {
    let mut out = emit_program_module(cart, artifact)?;
    out.push_str(&driver_source(artifact, budget, policy));
    Ok(out)
}

/// Render the WASM driver with the exact binding constants substituted.
///
/// The template carries no ROM data; only the expected digest, the bounded
/// length and the build-time budget are rendered into it.
pub(super) fn driver_source(artifact: &AotArtifact, budget: u64, policy: HarnessPolicy) -> String {
    DRIVER_TEMPLATE
        .replace("@ROM_SHA256@", &artifact.rom_sha256_hex())
        .replace("@ROM_LEN@", &artifact.rom_len.to_string())
        .replace("@BUDGET@", &budget.to_string())
        .replace("@BUS_SETUP@", &bus_setup_source(policy))
}

/// Driver template. `run_rom` checks length and SHA-256 before constructing a
/// cartridge, CPU or bus, then returns a compact state report. Core errors are
/// propagated as JavaScript errors; there is no host boundary beyond the
/// `Uint8Array` argument.
const DRIVER_TEMPLATE: &str = r#"
// WASM driver: the only boundary is a caller-supplied Uint8Array. This module
// embeds the expected digest and length, never ROM bytes, and has no
// filesystem, process, environment or stdout access.
use gba_native::cart::Cartridge;
use gba_native::sha256;
use wasm_bindgen::prelude::*;

const ROM_SHA256: &str = "@ROM_SHA256@";
const ROM_LEN: usize = @ROM_LEN@;
const BUDGET: u64 = @BUDGET@;

/// Check the external image before any cartridge, CPU or bus state exists.
fn verify_binding(bytes: &[u8]) -> gba_native::error::Result<()> {
    if bytes.len() != ROM_LEN {
        return Err(gba_native::error::Error::RomMismatch {
            expected: ROM_SHA256.to_string(),
            actual: format!("{} bytes (expected {} bytes)", bytes.len(), ROM_LEN),
        });
    }
    let actual = sha256::hex(&sha256::digest(bytes));
    if actual != ROM_SHA256 {
        return Err(gba_native::error::Error::RomMismatch {
            expected: ROM_SHA256.to_string(),
            actual,
        });
    }
    Ok(())
}

fn execute(
    bytes: Vec<u8>,
    audio: &mut Option<gba_native::audio::AudioEngine>,
) -> gba_native::error::Result<String> {
    verify_binding(&bytes)?;
    let cart = Cartridge::from_bytes(bytes)?;
@BUS_SETUP@
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(ENTRY_THUMB);
    let program = run_program(
        &mut cpu,
        &mut bus,
        BUDGET,
        &gba_native::input::InputScript::default(),
        audio,
    )?;
    let mut report = String::new();
    report.push_str("status=ok\n");
    report.push_str(&format!("pc=0x{:08X}\n", cpu.pc()));
    report.push_str(&format!("cpsr=0x{:08X}\n", cpu.cpsr()));
    report.push_str(&format!("thumb={}\n", if cpu.thumb() { 1 } else { 0 }));
    report.push_str(&format!("budget={}\n", BUDGET));
    report.push_str(&format!(
        "budget_remaining={}\n",
        program.budget_remaining
    ));
    report.push_str(&format!(
        "budget_used={}\n",
        BUDGET - program.budget_remaining
    ));
    report.push_str(&format!("fallbacks={}\n", program.fallbacks));
    report.push_str(&format!(
        "cpu_sha256={}\n",
        sha256::hex(&cpu.state_digest())
    ));
    report.push_str(&format!(
        "bus_sha256={}\n",
        sha256::hex(&bus.state_digest())
    ));
    report.push_str(&format!("rom_sha256={}\n", bus.cart().sha256_hex()));
    report.push_str("embedded_rom_bytes=0\n");
    report.push_str("requires_external_rom=true\n");
    Ok(report)
}

/// Execute the ROM-bound program from a browser-supplied `Uint8Array`.
///
/// The length and SHA-256 are checked before the cartridge, CPU or bus is
/// constructed. A wrong image throws a JavaScript error; on success this
/// returns a compact `key=value` state report. The caller supplies the ROM
/// bytes; this module never contains, fetches or writes them.
#[wasm_bindgen]
pub fn run_rom(bytes: Vec<u8>) -> std::result::Result<String, JsValue> {
    let mut audio = None;
    execute(bytes, &mut audio).map_err(|error| JsValue::from_str(&format!("{error}")))
}

/// Execute the same bounded program and return guest-state PCM.
///
/// The samples are mono signed 16-bit PSG plus Direct Sound at
/// `audio_sample_rate()`; the caller schedules playback. No unbounded run,
/// filesystem access, speaker access or UI exists here. The ROM binding checks
/// are identical to `run_rom`, so a wrong image throws before any guest state
/// is constructed.
#[wasm_bindgen]
pub fn run_rom_audio(bytes: Vec<u8>) -> std::result::Result<Vec<i16>, JsValue> {
    let mut audio = Some(gba_native::audio::AudioEngine::new(
        gba_native::audio::AudioSettings::default(),
    ));
    execute(bytes, &mut audio).map_err(|error| JsValue::from_str(&format!("{error}")))?;
    let mut engine = audio.expect("the audio engine stays present for this call");
    Ok(engine.take_samples())
}

/// Host render rate of the PCM returned by `run_rom_audio`.
#[wasm_bindgen]
pub fn audio_sample_rate() -> u32 {
    gba_native::audio::SAMPLE_RATE
}
"#;
