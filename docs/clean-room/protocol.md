# Independent implementation protocol

Decision date: 2026-10-24. Target: Game Boy Advance, Rust, portable core, hybrid
AOT-to-native research prototype.

## Boundary

Implement from public hardware/interface documentation and independently written
test cases. Do not consult emulator implementations, game decompilations, BIOS
disassemblies or leaked development kits. The mGBA-hosted **GBATEK document** is
a hardware reference, not permission to use mGBA's implementation. Resource
indexes contain links to emulator sources; those sources are outside this
boundary.

The current work is a documentation-driven independent implementation, **not a
certified two-team/legal clean-room process**. The same agent reads public
specifications and implements them. This document is not a legal opinion.

### Narrow BIOS analysis exception: RegisterRamReset (01h)

By owner authorization dated 2026-10-28, a separate analyst may inspect only the
public GBA BIOS disassembly for SWI `RegisterRamReset` (`01h`). The analyst's
behavior-only report may contain externally observable behavioral requirements,
hardware-register effects, confidence, source links and uncertainties only. It
must contain no pseudocode, disassembly, BIOS internal symbols or addresses,
instruction sequence, or implementation-specific control flow. Source links may
remain in the analyst-to-orchestrator report, but the orchestrator must remove
or replace any link that exposes BIOS disassembly before handing the reviewed
report to an implementer. The implementer receives only that sanitized report
and existing public hardware documentation.
Unknown effects remain unsupported and must fail closed. No BIOS bytes or ROM
bytes may be saved or committed. This is an informal process, not a legal
clean-room certification. Do not add the disassembly source to `sources.md`
until analysis has actually been performed.

If subsequent REA work examines a reference implementation, separate the analyst
and implementer contexts. The analyst may deliver behavioral requirements,
input/output observations and uncertainty, but not pseudocode, disassembly,
original identifiers or implementation-specific structure. Review that document
before giving it to an implementer. A stricter process requires separate people
and legal review.

## Architecture decision

The first milestone is a **hybrid GBA-to-native compiler plus runtime**, not a
standalone emulator and not a game packager.

1. Decode a small, independently implemented ARMv4T subset into typed IR.
2. Execute that IR in a reference interpreter.
3. Discover reachable straight-line blocks in immutable ROM from an explicit
   `(PC, ARM/Thumb)` entry, with bounded limits.
4. Emit native Rust statements for those blocks and compile a developer harness,
   including an experimental release build for an explicitly requested Cargo
   target triple.
5. Dispatch unknown or dynamic targets to the reference interpreter with a fresh
   fetch/decode. RAM code is never AOT-translated.
6. Compare native and reference registers, flags, bus state, image and exact
   instruction budget for a wholly synthetic fixture.

No JIT or executable writable memory is required. iOS JIT constraints therefore
do not block the current portable-core design. Standalone per-game packaging,
Android/iOS packaging, GPU vendor upscaling and retail-game compatibility are
later work and are not claimed. Isolated optional `apps/desktop` and `apps/web`
frontends present the same synthetic snapshot through native Winit/Pixels/WGPU
and browser WebGPU/WebGL2 respectively. They remain outside the std-only core,
accept no cartridge and are not playable or vendor-upscaled frontends
(`docs/gpu-frontend.md`, `docs/web-frontend.md`).

## Artifacts

- `sources.md`: permitted sources, retrieval identity and coverage.
- `gba-contract.md`: actual implemented behavior, AOT rules and explicit limits.
- `upscaling.md`: presentation baseline and future vendor options; not GBA
  hardware behavior.
- `gpu-frontend.md`: optional synthetic Winit/Pixels/WGPU window boundary,
  dependency pins, gate/artifact rules and observed limits.
- `web-frontend.md`: isolated synthetic WASM/browser presentation boundary,
  pinned-tool bootstrap, gates/artifacts and observed limits.
- `rea-status.md`: actual tool observations, including blockers. No Evidence ID
  exists for this investigation.
- `verification.md`: executable acceptance criteria and test boundaries.
- `decisions.md`: durable project decisions, including public visibility and
  the GitHub Issues workflow.
- `AGENTS.todo.md`: pointer to GitHub Issues only; no copied backlog. Open work
  is authoritative at <https://github.com/reisi007/gba-native/issues>.
- `README.md` / `AGENTS.md`: user-facing status and repository commands.
- `ratchet.config.jsonc` / `ratchet.baseline.json`: machine-managed quality
  rules and baseline; the baseline is never hand-edited.

ROMs and BIOS files remain user-supplied, unmodified and outside version control.
Do not redistribute game content, embedded logos or BIOS bytes. The synthetic
ROMs are generated from our own instruction words, with a zeroed logo region and
a documented direct-entry fixture entry branch. They are **not BIOS-bootable
retail cartridges**.

## Evidence vocabulary

- **Documented**: stated by an identified public hardware/vendor document.
- **Observed**: measured locally; distinguish ordinary tools from REA Evidence.
- **Design**: chosen implementation behavior, not a claim about hardware.
- **Fixture fact**: true only for the synthetic ROMs in this repository.
- **Unknown / unsupported**: not established or not implemented. Never hide these
  as success, fabricated input data, or silently selected fallback backends.

## Non-claims

- No BIOS boot, no retail-game playability, no save/audio or game-level input
  interaction. The core accepts host-injected KEYINPUT state, but has no frame
  scheduler, sampling guarantee or timing claim.
- No cycle/frame timing, DMA, IRQ, full tile/sprite PPU or playable window
  frontend. A bounded static Mode 0 BG0 text-background snapshot subset exists
  alongside bitmap modes; it is not a scanline/frame renderer, and other BG
  layers and OBJ/sprite rendering remain unsupported.
- No GPU vendor SDK, FSR, DLSS, ML model or custom upscaling shader/integration.
- No successful REA analysis or native Evidence record.
- No general static recovery of arbitrary ROM code. Discovery is bounded and
  stops at unknown/indirect control flow.
- The native proof is a developer test harness, not a shipped game build. The
  `aot build` output is likewise a headless developer artifact: a native
  executable, or for `wasm32-unknown-unknown` a ROM-bound WASM execution module
  plus JS glue. Neither is a playable standalone game, final application, GUI
  frontend, browser application, save-aware build or playable WASM frontend; the
  WASM module has no HTML/JS UI, renderer, input or persistence. Building one
  target triple with the installed toolchain does not establish cross-platform
  support.
- The optional `apps/desktop` Winit/Pixels window and `apps/web` WebGPU/WebGL2
  page present only the static synthetic bitmap. Neither is a playable/retail
  frontend, per-ROM application or vendor upscaler; neither establishes
  frame/cycle timing.
