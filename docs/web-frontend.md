# Browser GPU foundation (synthetic demo)

Status: implementation and locally observed evidence for the static synthetic
demo only. Not a playable emulator, not a retail-game compatibility claim, not
frame/cycle timing, not FSR/DLSS/AI upscaling, and not the per-ROM WebAssembly
application described by the end goal. This document does not declare the
foundation visually approved; the owner/reviewer judges the screenshot set.

Decision context: `docs/decisions.md` D10 (separate runtime/AOT/application
crates), D11 (Web as a first-class target with WebGPU plus an explicit WebGL2
compatibility path) and D12 (Android/iOS and configurable controls as later
targets). Those decisions are the target architecture; this page records what is
actually implemented.

## Boundary

`apps/web/` is a separate package and a separate Cargo workspace:

- its own `apps/web/Cargo.toml` (`[workspace]`, package `gba-native-web`),
  `apps/web/Cargo.lock`, `apps/web/package.json` and
  `apps/web/package-lock.json`;
- a direct relative-path dependency on `crates/core` (`gba-native-core`, aliased
  to the `gba_native` source namespace); it does not depend on the host AOT
  crate or the root compatibility facade;
- no changes to core/AOT behavior, the default root dependency graph, the root
  lockfile, the desktop frontend, or Ratchet baselines. Root tests and project
  documentation include integration contracts/status for this package;
- the root manifest excludes both `apps/desktop` and `apps/web`. The root
  `Cargo.lock` still contains exactly the three internal packages.

The browser crate is `#![forbid(unsafe_code)]`. It accepts no cartridge path,
save, upload, BIOS, audio or game input. The core stays std-only and has no
browser host-filesystem convenience API.

## Pinned dependencies

Rust direct dependencies (exact pins in `apps/web/Cargo.toml`, resolved in
`apps/web/Cargo.lock`):

| Crate | Version | License | Role |
| --- | --- | --- | --- |
| `gba-native-core` | 0.1.0 path | repository license undecided | std-only runtime/core demo |
| `pixels` | 0.17.2 | MIT | pixel buffer and nearest-neighbour surface renderer |
| `winit` | 0.30.13 | Apache-2.0 | canvas window lifecycle (`spawn_app`, `with_canvas`) |
| `wasm-bindgen` | 0.2.129 | MIT OR Apache-2.0 | WASM/JS boundary |
| `wasm-bindgen-futures` | 0.4.79 | MIT OR Apache-2.0 | `spawn_local` for async GPU init |
| `js-sys` / `web-sys` | 0.3.106 | MIT OR Apache-2.0 | canvas, events, console |
| `wgpu` | 29.0.4 (transitive) | MIT OR Apache-2.0 | WebGPU and WebGL2 backends |

Build/test tooling (repository-owned under `target/`, never shipped):

| Tool | Version | License | Install |
| --- | --- | --- | --- |
| Node.js | 24.22.1 | MIT-style Node license | official platform archive, literal SHA-256 pin plus installed-binary provenance manifest |
| wasm-bindgen CLI | 0.2.129 | MIT OR Apache-2.0 | `cargo install --root target/tools` |
| daisyUI | 5.7.50 | MIT | npm lockfile |
| `@tailwindcss/cli` | 4.3.3 | MIT | npm lockfile |
| `@playwright/test` | 1.64.1 | Apache-2.0 | npm lockfile; Chromium revision 1248 |

No CDN, remote font, runtime third-party request, vendor SDK, model weights,
emulator implementation source, ROM, BIOS or save data is used. These are
dependency licenses, not a license choice for the repository or the app.

## Build and test entry points

```text
./scripts/bootstrap-web-tools.sh    # pinned Node/wasm-bindgen/npm tooling in target/
./scripts/web.sh                    # gate: counter, core WASM check, tests, bundle, E2E
./scripts/web.sh --build-only       # same build checks without browser E2E (partial)
./scripts/web.sh --ci               # same gate with the CI/software-GPU browser mode
./scripts/web-e2e.sh [local|ci]     # functional browser E2E only (bundle must exist)
./scripts/web-browsers-install.sh   # provision Playwright Chromium (CI and fresh hosts)
./scripts/web-screenshots.sh        # UI-review capture only; never a CI gate
```

