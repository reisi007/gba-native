#!/usr/bin/env bash
#
# Browser foundation gate for apps/web (synthetic demo only).
#
# It is deliberately separate from scripts/short.sh and scripts/full.sh: the
# default std-only workspace never resolves Winit/Pixels/WGPU or any browser
# tooling. Ratchet is not required here.
#
#   1. hard limit: every maintained Rust file has at most 500 physical lines
#   2. core WASM compile check (std-only core, root lock still three packages)
#   3. host unit tests + Rust formatting + Clippy, both host and wasm32
#   4. standalone apps/web WASM release build, wasm-bindgen 0.2.129, local
#      Tailwind/DaisyUI CSS and a self-contained static bundle
#   5. dependency-tree contract: direct core only, no AOT/root facade
#   6. functional browser E2E (separate from the screenshot capture set)
#
# Every Cargo invocation uses the repository-ignored target/web directory.
# Tool setup is pinned and repository-owned: scripts/bootstrap-web-tools.sh.
#
# Usage: ./scripts/web.sh [--build-only] [--ci]
#   --build-only  skip the browser E2E run (partial gate, not CI evidence)
#   --ci          headless Chromium with SwiftShader WebGPU/WebGL2 flags; Linux
#                 CI E2E (mode ci, CI=true) re-executes itself under xvfb-run
#                 when DISPLAY is unset (candidate configuration under test
#                 for #24); screenshot capture and --build-only are unaffected

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"

BUILD_ONLY=0
MODE="local"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --build-only) BUILD_ONLY=1 ;;
    --ci) MODE="ci" ;;
    *) fail "scripts/web.sh: unknown argument $1 (expected --build-only or --ci)" ;;
  esac
  shift
done

require_cargo
export CARGO_TARGET_DIR="$ROOT/target/web"
DIST="$ROOT/output/web-dist"
TOOLS="$ROOT/target/tools"
NPM="$ROOT/target/web-tools/npm"
NODE_BIN="$TOOLS/node-pinned/bin/node"

command -v rustup >/dev/null 2>&1 || fail "rustup is required to verify the wasm32 target"
rustup target list --installed | grep -qx wasm32-unknown-unknown ||
  fail "wasm32-unknown-unknown is not installed; run: rustup target add wasm32-unknown-unknown"

info "browser tool bootstrap (pinned, repository-owned target/ caches)"
"$ROOT/scripts/bootstrap-web-tools.sh" --self-test
"$ROOT/scripts/bootstrap-web-tools.sh"

info "hard limit: 500 physical lines per maintained Rust file (including apps/web)"
cargo run --quiet --offline --manifest-path Cargo.toml --bin quality-counter -- \
  oversized --root . --max 500 --format items --hard

info "root workspace lock still contains exactly the three internal packages"
LOCK_PACKAGES=$(grep -c '^\[\[package\]\]$' Cargo.lock || true)
[ "$LOCK_PACKAGES" = "3" ] ||
  fail "root Cargo.lock has $LOCK_PACKAGES packages; expected 3 (gba-native, core, aot)"
for name in gba-native gba-native-core gba-native-aot; do
  grep -q "^name = \"$name\"$" Cargo.lock || fail "root Cargo.lock is missing $name"
done

info "core std-only WASM compile check (offline, locked, no host-fs browser path)"
cargo check --offline --locked -p gba-native-core --target wasm32-unknown-unknown

info "apps/web fmt --check"
cargo fmt --manifest-path apps/web/Cargo.toml --check

info "apps/web host unit tests"
cargo test --locked --manifest-path apps/web/Cargo.toml

info "apps/web browser lifecycle unit tests"
"$NODE_BIN" --test apps/web/tests/js/lifecycle.test.mjs apps/web/tests/js/input.test.mjs apps/web/tests/js/saves.test.mjs

info "apps/web Clippy (host all-targets; wasm32 library)"
cargo clippy --locked --manifest-path apps/web/Cargo.toml --all-targets -- -D warnings
cargo clippy --locked --target wasm32-unknown-unknown --manifest-path apps/web/Cargo.toml \
  --lib -- -D warnings

info "apps/web WASM release build"
cargo build --locked --release --target wasm32-unknown-unknown \
  --manifest-path apps/web/Cargo.toml --lib

info "host build of the loopback static server"
cargo build --locked --manifest-path apps/web/Cargo.toml --bin gba-native-web-serve

