#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

export BANGDREAM_OPTIMIZE_HOST="${BANGDREAM_OPTIMIZE_HOST:-127.0.0.1}"
export BANGDREAM_OPTIMIZE_PORT="${BANGDREAM_OPTIMIZE_PORT:-3100}"
export BANGDREAM_OPTIMIZE_GAME_DATA_ROOT="${BANGDREAM_OPTIMIZE_GAME_DATA_ROOT:-var/game-data}"
export BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED="${BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED:-1}"
export BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_INTERVAL_SECONDS="${BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_INTERVAL_SECONDS:-3600}"
export BANGDREAM_OPTIMIZE_TELEMETRY_JSONL="${BANGDREAM_OPTIMIZE_TELEMETRY_JSONL:-var/telemetry/internal.jsonl}"
export RUST_LOG="${RUST_LOG:-info}"

profile="${BANGDREAM_OPTIMIZE_CARGO_PROFILE:-release}"
cmd=(cargo run -p bangdream-optimize-server --bin bangdream-optimize-server)

if [[ "${profile}" == "release" ]]; then
  cmd+=(--release)
elif [[ "${profile}" != "dev" ]]; then
  echo "unsupported BANGDREAM_OPTIMIZE_CARGO_PROFILE=${profile}; use dev or release" >&2
  exit 1
fi
if (( $# > 0 )); then
  cmd+=(-- "$@")
fi

echo "server: http://${BANGDREAM_OPTIMIZE_HOST}:${BANGDREAM_OPTIMIZE_PORT}"
if [[ -n "${BANGDREAM_OPTIMIZE_WEB_ROOT:-}" ]]; then
  echo "web root: ${BANGDREAM_OPTIMIZE_WEB_ROOT}"
else
  echo "web root: disabled; run ./scripts/run-web.sh for the web UI"
fi
echo "game data: ${BANGDREAM_OPTIMIZE_GAME_DATA_ROOT}"
echo "game data sync: ${BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED}"
echo "game data sync interval: ${BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_INTERVAL_SECONDS}s"
echo "telemetry: ${BANGDREAM_OPTIMIZE_TELEMETRY_JSONL}"

exec "${cmd[@]}"
