#!/usr/bin/env bash
#
# Provision the Playwright Chromium revision used by the functional browser E2E.
#
# The browser binary is a cache, not a global tool: Playwright places it in the
# standard user cache unless PLAYWRIGHT_BROWSERS_PATH is set. On Linux (CI) this
# also installs the required system libraries via the distribution package
# manager; on macOS only the browser download is needed.
#
# The tool versions themselves (Node, wasm-bindgen, Tailwind/DaisyUI/Playwright
# npm packages) are provisioned by scripts/bootstrap-web-tools.sh.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
"$ROOT/scripts/bootstrap-web-tools.sh"

NODE_BIN="$ROOT/target/tools/node-pinned/bin/node"
NPM_DIR="$ROOT/target/web-tools/npm"
[ -x "$NODE_BIN" ] || fail "pinned Node runtime is missing after bootstrap"
[ -f "$NPM_DIR/node_modules/@playwright/test/cli.js" ] || fail "Playwright tooling is missing after bootstrap"

info "installing Playwright Chromium (pinned revision from package-lock.json)"
case "$(uname -s)" in
  Linux)
    (
      cd "$NPM_DIR"
      PATH="$ROOT/target/tools/node-pinned/bin:$PATH" npx playwright install --with-deps chromium
    )
    ;;
  Darwin)
    (
      cd "$NPM_DIR"
      PATH="$ROOT/target/tools/node-pinned/bin:$PATH" npx playwright install chromium
    )
    ;;
  *)
    fail "unsupported host OS for Playwright browser provisioning: $(uname -s)"
    ;;
esac

REVISION=$(cd "$NPM_DIR" && "$NODE_BIN" -e '
const fs = require("fs");
const browsers = JSON.parse(fs.readFileSync("node_modules/playwright-core/browsers.json", "utf8")).browsers;
console.log(browsers.find((entry) => entry.name === "chromium").revision);
')
info "Playwright Chromium revision $REVISION ready"
