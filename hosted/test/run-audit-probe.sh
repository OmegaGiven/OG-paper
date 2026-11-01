#!/usr/bin/env bash
# Start the example hosted OG Paper under `wrangler dev` (local, throwaway
# state) and run the launch-audit probes against it. See docs/LAUNCH-AUDIT.md.
set -euo pipefail
cd "$(dirname "$0")/../example"
export WRANGLER_SEND_METRICS=false
STATE=$(mktemp -d)
npx -y wrangler@4 d1 migrations apply DB --local --persist-to "$STATE" >/dev/null
npx -y wrangler@4 dev --port 8799 --persist-to "$STATE" >"$STATE/dev.log" 2>&1 &
DEV=$!
trap 'kill $DEV 2>/dev/null; rm -rf "$STATE"' EXIT
for _ in $(seq 1 60); do curl -sf http://localhost:8799/paper/api/me >/dev/null && break; sleep 1; done
BASE=http://localhost:8799 node ../test/audit-probe.mjs
