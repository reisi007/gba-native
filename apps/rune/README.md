# Rune UI application

**Status:** canonical desktop, browser and iOS UI entrypoint; supported-platform
cutover gates passed on commit `cf2ac67` and the evidence is recorded in
[`docs/clean-room/verification.md`](../../docs/clean-room/verification.md).
Android stays on its existing adapter pending Rune Android support (tracked in
issue [#31](https://github.com/reisi007/gba-native/issues/31)).
This status does not claim physical-GPU, physical-iOS, audible-output or
assistive-technology behavior.

This isolated package shares one Rune `AppSpec`, view and host-state model across
Linux, macOS, Windows, browser/WASM and Rune's iOS target. It pins Rune to
`bca47138e9c31a7870a79272ee2523b592b1248d` and depends directly on
`crates/core`, not on the root facade or AOT tooling. The UI presents the core's
synthetic snapshot only: it does not load a ROM, run guest gameplay, or produce
guest audio or save data. Host-side synthetic saves and a generated test tone
are UI demonstrations, not emulation features. An instruction budget is not
frame or cycle timing.

## Current platform status

| Target | Implemented in this package | Verification limits |
|---|---|---|
| Linux, macOS, Windows | Shared Rune window and controls; persisted settings; native fullscreen; host keyboard/pointer input; synthetic save slots and isolated named profiles; native Open/Save dialogs; WAV artifact and host-player adapter. Linux dialogs use the desktop portal. | All three host jobs passed in Rune UI run `42470521196`; Linux also passed the offscreen Mesa software-Vulkan render. This is not a physical-GPU or live-window test. macOS/Windows GPU presentation remains unverified. |
| Browser/WASM | Rune WebGPU shell; localStorage settings and IndexedDB-backed synthetic save profiles/slots; browser file picker/download; Web Audio tone request. `www/app.js` derives slot-dependent action availability from the Rune accessibility mirror and marks those buttons disabled. | The current WASM, adapter and Chromium gates passed locally on 2026-11-09. Audio request is not proof of audible output. Browser screen-reader behavior is not verified: the adapter is tested against stubbed and live Chromium DOM nodes, not with assistive technology. |
| iOS device/simulator | Shared app code and the isolated native adapter compile for `aarch64-apple-ios` and `aarch64-apple-ios-sim`. Playback writes and opens the exact core-generated WAV through AVFoundation; mute and master volume are applied by the core artifact. Import presents a `.sav`-typed `UIDocumentPicker`, stages at most 128 KiB plus one rejection byte at `saves/import.sav`, then requires a second Import press for Rune's exact fixture validation. Exports remain in the Files-shared Documents folder. | Both target checks and the simulator package/install/launch smoke passed in run `42470521196`. Physical-device signing/execution, audible output, and actual document-picker interaction are unverified. Playback status means only that AVAudioPlayer accepted a request; there is no guest/emulator audio. |

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
