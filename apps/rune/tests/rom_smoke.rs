//! Local-only headless smoke for a user-supplied cartridge.
//!
//! This test never ships with a ROM and is skipped when `GBA_NATIVE_ROM_PATH` is
//! unset, so CI and the default gates stay free of retail images. Run it with:
//!
//! ```text
//! GBA_NATIVE_ROM_PATH=/path/to/game.gba \
//! GBA_NATIVE_PROGRAM_PATH=output/rune-bound/rom_program.rs \
//!   cargo test --manifest-path apps/rune/Cargo.toml --test rom_smoke -- --nocapture
//! ```
//!
//! `GBA_NATIVE_PROGRAM_PATH` is the program emitted by
//! `gba-native aot emit-program` for that image; `scripts/rune-bound.sh` emits it.
//!
//! `RUNE_SMOKE_POLLS` increases the bounded slice count; every poll runs
//! `INSTRUCTIONS_PER_POLL` guest instructions.

// The native session reads the image from a path; browser tests use bytes.
#![cfg(not(target_arch = "wasm32"))]

use gba_native::audio::AudioSettings;
use gba_native_rune::{GameSession, SessionPolicy};

#[test]
fn bounded_session_advances_the_supplied_cartridge() {
    let Some(path) = std::env::var_os("GBA_NATIVE_ROM_PATH") else {
        return;
    };
    let polls = std::env::var("RUNE_SMOKE_POLLS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(60);
    let mut session = GameSession::from_path_with_policy(
        std::path::Path::new(&path),
        AudioSettings::default(),
        SessionPolicy::documented_opt_ins(),
    )
    .expect("the supplied image loads");
    for _ in 0..polls {
        let before = session.instructions();
        session
            .advance_poll(
                0,
                AudioSettings::default(),
                gba_native_rune::GameSession::poll_budget(1.0 / 60.0),
            )
            .expect("the bounded session keeps running");
        if session.error().is_some() || session.instructions() == before {
            break;
        }
    }
    let samples = session.take_audio_samples();
    let peak = samples
        .iter()
        .map(|sample| sample.unsigned_abs())
        .max()
        .unwrap_or(0);
    println!(
        "identity={} instructions={} digest={} audio_samples={} audio_peak={} error={:?}",
        session.identity(),
        session.instructions(),
        session.image_digest(),
        samples.len(),
        peak,
        session.error()
    );
    assert!(session.instructions() > 0, "no guest instruction executed");
}
