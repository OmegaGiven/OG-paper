#!/usr/bin/env bash
# The native app's account code against a real hosted server: the example site
# under `wrangler dev` (real Durable Objects, D1 and WebSockets, throwaway
# state). Signs in as a browser would, keeps a page, and gets it back on a
# second device. Run it with: forge run --on og-host1 bash tests/account/run-e2e.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT/hosted/example"
export WRANGLER_SEND_METRICS=false
STATE=$(mktemp -d)
PORT="${PORT:-$(node -e 'const s=require("net").createServer().listen(0,()=>{console.log(s.address().port);s.close()})')}"
npx -y wrangler@4 d1 migrations apply DB --local --persist-to "$STATE" >/dev/null
setsid npx -y wrangler@4 dev --port "$PORT" --persist-to "$STATE" >"$STATE/dev.log" 2>&1 &
DEV=$!
trap 'kill -- -$DEV 2>/dev/null; sleep 1; rm -rf "$STATE"' EXIT
for _ in $(seq 1 90); do curl -sf "http://localhost:$PORT/paper/api/me" >/dev/null && break; sleep 1; done
cd "$ROOT"
OGP_ACCOUNT_SITE="http://localhost:$PORT" cargo test -p og-paper --lib account::tests -- --ignored --nocapture --test-threads=1 \
  || { echo "--- wrangler log"; tail -40 "$STATE/dev.log"; exit 1; }
