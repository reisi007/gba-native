#!/usr/bin/env bash
# Render the shared Rune app offscreen through the native GPU backend.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo
export CARGO_TARGET_DIR="target/rune"
BIN="$CARGO_TARGET_DIR/debug/gba-native-rune"
[ -x "$BIN" ] || cargo build --locked --manifest-path apps/rune/Cargo.toml --bin gba-native-rune

mkdir -p "$ROOT/output"
OUT=$(mktemp -d "$ROOT/output/rune-native-review-$(date +%Y%m%d-%H%M%S)-XXXXXXXX")
SNAPSHOT="$OUT/demo.png"

info "render Rune's full app view using the native offscreen GPU path"
"$BIN" --size 1280x900 --scale 1 --snap "$SNAPSHOT" --first-pixels
[ -s "$SNAPSHOT" ] || fail "Rune did not produce the native snapshot"
python3 -c '
import pathlib, struct, sys
data = pathlib.Path(sys.argv[1]).read_bytes()
if data[:8] != b"\x89PNG\r\n\x1a\n" or len(data) < 24:
    raise SystemExit("Rune snapshot is not a valid PNG")
size = struct.unpack(">II", data[16:24])
if size != (1280, 900):
    raise SystemExit(f"unexpected Rune snapshot dimensions: {size}")
' "$SNAPSHOT"

info "native Rune offscreen render passed; capture: ${SNAPSHOT#"$ROOT"/}"
