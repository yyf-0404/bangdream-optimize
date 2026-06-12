#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

target="${BANGDREAM_OPTIMIZE_DESKTOP_WINDOWS_TARGET:-x86_64-pc-windows-msvc}"
binary_name="bangdream-optimize-desktop-app"
package_name="bangdream-optimize-desktop"
version="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' apps/desktop/src-tauri/tauri.conf.json | head -n 1)"
if [[ -z "${version}" ]]; then
  version="0.0.0"
fi
case "${target}" in
  x86_64-pc-windows-msvc) platform_name="windows-x64" ;;
  aarch64-pc-windows-msvc) platform_name="windows-arm64" ;;
  i686-pc-windows-msvc) platform_name="windows-x86" ;;
  *) platform_name="$(printf '%s' "${target}" | tr -cs '[:alnum:]' '-' | sed 's/^-//; s/-$//' | tr '[:upper:]' '[:lower:]')" ;;
esac
ext=""

if [[ "${target}" == x86_64-pc-windows-* ]]; then
  ext=".exe"
fi

if command -v rustup >/dev/null 2>&1; then
  if ! rustup target list --installed | grep -qx "${target}"; then
    echo "installing Rust target: ${target}"
    rustup target add "${target}"
  fi
else
  echo "rustup not found; assuming Rust target ${target} is already installed." >&2
fi

echo "packaging desktop binary for windows target: ${target}"
echo "output binary: apps/desktop/src-tauri/target/${target}/release/${binary_name}${ext}"
echo "package binary: apps/desktop/src-tauri/target/${target}/release/${package_name}-v${version}-${platform_name}${ext}"

cargo build \
  --manifest-path apps/desktop/src-tauri/Cargo.toml \
  --release \
  --target "${target}"

binary_path="apps/desktop/src-tauri/target/${target}/release/${binary_name}${ext}"
package_path="apps/desktop/src-tauri/target/${target}/release/${package_name}-v${version}-${platform_name}${ext}"
if [[ ! -f "${binary_path}" ]]; then
  echo "packaging failed: expected binary not found at ${binary_path}" >&2
  exit 1
fi

cp -f "${binary_path}" "${package_path}"

echo "desktop binary ready: ${binary_path}"
echo "desktop package ready: ${package_path}"
