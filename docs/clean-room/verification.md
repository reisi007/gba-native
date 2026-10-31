# Verification boundary

Status: experimental; no retail game is playable, no GPU vendor integration
exists, no successful REA Evidence was obtained, and this is not a formal
separate-team legal clean-room process.

Run `./scripts/short.sh` for the fast gate and `./scripts/full.sh` for the
complete gate. They invoke `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`,
`cargo test --release`, the demo and the real native AOT proof. Ratchet is
pinned to release v0.6.3 of `all-the-rest/ratchet-cli` (the tag is
lightweight and points at commit `4cace52ac7731985130292ead6ae81d44b4d72d6`);
a missing or mismatched tool fails
instead of skipping the gate.

## Automated checks

### Source size and quality gates

`src/bin/quality-counter` is the single maintained-Rust counter. It scans every
`*.rs` file in the repository except the root-level generated directories
`.git/`, `target/` and `output/`, so root files, `build.rs`, tests and new
untracked sources count. The exemption is positional: a nested directory with
one of those names, such as `src/output/` or `tests/target/`, is scanned like
any other source directory. An empty population exits 3. Physical lines include
blank and comment lines, and a trailing newline adds no phantom line.

- The hard limit is 500 physical lines. `scripts/short.sh` and CI run the
  counter with `--hard`, directly and independently of the Ratchet baseline, so
  editing or raising the baseline cannot dilute the limit.
- A 501-line file fails; 500 lines pass; a nested new file is found; comments
  and blank lines count; an unreadable file and an empty selection fail closed;
  a trailing newline does not produce a phantom line. Root-level `target/`,
  `output/` and `.git/` are skipped, while a 501-line file in a nested
  `src/output/` or `tests/target/` is still reported by `--hard`.
- The Ratchet rules are `oversized-rust-file` (ceiling, baseline 0),
  `rust-source-population` (floor over the same population) and
  `native-consistency` (slow floor 1). The baseline is machine-written by
  `ratchet baseline init/update`, never hand-edited.
- `tests/quality_counter.rs` covers the counter paths; `tests/scripts.rs` covers
  missing Ratchet, a mismatched Ratchet version and missing Cargo.

### Workspace split and compatibility contracts

The default repository graph is one Cargo workspace with three internal
packages: the root `gba-native` CLI/compatibility facade, `crates/core`
(`gba-native-core`) and `crates/aot` (`gba-native-aot`). The root package remains
a package (not a virtual workspace) because `CARGO_BIN_EXE_*`, manifest
resolution and `default-run` depend on it. All three are in `default-members`;
`apps/desktop` and `apps/web` are excluded and keep their own workspaces and
lockfiles.

- `tests/workspace_contract.rs` exercises the actual workspace, not manifest
  text. It imports every legacy facade module, performs a real discover/emit,
  and proves that core `Error` and the AOT/root error spellings are one identity.
  It runs `cargo metadata --offline --no-deps` and checks the three
  `default-members` and both frontend exclusions; it checks the root lockfile has
  exactly the three internal packages and no registry `source`/`checksum`; it
  runs a default `cargo tree` and rejects desktop/browser dependencies,
  including Pixels/Winit/WGPU and wasm-bindgen.
- The same test lists the actual test inventory in an owned fresh
  `CARGO_TARGET_DIR` with an **empty owned `CARGO_HOME`** and `--offline
   --locked`. It pins every target: 193 core integration tests across 28 files,
   41 core library unit tests, 34 AOT integration tests across 13 files, 22 AOT
   library unit tests, 31 root integration tests across six files and 5 root
   binary unit tests, exactly 326 selected package tests. The workspace
   compatibility contracts and frontend-tree failure contract add four tests to
   the full `cargo test -- --list` inventory (330 total). The web clean-checkout
  bootstrap test is included in the root integration targets; no prior core,
  AOT or root tests are removed. Bare
  `cargo run` default-run help, `demo` and `aot proof` are executed from an
  unrelated working directory that owns all generated output, and the quality
  counter is run through `--bin quality-counter` from the same directory.
- `crates/aot/tests/aot_native.rs` also runs `cargo tree` on the actual
  generated harness after building it. The tree contains the sibling
  `gba-native-core` package and neither `gba-native-aot` nor the root
  `gba-native` facade. The generated manifest keeps the `gba_native` source
  namespace via a package rename and still binds SHA-256+length, requires an
  external `--rom PATH` and uses no `ROM_BYTES` array.
- `tests/frontend_tree_contract.rs` uses a fake Cargo to prove that
  `scripts/frontend.sh` fails closed when the resolved tree is missing
  `gba-native-core` or contains the AOT crate/root facade. The static desktop
  manifest/lock contract is in `tests/workspace_contract.rs`; the real frontend
  gate performs the locked offline tree check against the actual registry
  resolution.
- The web frontend is isolated in `apps/web`, with its own Cargo/npm lockfiles
  and direct core dependency. `scripts/web.sh` runs the hard source-size check,
  core wasm32 check, host tests/Clippy, wasm build and dependency-tree checks,
  pinned local bundle tooling, and functional browser E2E. It is separate from
  the default gates and does not change the root three-package dependency graph.

### Cartridge

- 192-byte minimum and 32 MiB maximum accepted; 191 bytes and 32 MiB + 1 rejected
  without panicking.
- The path loader consults metadata first and rejects an oversized/sparse file
  before reading payload; the payload read is also capped at `MAX_ROM_LEN + 1`.
  A sparse file larger than 32 MiB is rejected without memory pressure.
- Title/game/maker/fixed/version/complement fields decoded.
- Fixed byte and complement diagnostics are independent.
- SHA-256 changes when bytes change.
- No test reads a commercial ROM or compares Nintendo logo bytes.

### Bus, CPU and decoder

- EWRAM/IWRAM mirror throughout their 16 MiB windows; palette/VRAM/OAM retain
  exact ends and out-of-region accesses error.
- Three ROM windows alias the same bytes; ROM writes and overreads error.
- BIOS, save, unmodelled I/O reads, bad widths and unaligned accesses return
  explicit errors. Unmodelled I/O writes are ignored and recorded in a bounded
  4096-address sample; the total operation count and truncation flag are exposed.
  The unused `0x04000400..=0x04FFFFFF` window is likewise write-only ignored.
- Palette/VRAM byte-store duplication and OAM byte-store ignore behavior.
- DISPCNT and BG0CNT halfword R/W; BG0HOFS/BG0VOFS halfword write-only;
  scroll bits 9..15 do not affect state. KEYINPUT defaults to all buttons
  released; host pressed-mask tests cover active-low mapping for all ten keys,
  unused-bit masking and digest inclusion. Guest KEYINPUT writes have no effect.
  32-bit I/O composes halfword accesses, and mutable BG0 register state changes
  `Bus::state_digest`.
- ARM/Thumb ALU, NZCV for carry/borrow/overflow, condition-false no-op, PC
  advance, direct branches, `BL` link/return, BX ARM↔Thumb round trip, PC
  operand pipeline values, aligned ALU PC writes, and the documented `S=1,
  Rd=PC` exception return from a valid SPSR.
- Full ARM data-processing family, LSL/LSR/ASR/ROR/RRX by immediate and by
  register (low-byte amounts, 0/32/>32/256 cases), ADC/SBC/RSC carry ordering,
  logical V/C preservation, and SPSR-based `S=1` PC return semantics. See
  `crates/core/tests/arm_alu.rs`, `crates/core/tests/arm_shift.rs` and `crates/core/tests/arm_control.rs`.
- `crates/core/tests/exception_return.rs` and
  `crates/aot/tests/aot_exception_return.rs`: valid ARM/Thumb state and bank
  restoration, PC alignment from the restored T bit, NZCV restoration instead
  of the ordinary ALU result, dynamic-PC discovery metadata, and fail-closed
  missing/invalid SPSR cases with no state mutation.
