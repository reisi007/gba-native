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

The functional and screenshot runners share `scripts/web-gpu-launch-args.sh`.
Its common Chromium preset follows the official WebGPU/SwiftShader flag-specific
configuration: `--enable-unsafe-webgpu`, `--use-webgpu-adapter=swiftshader`,
`--enable-dawn-features=allow_unsafe_apis`,
`--disable-dawn-features=use_dxc`, `--enable-webgpu-developer-features`,
`--use-gpu-in-tests` and `--enable-accelerated-2d-canvas`. On Linux only, the
helper adds `--enable-features=Vulkan --use-angle=swiftshader
--use-vulkan=swiftshader` and no `--disable-vulkan-surface` token (promoted
candidate configuration under test, see below). macOS and Windows do not
receive those Linux-specific switches. A non-empty `WEB_E2E_LAUNCH_ARGS`
remains a complete user override (it replaces the preset); unset or empty
values use the platform-aware default. For Linux CI A/B diagnostics, the
effective Variant A flags must contain no `--disable-vulkan-surface` token;
Variant B then appends exactly one, and an override that already carries one or
more copies fails the preflight before any browser or standalone probe. A
custom override without the token follows the same A/B derivation. Playwright
still reports forced WebGPU failures and device loss; these switches do not
introduce fallback or relaxed assertions.

