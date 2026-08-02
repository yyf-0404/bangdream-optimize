#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

bash -n scripts/build-web-assets.sh
bash -n scripts/update-production.sh
bash -n scripts/server-smoke.sh
bash -n scripts/sync-game-data.sh
bash -n scripts/run-server.sh
bash -n scripts/run-web.sh
bash -n scripts/internal-check.sh
if python3 --version >/dev/null 2>&1; then
  PYTHON=(python3)
elif py -3 --version >/dev/null 2>&1; then
  PYTHON=(py -3)
elif python --version >/dev/null 2>&1; then
  PYTHON=(python)
else
  echo "python 3 is required" >&2
  exit 1
fi
"${PYTHON[@]}" -m py_compile scripts/serve-web.py

node --check apps/web/src/main.js
node --check apps/web/src/actions/feedback.js
node --check apps/web/src/data/bangdream-import.js
node --check apps/web/src/data/calculation-errors.js
node --check apps/web/src/data/diagnostics.js
node --check apps/web/src/data/feedback.js
node --check apps/web/src/runtime/browser.js
node --check apps/web/src/runtime/desktop.js
node --check apps/web/config.js
node --test apps/web/test/*.test.js

cargo check --workspace --all-targets
cargo test -p bangdream-optimize-medley-solver
cargo test -p bangdream-optimize-desktop
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
