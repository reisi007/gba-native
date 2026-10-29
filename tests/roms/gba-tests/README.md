# Vendored GBA hardware test ROMs

Upstream: <https://github.com/jsmolka/gba-tests>, fetched from `master` on
2026-10-28. No upstream commit is pinned; the binding is the SHA-256 of every
file below, which records exactly the bytes that were verified.

License: MIT, Copyright (c) 2019 Julian Smolka. See `LICENSE` in this directory.
Redistribution of these binaries is permitted by that license.

## Why these files are in the repository

They are small (19 KiB total), MIT-licensed local black-box tests and PPU output
samples. The ARM, Thumb, memory and BIOS test programs draw their own PASS/FAIL
result; the `hello`, `shades` and `stripes` PPU samples instead draw fixed
expected output and idle. They are test *programs*, not emulator implementation
sources, not BIOS images and not game decompilations, so they do not touch the
repository's clean-room boundary.

They are **not** part of `cargo test`. `cargo test` must stay offline and
dependency-free, and these ROMs are executed manually against the bounded
interpreter/AOT runner. They are local-only: CI must not fetch or execute them,
and their binaries must not be included in CI or published artifacts. Treat every
result as evidence about this prototype only, never as a hardware-conformance or
game-compatibility claim.
Keep the ROM files unchanged; write generated artifacts only to a fresh ignored
directory under `output/`.

## Files

| File | SHA-256 | Covers |
| --- | --- | --- |
| `arm.gba` | `77ee88662552bdc885c1080c0172ff119d54db791bd73b21808cf1ff1fe5b40e` | ARM data processing, shifts, conditions, branches, block/data/halfword transfers, PSR transfer, swap, multiply |
| `thumb.gba` | `b5cb2291df4ab314b31c598acd9bff2ccfa0b38efff29daadfe97422ce369b67` | Thumb opcode formats |
| `memory.gba` | `21024fb6aae6343f5f0466dd54e3149de1fbeb23f78e7d85a015c983684d2f87` | RAM and I/O mapping, mirrored regions |
| `shades.gba` | `127ce348f17e4d5fd8a0821a7af757eda6109c66c9f7b8f7d1e0fa31831b407d` | PPU shades / colour handling |
| `stripes.gba` | `80d6cc05b5f289b9a2392621fc245270f08e9f0d77fd33982120cef240f5a504` | PPU stripes / background drawing |
| `hello.gba` | `38aed48b67bc0f701e8aa222b0c3334bd306bd29888707bb7224d81f5576c264` | Minimal PPU output smoke test |
| `bios.gba` | `9d7b369fa1aa661ff03692b3d79c6f644b623d72983d0fc890e6d87a0409a3c9` | BIOS-provided service behaviour |

`arm.gba` includes encodings outside this prototype's documented-valid subset,
including an ARMv2 P-variant test operation. ARM Architecture Reference Manuals
DDI0100E and DDI0100I mark the `CMP` bits [15:12] as SBZ and state that nonzero
SBZ values are unpredictable, so this encoding has no defined ARMv4T behavior.
In the published `arm/data_processing.asm`, tests 234–235 execute this encoding
and expect the legacy P-variant's mode-change and pipeline behavior. The
ARM7TDMI manual's generic flags-only description of compare/test operations
does not define this reserved-field encoding. Those expectations are outside
the ARMv4T contract, so the instruction remains an explicit error rather than
a guessed implementation.

## Result ladder

Run each ROM with the bounded runner and inspect the output appropriate to that
ROM (a PASS/FAIL marker for tests, or the expected static image for PPU samples):

```text
cargo run -- run tests/roms/gba-tests/<name>.gba --budget 5000000 --out output/<new-directory>
```

A run that ends in `status=bounded` proves only that the instruction budget was
consumed. For test ROMs, a pass requires the ROM's own PASS marker to be visible
in the rendered snapshot. PPU samples intentionally idle after drawing their
expected image and have no PASS marker; their snapshots are only visual smoke
evidence, not test-suite passes.

For a local macOS x86_64-target AOT build (on Apple silicon, this is a genuine
cross-target build and the generated binary can run under Rosetta), build and
run one ROM at a time with a fresh output directory:

```sh
rom=shades
target=x86_64-apple-darwin
out="output/gba-tests-$(date +%Y%m%d-%H%M%S)-$rom-$target"
cargo run --offline --locked --target "$target" -- aot build \
  "tests/roms/gba-tests/$rom.gba" --target "$target" \
  --out "$out" --budget 5000000
"$out/native/target/$target/release/gba-native-aot-harness" \
  --rom "tests/roms/gba-tests/$rom.gba"
```

