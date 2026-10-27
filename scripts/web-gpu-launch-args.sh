#!/usr/bin/env bash
# Print the pinned Chromium WebGPU/SwiftShader launch preset.
#
# Linux candidate configuration under test (issue #24). Compared with the
# earlier preset it selects ANGLE's SwiftShader backend
# (--use-angle=swiftshader) instead of the native Vulkan ANGLE backend and omits
# --disable-vulkan-surface. That configuration, then-current Variant B, passed
# all 19 functional tests in Linux run 40547520962 under the former policy that
# gated both suites. The recorded Linux
# runs also show a SharedImageBackingFactory failure and post-init device loss on
# the WebGPU canvas path; they do not establish that these flags fix it, so this
# remains a candidate configuration under test, not a root cause. No claim is
# made that it enables GPU rendering.
#
# The effective flags of this preset contain no --disable-vulkan-surface token.
# Linux A/B diagnostics (scripts/web-e2e.sh) require that, form the matched
# surface-on Variant B by appending exactly one token, and derive their matrix
# reference preset from B. The screenshot runner uses this preset unchanged.
#
# Linux CI functional E2E (scripts/web-e2e.sh ci with CI=true and no DISPLAY)
# is re-executed under xvfb-run as part of the same candidate; a missing wrapper
# fails that gate. The screenshot runner does not use that wrapper.

set -euo pipefail

if [ "$#" -gt 1 ]; then
  printf 'usage: %s [platform-name-for-contract-tests]\n' "$0" >&2
  exit 2
fi

PLATFORM="${1:-$(uname -s)}"
ARGS=(
  --enable-unsafe-webgpu
  --use-webgpu-adapter=swiftshader
  --enable-dawn-features=allow_unsafe_apis
  --disable-dawn-features=use_dxc
  --enable-webgpu-developer-features
  --use-gpu-in-tests
  --enable-accelerated-2d-canvas
)

if [ "$PLATFORM" = Linux ]; then
  ARGS+=(
    --enable-features=Vulkan
    --use-angle=swiftshader
    --use-vulkan=swiftshader
  )
fi

printf '%s' "${ARGS[0]}"
for arg in "${ARGS[@]:1}"; do
  printf ' %s' "$arg"
done
printf '\n'
