#!/usr/bin/env bash
#
# Native input-tester gate.
#
# Builds the committed synthetic input-tester ROM from its source, compiles the
# generated AOT harness for the host triple, runs it with the committed input
# script and compares the actual frame with the committed expected frame. With
# --visual it additionally renders both frames in Chromium through Playwright and
# compares every pixel, writing a screenshot artifact.
#
# This is synthetic fixture evidence only: no retail ROM, BIOS image, save data
# or vendored ROM bytes are involved. The default repository gates do not need
# Node, Playwright or a browser; only this script does, and only with --visual.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo

VISUAL=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --visual) VISUAL=1 ;;
    *) fail "scripts/input-tester.sh: unknown argument $1 (expected --visual)" ;;
  esac
  shift
done

ROM_SHA256="8a7667d43d9ec0333a5a7c0a032837919e101ca1c1893bdf5f95f51b27555539"
EXPECTED_FILE_SHA256="2824c8e9f509c1294c555af3318cec4d3fd4a37aa964f60e9ecc974721da941a"
EXPECTED_IMAGE_SHA256="5cbbd998a2bc5a026ebac07d91a0f058b769cb05dd14c09e30d52888ed80db8b"
BUDGET=20000000
EXPECTED="tests/roms/synthetic/input-tester.expected.ppm"
SCRIPT="tests/roms/synthetic/input-tester.input"

case "$(uname -s)" in
  Linux) TARGET="x86_64-unknown-linux-gnu" ;;
  Darwin)
    case "$(uname -m)" in
      arm64 | aarch64) TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) fail "unsupported macOS architecture: $(uname -m)" ;;
    esac
    ;;
  MINGW* | MSYS* | CYGWIN*)
    case "$(uname -m)" in
      x86_64 | amd64) TARGET="x86_64-pc-windows-msvc" ;;
      *) fail "unsupported Windows architecture: $(uname -m)" ;;
    esac
    ;;
  *) fail "unsupported host for the native input-tester gate: $(uname -s)" ;;
esac
TARGET="${INPUT_TESTER_TARGET:-$TARGET}"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    fail "no SHA-256 tool available (sha256sum or shasum)"
  fi
}

mkdir -p "$ROOT/output"
WORK=$(mktemp -d "$ROOT/output/input-tester.XXXXXX")
info "workspace: $WORK"
info "target: $TARGET"

info "regenerating the synthetic fixture from source"
cargo run --quiet --offline --bin gba-native -- fixture input-tester \
  --out "$WORK/input-tester.gba" >"$WORK/fixture.txt"
ACTUAL_ROM_SHA=$(sha256_file "$WORK/input-tester.gba")
[ "$ACTUAL_ROM_SHA" = "$ROM_SHA256" ] ||
  fail "regenerated ROM SHA-256 is $ACTUAL_ROM_SHA; expected $ROM_SHA256"
[ "$(sha256_file tests/roms/synthetic/input-tester.gba)" = "$ROM_SHA256" ] ||
  fail "the committed input-tester.gba changed; regenerate and update the README"
[ "$(sha256_file "$EXPECTED")" = "$EXPECTED_FILE_SHA256" ] ||
  fail "the committed expected PPM changed; update the README and CI pins"

info "building the native AOT harness"
cargo run --quiet --offline --bin gba-native -- aot build tests/roms/synthetic/input-tester.gba \
  --target "$TARGET" --budget "$BUDGET" --out "$WORK/native" >"$WORK/build.txt"

EXE_SUFFIX=""
case "$TARGET" in *windows*) EXE_SUFFIX=".exe" ;; esac
HARNESS="$WORK/native/native/target/$TARGET/release/gba-native-aot-harness$EXE_SUFFIX"
[ -x "$HARNESS" ] || fail "expected harness is missing: $HARNESS"

info "running the harness with the committed input script"
"$HARNESS" --rom tests/roms/synthetic/input-tester.gba \
  --input "$SCRIPT" --snapshot "$WORK/actual.ppm" >"$WORK/report.txt"
cat "$WORK/report.txt"

grep -qx "image_sha256=$EXPECTED_IMAGE_SHA256" "$WORK/report.txt" ||
  fail "native frame hash does not match the expected $EXPECTED_IMAGE_SHA256"
grep -qx "fallbacks=0" "$WORK/report.txt" ||
  fail "the native input-tester run used interpreter fallbacks"
grep -qx "budget_used=$BUDGET" "$WORK/report.txt" ||
  fail "the native input-tester run did not consume the requested budget"

cmp -s "$WORK/actual.ppm" "$EXPECTED" ||
  fail "actual frame bytes differ from the committed expected frame"
info "actual frame matches the committed expected frame byte for byte"

info "native guest audio artifact (volume 80)"
"$HARNESS" --rom tests/roms/synthetic/input-tester.gba \
  --input "$SCRIPT" --audio "$WORK/actual.wav" --volume 80 >"$WORK/audio-report.txt"
grep -qx "audio_audible=true" "$WORK/audio-report.txt" ||
  fail "native guest audio was silent"
grep -qx "audio_master_volume=80" "$WORK/audio-report.txt" ||
  fail "native audio volume was not applied"
grep -qx "audio_muted=false" "$WORK/audio-report.txt" ||
  fail "native audio mute state changed unexpectedly"
grep -qx "audio_direct_sound_samples=0" "$WORK/audio-report.txt" ||
  fail "the PSG-only fixture consumed Direct Sound FIFO samples"
[ -s "$WORK/actual.wav" ] || fail "native audio WAV is missing"
info "native guest audio matched: $(grep '^audio_wav_sha256=' "$WORK/audio-report.txt")"

if [ "$VISUAL" = "1" ]; then
  NODE_BIN="$ROOT/target/tools/node-pinned/bin/node"
  [ -x "$NODE_BIN" ] ||
    fail "the pinned Node runtime is missing; run ./scripts/bootstrap-web-tools.sh"
  [ -f "$ROOT/target/web-tools/npm/node_modules/@playwright/test/index.js" ] ||
    fail "Playwright tooling is missing; run ./scripts/bootstrap-web-tools.sh"
  info "browser visual comparison (Playwright/Chromium)"
  "$NODE_BIN" "$ROOT/scripts/input-tester-visual.mjs" \
    --actual "$WORK/actual.ppm" \
    --expected "$EXPECTED" \
    --screenshot "$WORK/visual.png" \
    --png "$WORK/actual.png"
  info "visual artifact: $WORK/visual.png"
else
  info "visual check skipped (pass --visual and provision the pinned browser tools)"
fi

info "input-tester gate passed"
printf 'workspace=%s\n' "$WORK"
