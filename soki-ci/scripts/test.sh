#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/_dev-common.sh"
source "$(dirname "$0")/_web-common.sh"
dev_cd_root
dev_cargo test
cd "$(dev_web_root)"
dev_pnpm install --frozen-lockfile 2>/dev/null || dev_pnpm install
dev_pnpm run typecheck
