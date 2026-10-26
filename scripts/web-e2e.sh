#!/usr/bin/env bash
#
# Functional browser E2E for the built bundle. Starts the loopback static
# server, copies the functional spec/config next to the owned Playwright
# tooling, and runs a full Chromium build (not the headless shell) so WebGPU is
# exercised.
#
# Usage: ./scripts/web-e2e.sh [local|ci]
#   local  headless Chromium; WebGL2 may use the host GPU, WebGPU uses SwiftShader
#   ci     same entry point, headless; Linux selects SwiftShader's Vulkan path
#
# The screenshot capture set is a separate script and never runs here.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

MODE="${1:-local}"
case "$MODE" in
  local | ci) ;;
  *) fail "scripts/web-e2e.sh: mode must be local or ci (got: $MODE)" ;;
esac

DIST="$ROOT/output/web-dist"
NPM="$ROOT/target/web-tools/npm"
SERVER="$ROOT/target/web/debug/gba-native-web-serve"
NODE_BIN="$ROOT/target/tools/node-pinned/bin/node"
LINUX_DIAGNOSTICS_DIR=""
LINUX_DIAGNOSTICS_ENABLED=""

[ -f "$DIST/index.html" ] && [ -s "$DIST/app.css" ] ||
  fail "the bundle is missing; run ./scripts/web.sh --build-only first"
[ -x "$SERVER" ] || fail "the static server binary is missing; run ./scripts/web.sh --build-only first"
[ -x "$NODE_BIN" ] || fail "the pinned Node runtime is missing; run ./scripts/bootstrap-web-tools.sh"
[ -f "$NPM/node_modules/@playwright/test/cli.js" ] ||
  fail "Playwright tooling is missing; run ./scripts/bootstrap-web-tools.sh"

if [ "$MODE" = "ci" ] && [ "$(uname -s)" = Linux ] && [ "${CI:-}" = true ]; then
  mkdir -p "$ROOT/output"
  DIAGNOSTIC_STAMP=$(date -u '+%Y%m%dT%H%M%SZ')
  for suffix in $(seq 0 99); do
    candidate="$ROOT/output/web-linux-diag-$DIAGNOSTIC_STAMP-$$-$suffix"
    if mkdir "$candidate" 2>/dev/null; then
      LINUX_DIAGNOSTICS_DIR="$candidate"
      break
    fi
  done
  [ -n "$LINUX_DIAGNOSTICS_DIR" ] ||
    fail "could not allocate a fresh output/web-linux-diag-* directory"
  LINUX_DIAGNOSTICS_ENABLED=1
  info "Linux browser diagnostics will be saved under $LINUX_DIAGNOSTICS_DIR"
fi

PLAYWRIGHT_REVISION=$(cd "$NPM" && "$NODE_BIN" -e '
const fs = require("fs");
const browsers = JSON.parse(fs.readFileSync("node_modules/playwright-core/browsers.json", "utf8")).browsers;
console.log(browsers.find((entry) => entry.name === "chromium").revision);
')
CACHE_DIRS=("$HOME/Library/Caches/ms-playwright" "${XDG_CACHE_HOME:-$HOME/.cache}/ms-playwright")
FOUND_BROWSER=0
for cache in "${CACHE_DIRS[@]}"; do
  if [ -d "$cache/chromium-$PLAYWRIGHT_REVISION" ]; then FOUND_BROWSER=1; fi
done
[ "$FOUND_BROWSER" = "1" ] ||
  fail "Playwright Chromium revision $PLAYWRIGHT_REVISION is not installed; run: (cd target/web-tools/npm && PATH=\"$ROOT/target/tools/node-pinned/bin:\$PATH\" npx playwright install chromium) — this is a browser cache, not a global tool install"

mkdir -p "$ROOT/output"
TMP=$(mktemp -d "$ROOT/output/web-e2e.XXXXXX")
SERVER_PID=""
SERVER_LOG="$TMP/server.log"
if [ -n "$LINUX_DIAGNOSTICS_DIR" ]; then
  SERVER_LOG="$LINUX_DIAGNOSTICS_DIR/server.log"
fi
cleanup() {
  if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

info "starting loopback server for $DIST"
"$SERVER" --root "$DIST" --bind 127.0.0.1:0 --lifetime-secs 900 >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 100); do
  if grep -q '^listening on ' "$SERVER_LOG"; then break; fi
  kill -0 "$SERVER_PID" 2>/dev/null || fail "server exited early: $(cat "$SERVER_LOG")"
  sleep 0.1
done
URL=$(sed -n 's/^listening on //p' "$SERVER_LOG" | head -1)
[ -n "$URL" ] || fail "server did not report a URL: $(cat "$SERVER_LOG")"
info "test origin: $URL"

E2E_DIR="$NPM/e2e"
rm -rf "$E2E_DIR"
mkdir -p "$E2E_DIR"
cp apps/web/playwright.config.ts apps/web/playwright.screenshots.config.ts "$E2E_DIR"/
cp apps/web/tests/functional.spec.ts "$E2E_DIR"/

CHANNEL="${WEB_E2E_CHANNEL:-chromium}"
HEADLESS="${WEB_E2E_HEADLESS:-1}"
DEFAULT_LAUNCH_ARGS=$("$ROOT/scripts/web-gpu-launch-args.sh")
LAUNCH_ARGS="${WEB_E2E_LAUNCH_ARGS:-$DEFAULT_LAUNCH_ARGS}"
if [ "$MODE" = "ci" ]; then
  HEADLESS=1
  CHANNEL="${WEB_E2E_CHANNEL:-chromium}"
fi

info "functional E2E (mode=$MODE channel=$CHANNEL headless=$HEADLESS)"
info "launch args: $LAUNCH_ARGS"
(
  cd "$E2E_DIR"
  if [ -n "$LINUX_DIAGNOSTICS_DIR" ]; then
    WEB_APP_URL="$URL" \
    WEB_E2E_CHANNEL="$CHANNEL" \
    WEB_E2E_HEADLESS="$HEADLESS" \
    WEB_E2E_LAUNCH_ARGS="$LAUNCH_ARGS" \
    WEB_E2E_LINUX_DIAGNOSTICS="$LINUX_DIAGNOSTICS_ENABLED" \
    WEB_E2E_OUTPUT_DIR="$LINUX_DIAGNOSTICS_DIR/test-results" \
      "$NODE_BIN" "$NPM/node_modules/@playwright/test/cli.js" test -c playwright.config.ts \
      2>&1 | tee "$LINUX_DIAGNOSTICS_DIR/playwright.log"
  else
    WEB_APP_URL="$URL" \
    WEB_E2E_CHANNEL="$CHANNEL" \
    WEB_E2E_HEADLESS="$HEADLESS" \
    WEB_E2E_LAUNCH_ARGS="$LAUNCH_ARGS" \
    WEB_E2E_LINUX_DIAGNOSTICS="" \
    WEB_E2E_OUTPUT_DIR="" \
      "$NODE_BIN" "$NPM/node_modules/@playwright/test/cli.js" test -c playwright.config.ts
  fi
)

info "functional E2E passed"
