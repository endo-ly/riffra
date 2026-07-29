#!/usr/bin/env bash
# Build the native audio engine, run tests, and install sidecar binaries
# to src-tauri/binaries.
set -euo pipefail

CONFIG="${1:-Release}"
ENGINE_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$ENGINE_DIR/../.." && pwd)"

CMAKE="${CMAKE:-$(command -v cmake || true)}"
if [ -z "$CMAKE" ]; then
  echo "cmake not found. Install CMake or add it to PATH." >&2
  exit 1
fi

CTEST="${CTEST:-$(command -v ctest || true)}"
if [ -z "$CTEST" ]; then
  echo "ctest not found. Install CMake or add it to PATH." >&2
  exit 1
fi

cd "$ENGINE_DIR"
"$CMAKE" -S . -B build -DCMAKE_BUILD_TYPE="$CONFIG"
"$CMAKE" --build build --config "$CONFIG" --parallel
"$CMAKE" --build build --target riffra-plugin-scan --config "$CONFIG" --parallel
"$CTEST" --test-dir build --output-on-failure -C "$CONFIG"
"$CMAKE" --install build --prefix "$REPO_ROOT" --component riffra-sidecars

echo "Audio engine built, tested, and installed to src-tauri/binaries"
