#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' apps/desktop/src-tauri/tauri.conf.json | head -n 1)"
if [[ -z "${version}" ]]; then
  version="0.0.0"
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) platform_name="linux-x64" ;;
  Linux-aarch64|Linux-arm64) platform_name="linux-arm64" ;;
  Darwin-x86_64) platform_name="macos-x64" ;;
  Darwin-arm64) platform_name="macos-arm64" ;;
  MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) platform_name="windows-x64" ;;
  *) platform_name="$(uname -s)-$(uname -m)" ;;
esac
platform_name="$(printf '%s' "${platform_name}" | tr -cs '[:alnum:]' '-' | sed 's/^-//; s/-$//' | tr '[:upper:]' '[:lower:]')"

if [[ "${BANGDREAM_OPTIMIZE_DESKTOP_BUNDLE:-0}" == "1" ]]; then
  if ! cargo tauri --version >/dev/null 2>&1; then
    echo "cargo-tauri is not installed. Install it with: cargo install tauri-cli --version '^2'." >&2
    exit 1
  fi
  cargo tauri build --config apps/desktop/src-tauri/tauri.conf.json
else
  cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --release
  binary_path="apps/desktop/src-tauri/target/release/bangdream-optimize-desktop-app"
  package_path="apps/desktop/src-tauri/target/release/bangdream-optimize-desktop-v${version}-${platform_name}"
  cp -f "${binary_path}" "${package_path}"
  echo "desktop binary: ${binary_path}"
  echo "desktop package: ${package_path}"
  echo "set BANGDREAM_OPTIMIZE_DESKTOP_BUNDLE=1 to run cargo-tauri bundle build."
fi
