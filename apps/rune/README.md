# Rune UI application

**Status:** canonical desktop, browser and iOS UI entrypoint; supported-platform
cutover gates passed on commit `cf2ac67` and the evidence is recorded in
[`docs/clean-room/verification.md`](../../docs/clean-room/verification.md).
Android stays on its existing adapter pending Rune Android support (tracked in
issue [#31](https://github.com/reisi007/gba-native/issues/31)).
This status does not claim physical-GPU, physical-iOS, audible-output or
assistive-technology behavior. A ROM-bound build also exists; it does not claim
Emerald/retail compatibility.

This isolated package shares one Rune `AppSpec`, view and host-state model across
Linux, macOS, Windows, browser/WASM and Rune's iOS target. It pins Rune to
`bca47138e9c31a7870a79272ee2523b592b1248d` and depends directly on
`crates/core` only; the translated program is pre-emitted by the host CLI, so the
app crate has no build-time dependency on `crates/aot`. The default, unbound
build keeps the Demo page with the core's static synthetic snapshot and the
Input Tester page, which runs the repository-generated input-test fixture with
the current host input, stopping after one completed guest drawing pass (capped
at 150,000 instructions).

Linux hosts need the ALSA development headers for the native output stream
(`libasound2-dev` and `pkg-config` on Debian/Ubuntu); browser and macOS builds do
not.

## One-game application builds

`GBA_NATIVE_ROM_PATH=/path/to/game.gba` turns the package into a one-game build:

- The host CLI (`gba-native aot emit-program <rom> output/rune-bound/rom_program.rs`)
  discovers reachable ROM blocks and emits target-neutral Rust; `build.rs` copies
  that pre-emitted program into `OUT_DIR` and compiles it into the executable.
  The app crate has no build-time dependency on the AOT tooling crate, and the
  original `.gba` is never copied into the source tree. `scripts/rune-bound.sh`
  runs both steps for desktop builds.
- The app title, SHA-256, length and game code are bound at build time, and the
  runtime rejects a different image before starting a session.
- At startup the app opens directly on its play screen, in fullscreen by
  default, and shows only the Game Boy shell with a directional pad, A/B,
  Start/Select and L/R. The 800x600 shell is aspect-fitted by the host GPU
  (bilinear sampling) instead of being CPU-upscaled, `F11` toggles the OS
  fullscreen state, and `Esc` leaves non-game pages. Generic Demo, Settings,
  Saves and ROM-picker navigation is hidden and its shortcuts are disabled in a
  bound build; `M`, `-`, `=` and (outside iOS) `P` stay available for audio.
- A native output stream (CoreAudio/WASAPI/ALSA through `cpal`) plays the core's
  rendered PCM continuously. The mixer state persists across poll slices, so PSG
  phases and Direct Sound FIFOs are not restarted every frame.
- Non-embedded desktop builds locate the fixed cartridge next to the executable
  (override with `GBA_NATIVE_ROM_RUNTIME_PATH`); embedded builds read the image
  from the executable instead. Browser builds fetch the fixed file name from the
  same origin; `scripts/rune-web.sh` copies it into the local bundle when
  `GBA_NATIVE_ROM_PATH` is set.
- `GBA_NATIVE_EMBED_ROM=1` additionally compiles the bound image into the
  executable for a local single-file desktop build. The switch is off by
  default, refuses to run in CI and refuses `wasm32`; the resulting binary is
  retail-derived, stays under the git-ignored `target/`, and must never be
  committed, published or attached to a CI artifact (decision D54):

  ```text
  GBA_NATIVE_ROM_PATH=/path/to/game.gba GBA_NATIVE_EMBED_ROM=1 \
    scripts/rune-bound.sh --release --target x86_64-pc-windows-gnu
  ```

  That produces
  `target/rune-bound/x86_64-pc-windows-gnu/release/gba-native-rune.exe`, which
  starts without a sibling `.gba`. Building for the host produces the equivalent
  local single-file binary. Without `GBA_NATIVE_EMBED_ROM`, the build keeps the
  external runtime image and no cartridge bytes enter the executable.

The runtime remains bounded: translated ROM blocks and interpreter fallbacks run
in 100,000-instruction poll slices, and dynamic RAM code still goes through a
fresh interpreter decode. **Session policy note:** the one-game builds enable the
documented opt-in policies, including the host-seeded GPIO real-time clock. The
core contract keeps GPIO-enabled buses on the reference interpreter, so these
sessions execute the interpreter even though the generated translated program is
compiled in and bound to the image; the native dispatcher path is covered by the
AOT proof and IR-parity tests, not by this app. The input mapping follows the
shared settings; host input injection has no hardware sampling/timing guarantee.
The browser streams renderer samples to Web Audio; native output goes through the
persistent `cpal` sink.

This is an experimental interactive runtime path, not BIOS execution or a retail
compatibility claim. The default strict bus and HLE policies are unchanged.
Unsupported instructions, I/O, SWIs or PPU state stop the session explicitly. The
PPU is a snapshot renderer, not a scanline/frame renderer; instruction counts are
not cycles or frames. Game save data is not persisted; the existing save pages
still manage synthetic host fixtures only. iOS currently loads its fixed ROM from
app-local storage and has no picker; Android remains on its separate synthetic
frontend while Rune lacks documented Android support.

**Self-containment limit:** the executable integrates the generated translated
program, but the current core still reads cartridge data and interpreter
fallbacks from the original image. A default build is therefore not a single-file
executable; it needs the bound `.gba` next to it (desktop) or in the same-origin
bundle (web). The opt-in `GBA_NATIVE_EMBED_ROM=1` build (D54) embeds the image for
local single-file use. Removing the external-image dependency for ordinary builds
requires a complete converted data image and is tracked in issue #33.

Local ROM-bound checks use two environment switches beyond
`GBA_NATIVE_ROM_PATH`: `scripts/rune-bound.sh` builds desktop binaries, and the
bound-game Chromium check runs with

```text
GBA_NATIVE_ROM_PATH=/path/to/game.gba RUNE_EMBEDDED_PROGRAM=1 \
  RUNE_SCREENSHOT_GREP='bound game' RUNE_SCREENSHOT_PROJECT=desktop \
  scripts/rune-web-screenshots.sh
```

`RUNE_EMBEDDED_PROGRAM=1` only marks the bundle as ROM-bound so the test is not
skipped; the bundle itself is produced by `scripts/rune-web.sh`.

## Current platform status

| Target | Implemented in this package | Verification limits |
|---|---|---|
| Linux, macOS, Windows | Shared Rune window; persisted settings; native fullscreen; host keyboard/pointer input; synthetic save slots/profiles; native Open/Save dialogs; one-game ROM-bound build with generated translated blocks; live output through `cpal` (CoreAudio/WASAPI/ALSA). | A local macOS run reaches the Emerald title screen and produces non-silent PCM (`audio_peak=14874` over 450M guest instructions). Linux/Windows runtime presentation and audible verification remain outstanding; cross-target compile checks are recorded in the verification log. |
| Browser/WASM | Rune WebGPU shell; localStorage settings and IndexedDB-backed synthetic save profiles/slots; same-origin bound `.gba` fetch, WASM reference interpreter, keyboard/touch controls and Web Audio sample path. | Requires a ROM-bound bundle built with `GBA_NATIVE_ROM_PATH`; the copied ROM file is local-only. Web Audio requests do not prove audible output; the Chromium smoke checks rendering and input, not sound. |
| iOS device/simulator | Shared app code and the isolated native adapter compile for iOS targets. Existing synthetic save import/export and test-tone behavior remain available. | The one-game ROM path is not wired to an iOS document source; physical-device signing/execution and actual picker interaction remain unverified. |

The Rune-specific host, browser and iOS runtime gates above are separately
recorded in the verification log. A successful cross-build or browser capture
alone is not runtime parity.

The Rune host/browser feature declaration is checked against the full core
contract. The iOS feature subset is tested against the isolated adapter's
capabilities, including native test-tone playback and save import picking.

Rune disables Export, Make default and Delete while no slot is selected, and
disables Make default when the selected slot is already default. Rune's incoming
Import action intentionally accepts only an exact byte-for-byte generated
512-byte `GBANSV01` fixture. Newly created synthetic slots use those same
fixtures. Existing host slots follow the core raw-image contract:
Rune lists, displays, defaults, deletes and exports any non-empty image up to
128 KiB byte-for-byte, including images without a `GBANSV01` prefix. This is
host-side persistence only, not guest save emulation or a retail compatibility
claim. Native imports read at most 128 KiB plus one rejection byte; the browser
picker rejects oversized files before reading their payload; core save loads
use the same bounded-read policy. The iOS picker accepts `.sav` documents only,
and Rune applies the same exact-fixture restriction after selection.

Native desktop/iOS profiles use marker-validated directories under the existing
per-fixture save root. New safe IDs are initialized only in absent namespaces;
unowned and symlinked directories are refused. The default profile keeps its
legacy path, and startup selects it while named profile slots remain registered
for later selection. This is host persistence only, not cartridge-save
completion or retail save compatibility. Browser profile behavior is unchanged.

Executable Rune UI contract coverage is in `apps/rune/src/ui/parity_tests.rs`.
It checks Rune-only save creation, default selection, export/delete and
collision-safe slot naming, verifies settings transitions, and checks that the
demo shell, pressed input, fullscreen demo and settings page use the shared core
contracts.

The settings test clicks Rune controls for touch controls, integer scaling,
mute, volume and the A binding, then checks the shared `Settings` value and
canonical persisted document. Browser-specific storage behavior is covered by
Rune's browser-shell and browser-save adapter tests.

Browser save profiles and their slots/default markers are stored together in
IndexedDB read/write transactions. No migration from the pre-release
`localStorage` save keys is performed: those keys are neither read nor deleted.
The import picker uses a separate temporary `localStorage` staging key, which is
consumed only after the IndexedDB import transaction commits. Browser storage is
still subject to quota, private-mode restrictions and eviction; IndexedDB does
not guarantee permanent storage, so export backups for portability.

Growing slot lists land their layout in the same frame
(`El::instant_changes`), so the action row cannot overlap a just-added row while
the list animates.

## Presentation and input

The synthetic shell is composed by the shared core UI. Rune draws its RGBA
surface at device-pixel size. It uses integer nearest-neighbor expansion when
the available device width permits; on a narrower viewport it nearest-samples
to the available width rather than cropping the shell horizontally. Mobile
settings and save pages use a compact toolbar, wrapped text, stacked actions and
a vertical scroll viewport. Mobile-landscape content remains scrollable.
Fractional display scales are not filtered by Rune, but narrow-screen
downsampling may reduce source detail.

The default web build is WebGPU-only. The optional `webgl` feature is exposed
for investigation but does not compile with this Rune revision: Rune's shared
instance cache requires `Send`/`Sync` values that the WebGL backend does not
provide. No WebGL2 fallback is claimed. Native AccessKit adapters are enabled;
web accessibility is not yet verified.

## Canonical browser bundle, interaction check and visual review

From the repository root:

```sh
scripts/web-browsers-install.sh
scripts/rune-web-serve.sh # build and serve the app on loopback
scripts/web.sh --ci       # full browser gate, including Playwright interactions
```

`web.sh` is the canonical browser gate: it builds an isolated WASM bundle under a unique
`target/rune-web-*` directory using pinned `wasm-bindgen 0.2.129`. The
gate runs the desktop settings/save interaction check and the
Playwright capture set for desktop, mobile portrait and mobile landscape,
including empty/filled save states and scrolled sections. PNGs are written under
an ignored `output/rune-web-review-*` directory for visual inspection. This is
not evidence of physical-GPU or iOS behavior.

Browser adapter unit tests use the pinned Node installation after bootstrap:

```sh
target/tools/node-pinned/bin/node --experimental-vm-modules --test \
  apps/rune/tests/browser-shell.test.mjs apps/rune/tests/browser-save.test.mjs
```

The combined reproducible browser gate is `scripts/web.sh`; tool and Chromium
provisioning is explicit through `scripts/web-browsers-install.sh`. The browser
interaction checks also cover settings persistence and exact synthetic-save
export/import round trips. Set `RUNE_SCREENSHOT_GREP` and
`RUNE_SCREENSHOT_PROJECT` to run a focused Playwright subset; the default run
captures all desktop/mobile states and executes the desktop interaction test.

## iOS simulator smoke

On macOS with Xcode, the simulator runtime and both Rust iOS targets installed,
run:

```sh
scripts/rune-ios-simulator.sh
```

The script builds linked device and simulator executables, packages an ad-hoc
simulator `.app`, installs and launches it on an available iPhone simulator,
and saves a launch screenshot in a unique `output/rune-ios-review-*` directory.
It does not sign for a physical device or verify audible output, actual
document-picker interaction, screen-reader behavior or feature parity.

## Native launch and headless checks

Launch the native Rune app on Linux, macOS or Windows:

```sh
CARGO_TARGET_DIR=target/rune cargo run --locked --manifest-path apps/rune/Cargo.toml
```

The canonical native host gate is `scripts/frontend.sh`; it is headless and
does not open a live GPU window.

## Headless checks

Run from the repository root so all Cargo artifacts stay under the ignored root
`target/` directory:

```sh
cargo fmt --manifest-path apps/rune/Cargo.toml --all --check
CARGO_TARGET_DIR=target/rune cargo test --locked --manifest-path apps/rune/Cargo.toml
CARGO_TARGET_DIR=target/rune cargo test --locked --manifest-path apps/rune/ios-platform/Cargo.toml
CARGO_TARGET_DIR=target/rune cargo clippy --locked --manifest-path apps/rune/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR=target/rune cargo check --locked --manifest-path apps/rune/Cargo.toml --all-targets --target wasm32-unknown-unknown
```

Native desktop target checks are compile-only and do not open windows or verify
GPU presentation. `scripts/rune-native-snapshot.sh` renders a native PNG using
the installed GPU backend and checks its dimensions; Linux CI provisions Mesa
software Vulkan for this, not a physical GPU. The iOS simulator smoke is a
separate macOS-only package and launch gate; it compiles both adapter targets
but does not exercise audible playback, document-picker interaction or a
physical device. Rune headless UI tests cover shared state, settings, saves, viewport
bounds and scrolling. The browser run includes a desktop interaction flow;
mobile runs are capture-only, and no browser screenshot proves screen-reader or
physical-GPU behavior.
