# gba-native

**Experimental, documentation-driven hybrid GBA-to-native prototype.** This is
not a playable emulator. It does not boot a BIOS or retail cartridges and has no
vendor upscaling integration. Optional, separate desktop and browser packages
present the static synthetic bitmap through Pixels/WGPU and WASM/WebGPU/WebGL2;
neither is a playable or per-ROM frontend. There is no successful REA
native-analysis evidence.
It is **not** a certified separate-team legal clean-room project.

The repository currently contains a small, deliberately bounded ARMv4T subset,
a bitmap snapshot renderer, a typed intermediate representation, a reference
interpreter, and a hybrid AOT path that translates discovered immutable-ROM
blocks into native Rust. The end-to-end proof uses only an entirely synthetic
guest that draws a deterministic 240x160 mode-3 image through executed
instructions.

The code is split into three internal packages in one Cargo workspace:

```text
gba-native (root)        CLI, safe output/ writes, compatibility facade, quality counter
├── gba-native-core      std-only runtime: cart/bus/CPU, typed IR, decode, interpreter, HLE, display
└── gba-native-aot       host-side discovery, emission, artifact binding and native harness
    └── gba-native-core  dependency under the historical `gba_native` alias
```

The root facade re-exports the core modules and `gba_native_aot::aot`, so the
historical `gba_native::{bus, cpu, ..., aot}` paths keep working. The AOT crate
depends on the core package under the `gba_native` alias; the optional
`apps/desktop` and `apps/web` packages each depend directly on the core and
remain outside this workspace. The browser package has a separate pinned
toolchain and dependency lock. The default third-party dependency set is empty —
the root workspace lockfile contains only these three internal path packages.

## Status

| Area | State |
| --- | --- |
| Cartridge header (`192 B..32 MiB`, checksum/fixed byte) | Implemented, logo never validated or embedded |
| ARM/Thumb ALU, shifts, PC operands, B/BL/BX | Implemented for the documented ARMv4T subset, including ARM MUL/MLA and Thumb MUL |
| ARM7TDMI MRS/MSR + banked r8-r14/SPSR state | Implemented for MSR-driven mode switches and the bounded framed SWI entry/return path |
| Bus (RAM, VRAM, palette, OAM, ROM windows, I/O) | Small explicit subset; unmodelled reads error, unused I/O writes are ignored and recorded |
| ARM/Thumb memory transfers | Word/byte/half/signed single transfers, ARM LDM/STM, Thumb PUSH/POP/LDM/STM for the documented policy; unaligned block bases reject explicitly |
| BIOS HLE | Six documented services callable directly or from typed ARM/Thumb SWI; no BIOS boot, other exceptions or timing |
| Bitmap display modes 3/4/5 | Snapshot only; no timing/tiles/sprites/windows |
| Typed IR + reference interpreter | Implemented |
| Hybrid AOT discovery/emission/native proof | Implemented for synthetic fixtures; generated source binds SHA-256+length, never embeds ROM bytes |
| Compiled native AOT artifact (`aot build`) | Experimental headless release executable for one Cargo target triple; external SHA-256/length-bound ROM; no playability, GUI or save claim |
| ROM-bound WASM execution module (`aot build --target wasm32-unknown-unknown`) | wasm-bindgen `run_rom(Uint8Array)` module plus JS glue; embeds SHA-256+length, never ROM bytes; no browser app/UI, GPU renderer, input, persistence or playability claim |
| Runtime fallback | Reference interpreter; RAM code is never AOT |
| Nearest-neighbor PPM output | Implemented |
| Optional synthetic GPU window (`apps/desktop`) | Separate Pixels 0.17/Winit 0.30/WGPU 29 package; static synthetic bitmap only, no ROM/input/audio/save; one macOS Metal smoke run (render acknowledgements, not a visible-frame count) |
| Optional synthetic browser GPU foundation (`apps/web`) | Separate WASM package with WebGPU and WebGL2, static synthetic demo only; no ROM/AOT/input/audio/save; browser functional E2E locally observed, visual review pending |
| FSR/DLSS/ML-FSR | **Not implemented**; explicit error |
| Retail game compatibility | **None claimed** |
| REA Evidence | **None obtained** |
| Android/iOS packaging | Later work |

## Commands

