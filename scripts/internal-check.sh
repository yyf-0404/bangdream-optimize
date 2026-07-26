#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

bash -n scripts/build-web-wasm.sh
bash -n scripts/server-smoke.sh
bash -n scripts/sync-game-data.sh
bash -n scripts/run-server.sh
bash -n scripts/run-web.sh
bash -n scripts/internal-check.sh
python3 -m py_compile scripts/serve-web.py

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
