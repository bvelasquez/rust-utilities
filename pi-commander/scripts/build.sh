#!/usr/bin/env bash
# Build pi-commander and print status.
set -euo pipefail
cd "$(dirname "$0")/.."
echo "building pi-commander (release)…"
cargo build --release
echo "built: target/release/pi-commander"
