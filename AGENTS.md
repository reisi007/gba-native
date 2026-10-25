# Repository agent instructions

## What this repository is

Experimental hybrid GBA-to-native prototype in Rust 2024, std-only. It is not a
playable emulator, not a retail-game compatibility claim, not a vendor-GPU/FSR/
DLSS integration, and not a formal two-team legal clean-room. REA native
analysis remains blocked and no REA Evidence exists. Read
`docs/clean-room/protocol.md` and `gba-contract.md` before changing behavior.

## Issue tracker

GitHub Issues are the authoritative open-work record:
<https://github.com/reisi007/gba-native/issues>. Do not add a second local
backlog. An explicit user commission is still required before work starts, and
an issue is closed only after independent verification, the repository gates
and CI pass, with the issue reference in the change. Lasting decisions live in
`docs/decisions.md`; completed work is recorded in Git.

## Green build gates

The documented entry points are:

```text
./scripts/short.sh   # fast: hard 500-line gate, Ratchet fast, fmt, clippy, fast tests
./scripts/full.sh    # short + debug/release tests, demo, native proof, Ratchet slow
```

The optional synthetic GPU frontend is a separate package and gate:

```text
./scripts/frontend.sh
```

It is not part of `short.sh`/`full.sh`, does not require Ratchet and never opens
a GPU window. It overrides `CARGO_TARGET_DIR` to the root-level ignored
`target/frontend` before any frontend Cargo command. Never run frontend Cargo
from inside `apps/desktop/` without a root-level target: generated
`apps/desktop/target/*.rs` files would enter the maintained-source scan.

CI runs the same two scripts. The optional frontend workflow runs
`scripts/frontend.sh` separately. The underlying gates remain:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --release
```

Ratchet is pinned to release `v0.5.0` of `all-the-rest/ratchet-cli` (peeled
commit `99c25b1e18331a1473d43091d37e23f23bc1c30d`; the annotated tag object is
`1feb7b5fae653272a922ed9f0479660ecde22ed2`). The scripts use `RATCHET_BIN` when
set, otherwise `ratchet` on `PATH`, and fail with diagnostics when the tool is
missing or reports another version. Baselines are machine-written with
`ratchet baseline init/update`; never hand-edit `ratchet.baseline.json`.

## Source size limit

Every maintained Rust source or test file has at most 500 physical lines,
including blank and comment lines; a trailing newline adds no phantom line.
There are no source exemptions. Generated build/runtime artifacts in the
root-level ignored directories `target/`, `output/` and `.git` are not
maintained sources and are skipped by the counter. Those names are exempt only
directly at the scan root: a nested `src/output/`, `tests/target/` or similar is
ordinary source-tree structure and is scanned. Every other `*.rs` file counts,
including root files, `build.rs`, tests and new untracked sources; an empty
population is an error.

`src/bin/quality-counter` is the single counting implementation. It feeds the
hard gate (`--hard`), the Ratchet rules and reports. The hard gate is enforced
directly by `scripts/short.sh` and CI as well as by a zero-debt Ratchet ceiling,
so a baseline edit or `baseline raise` cannot dilute it.

`cargo test` includes the substantive AOT integration test in
`tests/aot_native.rs`. That test writes an independently generated synthetic ROM
fixture, generates a Rust harness that binds SHA-256+length (never an embedded
ROM array), builds it with the installed `cargo` inside an owned temporary
directory, sets a separate `CARGO_TARGET_DIR`, uses `--offline`, executes it with
an explicit `--rom PATH`, and compares the native state with the IR reference.
It also checks fallback, wrong-ROM rejection and an independently expected
immediate-family regression. `tests/isa_golden.rs` checks decode/flags/memory
against hand-derived ISA expectations.

## Fresh independent verification

Use a clean target directory so stale artifacts cannot hide a failure:

```text
CARGO_TARGET_DIR="$(mktemp -d)/target" cargo test
CARGO_TARGET_DIR="$(mktemp -d)/target" cargo test --release
cargo run -- demo --out output/verify-demo.ppm
cargo run -- aot proof --out output/verify-aot
```

Do not delete or overwrite `output/` contents that you did not create. All
generated artifacts belong under the git-ignored `output/` directory or an owned
temporary directory.

## End-to-end evidence commands

```text
cargo run -- demo --size 480x320 --backend nearest
cargo run -- aot proof
cargo run -- info "<optional user-supplied .gba>"   # read-only
```

Expected native proof properties: 4 discovered ROM blocks / 17 translated
instructions, 230,411 guest instructions, 0 fallbacks, and
`native_vs_reference=match`. This is evidence for the synthetic fixture only.

## Hard boundaries

- Never modify, copy, embed or commit the user-supplied `.gba` file, Nintendo
  logo bytes, BIOS bytes or save data. The original ROM is read-only input.
  Generated AOT source must not contain a ROM byte array or raw image copy; bind
  SHA-256+length and require a caller-supplied `--rom PATH`.
- Do not use third-party emulator implementation sources, game decompilations,
  BIOS disassemblies or leaked SDKs.
- `#![forbid(unsafe_code)]` applies to the core. Do not add `unsafe`, JIT memory
  mappings, executable heap memory or a dependency on a JIT.
