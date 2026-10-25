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

- Exact ends of EWRAM/IWRAM/palette/VRAM/OAM; out-of-region addresses error.
- Three ROM windows alias the same bytes; ROM writes and overreads error.
- BIOS, save, unknown I/O, unimplemented I/O, bad widths and unaligned accesses
  return explicit errors. The last I/O byte `0x040003FF` reports
  `UnsupportedIoRead`/`UnsupportedIoWrite`, not `Unmapped`.
- Palette/VRAM byte-store duplication and OAM byte-store ignore behavior.
- DISPCNT/KEYINPUT only; KEYINPUT reports all buttons released.
- ARM/Thumb ALU, NZCV for carry/borrow/overflow, condition-false no-op, PC
  advance, direct branches, `BL` link/return, BX ARM↔Thumb round trip, PC
  operand pipeline values, and aligned ALU PC writes.
- Full ARM data-processing family, LSL/LSR/ASR/ROR/RRX by immediate and by
  register (low-byte amounts, 0/32/>32/256 cases), ADC/SBC/RSC carry ordering,
  logical V/C preservation, and explicit `S=1` PC rejection. See
  `tests/arm_alu.rs`, `tests/arm_shift.rs` and `tests/arm_control.rs`.
- Thumb move-shifted, the other fifteen register ALU opcodes, high-register
  ADD/CMP/MOV (low/low MOV/CMP only as the contract's **Design** extension),
  ADR/PC/SP adjustment, and both `BL` halves with an exact mid-pair budget
  stop. MUL, low/low ADD and ARMv5 BLX stay explicit errors. See
  `tests/thumb_alu.rs` and `tests/thumb_control.rs`.
- ARM7 odd-address LDRH rotation, STRH forced alignment, unaligned literal LDR
  rotation.
- ARM halfword classification and signed offsets (`tests/arm_halfword.rs`):
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
  `tests/psr_transfer.rs`, `tests/psr_field_mask.rs` and
  `tests/psr_banking.rs`.
- Register banking: all seven modes, shared r0-r7, FIQ-only r8-r12, per-mode
  r13/r14 with USER/SYSTEM sharing, independent exception SPSRs, and state
  digests that include inactive banks.
- Rejected encodings (multiply/swap, block transfers, `S=1` Rd=PC,
  malformed PSR fixed fields, ARMv5 BLX/BXJ, Thumb MUL/PUSH/BLX) report the raw
  word/state and leave PC unchanged.
- Unaligned/nonexecutable fetches fail explicitly.
- Unknown backend names are preserved in `UnsupportedBackend` (`--backend foo`
  reports `foo`, never `"unknown"`).

### BIOS HLE primitives

`src/bios/` implements six documented, timing-independent services as direct
Rust primitives: `RegisterRamReset` (`01h`), `Div` (`06h`), `DivArm` (`07h`),
`Sqrt` (`08h`), `CpuSet` (`0Bh`) and `CpuFastSet` (`0Ch`). They are called only
through `bios::call`; there is no guest SWI/SVC decode, instruction-budget
consumption, PC update, exception entry, SPSR write or cycle/wait-state model,
and every other service number is an explicit error. Exact policies are in
`hle-services.md`.

- `tests/bios_arithmetic.rs`: the documented division example, a signed
  division matrix, zero-divisor and `i32::MIN / -1` errors before mutation, and
  an integer-square-root property.
- `tests/bios_memory_copy.rs`: the literal `CpuFastSet` rounding oracle for
  requested counts `0, 1, 7, 8, 9, 15, 16, 17, 23, 24, 25` with independently
  stated expected word counts `0, 8, 8, 8, 16, 16, 16, 24, 24, 24, 32`, a
  sentinel word at every rounded end, fill source-read counts,
  ascending-overlap smear, and range/alignment/reserved-bit failures before
  mutation. The table is not computed from the production rounding formula.
- `tests/bios_reset.rs` and `tests/bios_dispatch.rs`: IWRAM-tail preservation,
  unconditional forced blank, and explicit `UnsupportedService`/unsupported
  reset-flag results that mutate nothing.

These tests use synthetic RAM/ROM fixtures only and require no BIOS image,
commercial ROM, save, network or external crate. This is not evidence for guest
SWI/SVC dispatch, BIOS boot, arbitrary requests, cycle timing or playability.

### Independent ISA golden vectors

`tests/isa_golden.rs` is written from the documented encoding and arithmetic
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
- `tests/arm_shift.rs`: independent value/carry matrix for every shift kind and
  0/1/31/32/33/63/64/255/256 register amounts, immediate specials, RRX both
  signs, and explicit Rs=15 rejection.
- `tests/arm_alu.rs`: all sixteen opcodes in register and immediate form,
  ADC/SBC/RSC carry-in and overflow edges, RRX-before-flags ordering, logical
  shifter-carry/V preservation, S=0 preservation, and encoding restrictions.
- `tests/arm_control.rs`: ARM PC+8/+12 operands, aligned S=0 PC writes,
  S=1 PC errors, B/BL targets and link register, conditional false LR
  preservation, BX state exchange and BX PC behavior.
- `tests/thumb_alu.rs`: all fifteen non-MUL register ALU forms, both shift
  groups, low-byte register amounts, high-register flags/PC rules, ADR/SP
  bases, low/low MOV/CMP, and reserved/BLX errors.
- `tests/thumb_control.rs`: the two BL halves, negative high offset,
  second-half-only wrap, and the exact one-instruction budget stop between
  halves.
- `tests/aot_control.rs`: discovery metadata for the ARM BL callee and its
  queued return candidate, the Thumb BL dynamic callee and queued return
  candidate, and ALU PC-write block termination.

### Display and presentation

- Mode 3 complete 240x160 image asserted pixel-by-pixel against RGB555
  conversion. The synthetic guest image is asserted over every pixel.
- Mode 4 palette index and frame page; mode 5 160x128 bounds/backdrop; forced
  blank white; BG2-disabled backdrop.
- Unsupported display mode and OBJ/BG/window layers rejected.
- Integer nearest scaling, exact 3:2 letterboxing, invalid sizes, and explicit
  rejection of FSR1/temporal-FSR/ML-FSR/DLSS names.
- PPM header and payload length.

### Hybrid AOT and native proof

`tests/aot_native.rs`:

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
- PSR/banking fixture (`tests/aot_psr.rs`): a synthetic ROM switches
  SYSTEM -> IRQ -> SYSTEM -> IRQ -> SYSTEM via MSR, writes and re-reads
  SPSR_irq, reads banked SP/LR on both entries and ends in a self-branch. It
  asserts zero fallbacks, a direct-AOT block section with no interpreter call,
  exact hand-derived registers/CPSR/PC/budget, and a CPU digest that includes
  inactive banks. `tests/aot_psr.rs:232` verifies that a CPSR control write
  clears cached r8-r14 BX constants, and `tests/aot_psr.rs:268` verifies that a
  flags-only MSR keeps them.
- Negative-halfword fixture (`tests/aot_halfword.rs`): literal `LDR` initialises
  an EWRAM base and value, `STRH r1,[r0,#-0x20]` is translated directly (the
  generated block contains the `0xFFFFFFE0` displacement and no interpreter
  call), a DISPCNT store initialises mode 3, and `LDRH r4,[r0,#-0x20]` reads the
  value back. The report is compared with the reference and with an
  independently constructed bus that has exactly the negative-offset target and
  the DISPCNT write, not the positive-offset alias; runtime fallbacks are 0.
- Expanded-ALU fixture (`tests/aot_alu.rs`): a synthetic ARM program exercises
  AND/EOR/ORR/BIC/MVN, shifted MOV/ADD, SUBS/ADCS, an ARM `BL` with a
  `BX LR` return, and an MSR-driven IRQ-bank write. The native report must match
  a separately constructed final `Cpu` (including the inactive IRQ stack bank
  and state digest), exact 26-instruction budget, 0 fallbacks, DISPCNT-only bus
  digest and full image digest. A second fixture proves a dynamic `MOV PC,Rs`
  target takes two fresh reference fallbacks while the native block terminates.
- Thumb-ALU fixture (`tests/aot_thumb_alu.rs`): Thumb move-shifted, AND/ORR/EOR/
  MVN/NEG/SBC/ADD, high-register MOVs and both `BL` halves. The generated block
  section has no interpreter call; the dynamic callee takes exactly three
  reference fallbacks; the known return executes natively. Exact 25-instruction
  budget, independently constructed final CPU digest, bus and image are checked.
  A second emitted harness stops at budget 19, exactly between the two BL halves,
  with the intermediate LR and second-half PC preserved.
- `tests/aot_control.rs` checks discovery metadata without building a harness:
  ARM `BL` follows its callee and queues a return candidate, Thumb `BL` is two
  instructions with a dynamic callee and a queued return candidate, and an ALU
  PC write ends the block without statically following the next word.
- Emitter refuses an artifact bound to a different cartridge and refuses an
  artifact containing an empty block (before any generated loop could run).
- Harness build/run leaves caller-owned sentinel files untouched; the harness
  type exposes no recursive cleanup API.
- Interpreter RAM self-modification is already covered independently by
  `tests/cpu.rs::ram_code_is_decoded_fresh_after_self_modification`. The separate
  committed native-harness self-modifying-RAM regression remains open as issue
  #17; this round did not add it.

Discovery unit tests additionally cover: per-block cap splits blocks with
queued continuations (`block_limit_splits`), a zero per-block cap is an explicit
error, a global instruction budget is exact and terminal, and default limits
give no splits. `tests/aot_control.rs` checks the `bl_calls`, `bl_returns`,
`dynamic_bl` and `pc_writes` metadata without following data words; candidate
return continuations are counted there even when they are also external
targets.

A generated crate is written to a fresh owned directory, uses a separate
`CARGO_TARGET_DIR` inside that directory and `cargo --offline`, so it does not
require network access or take the parent Cargo lock. The test or CLI caller
removes only the directory it created.

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

Unit tests additionally cover a symlinked `output/` root, missing-parent
resolution, and the refusal of an existing fresh-directory target.

## Manual evidence commands

```text
cargo run -- demo
cargo run -- demo --size 480x320 --backend nearest
cargo run -- aot proof
cargo run -- info "<user-supplied .gba>"   # read-only, optional
```

The demo writes `output/demo.ppm` (ignored by Git). `aot proof` allocates a new
unique workspace under `output/` (or a new explicit `--out` directory), writes
a synthetic `fixture.gba` and its generated crate there, passes the fixture path
explicitly to the harness, and prints `proof=ok` plus
`native_vs_reference=match`. `aot emit` reports `embedded_rom_bytes=0`.

## Gate evidence

Observed at this implementation round (2026-10-25). These are recorded
observations, not a generated inventory:

- `./scripts/short.sh`: hard limit 0 oversized files, Ratchet fast 2/2 rules
  (`oversized-rust-file` 0 ≤ 0, `rust-source-population` 69 ≥ 36),
  `cargo fmt --check`, Clippy with warnings denied and the fast unit tests all
  passed.
- `./scripts/full.sh`: passed end to end, including debug and release suites,
  the demo (230,411 instructions, image SHA-256
  `b02968d620ee66d7175c29bbde12bc9ff075290f3e8086b26e9d4829b9ccb154`) and the
  native AOT proof (4 blocks / 17 translated instructions, 230,411 instructions,
  0 fallbacks, `native_vs_reference=match`). The discovery counts did not change
  because the demo fixture does not use the expanded ALU/control operations and
  no BIOS HLE primitive is reached by guest execution.
- `cargo test`: 200 tests passed (40 library unit tests, 5 CLI binary unit
  tests, 155 integration tests: AOT ALU 2, AOT control 4, AOT halfword 1, AOT
  native 5, AOT PSR 3, AOT Thumb ALU 1, ARM ALU 6, ARM control 9, ARM halfword
  5, ARM shift 4, BIOS arithmetic 6, BIOS dispatch 2, BIOS memory copy 13, BIOS
  reset 4, bus 6, cartridge 6, CLI output 5, CPU 12, demo 3, display 5, frontend
  gates 4, ISA golden 4, PSR banking 6, PSR field mask 4, PSR transfer 10,
  quality counter 10, scripts 3, Thumb ALU 7, Thumb control 5). This is the
  exact integrated snapshot for this round, not a guarantee of GBA completeness.
  The historical CPU-only snapshot was 164 tests (33 library unit, 5 CLI binary,
  126 integration), the isolated HLE snapshot was 155 tests (37 library unit, 5
  CLI binary, 113 integration) and the isolated GPU-foundation snapshot was 127
  tests (30 library unit, 5 CLI binary, 92 integration); none is reused as this
  round's count. The four frontend-gate tests use a fake Cargo or a deliberately
  minimal tool path and never fetch or build the optional GPU frontend.
- `cargo test --release`: 200 tests passed.
- Both suites were also repeated with fresh `CARGO_TARGET_DIR` directories, with
  the same 200/200 result in debug and release.
- The optional `apps/desktop` package is not part of that count: its separate
  `scripts/frontend.sh` gate ran 15 frontend unit tests plus locked fmt, Clippy,
  build and a headless `--help`, and all passed. None of its dependencies enter
  the root default graph.
- Generated harnesses built and run by tests: `tests/aot_native.rs` four (demo,
  mid-budget, immediate regression, fallback), `tests/aot_alu.rs` two (expanded
  ALU/banking/BL and dynamic-PC fallback), `tests/aot_thumb_alu.rs` two
  (full run and exact mid-pair stop), `tests/aot_psr.rs` one and
  `tests/aot_halfword.rs` one — ten in total. Their
  contract/invalidation tests make no generated build. `tests/cli_output.rs`
  runs two real default `aot proof` builds and refuses all redirect/invalid-path
  cases before Cargo starts.
- Ratchet v0.5.0: `check --tier fast` = 2 rules / 0 failed
  (`oversized-rust-file` 0 ≤ 0, `rust-source-population` 69 ≥ 36);
  `check --tier slow` = 1 rule / 0 failed (`native-consistency` 1 ≥ 1).
  A full `ratchet check` is 3 rules / 0 failed. The machine-written baseline is
  `{native-consistency: 1, oversized-rust-file: 0, rust-source-population: 36}`;
  adding files leaves the floor satisfied without editing the baseline.
- Maintained Rust population: 69 files (including `apps/desktop/src` and
  `tests/frontend_gates.rs`); maximum 481 physical lines (`src/cpu.rs`) and the
  largest other maintained file is `tests/bios_memory_copy.rs` at 447 lines. The
  code meets the hard limit without a baseline exemption or hand-edited
  baseline. The earlier 501-line/nested-directory boundary tests are unchanged
  in `tests/quality_counter.rs` and cover nested `src/output/` and
  `tests/target/` as maintained sources.
- A separate temporary untracked 501-line Rust probe with a 1-line root `lib.rs`
  in `src/output/` was reported by `oversized --root <temp> --max 500 --hard`
  with exit status 1 and stdout `1`, while root-level `target/` and `output/`
  probes were skipped; the probe was removed afterwards. `scripts/short.sh` also
  passed when invoked from another working directory.

The pre-refactor worktree was split into `src/aot/`, `src/decode/`,
`src/main.rs` and `src/cli/`; the earlier very large line counts were
uncommitted working-tree measurements, not initial HEAD sizes, and are not
used as historical evidence.
- Bounded read-only local retail observation: the existing CLI stops at abstract
  address `0x08000224` on the ARM single word store (base-register `STR`)
  family, after the previously blocking PC-operand ALU instruction at
  `0x08000220` now executes. No retail bytes, AOT source or copied payload were
  produced; the ROM SHA-256 remains
  `7c599c56849efeebeb93bd71f714932ae4cdf980db51c9c1016b4431057f71d4`.
- The six direct BIOS HLE primitives are covered only by synthetic tests in this
  integrated worktree. This snapshot is not evidence that they are wired to
  guest SWI/SVC dispatch, executed by the interpreter/CPU, integrated with the
  GPU/display stream, cycle-accurate, or playability evidence.

## Not established

No hardware conformance ROM suite has been run. No retail game is supported or known playable.
No BIOS boots, exception entry/return, SWI, interrupt delivery, save
persistence, audio, input interaction, DMA, cycle timing, tile/sprite rendering
or a playable window frontend is verified. The optional synthetic
`apps/desktop` frontend presents one static synthetic bitmap and has
build/unit-test evidence plus one local macOS Metal smoke observation; no live
Windows/Linux GPU run, cross-platform presentation claim, playable/input loop
or visible-frame counter is verified. PSR-transfer and banking evidence is
limited to the synthetic/direct-entry subset. The six direct BIOS HLE primitives
are synthetic-test evidence for those primitives only; guest SWI/SVC dispatch,
BIOS execution and hardware conformance remain unverified. No real GBA hardware
measurement is claimed. Neither FSR nor DLSS nor AI upscaling has been integrated
or tested. REA native analysis remains blocked as recorded.

Thumb `MUL`, low/low Thumb `ADD`, ARMv5 `BLX`/`BXJ`, `S=1` PC restore, and all
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
