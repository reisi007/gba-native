# GBA prototype contract — first milestone

Authority: S1 in `sources.md` for documented hardware behavior. Items marked
**Design** are this prototype's bounded choices and are not hardware claims.

This document describes what the code **actually implements and tests**. It is
not a roadmap promise and not a retail-game compatibility statement.

## Status and evidence language

- Experimental prototype; no retail game is supported or known playable.
- No BIOS is supplied or executed; direct entry is a test policy.
- ARM7TDMI PSR transfers and MSR-driven register banking are implemented for
  this subset. Exception entry/return, SWI and BIOS boot remain absent.
- No GPU vendor SDK, FSR, DLSS or ML upscaling is integrated.
- No successful REA Evidence was obtained (`rea-status.md`).
- This is not a certified separate-team/legal clean-room process.
- Instruction counts are budgets, not cycles or frames.

## Cartridge

- Accept exactly 192 bytes through 32 MiB inclusive; shorter/larger files return
  `CartSize` and do not panic.
- Decode title `0xA0..0xAC`, game code `0xAC..0xB0`, maker `0xB0..0xB2`, fixed
  byte `0xB2`, main-unit `0xB3`, device `0xB4`, version `0xBC`, complement
  `0xBD`.
- `fixed_ok` requires `0x96`. The complement is computed as
  `chk = 0; chk -= byte for 0xA0..=0xBC; chk = (chk - 0x19) & 0xFF`.
  Both diagnostics are reported independently; an invalid header value does not
  reject the image at parse time.
- SHA-256 identifies the exact bytes for tooling and AOT binding.
- The path loader rejects files whose metadata length exceeds 32 MiB before
  reading any payload and caps the read at `MAX_ROM_LEN + 1` bytes, so a huge or
  sparse file cannot force an unbounded allocation. The AOT harness uses this
  same bounded loader.
- **No Nintendo logo bytes are embedded or compared.** Logo validity is reported
  as “not validated”. The synthetic fixture keeps `0x04..0xA0` zero and uses the
  documented entry-point word at `0x00` as an ARM branch; direct entry ignores
  that branch and starts at `0x08000000`.

## Direct entry and CPU state

**Design:** the runner starts in system mode with CPSR `0x000000DF` (ARM, IRQ/FIQ
masked), SP `0x03007F00`, PC `0x08000000`, and all other registers zero. This is
not BIOS reset execution. There is no exception vectoring, SWI/HLE service, BIOS
read path, exception return, or PSR restore through ALU/PC writes. The ARM7TDMI
register banks and exception-mode SPSRs exist for MSR-driven mode switches and
SPSR transfers (below); they are not entered through exceptions. BIOS accesses
return `BiosAccess`.

- Instruction fetch is allowed only from ROM, EWRAM and IWRAM. Fetching from
  I/O, palette, VRAM, OAM, BIOS or unmapped memory returns `NonExecutable`.
- RAM is executable but is **never AOT-translated**, so self-modifying RAM code
  is decoded freshly by the interpreter on every execution.
- The CPU is instruction-budgeted. There is no scheduler, cycle count, scanline
  timing or frame completion concept.

## Bus

Implemented regions and widths:

| Region | Address | Size | Reads | Writes |
| --- | --- | --- | --- | --- |
| EWRAM | `0x02000000` | 256 KiB | 8/16/32 | 8/16/32 |
| IWRAM | `0x03000000` | 32 KiB | 8/16/32 | 8/16/32 |
| I/O | `0x04000000` | implemented registers only | 16 | 16 |
| Palette | `0x05000000` | 1 KiB | 8/16/32 | 8/16/32 |
| VRAM | `0x06000000` | 96 KiB | 8/16/32 | 8/16/32 |
| OAM | `0x07000000` | 1 KiB | 8/16/32 | 8/16/32 |
| ROM | `0x08000000`, `0x0A000000`, `0x0C000000` | 32 MiB windows over the same bytes | 8/16/32 | none |

