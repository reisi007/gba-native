# Source layout

- `crates/core/src/cart.rs`, `bus.rs`, `bus/backup.rs`, `bus/eeprom.rs`,
  `cpu.rs`, `cpu/registers.rs`, `cpu/exception.rs`: header, explicit memory map,
  SRAM/flash/serial EEPROM device policies, CPU register banks, documented stack
  initialization and framed SWI/ARM exception entry/return.
- `crates/core/src/ir.rs`, `ir/`, `decode/`, `interpret.rs`, `interpret/`,
  `memory.rs`: typed IR, independent ARM/Thumb decode (`arm.rs`, `thumb.rs`,
  memory/block submodules, tests), reference semantics and shared pure
  address/width/sign-extension helpers.
- `crates/core/src/display.rs`, `display/` (`text.rs`, `affine.rs`, `obj.rs`,
  `window.rs`, `blend.rs`, `layer.rs`), `render.rs`: static Mode 0/1/2 subset
  and bitmap snapshot compositor, nearest/letterbox, PPM.
- `crates/core/src/demo.rs`, `demo/input_tester.rs`: entirely synthetic ROM
  fixtures, including the committed Mode 3 input-tester guest (ten KEYINPUT
  bands, deterministic expected frame).
- `crates/core/src/input/` (`mod.rs`, `keys.rs`, `touch.rs`, `settings.rs`,
  `script.rs`): portable button/key-binding model, normalised touch layout,
  persisted settings text format and instruction-indexed input scripts.
- `crates/core/src/perf.rs`: wall-time, guest instructions/second and the stated
  real-time factor against the documented GBA clock.
- `crates/core/src/save.rs`: host save library with multiple named slots per ROM
  SHA-256, a default marker and path-safe slot names; `Bus::backup_image` /
  `Bus::load_backup` move raw backup images in and out of the device model.
- `crates/core/src/audio.rs`, `audio/` (`direct_sound.rs`, `tests.rs`):
  dependency-free PSG mixer plus timer-clocked Direct Sound FIFO A/B playback,
  stateful `AudioEngine`, volume/mute settings and a WAV sink. It has no platform
  dependency; native live playback is target-specific and the browser uses
  WebAudio. Full PSG timing and live guest-PCM sinks remain tracked in #29.
- `crates/core/src/bios/`: six documentation-driven HLE services (`mod.rs`,
  `arithmetic.rs`, `memory.rs`), the opt-in documented decompression formats
  (`decompress.rs`) and `BgAffineSet`/`ObjAffineSet` (`affine.rs`, `affine/`),
  the shared destination classifier (`target.rs`), and typed guest SWI
  framing/dispatch (`swi.rs`).
- `crates/aot/src/aot/`: bounded discovery (`discover.rs`), trace-seed parsing
  and sealed work-RAM artifacts (`trace.rs`), artifact binding
  (`artifact.rs`), Rust emission (`emit.rs`, `emit_ops.rs`, `emit_mem.rs`,
  `emit_program.rs`, `emit_wasm.rs`), report parsing (`report.rs`),
  target-profile build/metadata (`target_build.rs`), harness build/run
  (`harness.rs`), the bounded wasm32 module build/metadata/pinned-CLI path
  (`wasm_build.rs`, `wasm_meta.rs`, `wasm_tool.rs`) and tests.
  This package depends on `gba-native-core` under the `gba_native` alias.
- Root `src/lib.rs`: compatibility facade re-exporting the core modules and
  `gba_native_aot::aot`; `src/main.rs`, `src/cli/`: CLI dispatch, commands
  (including the experimental `aot build` native and wasm32 branches) and safe
  `output/` writes.
- `tools/browser/`: pinned Playwright-only package and lock used to test the
  canonical Rune browser application. The bootstrap installs its dependencies
  under the ignored root `target/web-tools/` directory.
- `src/bin/quality-counter/`: the maintained-Rust population and 500-line
  counter used by the hard gate and Ratchet.
- `tests/`: root CLI-output, target-profile CLI build, quality-counter/script
  failure paths, Rune manifest/gate contract, browser-tool bootstrap regression
  and workspace compatibility contracts.
- `tests/roms/gba-tests/`: optional MIT-licensed local black-box ROM tests;
  immutable inputs, not part of `cargo test` or CI. See the directory README for
  cross-target execution instructions and visible-pass criteria.
