# Rune UI transition app

**Status:** additive migration prototype; not a frontend cutover. Issue [#30](https://github.com/reisi007/gba-native/issues/30)
tracks platform parity. Existing desktop and browser frontends stay in place
until equivalent behavior is verified. Android is outside this Rune migration.

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
| Linux, macOS, Windows | Shared Rune window and controls; persisted settings; native fullscreen; host keyboard/pointer input; synthetic save slots; native Open/Save dialogs; WAV artifact and host-player adapter. Linux dialogs use the desktop portal. | Cross-target checks pass. CI also runs a Linux offscreen render with Mesa software Vulkan; it is not a physical-GPU or live-window test. macOS/Windows GPU presentation remains unverified. |
| Browser/WASM | Rune WebGPU shell; localStorage settings and synthetic slots; browser file picker/download; Web Audio tone request. `www/app.js` derives slot-dependent action availability from the Rune accessibility mirror and marks those buttons disabled. | Chromium screenshot review and WASM build pass. Audio request is not proof of audible output. Browser screen-reader behavior is not verified: the adapter is tested against stubbed and live Chromium DOM nodes, not with assistive technology. |
| iOS device/simulator | Shared app code compiles for `aarch64-apple-ios` and `aarch64-apple-ios-sim`; settings use app-support storage; synthetic saves and WAV artifacts use the Files-shared Documents folder. | `scripts/rune-ios-simulator.sh` builds both Apple targets, packages and launches the simulator app, and captures its screen in macOS CI; that workflow gate has not run here. Physical-device signing/execution is unverified. There is no native audio playback or document-picker adapter: copy an exact fixture to `On My iPhone/iPad/GBA Native Rune/saves/import.sav`, then use Import; exports appear in that folder. |

The existing frontends remain the behavioral reference during this additive
transition. A successful cross-build or browser capture is not runtime parity.
Do not claim the migration complete until the remaining platform and behavior
gates in issue #30 have independent evidence.

The Rune host/browser feature declaration is checked against the full core
contract. The iOS feature subset is also tested explicitly; it intentionally
omits native audio playback, which is not counted as migrated until an iOS
adapter exists.

Save-action affordances differ deliberately from the existing frontends. Rune
disables Export, Make default and Delete while no slot is selected, and disables
Make default when the selected slot is already default; the desktop saves page
leaves those controls enabled and treats the same actions as silent no-ops. The
core save library, slot naming and exact-fixture validation are shared, so this
is an affordance difference, not a different save format.

Executable parity coverage is in `apps/rune/src/ui/parity_tests.rs`. It drives
the real desktop save state machine (`apps/desktop/src/saves.rs`) and Rune's UI
through the same sequence — create two slots, make the second default, export,
delete — and requires equal slot names (`slot<n>`), zero-based fixture seeds,
default markers and exported bytes. It also compares Rune's demo shell (empty
and pressed input), fullscreen demo and settings page against the shared core
renderers.

The settings parity test also clicks the Rune controls, changes touch controls,
integer scaling, mute, volume and the A binding, then compares the result with
the shared desktop `Settings` value and canonical persisted document. The Web
test performs the same transitions through the real browser settings module and
verifies persistence through its storage adapter.

The browser side is `apps/rune/tests/web-parity.test.mjs`, which imports the
real `apps/web/www` modules and checks the same fixture round trip, default
transitions and settings defaults. Accepted, asserted differences: the browser
rejects the three JS prototype names (`__proto__`, `prototype`, `constructor`)
and stores JSON settings, while the native library accepts those names and
stores a `key=value` document; invalid settings input is normalised by the
browser and rejected by the native parser. These are format/validation
differences, not save-content differences.

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

## Browser bundle, interaction check and visual review

From the repository root:

```sh
scripts/bootstrap-web-tools.sh
scripts/rune-web.sh
scripts/rune-web-screenshots.sh
```

`rune-web.sh` builds an isolated WASM bundle under a unique
`target/rune-web-*` directory using pinned `wasm-bindgen 0.2.129`. The
screenshots script runs the desktop settings/save interaction check and the
Playwright capture set for desktop, mobile portrait and mobile landscape,
including empty/filled save states and scrolled sections. PNGs are written under
an ignored `output/rune-web-review-*` directory for visual inspection. This is
not the functional `apps/web` gate and does not establish physical-GPU or iOS
behavior.

Browser adapter unit tests use the pinned Node installation after bootstrap:

```sh
target/tools/node-pinned/bin/node --experimental-vm-modules --test \
  apps/rune/tests/browser-shell.test.mjs apps/rune/tests/browser-save.test.mjs
```

The combined reproducible browser build/adapter gate is
`scripts/rune-browser.sh`; CI provisions the pinned tools with
`scripts/bootstrap-web-tools.sh` before running it. Browser review captures also
run a desktop settings-persistence and exact synthetic-save
export/import-roundtrip test. Set `RUNE_SCREENSHOT_GREP` and
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
It does not sign for a physical device or verify audio, document picking,
screen-reader behavior or feature parity.

## Headless checks

Run from the repository root so all Cargo artifacts stay under the ignored root
`target/` directory:

```sh
cargo fmt --manifest-path apps/rune/Cargo.toml --check
CARGO_TARGET_DIR=target/rune cargo test --locked --manifest-path apps/rune/Cargo.toml
CARGO_TARGET_DIR=target/rune cargo clippy --locked --manifest-path apps/rune/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR=target/rune cargo check --locked --manifest-path apps/rune/Cargo.toml --all-targets --target wasm32-unknown-unknown
```

Native desktop target checks are compile-only and do not open windows or verify
GPU presentation. `scripts/rune-native-snapshot.sh` renders a native PNG using
the installed GPU backend and checks its dimensions; Linux CI provisions Mesa
software Vulkan for this, not a physical GPU. The iOS simulator smoke is a
separate macOS-only package and launch gate; it does not exercise a physical
device. Rune headless UI tests cover shared state, settings, saves, viewport
bounds and scrolling. The browser run includes a desktop interaction flow;
mobile runs are capture-only, and no browser screenshot proves screen-reader or
physical-GPU behavior.
