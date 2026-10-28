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
  opcodes, shift kinds, zero-shift specials and CPSR flag rules.
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
  base-in-list behavior.
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
  vector, GBA stack tops, and the boundary that IRQ nesting is not modeled.

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

- §2.8.1 / Table 2-3: SWI return address is ARM PC+4 or Thumb PC+2 and the
  return instruction is `MOVS PC,R14_svc`.
- §2.8.2 and Table 2-4: exception entry saves CPSR to SPSR, enters ARM state,
  vectors SWI to address `0x08`, sets I and leaves F unchanged.
- §2.8.7: SWI enters Supervisor mode and `MOVS PC,R14_svc` restores PC/CPSR
  and resumes after the SWI.

These statements corroborate the public hardware contract only. No BIOS image,
instruction bytes, disassembly or implementation source was used.

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