```text
cargo run -- info <rom.gba>
cargo run -- demo [--out output/demo.ppm] [--size 480x320] [--backend nearest]
cargo run -- run <rom.gba> [--budget 1000000] [--out output/<new-directory>]
cargo run -- aot proof [--out output/<new-directory>]
cargo run -- aot emit <rom.gba> <generated.rs>
cargo run -- aot build <rom.gba> --target <triple> [--out output/<new-directory>] [--budget N]
cargo run -- aot build <rom.gba> --target wasm32-unknown-unknown [--out output/<new-directory>] [--budget N]
```

- `info` parses the documented 192-byte header and prints diagnostics. It does
  not validate the Nintendo logo.
- `demo` builds the synthetic direct-entry fixture, executes 230,411 guest
  instructions in the reference interpreter, snapshots the complete bitmap and
  writes a PPM. The guest itself draws the image via ARM setup, a BX to Thumb,
  a Thumb pixel loop and a Thumb BX back.
- `run` initializes the documented guest BIOS stacks and executes a bounded
  number of instructions, including typed SWIs to the six HLE services. Other
  services return explicit errors. Reaching the budget is not a boot or frame
  claim. Without `--out`, it creates no files. With `--out`, after both execution
  and the static bitmap snapshot succeed, it creates a fresh directory containing
  `screen.ppm` and `report.txt`. The report records the ROM and image hashes,
  requested/used instruction budget, final CPU/display state and an explicit
  non-claim; this post-budget snapshot is not a frame or hardware-timed render.
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
- `aot build` discovers blocks from a supplied ROM at the ARM entry
  `0x08000000` and creates a fresh owned workspace under `output/` with
  `generated.rs`, `artifact.meta` and `build.meta`. The command does not execute
  the artifact, and no ROM copy is written: the original file stays external and
  read-only. The required `--target` selects one of two bounded branches:
  - any non-WASM triple emits the compatibility native module and compiles a
    headless release executable for that Cargo triple. The executable stays in
    the generated crate's target-specific release directory, still requires
    `--rom PATH` at runtime, and checks SHA-256 plus length before execution.
    Target support is whatever the installed Cargo/rustc toolchain can actually
    build offline; this is not a cross-compilation, cross-platform or
    platform-support claim, and only targets that produce a native executable
    are supported here.
  - `wasm32-unknown-unknown` emits the target-neutral program section plus a
    wasm-bindgen `run_rom(bytes: Vec<u8>)` driver (JS `Uint8Array`), builds a
    fresh cdylib offline in release mode with the exact `wasm-bindgen` 0.2.129
    pin, and runs the repository-owned `target/tools/bin/wasm-bindgen` CLI
    (`--target web --no-typescript`) to produce an importable JS glue module plus
    `_bg.wasm`. `run_rom` checks the ROM length and SHA-256 before constructing
    the cartridge, CPU or bus, initializes the direct-entry CPU and BIOS stacks,
    executes the build-time instruction budget and returns a compact state
    report. Core errors become JavaScript errors; there is no filesystem,
    process or stdout boundary. Other `wasm32*`/`wasm64*` triples are rejected
    explicitly. The tool is verified at exactly 0.2.129 and never replaced by a
    global or auto-installed binary; a missing or mismatched CLI fails with an
    instruction to run `scripts/bootstrap-web-tools.sh`.
  `artifact.meta` and `build.meta` are ordered `key=value` files with ROM
  identity, target, entry/state, budget, discovery counts, generated-source
  SHA-256 and actual Cargo/rustc versions and host where available. The native
  branch records the produced executable hash; the WASM branch records
  `artifact_kind=wasm-module`, `module_sha256` and `js_glue_sha256` instead, and
  both branches' `build.meta` records the exact `wasm_bindgen_version` (also in
  `artifact.meta` for the WASM branch). Metadata contains no absolute paths,
  timestamps or ROM bytes. A generated `README.md` in the WASM workspace
  documents the `run_rom(Uint8Array)` call and the external-ROM binding.
  Neither branch is a playable standalone game, GUI frontend, save-aware app,
  final application or complete browser app: the WASM artifact is a
  browser-callable execution module with no HTML/JS UI, GPU renderer, input or
  persistence. Automated tests use synthetic fixtures; local supplied-ROM
  attempts and their outcomes are recorded in the verification log. The latest
  bounded native run stops at an unsupported BIOS-region access at
  `0x0000000A` (`direct-entry mode has no BIOS`); BIOS access remains outside the
  implemented subset. This is not a compatibility, boot or playability claim.
