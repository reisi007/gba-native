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
application, final app or complete browser app, and it does not satisfy the
broader per-ROM application targets in D11/D12. The separate ROM-bound WASM
module branch is recorded in D15.

## D15 — ROM-bound WASM execution module as a bounded `aot build` branch

Decided 2026-10-27 as a further bounded issue #10 slice, after the owner asked
for additional target artifacts and explicitly included WASM.
`gba-native aot build <rom.gba> --target wasm32-unknown-unknown` emits the
target-neutral program section plus a wasm-bindgen `run_rom(Uint8Array)` driver,
builds a fresh generated cdylib offline in release mode against the direct
`gba-native-core` path dependency and the exact `wasm-bindgen` 0.2.129 pin, and
post-processes the module with the repository-owned wasm-bindgen CLI
(`target/tools/bin/wasm-bindgen --target web --no-typescript`). The module
embeds only the expected SHA-256 and ROM length; the caller-supplied
`Uint8Array` is checked before any cartridge, CPU or bus is constructed. Core
errors propagate as JavaScript errors and the call returns a compact state
budget/fallback report. No filesystem, process or stdout boundary exists in the
module.

The generated workspace contains `generated.rs`, the fresh crate with its own
target directory, `pkg/` JS glue plus `_bg.wasm`, ordered `artifact.meta` and
`build.meta`, and a generated `README.md`. The WASM metadata records
`artifact_kind=wasm-module`, `module_sha256` and `js_glue_sha256` (no executable
claim), the target, ROM identity, entry/state, budget and discovery facts, the
program-source hash, actual Cargo/rustc facts and the wasm-bindgen CLI version,
with no absolute paths, timestamps or ROM bytes.

The root workspace, its lockfile, `apps/web` and the frontend packages are
unchanged; the generated crate owns its offline dependency resolution. Other
`wasm32*`/`wasm64*` triples are rejected explicitly, and a missing or mismatched
pinned tool fails with the `scripts/bootstrap-web-tools.sh` instruction instead
of a global fallback or auto-install. This is a browser-callable execution
module, not a browser application or GPU renderer: there is no HTML/JS UI, game
input, save profile, persistence, full frontend or playability claim, and the
broader per-ROM application targets in D11/D12 remain open. This slice does not
close issue #10.

## D16 — Narrow RegisterRamReset disassembly-analysis exception

Decided by the owner on 2026-10-28. This narrowly supersedes only D8's
no-disassembly clause for separate-analyst inspection of public GBA BIOS SWI
`RegisterRamReset` (`01h`), solely through the reviewed behavior-only process in
`docs/clean-room/protocol.md`. It authorizes analysis, not BIOS-byte handling,
code reconstruction, or implementation based on unseen disassembly. D8's other
constraints remain unchanged: unsupported or unknown effects must fail
explicitly, and no fake success is permitted. This remains an informal process,
not a legal clean-room certification. This records permission only; it does not
claim the analysis has been performed or the service implemented.

## D17 — Explicit synthetic zero-fill policy for BIOS data reads

Decided by the owner on 2026-10-28 as the bounded issue #6 follow-up. A supplied
ROM attempt had stopped on a BIOS data read under the direct-entry policy. The
owner authorized a deterministic synthetic fallback so bounded progress
experiments can continue without pretending that BIOS bytes exist.

`Bus::new(cart)` remains fail-closed exactly as before: BIOS data reads return
`BiosAccess`. The new, clearly named opt-in
`Bus::with_synthetic_bios_zero_fill(cart)` (or
`Bus::with_bios_data_policy(cart, BiosDataPolicy::SyntheticZeroFill)`) makes
`read8`, `read16` and `read32` at `0x00000000..=0x00003FFF` return zero after
the existing halfword/word alignment checks. Nothing else changes: BIOS
`fetch16`/`fetch32` still return `NonExecutable`, BIOS writes still return
`BiosAccess`, and every other address keeps its previous policy. The policy is
host configuration and is excluded from `Bus::state_digest`.

The CLI exposes this only through `gba-native run
--synthetic-bios-zero-fill`; the default remains strict. Successful output and
the optional snapshot report print the selected `bios_data_policy`, and the
opt-in carries an explicit note that the zero fill is an experimental,
non-hardware-faithful assumption rather than authentic BIOS data. Tests cover
the strict default, all three read widths, rejected fetches/writes and the CLI
opt-in with synthetic fixtures only.

This is not an authentic BIOS opcode source, not hardware behavior and not a
boot, timing or compatibility claim. It does not relax the prohibition on BIOS
image handling, disassembly outside D16's narrow process, or ROM modification.

## D18 — Unaligned block/stack bases align only the transfer address

Decided by the owner on 2026-10-29 to continue bounded local hardware-test-ROM
experiments within the clean-room rules. GBATEK states that the low address bits
of an ARM/Thumb block or stack base are ignored. It does not establish whether
writeback derives from the original or the aligned base. This prototype now
resolves that gap as a stated **Design** policy rather than rejecting the
instruction:

- The addressing mode is evaluated against `base & !3`, so every transferred
  word lies in the aligned block or stack range.
- Writeback, and the PUSH/POP SP update, always use the original, unmodified
  base register: `base ± transfer_size`.
- Base-in-list special cases are unchanged; a stored base value is still the
  original register value.

`gba_native::memory::aligned_block_base` is the single shared helper for the
reference interpreter and the generated native code. Focused ARM and Thumb
tests cover aligned and unaligned transfers, writeback and base-in-list, and an
AOT parity fixture asserts that generated execution matches the reference.

This is not a hardware-conformance claim. It does not establish cycle timing,
commercial-ROM compatibility or playability.

## D19 — ARM `S=1, Rd=PC` restores an existing SPSR only

Decided by the owner on 2026-10-29 to continue bounded local hardware-test-ROM
experiments within the clean-room rules. GBATEK
`#arm-opcodes-data-processing-alu` states that an ARM data-processing
instruction with `S=1` and `Rd=R15` restores the CPSR from the current mode's
SPSR and takes the computed result as the PC. This is documented CPU state
transfer, so the form is implemented instead of rejected:

- `Cpu::return_from_exception` validates that the current mode has an SPSR and
  that its saved mode is one of the seven ARM7TDMI modes before mutating state;
  USER/SYSTEM and invalid saved modes remain explicit errors.
- The restored CPSR replaces the ordinary `S=1` N/Z/C/V result, and the
  restored T bit determines PC alignment. Mode and T may change; MSR remains
  unable to change T.
- The reference interpreter and generated native code use the same helper.
  Discovery treats the operation as a PC write and invalidates cached banked
  registers across the CPSR control change.

This performs no exception entry and fetches no BIOS code. It is not a
hardware-conformance, timing, commercial-ROM or playability claim.

## D20 — Implement the documented Thumb empty block-list quirk

Decided by the owner on 2026-10-29 to continue bounded local hardware-test-ROM
experiments within the clean-room rules. GBATEK
`#thumb-opcodes-memory-multiple-loadstore-pushpop-and-ldmstm` documents the
ARMv4 Thumb empty `LDMIA`/`STMIA` list as an R15 transfer with `Rb` advanced by
`0x40`. It is implemented explicitly rather than treated as a NOP or rejected:

- Empty `STMIA` transfers R15; its stored value is the address of the STM
  instruction plus 6, matching the visible PC of a following Thumb instruction.
  This is checked against `MOV Rd,PC` and by black-box ROM test 229. Empty
  `LDMIA` loads PC, clears bit 0 and remains in Thumb state, following this
  prototype's existing Thumb PC-load rule.
- The transfer address uses the accepted block-alignment policy; base writeback
  is the original base plus `0x40`.
- Empty `PUSH`/`POP` remain unsupported. Interpreter and emitted native code
  implement the same behavior, with control-flow metadata marking empty LDMIA
  as a dynamic PC write.

This is a bounded CPU behavior implementation, not a hardware-conformance,
timing, commercial-ROM or playability claim.

## D21 — Deterministic prototype behavior for multiply `Rd=Rm` aliases

Decided by the owner on 2026-10-29 to continue the local `thumb.gba` and
`hello.gba` black-box tests. GBATEK
`#arm-opcodes-multiply-and-multiply-accumulate-mul-mla` classifies `MUL` and
`MLA` with `Rd=Rm` as unpredictable on ARM7TDMI, so no hardware result is
claimed. The prototype adopts this explicit deterministic policy for those
destination/multiplicand aliases:

- Both multiplicands are read from the pre-instruction CPU state; the low
  32-bit product is written to `Rd`, including when `Rd=Rm`.
- For `MLA Rd=Rm`, the accumulator is also read before `Rd` is overwritten; the
  low 32-bit product-plus-accumulator is written to `Rd`.
- Existing `MULS` flag behavior is unchanged: N/Z follow the result, C is
  preserved as the existing design choice, and V is unchanged.
- The same operation is emitted by AOT and tested against independent expected
  values. Other unpredictable aliases and all unrelated unsupported
  instructions remain explicit errors.

This design extension may let a local test ROM progress but establishes neither
ARM7TDMI behavior nor compatibility. Only a visible PASS marker drawn by the ROM
counts as its test result.

## D22 — Test-driven prototype VRAM aliases

Decided by the owner on 2026-10-29 after `memory.gba` advanced past its palette
alias test and stopped at `0x06020000`. GBATEK
`#gba-memory-map` lists `0x06018000..0x06FFFFFF` as unused, but the local
black-box suite tests two video-memory aliases. The prototype adopts these
explicit policies to advance that suite:

- The VRAM address window `0x06000000..=0x06FFFFFF` repeats a 128 KiB aperture.
- In display modes 0..=2, aperture offsets `0x18000..=0x1FFFF` map to
  `0x10000..=0x17FFF`; in bitmap modes those offsets stay unmapped.
