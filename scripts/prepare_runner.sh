#!/usr/bin/env bash
set -euo pipefail

case "$1" in
    test|release) stage=$1 ;;
    *) exit 2 ;;
esac

recipe=scripts/runner.Dockerfile
digest=$(sha256sum "$recipe")
image="tervix-runner-$stage:${digest%% *}"
if docker image inspect "$image" >/dev/null 2>&1; then
    echo "Using cached $image"
else
    base=rust:bookworm
    if ! docker image inspect "$base" >/dev/null 2>&1; then
        base=rust:slim-bookworm
        if ! docker image inspect "$base" >/dev/null 2>&1; then
            echo "Downloading $base (cold runner cache)"
            timeout --kill-after=10s 5m docker pull "$base"
        fi
    fi
    echo "Preparing $stage dependencies from $base"
    docker build --pull=false --build-arg "BASE_IMAGE=$base" --target "$stage" \
        --tag "$image" - < "$recipe"
fi
echo "Checking installed $stage tools"
docker run --rm --pull=never --user "$(id -u):$(id -g)" --env HOME=/tmp \
    "$image" bash -euc '
        export RUSTUP_TOOLCHAIN="$RUST_VERSION"
        rustc --version
        cargo --version
        cc --version >/dev/null
        git --version
        python3 -c "import tomllib"
        pkg-config --modversion xkbcommon
        if [[ "$1" == release ]]; then
            gh --version
            dpkg-deb --version
            dpkg-shlibdeps --version
        else
            for package in fonts-dejavu-core libvulkan1 mesa-vulkan-drivers; do
                dpkg-query -W -f="\${Status}" "$package" | grep -qx "install ok installed"
            done
        fi
    ' bash "$stage"
echo "TERVIX_RUNNER_IMAGE=$image" >> "$GITHUB_ENV"
cache="${XDG_CACHE_HOME:-$HOME/.cache}/tervix-actions/$stage"
mkdir -p "$cache"
echo "TERVIX_CARGO_CACHE=$cache" >> "$GITHUB_ENV"
