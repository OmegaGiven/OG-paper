#!/usr/bin/env bash
# Build the web app, start the example hosted OG Paper under `wrangler dev`
# (local, throwaway state) and run the browser end-to-end tests against it
# (Chromium via Playwright). Usage: hosted/test/run-browser-e2e.sh [test.mjs ...]
# (default: browser-e2e.mjs chat-e2e.mjs hardening-e2e.mjs). SHOTS=dir keeps screenshots;
# REBUILD=1 rebuilds the wasm even if web/app/pkg has one.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
if [ ! -f web/app/pkg/og_paper_bg.wasm ] || [ -n "${REBUILD:-}" ]; then
  cargo build --release -p og-paper --lib --target wasm32-unknown-unknown
  "$(command -v wasm-bindgen || echo "$HOME/.cargo/bin/wasm-bindgen")" --target web --no-typescript --out-dir web/app/pkg \
    target/wasm32-unknown-unknown/release/og_paper.wasm
fi
# Playwright (its browsers are usually cached already).
PW="${PW:-$ROOT/target/pw}"
if [ ! -d "$PW/node_modules/playwright" ]; then
  mkdir -p "$PW"
  (cd "$PW" && npm init -y >/dev/null && npm i --silent "playwright@${PW_VERSION:-1.64.1}" >/dev/null)
  (cd "$PW" && npx playwright install chromium >/dev/null)
fi
cd hosted/example
export WRANGLER_SEND_METRICS=false
STATE=$(mktemp -d)
# A free port (another run may hold 8799), and wrangler in its own process
# group so workerd goes too when we stop it.
PORT="${PORT:-$(node -e 'const s=require("net").createServer().listen(0,()=>{console.log(s.address().port);s.close()})')}"
npx -y wrangler@4 d1 migrations apply DB --local --persist-to "$STATE" >/dev/null
setsid npx -y wrangler@4 dev --port "$PORT" --persist-to "$STATE" >"$STATE/dev.log" 2>&1 &
DEV=$!
trap 'kill -- -$DEV 2>/dev/null; sleep 1; rm -rf "$STATE"' EXIT
for _ in $(seq 1 90); do curl -sf "http://localhost:$PORT/paper/api/me" >/dev/null && break; sleep 1; done
TESTS=("$@")
[ ${#TESTS[@]} -eq 0 ] && TESTS=(browser-e2e.mjs chat-e2e.mjs hardening-e2e.mjs)
for t in "${TESTS[@]}"; do
  echo "== $t"
  BASE="http://localhost:$PORT" PW="$PW" node "../test/$t" || { echo "--- wrangler log"; tail -40 "$STATE/dev.log"; exit 1; }
done
