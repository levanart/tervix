#!/usr/bin/env bash
set -euo pipefail

target=x86_64-pc-windows-gnu
exe="target/$target/release/tervix.exe"
version=$1
cargo build --release --locked -p tervix --target "$target"
# A standalone download must not depend on the MinGW runtime being installed.
imports=$(x86_64-w64-mingw32-objdump -p "$exe")
if grep -Eiq 'DLL Name:.*(libgcc|libstdc\+\+|libwinpthread)' <<< "$imports"; then
    echo "Unexpected MinGW runtime dependency in tervix.exe" >&2
    exit 1
fi
name="tervix-v$version-windows-x64"
mkdir -p dist
install -m755 "$exe" "dist/$name.exe"
makensis -V2 -WX -NOCD "-DVERSION=$version" "-DPAYLOAD=$exe" \
    "-DOUTPUT=dist/$name-Setup.exe" packaging/windows/tervix.nsi
