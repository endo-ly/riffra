#!/usr/bin/env bash
# Build the native audio engine, run tests, and install sidecar binaries
# to apps/desktop/src-tauri/binaries.
set -euo pipefail

CONFIG="${1:-Release}"
ENGINE_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$ENGINE_DIR/../.." && pwd)"
BUILD_DIR="${BUILD_DIR:-$ENGINE_DIR/build}"
SKIP_TESTS="${SKIP_TESTS:-0}"

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
"$CMAKE" -S . -B "$BUILD_DIR" -DCMAKE_BUILD_TYPE="$CONFIG"
"$CMAKE" --build "$BUILD_DIR" --config "$CONFIG" --parallel
if [ "$SKIP_TESTS" -ne 1 ]; then
  "$CTEST" --test-dir "$BUILD_DIR" --output-on-failure -C "$CONFIG"
fi
"$CMAKE" --install "$BUILD_DIR" --prefix "$REPO_ROOT" --component riffra-sidecars --config "$CONFIG"

echo "Audio engine built, tested, and installed to apps/desktop/src-tauri/binaries"
