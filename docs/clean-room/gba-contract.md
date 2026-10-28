# GBA prototype contract — first milestone

Authority: S1 in `sources.md` for GBA interface behavior; S6 corroborates the
ARM7TDMI SWI exception entry/return contract. Items marked **Design** are this
prototype's bounded choices and are not hardware claims.

This document describes what the code **actually implements and tests**. It is
not a roadmap promise and not a retail-game compatibility statement.

## Status and evidence language

- Experimental prototype; no retail game is supported or known playable.
- No BIOS is supplied or executed; direct entry is a test policy. The core can
  receive a host-injected pressed-key mask through `Bus::set_pressed_keys`, but
  this is not interactive game support and has no frame-sampling guarantee.
- The documented ARMv4T data-processing ALU, barrel shifter, PC-operand rules,
  `B`/`BL`/`BX` and the corresponding Thumb ALU/high-register/long-branch
  subset are implemented and independently tested. ARM word/byte single data
  transfers, extended halfword/signed transfers, ARM `LDM`/`STM`, Thumb
  word/byte/half/signed transfers, SP-relative word transfers and Thumb
  `PUSH`/`POP`/`LDMIA`/`STMIA` are implemented for the policy below. ARM7TDMI
  PSR transfers and MSR-driven register banking are implemented for this
  subset. Typed ARM/Thumb SWI reaches the six documented BIOS HLE services via
  explicit SVC entry, a saved handler frame, SYSTEM-mode service execution and
  architectural SWI return. Other exception classes, multiply/swap and BIOS
  boot remain absent; Thumb `MUL` remains explicit.
- No GPU vendor SDK, FSR, DLSS or ML upscaling is integrated. Separate optional
  `apps/desktop` and `apps/web` packages present the same static synthetic
  snapshot through native Winit/Pixels/WGPU and browser WebGPU/WebGL2. They each
  depend directly on the std-only `gba-native-core` package, accept no cartridge
  or save data, are not playable/per-ROM frontends, and stay outside the default
  CLI and its headless PPM output.
- `aot build` compiles a headless release executable for a locally supported
  Cargo target from an externally bound ROM. It is an experimental artifact, not
  a playable game, GUI frontend, save-aware application, final app or browser
  build; target/profile support is not a cross-platform claim.
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
not BIOS reset execution. `Cpu::direct_entry()` remains unchanged; callers that
execute guest SWIs initialize the documented USER/SYS, IRQ and SVC stack tops
explicitly. The CLI guest runner and generated AOT harness do so. The
interpreter supports typed ARM/Thumb SWIs only for the six HLE services below:
it enters SVC at vector `0x08` without fetching BIOS bytes, frames handler state,
runs the service in SYSTEM mode and returns through full CPSR restoration.
Other exception classes, IRQ delivery and BIOS access remain unsupported.

- Instruction fetch is allowed only from ROM, EWRAM and IWRAM. Fetching from
  I/O, palette, VRAM, OAM, BIOS or unmapped memory returns `NonExecutable`.
- RAM is executable but is **never AOT-translated**, so self-modifying RAM code
  is decoded freshly by the interpreter on every execution.
- The CPU is instruction-budgeted. There is no scheduler, cycle count, scanline
  timing or frame completion concept.

## BIOS HLE services and guest SWI framing

`gba_native::bios::call(service, &mut cpu, &mut bus)` exposes exactly six
documented, timing-independent services as direct Rust primitives derived from
S1. Guest ARM/Thumb SWIs dispatch to those same services through
`bios::execute_swi`:

| SWI | Service | Boundary |
| --- | --- | --- |
| `01h` | `RegisterRamReset` | Clears selected memory groups 0..4 and always forces blank; documented groups 5..7 are unsupported without SIO/sound/other-I/O state, and reserved bits 8..31 are rejected |
| `06h` | `Div` | Signed truncated quotient, dividend-signed remainder, unsigned quotient magnitude; zero divisor and `MIN / -1` error before mutation |
| `07h` | `DivArm` | `Div` with the documented reversed r0/r1 argument order |
| `08h` | `Sqrt` | Floor unsigned integer square root of r0 |
| `0Bh` | `CpuSet` | Halfword/word copy or fill with prevalidated ranges |
| `0Ch` | `CpuFastSet` | Word copy or fill rounded up to 8-word blocks before range validation |

The direct `bios::call` API does not consume an instruction, advance PC, enter
SVC/IRQ mode, write SPSR, fetch a vector, count cycles or model wait states. A
guest SWI consumes exactly one guest instruction and performs the framed SVC
path described below; service internals add no budget, cycle or wait-state
accounting. Every other service number is an explicit
`BiosError::UnsupportedService`; Halt/IntrWait/VBlankIntrWait, Stop/Sleep,
SoftReset/HardReset and decompression are not implemented. Unsupported requests
are never successful no-ops. See `docs/clean-room/hle-services.md` for exact
policies and tests.

### Guest SWI/SVC path

ARM SWI service selection uses comment bits 23..16; Thumb SWI uses the encoded
low byte. An executed SWI first validates the known service request, then checks
that the SVC stack can hold an aligned four-word frame wholly inside EWRAM or
IWRAM. Unsupported services and known service precondition errors are rejected
before CPU or bus mutation. A supported SWI from SVC mode returns an explicit
`SwiNesting` error before mutation because this bounded implementation cannot
preserve an already-live `SPSR_svc`/`LR_svc` pair.