- The three ROM windows alias the same immutable cartridge bytes via a 32 MiB
  offset mask. Writes return `RomWrite`; reads past the image return
  `RomOverread`. ROM bytes cannot be mutated through the bus.
- No general address mirroring is modeled. EWRAM/VRAM/palette/OAM addresses just
  outside their exact regions are `Unmapped`; this prototype does not claim the
  GBA's partial-decoding mirrors or open-bus behavior.
- Alignment: 16-bit bus accesses require even addresses and 32-bit accesses
  require 4-byte alignment. I/O supports only halfword access to the implemented
  registers. Other widths/addresses return explicit errors.
- Implemented I/O: DISPCNT halfword read/write and KEYINPUT halfword read
  (`0x03FF`, all buttons released). The I/O window is
  `0x04000000..=0x040003FF`; every other read/write in it, including the last
  byte at `0x040003FF`, returns `UnsupportedIoRead`/`UnsupportedIoWrite` rather
  than `Unmapped`. There is no VCOUNT, DISPSTAT, DMA, timer, interrupt, sound,
  serial, WAITCNT, BG affine parameter or save-register state.
- Save hardware (`0x0E000000..=0x0E00FFFF`) is unsupported. BIOS reads/writes
  and unmapped addresses are errors.
- CPU data-transfer rules actually implemented and tested:
  - A word load aligns down and rotates right by `(address & 3) * 8`; literal
    `LDR` supports this path.
  - A halfword load aligns down; odd addresses rotate the 32-bit halfword
    result right by 8 (ARM7 behavior). Signed halfword/byte loads are not
    implemented.
  - Halfword stores force-align down.
  - Byte stores to palette/VRAM duplicate the byte in both halves of the
    addressed halfword. Byte stores to OAM are ignored. These rules are tested.
- Bus errors are terminal for that execution run. A partially completed
  multi-access operation is not promised to be atomic.

## Implemented ARM subset

All 16 condition codes are evaluated from NZCV. A false condition is a no-op
that still consumes one instruction and advances PC, including for a PSR
transfer whose executed form would be restricted. PC reads as an ordinary
register operand are rejected; control flow is only via `B`/`BX`. Writes to PC
via ALU/load are rejected. BL, multiply, swap, block transfer, SWI,
coprocessor and newer ARM encodings are rejected with address/state/raw word
and are never NOPs.

| Family | Implemented | Rejected |
| --- | --- | --- |
| Data processing | `MOV`, `ADD`, `SUB`, `CMP`; register or immediate operand; `S` bit honored; immediate rotation carry tracked; CMP enforces its fixed `S=1`/`Rd=0` encoding | shifts, logical/ADC/SBC/RSB/RSC, PC operands, PC destination |
| Branch | conditional/unconditional `B`; `BX` register | `BL`, `BX PC`, `BLX` |
| Word load | `LDR` literal only, including ARM7 rotated unaligned literal read | `LDR`/`STR` with base register, register offset, writeback |
| Halfword | pre-indexed immediate `LDRH`/`STRH`, ±offset, no writeback | `LDRSB`, `LDRSH`, post-index, register offset, writeback |
| PSR transfer | `MRS` CPSR/SPSR; `MSR` CPSR/SPSR with documented field bytes, register or rotated imm8 source | exception entry/return, SPSR restore, PC destination, T-bit or invalid-mode changes |
| Everything else | none | multiply/swap, LDM/STM, SWI, coprocessor, unconditional/newer space |

Immediate data-processing encodings are classified by bit 25 before any
`bits 7..4` inspection: for an immediate, `bits 11..0` are the rotated imm8
payload, not a multiply/swap/halfword opcode. `CMP r0,#0xB0` is therefore `CMP`,
not `LDRH r0,[r0]`; unsupported `BIC`-class immediates error instead of aliasing
`STRH`, and a malformed `CMP` with `Rd != 0` errors instead of storing. An
independent test sweeps all 256 imm8 values × 16 rotations for `MOV`/`ADD`/`SUB`
with `S=0/1` and for `CMP`, checking typed IR, results, NZCV and memory
immutability against separately written ISA expectations.

