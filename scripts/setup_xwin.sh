#!/usr/bin/env bash
set -euo pipefail

export RUSTUP_TOOLCHAIN="${RUST_VERSION:?Run this script in the official Rust container}"
export XWIN_CACHE_DIR="/opt/tervix-windows-sdk/vs17-44a3e7dc-xwin-0.23.1"
printf 'RUSTUP_TOOLCHAIN=%s\nXWIN_CACHE_DIR=%s\n' "$RUSTUP_TOOLCHAIN" "$XWIN_CACHE_DIR" >> "$GITHUB_ENV"

rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
tools="$RUNNER_TEMP/tervix-tools"
mkdir -p "$tools"
archive="$tools/cargo-xwin.tar.gz"
curl --fail --silent --show-error --location --retry 2 --max-time 60 \
    https://github.com/rust-cross/cargo-xwin/releases/download/v0.23.1/cargo-xwin-v0.23.1.x86_64-unknown-linux-musl.tar.gz \
    --output "$archive"
printf '%s  %s\n' 'c492c6dfb7e5ac0eee586b796e0fc950ba13077e7f0bbb046f445d71790d5360' "$archive" | sha256sum --check
tar --extract --gzip --file "$archive" --directory "$tools" cargo-xwin
printf '%s\n' "$tools" >> "$GITHUB_PATH"
export PATH="$tools:$PATH"

# The runner cannot reach aka.ms. Seed xwin's VS 17 cache with the official
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

cargo xwin cache xwin
