# Repository agent instructions

## What this repository is

Experimental hybrid GBA-to-native prototype in Rust 2024, std-only. It is not a
playable emulator, not a retail-game compatibility claim, not a vendor-GPU/FSR/
DLSS integration, and not a formal two-team legal clean-room. REA native
analysis remains blocked and no REA Evidence exists. Read
`docs/clean-room/protocol.md` and `docs/clean-room/gba-contract.md` before
changing behavior.

## Issue tracker

GitHub Issues are the authoritative open-work record:
<https://github.com/reisi007/gba-native/issues>. Do not add a second local
backlog. An explicit user commission is still required before work starts, and
an issue is closed only after independent verification, the repository gates
and CI pass, with the issue reference in the change. Lasting decisions live in
`docs/decisions.md`; completed work is recorded in Git.

## Green build gates

The workspace has three internal packages: the root `gba-native` CLI plus
compatibility facade, `crates/core` (`gba-native-core`, std-only runtime) and
`crates/aot` (`gba-native-aot`, host-side AOT tooling). The root
`[workspace]` lists all three in `default-members`; `apps/desktop` and `apps/web`
are excluded. A bare `cargo test` therefore runs core, AOT and root tests, while a bare
`cargo run` uses the root package's `default-run = "gba-native"` binary.
Per-package loops are `cargo test -p gba-native-core`, `cargo test -p
gba-native-aot` and `cargo test -p gba-native`; the workspace lockfile is shared.

The documented entry points are:

```text
./scripts/short.sh   # fast: hard 500-line gate, Ratchet fast, fmt, clippy, fast tests
./scripts/full.sh    # short + debug/release tests, demo, native proof, Ratchet slow
```

The optional synthetic desktop GPU frontend is a separate package and gate:

```text
./scripts/frontend.sh
```

It is not part of `short.sh`/`full.sh`, does not require Ratchet and never opens
a GPU window. It overrides `CARGO_TARGET_DIR` to the root-level ignored
`target/frontend` before any frontend Cargo command. Never run frontend Cargo
from inside `apps/desktop/` without a root-level target: generated
`apps/desktop/target/*.rs` files would enter the maintained-source scan.

The synthetic browser GPU foundation is also a separate package and gate:

```text
./scripts/web.sh          # full browser gate, including WASM and functional E2E
./scripts/web.sh --ci     # same fail-closed browser checks with CI GPU flags
./scripts/web.sh --build-only  # partial gate, skips browser E2E
```

`apps/web` has its own Cargo/npm locks, depends directly on `crates/core`, and is
not a ROM, input, audio or save surface. The gate uses repository-owned pinned
tools under root `target/`; do not install global tools or run Cargo inside
`apps/web/` without a root-level `CARGO_TARGET_DIR`. It is independent of
`short.sh`/`full.sh` and the default three-package workspace.

