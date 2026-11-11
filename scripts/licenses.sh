#!/usr/bin/env bash
# Regenerates THIRD-PARTY-LICENSES.html (shipped in the release zip) for
# the Windows build. Fails if a dependency has a licence that isn't in
# about.toml's `accepted` list. Run after adding or updating dependencies.
# Needs: cargo install cargo-about --locked --features cli
set -euo pipefail
cd "$(dirname "$0")/.."
cargo about generate --workspace --offline --fail about.hbs -o THIRD-PARTY-LICENSES.html
echo "THIRD-PARTY-LICENSES.html updated"