- `tests/roms/synthetic/`: our own committed 4 KiB input-tester ROM, input
  script, expected frame and the 4 KiB save-tester ROM.
  `crates/core/tests/input_tester_fixture.rs` checks the generator/digests and
  reference frame; `scripts/input-tester.sh` builds and runs the native AOT
  artifact (visual Playwright comparison with `--visual`).
- `crates/core/tests/`: header/bus/EEPROM/CPU/display/demo tests, direct BIOS HLE
  and typed guest SWI decode/execution tests, independent ISA golden vectors
  and ARM/Thumb single/block/stack memory vectors.
- `crates/aot/tests/`: discovery metadata, PSR/halfword/ALU/memory/block/SWI
  fixtures (including the ARMv4 empty-list PC transfer), the native RAM
  self-modification regression, the target-profile build regression and the
  native AOT proof.
- `scripts/short.sh`, `scripts/full.sh`: documented fast and full gates;
  `scripts/native-consistency.sh` is the slow Ratchet counter.
- `crates/core/src/frontend/`: window-free frontend model shared by the
  Android and Rune apps (`image`, `viewport`, `ui` including
  `ui/shell.rs`, `settings`, `saves`, `tone`, `player`, `features`). It is the
  canonical feature contract.
- `apps/rune/`: canonical shared Rune UI app pinned to one upstream
  revision with its own lock. A shared Rune model/view targets Linux, macOS,
  Windows, browser/WASM and iOS; `www/` and `scripts/rune-web.sh` build its
  WebGPU browser shell, while `scripts/rune-web-screenshots.sh` checks the
  desktop settings/save flow and captures desktop/mobile review states. It reuses the core synthetic shell
  compositor, settings and save contracts, with platform adapters for host
  input, native/browser file operations and test-tone output. Mobile layouts
  wrap, stack and scroll. `scripts/rune-ios-simulator.sh` links device and
  simulator executables, packages and launches a simulator app, and captures
  its screen in macOS CI; physical-device signing/execution and interactive
  audible playback/document-picker behavior remain unverified. iOS save slots and WAV artifacts
  use the Files-shared Documents folder, with import from `saves/import.sav`.
  WebGL is not buildable with the pinned revision. `scripts/rune-native-snapshot.sh` exercises the native
  offscreen renderer; Linux CI provisions Mesa software Vulkan, not a physical
  GPU. Android remains on its existing adapter pending issue #31.
  `scripts/rune-web-serve.sh` builds and serves the browser app on loopback.
- `apps/android/`: separate NativeActivity package; `model.rs` is the
  host-tested shell (composition, touch, settings, saves, tone artifact) and
  `app.rs` the Android-only window and event loop.
- `scripts/android.sh`: Android NDK cross-build gate for the three ABIs, with
  ELF machine/interpreter verification; part of the `android-native` CI job.
- `scripts/android-app.sh`: builds and signs the synthetic APK for the three
  ABIs and verifies package, activity, native-code list and contents; part of
  the `android-app` CI job.
- `scripts/cross.sh`: optional local cross-build matrix for macOS arm64/x86_64,
  Linux x86_64 (zig), Windows x86_64 GNU (mingw-w64) and wasm32; it reports a
  missing linker instead of claiming success.
- `scripts/input-tester.sh`: native input-tester gate, optionally with the
  pinned Chromium/Playwright pixel comparison.
- `scripts/save-tester.sh`: native save-tester gate (fresh/loaded frames, two
  independent raw images, CLI slots/default/import/export); part of Linux CI.
- `scripts/play-tester.sh`: native playback gate; compiles the test-only
  `tests/support/fake_player.rs` helper, puts it first on `PATH`, and requires
  the CLI and generated AOT harness to invoke it with the generated WAV
  (`audio_played=true`), including the fail-closed nonzero-exit path. Part of
  Linux and Windows CI; no real audio device is required.
- `crates/core/tests/local_name_entry.rs`: ignored, environment-gated local
  retail name-entry regression (`GBA_NATIVE_LOCAL_ROM`); never part of CI.
- `scripts/frontend.sh`, `scripts/web.sh`: canonical Rune host/browser gates;
  `scripts/rune-web.sh` builds the WASM bundle and
  `scripts/rune-web-screenshots.sh` exercises the browser UI.
- `scripts/bootstrap-web-tools.sh`, `scripts/web-browsers-install.sh` and
  `tools/browser/`: pinned tooling and Chromium provisioning for Rune's browser
  gate. `scripts/web-gpu-launch-args.sh` supplies its SwiftShader test flags.
- `ratchet.config.jsonc`, `ratchet.baseline.json`: machine-managed quality
  rules and baseline.
