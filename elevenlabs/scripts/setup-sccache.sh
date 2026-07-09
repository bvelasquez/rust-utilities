#!/usr/bin/env bash
# Install and verify sccache for faster Rust rebuilds.
set -euo pipefail

source "$(dirname "$0")/_dev-common.sh"

if ! command -v brew >/dev/null 2>&1; then
    echo "error: Homebrew required — install from https://brew.sh" >&2
    exit 1
fi

if ! command -v sccache >/dev/null 2>&1; then
    echo "Installing sccache via Homebrew..."
    brew install sccache
else
    echo "sccache already installed: $(sccache --version)"
fi

if ! command -v ld64.lld >/dev/null 2>&1; then
    echo "Installing lld linker via Homebrew (faster dev linking)..."
    brew install lld
else
    echo "lld already installed: $(ld64.lld --version | head -1)"
fi

dev_enable_sccache
echo "RUSTC_WRAPPER=${RUSTC_WRAPPER:-<unset>}"

echo ""
echo "Running a quick compile check to warm the cache..."
dev_cargo build -q

echo ""
sccache --show-stats 2>/dev/null || true
echo ""
echo "sccache is ready. Use dev_cargo or export RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0"
echo "  .cargo/config.toml — uses Homebrew ld64.lld for faster linking"
