#!/usr/bin/env bash
#
# UI-review screenshot capture for the browser foundation.
#
# Capture only: this is NOT a functional gate and never runs in CI. It builds
# the current bundle, serves it on loopback, writes PNGs/JSON into a uniquely
# named ignored output/web-review-* directory and leaves that directory in place
# for the reviewer. The functional E2E evidence is scripts/web-e2e.sh.
#
# Playwright browsers are the pinned cache from scripts/web-e2e.sh; missing
# browsers fail with the install command instead of a silent skip.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

require_cargo
cd "$ROOT"
info "build current browser bundle (build-only gate)"
"$ROOT/scripts/web.sh" --build-only

DIST="$ROOT/output/web-dist"
NPM="$ROOT/target/web-tools/npm"
SERVER="$ROOT/target/web/debug/gba-native-web-serve"
NODE_BIN="$ROOT/target/tools/node-pinned/bin/node"
mkdir -p "$ROOT/output"
OUT=$(mktemp -d "$ROOT/output/web-review-$(date +%Y%m%d-%H%M%S)-XXXXXX")

[ -x "$SERVER" ] || fail "the static server binary is missing; rerun scripts/web.sh --build-only"
[ -x "$NODE_BIN" ] || fail "the pinned Node runtime is missing; run ./scripts/bootstrap-web-tools.sh"
TMP=$(mktemp -d "$ROOT/output/web-review-server.XXXXXX")
SERVER_PID=""
cleanup() {
  if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

info "starting loopback server"
"$SERVER" --root "$DIST" --bind 127.0.0.1:0 --lifetime-secs 1800 >"$TMP/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 100); do
  grep -q '^listening on ' "$TMP/server.log" && break
  kill -0 "$SERVER_PID" 2>/dev/null || fail "server exited early: $(cat "$TMP/server.log")"
  sleep 0.1
done
URL=$(sed -n 's/^listening on //p' "$TMP/server.log" | head -1)
[ -n "$URL" ] || fail "server did not report a URL: $(cat "$TMP/server.log")"
info "test origin: $URL"

E2E_DIR="$NPM/e2e-screenshots"
rm -rf "$E2E_DIR"
mkdir -p "$E2E_DIR/screenshots"
cp apps/web/playwright.screenshots.config.ts "$E2E_DIR"/
cp apps/web/tests/screenshots/ui-review.config.ts apps/web/tests/screenshots/ui-screenshots.spec.ts \
  "$E2E_DIR/screenshots"/

CHANNEL="${WEB_E2E_CHANNEL:-chromium}"
HEADLESS="${WEB_E2E_HEADLESS:-1}"
LAUNCH_ARGS="${WEB_E2E_LAUNCH_ARGS:---enable-unsafe-webgpu --use-webgpu-adapter=swiftshader}"

info "capturing PNGs/JSON into $OUT"
(
  cd "$E2E_DIR"
  WEB_APP_URL="$URL" \
  WEB_REVIEW_OUT="$OUT" \
  WEB_E2E_CHANNEL="$CHANNEL" \
  WEB_E2E_HEADLESS="$HEADLESS" \
  WEB_E2E_LAUNCH_ARGS="$LAUNCH_ARGS" \
    "$NODE_BIN" "$NPM/node_modules/@playwright/test/cli.js" test -c playwright.screenshots.config.ts
)

find "$OUT" -type f \( -name '*.png' -o -name '*.json' \) | sort >"$OUT/inventory.txt"
COUNT=$(grep -c '\.png$' "$OUT/inventory.txt" || true)
info "capture set complete: $COUNT PNG(s)"
info "inventory: $OUT/inventory.txt"
info "captures: $OUT"
