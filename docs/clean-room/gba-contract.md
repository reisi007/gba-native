# GBA prototype contract — first milestone

Authority: S1 in `sources.md` for GBA interface behavior; S6 corroborates the
ARM7TDMI SWI exception entry/return contract. Items marked **Design** are this
prototype's bounded choices and are not hardware claims.

This document describes what the code **actually implements and tests**. It is
not a roadmap promise and not a retail-game compatibility statement.

## Status and evidence language

- Experimental prototype; no retail game is supported or known playable.
- No BIOS is supplied or executed; direct entry is a test policy. BIOS data
  reads fail closed by default; `--synthetic-bios-zero-fill` is an explicit
  experimental opt-in for zero data reads only (see below), never authentic BIOS
  data, opcodes or executable BIOS. BIOS stores fail closed by default;
  `--synthetic-bios-rom-writes` is a separate explicit opt-in that models the
  documented mask-ROM "store has no effect" behavior without storing any BIOS
  byte. The core can
  receive a host-injected pressed-key mask through `Bus::set_pressed_keys`, but
  this is not interactive game support and has no frame-sampling guarantee.
- The documented ARMv4T data-processing ALU, barrel shifter, PC-operand rules,
  `B`/`BL`/`BX` and the corresponding Thumb ALU/high-register/long-branch
  subset are implemented and independently tested. ARM word/byte single data
  transfers, extended halfword/signed transfers, ARM `LDM`/`STM`, Thumb
  word/byte/half/signed transfers, SP-relative word transfers and Thumb
  `PUSH`/`POP`/`LDMIA`/`STMIA` are implemented for the policy below. ARM7TDMI
  PSR transfers and MSR-driven register banking are implemented for this
  subset. Typed ARM/Thumb SWI reaches the six default documented BIOS HLE
  services, plus an explicit decompression opt-in, via explicit SVC entry, a
  saved handler frame, SYSTEM-mode service execution and architectural SWI
  return. Other exception classes, multiply-long and
  halfword-multiply instructions, and BIOS boot remain absent.
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
interpreter supports typed ARM/Thumb SWIs for the six default HLE services
below; the explicit decompression opt-in adds the services listed in the same
section. It enters SVC at vector `0x08` without fetching BIOS bytes, frames
handler state, runs the service in SYSTEM mode and returns through full CPSR
restoration.
Other exception classes and BIOS instruction fetch remain unsupported. The
bounded behavior-only VBlank/timer-overflow IRQ dispatcher and return are
described below; they do not execute or reconstruct BIOS code. BIOS data reads
are governed by the policy below.

- Instruction fetch is allowed only from ROM, EWRAM and IWRAM. Fetching from
  I/O, palette, VRAM, OAM, BIOS or unmapped memory returns `NonExecutable`.
- RAM is executable but is **never AOT-translated**, so self-modifying RAM code
  is decoded freshly by the interpreter on every execution.
- The CPU is instruction-budgeted. There is no scheduler, cycle count, HBlank
  or drawing-period timing, or rendered-frame concept. The host-driven model
  derives a read-only `VCOUNT` line from a fixed per-line instruction budget and
  advances timer counters from the same nominal three-cycle instruction mapping
  (below); those mappings are approximations, not hardware scanline or cycle
  timing.

## Synthetic BIOS data policy (Design)

`Bus::new` stays fail-closed: data reads at `0x00000000..=0x00003FFF` return
`BiosAccess`. The owner-authorized explicit opt-in
`Bus::with_synthetic_bios_zero_fill` (equivalently
`Bus::with_bios_data_policy(cart, BiosDataPolicy::SyntheticZeroFill)`) returns
zero for `read8`, `read16` and `read32` across exactly that 16 KiB region, after
the usual halfword/word alignment checks. This deterministic zero fill is an
experimental fallback for bounded execution experiments; it is not authentic
BIOS data, supplies no instruction bytes and is not a claim about any Nintendo
BIOS or hardware behavior. Under both data policies, `fetch16`/`fetch32` from
BIOS return `NonExecutable`; under the default write policy a BIOS store returns
`BiosAccess`; and no other address or width is affected. The policy is host configuration, not guest state,
and is excluded from `Bus::state_digest`.