Extra halfword transfers are classified before the PSR-transfer shape. A
pre-indexed negative `STRH` (`P=1`, `U=0`, `L=0`) has the same coarse
`bits 24-23 = 10b`/`bit 20 = 0` signature as MRS/MSR, for example
`0xE14322B0` = `STRH r2,[r3,#-0x20]`. Valid MRS and register-form MSR encodings
have bits 11-4 zero, while the halfword pattern `1SH1` keeps bit 7 set and
immediate-form MSR has bit 25 set, so the classes are mutually exclusive and no
raw-word whitelist is needed. An unsupported encoding, including an MSR with an
empty field mask, is still rejected before condition evaluation; a false
condition only suppresses execution of a successfully decoded instruction.

### PSR transfers and register banking

Implemented from GBATEK `#arm-opcodes-psr-transfer-mrs-msr`,
`#arm-cpu-register-set`, `#arm-cpu-flags--condition-field-cond` and
`#arm-cpu-exceptions`:

- `MRS Rd,CPSR` and `MRS Rd,SPSR_<current mode>` write the selected PSR to
  R0-R14. `Rd = R15` and non-zero unused/fixed bits are rejected at decode.
- `MSR` accepts the documented field bytes `f` (bits 31-24), `s` (23-16), `x`
  (15-8), `c` (7-0). The byte selection is wider than the ARM7TDMI writable
  mask: within `f` only N/Z/C/V (bits 31-28) can change and reserved bits 27-24
  are preserved; `s` and `x` address reserved ARM7TDMI bytes and change nothing;
  within `c` only I/F/T/M4-M0 (the low byte) can change and reserved bits 15-8
  are preserved. No write fabricates or zeroes a reserved bit.
- An MSR encoding whose field mask is zero is rejected at decode and by the
  public `Cpu::write_psr`, because GBATEK states that at least one field bit must
  be set and an empty mask is unpredictable. This is a **Design** error-first
  choice, not a claim about the result on any particular hardware unit.
- Register source is R0-R14; immediate source is `imm8 ROR (2 * rotation)`.
  Fixed bits (bit 20, bits 24-23, MRS bits 19-16/11-0, MSR bits 15-12 and
  register-form bits 11-4) are validated. Malformed encodings error without
  execution.
- In USER mode only the condition-code byte can change. A control-field write
  succeeds as a documented no-op instead of inventing a privilege fault. SYSTEM
  is privileged and may change I/F/mode.
- The T bit is CPSR bit 5 and the single ARM/Thumb state source; `BX` changes
  it. An MSR that would change T is rejected before mutation, for CPSR and for
  SPSR control writes.
- Only the current exception mode's SPSR is accessible. USER and SYSTEM have no
  SPSR; MRS/MSR SPSR there returns `Error::Psr` instead of a fabricated value.
  Exception-mode SPSRs start at zero in the direct-entry design state.
- Valid modes are USER `0x10`, FIQ `0x11`, IRQ `0x12`, SVC `0x13`, ABT `0x17`,
  UND `0x1B` and SYSTEM `0x1F`. Other values, including the old 26-bit
  compatibility modes, are rejected before mutation.
- R0-R7 are shared. R8-R12 are shared by every non-FIQ mode and banked only for
  FIQ. R13/R14 are banked per exception mode; USER and SYSTEM share one bank.
  FIQ/IRQ/SVC/ABT/UND each have an independent SPSR. An MSR control write
  switches the visible bank atomically with the mode bits.
- **Boundary:** there is no exception entry/vector fetch, no SWI/HLE, no
  banked-register save/restore by the CPU, no exception return and no SPSR
  restore. Those remain explicit unsupported behavior, not hidden successes.

Pipeline offsets are documented: ARM `PC = instruction address + 8`. Direct
branch targets are forced to 4-byte alignment.

## Implemented Thumb subset

