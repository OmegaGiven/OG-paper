#!/usr/bin/env bash
# The native app's vault sign-in and sync (account.rs, ignored test
# a_page_kept_in_a_vault_comes_back_on_a_new_device) against a THROWAWAY vault on og-host1
# (see run-e2e.sh for starting one). The test binary is built on og-host1 and run here,
# through a tunnel to the vault's loopback port.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HOST=root@192.168.0.137
APP=${OGV_APP:-og-vault-e2e-paper792}
PORT=${OGV_PORT:-18792}
case "$APP" in og-vault) echo "refusing to run against the real vault" >&2; exit 1;; esac
cd "$ROOT"
if [ -z "${SKIP_BUILD:-}" ]; then
  forge run --on og-host1 --pull target/vt 'set -e; f=$(cargo test -p og-paper --lib --no-run --message-format=json 2>/dev/null | python3 -c "import sys,json
for l in sys.stdin:
  try: m=json.loads(l)
  except Exception: continue
  if m.get(\"reason\")==\"compiler-artifact\" and m.get(\"executable\") and m[\"target\"][\"name\"]==\"og_paper\": print(m[\"executable\"])" | tail -1); mkdir -p target/vt; cp "$f" target/vt/og_paper_test'
fi
sql() { ssh "$HOST" "og-hostd exec $APP-db psql -U vault -d vault -Atc \"$1\""; }
aid=$(python3 -c 'import uuid; print(uuid.uuid4())')
name="nat-$(head -c3 /dev/urandom | od -An -tx1 | tr -d ' \n')"
tok="e2e-session-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
sql "insert into auth.account (id, username, display_name, role, password_hash, mfa_policy) values ('$aid', '$name', '$name', 'member', 'x', 'off')" >/dev/null
sql "insert into auth.session (id, account_id, token_hash, mfa_done, mfa_at, expires_at) values ('$(python3 -c 'import uuid; print(uuid.uuid4())')', '$aid', '$(printf %s "$tok" | sha256sum | cut -d' ' -f1)', true, now(), now() + interval '1 day')" >/dev/null
ssh -N -L "$PORT:127.0.0.1:$PORT" "$HOST" &
tunnel=$!
trap 'kill $tunnel 2>/dev/null' EXIT
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$PORT/.well-known/og-vault" >/dev/null && break; sleep 0.2; done
OGP_VAULT="http://127.0.0.1:$PORT" OGP_VAULT_SESSION="$tok" target/vt/og_paper_test --ignored --nocapture a_page_kept_in_a_vault
