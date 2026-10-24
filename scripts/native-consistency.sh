#!/usr/bin/env bash
#
# Slow Ratchet counter: the native/reference consistency proof.
#
# This is a differential check between the AOT emitter and the shared reference
# interpreter. It establishes compiler consistency for the synthetic fixture,
# not ISA correctness; the independent golden vectors are the ISA-subset
# evidence. Prints 1 only when the proof reports ok, a native/reference match
# and zero fallbacks. A failed proof is a counter error, never a zero.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo

WORK=$(owned_temp native-consistency)
cleanup() {
  rm -rf "$WORK"
}
trap cleanup EXIT

LOG="$WORK/proof.log"
if ! cargo run --quiet --offline --bin gba-native -- aot proof --out "$WORK/aot-proof" >"$LOG" 2>&1; then
  cat "$LOG" >&2
  fail "native AOT proof failed"
fi

grep -Fxq 'proof=ok' "$LOG" ||
  fail "native AOT proof did not report proof=ok"
grep -Fxq 'native_vs_reference=match' "$LOG" ||
  fail "native and reference states differ"
grep -Fxq 'fallbacks=0' "$LOG" ||
  fail "native proof used interpreter fallbacks; expected 0 for this fixture"

printf '1\n'
