# gba-native

**Experimental, documentation-driven hybrid GBA-to-native prototype.** This is
not a playable emulator. It does not boot a BIOS or retail cartridges and has no
vendor upscaling integration. An optional, separate desktop package can present
the static synthetic bitmap through a real Pixels/WGPU window; that window is
not a playable frontend. There is no successful REA native-analysis evidence.
It is **not** a certified separate-team legal clean-room project.

The repository currently contains a small, deliberately bounded ARMv4T subset,
a bitmap snapshot renderer, a typed intermediate representation, a reference
interpreter, and a hybrid AOT path that translates discovered immutable-ROM
blocks into native Rust. The end-to-end proof uses only an entirely synthetic
guest that draws a deterministic 240x160 mode-3 image through executed
instructions.

## Status

| Area | State |
| --- | --- |
| Cartridge header (`192 B..32 MiB`, checksum/fixed byte) | Implemented, logo never validated or embedded |
| ARM/Thumb ALU, shifts, PC operands, B/BL/BX | Implemented for the documented ARMv4T subset; Thumb MUL remains explicit |
| ARM7TDMI MRS/MSR + banked r8-r14/SPSR state | Implemented for MSR-driven mode switches; no exceptions/return |
| Bus (RAM, VRAM, palette, OAM, ROM windows) | Small explicit subset; unsupported accesses error |
| BIOS HLE primitives | Six documented services callable directly from Rust; no guest SWI/SVC, BIOS boot or timing |
| Bitmap display modes 3/4/5 | Snapshot only; no timing/tiles/sprites/windows |
| Typed IR + reference interpreter | Implemented |
| Hybrid AOT discovery/emission/native proof | Implemented for synthetic fixtures; generated source binds SHA-256+length, never embeds ROM bytes |
| Runtime fallback | Reference interpreter; RAM code is never AOT |
| Nearest-neighbor PPM output | Implemented |
| Optional synthetic GPU window (`apps/desktop`) | Separate Pixels 0.17/Winit 0.30/WGPU 29 package; static synthetic bitmap only, no ROM/input/audio/save; one macOS Metal smoke run (render acknowledgements, not a visible-frame count) |
| FSR/DLSS/ML-FSR | **Not implemented**; explicit error |
| Retail game compatibility | **None claimed** |
| REA Evidence | **None obtained** |
| Android/iOS packaging | Later work |

## Commands

```text
cargo run -- info <rom.gba>
cargo run -- demo [--out output/demo.ppm] [--size 480x320] [--backend nearest]
cargo run -- run <rom.gba> [--budget 1000000]
cargo run -- aot proof [--out output/<new-directory>]
cargo run -- aot emit <rom.gba> <generated.rs>
```

- `info` parses the documented 192-byte header and prints diagnostics. It does
  not validate the Nintendo logo.
- `demo` builds the synthetic direct-entry fixture, executes 230,411 guest
  instructions in the reference interpreter, snapshots the complete bitmap and
  writes a PPM. The guest itself draws the image via ARM setup, a BX to Thumb,
  a Thumb pixel loop and a Thumb BX back.
- `run` executes a bounded number of instructions. Reaching the budget is not a
  boot or frame claim.
- `aot proof` discovers blocks in the synthetic ROM, allocates a new unique
  workspace under `output/` (an explicit `--out` must not already exist), writes
  that synthetic fixture there, emits a native Rust crate in a fresh child
  directory that stores only SHA-256+length, builds the crate with Cargo in an
  isolated target directory, runs it with an explicit `--rom PATH`, and compares
  registers, flags, bus digest, full-image digest and instruction budget with
  the reference interpreter.
- `aot emit` only emits source for a supplied ROM. The generated source contains
  translated instruction constants and the digest, not a ROM copy; it requires
  `--rom PATH` at runtime. This command does not execute that ROM natively and
  makes no general static-recovery claim.
- Generated paths are restricted to `<cwd>/output/...`; relative and absolute
  paths are resolved into that root, symlinks below it are refused, and the
  location is validated before any directory is created. An existing explicit
  `--out` directory is refused rather than reused or overwritten.