BIOS **stores** have a separate policy, `BiosWritePolicy`. The default `Strict`
reports `BiosAccess`; the explicit `RomWritesDropped` opt-in completes the store
with no effect, matching mask ROM. It never makes BIOS data readable or
executable and does not affect the Game Pak ROM window, whose stores keep
reporting `RomWrite`. The HLE copy services accept a BIOS destination only under
that opt-in.

A third policy, `RomWritePolicy`, covers stores into the Game Pak ROM window.
The default `Strict` reports `RomWrite`; the explicit `Dropped` opt-in models the
documented no-effect store of a ROM-only cartridge. It never writes cartridge
bytes and models no flash command engine. The backup window is governed by its
own policy, not by this one.

A fourth policy, `BackupPolicy`, covers the 64 KiB Game Pak backup window
(`0x0E00_0000..=0x0E00_FFFF`). The default `Strict` reports every access there
as `UnsupportedSave`. The explicit `Sram32KVolatile` opt-in
(`run --synthetic-backup-sram`) models a byte-wide, volatile 32 KiB SRAM that
starts erased (`0xFF`), is lost when the process ends and has no flash command
engine and no persistence. Wider accesses and the window beyond 32 KiB stay
explicit `UnsupportedSave` errors. This is a device model for bounded progress
experiments, not a save feature; the SRAM contents are guest state included in
`Bus::state_digest`, while the selected policy is host configuration and is not
hashed.

The CLI surfaces are `run --synthetic-bios-zero-fill`,
`run --synthetic-bios-rom-writes`, `run --synthetic-rom-writes-ignored`,
`run --synthetic-backup-sram` and `run --synthetic-bios-decompress` (all default
to strict). Successful output prints all five policy values
(`bios_data_policy`, `bios_write_policy`, `rom_write_policy`, `backup_policy`,
`decompression_policy`) plus an explicit non-authentic assumption line for each
selected opt-in; the optional snapshot report prints the five policy values, the
BIOS data/write notes and the decompression note. Tests use synthetic fixtures
only.

## BIOS HLE services and guest SWI framing

`gba_native::bios::call(service, &mut cpu, &mut bus)` exposes the six
documented, timing-independent default services as direct Rust primitives
derived from S1. Guest ARM/Thumb SWIs dispatch to those same services through
`bios::execute_swi`:

| SWI | Service | Boundary |
| --- | --- | --- |
| `01h` | `RegisterRamReset` | Clears selected memory groups 0..4, applies the specified group-6 sound effects and modelled group-7 I/O reset, always forces blank, and ignores bits 8..31; group 5 (SIO) clears the modelled serial registers and selects general-purpose mode |
| `06h` | `Div` | Signed truncated quotient, dividend-signed remainder, unsigned quotient magnitude; zero divisor and `MIN / -1` error before mutation |
| `07h` | `DivArm` | `Div` with the documented reversed r0/r1 argument order |
| `08h` | `Sqrt` | Floor unsigned integer square root of r0 |
| `0Bh` | `CpuSet` | Halfword/word copy or fill with prevalidated ranges |
| `0Ch` | `CpuFastSet` | Word copy or fill rounded up to 8-word blocks before range validation |

The default policy exposes exactly these six services. A separate host policy,
`DecompressionPolicy::DocumentedFormats` (`run --synthetic-bios-decompress`),
adds the documented decompression SWIs `11h`/`12h` (LZ77, byte/halfword stores),
`14h`/`15h` (run-length), `16h`/`17h` (8-bit diff) and `18h` (16-bit diff).
The HLE is rebuilt from the public GBATEK format descriptions; no BIOS byte or
code is read. It requires a 4-byte-aligned source, a header whose reserved
nibble and compressed type match the service, and a destination wholly inside
the policy's writable regions. Byte variants accept only EWRAM/IWRAM; halfword
variants additionally accept palette/VRAM/OAM but require a halfword-aligned
destination and an even decompressed size. The LZ77 halfword variant rejects
`disp=000h`, which GBATEK documents as not working for the Vram function.
Decode failures (invalid header/back-reference or an exhausted readable source)
are explicit errors and the output is built in a host buffer before the first
destination store, so a failed decode leaves the destination untouched. A
structurally valid stream that simply describes different bytes cannot be
detected without a checksum. Preconditions (policy, header, destination range)
are rejected before the SVC frame; a decode failure can occur after frame entry
but before any destination store. `10h` BitUnPack and `13h` Huffman remain unsupported. The default
`Strict` policy keeps every decompression SWI an explicit `UnsupportedService`
error.

