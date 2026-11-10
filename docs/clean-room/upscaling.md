# Presentation and future AI upscaling

User intent: DLSS / AMD FSR, including AI-based upscaling, for higher output
resolutions. Agreed sequencing: core VM/AOT first, vendor integration later.

This repository remains experimental: no retail game is playable, no GPU vendor
backend is integrated, no REA Evidence exists, and it is not a formal
separate-team legal clean-room process.

## What exists now

Deterministic integer nearest-neighbor output and a selectable CPU
`sharp-bilinear` reference filter. Nearest uses integer scaling; sharp-bilinear
uses fractional scaling, with centered black bars and aspect-preserving
content. Output dimensions are bounded to 1..=4096 per axis; sharp-bilinear
requires at least 240x160, while nearest requires a valid integer scale. Both
produce PPM without a platform graphics dependency. The
sharp-bilinear CLI mode is not integrated into Rune or a GPU pass; neither mode
is FSR, DLSS, or AI upscaling.

The canonical `apps/rune` UI presents the same static synthetic snapshot on
Rune-supported desktop, browser/WASM and iOS targets. Android remains on its
existing adapter. Neither UI is integrated into the std-only core, accepts a
cartridge or implements a vendor backend. See `apps/rune/README.md`.

| Backend name | CLI value | Status |
| --- | --- | --- |
| Nearest | `nearest` | Implemented CPU reference |
| Sharp bilinear | `sharp-bilinear` | Implemented CPU reference; no GPU/UI integration |
| FSR 1 | `fsr1` | Explicitly unsupported, error |
| Temporal FSR | `temporal-fsr` | Explicitly unsupported, error |
| ML FSR | `ml-fsr` | Explicitly unsupported, error |
| DLSS Super Resolution | `dlss` | Explicitly unsupported, error |

Selecting any unimplemented vendor backend returns `UnsupportedBackend`. It is
never silently replaced by nearest-neighbor. No GPU detection claim is made.
The default core build has no graphics device, shader, model, weights or vendor
SDK. Rune presents the synthetic demo, or a one-game snapshot with compiled
translated blocks when built against a fixed ROM; Android remains
synthetic-only. Neither integrates a vendor backend.

## Intended separation

`cartridge → ARM/Thumb IR + bus → snapshot image → presentation backend`

Presentation must never alter emulated CPU state, hardware resolution, input
timing or save data. Higher **output** resolution does not mean the game itself
renders new geometry or contains higher-resolution artwork. The GBA does not
supply a 3D camera/depth buffer or stable object-motion vectors.

## AI/temporal feasibility gate

Before implementing a vendor backend:

1. Choose an actual target OS, GPU, SDK version and legal distribution terms.
2. Validate native API interop from Rust (DX12/Vulkan as supported); do not assume
   a portable graphics abstraction can directly hand resources to every SDK.
3. Specify how trustworthy motion, depth and jitter can be derived for GBA 2D
   layers. OAM displacement is insufficient for tile scrolling, animation,
   transparency, bitmap effects and occlusion. Constant zero vectors/fake depth
   do not establish valid integration or image quality.
4. Measure whether 240×160 is a usable input; large scaling factors may need a
   documented staged pipeline and cannot recover nonexistent original detail.
5. Reset history on ROM load, reset, scene cuts, resize, pause and backend switch.
6. Test HUD/text stability, transparency, scrolling and scene cuts against the
   native reference. Compare stills **and motion**, and measure GPU cost/latency.
7. Enable the backend only after device capability and SDK execution are verified.

S4/S5 establish the spatial/temporal distinction. Exact SDK/OS/device support
must be checked against the version selected at integration time, rather than
hardcoded from a generic feature name. Apple Silicon must not advertise DLSS.

If temporal reconstruction is unsuitable, a spatial neural image-upscaling model
is a separate, explicitly named alternative, not something marketed as DLSS/FSR.
Models, weights, inference runtimes and quality targets require another decision.

No SDK, shader, model or weights are bundled in this milestone. No vendor backend
has been integrated or tested.
