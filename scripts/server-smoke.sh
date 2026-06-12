#!/usr/bin/env bash
set -euo pipefail

base_url="${BANGDREAM_OPTIMIZE_BASE_URL:-http://127.0.0.1:3100}"

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is not installed." >&2
  exit 1
fi

echo "checking ${base_url}/health"
curl --fail --silent --show-error "${base_url}/health"
echo

if [[ -z "${BANGDREAM_OPTIMIZE_PLAYER_ID:-}" ]]; then
  echo "BANGDREAM_OPTIMIZE_PLAYER_ID is not set; skipping calc-result smoke."
  exit 0
fi

server="${BANGDREAM_OPTIMIZE_SERVER:-jp}"
player_id="${BANGDREAM_OPTIMIZE_PLAYER_ID}"

if [[ -n "${BANGDREAM_OPTIMIZE_EVENT_ID:-}" ]]; then
  payload=$(
    printf '{"playerId":%s,"server":"%s","eventId":%s}' \
      "${player_id}" \
      "${server}" \
      "${BANGDREAM_OPTIMIZE_EVENT_ID}"
  )
else
  payload=$(
    printf '{"playerId":%s,"server":"%s"}' \
      "${player_id}" \
      "${server}"
  )
fi

echo "checking ${base_url}/v1/calc-result"
curl \
  --fail \
  --silent \
  --show-error \
  --header "content-type: application/json" \
  --data "${payload}" \
  "${base_url}/v1/calc-result"
echo