- Generated paths are restricted to `<cwd>/output/...`; relative and absolute
  paths are resolved into that root, symlinks below it are refused, and the
  location is validated before any directory is created. An existing explicit
  `--out` directory is refused rather than reused or overwritten.
- Scaling is integer nearest-neighbor only, with exact 3:2 centering and black
  letterboxing. Vendor backends are rejected with a clear error.

The optional desktop frontend is a separate package (`apps/desktop/`, its own
Cargo workspace and lockfile). It depends directly on `crates/core`
(`gba-native-core`, aliased to the `gba_native` source namespace), never on the
host-side AOT crate or the root facade, and is not part of the default std-only
core graph:

```text
./scripts/frontend.sh   # optional: hard line gate, fmt, clippy, unit tests, build, --help
```

The gate never opens a GPU window. GUI smoke and the exact artifact rules are in
[`docs/gpu-frontend.md`](docs/gpu-frontend.md).

The browser foundation is a separate package and gate:

```text
./scripts/bootstrap-web-tools.sh  # pinned Node/wasm-bindgen/npm tooling in target/
./scripts/web.sh                  # full browser gate (requires wasm32 and Chromium)
./scripts/web.sh --build-only     # partial build checks without browser E2E
./scripts/web-screenshots.sh      # optional UI-review captures; not a gate
```

It runs the synthetic demo through the std-only core and presents one static
image using WebGPU or WebGL2. It is not a per-ROM browser application. Build
details, observed browser results, explicit gaps and the pending visual review
are in [`docs/web-frontend.md`](docs/web-frontend.md).

The separate `aot build --target wasm32-unknown-unknown` path produces a
ROM-bound WASM *execution module* and JS glue for a caller that already has a
browser app; it does not add HTML/JS, a renderer, input or persistence, and it
is not part of `apps/web` or the browser gate. Its build requires the pinned
wasm-bindgen CLI from `./scripts/bootstrap-web-tools.sh` and the installed
`wasm32-unknown-unknown` Rust target.

## Build and test

```text
./scripts/short.sh     # fast: hard 500-line gate, Ratchet fast, fmt, clippy, fast tests
./scripts/full.sh      # short + debug/release tests, demo, native proof, Ratchet slow
./scripts/frontend.sh  # optional desktop package: no Ratchet, no live GPU window
./scripts/web.sh --ci  # optional browser package and fail-closed Chromium E2E
```

Per-package commands are available when a smaller loop is useful:

```text
cargo test -p gba-native-core
cargo test -p gba-native-aot
cargo test -p gba-native
cargo run --bin quality-counter -- oversized --root . --max 500 --hard
```

