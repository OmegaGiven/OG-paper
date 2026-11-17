#!/usr/bin/env bash
# OG Paper vault mode in a browser, against a THROWAWAY OG Vault on og-host1 (never the real one):
#   1. start one from og-vault (its deploy script):
#        OGV_APP=og-vault-e2e-paper792 OGV_PORT=18792 sh <og-vault>/deploy/homelab/deploy.sh
#   2. tests/vault/run-e2e.sh          (web/app/pkg built first; SHOTS=dir keeps screenshots)
# Makes two people (alice, bob) with browser sessions in that vault, puts web/app in its
# dist/og-paper/web, tunnels its port here and runs tests/vault/vault-e2e.mjs with Playwright.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HOST=root@192.168.0.137
APP=${OGV_APP:-og-vault-e2e-paper792}
PORT=${OGV_PORT:-18792}
case "$APP" in og-vault) echo "refusing to run against the real vault" >&2; exit 1;; esac
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; SHOTS="$(cd "$SHOTS" && pwd)"; export SHOTS; fi
cd "$ROOT"
[ -f web/app/pkg/og_paper_bg.wasm ] || { echo "build web/app/pkg first (scripts/deploy-vault-web.sh builds it)" >&2; exit 1; }

sql() { ssh "$HOST" "og-hostd exec $APP-db psql -U vault -d vault -Atc \"$1\""; }
person() {
  local name="$1-$(head -c3 /dev/urandom | od -An -tx1 | tr -d ' \n')" aid tok
  aid=$(python3 -c 'import uuid; print(uuid.uuid4())')
  tok="e2e-session-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
  sql "insert into auth.account (id, username, display_name, role, password_hash, mfa_policy) values ('$aid', '$name', '$name', 'member', 'x', 'off')" >/dev/null
  sql "insert into auth.session (id, account_id, token_hash, mfa_done, mfa_at, expires_at) values ('$(python3 -c 'import uuid; print(uuid.uuid4())')', '$aid', '$(printf %s "$tok" | sha256sum | cut -d' ' -f1)', true, now(), now() + interval '1 day')" >/dev/null
  echo "$tok"
}
ALICE=$(person alice)
BOB=$(person bob)
export ALICE BOB

echo "== web/app into $APP's dist/og-paper/web"
stage="$(mktemp -d)"
cp -r web/app/. "$stage/"
sed -i 's|<meta charset="utf-8">|<meta charset="utf-8">\n<meta name="og-paper-vault" content="self">|' "$stage/index.html"
tar -C "$stage" -cf - . | ssh "$HOST" "set -e; d=/var/lib/og-host/apps/$APP/dist/og-paper; rm -rf \$d/web; mkdir -p \$d/web; tar -xf - -C \$d/web --no-same-owner; chmod -R a+rX \$d"
rm -rf "$stage"

PW="${PW:-$ROOT/target/pw}"
if [ ! -d "$PW/node_modules/playwright" ]; then
  mkdir -p "$PW"
  (cd "$PW" && npm init -y >/dev/null && npm i --silent "playwright@${PW_VERSION:-1.64.1}" >/dev/null)
  (cd "$PW" && npx playwright install chromium >/dev/null)
fi
ssh -N -L "$PORT:127.0.0.1:$PORT" "$HOST" &
tunnel=$!
trap 'kill $tunnel 2>/dev/null' EXIT
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$PORT/.well-known/og-vault" >/dev/null && break; sleep 0.2; done
VAULT="http://127.0.0.1:$PORT" PW="$PW" node tests/vault/vault-e2e.mjs