- Thumb move-shifted, the other fifteen register ALU opcodes plus `MUL`, high-register
  ADD/CMP/MOV (low/low MOV/CMP only as the contract's **Design** extension),
  ADR/PC/SP adjustment, and both `BL` halves with an exact mid-pair budget
  stop. Low/low ADD and ARMv5 BLX stay explicit errors. See
  `crates/core/tests/thumb_alu.rs` and `crates/core/tests/thumb_control.rs`.
- ARM7 odd-address LDRH rotation, STRH forced alignment, unaligned literal LDR
  rotation.
- ARM halfword classification and signed offsets (`crates/core/tests/arm_halfword.rs`):
  independently composed pre-indexed immediate `LDRH`/`STRH` with `U=0` zero,
  positive and negative magnitudes, all 15 condition codes, true/false
  conditional execution, and no memory write on a false condition. The exact
  regression words `0xE14322B0`, `0xE14000B0` and `0xE15322B0` are covered, and
  the same suite re-checks MRS/MSR for all R0-R14, all 16 immediate rotations and
  malformed fixed fields so halfword precedence cannot regress PSR decoding.
- ARM PSR transfers: CPSR/SPSR MRS, register and rotated-immediate MSR, field
  masks, empty-mask rejection, reserved-bit preservation, USER control-write
  ignoring, invalid mode/T-bit rejection before mutation, USER/SYSTEM SPSR
  errors, conditional no-op, and CPSR/Thumb coherence. See
  `crates/core/tests/psr_transfer.rs`, `crates/core/tests/psr_field_mask.rs` and
  `crates/core/tests/psr_banking.rs`.
- `crates/core/src/bus/dma.rs`: word and halfword copies advance by one data
  unit, a fixed source fills the destination, CNT_L keeps its written value,
  and a repeating channel stays enabled. `crates/core/tests/sound_dma.rs` checks
  special-mode arming without an immediate read, explicit four-word FIFO bursts,
  internal source-cursor advancement with visible latches preserved, invalid
  configurations before source access, bounded FIFO wrap/capacity, reset bits,
  and group-7 DMA disarming.
- `crates/core/src/bus/irq.rs`, `sound.rs` and `timer.rs`: the instruction-count
  frame model raises VBlank/IF once when its 228-line `VCOUNT` progression enters
  line 160, IRQ gating requires IME/IE/pending, and timers follow the
  instruction-budget counter model. Sound FIFO state is bounded; plain bus
  stepping consumes nothing. The host audio engine reads timer 0/1's overflow
  rate and issues the explicit FIFO pop and sound-DMA refill calls described in
  the Direct Sound round below.
  `VCOUNT` is readable as its modelled halfword and, since the byte-lane round,
  through exactly the two byte lanes at `0x04000006`/`0x04000007`. The halfword
  walks all documented lines `0..=227` and wraps to 0; the retained 93632
  instruction frame budget is split into 410 instructions per line plus one for
  each of the 152 spread remainder instructions, so the line budgets still sum
  to the frame budget. The DISPSTAT VBlank flag covers lines `160..=226` and is
  clear on line 227. This is an instruction-count approximation, not hardware
  scanline or cycle timing; the byte path handles only `VCOUNT`: the
  pre-existing byte-addressed sound-register reads are unchanged, every other
  address keeps the existing register-specific I/O dispatch, and
  otherwise-unmodelled reads remain explicit unsupported-width errors, including
  the neighbouring `DISPSTAT` lanes and the byte above `VCOUNT`.
- Register banking: all seven modes, shared r0-r7, FIQ-only r8-r12, per-mode
  r13/r14 with USER/SYSTEM sharing, independent exception SPSRs, and state
  digests that include inactive banks.
- Rejected encodings (multiply-long/halfword multiply, unpredictable SWP aliases,
  unsupported block transfers, malformed PSR fixed fields, ARMv5 BLX/BXJ, Thumb PUSH/BLX) report
  the raw word/state and leave PC unchanged. A decoded `S=1, Rd=PC` return with
  no valid SPSR is an explicit `Error::Psr` and leaves the complete CPU state
  unchanged.
- Unaligned/nonexecutable fetches fail explicitly.
- Unknown backend names are preserved in `UnsupportedBackend` (`--backend foo`
  reports `foo`, never `"unknown"`).

### BIOS HLE services

`crates/core/src/bios/` implements six documented, timing-independent services:
`RegisterRamReset` (`01h`), `Div` (`06h`), `DivArm` (`07h`), `Sqrt` (`08h`),
`CpuSet` (`0Bh`) and `CpuFastSet` (`0Ch`). They remain callable directly through
`bios::call` and are reachable through typed ARM/Thumb SWIs. Guest SWI execution
consumes one instruction, performs bounded SVC entry/frame/return, and invokes
the service in SYSTEM mode; handler internals have no timing model. Unsupported
services and supported SVC-origin nested calls error explicitly. Exact policies
are in `hle-services.md`.

- `crates/core/tests/bios_arithmetic.rs`: the documented division example, a signed
  division matrix, zero-divisor and `i32::MIN / -1` errors before mutation, and
  an integer-square-root property.
- `crates/core/tests/bios_memory_copy.rs`: the literal `CpuFastSet` rounding oracle for
  requested counts `0, 1, 7, 8, 9, 15, 16, 17, 23, 24, 25` with independently
  stated expected word counts `0, 8, 8, 8, 16, 16, 16, 24, 24, 24, 32`, a
  sentinel word at every rounded end, fill source-read counts,
  ascending-overlap smear, and range/alignment/reserved-bit failures before
  mutation. The table is not computed from the production rounding formula.
- `crates/core/tests/bios_reset.rs` and `crates/core/tests/bios_dispatch.rs`: flags
  0..4 memory-reset regressions, IWRAM-tail preservation and unconditional
  forced blank; flags 5..7 are accepted, group 6 applies the known sound effects,
  and group 7 resets the modelled other-I/O set while preserving sound (full bus
  digest regressions). Bits 8..31 select nothing and still force blank. Dispatch
  also checks explicit `UnsupportedService` results that mutate nothing.

These tests use synthetic RAM/ROM fixtures only and require no user-supplied
ROM, BIOS image, save, network or external crate. This is not evidence for BIOS
boot, arbitrary requests, cycle timing, hardware conformance or playability.

### Guest SWI/SVC framing

- `crates/core/tests/swi_decode.rs` covers typed ARM/Thumb SWI comments and
  instruction widths.
- `crates/core/tests/swi_execution.rs` covers full SVC entry and return,
  ARM/Thumb resume addresses, CPSR/mode/flag restoration, all six guest services,
  unsupported/precondition rejection before mutation, invalid SVC stacks and
  explicit rejection of SVC-origin nesting.
- The frame is 16 aligned bytes in EWRAM/IWRAM, with `(SPSR, R11, R12, R14)` at
  ascending addresses from `SVC_SP - 16`. GBATEK names the saved values but not
  their slot order; this is an explicit HLE design policy, not a claim about
  Nintendo BIOS frame-layout compatibility.
- `crates/aot/tests/aot_swi.rs` checks direct ARM/Thumb Div/CpuFastSet emission,
  known return continuations, stack initialization, independent frame/bus
  expectations, a complete bank/SPSR digest (including the saved Thumb CPSR),
  reference matching and exact budget stops. `aot_swi_services.rs` adds Div,
  DivArm, Sqrt, CpuSet and RegisterRamReset in one native run with hand-derived
  full-bank/SPSR CPU and bus/frame oracles. `aot_swi_errors.rs` checks a successful
  conditional-false unknown SWI, ARM comment service 00h rejection, division,
  CpuSet and reset precondition failures, invalid SVC stack and SVC-origin
  nesting. Its test-only generated-harness instrumentation reports before/after
  CPU and bus digests on failure to prove each rejection is pre-effect.
- No BIOS instruction bytes are fetched. Guest-run and generated-harness stack
  initialization is explicit; `Cpu::direct_entry()` remains unchanged.

### Independent ISA golden vectors

`crates/core/tests/isa_golden.rs` is written from the documented encoding and arithmetic
expectations, not from the decoder/interpreter:

- Exhaustive 256 imm8 × 16 rotation sweep for ARM immediate `MOV`/`ADD`/`SUB`
  with `S=0` and `S=1`, plus `CMP` (28,672 decoded/executed cases). Each case
  checks typed IR, result, NZCV or preservation, and memory immutability.
- Exact regression encodings: `0xE35000B0` is `CMP r0,#0xB0` (not `LDRH`),
  `BIC` `0xE3C100B0` is a typed data-processing operation (not `STRH`), and the
  malformed CMP encoding `0xE35010B4` with `Rd != 0` errors (does not alias
  `STRH`). Rejected alias candidates leave guest memory and PC unchanged.
- Arm immediate classification happens by bit 25 before inspecting bits 7..4.
- PSR-transfer vectors compose MRS/MSR words from the documented field table
  and check typed IR, all 16 immediate rotations, mask bytes, preservation and
  explicit error cases. They also verify that the earlier bit-25 immediate
  regression still holds (`MOV`/`CMP` immediates are not reclassified).
- `crates/core/tests/arm_shift.rs`: independent value/carry matrix for every shift kind and
  0/1/31/32/33/63/64/255/256 register amounts, immediate specials, RRX both
  signs, and explicit Rs=15 rejection.
- `crates/core/tests/arm_alu.rs`: all sixteen opcodes in register and immediate form,
  ADC/SBC/RSC carry-in and overflow edges, RRX-before-flags ordering, logical
  shifter-carry/V preservation, S=0 preservation, and encoding restrictions.
- `crates/core/tests/cpu.rs::arm_mul_and_mla_rd_eq_rm_use_pre_instruction_values_as_a_design_policy`:
  checks the owner-approved deterministic `MUL` and `MLA Rd=Rm` extensions,
  low-word results, N/Z behavior and C/V preservation; these are not ARM7TDMI
  conformance vectors.
- `crates/core/tests/arm_control.rs`: ARM PC+8/+12 operands, aligned S=0 PC writes,
  S=1 PC exception-return decode metadata, B/BL targets and link register,
  conditional false LR preservation, BX state exchange and BX PC behavior.
- `crates/core/tests/thumb_alu.rs`: all sixteen register ALU forms including
  `MUL`, both shift
  groups, low-byte register amounts, high-register flags/PC rules, ADR/SP
  bases, low/low MOV/CMP, and reserved/BLX errors.
- `crates/core/tests/thumb_control.rs`: the two BL halves, negative high offset,
  second-half-only wrap, and the exact one-instruction budget stop between
  halves.
- `crates/aot/tests/aot_control.rs`: discovery metadata for the ARM BL callee and its
  queued return candidate, the Thumb BL dynamic callee and queued return
  candidate, and ALU PC-write block termination.

### Memory, block and stack vectors

- `crates/core/tests/arm_memory.rs`: word store/load alignment and rotation,
  byte-width access across EWRAM/palette/VRAM/OAM, immediate-shift LSL/LSR/ASR/
  ROR/RRX offsets with unchanged flags, pre/post writeback, error-without-
  writeback, PC base/source pipeline values, conditional false no-access, and
  explicit rejection of PC-writeback, load-alias writeback and narrow PC loads.
- `crates/core/tests/arm_halfword_extended.rs`: preserved old pre-indexed shape,
  pre/post immediate and register offsets, unsigned/signed sign extension, odd
  LDRH rotation, odd LDRSH-as-signed-byte, STRH alignment, PC base/source,
  reserved register bits, ARMv5 LDRD/STRD rejection, and alias/writeback errors.
- `crates/core/tests/arm_block.rs`: IA/IB/DA/DB ascending addresses for both
  directions, writeback and ARMv4 base-in-list specials, PC load alignment/store
  value, `S=1`/empty/PC-base rejection, unaligned-base transfer alignment with
  original-base writeback (including base-in-list), and a documented observable
  partial-failure outcome.
- `crates/core/tests/thumb_memory.rs`: THUMB.7 word/byte register offsets,
  THUMB.8 half/signed register forms, THUMB.9 word/byte immediate scaling,
  preserved THUMB.10 halfword shape, THUMB.11 SP-relative word transfers and the
  preserved literal load.
- `crates/core/tests/thumb_block.rs`: PUSH/POP full-descending order and SP
  restore, LR/PC bits with `POP PC` alignment and Thumb state, empty-list
  PUSH/POP errors plus the ARMv4 LDMIA/STMIA R15/`+0x40` quirk, writeback/base-in-list specials, unaligned SP/base
  transfer alignment with original-base writeback, and partial-transfer
  observability.
- `crates/aot/tests/aot_memory.rs`: direct native word/byte/half/signed accesses,
  unaligned word load, a dynamic `LDR pc` target (one fallback), hand-expected
  registers/bus and reference match. A discovery-only case asserts that a
  same-block literal constant is invalidated by a later memory load or block
  load, so `BX` stays dynamic.
- `crates/aot/tests/aot_block.rs`: direct native ARM STMIA/LDMDB and Thumb
  PUSH/POP/LDMIA/STMIA with hand-expected registers, memory and exact budget.
- `crates/aot/tests/aot_block.rs::native_thumb_empty_ldmia_matches_the_documented_armv4_pc_quirk`:
  native empty-list PC transfer, `+0x40` base writeback, dynamic control-flow
  metadata and reference parity using a synthetic ROM.
- `crates/aot/tests/aot_fallback.rs`: an unknown dynamic target executes through
  interpreter fallback across the modeled active-to-VBlank boundary; native
  output matches reference CPU, bus, image and exact budget. A second synthetic
  fixture arms VBlank IRQs, installs a guest handler and unmasks IRQs; translated
  instructions detect the pending request and the handler executes through a
  fresh fallback, matching reference CPU, bus and budget. This covers IRQ entry
  parity for that fixture; the full bounded behavior-only frame/return contract
  is covered separately by `crates/aot/tests/aot_irq.rs`.
- `crates/core/tests/irq_dispatch.rs` and `crates/aot/tests/aot_irq.rs`: synthetic
  full-descending IRQ frame order, ARM handler entry, IF acknowledgement,
  `BX LR` to the IRQ-only `0x138` continuation, frame/register restore, Thumb
  resume and native/reference parity. Invalid handler targets and IRQ stacks
  fail before CPU or bus mutation; absent handlers, masked requests and attempted
  nesting keep the request pending. The tests do not execute BIOS instructions.
- `crates/aot/tests/aot_block_unaligned.rs`: native ARM STMIA/LDMDA and Thumb
  STMIA/LDMIA with unaligned bases; hand-expected aligned transfer words and
  original-base writeback compared against the reference interpreter.
- `crates/aot/tests/aot_ram.rs`: the dedicated issue #17 native-RAM regression.
  It asserts that only ROM blocks are present, the two RAM instructions each use
  a fresh reference fallback on both visits (exactly four), the patched
  `MOV r0,#2` is observed (`r0=2`), only explicit RAM/DISPCNT writes changed the
  bus, and the external-ROM/digest guard still rejects a wrong image. This is
  local evidence pending independent verification and CI, not a closed issue.

### Display and presentation

- Mode 0 BG0 static snapshot: independent 4bpp palette-bank/transparency and
  8bpp palette tests; horizontal/vertical flips; all four text-map sizes and
  screen-block quadrants; low-nine-bit scroll, horizontal/vertical wrap;
  disabled BG0/backdrop and forced blank; reserved/mosaic/other-layer/mode
  errors; and map/tile accesses beyond the first 64 KiB BG VRAM allocation.
- `crates/core/tests/bus.rs` checks BG0 register widths, write-only scroll reads,
  ignored scroll upper bits, state digests and preserved unsupported-I/O errors.
- Mode 3 complete 240x160 image asserted pixel-by-pixel against RGB555
  conversion. The synthetic guest image is asserted over every pixel.
- Mode 4 palette index and frame page; mode 5 160x128 bounds/backdrop; forced
  blank white; BG2-disabled backdrop.
- Mode 1/2 remain unsupported; unsupported OBJ/BG/window/CGB features are
  rejected. Mode 3/4/5 bitmap behavior remains covered by the existing tests.
- Integer nearest scaling, exact 3:2 letterboxing, invalid sizes, and explicit
  rejection of FSR1/temporal-FSR/ML-FSR/DLSS names.
- PPM header and payload length.

### Hybrid AOT and native proof

`crates/aot/tests/aot_native.rs`:

- Discovers the demo fixture as 4 blocks / 17 instructions (far fewer than ROM
  words; discovery does not scan data).
- Emits source with direct `cpu.set_reg`/`add_flags`/`bus.write16` paths, no
  decoder call inside translated blocks, and **no ROM byte array** (`ROM_BYTES`
  and fixture title bytes are absent). The source stores SHA-256 and length and
  requires `--rom PATH`.
- Builds a generated Rust crate with the installed `cargo` and executes it
  against a separately written synthetic fixture path.
- The generated crate lives in a fresh owned directory. `NativeHarness::build`
  requires a path that does not exist and creates it with `create_dir` plus
  `create_new` file handles; a pre-existing crate directory, `src`, manifest or
  target link is refused rather than reused. `aot proof` similarly creates a
  fresh workspace per run (`output/aot-proof-*` by default) and refuses an
  explicit `--out` that already exists.
- `tests/cli_output.rs` runs the actual `gba-native` binary in an owned temporary
  working directory and covers the former symlink redirects
  (`output/aot-proof/Cargo.toml`, `main.rs`, `src`, `target`), an existing
  `--out`, repeated default proofs, outside/traversal paths, and symlinked or
  dangling paths inside `output/`.
- Compares PC, CPSR/flags, all 16 registers, the full CPU-state digest
  (including inactive banks and SPSRs), VRAM digest, whole-bus digest, full-image
  SHA-256 and exact used instruction budget against the reference interpreter:
  230,411 instructions, 0 fallbacks.
- Runs the harness with the exact bound ROM path (accepted) and with a different
  valid synthetic ROM (rejected with `ROM mismatch`, exit 3).
- Fallback fixture: 5 blocks / 12 instructions discovered, dynamic target left
  undiscovered, 2 runtime interpreter fallback steps, final state/image match.
- Immediate regression fixture: independently generated synthetic ROM using
  problematic immediates (`0xB0`, `0x09`, `0x0B`, `0x0D`). The native report is
  compared against hand-derived register values, final CPSR `0x600000DF`, exact
  budget and a bus digest whose only change is the explicit DISPCNT store.
- PSR/banking fixture (`crates/aot/tests/aot_psr.rs`): a synthetic ROM switches
  SYSTEM -> IRQ -> SYSTEM -> IRQ -> SYSTEM via MSR, writes and re-reads
  SPSR_irq, reads banked SP/LR on both entries and ends in a self-branch. It
  asserts zero fallbacks, a direct-AOT block section with no interpreter call,
  exact hand-derived registers/CPSR/PC/budget, and a CPU digest that includes
  inactive banks. `crates/aot/tests/aot_psr.rs:232` verifies that a CPSR control write
  clears cached r8-r14 BX constants, and `crates/aot/tests/aot_psr.rs:268` verifies that a
  flags-only MSR keeps them.
- Negative-halfword fixture (`crates/aot/tests/aot_halfword.rs`): literal `LDR` initialises
  an EWRAM base and value, `STRH r1,[r0,#-0x20]` is translated directly (the
  generated block contains the `0xFFFFFFE0` displacement and no interpreter
  call), a DISPCNT store initialises mode 3, and `LDRH r4,[r0,#-0x20]` reads the
  value back. The report is compared with the reference and with an
  independently constructed bus that has exactly the negative-offset target and
  the DISPCNT write, not the positive-offset alias; runtime fallbacks are 0.
- Expanded-ALU fixture (`crates/aot/tests/aot_alu.rs`): a synthetic ARM program exercises
  AND/EOR/ORR/BIC/MVN, shifted MOV/ADD, SUBS/ADCS, an ARM `BL` with a
  `BX LR` return, and an MSR-driven IRQ-bank write. The native report must match
  a separately constructed final `Cpu` (including the inactive IRQ stack bank
  and state digest), exact 26-instruction budget, 0 fallbacks, DISPCNT-only bus
  digest and full image digest. A second fixture proves a dynamic `MOV PC,Rs`
  target takes two fresh reference fallbacks while the native block terminates.
  A third fixture checks native `MUL` and `MLA Rd=Rm` aliases against separate
  expected results and the reference policy implementation.
- Thumb-ALU fixture (`crates/aot/tests/aot_thumb_alu.rs`): Thumb move-shifted, AND/ORR/EOR/
  MVN/NEG/SBC/ADD, high-register MOVs and both `BL` halves. The generated block
  section has no interpreter call; the dynamic callee takes exactly three
  reference fallbacks; the known return executes natively. Exact 25-instruction
  budget, independently constructed final CPU digest, bus and image are checked.
  A second emitted harness stops at budget 19, exactly between the two BL halves,
  with the intermediate LR and second-half PC preserved.
- `crates/aot/tests/aot_control.rs` checks discovery metadata without building a harness:
  ARM `BL` follows its callee and queues a return candidate, Thumb `BL` is two
  instructions with a dynamic callee and a queued return candidate, and an ALU
  PC write ends the block without statically following the next word.
- Emitter refuses an artifact bound to a different cartridge and refuses an
  artifact containing an empty block (before any generated loop could run).
- Harness build/run leaves caller-owned sentinel files untouched; the harness
  type exposes no recursive cleanup API.
- Interpreter RAM self-modification is already covered independently by
  `crates/core/tests/cpu.rs::ram_code_is_decoded_fresh_after_self_modification`. The separate
  committed native-harness self-modifying-RAM regression remains open as issue
  #17; this round did not add it.

Discovery unit tests additionally cover: per-block cap splits blocks with
queued continuations (`block_limit_splits`), a zero per-block cap is an explicit
error, a global instruction budget is exact and terminal, and default limits
give no splits. `crates/aot/tests/aot_control.rs` checks the `bl_calls`, `bl_returns`,
`dynamic_bl` and `pc_writes` metadata without following data words; candidate
return continuations are counted there even when they are also external
targets.

A generated crate is written to a fresh owned directory, uses a separate
`CARGO_TARGET_DIR` inside that directory and `cargo --offline`, so it does not
require network access or take the parent Cargo lock. The test or CLI caller
removes only the directory it created.

### Target-profile native build

- `crates/aot/tests/aot_target_build.rs` derives the installed host triple from
  `rustc -vV` (with a `cargo -vV` host fallback) and calls
  `NativeHarness::build_for_target`. The generated harness is built in release
  mode with `--offline --target <host>` and its own target directory, then
  executed against the synthetic fixture with an explicit `--rom PATH`. The
  test checks the same native/reference budget/state differential as the host
  build, requires the external-ROM usage error without arguments, and rejects a
  different valid synthetic image with `ROM mismatch`.
- The same test refuses empty, whitespace-only, path-like and option-like
  targets before creating the crate directory, and refuses a pre-existing crate
  directory while preserving a caller-owned sentinel. `NativeHarness::build`
  (host/debug) remains unchanged.
- `tests/cli_aot_build.rs` runs the real `gba-native aot build` command for the
  installed host triple. It checks the default workspace is a fresh unique
  directory directly under `<cwd>/output/` and that a second default run
  allocates a different workspace while rendering the same deterministic
  metadata facts; it also checks the explicit `--out` rules, `generated.rs`, the
  generated crate, the target-specific release executable, and the ordered
  `artifact.meta`/`build.meta` keys and values. It independently recomputes the
  generated-source and executable SHA-256 values, verifies the ROM is unchanged
  and that no `.gba` copy exists in either workspace, and asserts metadata
  contains no absolute path or timestamp fields.
- Argument, target, missing-ROM, budget, unknown-option, outside/traversal and
  existing-`--out` failures are checked to create no output and start no target
  build. The success case uses the host triple, so Linux CI does not attempt an
  Apple cross-build.
- This is host-toolchain evidence only. The local macOS run exercises
  `aarch64-apple-darwin`; a successful triple build is not a claim that another
  host can cross-compile it, and the produced executable is not a playable game,
  GUI, save-aware app, final application or WASM build.

### macOS ARM native AOT CI gate (configured 2026-10-29)

`.github/workflows/ci.yml` adds a separate `macos-arm-native-aot` job on
`macos-latest` with Rust 1.98.0. It makes the existing native `aot build` path
repeatable on Apple silicon: it prints `rustc -vV`/`cargo -vV`, fails with an
explicit error unless `rustc -vV` reports the host `aarch64-apple-darwin`, and
then runs the two existing synthetic integration targets:

```text
cargo test --offline --locked -p gba-native-aot --test aot_target_build
cargo test --offline --locked -p gba-native --test cli_aot_build
```

- `aot_target_build` builds and executes an `aarch64-apple-darwin` release
  harness against a synthetic ROM, and rejects a different synthetic image.
  `cli_aot_build` invokes `aot build`, then checks the target-specific executable,
  owned workspace, ordered metadata and source/executable SHA-256 values; it does
  not execute the generated executable. Neither target exercises a cross target.
- Scope is synthetic fixtures only: no supplied or retail ROM, no BIOS image, no
  save data. Test commands use `--offline --locked`; generated nested crate
  builds use `--offline`. The hosted job still uses networked checkout/toolchain
  setup. The default workspace dependency set stays empty.
- This is native compilation of ROM-generated code for the host that runs the
  job, not classical host cross-compilation. It does not cover the Apple x86_64
  or iOS/Metal targets, does not change AOT semantics or the WASM branch, and
  establishes no boot, gameplay, runtime-compatibility or further
  platform-support claim. The Linux `gates` job, `scripts/short.sh` and
  `scripts/full.sh` are unchanged.
- The job requires an arm64 `macos-latest` runner. Where the label resolves to an
  Intel host, the host check fails instead of recording a macOS ARM result.
- CI evidence: run `40791284482` completed successfully on 2026-10-29. The
  `macos-arm-native-aot` job passed the host guard and both test commands; the
  Linux `gates` job also passed. The same targets passed locally on the current
  arm64 macOS host (`aarch64-apple-darwin`) with Rust 1.99.1 and CI-pinned Rust
  1.98.0: `aot_target_build` 4 passed and `cli_aot_build` 2 passed on each.

### ROM-bound WASM module build (2026-10-27, local)

The `--target wasm32-unknown-unknown` branch is covered by tool-free library
tests plus one locally exercised synthetic build; the default root tests stay
offline and do not require wasm-bindgen, Node, a browser or the wasm32 target.

- `crates/aot/src/aot/wasm_tests.rs` asserts that the emitted module embeds the
  exact SHA-256, length and build-time budget, contains no ROM byte array marker,
  has no `main`, filesystem, process, environment or stdout boundary, and that
  both binding checks precede cartridge, CPU and bus construction. It also checks
  the ordered WASM metadata keys and hashes, the `artifact_kind=wasm-module`
  label, the exact wasm-bindgen pin in build metadata, rejection of every other
  `wasm32*`/`wasm64*` triple, the exact version-banner rule, and the bootstrap
  instruction for a missing pinned CLI.
- `tests/cli_aot_build.rs` also checks that `--target wasm32-wasip1` fails with
  an explicit message and creates no `output/` directory, and the existing native
  CLI contract still passes unchanged.
- Local synthetic run: the demo fixture written by `aot proof` into an owned
  `output/` workspace was built with the real pinned wasm-bindgen 0.2.129
  (`--target web --no-typescript`) and imported in repository-pinned Node
  24.22.1. `run_rom(Uint8Array)` returned a `status=ok` state/budget/fallback
  report; a single-byte modification and a truncated image both threw the
  SHA-256/length `ROM mismatch` error before guest execution. The produced
  `_bg.wasm` has no WASI import and no demo-fixture byte marker. The module hash,
  JS glue hash and generated README are recorded in the owned workspace.
- Not established: a browser page or GPU presentation, input, saves/persistence,
  a complete frontend, per-ROM workload behavior beyond the synthetic fixture,
  or any retail-game/playability claim.

### Local supplied-ROM execution attempts (2026-10-28)

The owner-supplied ROM was kept external and read-only. The initial native
macOS ARM64 and WASM module attempts stopped at
`RegisterRamReset flags 0x000000FF request unsupported reset bits 5..31`; that
historical failure was superseded when groups 5..7 were modelled.

A pre-Sound-DMA bounded native run advanced past reset and initialization but
stopped with `BIOS access at 0x00000000: direct-entry mode has no BIOS`. That
observation was superseded by the later DMA2 change below. A modified in-memory
copy was rejected as `ROM mismatch` by the WASM binding before guest execution.
No filename, digest or ROM bytes are recorded here.

After bounded DMA1/2 Sound FIFO support was added, the same read-only native
30,000,000-instruction-budget CLI attempt advanced past the former DMA2
zero-address failure and stopped at `unsupported 8-bit I/O read at 0x04000006`.
This records only progress past the earlier failure; no recognizable screen,
boot, gameplay, automatic audio request or compatibility result was established.

The following historical two-phase description was superseded by the 2026-10-29
line-progression round below. The VCOUNT byte-lane change superseded that stop. It widens exactly one existing
modelled register: `0x04000006` and `0x04000007` now return the low and high byte
of the modelled `VCOUNT` halfword (`0` in the active phase, `160` in the modelled
VBlank phase, so the high byte is always `0`). The new path handles only
`VCOUNT`: the pre-existing byte-addressed sound-register reads are unchanged,
every other I/O address keeps the existing register-specific dispatch, and
otherwise-unmodelled reads remain explicit unsupported-width errors, including
the `DISPSTAT` lanes and the byte immediately above `VCOUNT`. The synthetic
regression is
the then-named `vcount_byte_lanes_track_phase_and_reject_selected_other_reads`
(now replaced by `vcount_byte_lanes_track_every_modelled_line`);
it checks both phases, both lanes, the unchanged 16-bit `VCOUNT` value and that
the `DISPSTAT` lanes and the byte above `VCOUNT` still error. This adds no
scanline counter, HBlank, IRQ scheduling, open-bus behavior or frame timing, and
the supplied ROM was not executed for this round and stayed external and
unmodified.

Afterwards `./scripts/full.sh` passed for this task and ended with
`==> full gate passed`: `cargo fmt --check`, Clippy with warnings denied, the
debug and release test suites, the demo, the synthetic native AOT proof
(`native_vs_reference=match`) and the Ratchet slow tier
(`COHERENT — 1 of 1 gates passed`). The pinned workspace inventory is 326
selected package tests and 330 total tests. Independent GPT-6 Luna review rounds
1 and 2 found two medium documentation findings and one low test-comment finding;
all were corrected, and round 3 approved. These results establish only the listed
implementation subset and synthetic proof; they are not a retail compatibility,
hardware timing, boot, rendering or playable-gameplay result. The pushed commit
`a0db5fc` passed main CI, the optional Frontend matrix and the Web foundation
workflow.

After that push, the same 30,000,000-instruction-budget native run was retried
read-only without `--out` and stopped with
`BIOS access at 0x0000000A: direct-entry mode has no BIOS` (exit 1), before
reaching its budget. This records only a BIOS-region access at that address; the
CLI does not distinguish an instruction fetch from a data read. BIOS reads and
code fetch remain unsupported by the direct-entry contract, and no BIOS bytes
or address-specific substitute were introduced. The earlier VCOUNT byte-read
error was not the terminal stop on this attempt. No snapshot was requested or
written; the ROM remained external and unmodified, and no filename, digest or
ROM bytes are recorded. This does not establish BIOS boot, gameplay, rendering,
frame/scanline timing or compatibility.

After the owner-authorized explicit `--synthetic-bios-zero-fill` zero-data
assumption was implemented, a read-only 30,000,000-instruction-budget
`gba-native run` retry with that flag passed the prior BIOS data-read stop but
exited 1 at `unaligned ARM instruction fetch at 0x082F6386`. An external
runtime-state diagnostic reported the failing PC as `0x082F6386`, ARM state
(`thumb=0`) and CPSR `0x00000092` (IRQ mode). No instruction bytes, BIOS bytes,
ROM filename, digest or other bytes are recorded; no `--out` was used and no
snapshot was written. This is progress only under an experimental,
non-hardware-faithful zero-fill assumption, and establishes no boot, rendering,
timing or compatibility. `./scripts/full.sh` passed with 336 workspace tests and
332 selected inventory tests, including the short and long Ratchet tiers, fmt,
clippy, debug/release tests, the demo and the synthetic proof.

For the bounded DMA/FIFO change, `./scripts/full.sh` passed: fmt, clippy, debug
and release tests, demo, native AOT proof (`native_vs_reference=match`) and the
Ratchet slow tier (`COHERENT — 1 of 1 gates passed`). The pinned workspace
inventory is 325 selected package tests and 329 total tests. A fresh OpenAI
GPT-6 Luna 6 review approved the final code after confirming enable-time DAD
latching; its one initial medium finding was fixed and rechecked. These results
establish only the listed implementation subset and synthetic proof, not retail
compatibility or hardware timing.

The macOS x86_64 artifact also built. Windows MSVC did not link because
`link.exe` was unavailable, and Linux x86_64 did not link because a Linux linker
was unavailable; no cross-linker was installed. These attempts establish only
the observed local toolchain outcomes, not general platform support. Automated
regression tests remain based on synthetic fixtures, and no playability claim
is made.

### Output path safety

`src/cli/output.rs` enforces the CLI contract; `tests/cli_output.rs` exercises
it through the real binary in an owned temporary working directory.

- Generated paths are restricted to `<cwd>/output/...`. A relative path is
  resolved against the process working directory and an absolute path is
  accepted only when it resolves below that same `output/` root.
- The location is validated before creating the root or any parent, so a
  rejected command leaves no outside file or directory behind.
- Existing components below `output/` must be real directories. A symlinked
  directory, a symlinked or dangling final component, and a symlinked `output/`
  root are refused without following the link.
- New nested directories under `output/` are created one component at a time
  with `create_dir`; fresh files use `create_new`. Existing regular files
  inside `output/` may be replaced, but nothing outside it is overwritten.
- An explicit `--out` directory for `aot proof` must not exist. The default
  `aot proof` allocates a new unique `output/aot-proof-*` workspace on every
  run and never erases or reuses an earlier result.
- `NativeHarness::build` requires a fresh crate directory and refuses any
  pre-existing directory or link instead of writing through it.
  `NativeHarness::build_for_target` shares the same fresh-directory and
  `create_new` rules, adds `--release --target <triple>`, and does not execute
  the resulting executable.

Unit tests additionally cover a symlinked `output/` root, missing-parent
resolution, and the refusal of an existing fresh-directory target.

## Manual evidence commands

```text
cargo run -- demo
cargo run -- demo --size 480x320 --backend nearest
cargo run -- aot proof
cargo run -- aot build "<user-supplied .gba>" --target <installed-triple>  # local, read-only
cargo run -- info "<user-supplied .gba>"   # read-only, optional
```

The demo writes `output/demo.ppm` (ignored by Git). `aot proof` allocates a new
unique workspace under `output/` (or a new explicit `--out` directory), writes
a synthetic `fixture.gba` and its generated crate there, passes the fixture path
explicitly to the harness, and prints `proof=ok` plus
`native_vs_reference=match`. `aot emit` reports `embedded_rom_bytes=0`.
`aot build` allocates a new unique workspace or a fresh explicit `--out`, writes
`generated.rs`, `artifact.meta` and `build.meta`, and compiles the release
executable for the requested target without executing it. Its metadata contains
no absolute paths, timestamps or ROM bytes.

## Gate evidence

Observed at this implementation round (2026-10-25). These are recorded
observations, not a generated inventory:

- `./scripts/short.sh`: hard limit 0 oversized files, Ratchet fast 2/2 rules
  (`oversized-rust-file` 0 ≤ 0, `rust-source-population` 94 ≥ 36),
  `cargo fmt --check`, Clippy with warnings denied and the fast unit tests all
  passed.
- `./scripts/full.sh`: passed end to end, including debug/release suites, the
  demo and native AOT proof. The synthetic demo still discovers 4 blocks / 17
  translated instructions and executes 230,411 instructions with 0 fallbacks;
  `native_vs_reference=match`, image SHA-256
  `b02968d620ee66d7175c29bbde12bc9ff075290f3e8086b26e9d4829b9ccb154`.
- The workspace inventory pins 267 named package tests: 168 core integration
  tests across 26 files, 32 core library unit tests, 30 AOT integration tests
  across 12 files, 10 AOT library unit tests, 22 root integration tests across
  four files and 5 binary unit tests. The prior 265-test workspace inventory is
  preserved by name and oracle; this AOT SWI round adds six tests. Three
  workspace compatibility tests and one frontend-tree contract test bring the
  full `cargo test` total to 271.
- `cargo test`: 271 tests passed; `cargo test --release`: 271 tests passed.
- Both suites were also repeated with fresh `CARGO_TARGET_DIR` directories, with
  271/271 passing tests in debug and release. This round's full logs are retained
  under `output/fixround-swi-aot-e40418c-run5/` (`short.log`, `full.log`,
  `frontend.log`, `fresh-debug.log` and `fresh-release.log`).
- The optional `apps/desktop` package is not part of that count: its separate
  `scripts/frontend.sh` gate ran 15 frontend unit tests plus locked fmt, Clippy,
  build, a locked offline dependency-tree check and a headless `--help`, and all
  passed. The tree contains `gba-native-core` and not `gba-native-aot` or the
  root facade. None of its dependencies enter the root default graph.
- Generated harnesses built and run by tests: `crates/aot/tests/aot_native.rs` four (demo,
  mid-budget, immediate regression, fallback), `crates/aot/tests/aot_alu.rs` two (expanded
  ALU/banking/BL and dynamic-PC fallback), `crates/aot/tests/aot_thumb_alu.rs` two
  (full run and exact mid-pair stop), `crates/aot/tests/aot_psr.rs` one,
  `crates/aot/tests/aot_halfword.rs` one, `crates/aot/tests/aot_memory.rs` one,
  `crates/aot/tests/aot_block.rs` two, `crates/aot/tests/aot_ram.rs` one,
  `crates/aot/tests/aot_swi.rs` three, `aot_swi_services.rs` one and
  `aot_swi_errors.rs` seven — twenty-five in total. Their
  contract/invalidation tests make no generated build. `crates/aot/tests/aot_native.rs`
  additionally asserts the generated Cargo tree contains only the core package.
  `tests/cli_output.rs` runs two real default `aot proof` builds and refuses all
  redirect/invalid-path cases before Cargo starts.
- Ratchet v0.5.0: `check --tier fast` = 2 rules / 0 failed
  (`oversized-rust-file` 0 ≤ 0, `rust-source-population` 96 ≥ 36);
  `check --tier slow` = 1 rule / 0 failed (`native-consistency` 1 ≥ 1).
  A full `ratchet check` is 3 rules / 0 failed. The machine-written baseline is
  `{native-consistency: 1, oversized-rust-file: 0, rust-source-population: 36}`;
  adding files leaves the floor satisfied without editing the baseline.
- Maintained Rust population: 96 files (including `apps/desktop/src`, the
  workspace/frontend contract tests and the new guest SWI tests); there
  are zero files above 500 physical lines. The hard limit passes without a
  baseline exemption or hand-edited baseline. The earlier
  501-line/nested-directory boundary tests are unchanged in
  `tests/quality_counter.rs` and cover nested `src/output/` and `tests/target/`
  as maintained sources. Release testing emitted non-fatal `rust-objcopy`
  warnings because this local toolchain could not load `libLLVM.dylib`; both
  fresh Cargo test commands exited successfully.
- A separate temporary untracked 501-line Rust probe with a 1-line root `lib.rs`
  in `src/output/` was reported by `oversized --root <temp> --max 500 --hard`
  with exit status 1 and stdout `1`, while root-level `target/` and `output/`
  probes were skipped; the probe was removed afterwards. `scripts/short.sh` also
  passed when invoked from another working directory. Fresh offline/locked
  debug and release runs used separate owned `CARGO_TARGET_DIR` directories and
  each passed all 271 tests.

### Browser foundation integration (2026-10-25)

- The root integration preserves the SWI-era core/AOT inventory and adds one
  clean-checkout bootstrap regression: 268 selected package tests plus four
  workspace/frontend contract tests, 272 total. The exact workspace inventory
  assertion, including the root package ID and all core/AOT targets, passed.
- `scripts/short.sh` and `scripts/full.sh` passed with Ratchet v0.5.0; the hard
  source gate found no oversized Rust files. Maintained Rust population is 106.
  `scripts/full.sh` retained the expected synthetic image digest
  `b02968d620ee66d7175c29bbde12bc9ff075290f3e8086b26e9d4829b9ccb154` and native
  proof (4 blocks, 17 translated instructions, budget 230411, zero fallbacks,
  `native_vs_reference=match`).
- Fresh `cargo test --offline --locked` and
  `cargo test --offline --locked --release` runs each passed all 272 tests with
  separate, new target directories. The integration logs are retained under
  `output/integration-web-unique/` (`short.log`, `full.log`, `frontend.log`,
  `web.log`, `fresh-debug.log`, `fresh-release.log`).
- The optional desktop gate passed its 15 tests. The separate browser gate
  passed its core wasm32 check and WASM release build; it ran 15 Rust library
  tests, one loopback server test, five lifecycle tests and all 15 functional
  browser E2E tests. It used pinned Node 24.22.1 and wasm-bindgen 0.2.129 in
  root-level ignored tool directories; the root lockfile remained at exactly
  three internal packages.
- Browser E2E was run locally on macOS arm64: forced WebGPU reported the browser
  WebGPU backend and forced WebGL2 presented through ANGLE/Metal. The Linux
  SwiftShader GitHub workflow was inspected but not run; no external CI push was
  performed. No screenshots were recaptured, and UI approval remains pending;
  the existing capture inventory is 24 state/viewport combinations and 97 PNGs.
- Release test commands emitted non-fatal `rust-objcopy` warnings because this
  host toolchain cannot load `libLLVM.dylib`; both test processes exited zero.

The historical commit 39380a3 (before the structural split) used `src/aot/`,
`src/decode/`, `src/main.rs` and `src/cli/`. Those old paths are historical
snapshots only; the current source lives in `crates/core/src/`,
`crates/aot/src/`, the root CLI/facade and the root `tests/` files listed above.
The earlier very large line counts were uncommitted working-tree measurements,
not initial HEAD sizes, and are not used as historical evidence.
- Historical bounded, read-only local retail observation (predates this SWI
  implementation; not repeated during this round): the CLI stopped at abstract
  address `0x082FCDE8` on Thumb SWI / reserved conditional-branch (`cond=14/15`)
  after reaching ARM single word stores. No retail ROM was accessed for this
  round. The historical observation is not a current compatibility result; its
  former stop point does not describe the implemented synthetic SWI path.
- The six HLE services and framed guest SWI path are covered only by synthetic
  tests. This is not evidence for a particular retail BIOS, BIOS boot, hardware
  conformance, cycle timing or playability.

### Target-profile build round (2026-10-27, local)

- Toolchain observed locally: `rustc 1.99.1`, host `aarch64-apple-darwin`.
  The target-profile integration test therefore built and ran the selected
  `aarch64-apple-darwin` release executable locally; it did not test
  cross-compilation from another host.
- `cargo test --offline -p gba-native-aot --test aot_target_build`: 4 passed.
  This includes one release `--target` build, its execution against the
  synthetic ROM, wrong-ROM `ROM mismatch`, invalid-target refusal and
  pre-existing-directory refusal, plus acceptance of short valid triples. The
  same suite passed with `--release`.
- `cargo test --offline --test cli_aot_build`: 2 passed. This includes three real
  `aot build` runs (two default, one fresh explicit `--out`), unique workspaces,
  deterministic metadata facts, independent source/executable SHA-256 checks,
  ordered metadata and no-ROM-copy assertions, plus failure paths that create no
  output and launch no build. The same suite passed with `--release`.
- `cargo test --offline -p gba-native-aot --test aot_native`: 5 passed;
  `cargo test --offline --test cli_output`: 5 passed. The existing host/debug
  harness and output-path contracts are unchanged.
- `cargo test --offline --test workspace_contract`: 3 passed with the updated
  inventory of 278 selected package tests and 282 full `cargo test` tests.
- `cargo fmt --check` and `cargo clippy --offline --all-targets -- -D warnings`
  passed. The hard limit reported 0 oversized files, and the maintained Rust
  population was 118.
- This round did not run `scripts/short.sh`, `scripts/full.sh` or CI. Independent
  verification and the repository gates remain the orchestrator's step. No
  retail ROM was used for these tests.

## Not established

No complete hardware conformance ROM suite has passed, and no real GBA hardware
was used. No retail game is supported or known playable. No BIOS boots; only the
bounded synthetic SWI path, behavior-only VBlank IRQ dispatcher frame/return,
and documented CPU-only ALU SPSR return have synthetic-test coverage. IRQ
nesting, FIQ, other interrupt sources, other exception entry/vectoring, save
persistence, hardware audio output or live speaker playback, input interaction,
DMA arbitration and automatic event timing, cycle timing, other BG modes/layers,
OBJ/sprite rendering or a playable frontend are not verified. The static Mode 0 BG0 snapshot is
only a synthetic, documentation-driven capability slice; it is not evidence
that a supplied ROM configures BG0 or reaches a recognizable/booted screen. The
optional synthetic `apps/desktop` frontend presents
one static bitmap and has build/unit-test evidence plus one local macOS Metal
smoke observation. The separate `apps/web` foundation presents that synthetic
fixture through browser WebGPU/WebGL2 and has local build/functional-E2E evidence;
visual approval remains pending. The separate ROM-bound WASM module from
`aot build --target wasm32-unknown-unknown` has synthetic-module and pinned-Node
evidence only; no browser hosting, GPU presentation, input, saves or per-ROM
application around it is verified. Neither frontend is a per-ROM playable/input
loop. No Windows/Linux physical-GPU run, cross-platform presentation claim or
visible-frame counter is verified. PSR-transfer and banking evidence is
limited to the synthetic/direct-entry subset. Guest SWI/SVC and all six HLE
services have synthetic-test evidence only; BIOS execution and hardware
conformance remain unverified. No real GBA hardware
measurement is claimed. Neither FSR nor DLSS nor AI upscaling has been integrated
or tested. REA native analysis remains blocked as recorded.

Low/low Thumb `ADD`, ARMv5 `BLX`/`BXJ`, other exception entry/vectoring, and all
load/store expansion beyond the listed literal/halfword forms remain explicitly
unsupported. Low/low Thumb `MOV`/`CMP` are accepted only as the explicit
**Design** extension described in the contract, not as documented-valid aliases.
The expanded ALU/control subset is not a complete ARMv4T implementation.

The discovery/native proof applies only to the synthetic fixtures. It is not
evidence of general static recovery, broad ARMv4T coverage or retail compatibility.
The native differential compares the emitter with the shared reference
interpreter, so it establishes compiler consistency, not ISA correctness; the
independent golden vectors are ISA-subset evidence for the listed encodings, not
hardware conformance. Tests exercise selected documented examples and boundary
cases; they are not an exhaustive proof of ARMv4T or GBA correctness. Future
compatibility reports must identify ROM digest, BIOS policy, test suite/version,
host, execution budget and observable pass condition, not just a screenshot or a
lack of errors.

## Local hardware-test ROM cross-build and run (2026-10-29)

All seven optional ROM-bound AOT artifacts were built for
`x86_64-apple-darwin` on an Apple-silicon Mac and executed under Rosetta. Each
was verified as a Mach-O x86_64 executable, required the unchanged external ROM
path, and reported `embedded_rom_bytes=0`. The reference snapshots for
`thumb.gba` and `memory.gba` visibly show their ROM-owned “All tests passed”
result; their AOT endpoints and image digests match. `arm.gba` remains blocked at
test 234 on the unpredictable `CMP` encoding, and `bios.gba` remains blocked on
its BIOS-vector read. `hello.gba`, `shades.gba` and `stripes.gba` consume the
5,000,000-instruction budget and are only PPU samples, not PASS results. Exact
per-ROM outcomes and the later SWP follow-up are recorded in
`tests/roms/gba-tests/README.md`. These local results do not establish Emerald
playability or compatibility beyond the named ROM-owned test results.

Artifacts and snapshots are local ignored output only. Vendored ROMs remain
read-only, are not part of `cargo test` or CI, and were neither copied into the
generated artifacts nor embedded. This run is not a hardware-conformance or
playability claim; BIOS execution remains blocked and Issue #10 stays open.

## Behavior-only IRQ dispatcher round (2026-10-29)

- `cargo test --offline -p gba-native-core --test irq_dispatch -p gba-native-aot
  --test aot_irq --test aot_fallback`: 5 core IRQ tests, 1 full IRQ AOT parity
  test and 2 fallback tests passed. The tests use generated synthetic fixtures;
  none reads a retail ROM or BIOS artifact.
- `cargo test --offline --test workspace_contract`: 3 passed with the pinned
  inventory updated to 375 selected package tests / 379 total workspace tests.
- A fresh-target `./scripts/full.sh` completed with `==> full gate passed`:
  hard 500-line gate, Ratchet fast, format, Clippy, debug and release tests,
  demo, synthetic native proof (`native_vs_reference=match`, `proof=ok`) and
  Ratchet slow tier all passed.
- No Emerald ROM was available in this round, so no Emerald run or pass is
  claimed. The IRQ evidence is limited to the synthetic behavior-only contract;
  it does not establish BIOS boot, hardware timing, or retail compatibility.

## Instruction-count VCOUNT progression round (2026-10-29)

- `VCOUNT` no longer reports only `0`/`160`. `crates/core/src/bus/irq.rs` spreads
  the retained 93,632-instruction frame budget over the documented 228 lines:
  410 instructions per line plus one for each of the 152 remainder instructions,
  distributed evenly through `bus::line_instructions` and
  `bus::instructions_until_line`. The 228 line budgets sum to 93,632, a fresh bus
  starts on line 0, the counter wraps from 227 to 0, and `Bus::tick_frames(n)`
  produces the same state as `n` single `Bus::tick_frame` calls.
- The DISPSTAT VBlank flag is derived from lines `160..=226`; line 227 is the
  final hidden line with the flag cleared. The VBlank IRQ request latches once when line 160 is
  entered, which is instruction 65,706 of the frame. This replaces the earlier
  undocumented one-tenth placeholder boundary at instruction 84,269; the
  behavior-only dispatcher frame, handler pointer/return and IME/IE/IF gating are
  unchanged.
- `cargo test --offline -p gba-native-core --lib bus::irq`: 7 passed (line-budget
  sum and remainder distribution, full `0..=227` walk with wrap, pre-boundary/
  160/226/227 flag and request behavior, batch/single-tick equivalence, `VCOUNT`
  byte lanes, DISPSTAT writable bits).
- `cargo test --offline -p gba-native-core --test irq_dispatch -p gba-native-aot
  --test aot_irq --test aot_fallback`: 6 core IRQ tests, 1 full IRQ AOT parity
  test and 2 fallback tests passed. The new core assertion checks that line 159
  raises no VBlank request or delivery and that line 160 makes the request
  deliverable.
- `cargo test --offline` (whole workspace): every suite passed; the pinned
  inventory is 379 selected package tests / 383 total workspace tests (core lib
  41 -> 44, `irq_dispatch` 5 -> 6).
- `cargo fmt --check`, `cargo clippy --offline --all-targets -- -D warnings` and
  the 500-line hard counter (0 oversized files, 144 maintained Rust files) are
  clean.
- `./scripts/full.sh` completed with `==> full gate passed`: hard size gate,
  Ratchet fast, format, Clippy, debug and release test suites, demo, synthetic
  native proof (`native_vs_reference=match`, `proof=ok`) and Ratchet slow tier.
- Limits: line boundaries are instruction-count slices, not hardware scanlines or
  cycles. One instruction is not three cycles, drawing/HBlank periods are not
  modelled, the 152 remainder instructions are spread evenly, no instruction
  observes a mid-line split, and HBlank, VCOUNT-match IRQs, per-line interrupt
  scheduling, DMA synchronization, BIOS execution and BIOS boot remain
  unsupported. GBATEK labels lines 160..227 as hidden scanlines but clears the
  DISPSTAT VBlank flag on 227, which this model follows. No ROM or BIOS file was
  read, executed, copied or modified in this round.

## Instruction-budget timer progression round (2026-10-29)

- `crates/core/src/bus/timer.rs` now advances `TM0..TM3` from the same nominal
  `INSTRUCTION_CYCLES = 3` mapping as the frame model. It implements the
  documented `F/1`, `F/64`, `F/256` and `F/1024` prescalers, `TMxCNT_L`
  reload-versus-counter behavior, reload on overflow and on the `0 -> 1` start
  edge, stop freezing, count-up cascading (including chained overflows), and
  timer overflow requests on `IF` bits 3..6. Timer 0's documented unused
  count-up bit is accepted and ignored, reading back as zero; reserved control
  bits read as zero. RegisterRamReset group 7 still clears the timer state.
- `crates/core/tests/timer_model.rs`: 16 boundary tests cover stopped versus
  running reads, a simultaneous 32-bit reload-plus-start write, every prescaler
  immediately below and across its first increment, overflow reload/request and
  write-one-to-clear acknowledgement, IME/IE gating, count-up prescaler
  independence, chained cascades, cascaded-timer IRQ requests, a disabled
  previous timer, Timer 0's ignored unused control bit with standard 32-bit
  write composition, running prescaler/count-up changes discarding partial
  phase, reserved bits, batch/single-tick equivalence, group-7 reset and
  nominal frame-cycle arithmetic. One test also delivers a timer overflow
  through the existing behavior-only IRQ dispatcher.
- `crates/core/tests/timer_guest.rs`: 2 synthetic guests (ARM and Thumb) start
  Timer 0, poll it into a register and spin; both observe `3` immediately after
  the enable instruction and `45` after the 20-instruction budget.
- `crates/aot/tests/aot_timer.rs`: the same ARM and Thumb fixtures are discovered,
  emitted, compiled and executed natively. Both reports match the independently
  run reference interpreter on PC, registers, CPU/bus digests and image; the
  test asserts `report.fallbacks == 0`.
- Commands and observed results:
  - `cargo test -p gba-native-core --test timer_model --test timer_guest`:
    16 and 2 passed.
  - `cargo test -p gba-native-aot --test aot_timer`: 2 passed.
  - `cargo test` and `cargo test --release`: every suite passed; the pinned
    inventory is 403 listed workspace tests / 399 selected package tests (core
    lib remains 44; new targets `timer_model` 16, `timer_guest` 2 and
    `aot_timer` 2).
  - `cargo fmt --check`, `cargo clippy --offline --all-targets -- -D warnings`
    and the 500-line hard counter (0 oversized files, 147 maintained Rust files)
    are clean.
  - `./scripts/full.sh` completed with `==> full gate passed`: hard size gate,
    Ratchet fast, format, Clippy, debug and release suites, demo, synthetic
    native proof (`native_vs_reference=match`, `proof=ok`) and Ratchet slow tier.
- Limits: timers are driven by instruction counts, not hardware cycles. Reads
  observe only completed instruction ticks; instruction lengths, waitstates,
  bus arbitration and mid-instruction reads are absent. Timers do not consume
  Sound FIFO samples, select or start Sound DMA, request FIFO refills, drive
  `VCOUNT`, HBlank or video capture, and `SOUNDCNT_H` timer-select bits remain
  storage-only. Timer 0's documented unused count-up bit is ignored and reads
  zero rather than being treated as a timing mode. No ROM or BIOS file was read,
  executed, copied or modified in this round.

## Round: documented mask-ROM writes and serial register storage (2026-10-29)

The external test cartridge stopped at `CpuSet` after 161,544 instructions with a
destination range wholly inside the 16 KiB BIOS. On hardware that is mask ROM, so
the store has no effect; the prototype treated it as a fail-closed error. Two
explicit, reported policies replace the blanket rejection without storing,
reading or executing a BIOS byte:

- `BiosWritePolicy::RomWritesDropped`, opted in with
  `run --synthetic-bios-rom-writes`, completes a BIOS store with no effect. The
  default `Strict` policy still reports `BiosAccess` for a bus store and
  `UnsupportedDestination` for an HLE copy destination, and the HLE copy
  services accept a BIOS destination only under the opt-in.
- `SIOCNT`, `RCNT`, `SIODATA32_L/H` and `SIODATA8` are documented R/W storage
  with byte-lane access. A started transfer with no connected partner finishes
  with SI read high and received bits one. Only normal-mode starts with the
  internal shift clock are answered; multiplay, UART, general-purpose/JOY-BUS
  and external-clock starts stay explicit errors. `RegisterRamReset` group 5
  clears the storage and selects general-purpose mode.

`crates/core/tests/mask_rom_and_serial.rs` covers the strict and opt-in BIOS
write paths, the unchanged `RomWrite`/`BiosAccess` boundaries, the serial storage
and byte lanes, the disconnected-transfer result, the UART rejection and the
group-5 reset (4 tests). The workspace inventory pins are updated accordingly
(407 listed / 403 selected).

Observed after the change: the same external run advanced from instruction
161,544 to 886,682. The `CpuSet` calls to a BIOS destination now complete with no
effect, the serial register reads work, and a start request in normal mode with
the **external** shift clock stays pending instead of being fabricated: with no
link partner there are no SC edges, the start bit stays set, and the guest
reaches its own timeout path. Multiplay and UART starts remain explicit errors.

The run then stops at a guest store into the Game Pak ROM window
(`0x080000C8`). That destination appears in a single instruction with no other
register change, so it is a PC-relative literal load from cartridge data.

`RomWritePolicy` adds the same explicit opt-in for ROM-only cartridge stores
(`--synthetic-rom-writes-ignored`); the default stays fail-closed, no cartridge
byte is written, no flash engine is modelled, and the backup region keeps
`UnsupportedSave`. With all three opt-ins the run advances to instruction
896,755 and stops at the guest's backup-device probe (`0x0E005555`).

`crates/core/tests/mask_rom_and_serial.rs` now also covers the ROM-only write
policy (5 tests).

## Round: volatile backup SRAM and V-Counter match IRQ (2026-10-29)

After the ROM-only store opt-in the guest issued the classic flash unlock write
(`0xAA` to `0x0E00_5555`) and stopped on `save hardware ... is not implemented`.
`BackupPolicy::Sram32KVolatile` (`run --synthetic-backup-sram`) answers that
window with a byte-wide, volatile, erased 32 KiB SRAM: the guest's probe sees
plain SRAM bytes and takes its own no-flash path. The model synthesises no flash
device ID, implements no command engine and persists nothing. The SRAM contents
are guest state in `Bus::state_digest`; wider accesses and the rest of the
64 KiB window remain explicit `UnsupportedSave` errors.

With all four opt-ins the run advanced past 20,000,000 instructions and cleared
forced blank (`DISPCNT=0x0000`, `display_forced_blank=0`). A tied observation
window then showed `IE=0x0005`, `IME=1`, `DISPSTAT=0x9628` (LYC=150) and
`IF=0` while the guest waited in a five-instruction loop at `0x080008C6`; an
enabled V-Counter match was never latched. The documented match flag (DISPSTAT
bit 2) and V-Counter request (`IF` bit 2) are now modelled for every line
transition, gated by the DISPSTAT enable bit 5. A write that makes `LYC` match
also requests immediately; that write evaluation is an implementation choice,
because the cited section does not state when a write is evaluated.

A read-only instruction trace of that loop (throwaway observer under `target/`,
never a repository artifact) decodes it as
`ldrh r1,[r2,#28] / adds r0,r3,#0 / ands r0,r1 / cmp r0,#0 / beq` on the IWRAM
halfword at `0x030022DC` with `r3=1`, i.e. a wait on bit 0. The guest sets and
clears that bit from ROM code at `0x080007D4` and `0x080008B8`. The loop exits
at `pc=0x080008D0`; six exits were counted in 500,000 instructions, which is
close to the model's `93632`-instruction frame but not proof of a strict
per-frame cadence. After an exit the guest reads `KEYINPUT` (`0x03FF`, all
released) and calls several subsystems before waiting again. **It is not a
hang.**

`crates/core/src/bus/irq/tests.rs` covers the derived flags, the request at the
matching line, its absence without the enable bit and the immediate request on a
matching write. `mask_rom_and_serial::backup_policy_is_explicit_and_volatile`
covers the backup policy.
Inventory pins: 413 listed / 409 selected, `--lib` 47.

After this change the same run still reports `final_pc=0x080008CE` at
20,000,000 instructions with an unchanged white image
(`image_sha256=232b3c7c...`), so the V-Counter model was **not** sufficient.

A write log over the whole 20,000,000-instruction run records the video state
as follows: the only palette/VRAM/OAM writes are the `RegisterRamReset` clears at
step 19 (512 palette, 49,152 VRAM and 512 OAM halfwords) plus one palette
halfword at step 25, and `DISPCNT` is written exactly twice — `0x0080` at step
19 (the same reset) and `0x0000` at step 1,140,644, from the game's own
`DISPCNT` setter at `0x08001074`. The snapshot report then reads
`display_backdrop_rgb555=0x7FFF`, and the step-25 palette write is the only
palette change after the reset, so that halfword is the backdrop entry that
makes the mode-0 snapshot white; the write log records no
VRAM content beyond that reset clear, no tile or sprite data and no layer
enable. In 50,000,000
instructions `DISPCNT` is never written again. Holding A, Start or A+Start from
boot produces a byte-identical `DISPCNT` timeline, so the guest is not waiting
for input. The video hardware is never initialised beyond that backdrop and no
start screen, frame or compatibility result is claimed. HBlank (IE bit 1,
DISPSTAT bit 4) remains unimplemented.

A second read-only trace narrows the idle state further. The guest's per-frame
code calls the two callback slots at `0x030022C0`/`0x030022C4` exactly once per
frame (`pc=0x0800051C`, reader at `0x0800052A`) and both read zero. The only
non-zero value ever stored there is `0x0816CB15` at step 1,139,600 (setter
`0x08000542`, caller `0x080004EE`); the boot code clears it again at step
1,140,711 after loading `[0x03005D88]`, which is zero, and comparing it to `1`.
The callback target `0x0816CB14` is never executed in 20,000,000 instructions,
and IWRAM `0x030022B4..=0x030022E8` (the suspected main state block) stays zero.
The guest is therefore alive and counting frames (`0x030022E0`/`0x030022E4`
increment once per frame) with no registered frame callback. The word
`0x03005D88` is written zero once by the guest and read exactly once by the
boot check; it is the switch that decides whether the callback survives.

A **counterfactual** experiment (not evidence of correct emulation, and not a
repository change) writes `1` to `0x03005D88` after step 1,140,600. The boot
check then keeps callback2 = `0x0816CB15`, the callback executes, and the run
stops after 1,678 further instructions with
`BIOS HLE: service 0x12 is not implemented`. Service `12h` is the BIOS LZ77
decompression entry point, which is outside the six documented HLE services and
must not be synthesized from BIOS bytes. The next blocking capability is
therefore decompression HLE (and possibly the related Huffman/RL entries), a
scope decision, not a rendering defect. No start screen, frame or compatibility
result is claimed.

## Round: opt-in decompression HLE (2026-10-29)

The previous round's counterfactual stopping point was SWI `12h` after 1,678
instructions. The owner approved extending the HLE with the documented
decompression formats under an explicit host policy (D33). The default remains
`DecompressionPolicy::Strict`, in which `11h/12h/14h/15h/16h/17h/18h` are
explicit `UnsupportedService` errors. `DocumentedFormats`
(`run --synthetic-bios-decompress`) implements LZ77, run-length, 8-bit diff and
16-bit diff from the public GBATEK format descriptions; BitUnPack (`10h`) and
Huffman (`13h`) remain unsupported.

The implementation validates the 4-byte-aligned source, the header's reserved
nibble/type and the destination before the first store. Byte variants accept
only EWRAM/IWRAM; halfword variants accept EWRAM/IWRAM/palette/VRAM/OAM and
require a halfword-aligned destination and an even size. LZ77 `disp=000h` is
rejected for `12h` per GBATEK's documented Vram limitation. Output is rebuilt
in a host buffer, so a decode failure leaves the destination untouched; a
structurally valid stream that simply describes different payload bytes cannot
be detected without a checksum. Synthetic tests are in `crates/core/src/bios/decompress/tests.rs`
(18 vectors including an overlapping back-reference, both halfword routes,
source alignment, policy-digest independence, a source-exhaustion case and a
zero-size unsupported destination) and
`crates/core/tests/bios_decompress.rs` (2 SWI-frame tests); `tests/cli_run.rs`
pins the explicit policy line and assumption. Inventory pins: 434 listed / 430
selected, `--lib` 65, `cli_run` 9, `bios_decompress` 2.

External evidence uses the same read-only ROM (SHA-256
`7c599c56849efeebeb93bd71f714932ae4cdf980db51c9c1016b4431057f71d4`) and the
same **counterfactual** boot-flag write as the previous round; the observer is a
throwaway under `target/`, not a repository artifact. Without the opt-in the run
stops at SWI `12h` after 1,678 instructions. With
`--synthetic-bios-decompress` semantics enabled, the same run passes that call
and continues to step 16,593,186, where it stops on the 32 KiB backup model at
`0x0E008000` (`save hardware ... is not implemented`). A throwaway experiment
that raises the volatile SRAM model to the documented 64 KiB maximum (not a
committed change) advances to step 18,911,571 and then stops on SWI `0Fh`
(`ObjAffineSet`), which remains outside the HLE contract.

The reached state is still not a start screen: `DISPCNT=0x0000` (mode 0, forced
blank clear, no layer enable), palette backdrop `0x0000` and a black mode-0
snapshot (`image_sha256=d8b44303...`) are observed. No frame, boot, gameplay or
compatibility claim is made. The next capability decisions are the backup
device size and `ObjAffineSet`, not a rendering defect. `10h`/`13h` also remain
unimplemented in the decompression family.

## Round: 64 KiB SRAM and opt-in ObjAffineSet (2026-10-29)

With the decompression opt-in the counterfactual run stopped at the 32 KiB
backup-model boundary `0x0E008000`. GBATEK documents the backup window as
`0E000000-0E00FFFF Game Pak SRAM (max 64 KBytes)`. `BackupPolicy::Sram64KVolatile`
(`run --synthetic-backup-sram64k`) now models the full documented window; the
32 KiB policy, its flag and the strict default are unchanged. Selecting both
capacity flags is an argument error.

The same run then reached SWI `0Fh` (`ObjAffineSet`). The first call used source
`0x03007DAC`, destination `0x03007DB4`, count 1, offset 2, scale `(512, 512)`
and angle `0x0000`. `AffinePolicy::Documented` (`run --synthetic-bios-affine`)
implements the documented six-byte source entries, the four destination
parameters at the `offset` stride (`4*offset` between calculations), the upper
eight angle bits and a 256-entry Q12 sine table generated from the mathematical
sine definition. The service is tested by 14 synthetic lib vectors and 2
SWI-frame tests; the 64 KiB policy by one additional mask-ROM/serial test and
three CLI option tests (two for the backup capacity, one for affine). Inventory
pins: 455 listed / 451 selected, `--lib` 80, `cli_run` 9, `cli_run_options` 3,
`bios_affine` 2, `mask_rom_and_serial` 7.

External evidence uses the same read-only ROM (SHA-256
`7c599c56849efeebeb93bd71f714932ae4cdf980db51c9c1016b4431057f71d4`) and the
same **counterfactual** boot-flag write; the observer is a throwaway under
`target/`, not a repository artifact. With the 64 KiB SRAM and affine opt-ins
the run executes repeated `ObjAffineSet` calls and advances to step 19,075,202.
At step 19,000,000 `DISPCNT=0x1F40` (mode 0, BG0-BG3 and OBJ enabled) and the
backdrop is `0x0000`. The run then stops on the unsigned, non-accumulating ARM multiply-long
encoding at `0x082F4D90`, which remains outside the implemented ARMv4T subset. Independently, `display::snapshot` rejects the same state with
`UnsupportedPpuFeature { dispcnt: 0x1F40 }` because the host renderer implements
only Mode 0 BG0, not BG1-BG3 or OBJ. No image, frame or start screen is
claimed. The next blocking capabilities are therefore the ARM multiply-long
instructions and the PPU layers/sprites, not the approved HLE boundaries.

## Round: native title screen through multiply-long, flash and Mode 0/1 PPU (2026-10-29)

The external run stopped at step 19,075,202 on `UMULL r3,r2,r1,r0` at
`0x082F4D90`. The ARMv4 multiply-long family (`UMULL`, `UMLAL`, `SMULL`,
`SMLAL`) is now implemented in the decoder, interpreter and AOT emitter with
raw-encoding vectors, 64-bit N/Z flags, invalid-alias rejection and a native
AOT reference-equality test.

After that, the run reached SWI `0Fh` and then SWI `0Eh`; `BgAffineSet` is now
part of the same `AffinePolicy::Documented` opt-in as `ObjAffineSet`. It writes
the documented 16-byte PA/PB/PC/PD/start-X/start-Y entry from the 20-byte
source and includes zero-scale, identity, quarter-turn and multi-entry vectors.

The boot flag was then diagnosed. Without a flash device the cartridge probe
writes `0` to `0x03005D88`, and the boot check clears callback2. A throwaway
observation that returned the documented flash autoselect ID (`0x62`/`0x13`)
made the probe store `1` **without any counterfactual patch** and start the
intro. `BackupPolicy::Flash128KVolatile` therefore models a volatile 128 KiB
two-bank flash with autoselect, bank switch, program, sector erase and chip
erase. The strict default and both SRAM models are unchanged; the model has no
persistence and makes no flash-compatibility claim. Flash tests cover the ID,
banking, chip erase, byte-only access and strict fail-closed behavior.

The PPU snapshot renderer now composes Mode 0 BG0-BG3 text + OBJ and Mode 1
BG0/BG1 text + BG2 affine + OBJ, with window 0, the OBJ window and
`BLDCNT` alpha/brightness blending. The new synthetic tests cover BG1 text,
Mode 1 BG2 affine, normal and affine OBJ, window clipping and alpha/brightness
mixing. Inventory pins: 481 listed / 477 selected, `--lib` 85, `cli_run` 9,
`cli_run_options` 4, `arm_multiply_long` 6, `display_layers` 8,
`mask_rom_and_serial` 12, `bios_affine` 2, `aot_multiply_long` 1.

The cross-compiled x86_64 CLI was run against the read-only ROM with the
explicit opt-ins `--synthetic-bios-zero-fill`, `--synthetic-bios-rom-writes`,
`--synthetic-rom-writes-ignored`, `--synthetic-backup-flash128k`,
`--synthetic-bios-decompress` and `--synthetic-bios-affine` at a
455,000,000-instruction budget. It completed the budget and wrote a static
snapshot to `output/emerald-native-start-title-20261029/`. The report records
`DISPCNT=0x1741`, `display_forced_blank=0` and
`image_sha256=1f2febbccf5c507fdac2ad8c6bebab3597147d8fffac426299ba90190265af67`.
The PNG visibly shows the `SMARAGD EDITION` logo and `DRÜCKE START` with the
2005 Game Freak notice. This is the observed title/start screen, not gameplay,
input, timing, boot-ROM or general compatibility evidence.

The renderer is still a static subset: 2D OBJ tile mapping, OBJ double-size
affine sprites, OBJ/BG mosaic, window 1, Mode 2 and scanline effects remain
explicit errors or absent. Alpha/brightness arithmetic uses 8-bit channels as a
stated approximation of the 5-bit hardware channels. The CLI run's success
depends on the explicit synthetic device/HLE opt-ins; the ROM itself was never
modified.

## Round: trace-seeded native title-screen run (2026-10-30)

The 2026-10-29 title-screen run executed the reference interpreter
(`interpret::run_budget`). It established the modelled title state, not native
translation. This round repeats the same read-only run with a trace-seeded
native AOT artifact.

A one-time reference trace at the same 455,000,000-instruction budget and the
same explicit opt-ins records `(address, ARM/Thumb state, live raw word)` per
executed instruction. The trace is scratch input under the ignored `target/`
tree and is not committed. ROM seeds are decoded from the immutable cartridge;
a seed word that disagrees with the cartridge is a hard error. Work-RAM seeds
become sealed one-instruction blocks: the generated dispatcher re-reads the
live encoding and calls the native block only on an exact match; any mismatch
uses the reference interpreter with a fresh fetch/decode and increments
`fallbacks`.

The artifact has 18,825 one-instruction blocks: 18,294 ROM blocks and 531
sealed work-RAM variants at 520 addresses (509 stable, 11 with two observed
variants). Six SWI sites use the documented HLE services enabled by the
explicit opt-ins. The artifact binds the ROM by SHA-256+length, contains no ROM
array (`embedded_rom_bytes=0`) and requires a caller-supplied `--rom PATH`.

The generated program was compiled into machine code by the installed
rustc/cargo:

- The cross build `--target x86_64-apple-darwin` on the aarch64 host produced a
  `Mach-O 64-bit executable x86_64` (8.7 MB); `build.meta` records target
  `x86_64-apple-darwin` and host `aarch64-apple-darwin`. The 18,301 out-of-line
  `block_*` symbols that survive inlining implement guest instructions
  directly; `otool -tV` shows the guest `LDRB`/`ADD`/PC-write sequence as native
  loads, address arithmetic, register writes and a budget decrement, not a call
  to the interpreter. The measured run took 56.91 s.
- The host build `--target aarch64-apple-darwin` produced a `Mach-O 64-bit
  executable arm64` that runs directly without a host instruction-translation
  layer. The measured run took 24.55 s.

The native run reports `budget_used=455000000`, `budget_remaining=0`,
`native_instructions=454990284`, `irq_dispatch_returns=9716`, `fallbacks=0`.
The three counters close exactly: 454,990,284 + 9,716 + 0 = 455,000,000. The
trace's summed per-address counts are also 454,990,284, so the run followed the
observed native path. The 9,716 units are the modelled BIOS IRQ-dispatcher
return, counted separately and not as interpreter fallbacks. CPU, bus, VRAM and
image SHA-256 values equal the reference interpreter run
(`image_sha256=1f2febbccf5c507fdac2ad8c6bebab3597147d8fffac426299ba90190265af67`);
the ROM SHA-256 is unchanged.

Runtime canary: a copy of the host-native arm64 harness had `interpret::step`,
`interpret::execute`, `decode_arm` and `decode_thumb` patched to `brk #0` and
was ad-hoc re-signed. It completed the run with the identical report and
hashes, so none of those entries was entered. A positive control patched
`deliver_pending_irq` (called at every block start) to the same trap and exited
on SIGTRAP (133), confirming the patch mechanism. The generated program's only
`interpret::step` call is the out-of-line fallback; the dispatcher never took
it.

Limits: this is trace-seeded, profile-guided translation of the observed path,
not a whole-ROM static translation. Untraced instructions, dynamic RAM code and
any sealed-guard mismatch still use the reference interpreter. BIOS SWI
services and the IRQ dispatcher entry/return are HLE host models, not
translated guest code; no BIOS bytes are fetched. The run depends on explicit
synthetic device/HLE opt-ins and is a static post-budget snapshot, not a frame,
boot, gameplay, timing or compatibility claim.

## Round: synthetic input tester and native visual comparison (2026-10-30)

The committed `tests/roms/synthetic/input-tester.gba` is a 4 KiB Mode 3 guest
written from our own instruction words. It reads active-low KEYINPUT and draws
ten 16-pixel bands in button order; a held button makes its band white. The
source function, committed bytes, input script and expected PPM are pinned:

- ROM SHA-256 `8a7667d43d9ec0333a5a7c0a032837919e101ca1c1893bdf5f95f51b27555539`
- expected PPM SHA-256 `2824c8e9f509c1294c555af3318cec4d3fd4a37aa964f60e9ecc974721da941a`
- expected RGB888 frame SHA-256 `5cbbd998a2bc5a026ebac07d91a0f058b769cb05dd14c09e30d52888ed80db8b`
- script `0:A+B+Start:20000000`, budget 20,000,000 instructions

The committed source is checked by
`crates/core/tests/input_tester_fixture.rs`, which regenerates the bytes, checks
both digests, runs the reference interpreter with the committed script and
compares the frame with the independent expected-image function. The default
`cargo test` gate stays offline; no browser, Node or retail ROM is required.

`scripts/input-tester.sh` built the same fixture through native AOT for the host
(`aarch64-apple-darwin`) and ran the generated executable with the committed
script. The report recorded `budget_used=20000000`, `fallbacks=0`,
`image_sha256=5cbb…db8b`, and an actual PPM byte-identical to the committed
expected frame. The same gate runs `--audio` at volume 80 and requires
`audio_audible=true` plus a non-empty WAV. With `--visual`, the pinned Chromium
rendered expected and actual frames and reported `visual_compared=38400`,
`visual_differences=0`, `visual_pixels_match=true`; the screenshot is kept in
the owned `output/input-tester.*` workspace. The Linux CI job runs the same
script on `ubuntu-latest` (the native target is its host triple), with the same
digests and visual comparison.

Performance report from that run (20,000,000 guest instructions):
`perf_guest_instructions_per_second≈31.9M`, `perf_realtime_factor≈1.90`
against the documented 16.777216 MHz clock, with the explicit caveat that one
retired instruction is not one hardware cycle. This is a synthetic fixture
measurement, not frame timing or a game-performance claim.

Limits: the fixture exercises our own input path, not a retail game. It does not
establish hardware conformance, audio, saves, link play, a playable emulator or
a retail compatibility level.

## Round: cross-target build matrix (2026-10-30)

`scripts/cross.sh` built the synthetic input-tester artifact for:

| Target | Result |
| --- | --- |
| `aarch64-apple-darwin` | Mach-O arm64; run with frame match and `audio_audible=true` |
| `x86_64-apple-darwin` | Mach-O x86_64; built (host is arm64) |
| `x86_64-unknown-linux-gnu` | ELF 64-bit x86-64 PIE; built with `zig cc` |
| `x86_64-pc-windows-gnu` | PE32+ x86-64 console executable; built with mingw-w64 |
| `wasm32-unknown-unknown` | 163,537-byte module; built with pinned wasm-bindgen 0.2.129 |

The Linux artifact is build evidence on a macOS host; execution is covered by
the Linux CI job. Windows MSVC, iOS and Android are not claimed; they need their
own linker/SDK or packaging work (#11). A successful cross-build is not a
platform-support, GPU, input or playability claim.

## Round: local retail name-entry regression (2026-10-30)

`crates/core/tests/local_name_entry.rs` is an ignored, environment-gated
integration test for a user-supplied read-only ROM. It drives the reference
runner from the title screen through the new-game intro, navigates the on-screen
name keyboard, types characters, confirms with START and checks the confirmation
dialog. The ROM path is supplied via `GBA_NATIVE_LOCAL_ROM`; the test never
copies, commits or embeds ROM bytes. It is not part of `cargo test` or CI.

The observed flow used START at instruction 460,000,000, periodic A presses
through the intro, directional navigation on the name keyboard and START to
confirm. The local frame and PC/DISPCNT values are a renderer-build golden, not
a hardware or retail compatibility claim. Evidence is written under
`output/local-name-entry-*` when the test is run locally.

The local release run passed on 2026-10-30 with `pc=0x080008C8`,
`dispcnt=0x1340` and image SHA-256
`558ae1e3c9fb682f2031525cc5783c2445e3ca47ae0b39bef6f5b3255c1785f8`; the
evidence directory is `output/local-name-entry-31759-*`. The test requires
`GBA_NATIVE_LOCAL_ROM`, is `#[ignore]`d and is never part of CI.

## Round: native audio generation, first slice (2026-10-30)

`crates/core/src/audio.rs` is a std-only, dependency-free PSG mixer. It renders
two square channels, the wave channel and the noise channel from the documented
sound-register image, applies a 0..=100 master volume or digital mute, and can
emit a canonical RIFF/WAVE file. `AudioEngine` advances once per guest
instruction (`advance_instruction`), so a register write changes the very next
generated sample rather than a later chunk. This is generation from guest state,
not a pre-recorded track.

Evidence:

- `cargo test -p gba-native-core --lib audio`: 11 mixer/engine/WAV tests pass,
  including a 440 Hz square sign-crossing count, mute silence, volume scaling,
  disabled-master silence, a stateful engine advance, and two tests proving that
  master-disable and volume writes change the amplitude at the next sample (no
  1024-instruction quantization).
- `gba-native audio --out output/audio-demo/tone.wav --frequency 440
  --seconds 0.5 --volume 80` wrote 16,384 mono samples at 32,768 Hz
  (32,812-byte WAV), `audible=true`, WAV SHA-256
  `004fa5dde7355bc14b16e20a8f0aec6e9be60babea69a74cc87747b0f35fbf57`.
- The same command with `--mute` reported `audible=false` and a different WAV
  digest (`26babbf888706edddea4164b24cd61e70ca922ae830d7abcd2b45ed8b8c4c7b4`).
  `tests/cli_audio.rs` additionally asserts that every muted data sample is
  zero and that volume 20 differs from volume 80.

The generated AOT harness accepts `--audio PATH`, `--volume 0..100` and
`--mute` and writes WAV from the run's sound-register state; this is the
cross-target guest-audio artifact path. The browser frontend persists the same
volume/mute fields and can play a fixed 440 Hz WebAudio test tone, but it does
not yet stream guest PCM; that sink remains outstanding in issue #29. The
persisted settings document now carries `master_volume` and `muted`.

The committed input-tester guest now programs a 440 Hz square at boot. A
2,000,000-instruction reference run with `--audio-out` reported
`audio_samples=11718`, `audio_master_volume=80`, `audio_muted=false`,
`audio_audible=true` and WAV SHA-256
`7804561066d21ab13e5531e7f4af2b7be44ac1522ebfdf3aa22b8de5e62dacee`. The
native AOT harness for `aarch64-apple-darwin` (2,000,000-instruction build,
`--audio`, volume 80) reported the same `audio_samples=11718`,
`audio_audible=true`, `fallbacks=0` and WAV SHA-256
`7804561066d21ab13e5531e7f4af2b7be44ac1522ebfdf3aa22b8de5e62dacee`. The frame
still matched `5cbbd998…80b8b`. This is guest-state-driven audio from our own
fixture, not a retail soundtrack claim.

Limits at the time of this first slice: the mixer had no Direct Sound FIFO
sample clock (added in the round below), no complete
sweep/envelope/length frame sequencer and no cycle-accurate resampling. Live
native speaker output was not wired then; the later playback round adds
post-run `--play` through an installed platform player. The WAV artifact
remains the portable output path. Full audio is tracked in issue #29 and is not
a retail-audio, hardware-timing or BIOS-audio claim.

## Round: host save library and synthetic save-tester (2026-10-31)

The host save library is `core::save::SaveLibrary`: raw backup images live under
`<root>/<rom-sha256>/<slot>.sav` with a `default` marker. Slot names are
path-safe (1..=32 characters, no leading dot, no separators). `Bus::backup_image`
and `Bus::load_backup` move the selected device image in and out. The CLI adds
`run --save-dir/--save-slot` and
`save list|import|export|use|default|delete`.

Synthetic evidence, all from our own 4 KiB `tests/roms/synthetic/save-tester.gba`
(ROM SHA-256 `f8dbe41f4dcd1e134759a23cce988b4a05f1185f102b3153d69240600e76c4e3`):

- fresh frame `53ff58d4cb667ee14a36ec6d6723fbfa623a7ddc489c4a11551254cdeaad8755`,
  32 KiB SRAM image SHA-256
  `39640902e8ceb5ebcce5769a9d891df7e0c3f1a9d8b0c3b0c175a34f3a823dca`;
- loaded frame `259d7e7302a5d04611c276395f3f9a5901e7c65061b4fb0e6b9b6f786905434c`,
  image SHA-256
  `bc470afabfeeece7e51c912dabc4178e507fd6535e1402b36370ddb4f6f1e938`.

`crates/core/tests/save_tester_fixture.rs` checks the committed ROM, both
frames, multiple slots and default selection. `tests/cli_save.rs` runs the CLI
twice into one slot and observes the fresh then loaded digest, then imports,
exports, uses and deletes additional slots. `scripts/save-tester.sh` builds the
native AOT harness, verifies the fresh and loaded frames against the same raw
`--save` image, checks a second independent image, and repeats the multi-slot,
default and import/export flow through the CLI. The Linux CI job runs it.

The browser save panel uses `apps/web/www/saves.mjs`: base64 raw images in
`localStorage`, named slots, default selection, import/export and delete.
`apps/web/tests/js/saves.test.mjs` covers the storage model; the Playwright test
imports two synthetic 32 KiB images, selects the default, downloads the default
image, reloads the page and checks persistence. The panel is host-side
management of synthetic raw images; the page does not execute guest saves.

Local retail proof: `crates/core/tests/local_emerald_save.rs` is ignored and
environment-gated (`GBA_NATIVE_LOCAL_ROM`), reads the retail ROM read-only, drives
the observed intro/name/truck/menu flow (`DOWN` to leave the truck, `START`,
menu navigation, `B` to close the bag, then `SICHERN`), persists the flash image
through `SaveLibrary` and reloads it. A 7,600,000,000-instruction run produced a
131,072-byte flash image with `save_non_erased_bytes=56331` and SHA-256
`7a71d40ac8a14200f78a3115b1c4017843a47d368caee870226030ed6de1dc16`
(`pc=0x08007B64`). Evidence is written only under
`output/local-emerald-save-12910-1793443071481114000`; the ROM is read-only. The test asserts a
non-erased image, reloads the exact bytes, and is never part of CI. This is a
local observation, not a retail-save compatibility or distribution claim.

Limits: this is persistence for the explicitly selected SRAM/flash models, not
save-device compatibility. No retail save data, ROM or BIOS bytes are committed.

## Round: Direct Sound FIFO audio and cross-target PCM (2026-10-31)

`core::audio` now mixes Direct Sound A/B on top of the PSG.
`AudioEngine::advance_instruction_bus` (used by the reference scripted runner
and generated AOT dispatchers) reads the SOUNDCNT_H volume/enable and
timer-select fields, consumes one signed 8-bit FIFO byte per timer-0/1 overflow
at `16.777216 MHz / (prescaler * (65536 - reload))`, and requests the 16-byte
sound-timed DMA burst once a FIFO is at most half full. A drained FIFO holds the
last sample; a malformed sound DMA returns an explicit error. The register-only
`advance_instruction` path stays PSG-only for tests without a bus. The renderer
is mono at the fixed 32,768 Hz host rate; stereo panning, SOUNDBIAS PWM and
hardware cycle timing are not modeled.

Evidence:

- `cargo test -p gba-native-core --lib audio`: 17 tests pass, covering timer
  rate (reload, prescaler, stop), FIFO consumption at the timer rate, a
  half-full DMA refill from EWRAM, underrun hold, SOUNDCNT_H enable/volume and a
  same-boundary Sound write.
- `cargo test -p gba-native-aot wasm` passes; the emitted module now exports
  `run_rom_audio` (mono `Int16Array`) and `audio_sample_rate` beside the existing
  `run_rom` report. Both entry points perform the same length/SHA-256 binding
  check before constructing any guest state.
- Synthetic input-tester parity at 2,000,000 instructions and volume 80: the
  native AOT harness and the wasm32 `run_rom_audio` export both produced 11,718
  samples at 32,768 Hz, `audio_audible=true`, `fallbacks=0`, and the identical
  WAV SHA-256
  `7804561066d21ab13e5531e7f4af2b7be44ac1522ebfdf3aa22b8de5e62dacee`.
  This fixture programs PSG only, so the Direct Sound path leaves its PCM
  byte-identical; it also matches the pre-Direct-Sound digest.
- Emerald-scale wasm check: `aot build --target wasm32-unknown-unknown` with the
  same explicit device policies as the native run (synthetic BIOS zero fill and
  ROM-write handling, dropped ROM writes, volatile 128 KiB flash, documented
  decompression and affine services) produced a module with no embedded ROM
  bytes. At 20,000,000 instructions it returned 117,187 samples at 32,768 Hz,
  `audio_audible=true`, peak 1628, RMS 106.798, and WAV SHA-256
  `b9c1e64ef8b83030590fa9587e8b08818151d592c3f87c5fd8a106f462747f16` — byte for
  byte identical to the native reference runner at the same budget, policies and
  volume 80. The ROM was read only at call time and never copied or embedded.
- `./scripts/input-tester.sh` passes end to end (20,000,000 instructions,
  `fallbacks=0`, committed frame byte match, `audio_audible=true`, WAV SHA-256
  `20f67766404d55d2e35eb7cd3ab8b0b77cc1bbf8cbbd7e50f14a86577a3a1448`) and now
  also asserts `audio_direct_sound_samples=0`, `audio_direct_sound_underruns=0`
  and `audio_direct_sound_dma_refills=0` for this PSG-only guest.
- Emerald, local read-only ROM, 600,000,000 instructions: 3,515,625 samples,
  `audio_direct_sound_samples=2870052`, `audio_direct_sound_dma_refills=179382`,
  `audio_direct_sound_underruns=6`, `audio_audible=true`, range
  `-18592..14476`, RMS 2772.69, WAV SHA-256
  `c00a81a1af723e2dc920af5f2d4d0b254126e2c373e7c95befe7e932e26dfaa4`
  (`pc=0x080069E4`). Per-second RMS rises from 0 to about 634..2347 over the
  first twelve seconds; the last ten seconds contain 24,047 sign changes, so the
  capture is time-varying audio rather than a fixed test tone. The ROM is never
  copied, embedded or committed.
- `./scripts/cross.sh` built the audio-enabled harness for `aarch64-apple-darwin`
  (Mach-O arm64, 651,184 bytes, host run `audio=audible` with the expected frame
  `5cbbd998…80b8b`), `x86_64-apple-darwin` (Mach-O x86_64), `x86_64-unknown-linux-gnu`
  (ELF x86-64), `x86_64-pc-windows-gnu` (PE32+) and `wasm32-unknown-unknown`
  (168,656-byte module). The `x86_64-apple-darwin` artifact was also executed
  under Rosetta on the arm64 host: expected frame match, 11,718 samples,
  `audio_audible=true` and WAV SHA-256
  `7804561066d21ab13e5531e7f4af2b7be44ac1522ebfdf3aa22b8de5e62dacee`, the same
  PCM as the arm64 harness and the wasm module. CI separately executes the same
  synthetic audio gate natively on Linux and Windows runners, so no supported
  native build target remains execution-unverified.

Playback evidence:

- `run --audio-out output/play-smoke/tone.wav --play` on macOS invoked
  `/usr/bin/afplay` and reported `audio_played=true` after the synthetic
  input-tester WAV (`78045610…dacee`).
- A 20,000,000-instruction Emerald run with `--play` reported the Direct Sound
  counters above (`94932` samples, `5936` refills, `6` underruns), WAV SHA-256
  `b9c1e64e…f16`, `audio_player=/usr/bin/afplay` and `audio_played=true`.
- The generated native AOT harness has the same `--play` contract: a synthetic
  input-tester run reported `audio_player=/usr/bin/afplay`,
  `audio_played=true` and the expected PCM digest, while an explicit
  `GBA_NATIVE_AUDIO_PLAYER=/nonexistent` failed with
  `cannot run audio player /nonexistent`.
- `tests/cli_play.rs` covers explicit player invocation, `--play` without
  `--audio-out`, and a missing override failing closed. Unit tests cover PATH
  search, override precedence, empty overrides, player exit failure and the
  no-player diagnostic. Linux/Windows use `paplay`/`aplay` and PowerShell
  `SoundPlayer`; these are detected, not guessed, and are not installed by the
  emulator.
- `scripts/play-tester.sh` compiles a fake platform player, puts it first on
  `PATH`, and requires both the CLI and the generated AOT harness to report
  `audio_played=true`, name the fake player and hand it the generated WAV. A
  nonzero player exit must fail with `failed with`. Linux and Windows CI run
  this gate, so platform player detection/arguments are executed, not only
  source-checked.
- The browser E2E suite (22 Playwright tests) exercises
  `__gbaNative.audio.playPcm(Int16Array, 32768)`, which schedules mono signed
  16-bit PCM of the same shape returned by the wasm `run_rom_audio` export.
  Browser *guest* playback is explicitly out of scope for the app: `apps/web`
  does not load the AOT module or execute ROMs. The module returns PCM and
  `playPcm` accepts it; wiring a module instance and its pacing into a page is
  a caller integration, documented rather than claimed.

Limits: this is synthetic/retail-ROM audio generation evidence, not a hardware
audio-conformance claim. `--play` is post-run playback of a generated WAV, not a
streaming speaker sink, and it depends on an installed platform player. CI
proves invocation/failure semantics with a fake player; it does not prove
physical sound output on a runner without an audio device. The browser
application does not execute guest ROMs and guest playback there is out of
scope; only the separately built wasm execution module returns guest PCM, and
the caller owns WebAudio scheduling. Direct Sound timing derives from the
instruction-budget timer model, so it is an approximation, not cycle timing.

### Windows host execution

The `windows-input-tester` CI job runs `scripts/input-tester.sh` on
`windows-latest` (host triple `x86_64-pc-windows-msvc`). It regenerates the
committed synthetic fixture, builds the audio-enabled native AOT harness, runs
it with the committed input script and `--audio`, checks the committed frame
and the `audio_audible=true`/volume/mute lines, and asserts the PSG-only fixture
consumes no Direct Sound samples. This is Windows host execution of the same
requested audio path, not a retail-ROM or compatibility claim; the synthetic
fixture is committed, so no retail ROM, BIOS or save data enters CI.

The first Windows run passed on `windows-latest` with target
`x86_64-pc-windows-msvc`, a byte-identical expected frame, and
`audio_wav_sha256=20f67766404d55d2e35eb7cd3ab8b0b77cc1bbf8cbbd7e50f14a86577a3a1448`
— the same PCM digest as Linux. `.gitattributes` pins the expected PPM as
non-text, the input script to LF and the synthetic ROM as binary so Windows
`autocrlf` cannot disturb the byte-pinned fixtures.

## Round: Android native compilation (2026-10-31)

`scripts/android.sh` cross-builds the default three-package workspace
(`gba-native-core`, `gba-native-aot`, root `gba-native`) for the Android NDK
ABIs and verifies every artifact. On this host (macOS arm64, NDK
`29.0.14206865`, Rust targets installed with `rustup target add`):

```text
abi=aarch64-linux-android
profile=debug
api=21
linker=.../ndk/29.0.14206865/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android21-clang
artifact=target/aarch64-linux-android/debug/gba-native
elf_machine=AArch64
elf_interpreter=/system/bin/linker64

abi=armv7-linux-androideabi
elf_machine=ARM
elf_interpreter=/system/bin/linker

abi=x86_64-linux-android
elf_machine=Advanced Micro Devices X86-64
elf_interpreter=/system/bin/linker64

android build ok: 3 ABI(s)
```

`--release --abi aarch64-linux-android` produced the same ELF machine and
interpreter in `target/aarch64-linux-android/release/gba-native`.

Failure paths are explicit, not silent skips:

```text
$ ANDROID_NDK_HOME=/nonexistent ./scripts/android.sh
error: ANDROID_NDK_HOME=/nonexistent is not a directory
$ ./scripts/android.sh --abi mips-linux-android
error: unsupported Android ABI target: mips-linux-android
```

`tests/scripts.rs` covers a missing NDK and missing Rust targets with a fake
NDK tree and a fake `rustup`, so the diagnostics are contractual.

Limits: this is compilation and link evidence for the std-only workspace. No
Android app, JNI surface, device or emulator execution, audio sink or
compatibility claim is made. Android playback remains future work; `--play`
still requires an installed platform player and fails closed.

## Round: Android app and shared frontend parity (2026-10-31)

The window-free frontend moved into `crates/core/src/frontend` and the desktop
app now re-exports it, so the desktop and Android targets run the same image
pipeline, viewport math, shell/fullscreen/settings screens, settings file and
tone artifact. Core tests cover the shared behaviour:

```text
frontend:: image 4 + viewport 4 + settings 3 + tone 3 + ui 5 = 19 tests
cargo test -p gba-native-core --lib -> 157 passed
```

The Android app is a separate package with its own lockfile. Host unit tests
never need a device:

```text
$ cargo test --locked --manifest-path apps/android/Cargo.toml
test result: ok. 9 passed
```

They assert that the app's demo frame digest and bytes equal
`frontend::demo_image()`, that shell/fullscreen composition equals the shared
`frontend::ui` screens, that every touch control is reachable and released,
that settings taps change and persist volume/mute/fullscreen/touch state
through the shared settings file, and that the tone artifact is byte-identical
to `frontend::tone::tone_wav`.

`scripts/android-app.sh` builds the release library for all three ABIs, links a
base APK with `aapt2`, adds `lib/<abi>/libgba_native_android.so`, signs it with
`apksigner` and a generated debug key, and verifies the package:

```text
apk=output/android-app/gba-native-android.apk
package=dev.gbanative.synthetic
activity=android.app.NativeActivity
profile=release
native-code=arm64-v8a armeabi-v7a x86_64
signature=verified
Signer #1 certificate SHA-256 digest: 1d8c02968c1efd7c17abb40fc92ea0db1b95372e5921d9b6e0457392a4aaadb5
android app build ok
```

The script also rejects an APK that lacks the launchable activity, misses one
of the three libraries, or contains a `.gba`/`.sav`/`.bin`/asset payload.

Limits: this is a signed debug APK and host unit tests, not device or emulator
execution, not a playability or Android compatibility claim. The app composes
only the committed synthetic demo snapshot and never loads a ROM, BIOS or save.
Audio remains the shared tone WAV artifact; there is no live Android sink,
matching the desktop artifact path.
