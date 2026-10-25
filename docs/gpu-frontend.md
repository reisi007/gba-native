# Isolated GPU frontend foundation

Status: design and implementation foundation for the synthetic fixture only.
This is not a playable emulator, not frame/cycle timing, not a retail-content
claim and not evidence of FSR, DLSS, AI reconstruction or vendor upscaling.

Decision context: `docs/decisions.md` D9 selects an isolated GPU frontend and a
std-only default core. D11 makes Web a first-class target with WebGPU/WebGL2
presentation. The separate `apps/web` package now presents the synthetic demo
only; it does not implement per-ROM browser execution. Existing core non-claims
still apply.

## Boundary

`apps/desktop/` is a separate package and a separate Cargo workspace:

- its own `apps/desktop/Cargo.toml` with an empty `[workspace]` table;
- its own `apps/desktop/Cargo.lock`;
- a relative-path dependency on `crates/core` (`gba-native-core`), aliased to
  the `gba_native` source namespace; it does not depend on the host-side AOT
  crate or the root compatibility facade;
- optional Winit/Pixels/WGPU dependencies that never enter the root default
  workspace or the root dependency graph;
- no changes to the core/AOT crates, the root CLI, the core `scripts/short.sh` /
  `scripts/full.sh` gates, existing CI or baselines. The optional
  `scripts/frontend.sh` gate, `.github/workflows/frontend.yml` and this document
  are separate and are never required by the std-only core.

The core remains `#![forbid(unsafe_code)]` and std-only. The frontend also uses
`#![forbid(unsafe_code)]` for its own code; external crates contain their own
internals, but the application adds no `unsafe` block. The crate is not a ROM,
save, BIOS or input surface: it accepts no cartridge path.

## Pinned dependencies

Direct frontend dependencies are exact-pinned and locked:

| Crate | Version | License | Role |
| --- | --- | --- | --- |
| `pixels` | 0.17.2 | MIT | GPU pixel buffer, surface presentation, custom-shader hook |
| `winit` | 0.30.13 | Apache-2.0 | Native window and `ApplicationHandler` lifecycle |
| `gba-native-core` | 0.1.0 | repository license undecided | std-only runtime core via relative path, aliased to `gba_native` |

`pixels` 0.17.2 resolves `wgpu` 29.0.4 (`MIT OR Apache-2.0`), not the unrelated
newest `wgpu` 30.0.1 release. It also resolves `raw-window-handle` 0.6.2
(`MIT OR Apache-2.0 OR Zlib`). The full transitive resolution is in
`apps/desktop/Cargo.lock`. These are dependency licenses, not a license choice
for this application or the repository.

No vendor SDK, model weights, emulator implementation source, ROM, BIOS, save
data or commercial fixture is used or copied.

## Behavior

`--synthetic` (also the default) executes the existing synthetic demo through
the core's `Cpu`, `Bus`, interpreter and display snapshot. The demo's existing
budget of 230,411 guest instructions is an instruction budget, not frame or
cycle timing. The resulting 240x160 RGB888 image is static after execution.

The frontend then:

1. validates image dimensions and buffer length and converts `0x00RRGGBB` to
   opaque RGBA8888;
2. creates an owned `Arc<Window>`, a Pixels `SurfaceTexture` from that owned
   window, and a real Pixels/WGPU surface (no OS software-pixel-window path);
3. copies the RGBA bytes once into the GPU pixel buffer;
4. presents with `ScalingMode::PixelPerfect`, black clear/letterbox and vsync;
5. reports adapter name/backend/device type/driver fields when Pixels exposes
   them, plus surface/pixel formats and present mode.

The initial window defaults to 3x (720x480). Resizes use physical pixels, so
high-DPI scaling is respected. The viewport is the largest integer scale that
fits both axes, centred with black bars; a letterboxed 800x600 surface therefore
shows 720x480 at `(40, 60)`. A surface smaller than the source clips rather than
inventing a fractional filter. Zero-size resize/minimise skips presentation and
restore/resume requests a fresh redraw.

Escape and the window close control exit cleanly. Ordinary mode uses
`ControlFlow::Wait` and no continuous frame loop: it intends two successful
render acknowledgements, and when acknowledgements are skipped it schedules at
most four bounded warm-up redraw requests. `--smoke N` requests redraws until N
`Pixels::render()` acknowledgements (bounded attempts), prints the actual
adapter/backend and count, then exits.

Initial window size is capped by the CLI at 4096 pixels per edge and 4096x2160
pixels; runtime OS-driven surfaces are additionally guarded at 8192 pixels per
edge and 16,777,216 pixels to avoid unbounded allocation. Oversized runtime
resizes are ignored with a diagnostic instead of allocating without bound.

## Errors and presentation accounting

Window creation, adapter/device/surface creation, surface resize and presentation
fail with a non-zero exit and a stage-specific message. Unsupported GPU surfaces
are not replaced by a software or nearest-neighbour fallback. Pixels 0.17.2
internally reconfigures and retries `Lost`/`Outdated`/suboptimal surfaces; its
`render()` returns `Ok` for `Occluded`/`Timeout` without necessarily presenting a
visible frame. Therefore the reported count means successful `Pixels::render()`
acknowledgements. It is not a guest frame count, not an FPS measure and the API
does not expose a separate visible-present counter.

## Clean extension points

