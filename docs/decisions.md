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

## D28 — Instruction-count `VCOUNT` progression across 228 lines

Approved by the owner on 2026-10-29. The two-value `VCOUNT` placeholder (`0` in
the active phase, `160` during VBlank) is replaced by a read-only line counter
over the documented 228 lines. The existing frame budget
`FRAME_INSTRUCTIONS = 280896 / 3 = 93632` guest instructions is retained and
divided without a lost remainder: every line owns `93632 / 228 = 410`
instructions, and the 152-instruction remainder is distributed one instruction
at a time across the frame (`bus::line_instructions`,
`bus::instructions_until_line`). The 228 line budgets therefore sum to exactly
93632. A fresh bus starts on line 0 with that line's full budget ahead, the
counter wraps from 227 to 0, and `Bus::tick_frames(n)` produces the same state
as `n` single `Bus::tick_frame` calls.

The old model's VBlank phase was an undocumented placeholder of one tenth of the
frame. GBATEK `#lcd-dimensions-and-timings` gives 228 equally long lines of 1232
cycles (228 × 1232 = 280896), so equal per-line instruction slices are the
closest instruction-count approximation of the documented proportions. The
VBlank request is therefore raised when the model enters line 160, and the
DISPSTAT VBlank flag follows the documented DISPSTAT wording
(`#lcd-io-interrupts-and-status`): set on lines `160..=226`, clear on line 227.
This moves the request from the previous 84269-instruction placeholder boundary
to 65706 instructions, which is the justified correction produced by replacing
the arbitrary tenth with the documented hidden-line period. The behavior-only
IRQ dispatcher contract itself is unchanged.

This is explicitly **not** scanline or cycle timing: one guest instruction is not
three cycles, line slices model neither drawing nor HBlank periods, the
remainder instructions are spread evenly rather than at their exact cycle
positions, and no instruction observes a mid-line split. HBlank IRQs,
VCOUNT-match IRQs, per-line interrupt scheduling, DMA/HBlank synchronization and
BIOS execution remain unsupported. Synthetic tests establish only this bounded
prototype contract.

## D29 — Instruction-budget timer progression

Approved by the owner on 2026-10-29. `TM0..TM3` leave the earlier storage-only
state and become counter models driven by the same nominal clock as D28: every
retired guest instruction contributes `INSTRUCTION_CYCLES = 3` cycles. Prescaler
selection is `F/1`, `F/64`, `F/256` or `F/1024`; count-up timers ignore their
prescaler and increment once per previous-timer overflow, including chained
cascades. Overflow copies the reload latch and can raise the matching `IF` bit
when the timer IRQ-enable bit is set, including overflow of a count-up timer.
The existing `IME`/`IE` gate and behavior-only dispatcher handle delivery; no new
exception path is introduced.

The documented reload/current-counter split is retained: `TMxCNT_L` writes only
the reload latch, reads return the current or frozen counter, and the reload is
copied on overflow or a `0 -> 1` start edge. A 32-bit reload-plus-control write is
composed low halfword first, so the written reload is used by the edge. Stop
freezes the counter; a prescaler or count-up change on a running timer keeps the
counter but drops the partial phase. Timer 0's documented unused count-up bit is
accepted and ignored, reading back as zero, while Timer 1..3 use it for
count-up. Reserved control bits are masked to zero. A 32-bit reload-plus-control
write remains two ordinary halfword writes.

This is **not** cycle-accurate timing. Instruction lengths, waitstates, bus
arbitration and mid-instruction reads are absent, so a counter poll sees only
completed instruction ticks. Timers never consume Sound FIFO samples, start or
select Sound DMA, request FIFO refills, drive `VCOUNT`/HBlank/video capture or
any other peripheral; `SOUNDCNT_H` timer-select bits stay storage-only. Synthetic
ARM, Thumb and native-AOT parity tests establish only this bounded prototype
contract.

## D30 — Explicit mask-ROM write policy and serial register storage

The uncovered stops on the external test cartridge were a `CpuSet` whose
destination range lies wholly inside the 16 KiB BIOS (`0x00000000..=0x00003FFF`)
and then, after that was modelled, stores into the Game Pak ROM window. On
hardware the BIOS is mask ROM and a store there has no effect instead of being a
CPU-visible fault, so the failure was a prototype policy boundary rather than a
demonstrated defect.

