#!/usr/bin/env bash
#
# Functional browser E2E for the built bundle. Starts the loopback static
# server, copies the functional spec/config next to the owned Playwright
# tooling, and runs a full Chromium build (not the headless shell) so WebGPU is
# exercised.
#
# Usage: ./scripts/web-e2e.sh [local|ci]
#   local  headless Chromium; WebGL2 may use the host GPU, WebGPU uses SwiftShader
#   ci     same entry point, headless; Linux selects ANGLE's SwiftShader backend
#   WEB_LINUX_AB_DIAGNOSTICS=1 opts Linux CI into isolated WebGPU smoke probes
#     plus a matched A/B full-suite comparison, a non-gating one-flag omission
#     matrix (real app vs standalone canvas) and one additive
#     feature-combination candidate (not an omission)
#
# Candidate configuration under test (issue #24): Linux CI functional E2E runs
# without DISPLAY are re-executed under the runner's xvfb-run wrapper. Recorded
# runs show SharedImageBackingFactory failures and post-init device loss on the
# WebGPU canvas path but do not establish that a display or these launch
# switches fix them. The wrapper is limited to `ci` mode with CI=true on Linux;
# local Linux runs keep their previous behaviour. The gate stays fail-closed.
#
# The screenshot capture set is a separate script and never runs here.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"

MODE="${1:-local}"
case "$MODE" in
  local | ci) ;;
  *) fail "scripts/web-e2e.sh: mode must be local or ci (got: $MODE)" ;;
esac

if [ "$MODE" = ci ] && [ "$(uname -s)" = Linux ] && [ "${CI:-}" = true ] &&
  [ -z "${DISPLAY:-}" ]; then
  # Candidate display provisioning under test, limited to the Linux CI E2E.
  # The wrapper needs the distribution xauth helper as well. A missing wrapper
  # fails the gate instead of skipping the browser checks.
  command -v xvfb-run >/dev/null 2>&1 ||
    fail "Linux CI without DISPLAY requires xvfb-run (distribution xvfb and xauth packages) for the candidate display configuration; provide DISPLAY or install it. This check is never skipped"
  info "Linux CI without DISPLAY: re-executing the browser E2E under xvfb-run (candidate configuration under test)"
  export WEB_E2E_XVFB=1
  exec xvfb-run -a -s "-screen 0 1280x1024x24" bash "$ROOT/scripts/web-e2e.sh" "$@"
