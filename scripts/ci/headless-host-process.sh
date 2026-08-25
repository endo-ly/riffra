#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"

data_root=$(mktemp -d "${TMPDIR:-/tmp}/riffra-headless-host.XXXXXX")
serve_pid=""
safe_mode="${RIFFRA_HEADLESS_SAFE_MODE:-1}"

case "$safe_mode" in
    0|1) ;;
    *)
        echo 'RIFFRA_HEADLESS_SAFE_MODE must be 0 or 1' >&2
        exit 1
        ;;
esac

cleanup() {
    if [[ -n "$serve_pid" ]] && kill -0 "$serve_pid" 2>/dev/null; then
        kill -INT "$serve_pid" 2>/dev/null || true
        wait "$serve_pid" 2>/dev/null || true
    fi
    rm -rf "$data_root"
}
trap cleanup EXIT

cargo build -p riffra-cli
binary="${CARGO_TARGET_DIR:-target}/debug/riffra"
serve_args=(serve)
if [[ "$safe_mode" == 1 ]]; then
    serve_args+=(--safe-mode)
fi
"$binary" --data-root "$data_root" "${serve_args[@]}" \
    >"$data_root/serve.stdout.log" 2>"$data_root/serve.stderr.log" &
serve_pid=$!

for _ in $(seq 1 200); do
    if [[ -f "$data_root/control/host.json" ]]; then
        break
    fi
    if ! kill -0 "$serve_pid" 2>/dev/null; then
        wait "$serve_pid" || true
        sed -n '1,160p' "$data_root/serve.stderr.log" >&2 || true
        exit 1
    fi
    sleep 0.05
done

test -f "$data_root/control/host.json"

"$binary" --data-root "$data_root" --attach session get \
    >"$data_root/session.json"
jq -e '.ok == true and .result.type == "session" and .sequence == 0' \
    "$data_root/session.json" >/dev/null
printf '%s\n' '{"requestId":"bootstrap","command":"host.bootstrap","params":{}}' |
    "$binary" --data-root "$data_root" --attach --interactive \
    >"$data_root/bootstrap.json"
jq -e '.ok == true and .result.type == "hostBootstrap" and .result.value.canonical.sequence == 0' \
    "$data_root/bootstrap.json" >/dev/null
"$binary" --data-root "$data_root" --attach track add \
    --name "Process Test" --kind instrument >"$data_root/track.json"
jq -e '.ok == true and .result.type == "session" and .sequence == 1' \
    "$data_root/track.json" >/dev/null
"$binary" --data-root "$data_root" --attach undo >"$data_root/undo.json"
jq -e '.ok == true and .result.type == "arrangementMutation" and .sequence == 2' \
    "$data_root/undo.json" >/dev/null

if [[ "$safe_mode" == 1 ]]; then
    "$binary" --data-root "$data_root" --attach audio status >"$data_root/safe-audio.json"
    jq -e '.ok == true and .result.type == "audioStatus"' \
        "$data_root/safe-audio.json" >/dev/null

    if "$binary" --data-root "$data_root" --attach transport play \
        --transport-sequence 1 >"$data_root/transport.stdout.log" 2>"$data_root/transport.stderr.log"; then
        echo 'transport play unexpectedly succeeded in Safe Mode' >&2
        exit 1
    fi
    grep -q 'runtimeUnavailable' "$data_root/transport.stderr.log"

    if "$binary" --data-root "$data_root" --attach audio probe \
        >"$data_root/probe.stdout.log" 2>"$data_root/probe.stderr.log"; then
        echo 'audio probe unexpectedly succeeded in Safe Mode' >&2
        exit 1
    fi
    grep -q 'runtimeUnavailable' "$data_root/probe.stderr.log"

    if "$binary" --data-root "$data_root" --attach plugin scan \
        --path "$data_root" >"$data_root/plugin.stdout.log" 2>"$data_root/plugin.stderr.log"; then
        echo 'plugin scan unexpectedly succeeded in Safe Mode' >&2
        exit 1
    fi
    grep -q 'runtimeUnavailable' "$data_root/plugin.stderr.log"
else
    "$binary" --data-root "$data_root" --attach host status >"$data_root/host.json"
    jq -e '.ok == true and .result.type == "hostStatus"' "$data_root/host.json" >/dev/null
    "$binary" --data-root "$data_root" --attach audio status >"$data_root/audio.json"
    jq -e '.ok == true and .result.type == "audioStatus"' "$data_root/audio.json" >/dev/null
fi

"$binary" --data-root "$data_root" --attach host shutdown \
    >"$data_root/shutdown.json"
jq -e '.ok == true and .result.type == "ok"' "$data_root/shutdown.json" >/dev/null
for _ in $(seq 1 200); do
    if ! kill -0 "$serve_pid" 2>/dev/null; then
        break
    fi
    sleep 0.05
done
if kill -0 "$serve_pid" 2>/dev/null; then
    echo 'riffra serve did not stop after host shutdown' >&2
    exit 1
fi
wait "$serve_pid"

test ! -e "$data_root/control/host.json"
test ! -e "$data_root/control/host.sock"
"$binary" --data-root "$data_root" session get >"$data_root/reopened.json"
jq -e '.ok == true and .result.type == "session" and .sequence == 0' \
    "$data_root/reopened.json" >/dev/null
