#!/usr/bin/env bash
# Build an isolated experimental Rune browser bundle; this is not the web gate.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
MANIFEST="$ROOT/apps/rune/Cargo.toml"
export CARGO_TARGET_DIR="$ROOT/target/rune"
BINDGEN=${WASM_BINDGEN:-"$ROOT/target/tools/bin/wasm-bindgen"}

if [ ! -x "$BINDGEN" ]; then
  printf 'Pinned wasm-bindgen is missing: %s\nRun scripts/bootstrap-web-tools.sh or set WASM_BINDGEN.\n' "$BINDGEN" >&2
  exit 1
fi
if [ "$("$BINDGEN" --version)" != 'wasm-bindgen 0.2.129' ]; then
  printf 'Expected wasm-bindgen 0.2.129: %s\n' "$BINDGEN" >&2
  exit 1
fi

cargo build --locked --manifest-path "$MANIFEST" --lib --target wasm32-unknown-unknown
DIST=$(mktemp -d "$ROOT/target/rune-web-XXXXXXXX")
mkdir "$DIST/pkg"
"$BINDGEN" --target web --no-typescript --out-dir "$DIST/pkg" --out-name gba_native_rune \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/debug/gba_native_rune.wasm"
cp "$ROOT/apps/rune/www/index.html" "$DIST/index.html"
cp "$ROOT/apps/rune/www/app.js" "$DIST/app.js"
cp "$ROOT/apps/rune/www/app.css" "$DIST/app.css"
printf 'Rune browser bundle: %s\nServe this directory on localhost or HTTPS. This is not a browser E2E test.\n' "$DIST"
