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
4. Emit native Rust statements for those blocks and compile a developer harness.
5. Dispatch unknown or dynamic targets to the reference interpreter with a fresh
   fetch/decode. RAM code is never AOT-translated.
6. Compare native and reference registers, flags, bus state, image and exact
   instruction budget for a wholly synthetic fixture.

No JIT or executable writable memory is required. iOS JIT constraints therefore
do not block the current portable-core design. Standalone per-game packaging,
Android/iOS packaging, GPU vendor upscaling and retail-game compatibility are
later work and are not claimed.

## Artifacts

- `sources.md`: permitted sources, retrieval identity and coverage.
- `gba-contract.md`: actual implemented behavior, AOT rules and explicit limits.
- `upscaling.md`: presentation baseline and future vendor options; not GBA
  hardware behavior.
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

- No BIOS boot, no retail-game playability, no save/audio/input interaction.
- No cycle timing, frame timing, DMA, IRQ, tile/sprite PPU or window frontend.
- No GPU vendor SDK, FSR, DLSS, ML model, shader or upscaling integration.
- No successful REA analysis or native Evidence record.
- No general static recovery of arbitrary ROM code. Discovery is bounded and
  stops at unknown/indirect control flow.
- The native proof is a developer test harness, not a shipped game build.
