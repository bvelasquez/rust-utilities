#!/usr/bin/env bash
set -euo pipefail

dev_web_root() {
  local here
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  printf '%s/web' "$here"
}

# pnpm 10 corepack installs can be broken on some machines; fall back to npx.
dev_pnpm() {
  if pnpm --version >/dev/null 2>&1; then
    pnpm "$@"
  else
    npx --yes pnpm@10 "$@"
  fi
}
