#!/usr/bin/env bash
# Start the example hosted OG Paper under `wrangler dev` (local, throwaway
# state) and run the launch-audit probes against it. See docs/LAUNCH-AUDIT.md.
set -euo pipefail
cd "$(dirname "$0")/../example"
export WRANGLER_SEND_METRICS=false
STATE=$(mktemp -d)
# A free port (something else may hold 8799), wrangler in its own process group.
PORT="${PORT:-$(node -e 'const s=require("net").createServer().listen(0,()=>{console.log(s.address().port);s.close()})')}"
npx -y wrangler@4 d1 migrations apply DB --local --persist-to "$STATE" >/dev/null
setsid npx -y wrangler@4 dev --port "$PORT" --persist-to "$STATE" >"$STATE/dev.log" 2>&1 &
DEV=$!
trap 'kill -- -$DEV 2>/dev/null; sleep 1; rm -rf "$STATE"' EXIT
for _ in $(seq 1 90); do curl -sf "http://localhost:$PORT/paper/api/me" >/dev/null && break; sleep 1; done
BASE="http://localhost:$PORT" node ../test/audit-probe.mjs
