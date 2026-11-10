#!/usr/bin/env bash
# Build a local ROM-bound Rune app.
#
# The host CLI pre-emits the translated program so apps/rune has no build-time
# dependency on host-side AOT tooling. Extra arguments go to `cargo build`,
# for example `--release`, `--target x86_64-pc-windows-gnu` or `--bin gba-native-rune`.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/rune-bound}"
if [ -z "${GBA_NATIVE_ROM_PATH:-}" ]; then
  fail "set GBA_NATIVE_ROM_PATH to the single cartridge for this local build"
fi
[ -f "$GBA_NATIVE_ROM_PATH" ] || fail "GBA_NATIVE_ROM_PATH is not a file: $GBA_NATIVE_ROM_PATH"

PROGRAM="$ROOT/output/rune-bound/rom_program.rs"
mkdir -p "$(dirname -- "$PROGRAM")"
cargo run --quiet --locked -- aot emit-program "$GBA_NATIVE_ROM_PATH" "$PROGRAM"
export GBA_NATIVE_PROGRAM_PATH="$PROGRAM"

exec cargo build --manifest-path "$ROOT/apps/rune/Cargo.toml" "$@"
