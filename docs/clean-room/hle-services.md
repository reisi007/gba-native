# Independently implemented BIOS HLE services and guest SWI path

Status: six documented Rust HLE primitives are callable directly and from typed
ARM/Thumb guest SWIs through a bounded SVC entry/frame/return path. No BIOS is
fetched or booted; the implementation is not a general exception subsystem and
does not establish playability. `README.md` and `gba-contract.md` state the same
boundary.

Decision background: **D8** in `docs/decisions.md` chooses documented BIOS HLE
derived from public interface/hardware documentation instead of a bundled or
disassembled Nintendo BIOS. D9 (GPU presentation) is unrelated future work.

## Source anchor and identity

The service interface and behavior are derived from the public GBATEK document
already registered as **S1** in `docs/clean-room/sources.md`. It is a hardware
interface reference, not an emulator implementation source.

- Retrieved 2026-10-25 from <https://mgba-emu.github.io/gbatek/>.
- Retrieved identity: 5,473,181 bytes, SHA-256
  `a701cf0f5be47b23dd31298d7dc593a7cccb3ae9a59087dea940305565025b70`.
- Consulted anchors:
  - `#bios-function-summary`
  - `#bios-arithmetic-functions`
  - `#swi-06h-gba-or-swi-09h-nds7nds9dsi7dsi9---div`
  - `#swi-07h-gba---divarm`
  - `#swi-08h-gba-or-swi-0dh-nds7nds9dsi7dsi9---sqrt`
  - `#bios-memory-copy`
  - `#swi-0bh-gbands7nds9dsi7dsi9---cpuset`
  - `#swi-0ch-gbands7nds9dsi7dsi9---cpufastset`
  - `#bios-reset-functions`
  - `#swi-01h-gba---registerramreset`
  - `#bios-halt-functions` (explicitly not implemented here)
  - `#how-bios-processes-swis`, `#gba-bios-ram-usage` and
  `#arm-cpu-exceptions` for SWI comments, four handler-save values, SYSTEM-mode
  service execution, stack tops, vector and exception return.

The ARM7TDMI Technical Reference Manual (S6 in `sources.md`) corroborates the
ARM/Thumb SWI resume-address rule, vector `0x08`, SVC entry flags and
`MOVS PC,R14_svc` return. GBATEK does not specify the ascending-address order of
the four BIOS-saved words; the slot order below is a bounded implementation
choice, not a hardware-compatibility claim.

No emulator implementation, game decompilation, BIOS disassembly, leaked SDK or
REA Evidence was used. The algorithms and test vectors here are independently
derived from the documented interface.

## API and layering boundary

```rust
pub fn bios::call(service: u8, cpu: &mut Cpu, bus: &mut Bus) -> Result<()>;
```

`call` interprets the supplied service number directly. It is a service
primitive, not a CPU exception:

- It does not consume an instruction or advance `PC`.
- There is no instruction budget, cycle count or hardware timing.
- It does not enter SVC mode, switch to SYSTEM, touch a banked stack, write an
  SPSR, change CPSR/T/NZCV, or fetch from a vector.
- It reads/writes only the registers documented as service arguments/results
  and the bus memory the service is defined to touch.
- Registers outside those documented outputs are preserved as an explicit
  **Design** choice; this is not a claim about a particular retail BIOS.
- Every unsupported, waiting or invalid request is an explicit `Error`; none is
  a successful no-op.

The direct API above remains useful to host-side tests and tools. The guest
instruction path is separate: `bios::execute_swi(comment, thumb, addr, cpu,
bus)` validates the call, enters SVC, saves a bounded handler frame, switches to
SYSTEM for the HLE call, restores the frame and returns with full CPSR state.

## Guest SWI/SVC framing

- ARM service selection uses comment bits 23..16; Thumb uses the encoded low
  byte. Unknown services return `BiosError::UnsupportedService`.