CI runs the same two scripts. A separate `macos-arm-native-aot` job on
`macos-latest` asserts an `aarch64-apple-darwin` host and runs the synthetic
`aot_target_build` and `cli_aot_build` integration targets for ROM-bound native
AOT. That job builds for its ARM runner host; it is not the x86 cross-target
test described by the optional local ROM procedure or a platform-support claim.
CI uses repository-generated synthetic fixtures; the separately documented
hardware-test ROMs under `tests/roms/gba-tests/` are local-only, read-only test
inputs and must never be fetched or executed by CI, or included in CI jobs or
artifacts. Their optional local manual use is documented with the ROMs.
The optional desktop and browser workflows run
`scripts/frontend.sh` and `scripts/web.sh --ci` separately. The underlying root
gates remain:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --release
```

Ratchet is pinned to release `v0.5.0` of `all-the-rest/ratchet-cli` (peeled
commit `99c25b1e18331a1473d43091d37e23f23bc1c30d`; the annotated tag object is
`1feb7b5fae653272a922ed9f0479660ecde22ed2`). The scripts use `RATCHET_BIN` when
set, otherwise `ratchet` on `PATH`, and fail with diagnostics when the tool is
missing or reports another version. Baselines are machine-written with
`ratchet baseline init/update`; never hand-edit `ratchet.baseline.json`.

## Source size limit

Every maintained Rust source or test file has at most 500 physical lines,
including blank and comment lines; a trailing newline adds no phantom line.
There are no source exemptions. Generated build/runtime artifacts in the
root-level ignored directories `target/`, `output/` and `.git` are not
maintained sources and are skipped by the counter. Those names are exempt only
directly at the scan root: a nested `src/output/`, `tests/target/` or similar is
ordinary source-tree structure and is scanned. Every other `*.rs` file counts,
including root files, `build.rs`, tests and new untracked sources; an empty
population is an error.

`src/bin/quality-counter` is the single counting implementation. It feeds the
hard gate (`--hard`), the Ratchet rules and reports. The hard gate is enforced
directly by `scripts/short.sh` and CI as well as by a zero-debt Ratchet ceiling,
so a baseline edit or `baseline raise` cannot dilute it.

`cargo test` includes the substantive AOT integration test in
`crates/aot/tests/aot_native.rs`. That test writes an independently generated
synthetic ROM fixture, generates a Rust harness that binds SHA-256+length (never
an embedded ROM array), builds it with the installed `cargo` inside an owned
temporary directory, sets a separate `CARGO_TARGET_DIR`, uses `--offline`,
executes it with an explicit `--rom PATH`, and compares the native state with the
IR reference. The generated manifest depends only on the sibling
`gba-native-core` package (renamed to the `gba_native` source namespace), never
on the AOT crate or the root facade; the test asserts the actual Cargo tree. It
also checks fallback, wrong-ROM rejection and an independently expected
immediate-family regression. `crates/core/tests/isa_golden.rs` checks
decode/flags/memory against hand-derived ISA expectations.

## Fresh independent verification

Use a clean target directory so stale artifacts cannot hide a failure:

```text
CARGO_TARGET_DIR="$(mktemp -d)/target" cargo test
CARGO_TARGET_DIR="$(mktemp -d)/target" cargo test --release
cargo run -- demo --out output/verify-demo.ppm
cargo run -- aot proof --out output/verify-aot
```

Do not delete or overwrite `output/` contents that you did not create. All
generated artifacts belong under the git-ignored `output/` directory or an owned
temporary directory.

## End-to-end evidence commands

```text
cargo run -- demo --size 480x320 --backend nearest
cargo run -- aot proof
cargo run -- info "<optional user-supplied .gba>"   # read-only
```

Expected native proof properties: 4 discovered ROM blocks / 17 translated
instructions, 230,411 guest instructions, 0 fallbacks, and
`native_vs_reference=match`. This is evidence for the synthetic fixture only.

## Hard boundaries

- Never modify, copy, embed or newly commit a user-supplied or vendored `.gba`
  file, Nintendo logo bytes, BIOS bytes or save data. All such ROMs are
  read-only inputs. Never copy ROM bytes into generated output or CI artifacts,
  publish them, or include their binaries in a CI workflow; local manual
  execution is optional and must follow `tests/roms/gba-tests/README.md`.
  Generated AOT source must not contain a ROM byte array or raw image copy; bind
  SHA-256+length and require a caller-supplied `--rom PATH`.
- Do not use third-party emulator implementation sources, game decompilations or
  leaked SDKs. BIOS disassembly analysis remains prohibited except that a
  separate analyst may inspect only public GBA BIOS `RegisterRamReset` (`01h`)
  material through the reviewed behavior-only process in
  `docs/clean-room/protocol.md`; all other BIOS analysis remains prohibited.
- `#![forbid(unsafe_code)]` applies to the core, the AOT crate and the root
  facade. Do not add `unsafe`, JIT memory mappings, executable heap memory or a
  dependency on a JIT.
- Do not silently turn unsupported instructions, I/O, PPU features or vendor
  backends into successful no-ops or nearest-neighbor fallbacks.