- Other memory behavior, including OAM boundaries and open-bus reads, remains
  unchanged and fail-closed.

Core bus vectors check the 128 KiB alias, tiled-mode 32 KiB alias and
bitmap-mode unmapped boundary. A generated-native fixture checks the first two;
the bitmap-mode boundary remains a core-bus-only assertion. This test-driven
extension is not a hardware-map or compatibility claim.

## D23 — Narrow test-driven OAM alias

Decided by the owner on 2026-10-29 after `memory.gba` passed the palette and
VRAM mirror cases and stopped at its OAM mirror case. GBATEK
`#gba-memory-map` lists `0x07000400..0x07FFFFFF` as unused; the prototype adds
only the exact alias the local black-box test checks:

- `0x07000400..=0x070007FF` maps to OAM offsets `0x000..=0x3FF`.
- `0x07000800` and later addresses remain unmapped.
- All existing OAM access-width rules, including ignored byte stores, apply
  equally through the alias.

Core bus vectors and a generated-native fixture verify the alias. It is a
test-driven prototype extension, not a hardware-map or compatibility claim.

## D24 — ARMv2 P-variant remains unsupported on ARM7TDMI

Rechecked on 2026-10-29 while investigating `arm.gba`'s `CMP` encoding
`0xE15FF000`. GBATEK's data-processing reference describes the `Rd=R15`
P-variant result/PSR behavior as applying to ARMv2 and below only. ARM7TDMI's
technical reference identifies its core as ARMv4T. Its §6.5 describes compare
and test operations as producing no data result, but does not classify a
nonzero `Rd` field. Both the ARM Architecture Reference Manual DDI0100E §4.1.14
and DDI0100I §A4.1.15 label the `CMP` bits [15:12] `SBZ`; both manuals list
`CMP` for ARMv4T and define nonzero SBZ values as UNPREDICTABLE. Thus no
ARMv4T behavior is defined for this encoding. The decoder continues to reject
nonzero `Rd` fields on test operations instead of emulating the older P-variant
or guessing an ARMv4T result.

This preserves the fail-closed boundary rather than making a local test pass via
an unsupported CPU behavior. The blocked ROM result is recorded in
`tests/roms/gba-tests/README.md`.

## D25 — Implement documented ARMv4T swap instructions

Decided 2026-10-29 while surveying the remaining `arm.gba` instruction families.
ARM Architecture Reference Manual DDI0100E §4.1.51–52 documents `SWP`/`SWPB`;
the operations are generic ARMv4T behavior, unlike the earlier blocked ARMv2
P-variant tests. Implement the word and byte forms in shared IR, interpreter and
AOT emission, including ARM7's unaligned word-load rotation and unrotated store.
Permit the documented `Rd=Rm` alias and reject R15 operands, `Rn=Rd`/`Rn=Rm`
aliases and nonzero SBZ bits rather than guessing unpredictable behavior.

The runtime executes the operation as one non-timed guest step. It models no
mid-instruction scheduling, cycle-accurate bus arbitration or DMA interaction.
Synthetic independent tests cover the memory effects and AOT constant
invalidation. `arm.gba` remains blocked earlier at test 234 and is not claimed to
pass because the later swap cases are implemented.

## D26 — Test-driven prototype palette alias

Recorded 2026-10-29 for the existing owner-approved extension exercised by the
optional `memory.gba` black-box mirror test. GBATEK `#gba-memory-map` lists
addresses after the physical 1 KiB palette region as unused; this prototype
maps the 16 MiB `0x05000000..=0x05FFFFFF` window to that 1 KiB storage using the
low ten address bits.

The existing palette access-width rules apply through the alias. Core bus
vectors and a generated-native synthetic fixture cover alias reads/writes. This
is a test-driven design extension, not a hardware-map or retail-compatibility
claim; no other unmapped region is relaxed by this decision.

## D27 — Behavior-only GBA IRQ dispatcher and return

Approved by the owner on 2026-10-29 after the recorded VBlank IRQ path stopped
at an unaligned ARM PC. Implement the public GBATEK BIOS interrupt-handling
contract as behavior only: save the documented six IRQ-frame values in their
documented full-descending order, call the word-aligned ARM handler through
`0x03007FFC`, and recognize the documented handler return at the BIOS
continuation `0x00000138` only in ARM IRQ mode. Restore the frame and perform the
documented interrupted-state return without fetching BIOS memory or creating
BIOS instruction bytes. Validate the complete frame range and executable
handler target before changing CPU or guest memory.

This does not reproduce or claim to execute the GBA BIOS. `0x03007FF8`, IRQ
nesting, FIQ, other IRQ sources, BIOS boot and all scanline/cycle timing remain
outside the model. Entry consumes no guest instruction budget; handler steps and
the behavior-only return each consume one modeled guest step. Synthetic core
and AOT tests establish only this bounded prototype contract, not hardware or
retail-game compatibility.
