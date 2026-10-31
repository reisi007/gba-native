#!/usr/bin/env bash
#
# Native save-tester gate.
#
# Builds the committed synthetic save-tester ROM through native AOT, runs it
# twice against the same raw backup image and requires the fresh and loaded
# frames. It also exercises the CLI save library with multiple slots, a default
# selection and import/export. No retail ROM, BIOS image or save data is used.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo

ROM="tests/roms/synthetic/save-tester.gba"
ROM_SHA256="f8dbe41f4dcd1e134759a23cce988b4a05f1185f102b3153d69240600e76c4e3"
FRESH_IMAGE="53ff58d4cb667ee14a36ec6d6723fbfa623a7ddc489c4a11551254cdeaad8755"
LOADED_IMAGE="259d7e7302a5d04611c276395f3f9a5901e7c65061b4fb0e6b9b6f786905434c"
FRESH_SAVE="39640902e8ceb5ebcce5769a9d891df7e0c3f1a9d8b0c3b0c175a34f3a823dca"
LOADED_SAVE="bc470afabfeeece7e51c912dabc4178e507fd6535e1402b36370ddb4f6f1e938"
BUDGET=5000000

case "$(uname -s)" in
  Linux) TARGET="x86_64-unknown-linux-gnu" ;;
  Darwin)
    case "$(uname -m)" in
      arm64 | aarch64) TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) fail "unsupported macOS architecture: $(uname -m)" ;;
    esac
    ;;
  *) fail "unsupported host for the native save-tester gate: $(uname -s)" ;;
esac
TARGET="${SAVE_TESTER_TARGET:-$TARGET}"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

[ "$(sha256_file "$ROM")" = "$ROM_SHA256" ] ||
  fail "committed save-tester ROM digest changed"

mkdir -p "$ROOT/output"
WORK=$(mktemp -d "$ROOT/output/save-tester.XXXXXX")
info "save-tester workspace: $WORK"

cargo run --quiet --offline --bin gba-native -- aot build "$ROM" \
  --target "$TARGET" --budget "$BUDGET" --synthetic-backup-sram --out "$WORK/native" \
  >"$WORK/build.txt"
HARNESS="$WORK/native/native/target/$TARGET/release/gba-native-aot-harness"
[ -x "$HARNESS" ] || fail "expected harness is missing: $HARNESS"

"$HARNESS" --rom "$ROM" --save "$WORK/slot-a.sav" >"$WORK/fresh.txt"
grep -qx "fallbacks=0" "$WORK/fresh.txt" || fail "fresh run used interpreter fallbacks"
grep -qx "image_sha256=$FRESH_IMAGE" "$WORK/fresh.txt" ||
  fail "fresh run produced an unexpected frame"
grep -qx "save_bytes=32768" "$WORK/fresh.txt" || fail "fresh save length is wrong"
grep -qx "save_sha256=$FRESH_SAVE" "$WORK/fresh.txt" || fail "fresh save digest is wrong"
info "fresh run: $FRESH_IMAGE"

"$HARNESS" --rom "$ROM" --save "$WORK/slot-a.sav" >"$WORK/loaded.txt"
grep -qx "image_sha256=$LOADED_IMAGE" "$WORK/loaded.txt" ||
  fail "loaded run did not read the persisted image"
grep -qx "save_sha256=$LOADED_SAVE" "$WORK/loaded.txt" ||
  fail "loaded run did not advance the persisted counter"
info "loaded run: $LOADED_IMAGE"

"$HARNESS" --rom "$ROM" --save "$WORK/slot-b.sav" >"$WORK/second.txt"
grep -qx "image_sha256=$FRESH_IMAGE" "$WORK/second.txt" ||
  fail "a separate save path must start fresh"

# CLI library: two independent slots, default selection, import/export.
cargo run --quiet --offline --bin gba-native -- run "$ROM" --budget "$BUDGET" \
  --synthetic-backup-sram --save-dir "$WORK/cli" --save-slot alpha >"$WORK/cli-alpha.txt"
grep -qx "save_sha256=$FRESH_SAVE" "$WORK/cli-alpha.txt" || fail "alpha save mismatch"
grep -qx "save_default=alpha" "$WORK/cli-alpha.txt" || fail "alpha is not the default"
cargo run --quiet --offline --bin gba-native -- run "$ROM" --budget "$BUDGET" \
  --synthetic-backup-sram --save-dir "$WORK/cli" --save-slot alpha >"$WORK/cli-alpha2.txt"
grep -qx "save_sha256=$LOADED_SAVE" "$WORK/cli-alpha2.txt" ||
  fail "CLI reload did not advance the counter"
cargo run --quiet --offline --bin gba-native -- run "$ROM" --budget "$BUDGET" \
  --synthetic-backup-sram --save-dir "$WORK/cli" --save-slot beta >"$WORK/cli-beta.txt"
grep -qx "save_sha256=$FRESH_SAVE" "$WORK/cli-beta.txt" || fail "beta save mismatch"
cargo run --quiet --offline --bin gba-native -- save list "$ROM" --dir "$WORK/cli" \
  >"$WORK/cli-list.txt"
grep -q "slots=2" "$WORK/cli-list.txt" || fail "CLI did not list two slots"
cargo run --quiet --offline --bin gba-native -- save use "$ROM" beta --dir "$WORK/cli" \
  >"$WORK/cli-use.txt"
grep -qx "default=beta" "$WORK/cli-use.txt" || fail "CLI default selection failed"
cargo run --quiet --offline --bin gba-native -- save export "$ROM" beta --dir "$WORK/cli" \
  --out "$WORK/export.sav" >"$WORK/cli-export.txt"
cargo run --quiet --offline --bin gba-native -- save import "$ROM" gamma --dir "$WORK/cli" \
  --file "$WORK/export.sav" >"$WORK/cli-import.txt"
cargo run --quiet --offline --bin gba-native -- save list "$ROM" --dir "$WORK/cli" \
  >"$WORK/cli-list2.txt"
grep -q "slot=gamma" "$WORK/cli-list2.txt" || fail "CLI import did not create gamma"
info "CLI slots, default and import/export matched"

info "save-tester gate passed"
printf 'workspace=%s\n' "$WORK"