- Do not silently turn unsupported instructions, I/O, PPU features or vendor
  backends into successful no-ops or nearest-neighbor fallbacks.
- CPU instruction budgets are not frame or cycle timing. Do not report a
  completed budget as a booted game or a rendered frame from hardware timing.
- The default dependency set stays empty. Tests must not require network access,
  commercial ROMs, BIOS files or external crates.
- AOT is restricted to immutable ROM bytes. RAM/self-modifying code must always
  go through the reference interpreter with a fresh fetch/decode.

## Source layout

- `src/cart.rs`, `src/bus.rs`, `src/cpu.rs`: header, explicit memory map, state.
- `src/ir.rs`, `src/decode/`, `src/interpret.rs`: typed IR, independent
  ARM/Thumb decode (`arm.rs`, `thumb.rs`, tests) and reference semantics.
- `src/display.rs`, `src/render.rs`: bitmap snapshot, nearest/letterbox, PPM.
- `src/demo.rs`: entirely synthetic ROM fixtures.
- `src/bios/`: six direct, documentation-driven HLE service primitives
  (`mod.rs`, `arithmetic.rs`, `memory.rs`); no guest SWI/SVC integration.
- `src/aot/`: bounded discovery (`discover.rs`), artifact binding
  (`artifact.rs`), Rust emission (`emit.rs`), report parsing (`report.rs`),
  harness build/run (`harness.rs`) and tests.
- `src/main.rs`, `src/cli/`: CLI dispatch, commands and safe `output/` writes.
- `apps/desktop/`: optional, separate synthetic Winit/Pixels/WGPU package with
  its own workspace/lockfile; no core dependency changes and no ROM surface.
- `src/bin/quality-counter/`: the maintained-Rust population and 500-line
  counter used by the hard gate and Ratchet.
- `tests/`: header/bus/CPU/display/demo tests, direct BIOS HLE arithmetic/
  memory-copy/reset/dispatch tests, independent ISA golden vectors, the native
  AOT proof, quality-counter/script failure paths and the CI gate contracts.
- `scripts/short.sh`, `scripts/full.sh`: documented fast and full gates;
  `scripts/native-consistency.sh` is the slow Ratchet counter.
- `ratchet.config.jsonc`, `ratchet.baseline.json`: machine-managed quality
  rules and baseline.

## Licensing / publication

No license has been selected yet. After the content/history audit, the owner
may make the repository public before the first CI push; automation does not
change visibility. Do not publish ROMs, BIOS files, save data, generated
retail-derived translations or vendor SDK/model weights.
