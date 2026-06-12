#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

DEPLOY_WEB_ROOT="${BANGDREAM_OPTIMIZE_WEB_ROOT:-/var/www/bangdream-optimize/web}"
WEB_SOURCE_DIR="${BANGDREAM_OPTIMIZE_WEB_SOURCE_DIR:-apps/web}"
NO_DEPLOY=0

usage() {
  cat <<'USAGE'
Usage: ./scripts/build-web-assets.sh [--no-deploy]

Build bangdream-optimize web assets (including WASM), then deploy the web directory.

Options:
  --no-deploy    Skip deploying to BANGDREAM_OPTIMIZE_WEB_ROOT.
  -h, --help     Show this help message.

Environment variables:
  BANGDREAM_OPTIMIZE_WEB_ROOT         Destination web root (default: /var/www/bangdream-optimize/web).
  BANGDREAM_OPTIMIZE_WEB_SOURCE_DIR    Source web dir to deploy (default: apps/web).
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-deploy)
      NO_DEPLOY=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "wasm-bindgen is not installed." >&2
  echo "Install it with: cargo install wasm-bindgen-cli" >&2
  exit 1
fi

cargo build -p bangdream-optimize-web-wasm --target wasm32-unknown-unknown --release

mkdir -p apps/web/pkg
wasm-bindgen \
  target/wasm32-unknown-unknown/release/bangdream_optimize_web_wasm.wasm \
  --target web \
  --out-dir apps/web/pkg \
  --out-name bangdream_optimize_web_wasm

if [[ "$NO_DEPLOY" -eq 1 ]]; then
  echo "build completed; skip deploy (--no-deploy enabled)"
else
  mkdir -p "$DEPLOY_WEB_ROOT"
  if command -v rsync >/dev/null 2>&1; then
    rsync -a --delete "$WEB_SOURCE_DIR/" "$DEPLOY_WEB_ROOT/"
  else
    cp -a "$WEB_SOURCE_DIR/." "$DEPLOY_WEB_ROOT/"
    echo "warning: rsync not found; using cp fallback without automatic deletion of removed files" >&2
  fi
  echo "deployed $WEB_SOURCE_DIR to $DEPLOY_WEB_ROOT"
fi
