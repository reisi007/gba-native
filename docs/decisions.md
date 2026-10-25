# Decisions

Durable project decisions that outlive a single task. Operational open work
lives in GitHub Issues, not here.

## D1 — Hybrid GBA-to-native prototype first

Decided 2026-10-24. The first milestone is a small hybrid AOT compiler plus
reference runtime, not a playable emulator, game packager, GPU integration or
retail-compatibility claim. Typed IR, a bounded ARM/Thumb subset, synthetic
fixtures and an independently verified native differential come before broader
hardware coverage. The exact implemented behavior stays in
`docs/clean-room/gba-contract.md`.

## D2 — Public repository before the first CI push

Decided 2026-10-24, replacing the earlier private-only assumption. After the
content and history audit, the repository may be made public before the first
CI run pushes a commit. The owner makes the visibility change; automation does
not. The original commercial ROM, BIOS bytes, save data and generated
retail-derived artifacts are never published, copied into version control or
embedded in generated source.

## D3 — GitHub Issues are the authoritative open-work record

Decided 2026-10-24. Open work, owners and completion state live in
<https://github.com/reisi007/gba-native/issues>. Local files do not duplicate the
issue list or keep a checked-off history. An explicit user commission is still
required before work starts; an issue is a record, not an authorization.
Issues are closed only after independent verification, the repository gates and
CI pass, with the issue reference in the change. `AGENTS.todo.md` is only a
pointer, and Git history is the completion record.

## D4 — Hard quality floor: 500 physical lines and Ratchet

Decided 2026-10-24. Every maintained Rust source or test file has at most 500
physical lines, including blank and comment lines; a trailing newline adds no
phantom line. The limit is checked directly by `scripts/short.sh` and CI as well
as by a zero-debt Ratchet ceiling, so editing or raising the baseline cannot
dilute it.

Generated build/runtime artifacts in the root-level directories `target/`,
`output/` and `.git` are not maintained sources and are outside the scan. That
exemption is positional: only entries directly under the scan root are skipped.
A nested directory with one of those names, such as `src/output/` or
`tests/target/`, is part of the source tree and is scanned, because a name
collision must never hide a maintained file. Every other `*.rs` file belongs to
the population, including root files, `build.rs`, test modules and new untracked
files. An empty selection is an error.

`scripts/short.sh` is the fast local gate; `scripts/full.sh` adds the debug and
release suites, the demo and the real native proof. CI executes the same two
scripts. Ratchet is pinned to release v0.5.0 of `all-the-rest/ratchet-cli`
(peeled commit `99c25b1e18331a1473d43091d37e23f23bc1c30d`; the annotated tag
object is `1feb7b5fae653272a922ed9f0479660ecde22ed2`); a missing or mismatched
tool fails the gate instead of skipping it.

## D5 — Evidence language stays explicit

The native differential establishes compiler consistency between the emitter
and the shared reference interpreter; it is not an ISA oracle and not hardware
conformance. Independent golden vectors cover only the listed ISA subset.
Synthetic-fixture results do not establish retail-game compatibility, BIOS
boot, timing, GPU upscaling or successful REA Evidence.

## D6 — Generated artifacts use fresh owned workspaces

Decided 2026-10-24 after the output-path review. CLI-generated files are
restricted to `<cwd>/output/`; absolute paths must resolve below that root.
The location is validated before any directory is created, symbolic links and
non-directory components below the root are refused, and fresh files use
`create_new`. `aot proof` always allocates a new workspace: the default creates a
unique `output/aot-proof-*` directory, and an explicit `--out` must not already
exist. `NativeHarness::build` accepts only a non-existing crate directory and
creates its contents with `create_dir`/`create_new`. Earlier output is never
erased or reused. This is an ownership and consistency contract, not a claim of
race-freedom against an actively hostile concurrent process.

## D7 — PSR transfers and banking stay explicit

Decided 2026-10-25. ARM7TDMI MRS/MSR support is deliberately narrow and
error-first: documented field bytes are applied, reserved CPSR/SPSR bits are
preserved, USER-mode control writes are ignored, USER/SYSTEM SPSR access is an
explicit error, and invalid mode or T-bit changes are rejected before mutation.
MSR-driven mode changes swap the documented register banks, but exception
entry/return, SWI, vector fetch and PSR restore remain unimplemented. An MSR
with no field bit set is rejected at decode and in the public write path instead
of being accepted as a successful no-op. Bounded AOT discovery invalidates cached
r8-r14 constants across CPSR control writes.
The native report includes a full CPU-state digest so banked state, not only the
visible register snapshot, participates in the differential.

## D8 — Documented HLE for the playability milestone

Decided by the owner on 2026-10-25. Continue toward Emerald playability using
independently implemented BIOS-service HLE derived from public interface and
hardware documentation, not a bundled or disassembled Nintendo BIOS. This is a
chosen implementation strategy, not evidence that HLE services already exist.
Missing services must fail explicitly; direct entry is not BIOS reset execution.
Title/new-game interaction, a controllable overworld and save/load validation
are the gameplay acceptance milestones; an exhausted instruction budget is not
playability. ROMs and existing saves remain local, unmodified test inputs and
are never published.

## D9 — GPU presentation, with a possible WebGPU target

Decided by the owner on 2026-10-25. Interactive presentation must use a GPU
backend, rather than making a software-only window the frontend foundation.
Use an isolated frontend crate with Winit and Pixels/WGPU as the desktop
direction; the default portable runtime/AOT core remains std-only. Investigate
a WebGPU browser target where feasible, without claiming it is already built
or that native AOT harnesses run unchanged in a browser. Later browser scaling
needs its own evaluated implementation, not an FSR/DLSS label. Nearest/pixel
presentation alone is still neither AI reconstruction nor vendor upscaling.