The direct `bios::call` API does not consume an instruction, advance PC, enter
SVC/IRQ mode, write SPSR, fetch a vector, count cycles or model wait states. A
guest SWI consumes exactly one guest instruction and performs the framed SVC
path described below; service internals add no budget, cycle or wait-state
accounting. Every other service number is an explicit
`BiosError::UnsupportedService`; Halt/IntrWait/VBlankIntrWait, Stop/Sleep,
SoftReset/HardReset, BitUnPack and Huffman remain unimplemented, and the
LZ77/RL/diff services are unsupported unless the opt-in is selected. Unsupported
requests are never successful no-ops. See `docs/clean-room/hle-services.md` for exact
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
in. HLE handler internals have no cycle, scheduler or wait-state accounting. A
bus failure after exception entry is terminal and is not rolled back.

### Bounded IRQ dispatcher and return (partial; Design)

The host-driven instruction-count frame model raises the VBlank request once when
line 160 of its 228-line `VCOUNT` progression is entered (see the `VCOUNT` model
under Bus). Between guest instructions, the runner enters a handler only when
IRQs are unmasked, IME and a matching IE/IF bit are set, and the nonzero handler
pointer at `0x03007FFC` is present. The pointer must be word-aligned and
executable ARM memory; invalid pointers fail explicitly before CPU or bus
mutation. A six-word full-descending frame must fit in aligned EWRAM or IWRAM
before entry begins.

The behavior-only dispatcher saves ascending-address words `{r0,r1,r2,r3,r12,
LR_irq}` at `SP_irq-24`, where the saved IRQ link is the interrupted PC + 4. It
saves the old CPSR in `SPSR_irq`, enters ARM IRQ mode with I set and F preserved,
sets the IRQ stack pointer to the frame and calls the handler with `LR_irq` set
to the modeled continuation `0x00000138`. A guest handler acknowledges IF and
returns with `BX LR`. Only IRQ-mode ARM execution at `0x00000138` is recognized
as the dispatcher epilogue: it restores the six saved values and IRQ SP, then
restores the interrupted CPSR and resumes at saved `LR_irq-4`. It performs this
as one modeled runtime step and never fetches or synthesizes BIOS instructions.
Entry consumes no guest instruction budget; guest-handler instructions and the
modeled epilogue each consume one.

This behavior-only model follows the public BIOS interrupt-handling contract;
it is not BIOS execution or a claim to reproduce BIOS bytes. Three request
sources are generated: the VBlank request once per frame at the modelled
line-160 transition, modelled timer overflow requests on `IF` bits 3..6, and the
V-Counter match request on `IF` bit 2 (see the `VCOUNT` model below). The
`0x03007FF8` interrupt-check flag, IRQ nesting, FIQ, serial/keypad/DMA IRQs,
HBlank IRQs, cycle timing and BIOS boot remain unsupported. Do not interpret
this as general GBA interrupt compatibility.

## Bus

Implemented regions and widths:

| Region | Address | Size | Reads | Writes |
| --- | --- | --- | --- | --- |
| EWRAM | `0x02000000` | 256 KiB | 8/16/32 | 8/16/32 |
| IWRAM | `0x03000000` | 32 KiB | 8/16/32 | 8/16/32 |
| I/O | `0x04000000` | implemented registers only | register-specific 8/16/32 | register-specific 8/16/32 |
| Palette | `0x05000000` | 1 KiB | 8/16/32 | 8/16/32 |
| VRAM | `0x06000000` | 96 KiB | 8/16/32 | 8/16/32 |
| OAM | `0x07000000` | 1 KiB | 8/16/32 | 8/16/32 |
| ROM | `0x08000000`, `0x0A000000`, `0x0C000000` | 32 MiB windows over the same bytes | 8/16/32 | none |

- The three ROM windows alias the same immutable cartridge bytes via a 32 MiB
  offset mask. Writes return `RomWrite`; reads past the image return
  `RomOverread`. ROM bytes cannot be mutated through the bus.
