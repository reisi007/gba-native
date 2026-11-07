#!/usr/bin/env bash
# Exercise Rune's browser UI and capture desktop/mobile states for review.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
NODE="$ROOT/target/tools/node-pinned/bin/node"
NPM="$ROOT/target/web-tools/npm"
PLAYWRIGHT="$NPM/node_modules/@playwright/test/cli.js"
mkdir -p "$ROOT/output" "$ROOT/target"
TMP=$(mktemp -d "$ROOT/target/rune-web-review-run-XXXXXXXX")
OUT=$(mktemp -d "$ROOT/output/rune-web-review-$(date +%Y%m%d-%H%M%S)-XXXXXX")
SERVER_PID=""

cleanup() {
  if [ -n "$SERVER_PID" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

[ -x "$NODE" ] && [ -f "$PLAYWRIGHT" ] || {
  printf 'Pinned Node/Playwright tools are missing; run scripts/bootstrap-web-tools.sh.\n' >&2
  exit 1
}

if ! "$ROOT/scripts/rune-web.sh" >"$TMP/build.log" 2>&1; then
  cat "$TMP/build.log" >&2
  exit 1
fi
DIST=$(sed -n 's/^Rune browser bundle: //p' "$TMP/build.log" | tail -n 1)
[ -d "$DIST" ] || {
  cat "$TMP/build.log" >&2
  printf 'Rune browser bundle directory was not reported.\n' >&2
  exit 1
}

"$NODE" "$ROOT/apps/rune/tests/serve-static.mjs" "$DIST" >"$TMP/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 100); do
  grep -q '^listening on ' "$TMP/server.log" && break
  kill -0 "$SERVER_PID" 2>/dev/null || {
    cat "$TMP/server.log" >&2
    exit 1
  }
  sleep 0.1
done
URL=$(sed -n 's/^listening on //p' "$TMP/server.log" | head -n 1)
[ -n "$URL" ] || {
  cat "$TMP/server.log" >&2
  printf 'Rune static server did not report its URL.\n' >&2
  exit 1
}

WORK="$NPM/e2e-rune-screenshots"
mkdir -p "$WORK/screenshots"
cp "$ROOT/apps/rune/playwright.screenshots.config.ts" "$WORK/"
cp "$ROOT/apps/rune/tests/screenshots/"* "$WORK/screenshots/"
LAUNCH_ARGS=$("$ROOT/scripts/web-gpu-launch-args.sh")
PLAYWRIGHT_ARGS=(test -c playwright.screenshots.config.ts)
if [ -n "${RUNE_SCREENSHOT_GREP:-}" ]; then
  PLAYWRIGHT_ARGS+=(--grep "$RUNE_SCREENSHOT_GREP")
fi
if [ -n "${RUNE_SCREENSHOT_PROJECT:-}" ]; then
  PLAYWRIGHT_ARGS+=(--project "$RUNE_SCREENSHOT_PROJECT")
fi
(
  cd "$WORK"
  RUNE_APP_URL="$URL" \
  RUNE_REVIEW_OUT="$OUT" \
  RUNE_E2E_CHANNEL="${RUNE_E2E_CHANNEL:-chromium}" \
  RUNE_E2E_HEADLESS="${RUNE_E2E_HEADLESS:-1}" \
  RUNE_E2E_LAUNCH_ARGS="${RUNE_E2E_LAUNCH_ARGS:-$LAUNCH_ARGS}" \
    "$NODE" "$PLAYWRIGHT" "${PLAYWRIGHT_ARGS[@]}"
)

find "$OUT" -type f \( -name '*.png' -o -name '*.json' \) | sort >"$OUT/inventory.txt"
COUNT=$(grep -c '\.png$' "$OUT/inventory.txt" || true)
printf 'Rune UI screenshots: %s PNG(s)\nOutput: %s\n' "$COUNT" "$OUT"
