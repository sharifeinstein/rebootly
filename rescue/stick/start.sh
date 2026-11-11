#!/bin/sh
# Rebootly stick launcher for Linux (the Rebootly rescue system runs it at
# boot; on another live Linux run it yourself: sh /media/<you>/Ventoy/start.sh).
# Picks the binary for this CPU, then opens the text screens with the
# stick's packs. Extra arguments go to `rebootly-cli tui`.
set -u

here=$(cd "$(dirname "$0")" && pwd)

case $(uname -m) in
    x86_64 | amd64) arch=x64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *)
        echo "Rebootly has no build for this CPU ($(uname -m))." >&2
        exit 1
        ;;
esac

bin="$here/bin/linux-$arch/rebootly-cli"
if [ ! -f "$bin" ]; then
    echo "This stick has no Rebootly for Linux on $arch ($bin is missing)." >&2
    exit 1
fi

# Reading disks and unlocking BitLocker need root.
if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        exec sudo sh "$0" "$@"
    fi
    echo "Rebootly needs root: run this as root (su -c 'sh $0')." >&2
    exit 1
fi

# exFAT and some mounts don't keep the executable bit, or are mounted
# noexec: run a copy from memory.
run=$bin
if [ ! -x "$bin" ] || ! "$bin" --version >/dev/null 2>&1; then
    tmp=$(mktemp -d /tmp/rebootly.XXXXXX) || exit 1
    cp "$bin" "$tmp/rebootly-cli" && chmod 755 "$tmp/rebootly-cli" || exit 1
    run=$tmp/rebootly-cli
fi

# Started by the rescue system: it flushes and unmounts afterwards (this
# script is on the stick, so the stick can't be unmounted from here).
if [ -n "${REBOOTLY_RESCUE:-}" ]; then
    exec "$run" tui --packs-dir "$here/packs" "$@"
fi
"$run" tui --packs-dir "$here/packs" "$@"
rc=$?
# Flush and unmount what Rebootly mounted (targets and read-only sources).
"$run" close >/dev/null 2>&1 || true
exit $rc
