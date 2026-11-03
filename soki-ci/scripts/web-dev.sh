#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/_web-common.sh"
cd "$(dev_web_root)"
dev_pnpm install 2>/dev/null || true
dev_pnpm run dev
