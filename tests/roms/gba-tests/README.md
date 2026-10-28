# Vendored GBA hardware test ROMs

Upstream: <https://github.com/jsmolka/gba-tests>, fetched from `master` on
2026-10-28. No upstream commit is pinned; the binding is the SHA-256 of every
file below, which records exactly the bytes that were verified.

License: MIT, Copyright (c) 2019 Julian Smolka. See `LICENSE` in this directory.
Redistribution of these binaries is permitted by that license.

## Why these files are in the repository

They are small (19 KiB total), MIT-licensed **black-box oracles**: each ROM draws
its own PASS/FAIL result on the GBA screen. They are test *programs*, not
emulator implementation sources, not BIOS images and not game decompilations, so
they do not touch the repository's clean-room boundary.

They are **not** part of `cargo test`. `cargo test` must stay offline and
dependency-free, and these ROMs are executed manually against the bounded
interpreter/AOT runner. Treat every result as evidence about this prototype
only, never as a hardware-conformance or game-compatibility claim.

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

`arm.gba` deliberately includes the ARMv5/vendor encodings that this prototype
still rejects; a run that stops on an explicit `UnsupportedInstruction` is a
correct fail-closed result, not a regression.

## Result ladder

Run each ROM with the bounded runner and read the kernel-drawn result:

```text
cargo run -- run tests/roms/gba-tests/<name>.gba --budget 5000000 --out output/<new-directory>
```

A run that ends in `status=bounded` proves only that the instruction budget was
consumed. It is not a pass. A pass requires the ROM's own PASS marker to be
visible in the rendered snapshot.

## Refreshing

Re-download from
`https://raw.githubusercontent.com/jsmolka/gba-tests/master/<path>/<name>.gba`
and confirm every SHA-256 above is unchanged before replacing a file.
