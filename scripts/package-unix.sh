#!/usr/bin/env bash
set -euo pipefail

target="${1:?target is required}"
platform="${2:?platform is required}"
version="${3:?version is required}"
bundle_neovim="${4:-true}"
neovim_version="${5:-stable}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pkg="${root}/dist/package/lazyvim-${version}-${platform}"
out="${root}/dist/lazyvim-${platform}.tar.gz"

rm -rf "$pkg" "$out"
mkdir -p "$pkg"
cp "${root}/target/${target}/release/lazyvim" "$pkg/lazyvim"
cp "${root}/README.md" "$pkg/README.md"
cp "${root}/LICENSE" "$pkg/LICENSE"
chmod +x "$pkg/lazyvim"

if [[ "$bundle_neovim" == "true" ]]; then
  case "$platform" in
    linux-x86_64) nvim_asset="nvim-linux-x86_64.tar.gz"; nvim_dir="nvim-linux-x86_64" ;;
    macos-x86_64) nvim_asset="nvim-macos-x86_64.tar.gz"; nvim_dir="nvim-macos-x86_64" ;;
    macos-arm64) nvim_asset="nvim-macos-arm64.tar.gz"; nvim_dir="nvim-macos-arm64" ;;
    *) echo "Unsupported bundled Neovim platform: $platform" >&2; exit 2 ;;
  esac

  tmp="${root}/dist/neovim-${platform}"
  rm -rf "$tmp"
  mkdir -p "$tmp"
  curl -fL "https://github.com/neovim/neovim/releases/download/${neovim_version}/${nvim_asset}" -o "$tmp/${nvim_asset}"
  tar -xzf "$tmp/${nvim_asset}" -C "$tmp"
  mv "$tmp/${nvim_dir}" "$pkg/nvim"
fi

tar -C "${root}/dist/package" -czf "$out" "$(basename "$pkg")"
