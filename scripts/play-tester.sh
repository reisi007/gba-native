#!/usr/bin/env bash
#
# Native playback gate: an executable fake platform player is placed first on
# PATH, then the CLI and the generated AOT harness must invoke it with the
# generated WAV and report audio_played=true. A nonzero player exit must be a
# hard error. Synthetic fixture only; no retail ROM, save or BIOS is involved.
#
# This runs on Linux and Windows CI (Git Bash) as well as locally on macOS.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo

ROM="tests/roms/synthetic/input-tester.gba"
SCRIPT="tests/roms/synthetic/input-tester.input"
BUDGET=2000000

case "$(uname -s)" in
  Linux)
    TARGET="x86_64-unknown-linux-gnu"
    PLAYER_NAME="paplay"
    ;;
  Darwin)
    case "$(uname -m)" in
      arm64 | aarch64) TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) fail "unsupported macOS architecture: $(uname -m)" ;;
    esac
    PLAYER_NAME="afplay"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    case "$(uname -m)" in
      x86_64 | amd64) TARGET="x86_64-pc-windows-msvc" ;;
      *) fail "unsupported Windows architecture: $(uname -m)" ;;
    esac
    PLAYER_NAME="powershell"
    ;;
  *) fail "unsupported host for the playback gate: $(uname -s)" ;;
esac
EXE_SUFFIX=""
case "$TARGET" in *windows*) EXE_SUFFIX=".exe" ;; esac
PLAYER="$PLAYER_NAME$EXE_SUFFIX"

mkdir -p "$ROOT/output"
WORK=$(mktemp -d "$ROOT/output/play-tester.XXXXXX")
info "playback workspace: $WORK"
info "target: $TARGET; fake player: $PLAYER_NAME"

rustc --edition 2021 "$ROOT/tests/support/fake_player.rs" -O -o "$WORK/$PLAYER"

check_played() {
  report=$1
  wav=$2
  grep -qx "audio_played=true" "$report" ||
    fail "playback report did not set audio_played=true"
  grep -Eq "^audio_player=.*${PLAYER_NAME}${EXE_SUFFIX}$" "$report" ||
    fail "playback report did not name the fake $PLAYER_NAME player"
  [ -f "$wav" ] || fail "playback gate lost the generated WAV at $wav"
  grep -q "$(basename "$wav")" "$3" ||
    fail "the fake player did not receive the generated WAV"
}

info "CLI playback through a fake player on PATH"
PATH="$WORK:$PATH" \
  GBA_NATIVE_FAKE_PLAYER_RECORD="$WORK/cli.record" \
  cargo run --quiet --offline --bin gba-native -- run "$ROM" \
    --budget 200000 --audio-out "$WORK/cli.wav" --play >"$WORK/cli.txt"
check_played "$WORK/cli.txt" "$WORK/cli.wav" "$WORK/cli.record"

info "CLI rejects a nonzero player exit"
if PATH="$WORK:$PATH" \
  GBA_NATIVE_FAKE_PLAYER_RECORD="$WORK/fail.record" \
  GBA_NATIVE_FAKE_PLAYER_FAIL=1 \
  cargo run --quiet --offline --bin gba-native -- run "$ROM" \
    --budget 200000 --audio-out "$WORK/fail.wav" --play >"$WORK/fail.txt" 2>&1; then
  fail "a failing player must make the CLI fail"
fi
grep -q "failed with" "$WORK/fail.txt" ||
  fail "the failing player did not produce an explicit error"

info "native AOT harness playback through the fake player on PATH"
cargo run --quiet --offline --bin gba-native -- aot build "$ROM" \
  --target "$TARGET" --budget "$BUDGET" --out "$WORK/native" >"$WORK/build.txt"
HARNESS="$WORK/native/native/target/$TARGET/release/gba-native-aot-harness$EXE_SUFFIX"
[ -x "$HARNESS" ] || fail "expected harness is missing: $HARNESS"
PATH="$WORK:$PATH" \
  GBA_NATIVE_FAKE_PLAYER_RECORD="$WORK/harness.record" \
  "$HARNESS" --rom "$ROM" --input "$SCRIPT" \
    --audio "$WORK/harness.wav" --play >"$WORK/harness.txt"
check_played "$WORK/harness.txt" "$WORK/harness.wav" "$WORK/harness.record"
grep -qx "fallbacks=0" "$WORK/harness.txt" ||
  fail "the playback harness run used interpreter fallbacks"

info "native AOT harness rejects a nonzero player exit"
if PATH="$WORK:$PATH" \
  GBA_NATIVE_FAKE_PLAYER_RECORD="$WORK/harness-fail.record" \
  GBA_NATIVE_FAKE_PLAYER_FAIL=1 \
  "$HARNESS" --rom "$ROM" --input "$SCRIPT" \
    --audio "$WORK/harness-fail.wav" --play >"$WORK/harness-fail.txt" 2>&1; then
  fail "a failing player must make the harness fail"
fi
grep -q "failed with" "$WORK/harness-fail.txt" ||
  fail "the failing harness player did not produce an explicit error"

info "playback gate passed"
printf 'workspace=%s\n' "$WORK"