Two explicit policies replace the previous blanket rejection:

- `BiosWritePolicy::Strict` remains the default and reports `BiosAccess`.
- `BiosWritePolicy::RomWritesDropped` is an explicit host opt-in
  (`run --synthetic-bios-rom-writes`) that completes the store with no effect.
  It never stores, synthesizes, reads back or executes a BIOS byte, and the HLE
  copy services accept a BIOS destination only under this policy.

The serial registers `SIOCNT`, `RCNT`, `SIODATA32_L/H` and `SIODATA8` are
documented R/W storage. Only a normal-mode transfer with the internal shift
clock can be answered, and only because a disconnected port has a documented
state (SI high, received bits one). An externally clocked start is accepted as
*pending*: with no partner there are no SC edges, so the start bit stays set and
the data registers keep their values, which lets the guest reach its own timeout
path. Multiplay and UART starts stay explicit `UnsupportedIoWrite` errors rather
than fabricated completions; in general-purpose/JOY-BUS mode `SIOCNT` is
documented as unused and is plain storage. `RegisterRamReset` group 5 clears that storage
and selects general-purpose mode.

The Game Pak ROM window keeps its own fail-closed `RomWrite` error: writing
there depends on a flash command engine that this prototype does not model, so
it must not be turned into a silent no-op.

## D31 — Explicit ROM-only cartridge write policy

After the BIOS destination and the serial start were modelled, the same external
run stopped at a guest store into the Game Pak ROM window (`0x080000C8`). The
destination appears in one instruction with no other register change, so it is a
PC-relative literal load from cartridge data rather than a computed RAM pointer.

A ROM-only cartridge cannot be written; a flash cartridge would need a command
engine that this prototype does not model. `RomWritePolicy::Strict` therefore
stays the default and reports `RomWrite`, while the explicit opt-in
`--synthetic-rom-writes-ignored` models the documented no-effect store. It never
writes cartridge bytes, models no flash engine and does not govern the backup
window, which has its own policy (D32).

With the three opt-ins the run advanced to the guest's backup-device probe
(`0x0E005555`), which needs a save-type model that is out of scope.

## D32 — Volatile SRAM backup model and V-Counter match IRQ

Two further policy boundaries were uncovered by the same external run. After the
ROM-only store opt-in the guest probed the backup window with the classic flash
unlock sequence (`0xAA` to `0x0E00_5555`), which the prototype answered with
`UnsupportedSave`. Answering that window with plain SRAM bytes — so the guest's
probe takes its own no-flash path — is a device model, not a save feature: the
model synthesises no flash device ID and implements no command engine.

- `BackupPolicy::Strict` remains the default and reports every access in the
  64 KiB window as `UnsupportedSave`.
- `BackupPolicy::Sram32KVolatile` (`run --synthetic-backup-sram`) models a
  byte-wide, volatile 32 KiB SRAM that starts erased (`0xFF`). It has no flash
  command engine, no persistence and no file surface; wider accesses and the
  window beyond 32 KiB stay explicit errors. The SRAM contents are guest state
  and are included in `Bus::state_digest`.

The same run then waited in a five-instruction loop at `0x080008C6` with
`IE=0x0005`, `IME=1`, `DISPSTAT=0x9628` (LYC=150, V-Blank and V-Counter IRQ
enabled) and an advancing `VCOUNT`. The loop is a per-frame wait, not a hang:
it exits about once per frame and the guest then reads `KEYINPUT` and calls its
subsystems. `IE` bit 0 is V-Blank and bit 2 is the
V-Counter match. `IF` read zero at the observation point, which only shows that
the guest's acknowledge path had run; the V-Counter request itself was never
latched because the prototype had no match model. The documented match flag and
request are now modelled: DISPSTAT bits
8..15 are writable as `LYC`, the read-only match flag (bit 2) reflects
`LYC == VCOUNT`, and with the V-Counter IRQ enable (bit 5) each transition into
the matching line latches `IF` bit 2. This is a generic, documented register
behaviour, not a cartridge-specific condition, and it does not add HBlank,
cycle timing or BIOS execution.

HBlank remains unimplemented: `IE` bit 1 and `DISPSTAT` bit 4 are still absent
from the model, so a guest that needs HBlank still stops or waits instead of
receiving a fabricated interrupt. D32 narrows D28's exclusion list: the
VCOUNT-match request is now modelled, while D28's other exclusions (HBlank,
per-line scheduling, cycle timing) still hold.

