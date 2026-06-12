#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

host="${BANGDREAM_OPTIMIZE_WEB_HOST:-127.0.0.1}"
port="${BANGDREAM_OPTIMIZE_WEB_PORT:-8080}"
root="${BANGDREAM_OPTIMIZE_WEB_ROOT:-apps/web}"
game_data_root="${BANGDREAM_OPTIMIZE_GAME_DATA_ROOT:-var/game-data}"

echo "web: http://${host}:${port}"
echo "web root: ${root}"
echo "game data: http://${host}:${port}/game-data -> ${game_data_root}"
echo "backend api: configured by ${root}/config.js"

exec python3 scripts/serve-web.py \
  --host "${host}" \
  --port "${port}" \
  --web-root "${root}" \
  --game-data-root "${game_data_root}"
