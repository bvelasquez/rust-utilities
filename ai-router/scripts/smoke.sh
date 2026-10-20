#!/usr/bin/env bash
# Boots airouter against a fake upstream is covered by `cargo test`.
# This script checks the real binary: init, doctor, schema, and /health.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --quiet
BIN="target/debug/airouter"
DIR="$(mktemp -d)"
trap 'rm -rf "$DIR"' EXIT

"$BIN" --config "$DIR/config.json" init >/dev/null
"$BIN" --config "$DIR/config.json" models add openai/gpt-4o-mini >/dev/null
"$BIN" --config "$DIR/config.json" doctor --offline >/dev/null
"$BIN" --config "$DIR/config.json" schema | head -c 20 >/dev/null

"$BIN" --config "$DIR/config.json" serve >/tmp/airouter-smoke.log 2>&1 &
PID=$!
trap 'kill "$PID" 2>/dev/null || true; rm -rf "$DIR"' EXIT
for _ in $(seq 1 50); do
  if curl -sf "http://127.0.0.1:8787/health" >/dev/null; then
    break
  fi
  sleep 0.1
done
curl -sf "http://127.0.0.1:8787/health" | grep -q ok
TOKEN="$(tr -d '[:space:]' < "$DIR/token")"
curl -sf -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:8787/v1/models" | grep -q gpt-4o-mini
echo "smoke ok"