For a non-SVC caller, entry saves the old CPSR in `SPSR_svc`, writes the resume
address (ARM instruction address + 4 or Thumb address + 2) to `LR_svc`, enters
ARM SVC mode with IRQ masked and FIQ unchanged, and sets PC to `0x00000008`.
The handler saves four words at `SVC_SP - 16`, changes mode to SYSTEM while it
calls the HLE primitive, then returns to SVC, restores the frame and performs
`MOVS PC, LR` semantics, restoring the full interrupted CPSR and resume PC.

**Design policy, not a hardware claim:** the ascending-address frame words are
`SPSR`, `R11`, `R12`, `R14`, starting at `SVC_SP - 16`; the SVC SP is restored
to its pre-entry value on success. GBATEK identifies these four saved values but
does not establish their slot order in memory. The implementation does not claim
to reproduce a particular Nintendo BIOS's exact frame layout. A frame SP that
is misaligned, underflows, or is outside work RAM is rejected before framing.

`Cpu::initialize_bios_stacks()` explicitly sets USER/SYS SP to `0x03007F00`, IRQ
SP to `0x03007FA0`, and SVC SP to `0x03007FE0`. It is not implicit in
`Cpu::direct_entry()`. The `run` command and generated AOT harness initialize
these stack tops; synthetic direct-entry tests remain unchanged unless they opt
in. This model has no IRQ delivery, BIOS code fetch, cycle timing, scheduler or
wait-state accounting inside the HLE handler. A bus failure after exception
entry is terminal and is not rolled back.

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
- Implemented I/O: DISPCNT halfword read/write; BG0CNT (`0x04000008`)
  halfword read/write; BG0HOFS/BG0VOFS (`0x04000010`/`0x04000012`) halfword
  writes; and KEYINPUT halfword read (`0x04000130`). The host injects a `u16`
  pressed mask through `Bus::set_pressed_keys`: bits 0..9 are A, B, Select,
  Start, Right, Left, Up, Down, R and L, respectively; unused bits are ignored.
  Guest reads see active-low KEYINPUT (`0x03FF` by default, all released), while
  guest writes remain `UnsupportedIoWrite`. This host setter provides no frame
  scheduler, sampling guarantee or timing claim. BG0 scroll reads are
  write-only-register `UnsupportedIoRead` errors. Scroll writes retain only
  documented bits 0..8; unused upper bits have no state effect.
  Every other width/address, including the last I/O byte at `0x040003FF`, stays
  an `UnsupportedIoRead`/`UnsupportedIoWrite` rather than `Unmapped`. Mutable
  display-register and dynamic KEYINPUT state are included in `Bus::state_digest`.
  There is no VCOUNT, DISPSTAT, DMA, timer, interrupt, sound, serial, WAITCNT,
  BG affine parameter or save-register state.
- Save hardware (`0x0E000000..=0x0E00FFFF`) is unsupported. BIOS reads/writes
  and unmapped addresses are errors.
- CPU data-transfer rules actually implemented and tested:
  - A byte load/store uses `read8`/`write8` and the byte's exact address. It is
    never emulated as a wider access plus extraction. Byte stores to
    palette/VRAM duplicate the byte in both halves of the addressed halfword;
    byte stores to OAM are ignored; byte reads from palette/VRAM/OAM return the
    addressed byte.
  - A halfword load reads `address & !1`; an odd LDRH rotates the 32-bit
    zero-extended halfword right by 8 (ARM7 behavior). An odd LDRSH is a signed
    byte read at the odd address itself, not a rotated halfword. LDRSB is a
    signed byte at the exact address. Halfword stores force-align down.
  - A word load reads `address & !3` and rotates right by `(address & 3) * 8`;
    a word store force-aligns down. The preserved literal-`LDR` path follows the
    same rule.
  - ARM single transfers implement pre/post indexing, add/subtract, an
    immediate-12 or shifted-register offset (immediate shifts only; bit 4 must
    be zero and the offset register may not be PC), optional pre-index
    writeback and post-index writeback. The post-index `T` bit selects forced
    user access on hardware; this prototype has no MMU/privilege split, so the
    same checked GBA bus is used and CPU mode is unchanged (Design).
  - PC as a base reads instruction address + 8 and is rejected with writeback.
    PC as a stored source writes instruction address + 12. A word `LDR pc`
    force-aligns to `!3` and keeps ARM state (ARMv4, no exchange). Narrow loads
    into PC are explicit unsupported encodings.
  - Stores read the old source/base before any writeback. Writeback is
    committed only after the checked access succeeded; a bus error performs no
    writeback. A load whose destination aliases a written-back base is
    rejected before execution as unpredictable.
- Bus errors are terminal for that execution run. A partially completed
  multi-access operation is not promised to be atomic: registers or memory
  words already transferred remain changed, later accesses are not attempted,
  and no writeback is fabricated.

## Implemented ARM subset

Condition fields 0..14 are evaluated from NZCV. Field 1111b is the reserved
unconditional/newer space on ARMv4T and is rejected explicitly; it is not a
sixteenth condition. A false condition is a no-op that still consumes one
instruction and advances PC, including for a PSR transfer whose executed form
would be restricted. Multiply, swap, coprocessor, ARMv5 `BLX`/`BXJ` and other
newer encodings are rejected with address/state/raw word and are never NOPs.
Typed SWI is handled only by the bounded six-service path above; unsupported
service numbers and SVC-origin nesting are explicit errors.

