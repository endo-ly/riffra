#!/usr/bin/env bash
# Build the native audio engine, run tests, and install sidecars and resources.
set -euo pipefail

CONFIG="${1:-Release}"
ENGINE_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$ENGINE_DIR/../.." && pwd)"
BUILD_DIR="${BUILD_DIR:-$ENGINE_DIR/build}"
SKIP_TESTS="${SKIP_TESTS:-0}"
SIDECARS_ONLY="${SIDECARS_ONLY:-0}"

case "$CONFIG" in
  Debug)
    headless_destination="target/debug"
    headless_resources_destination="target/debug"
    ;;
  RelWithDebInfo)
    headless_destination="target/debug"
    headless_resources_destination="target/debug"
    ;;
  Release)
    headless_destination="target/release"
    headless_resources_destination="target/release"
    ;;
  *)
    echo "Configuration must be Debug, RelWithDebInfo, or Release." >&2
    exit 1
    ;;
esac
headless_destination="${RIFFRA_HEADLESS_BINARIES_DESTINATION:-$headless_destination}"
headless_resources_destination="${RIFFRA_HEADLESS_RESOURCES_DESTINATION:-$headless_resources_destination}"

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
configure_args=(
  -S .
  -B "$BUILD_DIR"
  -DCMAKE_BUILD_TYPE="$CONFIG"
  -DRIFFRA_HEADLESS_BINARIES_DESTINATION="$headless_destination"
  -DRIFFRA_HEADLESS_RESOURCES_DESTINATION="$headless_resources_destination"
)
if [ -n "${CMAKE_C_COMPILER_LAUNCHER:-}" ]; then
  configure_args+=("-DCMAKE_C_COMPILER_LAUNCHER=$CMAKE_C_COMPILER_LAUNCHER")
fi
if [ -n "${CMAKE_CXX_COMPILER_LAUNCHER:-}" ]; then
  configure_args+=("-DCMAKE_CXX_COMPILER_LAUNCHER=$CMAKE_CXX_COMPILER_LAUNCHER")
fi
if [ "$SKIP_TESTS" -eq 1 ]; then
  configure_args+=("-DBUILD_TESTING=OFF")
else
  configure_args+=("-DBUILD_TESTING=ON")
fi
"$CMAKE" "${configure_args[@]}"
build_args=(--build "$BUILD_DIR" --config "$CONFIG")
if [[ "$SIDECARS_ONLY" -eq 1 ]]; then
  build_args+=(--target riffra-runtime-sidecars)
fi
build_args+=(--parallel)
if [ -n "${CMAKE_BUILD_PARALLEL_LEVEL:-}" ]; then
  build_args+=("$CMAKE_BUILD_PARALLEL_LEVEL")
fi
"$CMAKE" "${build_args[@]}"
if [ "$SKIP_TESTS" -ne 1 ]; then
  ctest_args=(--test-dir "$BUILD_DIR" --output-on-failure -C "$CONFIG")
  if [ -n "${CTEST_PARALLEL_LEVEL:-}" ]; then
    ctest_args+=(--parallel "$CTEST_PARALLEL_LEVEL")
  fi
  if [ "${RIFFRA_RUN_SLOW_TESTS:-0}" -ne 1 ]; then
    ctest_args+=(-LE slow)
  fi
  "$CTEST" "${ctest_args[@]}"
fi
"$CMAKE" --install "$BUILD_DIR" --prefix "$REPO_ROOT" --component riffra-sidecars --config "$CONFIG"

echo "Audio engine built and installed to apps/desktop/src-tauri and $headless_destination"
