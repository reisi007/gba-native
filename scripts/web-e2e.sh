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
#   WEB_LINUX_AB_DIAGNOSTICS=1 opts Linux CI into the matched Vulkan/OpenGL run
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

LINUX_AB_DIAGNOSTICS=0
case "${WEB_LINUX_AB_DIAGNOSTICS:-}" in
  "" | 0) ;;
  1)
    [ "$MODE" = ci ] && [ "$(uname -s)" = Linux ] && [ "${CI:-}" = true ] ||
      fail "WEB_LINUX_AB_DIAGNOSTICS=1 is supported only for Linux CI runs of scripts/web-e2e.sh ci"
    LINUX_AB_DIAGNOSTICS=1
    ;;
  *) fail "WEB_LINUX_AB_DIAGNOSTICS must be unset, 0, or 1" ;;
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
  DIAGNOSTIC_PREFIX="web-linux-proc"
  if [ "$LINUX_AB_DIAGNOSTICS" = 1 ]; then
    DIAGNOSTIC_PREFIX="web-linux-ab"
  fi
  for suffix in $(seq 0 99); do
    candidate="$ROOT/output/$DIAGNOSTIC_PREFIX-$DIAGNOSTIC_STAMP-$$-$suffix"
    if mkdir "$candidate" 2>/dev/null; then
      LINUX_DIAGNOSTICS_DIR="$candidate"
      break
    fi
  done
  [ -n "$LINUX_DIAGNOSTICS_DIR" ] ||
    fail "could not allocate a fresh output/web-linux-proc-* directory"
  LINUX_DIAGNOSTICS_ENABLED=1
  info "Linux browser/process diagnostics will be saved under $LINUX_DIAGNOSTICS_DIR"
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
if [ -n "$LINUX_DIAGNOSTICS_DIR" ]; then
  # Add process logging only for Linux CI diagnostics; preserve the selected
  # preset or user override and every existing browser launch flag.
  LAUNCH_ARGS="$LAUNCH_ARGS --enable-logging=stderr --log-level=0"
fi

if [ "$LINUX_AB_DIAGNOSTICS" = 1 ]; then
  read -r -a A_LAUNCH_ARGS <<<"$LAUNCH_ARGS"
  ANGLE_VULKAN_COUNT=0
  for arg in "${A_LAUNCH_ARGS[@]}"; do
    if [ "$arg" = "--use-angle=vulkan" ]; then
      ANGLE_VULKAN_COUNT=$((ANGLE_VULKAN_COUNT + 1))
    fi
  done
  [ "$ANGLE_VULKAN_COUNT" -eq 1 ] ||
    fail "Linux A/B diagnostics require exactly one --use-angle=vulkan token (found $ANGLE_VULKAN_COUNT); refusing to form variant B"

  B_LAUNCH_ARGS=()
  for arg in "${A_LAUNCH_ARGS[@]}"; do
    if [ "$arg" = "--use-angle=vulkan" ]; then
      B_LAUNCH_ARGS+=(--use-angle=opengl)
    else
      B_LAUNCH_ARGS+=("$arg")
    fi
  done
  B_LAUNCH_ARGS_TEXT="${B_LAUNCH_ARGS[*]}"

  A_STATUS=""
  B_STATUS=""
  for variant in A B; do
    if [ "$variant" = A ]; then
      variant_dir="$LINUX_DIAGNOSTICS_DIR/variant-a"
      variant_launch_args="$LAUNCH_ARGS"
    else
      variant_dir="$LINUX_DIAGNOSTICS_DIR/variant-b"
      variant_launch_args="$B_LAUNCH_ARGS_TEXT"
    fi
    mkdir -p "$variant_dir/test-results"
    printf '%s\n' "$variant_launch_args" >"$variant_dir/launch-args.txt"
    info "functional E2E variant $variant (mode=$MODE channel=$CHANNEL headless=$HEADLESS)"
    info "variant $variant launch args: $variant_launch_args"
    if (
      cd "$E2E_DIR"
      DEBUG=pw:browser \
      WEB_APP_URL="$URL" \
      WEB_E2E_CHANNEL="$CHANNEL" \
      WEB_E2E_HEADLESS="$HEADLESS" \
      WEB_E2E_LAUNCH_ARGS="$variant_launch_args" \
      WEB_E2E_LINUX_DIAGNOSTICS="$LINUX_DIAGNOSTICS_ENABLED" \
      WEB_E2E_OUTPUT_DIR="$variant_dir/test-results" \
        "$NODE_BIN" "$NPM/node_modules/@playwright/test/cli.js" test -c playwright.config.ts \
        2>&1 | tee "$variant_dir/playwright.log"
    ); then
      variant_status=0
    else
      variant_status=$?
    fi
    printf '%s\n' "$variant_status" >"$variant_dir/exit-status.txt"
    if [ "$variant" = A ]; then
      A_STATUS="$variant_status"
    else
      B_STATUS="$variant_status"
    fi
  done

  printf 'A=%s\nB=%s\n' "$A_STATUS" "$B_STATUS" >"$LINUX_DIAGNOSTICS_DIR/summary.txt"
  info "Linux A/B E2E statuses: A=$A_STATUS B=$B_STATUS; logs and artifacts: $LINUX_DIAGNOSTICS_DIR"
  if [ "$A_STATUS" -ne 0 ] || [ "$B_STATUS" -ne 0 ]; then
    fail "Linux A/B browser E2E failed (A=$A_STATUS, B=$B_STATUS); see $LINUX_DIAGNOSTICS_DIR/summary.txt"
  fi
else
  info "functional E2E (mode=$MODE channel=$CHANNEL headless=$HEADLESS)"
  info "launch args: $LAUNCH_ARGS"
  (
    cd "$E2E_DIR"
    if [ -n "$LINUX_DIAGNOSTICS_DIR" ]; then
      DEBUG=pw:browser \
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
fi

info "functional E2E passed"
