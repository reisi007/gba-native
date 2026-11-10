#[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios"), not(rom_embedded)))]
use std::path::{Path, PathBuf};

use gba_native::{audio::AudioSettings, frontend::image::RgbaImage};

use super::{AppState, Page};

pub(super) fn start_bound_game(state: &mut AppState) {
    let Some(binding) = crate::rom_binding::BOUND_ROM else {
        state.game_status =
            Some("Build this app with GBA_NATIVE_ROM_PATH for one-game startup".to_owned());
        return;
    };
    state.select_page(Page::Game);
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios"), rom_embedded))]
    state.load_game_embedded();
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios"), not(rom_embedded)))]
    match bound_native_path(binding) {
        Ok(path) => state.load_game_path(&path),
        Err(error) => state.game_status = Some(format!("Could not locate bound game: {error}")),
    }
    #[cfg(target_arch = "wasm32")]
    {
        state.game_status = Some(format!(
            "Loading converted {} ({})",
            binding.title, binding.game_code
        ));
    }
    #[cfg(target_os = "ios")]
    {
        state.game_status =
            Some("iOS currently loads the fixed bound ROM from app-local storage.".to_owned());
    }
    let _ = binding;
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios"), not(rom_embedded)))]
fn bound_native_path(binding: crate::rom_binding::RomBinding) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("GBA_NATIVE_ROM_RUNTIME_PATH") {
        return Ok(PathBuf::from(path));
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let directory = executable
        .parent()
        .ok_or("executable has no parent directory")?;
    Ok(directory.join(binding.file_name))
}

pub(super) fn presentation(state: &AppState) -> Result<RgbaImage, String> {
    if state.model.page() != Page::Game {
        return state.model.composed_image();
    }
    let game = state
        .game
        .as_ref()
        .ok_or_else(|| "Bound game is not loaded".to_owned())?;
    Ok(game.presentation(
        state.model.input().mask(),
        state.model.settings().fullscreen,
        state.model.settings().touch_controls,
    ))
}

pub(super) fn poll_game(state: &mut AppState, _elapsed: f64) -> bool {
    if state.model.page() != Page::Game {
        return false;
    }
    #[cfg(target_arch = "wasm32")]
    if state.game.is_none() && crate::rom_binding::BOUND_ROM.is_some() {
        if let Some(error) = crate::browser_rom::take_error() {
            state.game_status = Some(format!("Bound game load failed: {error}"));
            return true;
        }
        let bytes = crate::browser_rom::take();
        if bytes.is_empty() {
            return false;
        }
        state.load_game_bytes(bytes);
        return true;
    }
    let Some(game) = &mut state.game else {
        return false;
    };
    if !game.running() {
        return false;
    }
    let settings = AudioSettings::new(
        state.model.settings().master_volume,
        state.model.settings().muted,
    );
    #[cfg(not(target_arch = "wasm32"))]
    let elapsed = {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(state.last_poll).as_secs_f64();
        state.last_poll = now;
        elapsed.min(0.25)
    };
    #[cfg(target_arch = "wasm32")]
    let elapsed = _elapsed;
    let budget = crate::game::GameSession::poll_budget(elapsed);
    if let Err(error) = game.advance_poll(state.model.input().mask(), settings, budget) {
        state.game_status = Some(format!("Execution stopped: {error}"));
    }
    #[cfg(target_arch = "wasm32")]
    let mut audio_unavailable = false;
    #[cfg(target_arch = "wasm32")]
    {
        let samples = game.take_audio_samples();
        if !samples.is_empty() && !crate::audio::play_guest_samples(&samples) {
            audio_unavailable = true;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    state.audio_sink.push(&game.take_audio_samples());
    #[cfg(target_arch = "wasm32")]
    if audio_unavailable {
        state.game_status = Some(
            "Web Audio is unavailable or blocked; the game continues without sound".to_owned(),
        );
    }
    state.refresh_composition();
    true
}

impl AppState {
    fn audio_settings(&self) -> AudioSettings {
        AudioSettings::new(
            self.model.settings().master_volume,
            self.model.settings().muted,
        )
    }

    fn install_game(&mut self, result: Result<crate::game::GameSession, String>) {
        match result {
            Ok(game) => {
                let verified = crate::rom_binding::BOUND_ROM
                    .map(|binding| game.verify_binding(binding))
                    .transpose();
                match verified {
                    Ok(_) => {
                        self.game_status = Some(format!(
                            "Loaded {} · {}",
                            game.identity(),
                            game.policy().describe()
                        ));
                        self.game = Some(game);
                        #[cfg(not(target_arch = "wasm32"))]
                        self.audio_sink.start();
                    }
                    Err(error) => {
                        self.game = None;
                        self.game_status = Some(error);
                    }
                }
            }
            Err(error) => {
                self.game = None;
                self.game_status = Some(format!("Could not load bound game: {error}"));
            }
        }
        self.refresh_composition();
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios"), not(rom_embedded)))]
    fn load_game_path(&mut self, path: &Path) {
        let settings = self.audio_settings();
        let policy = crate::game::SessionPolicy::documented_opt_ins();
        self.install_game(crate::game::GameSession::from_path_with_policy(
            path, settings, policy,
        ));
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios"), rom_embedded))]
    fn load_game_embedded(&mut self) {
        let settings = self.audio_settings();
        let policy = crate::game::SessionPolicy::documented_opt_ins();
        self.install_game(crate::game::GameSession::from_bytes_with_policy(
            crate::rom_binding::bound_rom_bytes().to_vec(),
            settings,
            policy,
        ));
    }

    #[cfg(target_arch = "wasm32")]
    fn load_game_bytes(&mut self, bytes: Vec<u8>) {
        let settings = self.audio_settings();
        let policy = crate::game::SessionPolicy::documented_opt_ins();
        self.install_game(crate::game::GameSession::from_bytes_with_policy(
            bytes, settings, policy,
        ));
    }
}
