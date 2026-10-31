#!/usr/bin/env bash
#
# Cargo linker shim for optional local cross-builds.
#
# Cargo can only point at an executable, not at `zig cc -target ...`, so this
# shim forwards the compiler/linker arguments to zig. It is only used by
# scripts/cross.sh and by explicit CARGO_TARGET_<TRIPLE>_LINKER settings; the
# default repository gates and CI native builds do not need it.
#
# Usage from Cargo: this script receives the normal linker arguments and reads
# ZIG_CROSS_TARGET (for example x86_64-linux-gnu). Direct invocation is not part
# of the public contract.

set -euo pipefail

if ! command -v zig >/dev/null 2>&1; then
  echo "zig-linker: zig is required for this cross target" >&2
  exit 1
fi
if [ -z "${ZIG_CROSS_TARGET:-}" ]; then
  echo "zig-linker: ZIG_CROSS_TARGET is not set" >&2
  exit 1
fi

args=()
shim_dir=""
case "$ZIG_CROSS_TARGET" in
  *windows*)
    # Zig 0.17 links its MinGW CRT implicitly and does not expose the historical
    # GCC archive names that rustc requests (`-lmsvcrt`, `-l:libpthread.a`, ...).
    # Provide empty archives with those names; the real symbols and import
    # libraries come from Zig's implicit CRT/import set. This keeps rustc's
    # explicit `-l` arguments satisfiable without a system mingw-w64 install.
    shim_dir="${TMPDIR:-/tmp}/gba-native-zig-shim-${ZIG_CROSS_TARGET//[^A-Za-z0-9_.-]/_}"
    mkdir -p "$shim_dir"
    for library in msvcrt pthread mingwex mingw32 gcc gcc_eh user32 kernel32 ntdll userenv ws2_32 dbghelp; do
      [ -f "$shim_dir/lib$library.a" ] || (cd "$shim_dir" && zig ar cr "lib$library.a")
    done
    ;;
esac
for argument in "$@"; do
  # rustc asks the platform linker not to add its default libraries. Zig's
  # MinGW runtime is compiled on demand and is only added with its defaults, so
  # keep them for the windows-gnu target. Rust still passes every required
  # library explicitly; this only supplies the search paths and CRT objects.
  case "$ZIG_CROSS_TARGET:$argument" in
    *windows*:-nodefaultlibs) continue ;;
    *windows*:-lgcc|-lgcc_eh|-l:libgcc.a|-l:libgcc_eh.a) continue ;;
    *windows*:-l:*) args+=("$argument"); continue ;;
    *windows*:-l*) args+=("-l:lib${argument#-l}.a"); continue ;;
  esac
  args+=("$argument")
done

if [ -n "$shim_dir" ]; then
  exec zig cc -target "$ZIG_CROSS_TARGET" -L"$shim_dir" "${args[@]}"
fi
exec zig cc -target "$ZIG_CROSS_TARGET" "${args[@]}"