info "dependency tree: direct core only; no AOT crate or root facade"
TREE=$(cargo tree --locked --offline --target wasm32-unknown-unknown \
  --manifest-path apps/web/Cargo.toml)
printf '%s\n' "$TREE" | grep -q 'gba-native-core v' ||
  fail "apps/web is missing the direct gba-native-core runtime"
if printf '%s\n' "$TREE" | grep -Eq '(^|[[:space:]])gba-native-aot v'; then
  fail "apps/web must not depend on host-side gba-native-aot tooling"
fi
if printf '%s\n' "$TREE" | grep -Eq '(^|[[:space:]])gba-native v'; then
  fail "apps/web must not depend on the root gba-native compatibility facade"
fi
printf '%s\n' "$TREE" | grep -q 'pixels v0.17.2' || fail "pixels is not pinned to 0.17.2"
printf '%s\n' "$TREE" | grep -q 'winit v0.30.13' || fail "winit is not pinned to 0.30.13"
printf '%s\n' "$TREE" | grep -q 'wgpu v29.0.4' || fail "wgpu is not pinned to 29.0.4"

info "static bundle in output/web-dist (owned marker)"
if [ -e "$DIST" ] && [ ! -f "$DIST/.gba-web-dist" ]; then
  fail "$DIST exists but was not created by this script; refusing to overwrite it"
fi
rm -rf "$DIST"
mkdir -p "$DIST/pkg"
: >"$DIST/.gba-web-dist"
cp apps/web/www/index.html apps/web/www/app.js apps/web/www/lifecycle.mjs apps/web/www/input.mjs apps/web/www/saves.mjs "$DIST"/

info "wasm-bindgen $("$TOOLS/bin/wasm-bindgen" --version | awk '{print $2}') web bindings"
"$TOOLS/bin/wasm-bindgen" --target web --no-typescript --out-dir "$DIST/pkg" \
  --out-name gba_native_web \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/gba_native_web.wasm"

info "locally built DaisyUI 5 + Tailwind 4 CSS"
# Tailwind resolves `@import "tailwindcss"` relative to the input CSS file, so
# the input is copied next to the owned node_modules. The committed sources stay
# in apps/web/www; the copy lives only under the ignored target/ tree.
BUILD_WWW="$NPM/www"
rm -rf "$BUILD_WWW"
mkdir -p "$BUILD_WWW"
cp apps/web/www/input.css apps/web/www/index.html apps/web/www/app.js apps/web/www/lifecycle.mjs apps/web/www/input.mjs "$BUILD_WWW"/
"$NODE_BIN" "$NPM/node_modules/@tailwindcss/cli/dist/index.mjs" -i "$BUILD_WWW/input.css" \
  -o "$DIST/app.css" --minify

info "bundle contracts: expected WASM exports, no remote runtime assets"
grep -q 'export function set_status_sink' "$DIST/pkg/gba_native_web.js" ||
  fail "wasm-bindgen output is missing the set_status_sink export"
grep -q 'export function start' "$DIST/pkg/gba_native_web.js" ||
  fail "wasm-bindgen output is missing the start export"
[ -s "$DIST/pkg/gba_native_web_bg.wasm" ] || fail "WASM binary is missing"
if grep -a -q 'wasi_snapshot_preview1' "$DIST/pkg/gba_native_web_bg.wasm"; then
  fail "the browser WASM imports WASI; the browser path must not need a host filesystem"
fi
[ -s "$DIST/app.css" ] || fail "built CSS is missing"
if grep -RInE \
  '(src|href)="https?://|url\(["'"'"']?https?://|@import[^;]*https?://|fetch\(["'"'"']https?://|import\(["'"'"']https?://' \
  "$DIST/index.html" "$DIST/app.js" "$DIST/app.css" "$DIST/pkg/gba_native_web.js" \
  >/dev/null 2>&1; then
  fail "bundle references a remote resource; no CDN/runtime third-party request is allowed"
fi
[ -f apps/web/node_modules ] && fail "apps/web/node_modules must not exist"

info "bundle size"
du -sh "$DIST"

if [ "$BUILD_ONLY" = "1" ]; then
  info "build-only gate passed (browser E2E not run; not full CI evidence)"
  exit 0
fi

info "functional browser E2E ($MODE)"
"$ROOT/scripts/web-e2e.sh" "$MODE"

info "browser gate passed (build, host tests, WASM bundle, functional E2E)"
info "screenshot capture is separate: ./scripts/web-screenshots.sh"