- Work RAM mirrors its bank across the documented 16 MiB window: 256 KiB EWRAM
  over `0x02000000..=0x02FFFFFF` and 32 KiB IWRAM over `0x03000000..=0x03FFFFFF`
  (`#gba-memory-map`). By explicit prototype policies recorded in D22/D26, the
  1 KiB palette array aliases across `0x05000000..=0x05FFFFFF`, and VRAM uses a
  repeating 128 KiB aperture across `0x06000000..=0x06FFFFFF`. In tiled modes
  0..=2 the aperture's `0x18000..=0x1FFFF` hole aliases the last physical 32 KiB;
  that hole remains unmapped in bitmap modes. These test-driven extensions
  satisfy local black-box mirror cases, while GBATEK labels addresses outside
  the physical palette/VRAM regions unused; they are not hardware-map claims.
  OAM has one additional narrow prototype alias at
  `0x07000400..=0x070007FF`, mapping to its first 1 KiB; the following addresses
  remain unmapped. This test-driven extension satisfies a local black-box mirror
  case while GBATEK labels the alias unused, and is not a hardware-map claim.
  Other unmapped reads remain explicit errors and no open-bus behavior is claimed.
- Alignment: 16-bit bus accesses require even addresses and 32-bit accesses
  require 4-byte alignment. I/O uses register-specific 8/16-bit access, and a
  32-bit access composes two halfword accesses (including across register pairs).
- Implemented I/O: DISPCNT halfword read/write; BG0CNT (`0x04000008`)
  halfword read/write; BG0HOFS/BG0VOFS (`0x04000010`/`0x04000012`) halfword
  writes; KEYINPUT halfword read (`0x04000130`); WAITCNT (`0x04000204`) halfword
  read/write, retaining only documented bits 0..13; and the four DMA channels
  `0x040000B0..0x040000DF` (see below). The host injects a `u16`
  pressed mask through `Bus::set_pressed_keys`: bits 0..9 are A, B, Select,
  Start, Right, Left, Up, Down, R and L, respectively; unused bits are ignored.
  Guest reads see active-low KEYINPUT (`0x03FF` by default, all released), while
  guest writes have no effect. This host setter provides no frame scheduler,
  sampling guarantee or timing claim. BG0 scroll reads are
  write-only-register `UnsupportedIoRead` errors. Scroll writes retain only
  documented bits 0..8; unused upper bits have no state effect.
  A 32-bit I/O access is composed from two halfword accesses, as on hardware.
- I/O write policy: a write that no modelled register claims is **ignored and
  recorded** (`Bus::ignored_io_writes`); the first 4096 distinct addresses are
  retained, while `ignored_io_write_count` and `ignored_io_writes_truncated`
  report total operations and whether the sample overflowed. This matches the
  hardware behaviour for unused I/O space and for the unused
  `0x04000400..=0x04FFFFFF` window. Reads of unmodelled registers stay explicit
  errors, because open bus is not modelled.
  `KEYINPUT` is read-only: a write is accepted with no effect. This keeps the
  set of silently-dropped addresses small, inspectable and out of the digest.
  Mutable display-register and dynamic KEYINPUT state are included in
  `Bus::state_digest`. Sound registers remain storage-only apart from bounded
  Sound A/B FIFO queues; timer counters, reload latches, controls and prescaler
  phases are part of the instruction-budget timer model (below). There is no
  save-register state, no BG affine parameter state, and no serial shift engine.
  The serial registers `SIOCNT`/`RCNT`/`SIODATA*` are documented storage only: a
  transfer started with no link partner finishes immediately with SI read as
  high and received bits all ones, because that is the documented state of a
  disconnected port. A normal-mode start with the internal shift clock is
  answered that way; an externally clocked start stays pending with the start
  bit set, because no partner supplies SC edges. Multiplay and UART starts stay
  explicit `UnsupportedIoWrite` errors instead of fabricated transfers, and
  general-purpose/JOY-BUS mode treats `SIOCNT` as unused storage. No serial IRQ
  is delivered.
