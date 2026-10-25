# Independently implemented BIOS HLE services (foundation)

Status: implementation foundation only. This document describes Rust
primitives that are callable directly from synthetic tests. They are not yet
wired to SWI/SVC decoding or the interpreter, do not perform exception entry,
do not boot a BIOS and do not establish playability. The public status summary
and `gba-contract.md` are updated separately when the CPU integration lands.

Decision background: **D8** in `docs/decisions.md` chooses documented BIOS HLE
derived from public interface/hardware documentation instead of a bundled or
disassembled Nintendo BIOS. D9 (GPU presentation) is unrelated future work.

## Source anchor and identity

The only hardware reference used for this module is the public GBATEK document
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

A later SWI integration owns the instruction budget, next-PC update and any
exception framing. That integration is **not** part of this module.

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
- Source regions are EWRAM, IWRAM, palette, VRAM, OAM and the three ROM windows.
  I/O, save space and unmapped addresses are `UnsupportedSource`. A source
  range reaching the 16 KiB BIOS area returns `Error::BiosAccess` instead of
  being read or silently rejected as success.
- Destination regions are EWRAM, IWRAM, palette, VRAM and OAM. ROM, I/O, save
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
- Bits 5, 6, 7 and every higher bit are rejected with
  `BiosError::UnsupportedResetFlags` **before** any memory or register mutation.
  SIO, sound and other-register resets are not faked.
- `DISPCNT=0080h` is written unconditionally, including for flags zero, so a
  display snapshot becomes forced-blank white.
- Clears use 16-bit or 32-bit bus writes, not byte writes. OAM zeroing is not a
  claim that objects are disabled; DISPCNT bit 6 and OAM attribute state are
  separate.
- No save, persistence, audio, serial or other I/O state is touched.

## Deliberately unimplemented

The following are explicit `BiosError::UnsupportedService` values, never spins
or silent successes:

- `00h` SoftReset and `26h` HardReset (no CPU reset/reboot model here).
- `02h` Halt, `04h` IntrWait and `05h` VBlankIntrWait: they require IRQ, IF/IE,
  IME and timing state that this prototype does not model.
- `03h` Stop/Sleep and `27h` CustomHalt.
- `09h`/`0Ah` ArcTan/ArcTan2, `0Dh` GetBiosChecksum, affine setup and every
  other service not listed above.
- The decompression family (`10h`..`18h`) is future work; no fake handlers are
  installed.
- BIOS bytes, BIOS boot, exception entry/return and SWI instruction consumption
  remain absent.

## Tests and evidence boundary

`tests/bios_arithmetic.rs`, `tests/bios_memory.rs` and
`tests/bios_dispatch.rs` use synthetic RAM/ROM fixtures only. They contain
independent expected values (including the documented division example, signed
division matrix, integer-root property, rounding table, fill-read count,
overlap smear, range/alignment/reserved-bit failures, atomic error paths,
IWRAM-tail preservation and forced-blank snapshot). They require no ROM, BIOS
file, save, network or external crate.

This evidence establishes only the listed primitive behavior. It is not BIOS
boot, not SWI/SVC integration, not retail-game compatibility, not cycle timing
and not a hardware-conformance claim.
