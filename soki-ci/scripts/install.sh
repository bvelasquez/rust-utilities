#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/_dev-common.sh"
dev_cd_root
"$(dirname "$0")/web-build.sh"
dev_cargo install --path . --force
