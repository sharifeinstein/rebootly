#!/bin/sh
# Runs build.sh in an Alpine container: from WSL, Linux or macOS with Docker.
#   sh rescue/alpine/build-in-docker.sh [--release 3.22] [--out dist]
# The Rust toolchain and crate downloads are kept in Docker volumes
# (rebootly-cargo, rebootly-rustup), so the second build is faster.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
release=${ALPINE_RELEASE:-3.22}
# The container's Alpine release follows build.sh's --release.
prev=
for a in "$@"; do
    [ "$prev" = --release ] && release=$a
    prev=$a
done

docker run --rm -t \
    -v "$repo:/src" -w /src \
    -v rebootly-cargo:/root/.cargo -v rebootly-rustup:/root/.rustup \
    -e ALPINE_RELEASE="$release" -e ALPINE_MIRROR \
    "alpine:$release" sh rescue/alpine/build.sh "$@"

# The container ran as root: give the results back to the caller.
docker run --rm -v "$repo:/src" "alpine:$release" \
    chown -R "$(id -u):$(id -g)" /src/dist /src/target/alpine 2>/dev/null || true