## D33 — Explicit opt-in for documented decompression HLE

The external run reached the guest's frame-callback path only under a
counterfactual boot-flag write, and that path immediately called SWI `12h`
(`LZ77UnCompReadNormalWrite16bit`) with source `0x08DC3EA4`, destination
`0x06000000` and header `0x0004E010` (compressed type 1, 1248 bytes). The
prototype's six-service HLE contract therefore stopped the title before any
graphics data could be loaded. The owner approved extending the HLE with
documented decompression formats on 2026-10-29.

D33 keeps the default boundary intact. `DecompressionPolicy::Strict` remains the
default and returns `UnsupportedService` for `11h/12h/14h/15h/16h/17h/18h`.
Only the explicit `DecompressionPolicy::DocumentedFormats` opt-in
(`run --synthetic-bios-decompress`) implements:

- LZ77 (`11h` byte stores, `12h` halfword stores),
- run-length (`14h` byte stores, `15h` halfword stores),
- 8-bit diff filter (`16h` byte stores, `17h` halfword stores),
- 16-bit diff filter (`18h` halfword stores).

The formats are reimplemented from the public GBATEK descriptions (S1), never
from BIOS bytes, code or disassembly. The header's compressed type and
reserved/data-size nibble must match the service; the source must be 4-byte
aligned. Byte variants accept only EWRAM/IWRAM. Halfword variants accept
EWRAM/IWRAM/palette/VRAM/OAM but require a halfword-aligned destination and an
even decompressed size. ROM, BIOS, save, I/O and unmapped destinations are
explicit errors. LZ77 `disp=000h` is rejected for `12h`, matching GBATEK's note
that the Vram variant works only with `disp=001h..FFFh`.

The decoder rebuilds the documented output bytes in a host buffer before the
first destination store, so a decode failure leaves the destination untouched;
a structurally valid stream that simply describes different bytes cannot be
detected without a checksum. The store itself is not transactional. `10h` BitUnPack
and `13h` Huffman remain unimplemented and explicit unsupported services.

This is a behavior-only format implementation with no BIOS timing or retail-BIOS
compatibility claim. It narrows the "six services" boundary only for callers
that explicitly select the policy; AOT harnesses remain on the default policy.

## D34 — 64 KiB volatile SRAM device model

The external run with the approved decompression opt-in stopped at
`0x0E008000`, the first address beyond the modelled 32 KiB SRAM. GBATEK
documents the backup window as `0E000000-0E00FFFF Game Pak SRAM (max 64 KBytes)`.
A throwaway experiment that raised the model to 64 KiB advanced from 16,593,186
to 18,911,571 instructions, so the boundary was the device capacity, not a flash
command.

`BackupPolicy::Sram64KVolatile` (`run --synthetic-backup-sram64k`) adds a second
explicit, volatile device model covering the whole documented 64 KiB window.
The existing 32 KiB policy and its flag are unchanged; `Strict` remains the
default. Neither model persists anything, implements a flash command engine or
claims a flash device ID. The modelled bytes are guest state in
`Bus::state_digest`; the policy is host configuration. Selecting both capacity
flags is an argument error.

## D35 — Explicit opt-in for documented ObjAffineSet

With D34 the counterfactual run reached SWI `0Fh` (`ObjAffineSet`) at step
18,911,571. The first call used source `0x03007DAC`, destination `0x03007DB4`,
count 1, offset 2, scale `(512, 512)`, angle `0x0000`. The owner approved
implementing the documented service as a further explicit opt-in.

`AffinePolicy::Documented` (`run --synthetic-bios-affine`) implements only SWI
`0Fh` from the public GBATEK interface: six-byte source entries (s16 scale x,
s16 scale y, u16 angle), four s16 destination parameters (PA, PB, PC, PD), the
upper eight angle bits used, and destination stride `offset` between parameters
with the next calculation starting `4*offset` bytes later. `offset=2` therefore
stores the four parameters contiguously; `offset=8` places one parameter per
8-byte OAM slot, matching the documented OAM structure.

