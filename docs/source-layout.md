# Source layout

- `crates/core/src/cart.rs`, `bus.rs`, `bus/backup.rs`, `cpu.rs`,
  `cpu/exception.rs`: header, explicit memory map, SRAM/flash device policies,
  state, documented stack initialization and framed SWI exception entry/return.
- `crates/core/src/ir.rs`, `ir/`, `decode/`, `interpret.rs`, `interpret/`,
  `memory.rs`: typed IR, independent ARM/Thumb decode (`arm.rs`, `thumb.rs`,
  memory/block submodules, tests), reference semantics and shared pure
  address/width/sign-extension helpers.
- `crates/core/src/display.rs`, `display/` (`text.rs`, `affine.rs`, `obj.rs`,
  `window.rs`, `blend.rs`, `layer.rs`), `render.rs`: static Mode 0/1 and bitmap
  snapshot compositor, nearest/letterbox, PPM.
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
- `apps/desktop/`: optional, separately excluded synthetic Winit/Pixels/WGPU
  package with its own workspace/lockfile. It depends directly on
  `crates/core` (as `gba-native-core`, aliased to `gba_native`), not on the AOT
  crate or the root facade, and has no ROM surface. `ui.rs` composes the
  windowed GBA shell, fullscreen frame and persisted settings page; `settings.rs`
  owns the per-user file; `app/input.rs` handles keyboard, pointer and touch.
- `apps/web/`: optional, separately excluded synthetic WASM/WebGPU/WebGL2
  package with its own Cargo/npm locks. It depends directly on `crates/core`,
  not the AOT crate or root facade; it accepts no ROM or guest audio and its
  save panel manages synthetic raw images only. Its
  committed `www/input.mjs` maps browser keys and touch buttons to KEYINPUT and
  persists the same settings fields in a JSON localStorage payload. `www/saves.mjs`
  is the browser save library (base64 raw images in localStorage, named slots,
  default, import/export) and is covered by Node tests and Playwright E2E.
- `src/bin/quality-counter/`: the maintained-Rust population and 500-line
  counter used by the hard gate and Ratchet.
- `tests/`: root CLI-output, target-profile CLI build, quality-counter/script
  failure paths, optional frontend gate contracts, web-tool bootstrap regression
  and workspace compatibility contracts.
- `tests/roms/gba-tests/`: optional MIT-licensed local black-box ROM tests;
  immutable inputs, not part of `cargo test` or CI. See the directory README for
  cross-target execution instructions and visible-pass criteria.
- `tests/roms/synthetic/`: our own committed 4 KiB input-tester ROM, input
  script, expected frame and the 4 KiB save-tester ROM.
  `crates/core/tests/input_tester_fixture.rs` checks the generator/digests and
  reference frame; `scripts/input-tester.sh` builds and runs the native AOT
  artifact (visual Playwright comparison with `--visual`).
- `crates/core/tests/`: header/bus/CPU/display/demo tests, direct BIOS HLE and
  typed guest SWI decode/execution tests, independent ISA golden vectors and
  ARM/Thumb single/block/stack memory vectors.
- `crates/aot/tests/`: discovery metadata, PSR/halfword/ALU/memory/block/SWI
  fixtures, the native RAM self-modification regression, the target-profile
  build regression and the native AOT proof.
- `scripts/short.sh`, `scripts/full.sh`: documented fast and full gates;
  `scripts/native-consistency.sh` is the slow Ratchet counter.
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
- `scripts/web.sh`, `scripts/web-e2e.sh`, `scripts/web-screenshots.sh` and
  `scripts/bootstrap-web-tools.sh`: isolated browser build/E2E/review flow and
  pinned repository-owned tooling.
- `ratchet.config.jsonc`, `ratchet.baseline.json`: machine-managed quality
  rules and baseline.
