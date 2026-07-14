#!/usr/bin/env bash
# Publish model-use to crates.io (requires `cargo login` first).
set -euo pipefail

source "$(dirname "$0")/_dev-common.sh"

dev_cd_root
version="$(awk -F\" '/^version = / { print $2; exit }' Cargo.toml)"
echo "Publishing model-use v${version} to crates.io..."
cargo publish "$@"
