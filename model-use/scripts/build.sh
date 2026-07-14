#!/usr/bin/env bash
# Build the release binary (target/release/model-use).
set -euo pipefail

source "$(dirname "$0")/_dev-common.sh"

dev_cd_root
version="$(awk -F\" '/^version = / { print $2; exit }' Cargo.toml)"
echo "Building model-use v${version} (release)..."
dev_cargo build --release
echo ""
echo "Built: $(_dev_root)/target/release/model-use"
"$(_dev_root)/target/release/model-use" --version 2>/dev/null || true