All Cargo commands run with `CARGO_TARGET_DIR=<repo>/target/web`. Tool caches are
`target/tools` and `target/web-tools`; no `apps/web/target` or
`apps/web/node_modules` is created. The static bundle is
`output/web-dist` with an ownership marker: the script refuses to replace a
directory it did not create. Screenshot output is a unique
`output/web-review-<timestamp>-<random>` directory.

Node 24.22.1 archive digests are pinned in `scripts/bootstrap-web-tools.sh` from
the official release checksum file
(`https://nodejs.org/dist/v24.22.1/SHASUMS256.txt`); the bootstrap does not fetch
that checksum file at install time:

| Platform | Archive | SHA-256 |
| --- | --- | --- |
| macOS arm64 | `node-v24.22.1-darwin-arm64.tar.gz` | `89c682593d76cd44233fa53b1563264e7de9cbb189cad169135c5b4fadfbc0a7` |
| macOS x64 | `node-v24.22.1-darwin-x64.tar.gz` | `d132557e7ee2b85af08b0c8c34338e65c6430f2238d01f727728ef42dc83b035` |
| Linux arm64 | `node-v24.22.1-linux-arm64.tar.gz` | `33c37be234ba8bee041961047b19160f4d7d0f164e8e246f78bdd10feff148b3` |
| Linux x64 | `node-v24.22.1-linux-x64.tar.gz` | `eb6728e77c576a35162c122c45ccadf224d89ff00abc8dae46cc2897c6002d7c` |

The archive cache must match its literal pin before extraction. An installed
binary is trusted only when its target-local manifest records the exact version,
platform, pinned archive digest and binary digest, and the current binary still
matches that digest. A version-only pre-existing executable is not trusted; a
new verified alias is installed at `target/tools/node-pinned/` without touching
the legacy `target/tools/node` path. An install without its manifest is treated
as absent and reinstalled from the verified archive. Unsupported platforms and
mismatched cached archives fail closed.

`scripts/web.sh` checks:

1. the root hard 500-physical-line counter (includes `apps/web`);
2. the root lockfile still has exactly `gba-native`, `gba-native-core`,
   `gba-native-aot`;
3. `cargo check -p gba-native-core --target wasm32-unknown-unknown --offline
   --locked`;
4. `apps/web` rustfmt, host unit tests, host Clippy `--all-targets -D warnings`
   and wasm32 Clippy;
5. a standalone wasm32 release build, wasm-bindgen 0.2.129 (`--target web`),
   locally built Tailwind 4 + DaisyUI 5 CSS;
6. the wasm-target dependency tree: direct `gba-native-core` only, no
   `gba-native-aot` and no root `gba-native` facade, with the three exact
   frontend pins;
7. expected WASM exports, no remote bundle resource, no nested tool artifacts;
8. functional browser E2E through the same script the CI job uses.

Missing tools, a missing wasm32 target, a missing Playwright Chromium revision or
a missing GPU backend fail with a diagnostic; they are never skipped silently.
The gate does not edit baselines, install global tools, delete foreign output or
upload artifacts.

## Source execution and parity evidence

The browser executes the existing synthetic fixture through the core's `Cpu`,
`Bus`, interpreter and display snapshot exactly once:

- image `240x160`, packed RGB888 → opaque RGBA8888;
- SHA-256 of the three-byte RGB stream:
  `b02968d620ee66d7175c29bbde12bc9ff075290f3e8086b26e9d4829b9ccb154`;
- instruction budget `230411`;
- host unit tests assert both values and known pixel samples (`(120,80)`
  `#c6de94`, `(60,40)` `#e76b4a`, `(180,120)` `#a54ae7`, `(239,159)` `#ff7b29`);
- the E2E suite asserts the same SHA/budget from the real WASM page and samples
  the real presented canvas from a Playwright PNG (tolerance ≤ 6 per channel for
  colour management).

The budget is an instruction count, not frames, FPS, cycle timing or a visible
frame counter. The image is static after execution. `presented` counts
`Pixels::render()` acknowledgements; Pixels can return `Ok` for
occluded/timeout frames without a guaranteed visible present.

## Backend contract

The page exposes `auto`, `webgpu` and `webgl2` and reports both the requested and
the actual adapter backend (`BrowserWebGpu` / `Gl`), adapter name/type/driver
when the browser exposes them, vendor/device IDs, surface and pixel formats,
present mode, adapter `maxTextureDimension2D`/`maxBufferSize`, the letterbox
viewport and the acknowledgement count.

