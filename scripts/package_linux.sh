#!/usr/bin/env bash
set -euo pipefail

binary=$1
version=$2
name="tervix-v$version-linux-x86_64"
dist="$PWD/dist"
mkdir "$dist"
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT

install -m755 "$binary" "$dist/$name"
install -Dm755 "$binary" "$work/$name/tervix"
install -m644 LICENSE CHANGELOG.md "$work/$name/"
tar -C "$work" -czf "$dist/$name.tar.gz" "$name"

# Debian sorts prereleases before the final version when separated by '~'.
deb_version=${version/-/\~}
pkg="$work/package"
install -Dm755 "$binary" "$pkg/usr/bin/tervix"
install -Dm644 LICENSE "$pkg/usr/share/doc/tervix/copyright"
mkdir -p "$pkg/DEBIAN" "$work/debian"
cd "$work"
cat > debian/control <<'EOF'
Source: tervix
Maintainer: Tervix maintainers <noreply@github.com>

Package: tervix
Architecture: amd64
Description: Tervix developer workspace
EOF
depends=$(dpkg-shlibdeps -O -e"$pkg/usr/bin/tervix")
cat > "$pkg/DEBIAN/control" <<EOF
Package: tervix
Version: $deb_version
Architecture: amd64
Maintainer: Tervix maintainers <noreply@github.com>
Depends: ${depends#shlibs:Depends=}, libxkbcommon0, libvulkan1
Description: Tervix developer workspace
EOF
deb="$dist/tervix_${deb_version}_amd64.deb"
dpkg-deb --root-owner-group --build "$pkg" "$deb"
dpkg-deb -x "$deb" "$work/check"
"$work/check/usr/bin/tervix" --layout-demo
cd "$dist"
sha256sum -- * > SHA256SUMS.txt
