#!/usr/bin/env bash
# Spin up an ephemeral Zenithar server on a temp DB, run the e2e suite, tear down.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/backend/target/debug/zenithar-backend"
PORT="${ZEN_TEST_PORT:-3990}"
DATA="$(mktemp -d)"

if [ ! -x "$BIN" ]; then
  echo "building backend…"
  (cd "$ROOT/backend" && cargo build)
fi

echo "starting server on 127.0.0.1:$PORT (temp db)"
# Run inside the temp dir so the bootstrap `.env` lands there (isolated).
( cd "$DATA" && exec env ZENITHAR_BIND="127.0.0.1:$PORT" ZENITHAR_DB="$DATA/zenithar.db" \
  "$BIN" >"$DATA/server.log" 2>&1 ) &
SRV=$!

cleanup() {
  kill "$SRV" 2>/dev/null || true
  wait "$SRV" 2>/dev/null || true
  rm -rf "$DATA"
}
trap cleanup EXIT

# Wait for health.
for _ in $(seq 1 60); do
  if curl -sf "http://127.0.0.1:$PORT/api/health" >/dev/null 2>&1; then break; fi
  sleep 0.1
done
if ! curl -sf "http://127.0.0.1:$PORT/api/health" >/dev/null 2>&1; then
  echo "server did not come up; log:" >&2
  cat "$DATA/server.log" >&2
  exit 1
fi

# The bootstrap admin link is written to .env (in the cwd = temp dir) on first start.
ADMIN_LINK="$(sed -n 's/^ZENITHAR_ADMIN_LINK=//p' "$DATA/.env")"

cd "$ROOT/tests"
# Ensure the browser is present (no-op once cached).
bunx playwright install chromium >/dev/null 2>&1 || true
BASE="http://127.0.0.1:$PORT" ADMIN_LINK="$ADMIN_LINK" bunx playwright test
