#!/bin/sh
# Builds the Rebootly rescue ISO (Alpine Linux) and a static Linux build of
# rebootly-cli (SPEC §8). Run as root on Alpine, from the repository:
#
#   sh rescue/alpine/build.sh [--release 3.22] [--out dist]
#
# or from WSL / Linux / macOS with Docker: sh rescue/alpine/build-in-docker.sh
#
# Writes to the output folder (default dist/):
#   rebootly-rescue-<arch>.iso (+ .sha256)  the rescue system, for the stick's iso/
#   rebootly-rescue-<arch>.sources.txt      where the sources of its packages are (GPL)
#   linux-<x64|arm64>/rebootly-cli          for the stick's bin/ (any Linux)
# Needs the internet (Alpine packages, rustup, crates.io, Alpine's aports).
set -eu

release=${ALPINE_RELEASE:-3.22}
mirror=${ALPINE_MIRROR:-https://dl-cdn.alpinelinux.org/alpine}
out=dist

usage() {
    sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
}
die() {
    echo "build.sh: $*" >&2
    exit 1
}
step() {
    printf '\n==> %s\n' "$*"
}

while [ $# -gt 0 ]; do
    case $1 in
        --release) release=$2; shift 2 ;;
        --out) out=$2; shift 2 ;;
        -h | --help) usage; exit 0 ;;
        *) die "unknown option $1 (see --help)" ;;
    esac
done

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
[ -f /etc/alpine-release ] || die "run this on Alpine Linux, or use build-in-docker.sh"
[ "$(id -u)" -eq 0 ] || die "run as root (it installs build packages)"
arch=$(uname -m)
case $arch in
    x86_64) short=x64 ;;
    aarch64) short=arm64 ;;
    *) die "no rescue image for $arch" ;;
esac
mkdir -p "$out"
out=$(cd "$out" && pwd)

step "Build packages"
apk add --no-cache alpine-sdk alpine-conf apk-tools busybox fakeroot syslinux \
    xorriso squashfs-tools mtools dosfstools grub grub-efi git rustup

step "rebootly-cli for Linux $arch (static, musl)"
if [ ! -x "$HOME/.cargo/bin/cargo" ]; then
    rustup-init -y --profile minimal --default-toolchain stable
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
target=$arch-unknown-linux-musl
(
    cd "$repo"
    RUSTFLAGS="-C target-feature=+crt-static" CARGO_TARGET_DIR="$repo/target/alpine" \
        cargo build --release --locked -p rebootly-cli --target "$target"
)
bin=$repo/target/alpine/$target/release/rebootly-cli
if readelf -d "$bin" 2>/dev/null | grep -q NEEDED; then
    die "$bin isn't static (it would only run on Alpine)"
fi
"$bin" --version
mkdir -p "$out/linux-$short"
cp "$bin" "$out/linux-$short/rebootly-cli"

step "Signing key for the image's package index"
if ! ls "$HOME"/.abuild/*.rsa >/dev/null 2>&1; then
    abuild-keygen -a -i -n
fi

work=$(mktemp -d /tmp/rebootly-iso.XXXXXX)
trap 'rm -rf "$work"' EXIT

step "Alpine $release image scripts"
git clone --quiet --depth 1 --branch "$release-stable" --filter=blob:none --sparse \
    https://gitlab.alpinelinux.org/alpine/aports.git "$work/aports"
git -C "$work/aports" sparse-checkout set scripts
cp "$here/mkimg.rebootly.sh" "$here/genapkovl-rebootly.sh" "$work/aports/scripts/"
chmod +x "$work/aports/scripts/genapkovl-rebootly.sh"

step "ISO"
export REBOOTLY_BIN="$bin" REBOOTLY_START="$here/rebootly-start"
(
    cd "$work/aports/scripts"
    sh mkimage.sh --tag "v$release" --outdir "$work/iso" --arch "$arch" \
        --repository "$mirror/v$release/main" \
        --repository "$mirror/v$release/community" \
        --profile rebootly
)
iso=rebootly-rescue-$arch.iso
cp "$work/iso/$iso" "$out/$iso"
(cd "$out" && sha256sum "$iso" > "$iso.sha256")

step "Source notice"
{
    echo "Rebootly rescue ($iso) is Alpine Linux v$release with Rebootly added."
    echo "Rebootly is Apache-2.0 (LICENSE.txt, THIRD-PARTY-LICENSES.html on the stick)."
    echo "Built $(date -u +%Y-%m-%d) from Rebootly $("$bin" --version | head -n 1)."
    echo
    echo "The Alpine packages in it are listed below. Their licences (many GPL) and"
    echo "build recipes are in Alpine's aports repository, branch $release-stable:"
    echo "  https://gitlab.alpinelinux.org/alpine/aports/-/tree/$release-stable"
    echo "and the exact source archives they were built from at:"
    echo "  https://distfiles.alpinelinux.org/distfiles/v$release/"
    echo "Ask the Rebootly maintainer for a copy of any of these sources if those"
    echo "places stop offering them."
    echo
    xorriso -indev "$out/$iso" -find /apks -name '*.apk' 2>/dev/null |
        sed "s|^'||; s|'\$||; s|.*/||; s|\.apk\$||" | sort
} > "$out/rebootly-rescue-$arch.sources.txt"

printf '\nDone:\n'
ls -l "$out/$iso" "$out/linux-$short/rebootly-cli"
