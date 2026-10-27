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

## D10 — Separate runtime, AOT and application crates

Decided by the owner on 2026-10-25. The portable runtime (including typed IR
and HLE), AOT tooling and CLI are separate crates: `gba-native-core` in
`crates/core`, `gba-native-aot` in `crates/aot`, and the root `gba-native` CLI
plus compatibility facade. The GPU frontend remains isolated in `apps/desktop`,
excluded from the root workspace and depending directly on the core package.
Frontends must not need to depend on host-side AOT build/process tooling just to
execute the runtime. The root facade preserves the existing
`gba_native::{bus, cpu, cart, decode, demo, display, error, interpret, ir, bios,
render, semantics, sha256, aot}` import paths where practical. Core/default
validation remains std-only and usable offline without resolving the frontend's
external dependencies: the root workspace lockfile contains only the three
internal path packages, and the default third-party dependency set is empty.
Implementation, independent verification and CI evidence are tracked in issue
#23. This decision records the architecture, not a playability or compatibility
claim.

## D11 — Per-ROM applications, Web as a target, multiple save profiles

Reaffirmed by the owner on 2026-10-25. The end product is a ROM-specific build
containing translated code, runtime and the target frontend, not merely a
generic ROM-player window. Native deliverables and a WASM/browser build are
target variants of that same goal. Web is a first-class target, superseding
the earlier wording that it was only a possibility. Browser presentation must
address WebGPU and a WebGL2 compatibility path with explicit capability limits;
neither browser backend is claimed implemented yet.

Each ROM must support multiple persistent save profiles/slots across supported
environments, with a portable data format and platform-specific persistence.
Save management belongs in the application; mutable save data remains separate
from compiled code. Native files and browser storage are implementation targets,
not existing features. This does not authorize embedding or publishing the
original ROM or existing user saves: the current external, read-only,
SHA-256/length-bound ROM contract and data-preservation boundaries still apply.

Implementation note (2026-10-25): `apps/web` now provides an isolated synthetic
WASM presentation foundation with WebGPU and WebGL2 paths. It does not execute a
user ROM, implement browser AOT, input, or persistence, and does not satisfy the
per-ROM application or save-profile targets above.

## D12 — Mobile targets and configurable controls

Clarified by the owner on 2026-10-25. Android and iOS are additional native
target candidates. Browser and phone applications must include on-screen touch
controls, runtime-switchable control modes and user-remappable key bindings.
Bindings/settings must be persistent and must not require recompiling the
ROM-specific application. Platform input is mapped to logical GBA actions
outside the portable runtime; native key codes and browser event types must not
become CPU/AOT dependencies. Keep control settings separate from cartridge save
data and ROM bytes. These are target requirements, not a claim that mobile
packages, browser controls or remapping are already implemented or tested.

## D13 — Full framed guest SWI HLE

Decided by the owner on 2026-10-25. Reach the six existing documented GBA HLE
primitives from typed ARM/Thumb SWI instructions using ARM7TDMI SVC exception
entry and a non-collapsed handler frame: save the documented SPSR/R11/R12/R14
tuple on the SVC stack, switch the HLE handler to SYSTEM mode, and return with
the documented MOVS-PC/LR semantics. Do not fetch or reconstruct BIOS code.

 `Cpu::direct_entry()` remains the unchanged synthetic baseline. Guest runs and
 generated native harnesses explicitly initialize the documented GBA USER/SYS,
 IRQ and SVC stack tops. The HLE services run atomically inside one guest SWI
 step; handler/service internals have no instruction, scheduler, cycle, wait-state
 or frame timing. IRQs cannot occur inside the handler in this model.

GBATEK names the four stack-saved values but does not specify their ascending-
address slot order. This bounded HLE design stores `(SPSR, R11, R12, R14)` at
ascending word addresses beginning at `SVC_SP - 16`. That is an explicit
implementation policy, not a claim about Nintendo BIOS frame-layout
compatibility.

Unknown services and known service precondition failures are rejected before
framing mutation where possible. A SWI originating in SVC mode is explicitly
rejected because this bounded implementation does not preserve an already-live
SPSR_svc/LR_svc for nested entry. Unexpected bus failures after entry are
terminal and are not rolled back. Wait, sound, reset-except-memory and
decompression services remain explicit unsupported errors; this decision does
not establish BIOS boot, retail playability or close the broader playability
issues.

## D14 — Target-profile AOT builds are local headless artifacts

Decided 2026-10-27 as the bounded issue #10 slice. `gba-native aot build`
requires an explicit Cargo target triple, compiles the ROM-bound native module
in release mode with an own offline target directory, and writes only under the
ignored `<cwd>/output/` root. The original ROM remains external and read-only;
the artifact binds SHA-256+length and requires `--rom PATH` at runtime. Generated
metadata is ordered, hash/toolchain-based and free of absolute paths and
timestamps. Target support is whatever the installed toolchain can build; a
successful local build is not a cross-platform claim. The result is an
experimental headless artifact, not a playable game, GUI frontend, save-aware
application, final app or WASM build, and it does not satisfy the broader
per-ROM application targets in D11/D12.