- `VCOUNT` and the instruction-count line model: the host-driven frame model
  exposes a read-only `VCOUNT` halfword at `0x04000006` with an 8-bit path for
  exactly that register: `0x04000006` returns its low byte and `0x04000007` its
  high byte. `VCOUNT` progresses through all 228 documented lines `0..=227` and
  wraps from 227 to 0 once per frame, so the high byte is always `0` and the
  halfword value is unchanged by the byte path. The total frame budget stays
  `FRAME_INSTRUCTIONS` = 93,632 guest instructions (280,896 documented cycles at
  a nominal three cycles per instruction). GBATEK `#lcd-dimensions-and-timings`
  gives 228 equally long lines of 1232 cycles, so the budget is spread evenly:
  each line owns `93632 / 228 = 410` instructions, and the 152-instruction
  remainder is distributed one instruction at a time across the frame
  (`bus::line_instructions`, `bus::instructions_until_line`). The 228 line
  budgets therefore sum to exactly 93,632 with no lost remainder. A fresh bus
  starts on line 0 with that line's full budget ahead; `Bus::tick_frame`
  advances one guest instruction and `Bus::tick_frames` produces exactly the
  same sequence as repeated single ticks. The DISPSTAT VBlank flag (bit 0) is
  set on lines `160..=226` and clear on line 227, matching GBATEK
  `#lcd-io-interrupts-and-status` ("set in line 160..226; not 227"); the VBlank
  IRQ request latches once when line 160 is entered. This replaces the earlier
  two-value `0`/`160` placeholder and its undocumented one-tenth VBlank phase.
  **Design approximation, not a timing claim:** a line is an instruction-count
  slice, not hardware timing. One guest instruction is not three cycles, lines
  do not model drawing or HBlank periods, the 152 remainder instructions are
  spread evenly instead of at their exact cycle positions, and no instruction
  observes a mid-line split. The DISPSTAT V-Count setting (bits 8..15) is
  writable and compared against the current line: GBATEK
  `#lcd-io-interrupts-and-status` says "when its value is identical to the
  content of the VCOUNT register then the V-Counter flag is set (Bit 2), and
  (if enabled in Bit 5) an interrupt is requested". The model implements
  exactly that: the read-only match flag (bit 2) reads set while they are
  equal, and with the V-Counter IRQ enable (bit 5) a line transition into the
  matching line latches the V-Counter request in `IF` bit 2. A write that makes
  the setting match the current line raises the request immediately; the cited
  section states the flag/request condition for `LYC == VCOUNT` but not when a
  write is evaluated, so this immediate evaluation is an implementation choice,
  not a quoted rule. All `LYC` values `0..=255` are accepted, but only the
  modelled lines `0..=227` can match. There is no HBlank, per-line interrupt
  scheduling beyond this match, wait-state/cycle accounting or scanline
  renderer. The
  pre-existing byte-addressed sound-register reads are unchanged, every other
  I/O address keeps the existing register-specific dispatch, and
  otherwise-unmodelled reads remain explicit `UnsupportedIoRead { width: 8 }`
  errors, including the neighbouring `DISPSTAT` lanes and the byte above
  `VCOUNT`. Byte writes to `VCOUNT` remain ignored-and-recorded like any other
  unmodelled I/O write. The DISPSTAT V-Counter match model is the one described
  above; no other per-line interrupt source exists.

### Timers

`TM0..TM3` expose the documented 16-bit counter/reload halfword and control
halfword at `0x04000100 + 4*n`. Each retired guest instruction contributes
`bus::INSTRUCTION_CYCLES` = 3 nominal cycles to every **enabled** timer. A normal
timer increments whenever its selected prescaler (`F/1`, `F/64`, `F/256`,
`F/1024`) accumulates that many cycles; a cascaded Timer 1..3 ignores its own
prescaler and increments once for each previous-timer overflow. Cascades are
evaluated in timer order, so an overflow can propagate through consecutive
timers in one nominal cycle. Disabled timers do not advance.

The documented latch behavior is implemented. Writing `TMxCNT_L` changes only
the reload value and never the current counter. Reading `TMxCNT_L` returns the
current counter, or the frozen counter after disable. The reload value is copied
to the counter on overflow and on a `0 -> 1` start edge in `TMxCNT_H`. A 32-bit
I/O write is composed low halfword first, so a simultaneously written reload is
the value used by the start edge. A stop freezes the counter; a later start
reloads it. Changing only the prescaler or count-up selection on a running timer
keeps the counter but discards the partial prescaler phase.

Prescaler bits `0..1`, the count-up bit `2`, the overflow-IRQ bit `6` and the
start bit `7` are modelled. Reserved control bits are stored as zero. Timer 0's
count-up bit is documented as not used: it is accepted, ignored and reads back
as zero, while Timer 1..3 use it for count-up. With the IRQ-enable bit set, an
overflow latches the matching `IF` bit (3..6); the existing `IME`/`IE` gate and
dispatcher decide delivery. Without that bit the overflow still reloads and
still drives a cascade, but raises no request.