- A supported non-SVC caller enters ARM SVC mode. The old CPSR is placed in
  `SPSR_svc`; `LR_svc` receives ARM instruction address + 4 or Thumb address + 2;
  IRQ is masked, FIQ is preserved, and PC is set to vector `0x08`. No BIOS bytes
  are fetched.
- The handler frame occupies 16 aligned bytes below the pre-entry SVC SP and
  must fit entirely in EWRAM or IWRAM. Misaligned, underflowing or out-of-range
  frames fail before mutation. The handler executes the HLE service in SYSTEM
  mode, then returns to SVC, restores the saved values and performs
  `MOVS PC,LR` semantics to restore the full interrupted CPSR and resume PC.
- **Bounded design policy:** ascending word addresses from `SVC_SP - 16` hold
  `(SPSR, R11, R12, R14)` in that order. GBATEK identifies the values but not
  their slot order, so this does not claim to reproduce a Nintendo BIOS's exact
  stack layout. The SVC SP is restored to its pre-entry value on success.
- A supported SWI originating in SVC mode fails with `SwiNesting` before
  mutation; a nested entry cannot safely preserve the already-live `SPSR_svc`
  and `LR_svc` in this bounded model. Known service preconditions are validated
  before exception entry. An unexpected bus failure after entry is terminal and
  is not rolled back.
- The SWI consumes one guest instruction. Service and handler internals do not
  consume further instruction budget, model IRQ delivery, or account for cycles,
  wait states, scheduling or frames.
- `Cpu::direct_entry()` does not implicitly initialize BIOS stacks.
  `Cpu::initialize_bios_stacks()` sets USER/SYS SP `0x03007F00`, IRQ SP
  `0x03007FA0` and SVC SP `0x03007FE0`; the guest CLI runner and generated AOT
  harness call it explicitly.

## Implemented services

| SWI | Service | Registers | Implemented behavior |
| --- | --- | --- | --- |
| `01h` | `RegisterRamReset` | r0 flags | Clears the selected documented memory regions and always forces blank. |
| `06h` | `Div` | r0 numerator, r1 denominator | Signed truncated quotient in r0, dividend-signed remainder in r1, unsigned quotient magnitude in r3. |
| `07h` | `DivArm` | r0 denominator, r1 numerator | Same as `Div` with the documented reversed argument order. |
| `08h` | `Sqrt` | r0 unsigned input | Floor unsigned integer square root in r0, range 0..=65535. |
| `0Bh` | `CpuSet` | r0 source, r1 destination, r2 control | 16-bit or 32-bit copy/fill with documented control bits. |
| `0Ch` | `CpuFastSet` | r0 source, r1 destination, r2 control | 32-bit copy/fill with GBA 8-word upward rounding. |

### Division policy

The result is computed completely before any register write. Zero divisors
return `BiosError::ZeroDivisor`; `i32::MIN / -1` returns
`BiosError::DivisionOverflow`. Both leave r0/r1/r3 and all other registers
unchanged. The overflowing edge deliberately does **not** produce a wrapped
quotient, and no `abs()` panic path exists. Rust's checked signed division
truncates toward zero and keeps the dividend's sign on the remainder, matching
the documented `-1234 / 10 -> -123, -4, +123` example. No floating point is used.

### Square-root policy

`Sqrt` uses a restoring binary integer algorithm with `u64` intermediates. It is
bit-exact floor semantics: `root² <= input < (root + 1)²` in 64-bit arithmetic.
Only r0 changes.

### CpuSet (`0Bh`) policy

- Control mask: `0x051FFFFF` (count bits 0..20, fill bit 24, word bit 26).
  Any other bit is `BiosError::ReservedControl` before memory is touched.
- Unit size is 16-bit unless bit 26 is set. Source and destination must be
  aligned to 2 or 4 bytes; violations return `BiosError::Misaligned`.
- Count zero is a no-access success after the control and alignment checks.
- The expanded `units * width` range is checked before the first bus access.
  Overflow returns `RangeOverflow`; a range that leaves its documented bus
  region or crosses into another one returns `RangeOutsideRegion`.
