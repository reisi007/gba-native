#!/usr/bin/env bash
# Build the canonical Rune WASM app and serve it on loopback for browser use.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
NODE="$ROOT/target/tools/node-pinned/bin/node"
mkdir -p "$ROOT/target"
BUILD_LOG=$(mktemp "$ROOT/target/rune-web-preview-build.XXXXXXXX")
cleanup() { rm -f "$BUILD_LOG"; }
trap cleanup EXIT INT TERM

[ -x "$NODE" ] || {
  printf 'Pinned Node is missing; run scripts/web-browsers-install.sh first.\n' >&2
  exit 1
}

if ! bash "$ROOT/scripts/rune-web.sh" >"$BUILD_LOG" 2>&1; then
  cat "$BUILD_LOG" >&2
  exit 1
fi
cat "$BUILD_LOG"
DIST=$(sed -n 's/^Rune browser bundle: //p' "$BUILD_LOG" | tail -n 1)
[ -d "$DIST" ] || {
  printf 'Rune browser bundle directory was not reported.\n' >&2
  exit 1
}
rm -f "$BUILD_LOG"
trap - EXIT INT TERM

printf 'Serving Rune on loopback; press Ctrl-C to stop.\n'
exec "$NODE" "$ROOT/apps/rune/tools/serve-static.mjs" "$DIST"