**Design approximation, not a timing claim:** instruction lengths, waitstates,
bus arbitration and mid-instruction reads are absent. A poll observes only
completed instruction ticks. Timers do not consume Sound FIFO samples, select or
start Sound DMA, request FIFO refills, drive `VCOUNT`, HBlank, video capture or
any other peripheral. `SOUNDCNT_H` timer-select bits remain storage-only, and the
documented `VCOUNT`-related timing sections are not used to derive prescaler
phase. RegisterRamReset group 7 returns all four timers to their stopped, zeroed
default state. Timer access stays halfword-oriented: an 8-bit timer read remains
the existing explicit `UnsupportedIoRead { width: 8 }`, and an 8-bit timer write
follows the bus-wide ignored-and-recorded policy for unmodelled byte I/O.

### DMA channels

`DMA0..DMA3` expose SAD, DAD, CNT_L and CNT_H at `0x040000B0 + 12*n`, as 16-bit
registers plus 32-bit reads/writes over the address pairs. SAD, DAD and CNT_L
keep their written values, matching the documented hardware note that the
transfer uses internal pointers; a count of zero means the channel maximum
(`0x4000`, or `0x10000` for DMA3). Destination and source control support
increment, decrement and fixed addressing; source control 3 is rejected because
the documented encoding prohibits it. Immediate-timing transfers run on the
0->1 enable edge and clear enable after a non-repeating transfer. VBlank/HBlank
channels remain armed; this model provides no corresponding event.

For DMA1/2, Special timing is modeled only as an explicit Sound FIFO request:
`Bus::request_sound_fifo_dma(SoundFifo::A|B)` selects an enabled channel by its
latched DAD address, without assuming a fixed DMA1/A or DMA2/B pairing. The
supported shape requires repeat, incrementing source and fixed FIFO destination;
the source must be a 16-byte range in EWRAM or IWRAM. The destination is captured
on the enable edge, so later writes to the visible DAD latch do not redirect an
armed transfer. A request reads four 32-bit words into the selected 32-byte
FIFO, advances only the internal source cursor by 16 bytes while keeping the
FIFO destination fixed, and does not itself rewrite visible SAD, DAD or CNT_L.
Special-mode count and width are ignored. Unsupported configurations fail
before source access;
DMA0 Special timing and DMA3 video capture are unsupported rather than treated
as immediate transfers.

**Stated limitations.** The two 32-byte FIFO queues accept 32-bit writes and
support explicit host consumption of one signed sample. `SOUNDCNT_H` FIFO reset
bits clear their corresponding queues; RegisterRamReset group 6 clears both and
group 7 preserves them. No sample is consumed automatically and no DMA request
is generated from the documented 16-byte refill threshold, a timer, a frame or
audio output. There is no DMA arbitration, scanline/cycle timing, automatic
VBlank/HBlank trigger, Game Pak DRQ, video-capture transfer or DMA-driven IRQ.
- The backup window (`0x0E000000..=0x0E00FFFF`) is fail-closed by default and
  only answers under the explicit `BackupPolicy::Sram32KVolatile` opt-in (see
  the backup policy above). BIOS reads/writes and unmapped addresses are errors;
  the only exception is the explicit synthetic zero-fill opt-in, which changes
  BIOS **data reads** to zero and leaves BIOS fetch/write errors and every other
  address unchanged.
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
would be restricted. Multiply-long, halfword multiply, coprocessor, ARMv5
`BLX`/`BXJ` and other newer encodings are rejected with address/state/raw word
and are never NOPs.
Typed SWI is handled only by the bounded default-service path above (plus the
explicit decompression opt-in); unsupported service numbers and SVC-origin
nesting are explicit errors.

