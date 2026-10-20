#!/usr/bin/env bash
# Static-ish Linux build for copying onto a VM.
# Needs the musl target and a musl C toolchain (musl-gcc) because rusqlite bundles SQLite.
set -euo pipefail
cd "$(dirname "$0")/.."

case "$(uname -m)" in
  x86_64) TARGET="x86_64-unknown-linux-musl" ;;
  aarch64|arm64) TARGET="aarch64-unknown-linux-musl" ;;
  *)
    echo "unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

if ! rustup target list --installed | grep -q "^${TARGET}$"; then
  rustup target add "$TARGET"
fi

if ! command -v musl-gcc >/dev/null 2>&1; then
  echo "musl-gcc is not installed. On Debian/Ubuntu: sudo apt install musl-tools" >&2
  exit 1
fi

export "CC_${TARGET//-/_}=musl-gcc"
cargo build --release --target "$TARGET"
echo "binary: target/${TARGET}/release/airouter"