Linux candidate configuration under test (issue #24). Promoted Variant A selects
ANGLE's SwiftShader backend (`--use-angle=swiftshader`) instead of
`--use-angle=vulkan` and omits `--disable-vulkan-surface`; it is the
configuration that, as the then-current Variant B, passed all 19 functional
tests in Linux CI run 40547520962 under the former gating policy. The promotion
rests on that matched run. The promoted configuration was later re-executed as
new Variant A together with the gated standalone probes in
[run 40557708749](https://github.com/reisi007/gba-native/actions/runs/40557708749):
Variant A passed 19/19 and both probes exited 0, with the canvas smoke confirming
the expected magenta. That run still used the former both-gated policy and does
not establish a root cause; the new A/B gating policy waits for CI validation.
It is a candidate, not an established root cause; it makes no physical-GPU,
driver, vendor or retail-compatibility claim and does not claim that a switch
enables GPU rendering. The new Variant B appends exactly one
`--disable-vulkan-surface` token to Variant A and reproduces the earlier
surface-on Variant A flag set. It continues to execute the full functional
suite in every Linux A/B diagnostics run, and its result, Playwright log,
`test-results` and exit status are retained, but it is a non-gating diagnostic:
Variant A and the two standalone probes gate the Linux browser job.

Recorded facts from run 40501875768 (software runner, Playwright Chromium
156.0.8078.4):

- the standalone offscreen probe obtained adapter `google`/`swiftshader`
  (`SwiftShader Device (Subzero)`, `backend=vulkan`), created a device, submitted
  a compute pass and read back the expected bytes, so an adapter and device were
  available in that run;
- the standalone canvas probes lost the device at 32–36 ms
  (`deviceLoss.phase=work`) with `OperationError: A valid external Instance
  reference no longer exists.`; the GPU process logged `Could not find
  SharedImageBackingFactory ... usage: ...WebgpuSwapChainTexture ...
  debug_label: WebGPUSwapBufferProvider` and `SharedImageStub: Unable to create
  shared image`;
- the real app path differed in timing and shape: it reached `ready` at ~112 ms
  with `presented=1`, applied a `lost` status at ~745–749 ms with `presented=2`,
  and the post-work snapshot at ~7.1 s still reported `lost` (`WebGPU device lost
  (Destroyed): Device was destroyed.`) with the canvas hidden and Restart
  offered;
- with the earlier explicit `--use-angle=vulkan` preset in run 40501875768, the
  GPU process also logged repeated initialization failures (`Extension not
  supported: VK_KHR_surface`/`VK_KHR_xcb_surface`, `Exiting GPU process due to
  errors during initialization`).

Those observations do not establish cause: they do not show whether the display
configuration, the ANGLE backend, a shared-image/backing condition or another
GPU-process condition produces the loss. The candidate changes only the Linux
launch preset and adds a display wrapper for one gate; every assertion, pixel
golden and forced-WebGPU failure path is unchanged, so a wrong candidate keeps
the gate red instead of masking it.

Linux CI run 40547520962 used the previous A/B pairing and is the evidence for
this promotion. The then-current Variant A (surface token present) exited 1 with
17 of 19 tests passing; the two failures were WebGPU canvas golden tests in
which the app reported `ready` on SwiftShader/Vulkan with two presentations, yet
every sampled source pixel was black. The then-current Variant B — the same flags
with only `--disable-vulkan-surface` absent — exited 0 with 19 of 19 tests
passing. In the same run the standalone canvas smoke still compared white
against the expected magenta and failed, while the offscreen probe passed. The
four non-gating one-flag omission controls (eight app/canvas probe rows) had
only `omit-disable-vulkan-surface` exit 0 for both entries. Its canvas and app
results recorded no device loss or SharedImageBackingFactory errors; other
omission rows did record such observations. That matched run supports the
promoted configuration but is not
root-cause proof, and that matched run alone does not show that the standalone
canvas smoke passes under the promoted configuration. Those run labels are the
old pairing: old A failed, old B passed. A later run, 40557708749, re-ran new
Variant A with both standalone probes: Variant A passed 19/19, the canvas smoke
exited 0 with the expected magenta and the offscreen smoke exited 0. That run
used the former policy that gated both full suites and does not establish a root
cause.

Display wrapper scope: only Linux CI functional E2E. `scripts/web-e2e.sh`
validates its mode first, then re-executes itself under the runner image's
`xvfb-run` wrapper only when all of `ci` mode, `CI=true`, Linux and an unset
`DISPLAY` hold, passing its mode and arguments through unchanged. The wrapper
comes from the runner image's distribution `xvfb` package;
`scripts/web-browsers-install.sh` separately provisions Chromium and its distro
dependencies through Playwright `--with-deps`. Local Linux runs (`local` mode or
no `CI=true`), macOS/Windows and runs with `DISPLAY` set keep their previous
behaviour and are not wrapped. If the wrapper is missing in that Linux CI case
the gate fails with a diagnostic before any browser probe is launched; the
browser checks are never skipped, timed out as passed or replaced by a software
frame. The screenshot runner (`scripts/web-screenshots.sh`) is not wrapped: it
invokes Playwright directly and is not a CI gate, so its capture behaviour on a
host without a display is unchanged. `scripts/web.sh --build-only` exits before
`scripts/web-e2e.sh` and therefore never reaches the wrapper.

`summary.txt` records `xvfb-run-wrapper=1` when that re-execution happened. The
value records wrapper use only; it does not show that Chromium consumed a virtual
display or that display-dependent behaviour changed. The Linux result of this
candidate was not reproducible on the macOS development host and still requires
CI confirmation.

Linux CI's explicitly gated `WEB_LINUX_AB_DIAGNOSTICS=1` path also runs two
standalone probes once with the exact current Variant A arguments (the promoted
preset including the diagnostics logging and no `--disable-vulkan-surface`
token) and pinned Chromium revision. Both use `page.route()` to serve a
CSP-restricted document from the existing loopback HTTP origin and verify
`isSecureContext`. The canvas probe configures the preferred WebGPU format,
submits two clear passes and checks a Playwright screenshot's clear-color pixel;
the offscreen probe verifies a compute-buffer readback without creating a
canvas. Each records console/page errors, adapter details, a bounded post-work
wait and `device.lost` separately from its deliberate teardown. These
diagnostics are not GPU-hardware evidence and do not establish root cause;
either probe failing fails the CI browser gate after both full A/B functional
suites have run. Variant A gates that result; Variant B still executes and
records its status as a non-gating diagnostic. An earlier recorded canvas smoke
used the earlier surface-on flags and failed its white-versus-magenta pixel
comparison. Run 40557708749 later re-ran both standalone probes under the
promoted flags: the canvas smoke exited 0 with the expected magenta confirmed
and the offscreen smoke exited 0. Both probes remain gated.
Artifacts are retained below `output/web-linux-ab-*/smoke-{canvas,offscreen}`.
Animation-frame waits and Playwright navigation, evaluation, screenshot and
cleanup calls have deadlines; a 150-second harness ceiling persists a failed
result and exits the owned smoke process if cleanup stalls. Playwright's Browser
API does not expose the launched Chromium child exit code, so the fallback closes
the harness/control pipe and does not attempt a broad process kill.

The same explicitly gated `WEB_LINUX_AB_DIAGNOSTICS=1` path also runs a
non-gating flag matrix. Its `app` harness mode navigates the real served bundle
on its public route `/?autostart=1&backend=webgpu` without `page.route()`,
injected scripts or a fallback; the standalone `canvas`/`offscreen` modes keep
using the routed, CSP-restricted document from the existing loopback origin. The
app probe waits through the app's initial `empty` and subsequent `loading`
states for a terminal `ready`, `error` or `lost` outcome (30-second bound), and
requires a matching terminal state from the read-only app diagnostic API before
reporting a completed observation. It captures an initial snapshot and a
seven-second post-work snapshot of `window.__gbaNative.status()`, its bounded
history, and canvas
geometry and visibility, and records lost/error state as an observation instead
of suppressing it. Console, page-error and request-failure diagnostics stay in
`result.json`; only harness integrity fails closed.

The matrix baseline is the app with the exact documented reference preset flags,
recorded under `matrix-baseline-reference-app`. The reference preset is exactly
Variant B: the promoted Variant A plus one `--disable-vulkan-surface` token,
i.e. the earlier surface-on Variant A. Four omission controls then remove
exactly one token each — `--enable-features=Vulkan`,
`--use-angle=swiftshader`, `--use-vulkan=swiftshader` or
`--disable-vulkan-surface` — while every other token, including the
diagnostics-only `--enable-logging=stderr --log-level=0`, is retained, and each
resulting flag set probes both the real app route and the standalone canvas
document. Because the reference preset contains the surface token, all four
rows stay real one-token omissions; the `--disable-vulkan-surface` row
reproduces the promoted Variant A byte-for-byte, while the other three rows
retain the surface token. These one-flag omissions are the only removal
controls; the matrix adds no `--use-angle=opengl` variant because the recorded
control produced
`Requested GL implementation (gl=none,angle=none) not found in allowed
implementations`, so it was not a usable backend comparison. Every combination
owns a directory below
`output/web-linux-ab-*/matrix-<control>-<mode>/` with `launch-args.txt`,
`omitted-flag.txt`, `run-info.txt` (label, mode, omitted flag,
`preset=reference-surface-on`, entry, origin, channel, revision),
`exit-status.txt` and `process-status.json`; probed combinations add
`playwright.log`, and completed ones `result.json`. For a missing or duplicated
omission token, the directory keeps the exact reference preset flags it was
derived from in `launch-args.txt` plus an `invalid-input.txt` reason; the
combination is recorded as an explicit `invalid-input` result (exit 126) and is
not probed. A probe that exits without `result.json` is recorded as
`harness-failure`. All remaining combinations are still attempted and no
process is killed broadly. `outcome` (`completed`, `probe-reported-failure`,
`harness-failure`, `invalid-input`) and the recorded exit status describe
harness completion, not GPU health; the app probe's
`deviceLossObserved`/`errorStateObserved` carry the observed app outcome
separately.

The Variant A preflight is separate from those omission controls. The effective
Variant A flags must contain zero `--disable-vulkan-surface` tokens; Variant B
is then formed by appending exactly one. An override that already carries one
or more copies fails closed before any browser or standalone probe with exit 1
and records `matrix-preflight-variant-a-{canvas,app}` directories
(`launch-args.txt` with the effective Variant A flags after diagnostics logging
is appended, `surface-token-count.txt`,
`invalid-input.txt`, run info with `control=variant-a-preflight`, exit 126 and
process status) plus `matrix-preflight=invalid-input` in `summary.txt`. An
override without the token follows the same A/B derivation. Missing
feature/ANGLE/Vulkan control tokens remain non-gating `invalid-input` matrix
rows.

One additional candidate, `features-vulkan-from-angle`, belongs to the same
non-gating matrix but is not an omission. Starting from the reference preset
(everything Variant B carries, including `--disable-vulkan-surface` and
`--enable-logging=stderr --log-level=0`), it replaces exactly the single argv
token `--enable-features=Vulkan` with
`--enable-features=Vulkan,VulkanFromANGLE,DefaultANGLEVulkan` and retains every
other token byte-for-byte in the same order. It does not add `--use-gl=angle`,
does not change `--use-angle`, does not use OpenGL and changes no app/runtime
code. The candidate was inspired by a public WebGPU-on-Linux report but is only
a candidate control: it proves neither causality nor a root cause and makes no
general Linux compatibility claim. Both the standalone canvas document and the
real app route are attempted even if one of them fails. Its directory keeps
`launch-args.txt`, `changed-arg.txt` (the changed and replacement token),
`run-info.txt` with `preset=reference-surface-on` and
`control=feature-substitution` (no `omitted_flag`), `exit-status.txt` and
`process-status.json` with `changedFlag`/`replacementFlag` instead of
`omittedFlag`; probes add `playwright.log` and completed probes `result.json`.
A missing or duplicated `--enable-features=Vulkan` token is recorded as
`invalid-input` (exit 126) for both entries, while every other matrix row is
still run.

Matrix statuses are listed in `summary.txt` after `matrix-non-gating=true`, in
`matrix.txt`, and as a separately labelled non-gating table in the GitHub step
summary; the additive `features-vulkan-from-angle` rows are included there.
They never change the job status: only the two standalone probes and the full
Variant A functional suite gate. Variant B continues to execute the full
19-test suite; its exit status, Playwright log, `test-results` and `summary.txt`
row stay recorded, but a Variant B failure is a non-gating diagnostic and does
not turn the job red. Neither a B failure nor a B success is hidden: the step
summary labels B as diagnostic/non-gating and never reports it as gated. The
A/B pair keeps its one-token contrast — variant A is the promoted candidate
preset, variant B appends exactly the single `--disable-vulkan-surface` token
to A, adds no OpenGL/ANGLE control and changes no other token. The
`omit-disable-vulkan-surface` matrix row therefore reproduces variant A's flag
set, while the matrix baseline and the other three omission rows use the
reference preset (variant B). Because the prior surface-on configuration failed
17/19 as old A in run 40547520962, the new policy records the same result as a
B diagnostic instead of gating on it; it does not claim that B passes. The
reference preset's standalone canvas/app probes are not the full 19-test
suites.

In the retained artifacts of Linux WebGPU CI run 40516175997
(`output/web-linux-ab-40516175997-review/.../web-linux-ab-20261027T135653Z-9085-0`),
the non-gating `omit-disable-vulkan-surface` rows for both the standalone canvas
and the app entry exited 0, and the canvas screenshot's centre pixel matched the
expected magenta (`pixelVerified=true`). That is probe evidence for those flag
sets only. In that run the standalone canvas smoke and both full-suite processes
exited 1; its Variant B still used `--use-angle=vulkan`, not the current
one-token comparison. Linux CI run 40547520962 then compared the surface-on
configuration (old Variant A: exit 1, 17/19, two black-pixel WebGPU canvas
goldens) with the token-omitted configuration (old Variant B: exit 0, 19/19)
and recorded the standalone canvas smoke failing white against the expected
magenta while the offscreen probe passed; only the
`omit-disable-vulkan-surface` matrix row exited 0 for both entries. Those run
labels are the old pairing. Run 40557708749 later re-ran new Variant A and both
standalone probes under the promoted flags: Variant A passed 19/19, the canvas
smoke exited 0 with the expected magenta and the offscreen smoke exited 0; that
run used the former both-gated policy. These facts do not establish a cause for
the earlier device loss and do not establish that removing the token helps
beyond those probes. A different feature-substitution
candidate stayed ready in its app probe but failed its canvas pixel probe, so it
was not promoted. Variant A and both standalone probes still have to pass on CI;
Variant B must still execute and be recorded, but it does not gate. Their
directories, `launch-args.txt`, Playwright output and exit statuses stay
recorded, and `summary.txt` still reports `A=<status>`, `B=<status>`,
`A-gated=true` and `B-non-gating=true`. Match or mismatch of a control is a
diagnostic signal, not root-cause proof, and this document makes no Linux matrix
result claim.

Rendering uses Pixels `ScalingMode::PixelPerfect`: largest integer scale fitting
the physical backing store, centred with black letterbox, `image-rendering:
pixelated`, no CSS blur. The surface is deferred until a non-zero canvas size;
the design cap is 8192 per edge and 16,777,216 pixels (labelled as an application
cap, not a queried adapter maximum). The core bitmap is declared sRGB for the
WebGL2 path and kept raw non-sRGB with an `rgba8unorm` WebGPU
pixel/render/surface path. The RGBA surface matched Linux Chromium's preferred
format in the recorded CI investigation; preferred formats vary by
browser/device. The WebGPU path does not configure an invalid sRGB canvas format
or apply a gamma conversion; WebGL2 retains its sRGB decode/encode round-trip.

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
| headless Chromium, `--enable-unsafe-webgpu --use-webgpu-adapter=swiftshader` (macOS arm64) | `auto`/`webgpu` | `actual=webgpu`, `backend=BrowserWebGpu`, surface/render `Rgba8Unorm`, adapter name not exposed, device type `Other`; browser preference is `bgra8unorm` here, so its extra-copy warning remains (the UI does not call the software adapter physical) |
| headless Chromium, same flags | `webgl2` | `backend=Gl`, adapter `ANGLE (Apple, ANGLE Metal Renderer: Apple M5 Pro)`, type `IntegratedGpu`, surface `Rgba8UnormSrgb` |
| headful system Chrome 156.0.8078.47 (before the RGBA surface change) | `webgpu` | ready, `backend=BrowserWebGpu`, adapter details redacted, sampled pixels exact; not re-run for the changed format |
| headless Chromium without WebGPU flags | `webgpu` | explicit “requested explicitly … no WebGPU adapter” error; no fallback |
| Playwright page with `Navigator.prototype.gpu` removed | `auto` then `webgpu` | auto ready on WebGL2 with `Auto compatibility path: yes`; forced WebGPU typed error |
| Linux CI run 40236292333, before the Vulkan launch switches (only the two original SwiftShader flags) | `auto`/`webgpu` | WebGPU initialized with `Rgba8Unorm` render/surface and acknowledged two presents, then reported `WebGPU device lost (Destroyed): Device was destroyed.`; four tests failed and 15 passed. The WebGL2 path passed. |
| Linux CI run 40501875768, native-Vulkan ANGLE preset, software adapter, standalone canvas probe | `webgpu` | SwiftShader adapter `google`/`swiftshader`/`Vulkan` obtained, device created, and the matched offscreen compute probe succeeded; the canvas probe alone lost the device at 32–36 ms with `OperationError: A valid external Instance reference no longer exists.` after `Could not find SharedImageBackingFactory ... WebgpuSwapChainTexture` and repeated `VK_KHR_surface`/`VK_KHR_xcb_surface` init errors. |
| Linux CI run 40501875768, same preset, real app route | `webgpu` | Reached `ready` at ~112 ms with `presented=1`, applied `lost` at ~745–749 ms with `presented=2` and `WebGPU device lost (Destroyed): Device was destroyed.`; the post-work snapshot at ~7.1 s still reported `lost` with the canvas hidden. |
| Linux WebGPU CI run 40516175997, non-gating `omit-disable-vulkan-surface` probes (then-Variant A with that one token removed) | `webgpu` | Both the standalone canvas and the real app probe exited 0; the canvas screenshot's centre pixel matched the expected magenta (`pixelVerified=true`). In that run the canvas smoke and both full-suite processes exited 1; its Variant B used `--use-angle=vulkan`, not the current one-token comparison. This is probe evidence, not full-suite success or root-cause proof. |
| Linux WebGPU CI run 40547520962, then-current Variant A (surface token present; now new B), full suite under the former both-gated policy | `webgpu` | Exit 1 with 17/19 passing; two WebGPU canvas golden tests failed although the app reported ready on SwiftShader/Vulkan with two presentations and all sampled source pixels were black. Under the new policy this surface-on configuration runs as the non-gating Variant B diagnostic. |
| Linux WebGPU CI run 40547520962, then-current Variant B (surface token omitted; now new A), full suite under the former both-gated policy | `webgpu` | Exit 0 with 19/19 passing. This is the configuration promoted to Variant A; it was later re-run as new Variant A in run 40557708749 (19/19), where both standalone probes also exited 0. |
| Linux WebGPU CI run 40547520962, standalone canvas and offscreen probes (then-current surface-on A) | `webgpu` | The offscreen probe passed; the canvas smoke still compared white against the expected magenta and failed. Both probes remain gated. |
| Linux WebGPU CI run 40547520962, four non-gating one-flag omission controls (eight app/canvas probe rows) | `webgpu` | Only `omit-disable-vulkan-surface` exited 0 for both entries. Its canvas/app results recorded no device loss or SharedImageBackingFactory errors; other omission rows did record such observations. This is not root-cause proof. |
| Linux WebGPU CI run 40557708749, promoted Variant A, full suite, former both-gated policy | `webgpu` | Exit 0 with 19/19 passing. The overall workflow still failed because Variant B was gated in that run. |
| Linux WebGPU CI run 40557708749, new Variant B (A plus exactly one `--disable-vulkan-surface`), full suite, former both-gated policy | `webgpu` | Exit 1 with 17/19 passing; the same two WebGPU pixel-golden failures as run 40547520962. The app reported ready on SwiftShader/Vulkan with two presentations and all sampled source pixels were black. No SharedImageBackingFactory failure was observed. The functional logs contain a deliberate synthetic device-loss message from a diagnostics-regression fixture; it is test data, not evidence of runtime device loss. Under the new policy this result is recorded as a non-gating diagnostic; the new policy has not yet been validated by CI, and this run does not prove a root cause. |
| Linux WebGPU CI run 40557708749, standalone canvas and offscreen probes | `webgpu` | Canvas smoke exited 0 with the expected magenta confirmed; offscreen exited 0. Both probes remain gated. No SharedImageBackingFactory failure was observed in the probe logs. |

Adapter names/device types are browser-redacted in some WebGPU runs; they are
reported as “(not exposed by this browser)”. The Linux gate exercises the
promoted candidate preset (ANGLE's SwiftShader backend for Linux without the
surface token, with the Linux CI functional E2E re-executed under `xvfb-run`
when no display is set) as Variant A, which gates the job together with both
standalone probes, and the matched Variant B that appends exactly one
`--disable-vulkan-surface` token. Variant B continues to execute the full suite
and is recorded as a non-gating diagnostic. Linux WebGPU CI run 40516175997
failed under the then-current policy: both full suites did not all pass, while
the artifacts additionally recorded the non-gating one-token omission probes
described above. Run 40547520962 used the old pairing — old A (surface token
present) failed 17/19 and old B (token omitted) passed 19/19 — and its
standalone canvas smoke failed the magenta comparison. The promoted
configuration was later re-run as new Variant A in run 40557708749
([CI run](https://github.com/reisi007/gba-native/actions/runs/40557708749))
under the former both-gated policy: the canvas smoke exited 0 with the expected
magenta confirmed, the offscreen smoke exited 0, Variant A exited 0 with 19/19
passing and Variant B exited 1 with 17/19 (the same two WebGPU pixel-golden
failures; the app reported ready on SwiftShader/Vulkan with two presentations
and all sampled source pixels black). No SharedImageBackingFactory failure was
observed in that run's smoke/A/B logs. The functional-suite logs contain a
deliberate synthetic device-loss message from a diagnostics-regression fixture;
it is test data, not evidence of runtime device loss. Its overall workflow
failed because Variant B was gated. That run does not prove a root cause. The
new policy — Variant A and both standalone probes gate; Variant B
executes and is recorded as a non-gating diagnostic — has not yet been
validated by CI. Those results do not show a passing Linux suite under the
promoted flags and do not identify a cause. The recorded Linux runs established
WebGPU API initialization, the RGBA format and a working offscreen device, not a
stable canvas presentation or a successful Linux suite. This is
software-backend configuration/evidence, not a physical-GPU claim. A browser
without the required backend fails the forced-backend test; the suite does not
skip it.

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

The Linux CI functional E2E additionally re-executes itself under the runner
image's `xvfb-run` wrapper when `DISPLAY` is unset, but only in `ci` mode with
`CI=true` (candidate configuration under test, see above). Local `local`-mode runs,
runs without `CI=true`, screenshot capture and `scripts/web.sh --build-only` are
not wrapped.

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

The fresh local normal and `--ci` gates on 2026-10-25 passed the core wasm32
check, WASM release build, pinned bundle build, 16 Rust library tests, one
loopback-server test, five lifecycle tests and all 19 functional browser E2E
tests using the expanded common SwiftShader preset. Both ran on macOS arm64;
`--ci` correctly did not receive Linux Vulkan switches. Chromium reported
`BrowserWebGpu` with adapter `SwiftShader Device (LLVM 10.0.0)` and
`Rgba8Unorm`; real PNG samples matched all five RGB goldens exactly, with source
SHA-256 and the 230,411-instruction budget unchanged. macOS prefers
`bgra8unorm`, so its preferred-format extra-copy warning remains. These runs are
not Linux CI evidence and do not establish whether the device-loss failure from
Linux run 40236292333 is fixed. The new logs are in
`output/web-linux-flags-macos.tGT2wY/web-normal.log` and
`output/web-linux-flags-macos.tGT2wY/web-ci.log`; prior format logs remain in
`output/web-linux-format-xyJBkfLLOd/`. Screenshot review remains pending.

## Remaining gaps (explicitly not implemented)

- no per-ROM compiled/translated application, no browser ROM input/upload inside
  this package, and no browser AOT emission here: `apps/web` is not the output of
  the AOT CLI. A separate `gba-native aot build --target wasm32-unknown-unknown`
  branch now produces a ROM-bound WASM *execution module* plus JS glue, but it
  ships no HTML/JS UI, renderer, input or persistence and is not wired into this
  page or the browser gate;
- no gameplay loop, audio, frame/cycle timing, save data, save profiles or
  portable persistence;
- no GBA input mapping, touch controls, remapping or Android/iOS packages (#25);
- no WebGL1/Canvas2D fallback, full automatic context restoration, vendor
  upscaling, FSR/DLSS/AI or performance claims;
- no retail-game compatibility or BIOS boot claim.
