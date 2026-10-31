# Source register

Retrieved 2026-10-24. No emulator implementation source was used.

Status of this repository: experimental; no retail game is playable, no GPU
vendor upscaling is integrated, no REA Evidence was obtained, and this is not a
formal separate-team legal clean-room process.

## S1 — GBATEK (hardware and CPU reference)

URL: <https://mgba-emu.github.io/gbatek/>

Retrieved HTML: 5,473,181 bytes; SHA-256:
`a701cf0f5be47b23dd31298d7dc593a7cccb3ae9a59087dea940305565025b70`.
This identifies the fetched document, not an immutable upstream version. Public
reference material is cited and paraphrased, not vendored into this project.
Additional current sections below were fetched from this URL on 2026-10-28; that
retrieval was not separately retained or hashed.

Relevant anchors actually consulted for this milestone:

- `#gba-cartridge-header`: 192-byte header, fields, complement checksum.
- `#gba-cartridge-rom`: ROM bus, wait-state background, backup IDs.
- `#gba-memory-map`: memory sizes, buses, little-endian data representation.
- `#arm-cpu-flags--condition-field-cond`: conditions, CPSR flags, control bits,
  valid ARM7TDMI mode values and SPSR set.
- `#arm-cpu-register-set`: shared r0-r7, FIQ-banked r8-r12, per-mode r13/r14 and
  USER/SYSTEM sharing.
- `#arm-cpu-exceptions`: exception-mode SPSR ownership and the documented SWI
  vector, entry and return behavior.
- `#arm-opcodes-psr-transfer-mrs-msr`: MRS/MSR encoding, field bytes, USER
  restriction, SPSR availability and T-bit restriction.
- `#arm-cpu-memory-alignments`: aligned stores, rotated ARM7 word reads,
  odd-address LDRH rotation and LDRSH/LDRSB note.
- `#arm-opcodes-branch-and-branch-with-link-b-bl-bx-blx-swi-bkpt`.
- `#arm-opcodes-data-processing-alu` and `#second-operand-op2`: all sixteen
  opcodes, shift kinds, zero-shift specials and CPSR flag rules. The P-variant
  description explicitly limits `Rd=R15` result/PSR behavior to ARMv2 and below.
- `#arm-opcodes-multiply-and-multiply-accumulate-mul-mla`: MUL/MLA encoding,
  the `Rd`/`Rm` and R15 restrictions, low-32-bit result and the documented
  N/Z/C/V rule that C is "destroyed" on ARMv4.
- `#40000b8h---dma0cnt_l---dma-0-word-count-w-14-bit-14000h` and
  `#40000bah---dma0cnt_h---dma-0-control-rw`: DMA channel registers, the
  zero-means-maximum count, the addressing-control fields, the documented
  prohibition on source control 3, and the note that SAD/DAD/CNT_L keep their
  written values while the transfer uses internal pointers; channel-specific
  start timings and the DMA1/2 sound versus DMA3 video-capture distinction.
- `#gba-sound-channel-a-and-b-dma-sound`: FIFO A/B addresses, 32-byte capacity,
  the documented 16-byte refill threshold and four-word sound-DMA burst. The
  reference does not establish a fixed DMA1/A and DMA2/B pairing.
- `#4000082h---soundcnt-h---dma-sound-control-mixing`: Direct Sound control and
  the FIFO A/B reset bits.
- `#gba-timers`, `#4000100h---tm0cnt_l---timer-0-counterreload-rw` and
  `#4000102h---tm0cnt_h---timer-0-control-rw`: the four incrementing 16-bit
  timers, `F/1`/`F/64`/`F/256`/`F/1024` prescaler selection, count-up cascading
  (not used for Timer 0), overflow IRQ enable, start/stop, the reload/current
  counter distinction, reload on overflow and on the `0 -> 1` start edge, and
  the 32-bit reload-plus-start note. The implementation advances these counters
  only from the nominal three-cycle instruction mapping; no hardware cycle or
  prescaler phase is inferred from the reference. The host audio engine later
  uses the documented overflow-rate formula as the Direct Sound sample clock and
  issues explicit FIFO/DMA calls (see the Direct Sound round in
  `verification.md`); that is an audio approximation, not hardware timing.
  These anchors were consulted on 2026-10-29 in the same upstream GBATEK text at
  <https://problemkaputt.de/gbatek.htm>; that retrieval was not separately
  retained or hashed.
