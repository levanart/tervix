#!/usr/bin/env bash
set -euo pipefail

export RUSTUP_TOOLCHAIN="${RUST_VERSION:?Run this script in the official Rust container}"
export XWIN_CACHE_DIR="${RUNNER_TEMP:?GitHub Actions runner temp directory is required}/cargo-xwin"
printf 'RUSTUP_TOOLCHAIN=%s\nXWIN_CACHE_DIR=%s\n' "$RUSTUP_TOOLCHAIN" "$XWIN_CACHE_DIR" >> "$GITHUB_ENV"

rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
cargo install --locked cargo-xwin --version 0.23.0

# The runner cannot reach aka.ms. Seed xwin 0.9's VS 17 cache with the official
# channel manifest at the Microsoft CDN URL that aka.ms redirects to.
manifest_url='https://download.visualstudio.microsoft.com/download/pr/95995f57-8355-4ff5-8d90-a7e1669fb3b3/a788dfa3e6f50a33f7dee8396fd40d2946c69a776c0cfa5208f56d461fd1a825/VisualStudio.17.Release.chman'
manifest_sha='44a3e7dc44486c945abdc1fae97e7e22de4d05e80f03b00e9302ba4b72c3e0e3'
cache="$XWIN_CACHE_DIR/xwin/dl"
mkdir -p "$cache"
manifest="$(mktemp "$cache/manifest.XXXXXX")"
trap 'rm -f -- "$manifest"' EXIT
curl --fail --silent --show-error --location --retry 2 --max-time 60 "$manifest_url" --output "$manifest"
printf '%s  %s\n' "$manifest_sha" "$manifest" | sha256sum --check
mv -- "$manifest" "$cache/manifest_17.json"