The HLE computes the mathematical affine matrix from a 256-entry Q12 sine table
generated from the mathematical sine definition, not copied from a BIOS image.
It does not claim bit-identical results to a particular BIOS sine table or
rounding. Products use `i32` and the final store keeps the low 16 bits, matching
a hardware `STRH`. Source/destination ranges, 2-byte alignment and the even
offset precondition are validated before the first store; strict policy keeps
`0Fh` an explicit unsupported service. `BgAffineSet` (`0Eh`) and every other
service remain unimplemented at this decision; D38 later extends the same
`AffinePolicy::Documented` opt-in to `BgAffineSet`.

## D36 — ARM multiply-long instructions

The external run stopped at `0x082F4D90` on `UMULL r3,r2,r1,r0` (opcode 4,
unsigned non-accumulating long multiply) after the affine HLE had advanced it
to step 19,075,202. The multiply-long family is a documented ARMv4 instruction
group, not a new HLE boundary, so it is implemented in the decoder,
interpreter and AOT emitter:

- `UMULL`/`UMLAL` use an unsigned 64-bit product/accumulator.
- `SMULL`/`SMLAL` sign-extend both operands and use a signed 64-bit
  product/accumulator.
- `RdLo` receives the low half and `RdHi` the high half; `RdHi == RdLo` and R15
  operands are rejected as invalid/unpredictable.
- With `S=1`, N is bit 63 and Z covers the full 64-bit result. C remains the
  existing documented-design preservation choice and V is unaffected, matching
  the 32-bit `Mul` policy.

The external run then continued past the former stop to the title sequence.
Synthetic raw-encoding vectors cover all four instructions, flags, the invalid
aliases and native-AOT reference equality.

## D37 — Volatile 128 KiB flash device model

Without a flash device the cartridge probe writes `0` to `0x03005D88`; the
boot check at `0x08000408` then clears callback2 and the game idles. A
throwaway observation that returned the documented flash autoselect ID
(`0x62`/`0x13`) made the probe store `1` and boot the intro **without any
counterfactual flag patch**. The missing capability was therefore the flash
device, not a cartridge-specific flag.

`BackupPolicy::Flash128KVolatile` (`run --synthetic-backup-flash128k`) models a
volatile 128 KiB flash with two 64 KiB banks and the documented command subset:
unlock `0xAA`/`0x55`, autoselect `0x90` (manufacturer `0x62`, device `0x13`),
bank switch `0xB0`, program `0xA0`, sector erase `0x30` and chip erase `0x10`.
Reads outside autoselect use the selected bank. The model persists nothing,
implements no other flash commands and is not a save-file or flash-compatibility
claim. The storage and command/bank state are guest state in
`Bus::state_digest`; the policy is host configuration. The strict default and
both SRAM opt-ins are unchanged.

## D38 — Mode 0/1 PPU and the observed start screen

After the multiply-long and flash work the unmodified ROM reaches the title
sequence. The static renderer now composes the documented Mode 0 layers
(BG0-BG3 text + OBJ) and Mode 1 layers (BG0/BG1 text + BG2 affine + OBJ), with
window 0, the OBJ window and `BLDCNT` alpha/brightness blending. BG2/BG3 affine
matrix/reference registers and BG0-BG3 scroll/control registers are modelled;
OBJ normal and affine sprites use 1D mapping with 4/8bpp tiles.

The counterfactual-free cross-compiled x86_64 CLI, run with the explicit
`synthetic-bios-zero-fill`, `synthetic-bios-rom-writes`,
`synthetic-rom-writes-ignored`, `synthetic-backup-flash128k`,
`synthetic-bios-decompress` and `synthetic-bios-affine` flags at a
455,000,000-instruction budget, produces the title screen:
`DISPCNT=0x1741`, `image_sha256=1f2febbccf5c507fdac2ad8c6bebab3597147d8fffac426299ba90190265af67`,
visibly showing the `SMARAGD EDITION` logo and `DRÜCKE START` with the 2005 Game Freak
notice. This is a static snapshot of the observed title state, not a gameplay,
input, timing or compatibility claim.

The renderer remains a deliberate subset: 2D OBJ tile mapping, OBJ double-size
affine sprites, OBJ/BG mosaic, window 1, Mode 2 and scanline effects are
explicit errors or absent. Alpha/brightness math uses the final 8-bit channels
as a stated approximation of the 5-bit hardware channels.

## D39 — Trace-seeded native AOT with sealed work-RAM blocks

