#!/usr/bin/env bash
# Install pi-commander into ~/.cargo/bin (same convention as soki-ci).
set -euo pipefail
cd "$(dirname "$0")/.."
make install
