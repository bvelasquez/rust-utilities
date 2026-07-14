#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "$0")/_dev-common.sh"

dev_cd_root
dev_cargo test "$@"