| Family | Implemented | Rejected |
| --- | --- | --- |
| Data processing | all sixteen operations: `AND`, `EOR`, `SUB`, `RSB`, `ADD`, `ADC`, `SBC`, `RSC`, `TST`, `TEQ`, `CMP`, `CMN`, `ORR`, `MOV`, `BIC`, `MVN`; register or rotated-immediate operand; LSL/LSR/ASR/ROR/RRX shifted register by immediate or by register; `S` honored; PC operands and `S=0` PC destinations | `S=1` with `Rd=PC` (needs SPSR restore), ARMv2 P-variant test ops, ARMv5/newer |
| Branch | conditional/unconditional `B`; `BL` (LR = address+4); `BX` register, including the documented ARM7 `BX PC` behavior | `BLX` register/immediate, `BXJ` |
| Word/byte load/store | `LDR`/`STR`/`LDRB`/`STRB` with immediate-12 or shifted-register offset, pre/post indexing, add/subtract, pre-index writeback, post-index `T` on the flat bus, documented PC base/source offsets and word `LDR pc` alignment | PC base with writeback, load `Rd=Rn` with writeback, byte load into PC, offset register PC or reserved bit 4 set |
| Halfword/signed | `LDRH`/`STRH`/`LDRSB`/`LDRSH` with immediate or unshifted register offset, pre/post indexing and writeback; ARM7 odd-address rules | `LDRD`/`STRD` (ARMv5), narrow loads into PC, PC base with writeback, load alias with writeback, register-form reserved bits 11-8 nonzero |
| Block | `LDM`/`STM` IA/IB/DA/DB with non-empty register lists, optional writeback, ARMv4 base-in-list specials, PC load/store entries | `S=1` user-bank/SPSR transfer, empty list, PC base, unaligned base (bounded runtime policy) |
| PSR transfer | `MRS` CPSR/SPSR; `MSR` CPSR/SPSR with documented field bytes, register or rotated imm8 source | non-SWI exception entry/return, SPSR restore, PC destination, T-bit or invalid-mode changes |
| Software interrupt | ARM and Thumb SWI with typed comment metadata; framed SVC entry, six-service HLE dispatch and full `MOVS PC,LR` return | unsupported service numbers, nested SVC-origin SWIs |
| Everything else | none | multiply/swap, coprocessor, unconditional/newer space |

Immediate data-processing encodings are classified by bit 25 before any
`bits 7..4` inspection: for an immediate, `bits 11..0` are the rotated imm8
payload, not a multiply/swap/halfword opcode. `CMP r0,#0xB0` is therefore `CMP`,
not `LDRH r0,[r0]`; `BIC r0,r1,#0xB0` is a typed data-processing operation, not
`STRH`, and a malformed `CMP` with `Rd != 0` errors instead of storing. An
independent test sweeps all 256 imm8 values × 16 rotations for `MOV`/`ADD`/`SUB`
with `S=0/1` and for `CMP`, checking typed IR, results, NZCV and memory
immutability against separately written ISA expectations.

The barrel shifter follows the documented special cases: LSL#0 leaves C
unchanged; LSR#0/ASR#0 mean 32; ROR#0 is RRX; a register-specified shift uses
only the low eight bits, so 0/32/33/63/64/255/256 behave as documented. MOV/MVN
require their fixed `Rn=0`, and CMP/CMN/TST/TEQ require their fixed `Rd=0`
encoding. An empty MSR field mask remains an error.