- `#using-r15-pc` and `#reading-from-r15`: ARM PC+8 versus PC+12 for
  register-specified shifts, and PC write behavior.
- `#mis-aligned-pcr15-branch-opcodes-or-movaluldr-with-rdr15`: forced PC
  alignment in ARM/Thumb state.
- `#arm-opcodes-memory-single-data-transfer-ldr-str-pld`: P/U/B/W/L fields,
  post-index T bit, `Rn=R15 -> PC+8`, `Rd=R15 -> PC+12`, ARMv4 `LDR PC` keeps T,
  register-offset reserved bit 4 and Rm=R0-R14.
- `#arm-opcodes-memory-halfword-doubleword-and-signed-data-transfer`: pre/post,
  immediate/register offsets, `1SH1` opcode space, ARMv5 LDRD/STRD boundary.
- `#arm-opcodes-memory-block-data-transfer-ldm-stm`: IA/IB/DA/DB, ascending
  register/memory order, ARMv4 base-in-list and empty-list quirks, `S=1`
  boundary, PC entries.
- `#arm-cpu-memory-alignments`: forced-aligned stores/block transfers, rotated
  word reads, odd LDRH rotation, and ARM7 odd LDRSH = LDRSB at the odd address.
- `#thumb-instruction-summary`.
- `#thumb-opcodes-register-operations-alu-bx`: move-shifted register,
  add/subtract, ALU opcodes, high-register operations and BX.
- `#thumb-opcodes-memory-loadstore-ldrstr`: THUMB.7 register-offset word/byte,
  THUMB.8 half/signed register forms, THUMB.9 immediate word/byte, THUMB.10
  immediate halfword, THUMB.11 SP-relative word and the literal load.
- `#thumb-opcodes-memory-multiple-loadstore-pushpop-and-ldmstm`: THUMB.14
  full-descending PUSH/POP with LR/PC bit and THUMB.15 LDMIA/STMIA writeback and
  base-in-list behavior, plus the ARMv4 empty-list R15 transfer and `Rb+0x40`
  writeback quirk.
- `#thumb-opcodes-jumps-and-calls`: conditional/unconditional branches and the
  two-half BL/BLX encoding.
- `#lcd-io-display-control`: mode selection, BG enable flags and forced blank.
- `#lcd-io-bg-control`: BG priority, character/screen bases, color depth,
  mosaic, reserved bits, text-map sizes and quadrant ordering.
- `#lcd-io-bg-scrolling`: BG0 horizontal/vertical offsets and text-mode use.
- `#lcd-vram-overview`: the first 64 KiB BG allocation in tile/map modes and
  the separate final 32 KiB OBJ allocation.
- `#lcd-vram-character-data`: 8x8 4bpp/8bpp tile layout and pixel ordering.
- `#lcd-vram-bg-screen-data-format-bg-map`: text map entries, flips, palette
  banks and screen-block arrangement.
- `#lcd-color-palettes`: BG palette layout, transparent index zero, backdrop
  color and RGB555 channel fields.
- `#lcd-vram-bitmap-bg-modes`.
- `#bios-function-summary`, `#bios-arithmetic-functions`,
  `#swi-06h-gba-or-swi-09h-nds7nds9dsi7dsi9---div`,
  `#swi-07h-gba---divarm`,
  `#swi-08h-gba-or-swi-0dh-nds7nds9dsi7dsi9---sqrt`, `#bios-memory-copy`,
  `#swi-0bh-gbands7nds9dsi7dsi9---cpuset`,
  `#swi-0ch-gbands7nds9dsi7dsi9---cpufastset`, `#bios-reset-functions`,
  `#swi-01h-gba---registerramreset`: documented interface and behavior for the
  direct BIOS HLE primitives (arithmetic, memory copy/fill and RAM reset). No
  BIOS image, disassembly or implementation source was used.