- `auto` probes WebGPU once with
  `wgpu::util::is_browser_webgpu_supported()`; if unavailable it selects the
  WebGL2 compatibility path and reports `Auto compatibility path: yes`.
- A forced backend never silently degrades. Unsupported WebGPU/WebGL2, an
  unknown query value, adapter failure, a backend mismatch or a presentation
  failure produces a typed error, a visible overlay and a Restart control.
- There is no WebGL1 or Canvas2D fallback and no CPU-success pretence.
- The canvas context type is fixed once created, so a retry/reload creates a
  fresh canvas/page. If an auto WebGPU initialization fails after the probe, the
  page performs at most one recorded reload to WebGL2 (reason shown after the
  reload). Explicit restarts are bounded to three per 30 seconds. No retry
  storm, no automatic loop.
- WebGL context loss calls `preventDefault`, records the reason, halts
  rendering, hides the dead canvas and offers Restart. A WebGPU device-lost
  callback writes a typed reason that the page polls at 500 ms and turns into
  the same halted state; late ready/viewport statuses cannot re-show the dead
  canvas. Full automatic context restoration is intentionally not implemented.
- WebGL2 has no compute/storage-buffer or shader-fragment-interlock path for a
  future upscaler; nothing beyond nearest presentation is claimed.

Rendering uses Pixels `ScalingMode::PixelPerfect`: largest integer scale fitting
the physical backing store, centred with black letterbox, `image-rendering:
pixelated`, no CSS blur. The surface is deferred until a non-zero canvas size;
the design cap is 8192 per edge and 16,777,216 pixels (labelled as an application
cap, not a queried adapter maximum). The core bitmap is declared sRGB for the
WebGL2 path and kept raw non-sRGB with a `bgra8unorm` WebGPU canvas, avoiding
the invalid sRGB canvas-configure format and double gamma conversion.

Canvas physical geometry is computed from the live CSS content rectangle times
`devicePixelRatio`, rounded to the nearest backing pixel. Winit's resize event
and the current canvas backing readback are accepted only when each agrees with
that independent expectation within one pixel; a CSS-sized
`devicePixelContentBoxSize` report is corrected instead of being mistaken for
physical pixels or multiplied twice. The correction applies on initial window
creation, resize and scale-factor changes before surface-cap validation. The
functional suite checks actual `canvas.width`/`height` at DPR 1, 2 and 1.25,
verifies the nearest-neighbour source samples, and exercises an initial over-cap
surface followed by a fresh-canvas restart.

## Page, states and accessibility

There is one route (`/`) with explicit states:

- **empty** — nothing executed; placeholder text, Start enabled;
- **loading** — spinner and progress text, never a blank screen;
- **ready** — real canvas visible with backend/adapter facts;
- **error** — typed failure alert with Restart;
- **lost** — context/device-loss halt with Restart.

The shell uses DaisyUI 5 components on Tailwind 4 semantic colours (card, button,
select, badge, alert, loading). It is responsive for desktop, mobile
portrait and mobile landscape, keyboard operable (native select, buttons,
skip link, `Enter`/`Space` start), with labelled controls and an `aria-live`
status. The visible non-claim is: “Experimental synthetic GPU demo; no playable
ROM, saves or game input yet.” There are no fake GBA buttons, save widgets or
touch controls: those belong to #25/#8 and are not implemented.

Security: the development/test static server accepts only literal loopback
`SocketAddr` IPs (`127.0.0.1` or `::1`), not wildcard, public IP or hostname
bindings. It serves only the bundle root with canonical-path checks and rejects
path traversal and symlink escapes. The application uses localhost/HTTPS only
for a real deployment, narrow CSP
(`script-src 'self' 'wasm-unsafe-eval'`), same-origin assets, no inline script,
no ROM picker/upload, no save access, no network APIs in the app. The loopback
server also sends correct WASM MIME and `nosniff`/CSP headers.

## Observed browser evidence

Local runs on 2026-10-25 with this repository, Playwright Chromium
156.0.8078.4 (revision 1248), macOS arm64:

| Run | Forced/auto backend | Observed result |
| --- | --- | --- |
| headless Chromium, `--enable-unsafe-webgpu --use-webgpu-adapter=swiftshader` | `auto`/`webgpu` | `actual=webgpu`, `backend=BrowserWebGpu`, surface `Bgra8Unorm`, adapter name not exposed, device type `Other` (the launch flag provisions a software WebGPU adapter; the UI does not call it physical) |
| headless Chromium, same flags | `webgl2` | `backend=Gl`, adapter `ANGLE (Apple, ANGLE Metal Renderer: Apple M5 Pro)`, type `IntegratedGpu`, surface `Rgba8UnormSrgb` |
| headful system Chrome 156.0.8078.47 | `webgpu` | ready, `backend=BrowserWebGpu`, adapter details redacted, sampled pixels exact |
| headless Chromium without WebGPU flags | `webgpu` | explicit “requested explicitly … no WebGPU adapter” error; no fallback |
| Playwright page with `Navigator.prototype.gpu` removed | `auto` then `webgpu` | auto ready on WebGL2 with `Auto compatibility path: yes`; forced WebGPU typed error |

Adapter names/device types are browser-redacted in some WebGPU runs; they are
reported as “(not exposed by this browser)”. CI uses Chromium with the
SwiftShader WebGPU flag — API/software evidence, not a physical-GPU claim. A
browser without the required backend fails the forced-backend test; the suite
does not skip it.

Playwright's `deviceScaleFactor` emulation has been observed reporting
`devicePixelContentBoxSize` in CSS pixels. The application independently checks
CSS geometry × DPR and now sets the physical backing size; the functional suite
asserts backing dimensions against that expected size (±1 device pixel for
fractional rounding) and retains the real-canvas pixel oracles. This emulation is
not a claim about physical-pixel sampling on a real high-DPI device.

## CI

`.github/workflows/web.yml` runs on pushes and pull requests with
`contents: read`, a bounded timeout, the verified `actions/checkout` v7.0.1
commit, Rust 1.98.0 with `wasm32-unknown-unknown`, and the same
`scripts/web.sh --ci` entry point used locally. Browser provisioning is explicit:
`scripts/web-browsers-install.sh` bootstraps the owned toolchain and installs the
Playwright Chromium revision plus its Linux dependencies. There are no secrets,
paid resources, artifact/ROM uploads or changes to the existing CI/frontend
workflows. Screenshots are not a CI gate.

## UI-review capture set

```text
./scripts/web-screenshots.sh
```

This builds the current bundle, serves it on loopback and runs
`playwright.screenshots.config.ts` with a separate route/state/viewport manifest
(`apps/web/tests/screenshots/ui-review.config.ts`) and generic
`ui-screenshots.spec.ts`. States: empty, filled-WebGPU, filled-WebGL2,
error-invalid-backend (real validation), lost-context (real WEBGL_lose_context),
fallback-webgl2-ready (capture-only warning fixture with real WebGL2 presentation),
storage-restart-blocked (real storage denial), and error-oversize-initial (real
surface-cap path).
Viewports: desktop 1280x900, mobile portrait 390x844 @2x, mobile landscape
844x390 @2x. Short pages get every section plus a full-page PNG; the JSON per
combination records section indexes and whether an overview exists. The set only
captures pixels and never asserts gameplay/GPU success, so a failed capture means
the harness or page is broken.

Latest capture inventory provided for this integration: 24 state/viewport
combinations and 97 PNGs. The captures remain review artifacts, not repository
assets; the owner/reviewer reads the images. This document does not claim visual
approval.

The integrated local gate run on 2026-10-25 passed the core wasm32 check, the
WASM release build, the pinned bundle build, 15 Rust library tests, one loopback
server test, five lifecycle tests and all 15 functional browser E2E tests. It ran
on macOS arm64; forced WebGPU and WebGL2 both passed there. The `--ci` mode and
workflow are configured for Linux/SwiftShader, but this local run is not Linux CI
evidence. The full local logs are in
`output/integration-web-unique/web.log`; root test/gate logs are in the same
directory. Screenshot review remains pending.

## Remaining gaps (explicitly not implemented)

- no per-ROM compiled/translated application, no browser ROM input/upload, no
  browser AOT emission (the current AOT CLI writes native `fn main` harnesses and
  is not WASM-compatible);
- no gameplay loop, audio, frame/cycle timing, save data, save profiles or
  portable persistence;
- no GBA input mapping, touch controls, remapping or Android/iOS packages (#25);
- no WebGL1/Canvas2D fallback, full automatic context restoration, vendor
  upscaling, FSR/DLSS/AI or performance claims;
- no retail-game compatibility or BIOS boot claim.
