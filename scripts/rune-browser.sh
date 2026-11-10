#!/usr/bin/env bash
# Headless WASM and browser-adapter gate for the isolated Rune UI package.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo

# The host CLI owns AOT discovery; apps/rune must not depend on that tooling.
if [ -n "${GBA_NATIVE_ROM_PATH:-}" ]; then
  [ -f "$GBA_NATIVE_ROM_PATH" ] || fail "GBA_NATIVE_ROM_PATH is not a file: $GBA_NATIVE_ROM_PATH"
  PROGRAM="$ROOT/output/rune-bound/rom_program.rs"
  mkdir -p "$(dirname -- "$PROGRAM")"
  cargo run --quiet --locked -- aot emit-program "$GBA_NATIVE_ROM_PATH" "$PROGRAM"
  export GBA_NATIVE_PROGRAM_PATH="$PROGRAM"
fi

export CARGO_TARGET_DIR="target/rune"
NODE="$ROOT/target/tools/node-pinned/bin/node"

[ -x "$NODE" ] || fail "pinned Node is missing; run scripts/bootstrap-web-tools.sh"
[ "$("$NODE" --version)" = "v24.22.1" ] || fail "expected pinned Node v24.22.1"
[ -x "$ROOT/target/tools/bin/wasm-bindgen" ] ||
  fail "pinned wasm-bindgen is missing; run scripts/bootstrap-web-tools.sh"

info "Rune WASM all-targets check"
cargo check --locked --manifest-path apps/rune/Cargo.toml --all-targets \
  --target wasm32-unknown-unknown

info "Rune browser bundle"
bash scripts/rune-web.sh

info "Rune browser adapter tests (Node VM stubs, no browser/GPU claim)"
"$NODE" --experimental-vm-modules --test \
  apps/rune/tests/browser-shell.test.mjs \
  apps/rune/tests/browser-save.test.mjs

info "Rune browser gate passed (no screen-reader, audible-output or GPU claim)"