- `#how-bios-processes-swis`, `#bios-function-summary`,
  `#arm-cpu-exceptions`, `#gba-bios-ram-usage` and
  `#bios-interrupt-handling`: SWI comment extraction, the four documented
  handler-save values and SYSTEM-mode switch, exception entry/return flags and
  vector, GBA stack tops, and the behavior-only IRQ dispatcher contract: the
  six saved register values and order, ARM-state handler handoff, continuation
  return contract, and the boundaries around `0x03007FF8`, nesting and other
  interrupt sources. No BIOS bytes, disassembly or BIOS instruction fetch were
  used.
- `#gba-memory-map`, `#gba-io-map`, `#lcd-io-interrupts-and-status` and
  `#gba-unpredictable-things`: the I/O block address range, the documented
  8/16/32-bit I/O access widths, `VCOUNT` at `0x04000006` as a readable 16-bit
  register with the current scanline in bits 0..7 and zeros in bits 8..15, the
  LCD status/interrupt field layout, and the document's notes on register-width
  and byte-lane behavior. Only the `VCOUNT` byte lanes at `0x04000006`/
  `0x04000007` are added from this group; the bounded synthetic line
  progression remains an instruction-count model, not hardware line timing.
  The pre-existing byte-addressed sound-register reads are unchanged, every
  other I/O address keeps the existing register-specific dispatch, and
  otherwise-unmodelled reads remain explicit unsupported-width errors. These
  anchors were consulted on 2026-10-28 in the upstream GBATEK document at
  <https://problemkaputt.de/gbatek.htm>, which is the same reference text as S1;
  that retrieval was not separately retained or hashed.
