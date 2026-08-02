#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPOSITORY_ROOT"

REMOTE="${BANGDREAM_OPTIMIZE_UPDATE_REMOTE:-origin}"
BRANCH="${BANGDREAM_OPTIMIZE_UPDATE_BRANCH:-master}"
WEB_ROOT="${BANGDREAM_OPTIMIZE_WEB_ROOT:-/var/www/bangdream-optimize/web}"
BACKEND_SERVICE="${BANGDREAM_OPTIMIZE_BACKEND_SERVICE:-bangdream-optimize-backend}"
NGINX_SERVICE="${BANGDREAM_OPTIMIZE_NGINX_SERVICE:-nginx}"
HEALTH_URL="${BANGDREAM_OPTIMIZE_HEALTH_URL:-http://127.0.0.1:3100/health}"
HEALTH_TIMEOUT="${BANGDREAM_OPTIMIZE_HEALTH_TIMEOUT_SECONDS:-600}"
ASSUME_YES=0

usage() {
  cat <<'USAGE'
Usage: bash /path/to/repository/scripts/update-production.sh [options]

The repository root is resolved from this script's own location. Force-sync
that checkout, rebuild the backend and Web/WASM assets,
restart the backend, deploy the frontend, and restart Nginx.

WARNING: tracked local changes and untracked, non-ignored files are deleted.
Ignored runtime data and secrets are retained.

Options:
  -y, --yes                 Skip the destructive-operation confirmation.
  --remote <name>           Git remote (default: origin).
  --branch <name>           Remote branch (default: master).
  --web-root <path>         Frontend destination.
  --backend-service <name>  systemd backend unit.
  --nginx-service <name>    systemd Nginx unit.
  --health-url <url>        Backend health-check URL.
  --health-timeout <sec>    Health-check timeout (default: 600).
  -h, --help                Show this help message.

The same settings can be supplied through these environment variables:
  BANGDREAM_OPTIMIZE_UPDATE_REMOTE
  BANGDREAM_OPTIMIZE_UPDATE_BRANCH
  BANGDREAM_OPTIMIZE_WEB_ROOT
  BANGDREAM_OPTIMIZE_BACKEND_SERVICE
  BANGDREAM_OPTIMIZE_NGINX_SERVICE
  BANGDREAM_OPTIMIZE_HEALTH_URL
  BANGDREAM_OPTIMIZE_HEALTH_TIMEOUT_SECONDS
USAGE
}

log() {
  printf '\n[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*"
}

