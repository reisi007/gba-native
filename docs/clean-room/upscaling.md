# Presentation and future AI upscaling

User intent: DLSS / AMD FSR, including AI-based upscaling, for higher output
resolutions. Agreed sequencing: core VM/AOT first, vendor integration later.

This repository remains experimental: no retail game is playable, no GPU vendor
backend is integrated, no REA Evidence exists, and it is not a formal
separate-team legal clean-room process.

## What exists now

Deterministic **integer** nearest-neighbor output with exact 3:2 content,
centered and black-letterboxed. Output dimensions are bounded to 1..=4096 per
axis and must be at least 240x160; content is always an integer multiple of the
native 240x160 image. PPM output needs no platform graphics dependency. This
establishes a tested baseline only: **it is neither FSR nor DLSS nor AI
upscaling**.

The optional, separate `apps/desktop` package adds a Winit/Pixels/WGPU window
for the same static synthetic snapshot, with pixel-perfect integer scaling and
a black letterbox. It is not integrated into the std-only core, accepts no
cartridge and is not a vendor backend. See `docs/gpu-frontend.md`.

| Backend name | CLI value | Status |
| --- | --- | --- |
| Nearest | `nearest` | Implemented CPU reference |
| FSR 1 | `fsr1` | Explicitly unsupported, error |
| Temporal FSR | `temporal-fsr` | Explicitly unsupported, error |
| ML FSR | `ml-fsr` | Explicitly unsupported, error |
| DLSS Super Resolution | `dlss` | Explicitly unsupported, error |

Selecting any unimplemented vendor backend returns `UnsupportedBackend`. It is
never silently replaced by nearest-neighbor. No GPU detection claim is made.
The default core build has no graphics device, shader, model, weights or vendor
SDK. The optional desktop package creates a Pixels/WGPU surface only to present
the synthetic bitmap; it integrates no vendor backend.

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