- Scaling is integer nearest-neighbor only, with exact 3:2 centering and black
  letterboxing. Vendor backends are rejected with a clear error.

The optional frontend is a separate package (`apps/desktop/`, its own Cargo
workspace and lockfile) and is not part of the default std-only core graph:

```text
./scripts/frontend.sh   # optional: hard line gate, fmt, clippy, unit tests, build, --help
```

The gate never opens a GPU window. GUI smoke and the exact artifact rules are in
[`docs/gpu-frontend.md`](docs/gpu-frontend.md).

## Build and test

```text
./scripts/short.sh     # fast: hard 500-line gate, Ratchet fast, fmt, clippy, fast tests
./scripts/full.sh      # short + debug/release tests, demo, native proof, Ratchet slow
./scripts/frontend.sh  # optional separate package: no Ratchet, no live GPU window
```

The underlying gates are `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test` and
`cargo test --release`. Ratchet is pinned to release `v0.5.0` of
`all-the-rest/ratchet-cli` (peeled commit
`99c25b1e18331a1473d43091d37e23f23bc1c30d`, tag object
`1feb7b5fae653272a922ed9f0479660ecde22ed2`); CI installs it with the official
action pinned to the commit. Locally the scripts use `RATCHET_BIN` or
`ratchet` on `PATH`, validate the version and fail closed when it is missing:

```text
RATCHET_BIN=/path/to/ratchet-0.5.0 ./scripts/short.sh
RATCHET_BIN=/path/to/ratchet-0.5.0 ./scripts/full.sh
```

The scripts resolve the repository root themselves, so they can be run from
any working directory. The pinned source can be built from the `v0.5.0` tag of
`all-the-rest/ratchet-cli`; the scripts never download or install anything.

Every maintained Rust source/test file is limited to 500 physical lines
(blank and comment lines included, a trailing newline adds no phantom line).
That population includes `apps/desktop/src` and the frontend tests. The
root-level `target/`, `output/` and `.git` directories hold generated
build/runtime or version-control artifacts and are not part of the maintained
source population. The exemption is positional: a nested directory with one of
those names, such as `src/output/` or `apps/desktop/target/`, is scanned like
any other source directory. `scripts/short.sh`, `scripts/frontend.sh` and CI run
the hard limit directly, so the zero-debt Ratchet baseline cannot be edited to
dilute it.

`scripts/frontend.sh` overrides `CARGO_TARGET_DIR` to the root-level ignored
`target/frontend` before any frontend Cargo command. Do not run frontend Cargo
directly inside `apps/desktop/` without setting a root-level target: the
checked-in `apps/desktop/.gitignore` hides generated files from Git but is not a
quality-scan exemption.

The native integration test requires only the installed Cargo/rustc. It creates
a fresh generated crate in an owned temporary directory, uses `--offline` and a
separate `CARGO_TARGET_DIR`, and removes that directory afterwards. The harness
builder accepts only a non-existing crate directory and creates its files with
`create_new`. No network, third-party crate, commercial ROM or BIOS is needed.

## Open work