- Source regions are the full 16 MiB EWRAM/IWRAM mirror windows, palette, VRAM,
  OAM and the three ROM windows. Crossing physical RAM-bank mirror seams within
  one address window is valid; crossing from EWRAM to IWRAM is not.
  I/O, save space and unmapped addresses are `UnsupportedSource`. A source
  range reaching the 16 KiB BIOS area returns `Error::BiosAccess` instead of
  being read or silently rejected as success.
- Destination regions are the full 16 MiB EWRAM/IWRAM mirror windows, palette,
  VRAM and OAM. ROM, I/O, save
  space, BIOS and unmapped destinations are `UnsupportedDestination`; the whole
  unsupported range is rejected rather than partially attempted.
- Fill reads exactly one source unit and writes it repeatedly. Copy reads and
  writes ascending units; forward overlap therefore smears and is not a
  snapshot `memmove`.
- ROM sources are checked against the actual cartridge length before mutation.
- Validation is pre-effect. If an unexpected bus error occurs after the first
  successful access, completed writes remain: the transfer is fail-closed but
  not transactional or atomic. With the current bus implementation the
  prevalidation above makes such a mid-transfer backend error unreachable for
  these documented regions; the rule defines behavior for future bus extensions.

### CpuFastSet (`0Ch`) policy

- Control mask: `0x011FFFFF` (count bits 0..20, fill bit 24). Bit 26 is reserved
  for this service and is rejected even though `CpuSet` uses it.
- Source and destination must be 4-byte aligned.
- The requested word count is rounded **up** to a multiple of 8 words: 1 word
  writes 8, 8 writes 8, 9 writes 16, and 0 remains a no-access success.
- Range validation happens after rounding, so a request that fits before
  rounding but crosses the region afterward fails before mutation.
- Fill reads one source word and replicates it; copy is ascending and smears on
  forward overlap. Range, BIOS-source and destination policies match `CpuSet`.

### RegisterRamReset (`01h`) policy

- Implemented flags: bit 0 EWRAM (256 KiB), bit 1 IWRAM (all but the last 512
  bytes), bit 2 palette (1 KiB), bit 3 VRAM (96 KiB), bit 4 OAM (1 KiB).
- IWRAM `03007E00h..03007FFFh` is preserved.
- GBATEK documents bit 5 as resetting SIO registers and switching to general-
  purpose mode, bit 6 as resetting sound registers, and bit 7 as resetting all
  other registers except SIO and sound. All three are accepted. Group 5 has no
  modeled serial state. Group 6 applies only the sufficiently specified sound
  effects: `SOUNDCNT_L := 0`, `SOUNDCNT_H := 0x880E`, `SOUNDCNT_X := 0`, clears
  the upper SOUNDBIAS bits while preserving the low 10, clears wave RAM, and
  empties both modeled Sound FIFOs.
  Other sound bytes remain unchanged because their end values are ambiguous.
  Group 7 clears exactly the modeled "other I/O" set: BG0 registers, WAITCNT,
  DMA, DISPSTAT/IE/IF/IME and timer registers. `DISPCNT` is deliberately outside
  group 7: the service forces blank separately and that value survives.
- Regressions compare the full bus digest: group 7 clears its modeled set while
  preserving sound; group 6 applies known effects while preserving group-7
  state and unresolved sound bytes. Any newly modeled register must be assigned
  to the correct reset group and added to these expectations.
- Bits 8..31 carry no documented meaning and are ignored, matching the
  documented service rather than rejecting the call.
- GBATEK also notes that the SIO `SIODATA32` least-significant bits are destroyed
  even when bit 5 is clear. The current bus has no `SIODATA32` state, so this
  side effect is not fabricated.
- Every request writes `DISPCNT=0080h`, including flags zero, so a display
  snapshot becomes forced-blank white.
