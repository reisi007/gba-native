#!/usr/bin/env bash
# Build the Rune browser bundle; the canonical web gate also runs Chromium checks.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
MANIFEST="$ROOT/apps/rune/Cargo.toml"
PROGRAM="$ROOT/output/rune-bound/rom_program.rs"

# The host CLI owns AOT discovery; apps/rune must not depend on that tooling.
if [ -n "${GBA_NATIVE_ROM_PATH:-}" ]; then
  [ -f "$GBA_NATIVE_ROM_PATH" ] || {
    printf 'GBA_NATIVE_ROM_PATH is not a file: %s\n' "$GBA_NATIVE_ROM_PATH" >&2
    exit 1
  }
  mkdir -p "$(dirname -- "$PROGRAM")"
  (cd "$ROOT" && cargo run --quiet --locked -- aot emit-program "$GBA_NATIVE_ROM_PATH" "$PROGRAM")
  export GBA_NATIVE_PROGRAM_PATH="$PROGRAM"
fi

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
# A ROM-bound build fetches its one fixed game from the same origin. The bundle
# is a local artifact: never serve it publicly or commit the copied ROM file.
if [ -n "${GBA_NATIVE_ROM_PATH:-}" ]; then
  if [ -n "${CI:-}" ] && [ "$CI" != "false" ] && [ "$CI" != "0" ]; then
    printf 'Refusing to stage a retail ROM bundle in CI (GBA_NATIVE_ROM_PATH is set).\n' >&2
    exit 1
  fi
  [ -f "$GBA_NATIVE_ROM_PATH" ] || {
    printf 'GBA_NATIVE_ROM_PATH is not a file: %s\n' "$GBA_NATIVE_ROM_PATH" >&2
    exit 1
  }
  cp "$GBA_NATIVE_ROM_PATH" "$DIST/$(basename -- "$GBA_NATIVE_ROM_PATH")"
fi
printf 'Rune browser bundle: %s\nServe this directory on localhost or HTTPS. This is not a browser E2E test.\n' "$DIST"