Open work is tracked in [GitHub Issues](https://github.com/reisi007/gba-native/issues)
and assigned to `reisi007`; this README does not duplicate issue status. The
honest gaps by area:

- This milestone and verification — [#1](https://github.com/reisi007/gba-native/issues/1),
  quality foundation — [#2](https://github.com/reisi007/gba-native/issues/2)
- CPU, BIOS, timing/DMA/IRQ, PPU, input/audio, saves/RTC —
  [#5](https://github.com/reisi007/gba-native/issues/5),
  [#6](https://github.com/reisi007/gba-native/issues/6),
  [#4](https://github.com/reisi007/gba-native/issues/4),
  [#3](https://github.com/reisi007/gba-native/issues/3),
  [#7](https://github.com/reisi007/gba-native/issues/7),
  [#8](https://github.com/reisi007/gba-native/issues/8)
- Desktop frontend, standalone builds, Android/iOS —
  [#9](https://github.com/reisi007/gba-native/issues/9),
  [#10](https://github.com/reisi007/gba-native/issues/10),
  [#11](https://github.com/reisi007/gba-native/issues/11)
- REA evidence and retail compatibility —
  [#12](https://github.com/reisi007/gba-native/issues/12),
  [#14](https://github.com/reisi007/gba-native/issues/14)
- Spatial FSR, DLSS/ML/temporal FSR —
  [#13](https://github.com/reisi007/gba-native/issues/13),
  [#16](https://github.com/reisi007/gba-native/issues/16)
- AOT discovery, native self-modifying-RAM harness regression, performance and
  artifact trust — [#15](https://github.com/reisi007/gba-native/issues/15),
  [#17](https://github.com/reisi007/gba-native/issues/17),
  [#18](https://github.com/reisi007/gba-native/issues/18),
  [#19](https://github.com/reisi007/gba-native/issues/19)

None of these gaps is claimed as implemented by this prototype. The reference
interpreter already has a committed self-modifying-RAM regression
(`tests/cpu.rs::ram_code_is_decoded_fresh_after_self_modification`). Issue #17
covers the *native harness* regression, which is still missing and stays open
until implemented and independently verified.

## Supported subset (short version)

- ARM: all sixteen data-processing operations (`AND`, `EOR`, `SUB`, `RSB`,
  `ADD`, `ADC`, `SBC`, `RSC`, `TST`, `TEQ`, `CMP`, `CMN`, `ORR`, `MOV`, `BIC`,
  `MVN`) with immediate or register operands and LSL/LSR/ASR/ROR/RRX shifts by
  immediate or by the low eight bits of a register; `B`, `BL` and `BX`.
  Documented PC reads (`+8`, or `+12` for register-specified shifts), ALU PC
  writes with `S=0`, and PC wrapping are implemented. `S=1` with `Rd=PC`
  requires an SPSR restore and stays an explicit error; ARMv5/newer exchange
  encodings are rejected. `MRS`/`MSR`, literal `LDR` and immediate
  `LDRH`/`STRH` remain as before. Condition fields 0..14 are evaluated; field
  1111b is reserved and rejected explicitly. Immediate payloads are classified
  by bit 25 and independently swept over all imm8/rotation values.
- CPU state: the seven ARM7TDMI modes, shared r0-r7, FIQ-only r8-r12 banking,
  per-exception r13/r14 banks with USER/SYSTEM sharing, and independent
  FIQ/IRQ/SVC/ABT/UND SPSRs. MSR mode writes switch banks; USER control writes
  are ignored as documented; invalid modes and T-bit changes error before
  mutation; USER/SYSTEM SPSR access errors. Exception entry/return, SWI and
  BIOS boot are not implemented.
- Thumb: register ALU operations (all except `MUL`, which is explicit), shift by
  immediate and by register, `ADC`/`SBC`/`NEG`/`TST`/`CMN`, `MOV`/`ADD`/`SUB`
  register and immediate forms, high-register `ADD`/`CMP`/`MOV` (low/low
  `MOV`/`CMP` are an explicit **Design** extension beyond the documented
  high-register encodings), `ADR`, `ADD PC/SP`, `ADD`/`SUB SP,#imm`, both
  halves of `BL`, `BX`, literal `LDR`, `LDRH`/`STRH`, and conditional or
  unconditional `B`. The ARMv5 `BLX` second half and register form are rejected.
- Memory: exact EWRAM/IWRAM/palette/VRAM/OAM regions, three ROM windows,
  DISPCNT and KEYINPUT. No mirrors, open bus, DMA, timers, interrupts or saves.
- Display: 240x160 mode-3 RGB555 snapshot; modes 4 and 5 with page selection.
  Tiles, sprites, windows, blending and affine transforms are unsupported.
- AOT: only immutable ROM blocks; unknown targets use the reference interpreter;
  RAM is always interpreted freshly.

See `docs/clean-room/gba-contract.md` for exact behavior and limits.

## Clean-room boundary

Implementation is based on public hardware documentation. No third-party
emulator implementation, game decompilation, BIOS code or leaked development
kit was used. The same agent read public specifications and wrote the code; this
is **not** a formal two-team/legal clean-room process. See
`docs/clean-room/protocol.md`, `sources.md` and `rea-status.md`.

The supplied commercial ROM is never modified, copied into version control,
disassembled through REA, or used by automated tests.
