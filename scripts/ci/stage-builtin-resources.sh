#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <destination>" >&2
    exit 2
fi

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"
destination=$1
if [[ -z "$destination" ]]; then
    echo 'A destination for built-in instrument resources is required.' >&2
    exit 1
fi
revision=${RIFFRA_SONALLOY_REVISION:-}

if [[ -z "$revision" ]]; then
    revision=$(sed -n 's/^set(RIFFRA_SONALLOY_REVISION "\([^"]*\)".*/\1/p' \
        "$repo_root/native/audio-engine/CMakeLists.txt")
fi
if [[ -z "$revision" ]]; then
    echo 'Sonalloy revision could not be resolved from native/audio-engine/CMakeLists.txt' >&2
    exit 1
fi

temporary_root=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/riffra-sonalloy-resources.XXXXXX")
trap 'rm -rf "$temporary_root"' EXIT

git init -q "$temporary_root"
git -C "$temporary_root" remote add origin https://github.com/endo-ly/sonalloy.git
git -C "$temporary_root" fetch --quiet --depth 1 origin "$revision"
git -C "$temporary_root" checkout --quiet --detach FETCH_HEAD

cmake \
    "-DSOURCE_PRESETS=$temporary_root/presets" \
    "-DDESTINATION=$destination" \
    "-DSOURCE_REVISION=$revision" \
    -P "$repo_root/native/audio-engine/cmake/stage_builtin_resources.cmake"
