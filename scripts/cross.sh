#!/usr/bin/env bash
#
# Optional local cross-build matrix for the synthetic input-tester fixture.
#
# Builds the same ROM-bound native artifact for every supported target and
# verifies the produced file type. Host-executable Apple artifacts are also run
# with the committed input script and checked against the expected frame. Linux
# and Windows artifacts are build-only on a macOS host; they are not executed
# here and no compatibility claim is made.
#
# Prerequisites are explicit, never silently skipped:
#   - Linux: zig (`brew install zig` or a distribution package)
#   - Windows: x86_64-w64-mingw32-gcc (`brew install mingw-w64`)
#   - wasm32: scripts/bootstrap-web-tools.sh and a cached wasm-bindgen crate
#
# This script is not part of scripts/short.sh, scripts/full.sh or CI. CI covers
# the native Linux path in the input-tester job.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo

ROM="tests/roms/synthetic/input-tester.gba"
SCRIPT="tests/roms/synthetic/input-tester.input"
EXPECTED="tests/roms/synthetic/input-tester.expected.ppm"
EXPECTED_IMAGE="5cbbd998a2bc5a026ebac07d91a0f058b769cb05dd14c09e30d52888ed80db8b"
BUDGET=2000000

mkdir -p "$ROOT/output"
WORK=$(mktemp -d "$ROOT/output/cross.XXXXXX")
info "cross workspace: $WORK"

TARGETS=(
  "aarch64-apple-darwin"
  "x86_64-apple-darwin"
  "x86_64-unknown-linux-gnu"
  "x86_64-pc-windows-gnu"
  "wasm32-unknown-unknown"
)

build_target() {
  target=$1
  out="$WORK/$target"
  env_prefix=()
  case "$target" in
    x86_64-unknown-linux-gnu)
      if [ "$(uname -s)" = Linux ]; then
        :
      elif command -v zig >/dev/null 2>&1; then
        env_prefix=(env "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=$ROOT/scripts/zig-linker.sh" "ZIG_CROSS_TARGET=x86_64-linux-gnu")
      else
        fail "zig is required to link the x86_64 Linux target on this host"
      fi
      ;;
    x86_64-pc-windows-gnu)
      mingw=$(command -v x86_64-w64-mingw32-gcc || true)
      [ -n "$mingw" ] ||
        fail "x86_64-w64-mingw32-gcc is required for the Windows GNU target (brew install mingw-w64)"
      env_prefix=(env "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=$mingw")
      ;;
    wasm32-unknown-unknown)
      [ -x "$ROOT/target/tools/bin/wasm-bindgen" ] ||
        fail "target-local wasm-bindgen is missing; run scripts/bootstrap-web-tools.sh"
      ;;
  esac

  info "building $target"
  ${env_prefix[@]+"${env_prefix[@]}"} cargo run --quiet --offline --bin gba-native -- aot build "$ROM" \
    --target "$target" --budget "$BUDGET" --out "$out" \
    >"$WORK/$target-build.txt" 2>&1 ||
    fail "cross build for $target failed; see $WORK/$target-build.txt"

  case "$target" in
    wasm32-unknown-unknown)
      wasm="$out/wasm/pkg/gba_native_wasm_module_bg.wasm"
      [ -s "$wasm" ] || fail "WASM build produced no module at $wasm"
      if grep -a -q 'GBA-NATIVE' "$wasm"; then
        fail "WASM module contains a ROM marker"
      fi
      printf 'target=%s kind=wasm32 artifact=%s bytes=%s\n' "$target" "$wasm" "$(wc -c <"$wasm")"
      ;;
    *)
      suffix=""
      case "$target" in *windows*) suffix=".exe" ;; esac
      exe="$out/native/target/$target/release/gba-native-aot-harness$suffix"
      [ -x "$exe" ] || fail "expected executable is missing: $exe"
      file "$exe"
      printf 'target=%s artifact=%s bytes=%s\n' "$target" "$exe" "$(wc -c <"$exe")"
      if [ "$target" = "aarch64-apple-darwin" ] && [ "$(uname -m)" = arm64 ]; then
        "$exe" --rom "$ROM" --input "$SCRIPT" \
          --snapshot "$WORK/$target.ppm" --audio "$WORK/$target.wav" --volume 80 \
          >"$WORK/$target-report.txt"
        grep -qx "image_sha256=$EXPECTED_IMAGE" "$WORK/$target-report.txt" ||
          fail "$target produced an unexpected input-tester frame"
        grep -qx "audio_audible=true" "$WORK/$target-report.txt" ||
          fail "$target produced silent guest audio"
        cmp -s "$WORK/$target.ppm" "$EXPECTED" ||
          fail "$target frame differs from the committed expected frame"
        printf 'target=%s run=ok image_sha256=%s audio=audible\n' "$target" "$EXPECTED_IMAGE"
      fi
      ;;
  esac
}

for target in "${TARGETS[@]}"; do
  build_target "$target"
done

info "cross-build matrix passed"
printf 'workspace=%s\n' "$WORK"