fi

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
  ANGLE_TOKEN="--use-angle=swiftshader"
  ANGLE_CONTROL="--use-angle=vulkan"
  ANGLE_TOKEN_COUNT=0
  for arg in "${A_LAUNCH_ARGS[@]}"; do
    if [ "$arg" = "$ANGLE_TOKEN" ]; then
      ANGLE_TOKEN_COUNT=$((ANGLE_TOKEN_COUNT + 1))
    fi
  done
  MATRIX_INVALID_INPUT_STATUS=126
  record_invalid_angle_omission() {
    invalid_angle_reason="--use-angle=swiftshader appears $ANGLE_TOKEN_COUNT times in Variant A flags; exactly one token is required for this omission control"
    : >"$LINUX_DIAGNOSTICS_DIR/matrix.txt"
    printf 'matrix-preflight=invalid-input\nmatrix-non-gating=true\n' \
      >"$LINUX_DIAGNOSTICS_DIR/summary.txt"
    for invalid_mode in canvas app; do
      invalid_dir="$LINUX_DIAGNOSTICS_DIR/matrix-omit-use-angle-swiftshader-$invalid_mode"
      if ! mkdir "$invalid_dir"; then
        info "could not allocate fresh $invalid_dir; retaining the existing A/B validation failure"
        continue
      fi
      if [ "$invalid_mode" = app ]; then
        invalid_entry='app:/?autostart=1&backend=webgpu'
      else
        invalid_entry='standalone:/__linux_webgpu_smoke__'
      fi
      printf '%s\n' "$LAUNCH_ARGS" >"$invalid_dir/launch-args.txt"
      printf '%s\n' '--use-angle=swiftshader' >"$invalid_dir/omitted-flag.txt"
      printf '%s\n' "$invalid_angle_reason" >"$invalid_dir/invalid-input.txt"
      printf 'label=omit-use-angle-swiftshader\nmode=%s\nomitted_flag=--use-angle=swiftshader\nentry=%s\norigin=%s\nchannel=%s\nheadless=%s\nchromium_revision=%s\nnon_gating=true\n' \
        "$invalid_mode" "$invalid_entry" "$URL" "$CHANNEL" "$HEADLESS" \
        "$PLAYWRIGHT_REVISION" >"$invalid_dir/run-info.txt"
      printf '%s\n' "$MATRIX_INVALID_INPUT_STATUS" >"$invalid_dir/exit-status.txt"
      printf '{"label":"omit-use-angle-swiftshader","mode":"%s","entry":"%s","omittedFlag":"--use-angle=swiftshader","processExitStatus":%s,"resultWritten":false,"outcome":"invalid-input","nonGating":true}\n' \
        "$invalid_mode" "$invalid_entry" "$MATRIX_INVALID_INPUT_STATUS" \
        >"$invalid_dir/process-status.json"
      printf 'omit-use-angle-swiftshader/%s omitted=--use-angle=swiftshader exit=%s outcome=invalid-input nonGating=true\n' \
        "$invalid_mode" "$MATRIX_INVALID_INPUT_STATUS" >>"$LINUX_DIAGNOSTICS_DIR/matrix.txt"
      printf 'matrix-omit-use-angle-swiftshader-%s=%s\n' \
        "$invalid_mode" "$MATRIX_INVALID_INPUT_STATUS" >>"$LINUX_DIAGNOSTICS_DIR/summary.txt"
    done
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
      {
        printf '\n### Non-gating GPU flag matrix preflight\n\n'
        printf 'The `omit-use-angle-swiftshader` control is recorded as `invalid-input` for both entries because Variant A contains the flag %s times. The existing A/B validation still fails closed before launching probes.\n' \
          "$ANGLE_TOKEN_COUNT"
      } >>"$GITHUB_STEP_SUMMARY"
    fi
  }
  if [ "$ANGLE_TOKEN_COUNT" -ne 1 ]; then
    record_invalid_angle_omission
    fail "Linux A/B diagnostics require exactly one --use-angle=swiftshader token (found $ANGLE_TOKEN_COUNT); refusing to form variant B"
  fi

  B_LAUNCH_ARGS=()
  for arg in "${A_LAUNCH_ARGS[@]}"; do
    if [ "$arg" = "$ANGLE_TOKEN" ]; then
      B_LAUNCH_ARGS+=("$ANGLE_CONTROL")
    else
      B_LAUNCH_ARGS+=("$arg")
    fi
  done
  B_LAUNCH_ARGS_TEXT="${B_LAUNCH_ARGS[*]}"

  run_linux_webgpu_smoke() {
    smoke_mode="$1"
    smoke_dir="$LINUX_DIAGNOSTICS_DIR/smoke-$smoke_mode"
    SMOKE_STATUS=125
    if ! mkdir "$smoke_dir"; then
      info "could not allocate fresh $smoke_dir; recording probe failure and continuing"
      return 0
    fi
    printf '%s\n' "$LAUNCH_ARGS" >"$smoke_dir/launch-args.txt"
    printf 'channel=%s\nheadless=%s\nchromium_revision=%s\norigin=%s\n' \
      "$CHANNEL" "$HEADLESS" "$PLAYWRIGHT_REVISION" "$URL" >"$smoke_dir/run-info.txt"
    info "standalone WebGPU smoke probe: $smoke_mode (current Linux variant A flags)"
    if (
      cd "$ROOT"
      DEBUG=pw:browser \
      WEBGPU_SMOKE_OUTPUT_DIR="$smoke_dir" \
      WEB_PLAYWRIGHT_CHROMIUM_REVISION="$PLAYWRIGHT_REVISION" \
      WEB_APP_URL="$URL" \
      WEB_E2E_CHANNEL="$CHANNEL" \
      WEB_E2E_LAUNCH_ARGS="$LAUNCH_ARGS" \
      PLAYWRIGHT_PACKAGE_PATH="$NPM/node_modules/playwright" \
        "$NODE_BIN" "$ROOT/scripts/web-linux-gpu-smoke.cjs" "$smoke_mode" \
        2>&1 | tee "$smoke_dir/playwright.log"
    ); then
      smoke_status=0
    else
      smoke_status=$?
    fi
    printf '%s\n' "$smoke_status" >"$smoke_dir/exit-status.txt"
    printf '{"mode":"%s","processExitStatus":%s,"resultWritten":%s}\n' \
      "$smoke_mode" "$smoke_status" "$([ -s "$smoke_dir/result.json" ] && printf true || printf false)" \
      >"$smoke_dir/process-status.json"
    SMOKE_STATUS="$smoke_status"
    return 0
  }

  run_linux_webgpu_smoke canvas
  SMOKE_CANVAS_STATUS="$SMOKE_STATUS"
  run_linux_webgpu_smoke offscreen
  SMOKE_OFFSCREEN_STATUS="$SMOKE_STATUS"

  A_STATUS=""
  B_STATUS=""
  # Variant A is the product preset; variant B is the matched one-token ANGLE
  # contrast. Both full suites gate the CI result exactly as before.
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

  # Non-gating diagnostics matrix: the real served app versus the standalone
  # canvas probe, with exactly one GL/ANGLE or Vulkan control token removed from
  # the Variant A flags at a time. One additional candidate substitutes only the
  # --enable-features=Vulkan token (see FEATURE_CONTROL_* below); it is not an
  # omission. A missing token is an explicit invalid-input result, never a
  # silently skipped combination. These probes run after the gated smoke/A
  # suites and never influence their statuses or CI gating.
  APP_ENTRY="/?autostart=1&backend=webgpu"
  CANVAS_ENTRY="/__linux_webgpu_smoke__"
  MATRIX_OMISSION_FLAGS=(
    "--enable-features=Vulkan:omit-enable-features-vulkan"
    "--use-angle=swiftshader:omit-use-angle-swiftshader"
    "--use-vulkan=swiftshader:omit-use-vulkan-swiftshader"
    "--disable-vulkan-surface:omit-disable-vulkan-surface"
  )
  FEATURE_CONTROL_LABEL="features-vulkan-from-angle"
  FEATURE_CONTROL_FROM="--enable-features=Vulkan"
  FEATURE_CONTROL_TO="--enable-features=Vulkan,VulkanFromANGLE,DefaultANGLEVulkan"
  FEATURE_CONTROL_SUMMARY="changed=$FEATURE_CONTROL_FROM -> $FEATURE_CONTROL_TO"
  MATRIX_RECORDS=()

  matrix_entry() {
    if [ "$1" = app ]; then
      printf 'app:%s' "$APP_ENTRY"
    else
      printf 'standalone:%s' "$CANVAS_ENTRY"
    fi
  }

  json_escape() {
    local escape_text="$1" control code replacement
    escape_text="${escape_text//\\/\\\\}"
    escape_text="${escape_text//\"/\\\"}"
    for code in {1..31}; do
      printf -v control '%b' "\\$(printf '%03o' "$code")"
      printf -v replacement '\\u%04x' "$code"
      escape_text="${escape_text//"$control"/"$replacement"}"
    done
    printf '%s' "$escape_text"
  }

  write_matrix_record() {
    record_label="$1"
    record_mode="$2"
    record_omitted="$3"
    record_status="$4"
    record_outcome="$5"
    record_args="$6"
    record_reason="$7"
    record_dir="$LINUX_DIAGNOSTICS_DIR/matrix-$record_label-$record_mode"
    record_result_written=false
    if [ -s "$record_dir/result.json" ]; then record_result_written=true; fi
    printf '%s\n' "$record_status" >"$record_dir/exit-status.txt"
    printf '{"label":"%s","mode":"%s","entry":"%s","omittedFlag":"%s","launchArgs":"%s","processExitStatus":%s,"resultWritten":%s,"outcome":"%s","reason":"%s","nonGating":true}\n' \
      "$record_label" "$record_mode" "$(matrix_entry "$record_mode")" "$record_omitted" \
      "$(json_escape "$record_args")" "$record_status" "$record_result_written" \
      "$record_outcome" "$(json_escape "$record_reason")" >"$record_dir/process-status.json"
    MATRIX_RECORDS+=("$record_label|$record_mode|omitted=$record_omitted|$record_status|$record_outcome")
  }

  write_matrix_run_info() {
    info_label="$1"
    info_mode="$2"
    info_omitted="$3"
    info_dir="$LINUX_DIAGNOSTICS_DIR/matrix-$info_label-$info_mode"
    printf 'label=%s\nmode=%s\nomitted_flag=%s\nentry=%s\norigin=%s\nchannel=%s\nheadless=%s\nchromium_revision=%s\nnon_gating=true\n' \
      "$info_label" "$info_mode" "$info_omitted" "$(matrix_entry "$info_mode")" \
      "$URL" "$CHANNEL" "$HEADLESS" "$PLAYWRIGHT_REVISION" >"$info_dir/run-info.txt"
  }

  run_matrix_probe() {
    probe_label="$1"
    probe_mode="$2"
    probe_omitted="$3"
    probe_args="$4"
    probe_dir="$LINUX_DIAGNOSTICS_DIR/matrix-$probe_label-$probe_mode"
    if ! mkdir "$probe_dir"; then
      info "could not allocate fresh $probe_dir; recording matrix failure and continuing"
      MATRIX_RECORDS+=("$probe_label|$probe_mode|omitted=$probe_omitted|125|probe-directory-unavailable")
      return 0
    fi
    printf '%s\n' "$probe_args" >"$probe_dir/launch-args.txt"
    printf '%s\n' "$probe_omitted" >"$probe_dir/omitted-flag.txt"
    write_matrix_run_info "$probe_label" "$probe_mode" "$probe_omitted"
    info "non-gating GPU matrix probe: $probe_label mode=$probe_mode omitted=$probe_omitted"
    if (
      cd "$ROOT"
      DEBUG=pw:browser \
      WEBGPU_SMOKE_OUTPUT_DIR="$probe_dir" \
      WEB_PLAYWRIGHT_CHROMIUM_REVISION="$PLAYWRIGHT_REVISION" \
      WEB_APP_URL="$URL" \
      WEB_E2E_CHANNEL="$CHANNEL" \
      WEB_E2E_LAUNCH_ARGS="$probe_args" \
      PLAYWRIGHT_PACKAGE_PATH="$NPM/node_modules/playwright" \
        "$NODE_BIN" "$ROOT/scripts/web-linux-gpu-smoke.cjs" "$probe_mode" \
        2>&1 | tee "$probe_dir/playwright.log"
    ); then
      probe_status=0
    else
      probe_status=$?
    fi
    probe_outcome=harness-failure
    if [ -s "$probe_dir/result.json" ]; then
      if [ "$probe_status" -eq 0 ]; then
        probe_outcome=completed
      else
        probe_outcome=probe-reported-failure
      fi
    fi
    write_matrix_record "$probe_label" "$probe_mode" "$probe_omitted" "$probe_status" \
      "$probe_outcome" "$probe_args" ""
    return 0
  }

  record_matrix_invalid_input() {
    invalid_label="$1"
    invalid_mode="$2"
    invalid_omitted="$3"
    invalid_count="$4"
    invalid_dir="$LINUX_DIAGNOSTICS_DIR/matrix-$invalid_label-$invalid_mode"
    if ! mkdir "$invalid_dir"; then
      info "could not allocate fresh $invalid_dir; recording matrix failure and continuing"
      MATRIX_RECORDS+=("$invalid_label|$invalid_mode|omitted=$invalid_omitted|125|probe-directory-unavailable")
      return 0
    fi
    invalid_reason="$invalid_omitted appears $invalid_count times in the Variant A flags; exactly one omission control is required, so this combination was not probed"
    printf '%s\n' "$LAUNCH_ARGS" >"$invalid_dir/launch-args.txt"
    printf '%s\n' "$invalid_reason" >"$invalid_dir/invalid-input.txt"
    printf '%s\n' "$invalid_omitted" >"$invalid_dir/omitted-flag.txt"
    write_matrix_run_info "$invalid_label" "$invalid_mode" "$invalid_omitted"
    info "non-gating GPU matrix probe: $invalid_label mode=$invalid_mode invalid input ($invalid_omitted count $invalid_count)"
    write_matrix_record "$invalid_label" "$invalid_mode" "$invalid_omitted" \
      "$MATRIX_INVALID_INPUT_STATUS" invalid-input "$LAUNCH_ARGS" "$invalid_reason"
    return 0
  }

  # Additive feature-combination candidate (never an omission): substitute only
  # the single --enable-features=Vulkan argv token and keep every other token.
  write_feature_run_info() {
    feature_info_mode="$1"
    feature_info_dir="$LINUX_DIAGNOSTICS_DIR/matrix-$FEATURE_CONTROL_LABEL-$feature_info_mode"
    printf 'label=%s\nmode=%s\ncontrol=feature-substitution\nchanged_flag=%s\nreplacement_flag=%s\nentry=%s\norigin=%s\nchannel=%s\nheadless=%s\nchromium_revision=%s\nnon_gating=true\n' \
      "$FEATURE_CONTROL_LABEL" "$feature_info_mode" "$FEATURE_CONTROL_FROM" \
      "$FEATURE_CONTROL_TO" "$(matrix_entry "$feature_info_mode")" "$URL" "$CHANNEL" \
      "$HEADLESS" "$PLAYWRIGHT_REVISION" >"$feature_info_dir/run-info.txt"
  }

  write_feature_record() {
    feature_mode="$1"
    feature_status="$2"
    feature_outcome="$3"
    feature_args="$4"
    feature_reason="$5"
    feature_dir="$LINUX_DIAGNOSTICS_DIR/matrix-$FEATURE_CONTROL_LABEL-$feature_mode"
    feature_result_written=false
    if [ -s "$feature_dir/result.json" ]; then feature_result_written=true; fi
    printf '%s\n' "$feature_status" >"$feature_dir/exit-status.txt"
    printf '{"label":"%s","mode":"%s","control":"feature-substitution","entry":"%s","changedFlag":"%s","replacementFlag":"%s","launchArgs":"%s","processExitStatus":%s,"resultWritten":%s,"outcome":"%s","reason":"%s","nonGating":true}\n' \
      "$FEATURE_CONTROL_LABEL" "$feature_mode" "$(matrix_entry "$feature_mode")" \
      "$FEATURE_CONTROL_FROM" "$FEATURE_CONTROL_TO" "$(json_escape "$feature_args")" \
      "$feature_status" "$feature_result_written" "$feature_outcome" \
      "$(json_escape "$feature_reason")" >"$feature_dir/process-status.json"
    MATRIX_RECORDS+=("$FEATURE_CONTROL_LABEL|$feature_mode|$FEATURE_CONTROL_SUMMARY|$feature_status|$feature_outcome")
  }

  run_feature_control_probe() {
    feature_probe_mode="$1"
    feature_probe_args="$2"
    feature_probe_dir="$LINUX_DIAGNOSTICS_DIR/matrix-$FEATURE_CONTROL_LABEL-$feature_probe_mode"
    if ! mkdir "$feature_probe_dir"; then
      info "could not allocate fresh $feature_probe_dir; recording matrix failure and continuing"
      MATRIX_RECORDS+=("$FEATURE_CONTROL_LABEL|$feature_probe_mode|$FEATURE_CONTROL_SUMMARY|125|probe-directory-unavailable")
      return 0
    fi
    printf '%s\n' "$feature_probe_args" >"$feature_probe_dir/launch-args.txt"
    printf '%s\n' "$FEATURE_CONTROL_SUMMARY" >"$feature_probe_dir/changed-arg.txt"
    write_feature_run_info "$feature_probe_mode"
    info "non-gating GPU matrix probe: $FEATURE_CONTROL_LABEL mode=$feature_probe_mode $FEATURE_CONTROL_SUMMARY"
    if (
      cd "$ROOT"
      DEBUG=pw:browser \
      WEBGPU_SMOKE_OUTPUT_DIR="$feature_probe_dir" \
      WEB_PLAYWRIGHT_CHROMIUM_REVISION="$PLAYWRIGHT_REVISION" \
      WEB_APP_URL="$URL" \
      WEB_E2E_CHANNEL="$CHANNEL" \
      WEB_E2E_LAUNCH_ARGS="$feature_probe_args" \
      PLAYWRIGHT_PACKAGE_PATH="$NPM/node_modules/playwright" \
        "$NODE_BIN" "$ROOT/scripts/web-linux-gpu-smoke.cjs" "$feature_probe_mode" \
        2>&1 | tee "$feature_probe_dir/playwright.log"
    ); then
      feature_probe_status=0
    else
      feature_probe_status=$?
    fi
    feature_probe_outcome=harness-failure
    if [ -s "$feature_probe_dir/result.json" ]; then
      if [ "$feature_probe_status" -eq 0 ]; then
        feature_probe_outcome=completed
      else
        feature_probe_outcome=probe-reported-failure
      fi
    fi
    write_feature_record "$feature_probe_mode" "$feature_probe_status" \
      "$feature_probe_outcome" "$feature_probe_args" ""
    return 0
  }

  record_feature_invalid_input() {
    feature_invalid_mode="$1"
    feature_invalid_count="$2"
    feature_invalid_dir="$LINUX_DIAGNOSTICS_DIR/matrix-$FEATURE_CONTROL_LABEL-$feature_invalid_mode"
    if ! mkdir "$feature_invalid_dir"; then
      info "could not allocate fresh $feature_invalid_dir; recording matrix failure and continuing"
      MATRIX_RECORDS+=("$FEATURE_CONTROL_LABEL|$feature_invalid_mode|$FEATURE_CONTROL_SUMMARY|125|probe-directory-unavailable")
      return 0
    fi
    feature_invalid_reason="$FEATURE_CONTROL_FROM appears $feature_invalid_count times in the Variant A flags; exactly one token is required to substitute $FEATURE_CONTROL_TO, so this combination was not probed"
    printf '%s\n' "$LAUNCH_ARGS" >"$feature_invalid_dir/launch-args.txt"
    printf '%s\n' "$feature_invalid_reason" >"$feature_invalid_dir/invalid-input.txt"
    printf '%s\n' "$FEATURE_CONTROL_SUMMARY" >"$feature_invalid_dir/changed-arg.txt"
    write_feature_run_info "$feature_invalid_mode"
    info "non-gating GPU matrix probe: $FEATURE_CONTROL_LABEL mode=$feature_invalid_mode invalid input ($FEATURE_CONTROL_FROM count $feature_invalid_count)"
    write_feature_record "$feature_invalid_mode" "$MATRIX_INVALID_INPUT_STATUS" \
      invalid-input "$LAUNCH_ARGS" "$feature_invalid_reason"
    return 0
  }

  run_matrix_probe baseline app - "$LAUNCH_ARGS"
  for omission_control in "${MATRIX_OMISSION_FLAGS[@]}"; do
    omission="${omission_control%%:*}"
    omission_label="${omission_control#*:}"
    omission_count=0
    matrix_variant_args=()
    for arg in "${A_LAUNCH_ARGS[@]}"; do
      if [ "$arg" = "$omission" ]; then
        omission_count=$((omission_count + 1))
        if [ "$omission_count" -eq 1 ]; then continue; fi
      fi
      matrix_variant_args+=("$arg")
    done
    if [ "$omission_count" -ne 1 ]; then
      record_matrix_invalid_input "$omission_label" canvas "$omission" "$omission_count"
      record_matrix_invalid_input "$omission_label" app "$omission" "$omission_count"
      continue
    fi
    matrix_variant_text="${matrix_variant_args[*]}"
    run_matrix_probe "$omission_label" canvas "$omission" "$matrix_variant_text"
    run_matrix_probe "$omission_label" app "$omission" "$matrix_variant_text"
  done

  feature_control_count=0
  FEATURE_CONTROL_ARGS=()
  for arg in "${A_LAUNCH_ARGS[@]}"; do
    if [ "$arg" = "$FEATURE_CONTROL_FROM" ]; then
      feature_control_count=$((feature_control_count + 1))
      if [ "$feature_control_count" -eq 1 ]; then
        FEATURE_CONTROL_ARGS+=("$FEATURE_CONTROL_TO")
        continue
      fi
    fi
    FEATURE_CONTROL_ARGS+=("$arg")
  done
  if [ "$feature_control_count" -ne 1 ]; then
    record_feature_invalid_input canvas "$feature_control_count"
    record_feature_invalid_input app "$feature_control_count"
  else
    FEATURE_CONTROL_ARGS_TEXT="${FEATURE_CONTROL_ARGS[*]}"
    run_feature_control_probe canvas "$FEATURE_CONTROL_ARGS_TEXT"
    run_feature_control_probe app "$FEATURE_CONTROL_ARGS_TEXT"
  fi

  SHARED_IMAGE_LOGS=(
    "$LINUX_DIAGNOSTICS_DIR/smoke-canvas/playwright.log"
    "$LINUX_DIAGNOSTICS_DIR/smoke-offscreen/playwright.log"
    "$LINUX_DIAGNOSTICS_DIR/variant-a/playwright.log"
    "$LINUX_DIAGNOSTICS_DIR/variant-b/playwright.log"
  )
  SHARED_LOGS_COMPLETE=1
  for log in "${SHARED_IMAGE_LOGS[@]}"; do
    [ -f "$log" ] || SHARED_LOGS_COMPLETE=0
  done
  if [ "$SHARED_LOGS_COMPLETE" != 1 ]; then
    SHARED_IMAGE_FAILURES=unknown-incomplete-logs
    printf 'Could not inspect every browser log for SharedImageBackingFactory failures.\n' \
      >"$LINUX_DIAGNOSTICS_DIR/shared-image-backing-factory.txt"
  elif grep -Hni 'SharedImageBackingFactory' "${SHARED_IMAGE_LOGS[@]}" \
    >"$LINUX_DIAGNOSTICS_DIR/shared-image-backing-factory.txt"; then
    if grep -Eiq '(error|fail)' "$LINUX_DIAGNOSTICS_DIR/shared-image-backing-factory.txt"; then
      SHARED_IMAGE_FAILURES=observed
    else
      SHARED_IMAGE_FAILURES=not-observed
    fi
  else
    grep_status=$?
    if [ "$grep_status" -eq 1 ]; then
      SHARED_IMAGE_FAILURES=not-observed
      printf 'No SharedImageBackingFactory messages were observed in the standalone probe or A/B browser logs.\n' \
        >"$LINUX_DIAGNOSTICS_DIR/shared-image-backing-factory.txt"
    else
      SHARED_IMAGE_FAILURES=unknown-log-read-error
      printf 'Could not read every browser log for SharedImageBackingFactory failures.\n' \
        >"$LINUX_DIAGNOSTICS_DIR/shared-image-backing-factory.txt"
    fi
  fi

  printf 'smoke-canvas=%s\nsmoke-offscreen=%s\nA=%s\nB=%s\nxvfb-run-wrapper=%s\nSharedImageBackingFactoryFailures=%s\nmatrix-non-gating=true\n' \
    "$SMOKE_CANVAS_STATUS" "$SMOKE_OFFSCREEN_STATUS" "$A_STATUS" "$B_STATUS" \
    "${WEB_E2E_XVFB:-0}" "$SHARED_IMAGE_FAILURES" >"$LINUX_DIAGNOSTICS_DIR/summary.txt"
  : >"$LINUX_DIAGNOSTICS_DIR/matrix.txt"
  for matrix_record in "${MATRIX_RECORDS[@]}"; do
    IFS='|' read -r matrix_label matrix_mode matrix_control matrix_status matrix_outcome <<<"$matrix_record"
    printf 'matrix-%s-%s=%s\n' "$matrix_label" "$matrix_mode" "$matrix_status" \
      >>"$LINUX_DIAGNOSTICS_DIR/summary.txt"
    printf '%s/%s %s exit=%s outcome=%s nonGating=true\n' \
      "$matrix_label" "$matrix_mode" "$matrix_control" "$matrix_status" "$matrix_outcome" \
      >>"$LINUX_DIAGNOSTICS_DIR/matrix.txt"
  done
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      printf '## Linux WebGPU diagnostic experiment\n\n'
      printf '| Check | Exit/status |\n| --- | ---: |\n'
      printf '| Canvas smoke | %s |\n| Offscreen smoke | %s |\n| Functional E2E A | %s |\n| Functional E2E B | %s |\n' \
        "$SMOKE_CANVAS_STATUS" "$SMOKE_OFFSCREEN_STATUS" "$A_STATUS" "$B_STATUS"
      printf '\nE2E re-executed under `xvfb-run`: **%s** (records wrapper use only; it does not show that Chromium consumed a virtual display).\n' "${WEB_E2E_XVFB:-0}"
      printf '\nSharedImageBackingFactory failures: **%s**.\n\n' "$SHARED_IMAGE_FAILURES"
      printf 'Artifacts: `%s`. These are software-runner diagnostics, not GPU hardware evidence or root-cause proof.\n' \
        "$LINUX_DIAGNOSTICS_DIR"
      printf '\n### Non-gating GPU flag matrix (diagnostics only)\n\n'
      printf '| Control | Mode | Change | Exit | Outcome |\n| --- | --- | --- | ---: | --- |\n'
      for matrix_record in "${MATRIX_RECORDS[@]}"; do
        IFS='|' read -r matrix_label matrix_mode matrix_control matrix_status matrix_outcome <<<"$matrix_record"
        printf '| `%s` | %s | `%s` | %s | %s |\n' \
          "$matrix_label" "$matrix_mode" "$matrix_control" "$matrix_status" "$matrix_outcome"
      done
      printf '\nThese matrix rows are **non-gating**: they never change the job status. The two standalone probes and the full A/B suites above remain the only gated results. Differing or matching matrix outcomes are not root-cause proof. The `features-vulkan-from-angle` row substitutes one argv token and is not an omission.\n'
      printf 'Matrix details: `%s/matrix.txt`.\n' "$LINUX_DIAGNOSTICS_DIR"
    } >>"$GITHUB_STEP_SUMMARY"
  fi
  info "Linux WebGPU smoke statuses: canvas=$SMOKE_CANVAS_STATUS offscreen=$SMOKE_OFFSCREEN_STATUS"
  info "Linux A/B E2E statuses: A=$A_STATUS B=$B_STATUS; xvfb-run wrapper=${WEB_E2E_XVFB:-0}; SharedImageBackingFactory failures=$SHARED_IMAGE_FAILURES; logs and artifacts: $LINUX_DIAGNOSTICS_DIR"
  info "non-gating GPU flag matrix results: $LINUX_DIAGNOSTICS_DIR/matrix.txt (these statuses never gate CI)"
  if [ "$SMOKE_CANVAS_STATUS" -ne 0 ] || [ "$SMOKE_OFFSCREEN_STATUS" -ne 0 ] ||
    [ "$A_STATUS" -ne 0 ] || [ "$B_STATUS" -ne 0 ]; then
    fail "Linux WebGPU diagnostics/browser E2E failed (canvas=$SMOKE_CANVAS_STATUS, offscreen=$SMOKE_OFFSCREEN_STATUS, A=$A_STATUS, B=$B_STATUS); see $LINUX_DIAGNOSTICS_DIR/summary.txt"
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