| Family | Implemented | Rejected |
| --- | --- | --- |
| Data processing | all sixteen operations: `AND`, `EOR`, `SUB`, `RSB`, `ADD`, `ADC`, `SBC`, `RSC`, `TST`, `TEQ`, `CMP`, `CMN`, `ORR`, `MOV`, `BIC`, `MVN`; register or rotated-immediate operand; LSL/LSR/ASR/ROR/RRX shifted register by immediate or by register; `S` honored; PC operands, `S=0` PC destinations and the documented `S=1, Rd=PC` exception return | `S=1` with `Rd=PC` in USER/SYSTEM (no SPSR) or with an invalid saved mode, test-op encodings with nonzero `Rd` (SBZ; nonzero is UNPREDICTABLE), ARMv5/newer |
| Branch | conditional/unconditional `B`; `BL` (LR = address+4); `BX` register, including the documented ARM7 `BX PC` behavior | `BLX` register/immediate, `BXJ` |
| Word/byte load/store | `LDR`/`STR`/`LDRB`/`STRB` with immediate-12 or shifted-register offset, pre/post indexing, add/subtract, pre-index writeback, post-index `T` on the flat bus, documented PC base/source offsets and word `LDR pc` alignment | PC base with writeback, load `Rd=Rn` with writeback, byte load into PC, offset register PC or reserved bit 4 set |
| Halfword/signed | `LDRH`/`STRH`/`LDRSB`/`LDRSH` with immediate or unshifted register offset, pre/post indexing and writeback; ARM7 odd-address rules | `LDRD`/`STRD` (ARMv5), narrow loads into PC, PC base with writeback, load alias with writeback, register-form reserved bits 11-8 nonzero |
| Block | `LDM`/`STM` IA/IB/DA/DB with non-empty register lists, optional writeback, ARMv4 base-in-list specials, PC load/store entries | `S=1` user-bank/SPSR transfer, empty list, PC base |
| PSR transfer | `MRS` CPSR/SPSR; `MSR` CPSR/SPSR with documented field bytes, register or rotated imm8 source | non-SWI exception entry/vectoring, MSR control writes that change T or use an invalid mode, PC destination |
| Software interrupt | ARM and Thumb SWI with typed comment metadata; framed SVC entry, six default-service HLE dispatch (plus the explicit decompression opt-in) and full `MOVS PC,LR` return | unsupported service numbers, nested SVC-origin SWIs |
| Multiply | `MUL`/`MLA` register form, `S` honored, low 32 bits of the product; unpredictable `MUL`/`MLA Rd=Rm` uses the explicit D21 design policy | R15 operands, `UMULL`/`UMLAL`/`SMULL`/`SMLAL`, halfword multiplies |
| Swap | ARM `SWP`/`SWPB`; `Rd=Rm` is allowed; unaligned word SWP rotates the loaded value by `8 * address[1:0]` but stores the source unrotated; SWPB zero-extends the loaded byte; destination and PC update only after both bus accesses succeed | R15 operands, `Rn=Rd` or `Rn=Rm` (UNPREDICTABLE), nonzero SBZ bits [11:8] |
| Everything else | none | multiply-long/halfword multiply, coprocessor, unconditional/newer space |

`MUL`/`MLA` set N/Z from the result and leave V unaffected. GBATEK documents C
as "destroyed" on ARMv4, which has no defined value; this prototype preserves
the previous C bit as a stated **Design** choice, so no test may depend on C
after a multiply. `MUL Rd=Rm` is architecturally unpredictable on ARM7TDMI; this
prototype explicitly reads both pre-instruction operands and writes their low
32-bit product. For `MLA Rd=Rm`, the accumulator is also read from the
pre-instruction state before the low 32-bit sum is written. These are
compatibility-oriented design extensions, not hardware-behavior claims.

`SWP`/`SWPB` use the ARMv4T forms specified by DDI0100E §4.1.51–52. Address
and source registers are snapshotted before the bus accesses, which preserves
the documented `Rd=Rm` case. A bus error on either access leaves `Rd` and PC
unchanged. The instruction is one indivisible guest step in this non-timed
runtime; cycle-accurate bus arbitration and mid-instruction DMA are not modeled.

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
- Block/stack base alignment is a **Design** policy: GBATEK says low address
  bits are ignored for LDM/STM/PUSH/POP. This prototype aligns only the transfer
  address (`base & !3`) before applying the addressing mode; writeback, and the
  PUSH/POP SP update, are always computed from the original, unmodified base
  register. The public document does not establish that writeback source, so
  this is not a hardware-conformance claim. The same policy applies to Thumb SP
  and `LDMIA`/`STMIA` bases.