The underlying gates are `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test` and
`cargo test --release`. The root workspace lists the root package,
`crates/core` and `crates/aot` in `default-members`, so those bare commands
select all three; `apps/desktop` and `apps/web` are explicitly excluded. Ratchet is pinned to
release `v0.5.0` of `all-the-rest/ratchet-cli` (peeled commit
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
That population includes `apps/desktop/src`, `apps/web/src` and frontend tests. The
root-level `target/`, `output/` and `.git` directories hold generated
build/runtime or version-control artifacts and are not part of the maintained
source population. The exemption is positional: a nested directory with one of
those names, such as `src/output/` or `apps/desktop/target/`, is scanned like
any other source directory. `scripts/short.sh`, `scripts/frontend.sh`,
`scripts/web.sh` and CI run the hard limit directly, so the zero-debt Ratchet baseline cannot be edited to
dilute it.

The frontend gates override `CARGO_TARGET_DIR` to root-level ignored locations
before frontend Cargo commands. The desktop gate uses `target/frontend`; the web
gate uses `target/web` and keeps Node/npm/wasm-bindgen tools under `target/tools`
and `target/web-tools`. Do not run frontend Cargo directly inside either app
without a root-level target: nested generated files are scanned as maintained
sources. `apps/web/node_modules` is not created.

The native integration test requires only the installed Cargo/rustc. It creates
a fresh generated crate in an owned temporary directory, uses `--offline` and a
separate `CARGO_TARGET_DIR`, and removes that directory afterwards. The harness
builder accepts only a non-existing crate directory, creates its files with
`create_new`, and generates a manifest that depends only on the sibling
`gba-native-core` package (never the AOT crate or root facade). A separate
target-profile test derives the installed host triple and builds the generated
harness in release mode with `--target`, then runs it against the synthetic
fixture and checks wrong-ROM rejection; it is host-toolchain evidence, not a
cross-compilation claim. The workspace contract tests additionally list the
complete baseline test inventory in a fresh target directory with an empty
`CARGO_HOME`, and execute the bare CLI commands from an unrelated working
directory. No network, third-party crate, commercial ROM or BIOS is needed.

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
- Desktop/browser frontend, standalone builds, Android/iOS —
  [#9](https://github.com/reisi007/gba-native/issues/9),
  [#10](https://github.com/reisi007/gba-native/issues/10),
  [#11](https://github.com/reisi007/gba-native/issues/11),
  [#24](https://github.com/reisi007/gba-native/issues/24)
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

These issues remain open; the bounded SWI/HLE implementation and its synthetic
tests do not close the broader CPU, playability or REA issues. The reference
interpreter already has a committed self-modifying-RAM regression
(`crates/core/tests/cpu.rs::ram_code_is_decoded_fresh_after_self_modification`).
Issue #17 now also has the dedicated *native harness* regression
(`crates/aot/tests/aot_ram.rs`): four fresh RAM fallbacks across two visits, no
RAM block in the artifact, and the patched `MOV r0,#2` observed. It remains
open pending independent verification and CI; local tests alone do not close it.

## Supported subset (short version)

- ARM: all sixteen data-processing operations (`AND`, `EOR`, `SUB`, `RSB`,
  `ADD`, `ADC`, `SBC`, `RSC`, `TST`, `TEQ`, `CMP`, `CMN`, `ORR`, `MOV`, `BIC`,
  `MVN`) with immediate or register operands and LSL/LSR/ASR/ROR/RRX shifts by
  immediate or by the low eight bits of a register; `B`, `BL` and `BX`.
  Documented PC reads (`+8`, or `+12` for register-specified shifts), ALU PC
  writes with `S=0`, and PC wrapping are implemented. `S=1` with `Rd=PC`
  requires an SPSR restore and stays an explicit error; ARMv5/newer exchange
  encodings are rejected. `MRS`/`MSR` remain as before. Single word/byte
  `LDR`/`STR` support immediate-12 or shifted-register offsets, pre/post
  indexing, writeback and the documented PC base/source rules; `LDRH`/`STRH`/
  `LDRSB`/`LDRSH` add pre/post immediate or unshifted register offsets; ARM
  `LDM`/`STM` cover IA/IB/DA/DB with PC entries and ARMv4 base-in-list
  semantics. Condition fields 0..14 are evaluated; field 1111b is reserved and
  rejected explicitly. Immediate payloads are classified by bit 25 and
  independently swept over all imm8/rotation values.
- CPU state: the seven ARM7TDMI modes, shared r0-r7, FIQ-only r8-r12 banking,
  per-exception r13/r14 banks with USER/SYSTEM sharing, and independent
  FIQ/IRQ/SVC/ABT/UND SPSRs. MSR mode writes switch banks; USER control writes
  are ignored as documented; invalid modes and T-bit changes error before
  mutation; USER/SYSTEM SPSR access errors. Typed ARM/Thumb SWI has a bounded
  SVC frame, six-service HLE dispatch and full CPSR return. IRQ delivery uses
  the host-driven frame model and the documented IWRAM handler pointer; BIOS
  boot is not implemented.
- Thumb: register ALU operations including `MUL`, shift by
  immediate and by register, `ADC`/`SBC`/`NEG`/`TST`/`CMN`, `MOV`/`ADD`/`SUB`
  register and immediate forms, high-register `ADD`/`CMP`/`MOV` (low/low
  `MOV`/`CMP` are an explicit **Design** extension beyond the documented
  high-register encodings), `ADR`, `ADD PC/SP`, `ADD`/`SUB SP,#imm`, both
  halves of `BL`, `BX`, literal `LDR`, word/byte register and immediate
  transfers, SP-relative word transfers, immediate/register `LDRH`/`STRH`,
  register `LDSB`/`LDSH`, `PUSH`/`POP`, `LDMIA`/`STMIA`, and conditional or
  unconditional `B`, and typed SWI to the six documented HLE services. The
  ARMv5 `BLX` second half and register form are rejected;
  empty stack/block lists and unaligned SP/base are explicit errors.
- Memory/I/O: EWRAM and IWRAM mirrors, exact palette/VRAM/OAM regions, three ROM
  windows, selected display/input/WAITCNT/DMA/IRQ/sound/timer registers. Unused
  I/O writes are ignored and recorded; unmodelled reads remain errors. There is
  no open bus, save support, cycle timing, audio generation or ticking timer.
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