| Family | Implemented | Rejected |
| --- | --- | --- |
| Add/subtract | `ADD`/`SUB` register and 3-bit immediate, flags set | carries, shifts |
| Immediate | `MOV`, `CMP`, `ADD`, `SUB` with 8-bit immediate | — |
| ALU | register `CMP` only | shifts, MUL, logical/ADC/SBC/NEG/CMN/etc. |
| High register | `MOV`, `CMP`, `BX` | high-register `ADD` |
| PC-relative | `LDR` literal | `ADR`, SP-relative loads/stores |
| Halfword | immediate `LDRH`/`STRH` | signed/register-offset forms |
| Branch | conditional `B` (`cond != 14/15`), unconditional `B` | `BL`/long branch, SWI, undefined `cond=14` |
| Block/stack | none | `PUSH`/`POP`, `LDM`/`STM` |

Thumb `PC = instruction address + 4`; branch targets are forced to 2-byte
alignment. `BX` alone changes ARM/Thumb state. Thumb `BL` is rejected as two
unimplemented halves rather than decoded as another instruction. The
high-register `MOV`/`CMP` forms require at least one high-register bit; valid
low/low encodings such as `0x4600` are deliberately outside this subset and are
rejected rather than treated as a different operation.

## Flag semantics

- `ADD`: N/Z from result, C from unsigned carry, V from signed overflow.
- `SUB`/`CMP`: N/Z from result, C is no-borrow, V from signed overflow.
- `MOV` (including `MOVS`): N/Z from result; C is the immediate-shift carry when
  a rotation actually occurs, otherwise preserved; V preserved.
- Thumb immediate `MOV` sets N/Z and preserves C/V.
- Unsupported flag-producing operations are not approximated.

## Display snapshot

Native image is 240x160, converted from RGB555 to RGB888 by bit replication
(`(v << 3) | (v >> 2)` per channel). This is a state snapshot, not a scanline
PPU.

- Mode 3: 240x160 direct RGB555 halfwords at VRAM offset 0.
- Mode 4: 240x160 palette indices; frame 0 at 0, frame 1 at `0xA000`, selected by
  DISPCNT bit 4.
- Mode 5: 160x128 RGB555 halfwords; frame 0/1 as above; rest of the 240x160
  output is backdrop (palette entry 0).
- Forced blank (DISPCNT bit 7) yields white.
- BG2 disabled (bit 10 clear) yields backdrop.
- DISPCNT mode bits other than 3/4/5, CGB bit 3, BG0/BG1/BG3 enables, OBJ enable,
  or any window enable are rejected as `UnsupportedPpuFeature`/display-mode
  errors. Bits 5/6 are accepted because they do not affect this bitmap subset.
- Tiles, sprites, OAM rendering, windows, blending, mosaic, nonidentity affine
  transforms and scanline effects are absent. Mosaic/blending registers are
  unsupported I/O, so they cannot be enabled through this bus.

Presentation (`upscaling.md`): integer nearest-neighbor only, exact 3:2 content,
centered black bars, output dimensions 1..=4096 and at least 240x160. FSR1,
temporal FSR, ML-FSR and DLSS names return `UnsupportedBackend`.

## Typed IR and interpreter

`ir::IrInstr` contains the instruction address, width/state, raw encoding,
condition and one typed operation (`Mov`, `Add`, `Sub`, `Cmp`, `B`, `Bx`,
`LdrLiteral`, `LdrHalf`, `StrHalf`, `Mrs`, `Msr`). The reference interpreter
executes these operations; it does not dispatch on raw opcodes or run an opcode
table. `Msr` carries the PSR selector, the four field bits and a register or
already-expanded immediate source.

## Hybrid AOT

- Discovery starts at an explicit, aligned `(PC, ARM/Thumb)` entry inside the
  immutable ROM and walks straight-line blocks. It follows direct `B` targets and the
  fallthrough of conditional branches. A `BX` target is followed only when local
  constant propagation (literal `LDR` or immediate `MOV`, same block, no
  intervening write) resolves it. This is intentionally not general static
  recovery.
