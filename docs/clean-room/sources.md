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

Relevant anchors actually consulted for this milestone:

- `#gba-cartridge-header`: 192-byte header, fields, complement checksum.
- `#gba-cartridge-rom`: ROM bus, wait-state background, backup IDs.
- `#gba-memory-map`: memory sizes, buses, little-endian data representation.
- `#arm-cpu-flags--condition-field-cond`: conditions, CPSR flags, control bits,
  valid ARM7TDMI mode values and SPSR set.
- `#arm-cpu-register-set`: shared r0-r7, FIQ-banked r8-r12, per-mode r13/r14 and
  USER/SYSTEM sharing.
- `#arm-cpu-exceptions`: which exception modes own an SPSR and the explicit
  boundary that exception entry/return is not implemented.
- `#arm-opcodes-psr-transfer-mrs-msr`: MRS/MSR encoding, field bytes, USER
  restriction, SPSR availability and T-bit restriction.
- `#arm-cpu-memory-alignments`: aligned stores, rotated ARM7 word reads,
  odd-address LDRH rotation and LDRSH/LDRSB note.
- `#arm-opcodes-branch-and-branch-with-link-b-bl-bx-blx-swi-bkpt`.
- `#arm-opcodes-data-processing-alu` and `#second-operand-op2`: all sixteen
  opcodes, shift kinds, zero-shift specials and CPSR flag rules.
- `#using-r15-pc` and `#reading-from-r15`: ARM PC+8 versus PC+12 for
  register-specified shifts, and PC write behavior.
- `#mis-aligned-pcr15-branch-opcodes-or-movaluldr-with-rdr15`: forced PC
  alignment in ARM/Thumb state.
- `#arm-opcodes-memory-single-data-transfer-ldr-str-pld`.
- `#arm-opcodes-memory-halfword-doubleword-and-signed-data-transfer`.
- `#thumb-instruction-summary`.
- `#thumb-opcodes-register-operations-alu-bx`: move-shifted register,
  add/subtract, ALU opcodes, high-register operations and BX.
- `#thumb-opcodes-memory-loadstore-ldrstr`.
- `#thumb-opcodes-jumps-and-calls`: conditional/unconditional branches and the
  two-half BL/BLX encoding.
- `#lcd-io-display-control`, `#lcd-vram-bitmap-bg-modes`.
- `#bios-function-summary`, `#bios-arithmetic-functions`,
  `#swi-06h-gba-or-swi-09h-nds7nds9dsi7dsi9---div`,
  `#swi-07h-gba---divarm`,
  `#swi-08h-gba-or-swi-0dh-nds7nds9dsi7dsi9---sqrt`, `#bios-memory-copy`,
  `#swi-0bh-gbands7nds9dsi7dsi9---cpuset`,
  `#swi-0ch-gbands7nds9dsi7dsi9---cpufastset`, `#bios-reset-functions`,
  `#swi-01h-gba---registerramreset`: documented interface and behavior for the
  direct BIOS HLE primitives (arithmetic, memory copy/fill and RAM reset). No
  BIOS image, disassembly or implementation source was used.

Only ARMv4T behavior applies. This reference also describes newer ARM CPUs and
other consoles; their opcodes/features must not accidentally enter the GBA core.
The implemented subset is deliberately narrower than the document; see
`gba-contract.md`.

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
