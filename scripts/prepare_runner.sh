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
echo "TERVIX_RUNNER_IMAGE=$image" >> "$GITHUB_ENV"
