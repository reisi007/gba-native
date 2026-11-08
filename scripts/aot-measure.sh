#!/usr/bin/env bash
# Reproducible, bounded host-observed AOT runtime measurement (synthetic only).
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"
cd "$ROOT"
require_cargo
command -v uname >/dev/null 2>&1 || fail "uname is required"
command -v awk >/dev/null 2>&1 || fail "awk is required"
command -v sort >/dev/null 2>&1 || fail "sort is required"

RUNS=${AOT_MEASURE_RUNS:-7}
[[ "$RUNS" =~ ^[0-9]{1,2}$ ]] || fail "AOT_MEASURE_RUNS must be an integer from 3 through 25 (default 7)"
RUNS=$((10#$RUNS))
(( RUNS >= 3 && RUNS <= 25 )) || fail "AOT_MEASURE_RUNS must be an integer from 3 through 25 (default 7)"
BUDGET=2000000
if command -v sha256sum >/dev/null 2>&1; then
  sha256_file() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  sha256_file() { shasum -a 256 "$1" | awk '{print $1}'; }
else
  fail "no SHA-256 tool available (sha256sum or shasum)"
fi
host_triple() {
  rustc -vV | sed -n 's/^host: //p'
}
field() {
  local key=$1 file=$2 value
  value=$(awk -F= -v k="$key" '$1==k { sub(/^[^=]*=/, ""); print; n++ } END { if (n != 1) exit 1 }' "$file") ||
    fail "missing or duplicate $key field in $file"
  [[ -n "$value" ]] || fail "empty $key field in $file"
  printf '%s\n' "$value"
}

mkdir -p "$ROOT/output"
WORK=$(mktemp -d "$ROOT/output/aot-measure.XXXXXX") || fail "cannot create owned output workspace"
info "owned measurement workspace: $WORK"
trap 'rm -rf -- "$WORK"' EXIT
TARGET=$(host_triple)
[[ -n "$TARGET" ]] || fail "rustc did not report a host triple"
OS=$(uname -s); ARCH=$(uname -m)
info "generating repository-owned input-tester fixture"
cargo run --quiet --offline --bin gba-native -- fixture input-tester --out "$WORK/input-tester.gba" >"$WORK/fixture.log" ||
  { cat "$WORK/fixture.log" >&2; fail "synthetic fixture generation failed"; }
ROM="$WORK/input-tester.gba"
info "building native release harness offline for $TARGET (budget $BUDGET)"
cargo run --quiet --offline --bin gba-native -- aot build "$ROM" --target "$TARGET" --budget "$BUDGET" --out "$WORK/native" >"$WORK/build.log" ||
  { cat "$WORK/build.log" >&2; fail "AOT build failed"; }
BUILT=$(field status "$WORK/build.log"); [[ "$BUILT" == built ]] || fail "unexpected build status: $BUILT"
GEN=$(field generated_dir "$WORK/build.log")
EXE=$(field executable "$WORK/build.log")
[[ -f "$GEN/generated.rs" ]] || fail "generated Rust source missing: $GEN/generated.rs"
[[ -x "$EXE" ]] || fail "native executable missing or not executable: $EXE"
[[ -f "$GEN/artifact.meta" && -f "$GEN/build.meta" ]] || fail "AOT metadata files missing below $GEN"
EXE_SHA=$(field executable_sha256 "$GEN/artifact.meta")
SRC_SHA=$(field source_sha256 "$GEN/artifact.meta")
[[ "$(sha256_file "$EXE")" == "$EXE_SHA" ]] || fail "executable SHA-256 disagrees with artifact.meta"
[[ "$(sha256_file "$GEN/generated.rs")" == "$SRC_SHA" ]] || fail "source SHA-256 disagrees with artifact.meta"

printf 'measurement=host-observed synthetic bounded run\nos=%s\narchitecture=%s\nhost_triple=%s\ncargo_version=%s\nrustc_version=%s\nprofile=release\noffline=true\nrequested_budget=%s\n' \
  "$OS" "$ARCH" "$TARGET" "$(cargo --version)" "$(rustc --version)" "$BUDGET"
while IFS= read -r line; do printf 'artifact_%s\n' "$line"; done <"$GEN/artifact.meta"
while IFS= read -r line; do printf 'build_%s\n' "$line"; done <"$GEN/build.meta"
printf 'generated_source_bytes=%s\nexecutable_bytes=%s\n' "$(wc -c <"$GEN/generated.rs" | tr -d ' ')" "$(wc -c <"$EXE" | tr -d ' ')"

printf 'sample perf_elapsed_millis budget_used native_instructions fallbacks cpu_sha256 bus_sha256 vram_sha256 image_sha256\n'
for ((i=1; i<=RUNS; i++)); do
  REPORT="$WORK/run-$i.txt"
  "$EXE" --rom "$ROM" >"$REPORT" || { cat "$REPORT" >&2; fail "runtime sample $i failed"; }
  B=$(field budget_used "$REPORT"); N=$(field native_instructions "$REPORT"); F=$(field fallbacks "$REPORT")
  [[ "$B" == "$BUDGET" ]] || fail "sample $i exhausted budget $B, expected $BUDGET"
  [[ "$(field budget_remaining "$REPORT")" == 0 ]] || fail "sample $i did not report exact budget exhaustion"
  P=$(field perf_elapsed_millis "$REPORT")
  CPU=$(field cpu_sha256 "$REPORT"); BUS=$(field bus_sha256 "$REPORT")
  VRAM=$(field vram_sha256 "$REPORT"); IMAGE=$(field image_sha256 "$REPORT")
  row="$B $N $F $CPU $BUS $VRAM $IMAGE"
  if (( i == 1 )); then BASE="$row"; else [[ "$row" == "$BASE" ]] || fail "deterministic counters/state differ in sample $i"; fi
  printf '%s %s %s %s %s %s %s %s %s\n' "$i" "$P" "$B" "$N" "$F" "$CPU" "$BUS" "$VRAM" "$IMAGE"
  printf '%s\n' "$P" >>"$WORK/times"
done
sort -n "$WORK/times" >"$WORK/sorted-times"
MIN=$(head -n1 "$WORK/sorted-times"); MAX=$(tail -n1 "$WORK/sorted-times")
MEDIAN=$(awk -v n="$RUNS" 'NR==int((n+1)/2) { lower=$1 } NR==int(n/2)+1 { upper=$1 } END { if (n%2) print lower; else printf "%.3f", (lower+upper)/2 }' "$WORK/sorted-times")
printf 'summary_runs=%s\nsummary_min_millis=%s\nsummary_median_millis=%s\nsummary_max_millis=%s\n' "$RUNS" "$MIN" "$MEDIAN" "$MAX"
info "measurements complete; the unique owned workspace will now be removed"
