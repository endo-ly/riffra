#!/usr/bin/env bash
set -euo pipefail

configuration="${1:-Debug}"
case "$configuration" in
  Debug|RelWithDebInfo) target_directory="target/debug" ;;
  Release) target_directory="target/release" ;;
  *) echo "Configuration must be Debug, RelWithDebInfo, or Release." >&2; exit 1 ;;
esac

target_triple="$(rustc -vV | sed -n 's/^host: //p')"
for sidecar in riffra-audio riffra-plugin-scan riffra-render sonalloy; do
  test -x "$target_directory/$sidecar"
  test -x "apps/desktop/src-tauri/binaries/${sidecar}-${target_triple}"
done