- Clears use 16-bit or 32-bit bus writes, not byte writes. OAM zeroing is not a
  claim that objects are disabled; DISPCNT bit 6 and OAM attribute state are
  separate.
- Sound register storage remains non-audio state; no audio is generated. Serial
  state and the documented unconditional `SIODATA32` side effect remain
  unmodeled; no save/persistence behavior is claimed.

## Deliberately unimplemented

The following are explicit `BiosError::UnsupportedService` values, never spins
or silent successes:

- `00h` SoftReset and `26h` HardReset (no CPU reset/reboot model here).
- `02h` Halt, `04h` IntrWait and `05h` VBlankIntrWait: IRQ/frame state is
  modelled, but these wait-service semantics are not implemented.
- `03h` Stop/Sleep and `27h` CustomHalt.
- `09h`/`0Ah` ArcTan/ArcTan2, `0Dh` GetBiosChecksum, affine setup and every
  other service not listed above.
- The decompression family (`10h`..`18h`) is future work; no fake handlers are
  installed.
- BIOS bytes and BIOS boot remain absent. The runtime has a bounded,
  behavior-only VBlank IRQ dispatcher frame and handler return; it never fetches
  or reconstructs BIOS instructions. Other exception paths, IRQ nesting, FIQ,
  unsupported SWI services and all timing-dependent services remain explicit
  errors or unsupported behavior.

## Tests and evidence boundary

`crates/core/tests/bios_arithmetic.rs`,
`crates/core/tests/bios_memory_copy.rs`,
`crates/core/tests/bios_reset.rs` and
`crates/core/tests/bios_dispatch.rs` use synthetic RAM/ROM
fixtures only. Their expected values are written independently of the
implementation:

- Arithmetic: the documented division example, a signed-division matrix, the
  zero-divisor and `i32::MIN / -1` error paths, and an integer-square-root
  property.
- Memory copy/fill: a literal CpuFastSet rounding table for requested counts
  `0, 1, 7, 8, 9, 15, 16, 17, 23, 24, 25` with independently stated expected
  word counts `0, 8, 8, 8, 16, 16, 16, 24, 24, 24, 32`, a sentinel word at
  every rounded end, fill source-read count, ascending-overlap smear, and the
  range/alignment/reserved-bit failures before mutation. The table is not
  computed from the production rounding formula.
- Reset: flags 0..4, IWRAM-tail preservation and unconditional forced blank;
  each documented-but-unmodeled flag 5..7 and every reserved bit 8..31 is
  checked as a pre-effect error. Reserved bits take classification precedence
  when combined with flags 5..7.
- Dispatch: explicit `UnsupportedService` results that mutate nothing.

`crates/core/tests/swi_decode.rs` checks typed ARM/Thumb comments.
`crates/core/tests/swi_execution.rs` checks full SVC entry/return, ARM and Thumb
resume addresses, CPSR flags/mode restoration, all six guest-reachable services,
unsupported/precondition errors before mutation, invalid stack state and
SVC-origin nesting. The AOT tests retain ARM Div and Thumb CpuFastSet budget
checks; an additional five-call ARM fixture covers Div, DivArm, Sqrt, CpuSet and
RegisterRamReset with hand-derived full-bank CPU and bus/frame oracles. The
original ARM/Thumb fixture independently checks the complete CPU digest,
including the saved Thumb CPSR. Native unsupported/precondition, invalid-stack
and nesting failures are also run; test-only harness instrumentation verifies
CPU and bus digests are identical before and after each rejected SWI. These
remain synthetic fixture tests, not BIOS or hardware conformance evidence.

They require no user-supplied ROM, BIOS file, save, network or external crate.
This is the
strongest independent evidence currently in the repository for these six
documented primitives and the bounded guest SWI path under synthetic conditions.
It is not evidence for arbitrary requests, a particular retail BIOS, all hardware
conformance, BIOS boot, retail-game compatibility or cycle timing.
