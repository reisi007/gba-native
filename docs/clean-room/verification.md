# Verification boundary

Status: experimental; no retail game is playable, no GPU vendor integration
exists, no successful REA Evidence was obtained, and this is not a formal
separate-team legal clean-room process.

Run `./scripts/short.sh` for the fast gate and `./scripts/full.sh` for the
complete gate. They invoke `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`,
`cargo test --release`, the demo and the real native AOT proof. Ratchet is
pinned to release v0.5.0 of `all-the-rest/ratchet-cli` (peeled commit
`99c25b1e18331a1473d43091d37e23f23bc1c30d`; the annotated tag object is
`1feb7b5fae653272a922ed9f0479660ecde22ed2`); a missing or mismatched tool fails
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
  instruction-budget counter model. Sound FIFO state is bounded and host-driven;
  no audio, timer-triggered sample consumption or automatic FIFO DMA is
  generated.
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
persistence, audio output, input interaction, DMA arbitration and automatic
event timing, cycle timing, other BG modes/layers, OBJ/sprite rendering or a
playable frontend are not verified. The static Mode 0 BG0 snapshot is
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
policy (5 tests). Inventory pins: 408 listed / 404 selected.

`DISPCNT` was `0x0080` (forced blank) at every stop, so no start screen, frame or
compatibility result was observed and none is claimed. The Game Pak window keeps
its fail-closed error because writes there depend on an unmodelled flash command
engine.
