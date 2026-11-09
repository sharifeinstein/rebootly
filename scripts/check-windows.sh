#!/usr/bin/env bash
# Type-check (and clippy) the workspace for Windows on a Linux host that
# can't download the Windows rust-std (e.g. the Claude cloud workspace).
# Needs the Rust `library/` source for the exact rustc commit at
#   $(rustc --print sysroot)/lib/rustlib/src/rust/library
# (see docs/PROGRESS.md "Windows type-check"). Checks only: nothing links or runs.
set -euo pipefail
cmd=${1:-check}; shift || true
target=${TARGET:-x86_64-pc-windows-msvc}
extra=()
[ "$cmd" = clippy ] && extra=(-- -D warnings)
RUSTC_BOOTSTRAP=1 exec cargo "$cmd" -q -Zbuild-std=std,panic_abort \
  --target "$target" --workspace --all-targets "$@" "${extra[@]}"