`ADC`/`SBC`/`RSC` evaluate the shifted operand and RRX against the old carry
before any flag update; arithmetic flags then come from the arithmetic result,
not from the shifter carry. Logical operations update C from the shifter carry
(unchanged for LSL#0 or a zero register amount) and preserve V. `S=0` preserves
all four flags.

Extra halfword transfers are classified before the PSR-transfer shape. A
pre-indexed negative `STRH` (`P=1`, `U=0`, `L=0`) has the same coarse
`bits 24-23 = 10b`/`bit 20 = 0` signature as MRS/MSR, for example
`0xE14322B0` = `STRH r2,[r3,#-0x20]`. Valid MRS and register-form MSR encodings
have bits 11-4 zero, while the halfword pattern `1SH1` keeps bit 7 set and
immediate-form MSR has bit 25 set, so the classes are mutually exclusive and no
raw-word whitelist is needed. An unsupported encoding, including an MSR with an
empty field mask, is still rejected before condition evaluation; a false
condition only suppresses execution of a successfully decoded instruction.

### Memory-transfer bounded policies

Implemented from GBATEK `#arm-cpu-memory-alignments`,
`#arm-opcodes-memory-single-data-transfer-ldr-str-pld`,
`#arm-opcodes-memory-halfword-doubleword-and-signed-data-transfer`,
`#arm-opcodes-memory-block-data-transfer-ldm-stm`,
`#thumb-opcodes-memory-loadstore-ldrstr` and
`#thumb-opcodes-memory-multiple-loadstore-pushpop-and-ldmstm`. The following are
explicit bounded choices where the public document does not establish a result
this prototype is willing to generalize:

- ARM `LDR`/`STR`/`LDRB`/`STRB` register offsets use immediate shifts only; bit
  4 must be zero and the offset register may not be PC. RRX uses the old C but
  never updates a flag. Post-index `T=1` is accepted but does not change CPU
  mode because this bus has no privilege/MMU distinction.
- PC as a single-transfer base reads `address + 8`; PC with writeback is
  rejected. PC as a stored source writes `address + 12` for word, byte and
  halfword stores. A word load into PC clears the low two bits and leaves the T
  bit unchanged. Byte/half/signed loads into PC are rejected.
- A load destination equal to the written-back base register is rejected at
  decode. Stores may use the same register for source and base: the old source
  is stored, then writeback commits.
- ARM halfword register offsets are unshifted and require reserved bits 11-8 to
  be zero. `L=0,S=1` is the ARMv5 doubleword space and is rejected, not treated
  as `STRH`. Odd `LDRH` rotates the 32-bit halfword result; odd `LDRSH` is a
  signed byte at the odd address.
- ARM `LDM`/`STM` implement IA/IB/DA/DB, non-empty lists, `W`, and PC entries.
  `S=1`, PC as base and empty lists are explicit errors. Registers always move
  in ascending address order. With writeback and the base in the list, STM
  stores the old base when it is the first (lowest) entry and the new base
  otherwise; LDM has no additional writeback and the loaded base value wins
  (documented ARMv4 behavior).
- Block/stack base alignment is a **Design** policy: GBATEK says low bits are
  ignored for LDM/STM/PUSH/POP, but it does not establish whether writeback is
  derived from the original or the aligned base. This prototype rejects an
  unaligned base before any access or writeback instead of choosing one result.
  The same policy applies to Thumb SP and `LDMIA`/`STMIA` bases.
- Thumb implements word/byte register-offset forms, half/signed
  register-offset forms, immediate word/byte transfer forms, the existing
  immediate halfword form, SP-relative word transfers and the preserved literal
  PC-relative load. `PUSH`/`POP` use full-descending SP with ascending memory
  order and the LR/PC bit; `POP PC` clears bit 0 and keeps Thumb state.
  `LDMIA`/`STMIA` use the ARMv4 base-in-list special documented for Thumb:
  STM stores the old base if it is the first entry, otherwise the new base;
  LDM's loaded base wins. Empty lists are explicit errors.
- No CPU data-abort exception is modeled. A bus error ends the run; a partial
  multi-access leaves earlier transfers visible, performs no writeback and
  never claims atomicity.

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
- **Boundary:** only the documented SWI-to-HLE path performs SVC entry, a bounded
  saved frame and exception return. Other exception classes/vectoring, IRQ
  delivery, BIOS execution and other SPSR-restoring PC writes remain unsupported.

Pipeline offsets are documented. ARM operands reading R15 use
`instruction address + 8`, or `+12` when the instruction uses a
register-specified shift. Thumb operands reading R15 use `instruction address +
4`; `ADR` and `ADD Rd,PC,#imm` address generation use the word-aligned PC+4
value. Direct branch targets are forced to 4-byte (ARM) or 2-byte (Thumb)
alignment.

ARM ALU/MOV writes to PC with `S=0` are allowed, force-align the low two bits
and keep the ARM state; `S=1` with `Rd=PC` is explicitly unsupported because it
would require an SPSR restore. Thumb `MOV PC,Rs`/`ADD PC,Rs` force-align the low
bit and keep Thumb state. `BX` bit 0 is the only state switch. `BX PC` follows
GBATEK's ARM7 behavior (ARM `$+8`; Thumb `(($+4) AND NOT 2)`, switching to ARM);
GBATEK notes that this case is not officially defined as predictable.

## Implemented Thumb subset

| Family | Implemented | Rejected |
| --- | --- | --- |
| Move shifted | `LSL`/`LSR`/`ASR` immediate, flags set | — |
| Add/subtract | `ADD`/`SUB` register and 3-bit immediate, flags set | — |
| Immediate | `MOV`, `CMP`, `ADD`, `SUB` with 8-bit immediate | — |
| ALU | `AND`, `EOR`, `LSL`, `LSR`, `ASR`, `ADC`, `SBC`, `ROR`, `TST`, `NEG`, `CMP`, `CMN`, `ORR`, `BIC`, `MVN`; shift amount from register low byte | `MUL` remains explicit |
| High register | documented high-register `ADD`, `CMP`, `MOV` (at least one of `Rd`/`Rs` high) and `BX` including PC; low/low `MOV`/`CMP` as an explicit **Design** extension | low/low `ADD` (documented reserved encoding), ARMv5 `BLX` register |
| Address | `ADR`, `ADD Rd,PC,#imm`, `ADD Rd,SP,#imm`, `ADD`/`SUB SP,#imm`; SP-relative word `LDR`/`STR` | — |
| PC-relative | `LDR` literal | — |
| Word/byte | register-offset `LDR`/`STR`/`LDRB`/`STRB`; immediate-offset word/byte forms | — |
| Halfword/signed | immediate `LDRH`/`STRH`; register-offset `LDRH`/`STRH` and sign-extended `LDSB`/`LDSH` | ARMv5/newer encodings outside these formats |
| Branch | conditional `B` (`cond != 14/15`), unconditional `B`, `BL` as two half instructions; typed SWI to the six documented HLE services | ARMv5 `BLX` second half, unsupported SWI services, undefined `cond=14` |
| Block/stack | `PUSH`/`POP` with low list plus LR/PC; `LDMIA`/`STMIA` low-register lists with writeback and documented base-in-list specials | empty lists, unaligned SP/base (bounded runtime policy), ARMv5/newer |

Thumb `PC = instruction address + 4`; branch targets are forced to 2-byte
alignment. `BX` alone changes ARM/Thumb state. High-register `ADD`/`MOV` never
set flags; high-register `CMP` always does. GBATEK THUMB.5 restricts the
high-register group to encodings with at least one high register bit, so the
accepted low/low `MOV` and `CMP` forms are an explicit **Design** extension of
this prototype, not documented-valid aliases; their behavior and tests are
unchanged by that label. Low/low `ADD` stays the documented reserved encoding
and is rejected.

Thumb `BL` is two independent 16-bit instructions. The first sets an
intermediate LR and does not branch; the second computes `LR + (imm11<<1)`, sets
`LR = next address | 1` and branches. Each half consumes one instruction, so a
budget can stop between them. Bounded discovery treats the first half as an
ordinary instruction and the second as a dynamic computed target; the known
return address is still queued as a discovery candidate. The ARMv5 `BLX` second
half (`0xE800..0xEFFF`) and register form are rejected.

## Flag semantics

- Logical operations (`AND`, `EOR`, `TST`, `TEQ`, `ORR`, `MOV`, `BIC`, `MVN`,
  and the Thumb shift-immediate/register forms): N/Z from the result, C from the
  shifter carry when the shift actually produces one, otherwise preserved; V
  preserved.
- Arithmetic operations (`ADD`, `ADC`, `SUB`, `SBC`, `RSB`, `RSC`, `CMP`, `CMN`,
  Thumb `NEG`): N/Z from the result, C from the arithmetic carry/no-borrow, V
  from signed overflow.
- `ADC`/`SBC`/`RSC` consume the old C as carry/borrow input after evaluating the
  operand (including RRX); they never use the shifter carry as their carry-in.
- ARM immediate rotations update C only when a rotation occurs; `MOVS` with
  rotation zero preserves C. Thumb immediate `MOV` sets N/Z and preserves C/V.
- Thumb high-register `ADD`/`MOV`, `ADR` and SP adjustments do not change flags.
- `S=0` preserves all flags; test operations and CMP/CMN always set them.
- Unsupported flag-producing operations are not approximated.

## Display snapshot

Native image is 240x160, converted from RGB555 to RGB888 by bit replication
(`(v << 3) | (v >> 2)` per channel). This is a static state snapshot, not a
scanline PPU, frame renderer or timing claim.

- Mode 0 implements BG0 text-background snapshots only. After unsupported-layer
  validation, BG0 disabled yields the backdrop (BG palette entry 0); forced
  blank (DISPCNT bit 7) yields white. Other enabled BG layers, OBJ, windows and
  CGB mode return `UnsupportedPpuFeature`. Otherwise nonblank Modes 1 and 2
  remain `UnsupportedDisplayMode`.
- BG0CNT priority is read and ignored because only one layer is composed.
  Character base (bits 2..3) selects a 16 KiB unit, color depth (bit 7) selects
  4bpp/8bpp, screen base (bits 8..12) selects a 2 KiB unit, and size (bits
  14..15) selects 256x256, 512x256, 256x512 or 512x512 pixels. Mosaic (bit 6)
  returns `UnsupportedBgControl`; reserved bits 4..5 and 13 also fail explicitly
  when enabled BG0 is rendered.
- Text maps use 32x32-entry (2 KiB) screen blocks. Larger sizes select SC0..SC3
  in row-major screen-block order at base +0, +2 KiB, +4 KiB and +6 KiB. BG0
  scroll uses the low nine bits of each write-only offset register and wraps at
  the selected background dimensions. Map entries select a tile, horizontal
  and vertical flip, and (in 4bpp mode) one of 16 palette banks.
- 4bpp tiles use 32 bytes and low-nibble-first left pixels; 8bpp tiles use 64
  bytes and one palette index per pixel. Palette index 0 is transparent in
  either depth and reveals the backdrop, which is BG palette entry 0. Nonzero
  indices read the selected BG palette entries and convert RGB555 to RGB888.
- Mode 0 BG maps and tile data are restricted to the first 64 KiB of VRAM.
  Selecting a map/tile byte outside that documented BG allocation returns
  `BgVramOutOfRange`; it does not read the separate OBJ allocation or wrap.
- Modes 3, 4 and 5 retain the existing bitmap paths: Mode 3 is 240x160 direct
  RGB555 at VRAM offset 0; Mode 4 is 240x160 palette indices with frame 0 at 0
  and frame 1 at `0xA000` selected by DISPCNT bit 4; Mode 5 is 160x128 RGB555
  with the remainder of the 240x160 output set to backdrop (palette entry 0).
  In each bitmap mode, BG2 disabled (bit 10 clear) yields backdrop. Bits 5/6
  remain accepted because they do not affect these implemented snapshots.
- OBJ rendering, other BG layers, OAM, windows, blending, BG0 mosaic,
  nonidentity affine transforms and scanline effects remain absent. Unsupported
  registers still fail through the I/O bus rather than being treated as no-ops.

Presentation (`upscaling.md`): the core CLI/CPU reference emits integer
nearest-neighbor PPM only, with exact 3:2 content, centered black bars and output
dimensions 1..=4096 and at least 240x160. FSR1, temporal FSR, ML-FSR and DLSS
names return `UnsupportedBackend`. The optional, separate `apps/desktop` package
presents the same static synthetic snapshot through Pixels/WGPU with
pixel-perfect integer scaling and a black letterbox; it is not part of the
std-only core or this PPM backend and is not a vendor upscaler. Its exact
boundary is in `docs/gpu-frontend.md`. The optional, separate `apps/web` package
executes the synthetic demo through WASM and presents its static snapshot through
browser WebGPU or WebGL2; it is not per-ROM execution, not part of the root
dependency graph, and not a vendor upscaler (`docs/web-frontend.md`). A separate
`aot build --target wasm32-unknown-unknown` branch produces a ROM-bound WASM
execution module callable from a browser, but it ships no HTML/JS application,
renderer, input or persistence and is not part of `apps/web`.

## Typed IR and interpreter

`ir::IrInstr` contains the instruction address, width/state, raw encoding,
condition and one typed operation. Data-processing sources are either
`Src::Reg`, `Src::Imm { value, carry }` or an explicit `Src::RegShift { rm,
shift }` with `ShiftKind` and an immediate or register amount. The remaining
ALU operations use `Op::Alu { opcode: AluOpcode, ... }`; MOV/ADD/SUB/CMP keep
their earlier explicit variants so previously supported IR shapes and tests are
unchanged. Branch operations are `B`, `Bl`, the two Thumb halves `BlHigh` and
`BlLow`, `Bx` and typed `Swi { comment }`. Memory operations are `LdrLiteral`, the preserved
pre-indexed-immediate `LdrHalf`/`StrHalf` shapes, and typed `MemSingle`
(pre/post, width, sign, add/subtract, writeback, post `T`), `BlockTransfer`,
`Push`, `Pop` and `ThumbBlock`. PSR operations remain `Mrs`/`Msr`. `MemOffset`
carries either a decoded immediate or a typed shifted register, and `MemWidth`
keeps byte/halfword/word access distinct.

The reference interpreter executes these operations and does not dispatch on
raw opcodes or run an opcode table. Shared `semantics` helpers evaluate typed
operands, PC pipeline values, shifts and operand carry; they are used by both
the reference interpreter and the emitter. `Msr` carries the PSR selector, the
four field bits and a register or already-expanded immediate source. Control-flow
metadata (`writes_pc`) marks direct branches, `BX`, SWI, the second (`BlLow`) half of
Thumb `BL`, ALU/MOV writes to R15, a word `MemSingle` load into R15, an ARM
`LDM` with PC in the list, and Thumb `POP {...,PC}`. Discovery and emission
therefore terminate blocks consistently; a false condition still exposes the
fallthrough. `writes_register` accounts for every destination and writeback
base, so no constant survives a load, block list, stack transfer or writeback.
The first (`BlHigh`) half writes only the intermediate LR and falls through: it
does not write PC and does not itself end the block.

## Hybrid AOT

- Discovery starts at an explicit, aligned `(PC, ARM/Thumb)` entry inside the
  immutable ROM and walks straight-line blocks. It follows direct `B` targets,
  the fallthrough of conditional branches, and both sides of a conditional
  instruction. An ARM `BL` follows its callee *and* its return address, bounded by
  the same global limits; it is never assumed that a target is a code word.
  Thumb `BL` is deliberately not fused: the first half is an ordinary LR update,
  the second half is a dynamic LR target, and the known return address is queued.
  A `BX` target is followed only when local constant propagation (literal `LDR`
  or immediate `MOV`, same block, no intervening write) resolves it. This is
  intentionally not general static recovery.
- A write to R15 through MOV/ALU or a word memory load terminates the current
  block and is not followed statically; a false condition still makes its
  fallthrough discoverable. The same applies to ARM `LDM` with PC and Thumb
  `POP PC`. This prevents translating instructions that are only reachable
  through a dynamic computed target. RAM is never discovered or translated.
- A typed SWI terminates its block and queues the known next-instruction
  continuation, because a supported HLE service returns there. `DiscoveryStats`
  counts SWIs separately. Native emission calls the framed
  `bios::execute_swi` path directly; generated harnesses initialize the
  documented BIOS stack tops before dispatch. Unsupported services still return
  explicit errors at runtime.
- Discovery stops at dynamic `BX`, dynamic `BL` targets, unknown/unsupported
  encodings and non-ROM targets. Limits are explicit: `max_blocks` and
  `max_instructions` are global terminal budgets (default 512/8192) and set
  `limit_reached`; hitting `max_block_instructions` (default 64) only ends the
  current block and queues its continuation, counted in
  `block_limit_splits`, so a small per-block cap can still discover reachable
  code. A per-block cap of zero is an explicit error rather than an infinite
  loop. It never decodes every ROM word as code.
- Every computed branch target is dispatched by `(PC, ARM/Thumb)`; the two states
  are distinct keys. A Thumb `BL` second half can therefore dispatch to a
  discovered block or a fresh interpreter fallback without decoding stale RAM.
  A dispatch key that is not a discovered ROM block goes through the reference
  interpreter for one instruction with a fresh fetch/decode. This includes all
  RAM code and any dynamic ROM target. Unsupported instructions still error.
- The emitter consumes typed IR and writes direct Rust statements for known
  operations. It calls bus helpers for memory, flag helpers for arithmetic,
  shared typed operand/shift helpers for registers and PC pipeline values, and
  `cpu.read_psr`/`cpu.write_psr` for PSR transfers; it does not call the decoder
  or interpreter for a known instruction. Each new ALU/control operation has a
  direct emission path. `MemSingle`, `BlockTransfer`, `Push`, `Pop` and
  `ThumbBlock` emit explicit address/value statements, width-correct
  `bus.read8`/`read16`/`read32`/`write8`/`write16`/`write32` calls, and
  writeback only after the checked access. Block and stack register lists are
  unrolled in ascending memory order; there is no decoder, interpreter step or
  whole-IR memory wrapper in a translated block.
- Emission is split into a target-neutral program section and a native harness
  driver. `emit_program_module` emits the translated block functions, the entry
  constants and `run_program(cpu, bus, budget)`, which dispatches known blocks,
  uses the same fresh reference fallback for unknown targets, and returns the
  remaining budget and fallback count without writing output. That section
  contains no `main`, argument parsing, file/path/process access, stdout or
  stderr reporting, ROM digest/length binding or ROM bytes; the caller loads the
  cartridge and initializes the CPU, bus and entry state. `emit_native_module`
  remains the compatibility API that composes the program with the native
  harness driver, which owns the external `--rom PATH` load, SHA-256+length
  binding, the state report and its exit codes. `emit_wasm_module` composes the
  same program section with a wasm-bindgen driver that accepts a caller-supplied
  `Uint8Array`, checks the embedded SHA-256+length before building the
  cartridge, CPU or bus, and returns a compact state/budget/fallback report with
  core errors propagated as JS errors; it has no filesystem, process or stdout
  boundary. This is a structural split of the existing synthetic-fixture
  harness, not a claim of general ROM compilation or playability.
- `NativeHarness::build_for_target(source, dir, target)` compiles the composed
  native module in Cargo release mode (`--offline`, `--release`,
  `--target <triple>`) with an own `CARGO_TARGET_DIR` inside the fresh crate
  directory, and returns the target-specific release executable path. It rejects
  an empty or malformed target before creating `dir`, keeps the
  `create_dir`/`create_new` ownership rules, and never executes the produced
  executable. A successful Cargo invocation that does not produce the expected
  executable fails explicitly; this path supports executable targets only. The
  target suffix policy only distinguishes Windows triples; every other target
  keeps the host-independent harness name. Whether a triple can be built is
  decided by the actual installed Cargo/rustc toolchain, not by this API; the
  method is not a cross-compilation or platform-support claim.
  `NativeHarness::build` remains the host/debug API used by the existing
  differential tests.
- `WasmModule::build_with_bindgen(source, dir, bindgen)` creates a fresh
  generated cdylib crate with the same `create_dir`/`create_new` ownership rules,
  a direct path dependency on `gba-native-core` (renamed to the `gba_native`
  namespace) and the exact `wasm-bindgen` 0.2.129 pin, compiles it offline in
  release mode for `wasm32-unknown-unknown` with an own `CARGO_TARGET_DIR`, then
  runs the repository-owned pinned wasm-bindgen CLI
  (`--target web --no-typescript`) into `pkg/` and verifies the non-empty JS glue
  and `_bg.wasm`. A missing or mismatched CLI is an explicit error with the
  `scripts/bootstrap-web-tools.sh` instruction; there is no global fallback or
  auto-install. Only `wasm32-unknown-unknown` is accepted by this branch; other
  `wasm32*`/`wasm64*` triples are rejected before any output is created. The
  generated artifact is a browser-callable execution module, not an HTML/JS
  application or GPU frontend.
- A CPSR control-field MSR can change the active register bank, so bounded
  discovery invalidates cached r8-r14 constants across it. It never carries a
  stale banked value into a later `BX`. Flags-only and reserved-field MSR forms
  do not invalidate the bank cache. Every register any decoded instruction can
  write is invalidated conservatively, including single-load destinations,
  block-list registers, Thumb stack results, SP and writeback bases; only
  immediate/MOV and literal loads introduce constants.
- Each translated instruction decrements the budget exactly once, including
  condition-false instructions, both sides of a conditional branch, each half of
  a Thumb `BL`, and a whole `LDM`/`STM`/`PUSH`/`POP`/Thumb block transfer. The
  budget is one per guest instruction, never one per transferred register and
  never a cycle or frame count. Reaching zero stops before the next instruction
  in either reference or native execution.
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
  instructions, SWIs, resolved/dynamic BX, ARM `BL` callees, candidate return
  continuations queued for an ARM `BL` or the second Thumb `BL` half, dynamic
  Thumb-`BL` targets, non-branch PC-write block terminations (e.g. ALU/MOV,
  word `LDR`, `LDM`, or `POP` to PC), decode stops, external targets, per-block
  splits and whether a global limit was reached. A queued return
  continuation is a candidate address, not proof of ROM-resident code; the same
  address is counted as an external target when it is not a ROM address. The
  native report reports runtime fallback instructions and a CPU-state digest
  covering inactive register banks and SPSRs, not only the visible register
  snapshot.
- **Design, not a general claim:** the synthetic demo still discovers 4 blocks /
  17 instructions and executes 230,411 guest instructions with 0 fallbacks.
  Separate synthetic fixtures exercise the expanded ALU with banked state and an
  ARM `BL`, Thumb ALU/`BL` halves with three fresh fallbacks for the dynamic
  callee, a dynamic MOV-to-PC fallback, and the existing fallback fixture
  (2 interpreter steps). Memory fixtures exercise direct native word/byte/
  half/signed transfers, a dynamic word `LDR pc` target (one fallback), ARM
  `STM`/`LDM` and Thumb `PUSH`/`POP`/`LDM`/`STM` with hand-derived final CPU and
  bus state. A dedicated native-RAM fixture (issue #17) writes `MOV r0,#1` /
  `BX lr` to EWRAM, calls it, patches it to `MOV r0,#2`, calls it again, and
  ends with exactly four fresh reference fallbacks (two RAM instructions per
  visit), no RAM block in the artifact, and final `r0=2`. This #17 native
  regression is implemented and locally tested but remains pending independent
  verification and CI; the issue is not closed by the implementation alone.
  Synthetic AOT SWI fixtures collectively cover native ARM `Div`, `DivArm`,
  `Sqrt`, `CpuSet` and `RegisterRamReset`, plus Thumb `CpuFastSet`, known return
  continuations, initialized SVC stacks, exact guest budgets and direct HLE
  emission without interpreter fallback. The five-call ARM fixture checks
  hand-derived complete CPU-bank/SPSR and bus/frame oracles, not only the
  interpreter; the ARM/Thumb fixture independently checks the complete digest,
  including its saved Thumb CPSR. Native failure fixtures check unsupported
  service 00h, false conditional unsupported SWI, service preconditions, invalid
  SVC stack and SVC-origin nesting; error-path instrumentation confirms CPU and
  bus digests are unchanged. These are fixture facts, not evidence that arbitrary
  retail code can be statically recovered.

## CLI output and artifacts

`info`, `demo`, `run`, `aot proof`, `aot emit` and `aot build` are documented in
`README.md`. PPM is the only image output of the core CLI. The optional,
separate `apps/desktop` package presents the static synthetic snapshot in a
Winit/Pixels window, while `apps/web` presents the same synthetic image in a
browser through WASM/WebGPU or WebGL2. Neither accepts a cartridge or is built
by the default workspace; neither is a core CLI output format. See
`docs/gpu-frontend.md` and `docs/web-frontend.md`.

- Generated files are restricted to `<cwd>/output/...`. Relative paths resolve
  against the process working directory; absolute paths are accepted only when
  they resolve below that same `output/` root. The location is validated before
  any root or parent directory is created.
- `run <rom.gba> [--budget N] [--out output/<new-directory>]` retains its current
  bounded execution and creates no files when `--out` is omitted. When requested,
  it creates the fresh directory only after execution and `display::snapshot`
  both succeed, then writes `screen.ppm` and `report.txt` through the safe output
  helpers. The report identifies the static snapshot, ROM/image hashes, requested
  and used instruction budgets, final CPU state, DISPCNT/mode and backdrop, and
  the `instruction_budget` stop reason. A run or snapshot error creates no run
  output directory. This is a post-budget bitmap snapshot, not a frame, boot,
  hardware-timed render, gameplay or compatibility claim; default/backdrop and
  forced-blank snapshots are not rejected merely because the display did not
  change.
- Existing components below `output/` must be real directories. A symlinked
  directory, a symlinked or dangling final component, and a symlinked `output/`
  root are refused without following the link. New directories are created one
  component at a time with `create_dir`; fresh files use `create_new`.
- An explicit `aot proof --out` directory must not already exist. The default
  proof allocates a new unique `output/aot-proof-*` workspace per run; previous
  runs and user-owned output files are never erased or reused.
- `NativeHarness::build` requires a fresh crate directory, creates it with
  `create_dir`/`create_new`, and refuses pre-existing directories, files or
  links. The generated manifest depends only on the sibling `gba-native-core`
  package (under the `gba_native` alias), never on the AOT crate or root facade.
  The harness exposes no recursive cleanup API; callers own and remove only the
  directory they created.
- `aot build <rom.gba> --target <triple> [--out <new-directory>] [--budget N]`
  is an experimental headless artifact command. It loads the supplied ROM
  read-only through the bounded cartridge loader, discovers from `0x08000000`
  in ARM state and validates the target before reading the ROM or creating
  output. A non-WASM target emits the compatibility native module and builds a
  release executable for it. `wasm32-unknown-unknown` emits the target-neutral
  program section plus the wasm-bindgen `run_rom(Uint8Array)` driver, builds a
  fresh cdylib offline with the exact wasm-bindgen 0.2.129 pin and runs the
  repository-owned pinned wasm-bindgen CLI into `pkg/` (JS glue plus `_bg.wasm`).
  Other `wasm32*`/`wasm64*` triples are rejected explicitly. The command never
  executes the built artifact.
- The default `aot build` workspace is a new unique
  `<cwd>/output/native-aot-app-*` directory; the WASM branch uses
  `<cwd>/output/wasm-aot-module-*`. An explicit `--out` must be a new path below
  `output/` and is refused when it already exists. The owned workspace contains
  `generated.rs`, the generated crate (with its own Cargo target directory),
  `artifact.meta` and `build.meta`; the WASM branch also contains `pkg/` and a
  generated `README.md`. The native executable is reported at its
  target-specific release path, the WASM module and JS glue at their `pkg/`
  paths. No ROM copy is written anywhere under the workspace.
- `artifact.meta` and `build.meta` are ordered `key=value` files. They record
  the ROM SHA-256 and length, target, entry/state, budget, discovery counts,
  generated-source SHA-256, and actual Cargo/rustc versions plus the rustc host
  where obtainable. The native branch records the produced-executable SHA-256;
  the WASM branch records `artifact_kind=wasm-module`, `module_sha256` and
  `js_glue_sha256` instead and adds `wasm_bindgen_version=0.2.129` to both
  files. `artifact.meta` also records `embedded_rom_bytes=0` and
  `requires_external_rom=true`. The metadata deliberately contains no absolute
  paths, timestamps or ROM bytes. The native executable still requires
  `--rom PATH` at runtime; the WASM module takes a caller-supplied `Uint8Array`
  and rejects a different image through its SHA-256+length binding before any
  cartridge, CPU or bus exists.
- `aot build` is a headless prototype artifact, not a playable standalone game,
  GUI frontend, save-aware application, final application or complete browser
  app. The WASM branch produces a browser-callable execution module only: no
  HTML/JS UI, GPU renderer, input, persistence or full frontend. Its verified
  behavior is limited to synthetic fixtures; building a target with the
  installed toolchain is not evidence of cross-platform support or retail-game
  compatibility.
- These are ownership and consistency checks, not a claim of race-freedom
  against an actively hostile process mutating the filesystem concurrently.

The demo guest image is produced by executing guest instructions
(`STRH` per pixel); the host never paints VRAM directly. Scaling happens only
after the snapshot.