die() {
  echo "error: $*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

on_error() {
  local status=$?
  echo "update failed at line ${BASH_LINENO[0]}: ${BASH_COMMAND}" >&2
  exit "$status"
}
trap on_error ERR

while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes)
      ASSUME_YES=1
      shift
      ;;
    --remote)
      [[ $# -ge 2 ]] || die "--remote requires a value"
      REMOTE="$2"
      shift 2
      ;;
    --branch)
      [[ $# -ge 2 ]] || die "--branch requires a value"
      BRANCH="$2"
      shift 2
      ;;
    --web-root)
      [[ $# -ge 2 ]] || die "--web-root requires a value"
      WEB_ROOT="$2"
      shift 2
      ;;
    --backend-service)
      [[ $# -ge 2 ]] || die "--backend-service requires a value"
      BACKEND_SERVICE="$2"
      shift 2
      ;;
    --nginx-service)
      [[ $# -ge 2 ]] || die "--nginx-service requires a value"
      NGINX_SERVICE="$2"
      shift 2
      ;;
    --health-url)
      [[ $# -ge 2 ]] || die "--health-url requires a value"
      HEALTH_URL="$2"
      shift 2
      ;;
    --health-timeout)
      [[ $# -ge 2 ]] || die "--health-timeout requires a value"
      HEALTH_TIMEOUT="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

[[ "$REMOTE" =~ ^[A-Za-z0-9._-]+$ ]] || die "invalid Git remote name: $REMOTE"
git check-ref-format "refs/heads/$BRANCH" >/dev/null || die "invalid Git branch name: $BRANCH"
[[ "$BACKEND_SERVICE" =~ ^[A-Za-z0-9_.@-]+$ ]] || die "invalid backend service name"
[[ "$NGINX_SERVICE" =~ ^[A-Za-z0-9_.@-]+$ ]] || die "invalid Nginx service name"
[[ "$HEALTH_TIMEOUT" =~ ^[1-9][0-9]*$ ]] || die "health timeout must be a positive integer"
[[ "$WEB_ROOT" == /* ]] || die "web root must be an absolute path"

require_command git
require_command cargo
require_command wasm-bindgen
require_command rsync
require_command curl
require_command systemctl
require_command nginx

if [[ $EUID -eq 0 ]]; then
  SUDO=()
else
  require_command sudo
  SUDO=(sudo)
fi

[[ -f Cargo.toml && -f scripts/build-web-assets.sh ]] || \
  die "run this script from the bangdream-optimize repository"
git remote get-url "$REMOTE" >/dev/null 2>&1 || die "Git remote does not exist: $REMOTE"

echo "Repository:      $(pwd)"
echo "Remote branch:  $REMOTE/$BRANCH"
echo "Web root:       $WEB_ROOT"
echo "Backend unit:   $BACKEND_SERVICE"
echo "Nginx unit:     $NGINX_SERVICE"
echo "Health URL:     $HEALTH_URL"
echo
echo "Tracked local changes and untracked, non-ignored files will be deleted."

if [[ "$ASSUME_YES" -ne 1 ]]; then
  if [[ ! -t 0 ]]; then
    die "confirmation requires a terminal; pass --yes for unattended updates"
  fi
  read -r -p "Continue? [y/N] " answer
  [[ "$answer" == "y" || "$answer" == "Y" ]] || die "update cancelled"
fi

old_commit="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"

log "Fetching $REMOTE and force-syncing $BRANCH"
git fetch --prune "$REMOTE"
remote_ref="refs/remotes/$REMOTE/$BRANCH"
git show-ref --verify --quiet "$remote_ref" || die "remote branch not found: $REMOTE/$BRANCH"
git reset --hard "$remote_ref"
git clean -fd
new_commit="$(git rev-parse --short HEAD)"
echo "Source updated: $old_commit -> $new_commit"

log "Building release backend and game-data sync executable"
cargo build --locked --release \
  -p bangdream-optimize-server \
  -p bangdream-optimize-sync-bestdori

log "Building Web/WASM assets"
bash scripts/build-web-assets.sh --no-deploy

log "Restarting backend service"
"${SUDO[@]}" systemctl restart "$BACKEND_SERVICE"

log "Waiting for backend health check"
deadline=$((SECONDS + HEALTH_TIMEOUT))
until curl --fail --silent --show-error --max-time 5 "$HEALTH_URL" >/dev/null 2>&1; do
  if (( SECONDS >= deadline )); then
    "${SUDO[@]}" systemctl --no-pager --full status "$BACKEND_SERVICE" || true
    "${SUDO[@]}" journalctl --no-pager -u "$BACKEND_SERVICE" -n 50 || true
    die "backend did not become healthy within ${HEALTH_TIMEOUT}s"
  fi
  sleep 2
done

log "Deploying frontend to $WEB_ROOT"
"${SUDO[@]}" install -d -m 0755 "$WEB_ROOT"
"${SUDO[@]}" rsync -a --delete apps/web/ "$WEB_ROOT/"

log "Validating Nginx configuration"
"${SUDO[@]}" nginx -t

log "Restarting Nginx"
"${SUDO[@]}" systemctl restart "$NGINX_SERVICE"
"${SUDO[@]}" systemctl is-active --quiet "$BACKEND_SERVICE"
"${SUDO[@]}" systemctl is-active --quiet "$NGINX_SERVICE"

log "Update completed at commit $new_commit"
echo "Backend: $HEALTH_URL"
echo "Frontend: $WEB_ROOT"