`apps/desktop/src/presentation.rs` is GPU-free: RGB888/RGBA conversion and
viewport geometry have unit tests for known colours, alpha, wrong lengths, zero
dimensions, integer scale, centring, minimum size, clipping and minimised
surfaces.

A later custom-shader path can use `Pixels::render_with` and its
`CommandEncoder`/render target/`PixelsContext`; it is not implemented here.
The first-class browser target has a separate synthetic-only implementation in
`apps/web` (see `docs/web-frontend.md`). It does not yet provide:

- Pixels offers async construction for Web targets, but this crate uses the
  native blocking path;
- a browser needs canvas/surface creation, async init and user file input;
- core input (`KEYINPUT`) is still not integrated by this frontend and remains
  documented released-only in the core;
- native AOT harnesses cannot be assumed to run unchanged in a browser;
- per-ROM browser execution or browser AOT; browser scaling is not an FSR/DLSS
  label.

None of those gaps is claimed as working. The static browser demo does not add a
ROM mode or make the frontend playable.

## Gate and artifacts

The documented entry point is the optional frontend gate:

```text
./scripts/frontend.sh
```

It resolves the repository root from any working directory, requires Cargo and
Rust 1.98.0, and overrides `CARGO_TARGET_DIR` to the root-level ignored
`<repo>/target/frontend` before any frontend Cargo command. It then runs the
root hard 500-line counter (including `apps/desktop/src`), `cargo fmt --check`,
locked Clippy with warnings denied, the locked unit tests, a locked build, a
locked offline dependency-tree check (the core package must be present and the
AOT crate/root facade absent) and a headless `--help` invocation. It does not
require Ratchet, never opens a GPU window and is not called by
`scripts/short.sh` or `scripts/full.sh`.

All generated frontend build artifacts stay under `<repo>/target/frontend`.
`target/` is ignored only directly at the repository root. A nested
`apps/desktop/target/` is scanned like ordinary source, so generated `*.rs`
files there would legitimately count as maintained sources and can break the
500-line gate. The checked-in `apps/desktop/.gitignore` keeps those files out of
Git but is not a quality-scan exemption. Never run frontend Cargo commands from
inside `apps/desktop/` without explicitly setting a root-level ignored target
directory. The gate does not delete other target directories or output, edit
baselines or install tools.

Equivalent manual commands, run from the repository root (the gate is still
preferred so the target override cannot be forgotten):

```text
export CARGO_TARGET_DIR=target/frontend
cargo fmt --manifest-path apps/desktop/Cargo.toml --check
cargo clippy --locked --manifest-path apps/desktop/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path apps/desktop/Cargo.toml
cargo build --locked --manifest-path apps/desktop/Cargo.toml
cargo tree --locked --offline --manifest-path apps/desktop/Cargo.toml
```

The tree must show `gba-native-core` and must not show `gba-native-aot` or the
root `gba-native` facade.

Headless CLI checks (with the export above):

```text
cargo run --locked --manifest-path apps/desktop/Cargo.toml -- --help
cargo run --locked --manifest-path apps/desktop/Cargo.toml -- --rom game.gba
cargo run --locked --manifest-path apps/desktop/Cargo.toml -- --size 999999x999
```

The first exits 0; invalid modes exit 2 before creating a window. No commercial
data is read.

Live GPU smoke, when a desktop GUI is available:

```text
export CARGO_TARGET_DIR=target/frontend
cargo run --locked --manifest-path apps/desktop/Cargo.toml -- \
  --synthetic --size 720x480 --smoke 5
```

A local observation on 2026-10-25 used `Apple M5 Pro`, backend `Metal`,
`IntegratedGpu`, `Bgra8UnormSrgb`, `AutoVsync`, scale 3 and
`presented=5 requested=5` at 720x480. That is one host observation, not a
cross-platform GPU claim. No live GPU smoke has been run on Windows or Linux,
and no physical-GPU cross-platform claim is made. The separate synthetic browser
WASM/WebGPU/WebGL2 build and functional E2E have local evidence; see
`docs/web-frontend.md`. A headless host without a window server cannot provide
native live-GPU evidence; `scripts/frontend.sh` deliberately does not attempt
one.

## Continuous integration

`.github/workflows/frontend.yml` runs the same `scripts/frontend.sh` under bash
on `ubuntu-latest`, `windows-latest` and `macos-latest` with Rust 1.98.0,
rustfmt and Clippy. The job has `contents: read`, no secrets, no artifact upload
and no cache action, and it neither simulates nor claims a visible GPU frame:
CI has no window or display. The workflow installs no emulator libraries,
GPU SDK packages or system packages; the job only compiles, lints, unit-tests
and checks `--help`. The first frontend dependency fetch is expected there; the
default root tests never fetch or build the frontend package.

## Non-claims

- Synthetic GPU presentation only; not playable retail, not a game loop.
- The demo instruction budget is not frame timing and the static image is not
  an emulated frame stream.
- Nearest/pixel-perfect scaling is not AI upscaling, FSR, DLSS or a vendor
  backend.
- No input, audio, save, BIOS, cartridge, retail-derived source, per-ROM browser
  execution or browser game frontend is implemented. The separate synthetic
  WebGPU/WebGL2 foundation is documented in `docs/web-frontend.md`.
- No GPU vendor SDK integration, no GPU timing claim and no performance claim.