Decided 2026-10-30 for the native title-screen evidence. The previous
title-screen run used the reference interpreter, so it did not separate native
translation from interpretation. `aot build --trace` now accepts a one-time
reference trace of `(address, state, raw encoding)` seeds.

- ROM seeds are decoded from the immutable cartridge image; a trace word that
  disagrees with the cartridge is a hard error, not a reinterpretation.
- Work-RAM seeds become sealed one-instruction blocks. The generated dispatcher
  re-reads the live encoding at the seeded address and calls the native block
  only on an exact match. A mismatch uses the reference interpreter with a fresh
  fetch/decode and counts as a fallback. No RAM or ROM snapshot is embedded and
  no follow-on RAM code is discovered.
- The artifact binds the ROM by SHA-256+length, reports `sealed_blocks`,
  `rejected_seeds`, SWIs and the native/IRQ/fallback counters, and contains no
  ROM bytes.
- The native report separates `native_instructions` (retired from generated
  blocks) from `irq_dispatch_returns` (modelled BIOS dispatcher returns) and
  `fallbacks`; the three counters plus the remaining budget must equal the
  budget.

This is a bounded, profile-guided AOT mode, not general static translation of
the ROM. The trace is scratch input under the ignored `target/` tree; committed
tests use synthetic fixtures only. Sealed RAM blocks are a reviewed exception to
the "static AOT is ROM-only" boundary, not a RAM-snapshot or general
self-modifying-code claim.

## D40 — Portable input, shell, fullscreen and persisted settings

Decided 2026-10-30. The runtime already exposed `Bus::set_pressed_keys`, but no
host-side button model, touch layout, settings document or frontend path used
it. The new std-only `core::input` module owns:

- the ten documented buttons and their KEYINPUT bits;
- canonical host key codes and default bindings (`Z/J`, `X/K`, Enter,
  Backspace, arrows/WASD, Q/E), with duplicate/empty-edit rejection;
- an 800x600 landscape and a portrait normalised layout for pointer/touch hit
  testing; the screen area is never a button;
- a stable `key=value` settings document (schema 1) with fullscreen, touch and
  integer-scaling preferences plus all bindings;
- instruction-indexed input scripts (`at:KEYS:hold`) and a scripted reference
  runner.

The desktop frontend renders a windowed GBA shell (guest screen at 2x, D-pad,
face/menu buttons, pressed highlighting), a fullscreen 3x frame, and a settings
page listing every active binding. F1 opens settings, F11 or Alt+Enter toggles
fullscreen, Escape closes settings or exits, and mouse/touch presses hit-test
the same layout. The per-user settings file is written atomically on change and
exit. The browser frontend uses the same key codes/binding defaults in
`www/input.mjs`, maps touch buttons to the same mask and persists the settings
in `localStorage`; the Fullscreen API falls back to a CSS mode when a browser
embedding denies it.

