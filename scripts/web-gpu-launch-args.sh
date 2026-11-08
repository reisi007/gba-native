#!/usr/bin/env bash
# Print the Chromium WebGPU/SwiftShader launch preset used by Rune browser checks.
# This is a software-browser test configuration, not a physical-GPU rendering
# claim.

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