- Discovery stops at dynamic `BX`, unknown/unsupported encodings and non-ROM
  targets. Limits are explicit: `max_blocks` and `max_instructions` are global
  terminal budgets (default 512/8192) and set `limit_reached`; hitting
  `max_block_instructions` (default 64) only ends the current block and queues
  its continuation, counted in `block_limit_splits`, so a small per-block cap
  can still discover reachable code. A per-block cap of zero is an explicit
  error rather than an infinite loop. It never decodes every ROM word as code.
- Every computed branch target is dispatched by `(PC, ARM/Thumb)`; the two states
  are distinct keys.
- The emitter consumes typed IR and writes direct Rust statements for known
  operations. It calls bus helpers for memory, flag helpers for arithmetic and
  `cpu.read_psr`/`cpu.write_psr` for PSR transfers; it does not call the decoder
  or interpreter for a known instruction.
- A CPSR control-field MSR can change the active register bank, so bounded
  discovery invalidates cached r8-r14 constants across it. It never carries a
  stale banked value into a later `BX`. Flags-only and reserved-field MSR forms
  do not invalidate the bank cache.
- Each translated instruction decrements the budget exactly once, including
  condition-false instructions and both sides of a conditional branch. Reaching
  zero stops before the next instruction in either reference or native execution.
- A dispatch key that is not a discovered ROM block goes through the reference
  interpreter for one instruction with a fresh fetch/decode. This includes all
  RAM code and any dynamic ROM target. Unsupported instructions still error.
- The artifact records ROM length and SHA-256. Emission re-decodes every IR
  instruction from the current cartridge and refuses a mismatch. The generated
  harness deliberately contains **no ROM byte array and no raw copy of the
  input image**; it may contain translated instruction constants and the
  digest. It requires an external `--rom PATH`, loads that file through the
  bounded cartridge loader, and compares SHA-256 plus length. A well-formed but
  different image is rejected with `ROM mismatch` and exit status 3; malformed
  input (for example a header shorter than 192 bytes) is rejected earlier by the
  cartridge loader with exit status 1. This is digest/length binding for an
  experimental prototype, not a signature, code-signing identity or
  anti-tamper guarantee.
- Partial coverage is explicit: `DiscoveryStats` reports blocks, translated
  instructions, resolved/dynamic BX, decode stops, external targets, per-block
  splits and whether a global limit was reached. The native report reports
  runtime fallback instructions and a CPU-state digest covering inactive
  register banks and SPSRs, not only the visible register snapshot.
- **Design, not a general claim:** the synthetic demo discovers 4 blocks / 17
  instructions and executes 230,411 guest instructions with 0 fallbacks. The
  fallback fixture deliberately leaves the dynamic target undiscovered and takes
  2 interpreter fallback steps. These are fixture facts, not evidence that
  arbitrary retail code can be statically recovered.

## CLI output and artifacts

`info`, `demo`, `run`, `aot proof` and `aot emit` are documented in `README.md`.
PPM is the only image output; there is no window frontend.

- Generated files are restricted to `<cwd>/output/...`. Relative paths resolve
  against the process working directory; absolute paths are accepted only when
  they resolve below that same `output/` root. The location is validated before
  any root or parent directory is created.
- Existing components below `output/` must be real directories. A symlinked
  directory, a symlinked or dangling final component, and a symlinked `output/`
  root are refused without following the link. New directories are created one
  component at a time with `create_dir`; fresh files use `create_new`.
- An explicit `aot proof --out` directory must not already exist. The default
  proof allocates a new unique `output/aot-proof-*` workspace per run; previous
  runs and user-owned output files are never erased or reused.
- `NativeHarness::build` requires a fresh crate directory, creates it with
  `create_dir`/`create_new`, and refuses pre-existing directories, files or
  links. The harness exposes no recursive cleanup API; callers own and remove
  only the directory they created.
- These are ownership and consistency checks, not a claim of race-freedom
  against an actively hostile process mutating the filesystem concurrently.

The demo guest image is produced by executing guest instructions
(`STRH` per pixel); the host never paints VRAM directly. Scaling happens only
after the snapshot.