This is a synthetic frontend: it does not accept a ROM, retail save data or
guest audio, and the buttons drive presentation state only. The browser save
panel manages host-side synthetic raw images. It is not a retail input or playability
claim. Android/iOS packaging remains open (#11); per-ROM input routing remains
open (#7).

## D41 — Synthetic input-tester ROM and Linux-native visual gate

Decided 2026-10-30. A committed synthetic Mode 3 fixture (`demo/input_tester.rs`,
`tests/roms/synthetic/input-tester.gba`, 4 KiB) draws ten 16-pixel bands in
KEYINPUT order. A band is white while its button is held and dark otherwise, so
one fixed mask produces a deterministic, human-readable frame. The source,
committed bytes, script and expected PPM are pinned by SHA-256 and checked by
`crates/core/tests/input_tester_fixture.rs`.

`scripts/input-tester.sh` regenerates the bytes, builds the AOT harness for the
host triple, runs it with the committed script and compares the output frame
byte-for-byte. `--visual` renders expected and actual frames in the pinned
Chromium through Playwright, compares every pixel in the browser and writes a
screenshot artifact. The Linux CI job runs this path on `ubuntu-latest`; the
default gates stay browser-free and offline.

This is fixture evidence for the input path only. It is not hardware
conformance, cycle timing, a retail-ROM test or a playability claim.

## D42 — Cross-target build matrix

Decided 2026-10-30. The supported cross targets are `aarch64-apple-darwin`,
`x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu` and
`wasm32-unknown-unknown`. `scripts/cross.sh` builds the same synthetic
input-tester artifact for every entry and verifies the produced file type;
host-executable Apple artifacts also run and match the expected frame.

- Linux uses `zig cc` as an optional host linker (`scripts/zig-linker.sh`).
- Windows GNU uses mingw-w64; the MSVC target needs a Windows SDK/linker and is
  not claimed.
- wasm32 uses the pinned wasm-bindgen CLI and stays a module, not a GUI app.
- A missing prerequisite fails with an installation hint; it never becomes a
  silent skip or a false success.

CI covers the Linux-native path on the actual Linux runner. The other targets
are local build evidence unless a runner with the required linker executes them;
a successful build is not a platform-support or gameplay claim.

## D43 — Native, state-driven audio generation

Decided 2026-10-30. Audio is generated from the same sound-hardware state the
guest writes; it is not a pre-recorded asset. `core::audio` is std-only and
contains no platform dependency:

- `render_psg` mixes two square channels, the wave channel and the noise channel
  from the documented sound-register image.
- `AudioEngine` keeps phase/LFSR state and advances once per guest instruction
  (`advance_instruction`), so a register write changes the very next generated
  sample instead of waiting for a fixed snapshot interval.
- `AudioEngine::advance_instruction_bus` additionally mixes Direct Sound from
  the live bus: FIFO A/B bytes are consumed at the timer-0/1 overflow rate
  (`16.777216 MHz / (prescaler * (65536 - reload))`), SOUNDCNT_H volume/enable
  and timer selection are applied, and a half-empty FIFO requests the
  sound-timed DMA burst. An underrun holds the last sample; unsupported DMA
  configurations return an explicit error.
- `AudioSettings` supplies a 0..=100 master volume and digital mute; the
  persisted settings document stores `master_volume` and `muted`.
- `wav_bytes` is a deterministic RIFF/WAVE sink. `render_tone` is a synthetic
  test signal, not game audio.

The guest-state host path is a WAV artifact from the reference runner and the
AOT harness; both now render PSG plus Direct Sound from guest state. The browser
frontend persists the same volume/mute fields and can play a fixed 440 Hz
WebAudio test tone, but it does not yet stream guest PCM; that sink remains
outstanding in issue #29. Native live playback is target-specific and must not
add a default-workspace dependency or unsafe code. The mixer is deliberately
bounded: it has no full sweep/envelope/length frame sequencer and no
cycle-accurate resampling. The Direct Sound rate is derived from the
instruction-budget timer model, so it is an audio approximation, not hardware
cycle timing. Those gaps and the cross-target acceptance tests are tracked in
issue #29. This is not a retail-audio or hardware-timing claim.

## D44 — Host save library and synthetic save fixture

Decided 2026-10-31. Saves are host-side backup images, keyed by the cartridge
SHA-256 and never copied into the repository:

- `core::save::SaveLibrary` stores `<root>/<sha256>/<slot>.sav` plus a `default`
  marker. Slot names are restricted to `A-Z`, `a-z`, `0-9`, `.`, `_`, `-`
  (1..=32 characters, no leading dot) so a name cannot escape the directory.
- The CLI exposes `run --save-dir/--save-slot` and
  `save list|import|export|use|default|delete`. The first stored slot becomes
  the default; deleting the default falls back to a remaining slot.
- The browser keeps the same fields in a JSON `localStorage` document, encoded
  as base64 raw images. The settings page supports import, export, use and
  delete; no upload or account is involved.
- The generated AOT harness accepts `--save PATH` and loads/writes the raw
  backup image next to the ROM binding.

The committed `tests/roms/synthetic/save-tester.gba` writes a magic and counter
to byte-wide SRAM. A fresh run paints one frame and counter 1; a run with the
persisted image paints a different frame and counter 2. `core::save`,
`scripts/save-tester.sh` and the Linux CI job exercise multiple slots, default
selection and import/export. A separate ignored, environment-gated test proves
the same host persistence locally with a user-supplied Emerald ROM: the observed
intro/name/truck/menu flow reaches SICHERN, writes a 128 KiB flash image with
56,331 non-erased bytes, reloads it through `SaveLibrary`, and writes its
evidence only under `output/`.

Limits: this is storage for explicitly selected SRAM/flash models, not a
save-device compatibility claim. The browser save panel manages synthetic raw
images; it does not execute guest saves in the page. No retail save data, ROM or
BIOS bytes are committed.