- `#lcd-io-interrupts-and-status` and `#lcd-dimensions-and-timings`: `VCOUNT`
  (`0x04000006`) as the current scanline in `0..=227`, the DISPSTAT VBlank flag
  wording ("set in line 160..226; not 227"), the 160 drawing lines plus 68
  hidden VBlank lines, and the 228-line/280896-cycle frame at 1232 cycles per
  line. Only a deterministic instruction-count approximation is implemented:
  the retained 93632-instruction budget is spread over the 228 lines and the
  VBlank request follows the modelled line-160 transition. The same section
  documents the V-Count setting in DISPSTAT bits 8..15 (`LYC`), the read-only
  match flag in bit 2, the enable in bit 5 and the V-Counter request in `IF`
  bit 2 ("when its value is identical to the content of the VCOUNT register
  then the V-Counter flag is set (Bit 2), and (if enabled in Bit 5) an
  interrupt is requested"); that flag/request condition is implemented on top
  of the instruction-count line model, with `LYC` values `0..=255` accepted and
  only `0..=227` reachable. The same section does not state when a *write* that
  creates a match is evaluated; the model raises it immediately and labels
  that choice as an implementation decision, not a quoted rule. No HBlank or cycle timing is taken
  from these sections. These anchors were consulted on 2026-10-29 in the same
  upstream GBATEK text at <https://problemkaputt.de/gbatek.htm>, which is the
  same reference text as S1; that retrieval was not separately retained or
  hashed.
- `#gba-memory-map` and `#gba-cart-backup-sramfram`: the backup window entry
  (`0E000000-0E00FFFF Game Pak SRAM (max 64 KBytes) - 8bit Bus width`) and the
  documented byte-wide SRAM/FRAM cartridge backup. The explicit
  `Sram32KVolatile` and `Sram64KVolatile` opt-ins model an erased, volatile,
  byte-wide 32 KiB or 64 KiB (documented window maximum) SRAM inside that
  window; no chip-size detection, no persistence and no flash behaviour is
  implemented or claimed.
- `#gba-cart-backup-flash-rom` and `#gba-cart-backup-ids`: consulted only to
  confirm that flash needs the documented command/ID sequence ("Used to detect
  the type (and presence) of FLASH chips"), which this prototype deliberately
  does **not** implement. Under either SRAM opt-in such writes are ordinary
  SRAM bytes and reads return them; the model never claims a flash device is
  absent.
  Both anchors were consulted on 2026-10-29 in the same upstream GBATEK text;
  that retrieval was not separately retained or hashed.
- `#bios-decompression-functions`,
  `#lz77uncompreadnormalwrite8bit-wram---swi-11h-gbands7nds9dsi7dsi9`,
  `#lz77uncompreadnormalwrite16bit-vram---swi-12h-gba`,
  `#rluncompreadnormalwrite8bit-wram---swi-14h-gbands7nds9dsi7dsi9`,
  `#rluncompreadnormalwrite16bit-vram---swi-15h-gba`,
  `#diff8bitunfilterwrite8bit-wram---swi-16h-gbands9dsi9`,
  `#diff8bitunfilterwrite16bit-vram---swi-17h-gba` and
  `#diff16bitunfilter---swi-18h-gbands9dsi9`, `#swi-0eh-gba---bgaffineset`: the
  32-bit header layout (reserved
  nibble, compressed type, 24-bit decompressed size), the LZ77 flag-byte and
  back-reference encoding, the run-length flag/length encoding, the 8/16-bit
  diff accumulation, the Wram/Vram write-unit split and the documented
  `disp=000h` Vram limitation. Only the opt-in `DocumentedFormats` policy
  implements these descriptions; no BIOS bytes, code or disassembly were used.
  These anchors were consulted on 2026-10-29 in the S1 rendering at
  <https://mgba-emu.github.io/gbatek/>, which is the same reference text;
  that retrieval was not separately retained or hashed.
- `#bios-rotationscaling-functions` and `#swi-0fh-gba---objaffineset`: the
  six-byte source structure (s16 scale X, s16 scale Y, u16 angle), the four
  destination parameters (PA, PB, PC, PD), the upper-eight-bit angle use and
  the `offset=2`/`offset=8` destination layout. Only the opt-in
  `AffinePolicy::Documented` implements `ObjAffineSet`; the Q12 sine table is
  generated from the mathematical sine definition, not copied from a BIOS
  image, and no bit-identical BIOS-table or rounding claim is made. Consulted
  on 2026-10-29 in the S1 rendering at <https://mgba-emu.github.io/gbatek/>;
  that retrieval was not separately retained or hashed.

- `#lcd-io-display-control`, `#lcd-io-bg-control`, `#lcd-io-bg-rotationscaling`,
  `#lcd-obj---overview`, `#lcd-obj---oam-attributes`,
  `#lcd-obj---oam-rotationscaling-parameters`,
  `#lcd-obj---vram-character-tile-mapping`, `#lcd-io-window-feature` and
  `#lcd-io-color-special-effects`: Mode 0/1 layer enable and priority, text and
  affine BG control/scroll, BG2/BG3 affine matrix/reference registers, OAM
  normal/affine attributes, 1D OBJ tile mapping, window ranges/masks and
  BLDCNT/BLDALPHA/BLDY blending modes. Only the static snapshot subset described
  in `gba-contract.md` is implemented; 2D OBJ mapping, double-size affine OBJ,
  OBJ mosaic and window 1 remain absent. These anchors were consulted on
  2026-10-29 in the S1 rendering at <https://mgba-emu.github.io/gbatek/>; that
  retrieval was not separately retained or hashed.

Only ARMv4T behavior applies. This reference also describes newer ARM CPUs and
other consoles; their opcodes/features must not accidentally enter the GBA core.
The implemented subset is deliberately narrower than the document; see
`gba-contract.md`.

## S6 — ARM7TDMI Technical Reference Manual

URL: <https://ww1.microchip.com/downloads/en/DeviceDoc/DDI0029G_7TDMI_R3_trm.pdf>

Retrieved 2026-10-25 through the document URL above; 1,820,983 bytes, SHA-256
`ce58f88939f4eb2a7029022cfac16724f95d7c47ed2424625e10286a66fe9078`. This
identifies the fetched PDF, not an immutable upstream revision.

Sections actually consulted:

- §2.1: the ARM7TDMI implements ARM architecture v4T.
- §2.8.1 / Table 2-3: SWI return address is ARM PC+4 or Thumb PC+2 and the
  return instruction is `MOVS PC,R14_svc`.
- §2.8.2 and Table 2-4: exception entry saves CPSR to SPSR, enters ARM state,
  vectors SWI to address `0x08`, sets I and leaves F unchanged.
- §2.8.7: SWI enters Supervisor mode and `MOVS PC,R14_svc` restores PC/CPSR
  and resumes after the SWI.
- §6.5: compare and test operations do not produce a data result; only ALU
  status flags are affected. This generic timing description does not classify
  a nonzero `Rd` field in those encodings.

These statements corroborate the public hardware contract only. No BIOS image,
instruction bytes, disassembly or implementation source was used.

## S7 — ARM Architecture Reference Manual

URL: <https://www.cs.miami.edu/~burt/learning/Csc521.141/Documents/arm_arm.pdf>

Retrieved 2026-10-29; 5,740,866 bytes, SHA-256
`c441eb7f981f3beb5ad901716808cd54d86e9b41aa68b42a2ce9701831b0b7ea`. This
identifies the fetched PDF, not an immutable upstream revision. It is ARM DDI
0100I, copyright 1996–2005.

Sections actually consulted:

- §A4.1.15 (printed pages A4-28–29): the `CMP` encoding labels bits [15:12]
  `SBZ` and its syntax has no destination-register operand. §A4.2's architecture
  table lists `CMP` for ARMv4T.
- Glossary (printed page Glossary-13): an `SBZ` field must be zero; nonzero
  values produce UNPREDICTABLE results.

Therefore this ARMv4T reference does not define behavior for `CMP` with a
nonzero `Rd` field, including the local test encoding `0xE15FF000`. It does not
support implementing the older ARMv2 P-variant behavior on ARM7TDMI. The manual
was consulted only as an architecture reference; no implementation sources
were used.

## S8 — ARM Architecture Reference Manual (ARMv4T-era edition)

URL: <https://www.cl.cam.ac.uk/teaching/0506/ECADArch/datasheets/arm.pdf>

Retrieved 2026-10-29; 13,344,133 bytes, SHA-256
`fa405a69d371c533f91aab331c8265c1d50ad0f5fe856f232f3a05f93f687114`. This
identifies the fetched PDF, not an immutable upstream revision. It is ARM DDI
0100E, copyright 1996–2000.

Sections actually consulted:

- §4.1.14 (printed page A4-25): the `CMP` encoding labels bits [15:12] `SBZ`.
  §4.2's architecture table (printed page A4-113) lists `CMP` for ARMv4T.
- Glossary (printed page Glossary-xi): nonzero values in an `SBZ` field produce
  UNPREDICTABLE results.
- §4.1.51–52 (printed pages A4-102–105): ARM `SWP`/`SWPB` encodings and
  operations; `Rd=Rm` is permitted, while R15 operands and `Rn=Rd`/`Rn=Rm`
  aliases are UNPREDICTABLE. Unaligned word SWP rotates the loaded word right by
  `8 * address[1:0]` and stores the source word without rotation; SWPB
  zero-extends the loaded byte.

This earlier ARM Architecture Reference Manual independently confirms that
`CMP` with a nonzero `Rd` field has no defined ARMv4T behavior.

## S2 — gbadev resources

<https://gbadev.net/resources>

Discovery index for documentation and future hardware test suites. Merely
listing a test suite here does **not** mean it has been fetched or passed.
Linked emulator/game/BIOS implementation sources were not opened.

## S3 — gbadoc

<https://github.com/gbadev-org/gbadoc>

Repository landing page inspected; identifies a community technical documentation
project under CC0-1.0. Not used as evidence for individual instruction semantics
in this milestone. No implementation files were fetched.

## S4 — AMD FSR 1 documentation

<https://gpuopen.com/fidelityfx-superresolution/>

Vendor describes a spatial EASU reconstruction pass and RCAS sharpening pass,
without motion vectors or frame history. This is **not ML-based upscaling**.
The vendor lists DX11/DX12/Vulkan and permits ports. No AMD shader source or SDK
is incorporated in this milestone.

## S5 — NVIDIA DLSS documentation

<https://developer.nvidia.com/rtx/dlss/get-started>

Vendor identifies RTX Tensor Cores and describes Super Resolution using motion
data and previous frames. This establishes that resizing a framebuffer is not
equivalent to DLSS. No SDK/library/model was downloaded or integrated.

## Design-only work

The typed IR, discovery strategy, local constant propagation, native Rust
emission, artifact digest binding, test harness and bitmap snapshot are **Design**
choices. They are not copied from an emulator, compiler or vendor source. The
Rust standard library is the only implementation dependency.

## Local observations

REA observations are in `rea-status.md`. Header inspection of the supplied ROM
uses our own parser, **not REA**. Synthetic instruction tests are local tests,
not measurements on real GBA hardware. No retail-game compatibility is inferred
from a correct header or a completed instruction budget.
