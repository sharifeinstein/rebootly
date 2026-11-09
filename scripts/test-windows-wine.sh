#!/usr/bin/env bash
# Build for x86_64-pc-windows-gnu (std from source, MinGW linker) and run the
# tests under Wine. Linux host only; needs gcc-mingw-w64-x86-64-posix, wine64
# and the rust-src `library/` described in docs/PROGRESS.md.
# Usage: scripts/test-windows-wine.sh [test|build] [cargo args…]
set -euo pipefail
cmd=${1:-test}; shift || true
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc-posix
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=/usr/lib/wine/wine64
export WINEDEBUG=-all WINEPREFIX=${WINEPREFIX:-/tmp/wineprefix}
[ -d "$WINEPREFIX" ] || /usr/lib/wine/wine64 wineboot -i >/dev/null 2>&1
RUSTC_BOOTSTRAP=1 exec cargo "$cmd" -q -Zbuild-std=std,panic_abort \
  --target x86_64-pc-windows-gnu "$@"
