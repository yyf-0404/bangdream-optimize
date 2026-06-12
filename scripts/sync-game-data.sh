#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

out="${BANGDREAM_OPTIMIZE_GAME_DATA_ROOT:-var/game-data}"
base_url="${BANGDREAM_OPTIMIZE_BESTDORI_BASE_URL:-https://bestdori.com}"
repair_dir="${BANGDREAM_OPTIMIZE_REPAIR_DIR:-tsugu-bangdream-bot/backend/config}"

args=(--out "${out}" --base-url "${base_url}")

if [[ -d "${repair_dir}" ]]; then
  args+=(--repair-dir "${repair_dir}")
fi

if [[ "$#" -eq 0 ]]; then
  args+=(--all-event-details --all-charts --all-card-details)
  echo "syncing all game data into ${out}"
  echo "pass additional sync-bestdori options after this script, for example: --event 287 --chart 232:expert"
else
  args+=("$@")
fi

cargo run -p bangdream-optimize-sync-bestdori -- "${args[@]}"
