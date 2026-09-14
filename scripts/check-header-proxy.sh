#!/usr/bin/env bash
# Read-only check through the public virtual host, not just the backend health route.
set -Eeuo pipefail
if [[ $# -lt 1 || $# -gt 2 || ! "$1" =~ ^https?:// ]]; then
  echo 'Usage: bash scripts/check-header-proxy.sh https://your-domain [assets/.../image.png]' >&2
  exit 2
fi
origin="${1%/}"
asset="${2:-assets/tw/characters/resourceset/res018083_rip/card_normal.png}"
url="$origin/bestdori/header/$asset"
body="$(mktemp)"
trap 'rm -f -- "$body"' EXIT
if ! response="$(curl --silent --show-error --location --max-time 45 --max-filesize 4194304 --output "$body" --write-out '%{http_code} %{content_type}' "$url")"; then
  echo "Header proxy request failed: $url" >&2
  exit 1
fi
if [[ "$response" != '200 image/png'* ]]; then
  echo "Header proxy failed: $response ($url)" >&2
  echo 'HTML usually means the request fell through to the SPA. Add /bestdori/header/ to the active Nginx server block; see docs/nginx-reverse-proxy.conf.' >&2
  exit 1
fi
signature="$(od -An -tx1 -N8 "$body" | tr -d ' \n\r')"
if [[ "$signature" != '89504e470d0a1a0a' ]]; then
  echo 'Header proxy response is not a valid PNG signature.' >&2
  exit 1
fi
echo "Header proxy OK: PNG received via $url"
