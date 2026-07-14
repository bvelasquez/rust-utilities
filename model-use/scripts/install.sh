#!/usr/bin/env bash
# Build and install model-use to ~/.cargo/bin (replaces any existing binary).
set -euo pipefail

source "$(dirname "$0")/_dev-common.sh"

dev_cd_root
version="$(awk -F\" '/^version = / { print $2; exit }' Cargo.toml)"
echo "Installing model-use v${version} to ~/.cargo/bin..."
dev_cargo install --path . --force
echo ""
if command -v model-use >/dev/null 2>&1; then
    echo "Installed: $(command -v model-use)"
    model-use --version 2>/dev/null || true
else
    echo "Installed. Ensure ~/.cargo/bin is on your PATH."
fi