- Thumb implements word/byte register-offset forms, half/signed
  register-offset forms, immediate word/byte transfer forms, the existing
  immediate halfword form, SP-relative word transfers and the preserved literal
  PC-relative load. `PUSH`/`POP` use full-descending SP with ascending memory
  order and the LR/PC bit; `POP PC` clears bit 0 and keeps Thumb state.
  `LDMIA`/`STMIA` use the ARMv4 base-in-list special documented for Thumb:
  STM stores the old base if it is the first entry, otherwise the new base;
  LDM's loaded base wins. For the documented ARMv4 empty-list quirk, empty
  `LDMIA`/`STMIA` transfers R15 as the sole word and writes `Rb+0x40`; empty
  `PUSH`/`POP` remain explicit errors. Empty-list LDM loads an even-aligned PC
  and retains Thumb state; empty-list STM stores the instruction address plus 6,
  matching the following Thumb instruction's visible PC as checked by ROM test
  229.
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
- **Boundary:** the documented SWI-to-HLE path performs exception entry, a
  bounded saved SVC frame and its SWI return. The behavior-only VBlank and
  timer-overflow IRQ model above saves the documented IRQ frame and models the
  handler return without
  BIOS execution, covering the modelled VBlank and timer-overflow requests.
  ARM data-processing `S=1, Rd=PC` can restore an already existing valid SPSR
  but performs no exception entry. Other exception
  classes/vectoring and BIOS execution remain unsupported.

Pipeline offsets are documented. ARM operands reading R15 use
`instruction address + 8`, or `+12` when the instruction uses a
register-specified shift. Thumb operands reading R15 use `instruction address +
4`; `ADR` and `ADD Rd,PC,#imm` address generation use the word-aligned PC+4
value. Direct branch targets are forced to 4-byte (ARM) or 2-byte (Thumb)
alignment.

ARM ALU/MOV writes to PC with `S=0` are allowed, force-align the low two bits
and keep the ARM state. The documented `S=1` with `Rd=PC` exception return
restores CPSR from the current mode's SPSR and uses the computed result as the
PC aligned for the restored T bit; ordinary ALU NZCV results are discarded
because the restored CPSR replaces them. A missing SPSR in USER/SYSTEM or an
invalid saved mode errors before any state mutation. This restores existing CPU
state only: it does not perform exception entry or fetch BIOS code. Thumb
`MOV PC,Rs`/`ADD PC,Rs` force-align the low bit and keep Thumb state. `BX` bit 0
is the only other state switch. `BX PC` follows GBATEK's ARM7 behavior (ARM
`$+8`; Thumb `(($+4) AND NOT 2)`, switching to ARM); GBATEK notes that this case
is not officially defined as predictable.

## Implemented Thumb subset

| Family | Implemented | Rejected |
| --- | --- | --- |
| Move shifted | `LSL`/`LSR`/`ASR` immediate, flags set | — |
| Add/subtract | `ADD`/`SUB` register and 3-bit immediate, flags set | — |
| Immediate | `MOV`, `CMP`, `ADD`, `SUB` with 8-bit immediate | — |
| ALU | `AND`, `EOR`, `LSL`, `LSR`, `ASR`, `ADC`, `SBC`, `ROR`, `TST`, `NEG`, `CMP`, `CMN`, `ORR`, `BIC`, `MVN`, `MUL`; shift amount from register low byte | — |
| High register | documented high-register `ADD`, `CMP`, `MOV` (at least one of `Rd`/`Rs` high) and `BX` including PC; low/low `MOV`/`CMP` as an explicit **Design** extension | low/low `ADD` (documented reserved encoding), ARMv5 `BLX` register |
| Address | `ADR`, `ADD Rd,PC,#imm`, `ADD Rd,SP,#imm`, `ADD`/`SUB SP,#imm`; SP-relative word `LDR`/`STR` | — |
| PC-relative | `LDR` literal | — |
| Word/byte | register-offset `LDR`/`STR`/`LDRB`/`STRB`; immediate-offset word/byte forms | — |
| Halfword/signed | immediate `LDRH`/`STRH`; register-offset `LDRH`/`STRH` and sign-extended `LDSB`/`LDSH` | ARMv5/newer encodings outside these formats |
| Branch | conditional `B` (`cond != 14/15`), unconditional `B`, `BL` as two half instructions; typed SWI to the six default HLE services (the decompression opt-in is not wired into AOT) | ARMv5 `BLX` second half, unsupported SWI services, undefined `cond=14` |
| Block/stack | `PUSH`/`POP` with low list plus LR/PC; `LDMIA`/`STMIA` low-register lists with writeback and documented base-in-list specials | empty lists, ARMv5/newer |

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
