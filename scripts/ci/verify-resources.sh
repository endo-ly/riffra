#!/usr/bin/env bash
set -euo pipefail

configuration="${1:-Debug}"
case "$configuration" in
  Debug|RelWithDebInfo) target_directory="target/debug" ;;
  Release) target_directory="target/release" ;;
  *) echo "Configuration must be Debug, RelWithDebInfo, or Release." >&2; exit 1 ;;
esac

for path in \
  "$target_directory/riffra-resources/instruments/builtin/manifest.json" \
  "$target_directory/riffra-resources/THIRD_PARTY_NOTICES.md" \
  "apps/desktop/src-tauri/resources/instruments/builtin/manifest.json" \
  "apps/desktop/src-tauri/resources/THIRD_PARTY_NOTICES.md" \
  "apps/desktop/src-tauri/resources/LICENSE-MIT" \
  "apps/desktop/src-tauri/resources/LICENSE-APACHE"; do
  test -f "$path"
done