- Guest SWI support is limited to the six documented HLE services by default.
  The documented LZ77/RL/diff decompression SWIs and
  `BgAffineSet`/`ObjAffineSet` (`0Eh`/`0Fh`) are explicit, host-selected
  opt-ins (`DecompressionPolicy::DocumentedFormats`,
  `AffinePolicy::Documented`); do not enable them by default or add further
  services without a reviewed contract change. Preserve
  explicit errors for unsupported services and SVC-origin nested SWIs; do not
  fetch or synthesize BIOS instruction bytes. The current ascending SVC-frame
  slot order is a bounded design policy, not a Nintendo BIOS compatibility
  claim.
- CPU instruction budgets are not frame or cycle timing. Do not report a
  completed budget as a booted game or a rendered frame from hardware timing.
- The default third-party dependency set stays empty: the workspace lockfile
  contains only the three internal path packages. Tests must not require network
  access, commercial ROMs, BIOS files or external crates. The MIT-licensed
  hardware-test ROMs in `tests/roms/gba-tests/` are optional local black-box
  tests, not part of `cargo test` or CI. A budget stop is not a ROM pass: only
  the ROM's own visible PASS result counts. The optional `apps/desktop` and
  `apps/web` packages have their own locks and are excluded from the default
  graph; browser registry dependencies and Node/npm tools stay outside the root
  lock and root build.
- Static AOT discovery is restricted to immutable ROM bytes. The explicit
  trace-seeded mode (`aot build --trace`) may emit sealed one-instruction
  work-RAM blocks only when the generated dispatcher re-reads the live encoding
  and executes the native block solely on an exact match; any mismatch must go
  through the reference interpreter with a fresh fetch/decode and count as a
  fallback. Sealed blocks are profile-guided translations of observed
  instructions, not general RAM translation, a RAM snapshot, or a whole-ROM
  coverage claim. Follow-on RAM code is never statically discovered.

## Source layout

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
- `crates/core/src/audio.rs`: dependency-free PSG mixer, stateful `AudioEngine`,
  volume/mute settings and a WAV sink. It has no platform dependency; native
  live playback is target-specific and the browser uses WebAudio. Full PSG
  timing and Direct Sound FIFO playback are tracked in #29.
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
  not the AOT crate or root facade; it accepts no ROM, save or audio. Its
  committed `www/input.mjs` maps browser keys and touch buttons to KEYINPUT and
  persists the same settings fields in a JSON localStorage payload.
- `src/bin/quality-counter/`: the maintained-Rust population and 500-line
  counter used by the hard gate and Ratchet.
- `tests/`: root CLI-output, target-profile CLI build, quality-counter/script
  failure paths, optional frontend gate contracts, web-tool bootstrap regression
  and workspace compatibility contracts.
- `tests/roms/gba-tests/`: optional MIT-licensed local black-box ROM tests;
  immutable inputs, not part of `cargo test` or CI. See the directory README for
  cross-target execution instructions and visible-pass criteria.
- `tests/roms/synthetic/`: our own committed 4 KiB input-tester ROM, input
  script and expected frame. `crates/core/tests/input_tester_fixture.rs` checks
  the generator/digests and reference frame; `scripts/input-tester.sh` builds and
  runs the native AOT artifact (visual Playwright comparison with `--visual`).
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
- `crates/core/tests/local_name_entry.rs`: ignored, environment-gated local
  retail name-entry regression (`GBA_NATIVE_LOCAL_ROM`); never part of CI.
- `scripts/web.sh`, `scripts/web-e2e.sh`, `scripts/web-screenshots.sh` and
  `scripts/bootstrap-web-tools.sh`: isolated browser build/E2E/review flow and
  pinned repository-owned tooling.
- `ratchet.config.jsonc`, `ratchet.baseline.json`: machine-managed quality
  rules and baseline.

## Licensing / publication

No license has been selected yet. After the content/history audit, the owner
may make the repository public before the first CI push; automation does not
change visibility. Do not publish ROMs, BIOS files, save data, generated
retail-derived translations or vendor SDK/model weights.