Repeat with each name in the table. `aot build` only compiles; the explicit
harness invocation executes the guest. The reference `run ... --out` command
above writes `screen.ppm` for visual inspection; compare the AOT report's
`image_sha256` with the reference image digest when both runs reach a snapshot.
An image or matching digest without the ROM's own visible PASS result is not a
test pass. Unsupported instructions, memory, PPU or BIOS behavior remains an
explicit failure and must not be suppressed to manufacture a PASS screen.

## Local run record (2026-10-29, final code r13)

All seven ROM-bound AOT artifacts were built for `x86_64-apple-darwin` on an
Apple-silicon Mac and run under Rosetta. Each was a Mach-O x86_64 executable;
all reported `embedded_rom_bytes=0` and required the original external ROM path.
The reference runner and each AOT artifact used a 5,000,000-instruction budget.
The two test ROMs with visible PASS screens passed; the other two test ROMs
remain explicitly blocked:

| ROM | Latest runtime result |
| --- | --- |
| `arm.gba` | Reference and AOT fail closed at test 234 on `CMP PC, PC, R0` (`0xE15FF000`) at `0x08000CE4`; its nonzero `Rd` field is SBZ/UNPREDICTABLE on ARMv4T, so no PASS snapshot. |
| `thumb.gba` | **ROM-owned PASS:** reference snapshot visibly says “All tests passed”; AOT has the same endpoint and image digest. Reference budget ended at `0x08000AAC`; AOT used 4,999,963 fallbacks. |
| `memory.gba` | **ROM-owned PASS:** reference snapshot visibly says “All tests passed”; AOT matches the reference digest with zero fallbacks. The test-driven palette/VRAM/OAM mirror extensions let its mirror suite complete; they are explicitly documented as prototype policies, not hardware-map claims. |
| `shades.gba` | Budget consumed. Visually inspected blue gradient; a PPU output sample, not a PASS/FAIL suite. Reference/AOT image digest `607b1a3e8f71388746869ca871286140679ed58e5c22d78913a44009a1ea1d9a`. |
| `stripes.gba` | Budget consumed. Visually inspected stripe pattern; a PPU output sample, not a PASS/FAIL suite. Reference/AOT image digest `da3e48e2995f40fea99c5db72c3181ae45fa992c0bb0373bc3fff2c432167fe3`. |
| `hello.gba` | Budget consumed. Visually inspected “Hello world!” output; a PPU smoke sample, not a PASS/FAIL suite. Reference/AOT image digest `fddf4a8499c1775578e19725458ea27e6648c3f481c421502f02f25ea573caa4`. |
| `bios.gba` | Reference and AOT fail closed at BIOS access `0x00000000`; its published test source reads BIOS vector contents at startup and after SWI/IRQ, which the bounded HLE cannot supply. No BIOS bytes are supplied or synthesized, so there is no PASS snapshot. |

The generic ARMv4T `SWP`/`SWPB` operations used by `arm/data_swap.asm` tests
450–453 are implemented and covered by independent synthetic interpreter and
native-AOT vectors, including unaligned SWP and the documented `Rd=Rm` case.
The `arm.gba` cartridge itself still stops at test 234 before reaching them; this
does not change its result or imply a ROM pass.

The final reference snapshots and visual-review PNGs are under the ignored
`output/gba-tests-20261029-reference-r13/<rom>/` directories. The `thumb` and
`memory` snapshots visibly show “All tests passed”; their 240x160 RGB image
digests both match AOT (`e33384104712250daf9c599ef59680eadf88c9e111f81a4d4de8d7a6b15be21b`).
A pre-SWP follow-up reference run wrote to
`output/gba-tests-20261029-followup-<rom>/`; the converted Thumb and memory
images again visibly show “All tests passed”. The ARM and BIOS follow-up runs
stopped at the same explicit unsupported boundaries. After SWP/SWPB support, a
fresh reference run wrote to `output/gba-tests-20261029-swp-<rom>/`; the converted
Thumb and memory images visibly show “All tests passed”, while ARM and BIOS
again stopped at their documented boundaries. The ARM swap cases are covered by
synthetic tests only because that cartridge stops earlier at the
SBZ/UNPREDICTABLE P-variant encoding.
The `hello`, `shades` and `stripes` snapshots match AOT pixel-for-pixel but are
only visual smoke evidence. `arm` and `bios` produced no snapshots because the
runner stopped at explicit unsupported boundaries. No ROM file was modified,
copied into generated output, or embedded in an artifact. These are local
observations only, not hardware-conformance or general compatibility evidence.

## Refreshing

Re-download from
`https://raw.githubusercontent.com/jsmolka/gba-tests/master/<path>/<name>.gba`
and confirm every SHA-256 above is unchanged before replacing a file.
