#!/usr/bin/env bash
set -euo pipefail

# Run the Actions step from stdin so only the checkout needs a bind mount.
exec docker run --rm -i --pull=never --user "$(id -u):$(id -g)" \
    --volume "$GITHUB_WORKSPACE:$GITHUB_WORKSPACE" --workdir "$GITHUB_WORKSPACE" \
    --volume "$TERVIX_CARGO_CACHE:/tmp/cargo" \
    --env HOME=/tmp --env CARGO_HOME=/tmp/cargo \
    --env GITHUB_WORKSPACE --env GITHUB_SHA --env RELEASE_VERSION --env GH_REPO --env GH_TOKEN \
    "$TERVIX_RUNNER_IMAGE" bash -e -o pipefail -s < "$1"
