#!/usr/bin/env bash
# Deploys the OG Paper web app to our vault on og-host1, which serves it at
#   https://og-host1.tail6c6721.ts.net:8610/paper/
# (og-vault docs/PAPER.md: vaultd reads dist/og-paper/web on each request, no restart).
# This is the vault test path only: the public app on ogstudio.org is untouched.
# The copy carries <meta name="og-paper-vault" content="self">, so Vault sign-in
# starts with this vault's address filled in.
#   scripts/deploy-vault-web.sh            build the wasm on a runner, then deploy
#   SKIP_BUILD=1 scripts/deploy-vault-web.sh   deploy web/app as it is (pkg/ already built)
set -euo pipefail
HOST=root@192.168.0.137
DIST=/var/lib/og-host/apps/og-vault/dist/og-paper
URL=https://og-host1.tail6c6721.ts.net:8610/paper/
cd "$(dirname "$0")/.."
if [ -n "$(git status --porcelain -- web/app crates)" ]; then
  echo "!! uncommitted changes in web/app or crates: commit first so VERSION.txt means something" >&2
  exit 1
fi
for f in web/app/*.js web/app/vault/*.js; do node --check "$f"; done
if [ -z "${SKIP_BUILD:-}" ]; then
  echo "== building the wasm app on og-host1"
  forge run --on og-host1 --pull web/app/pkg 'set -e
    rustup target add wasm32-unknown-unknown >/dev/null 2>&1
    command -v wasm-bindgen >/dev/null || cargo install wasm-bindgen-cli --version 0.2.129 --locked
    cargo build --release -p og-paper --lib --target wasm32-unknown-unknown
    wasm-bindgen --target web --no-typescript --out-dir web/app/pkg target/wasm32-unknown-unknown/release/og_paper.wasm
    if command -v wasm-opt >/dev/null; then wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int -o web/app/pkg/og_paper_bg.wasm web/app/pkg/og_paper_bg.wasm; fi'
fi
[ -f web/app/pkg/og_paper_bg.wasm ] || { echo "!! web/app/pkg is missing: build first" >&2; exit 1; }
ver="$(git rev-parse --short HEAD) $(git log -1 --format=%cI)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cp -r web/app/. "$stage/"
# This copy is served by a vault: its sign-in offers this vault first.
python3 - "$stage/index.html" <<'PY'
import sys
p = sys.argv[1]; s = open(p).read()
tag = '<meta name="og-paper-vault" content="self">'
if tag not in s:
    s = s.replace('<meta charset="utf-8">', '<meta charset="utf-8">\n' + tag, 1)
open(p, 'w').write(s)
PY
echo "$ver" > "$stage/VERSION.txt"

echo "== deploying web/app ($ver) to og-host1:$DIST/web"
# Unpack next to the live copy, then swap directories, so the vault never serves half a deploy.
tar -C "$stage" -cf - . | ssh "$HOST" "set -e; mkdir -p $DIST; rm -rf $DIST/web.new $DIST/web.old; mkdir $DIST/web.new
  tar -xf - -C $DIST/web.new --no-same-owner
  chmod -R a+rX $DIST/web.new
  if [ -d $DIST/web ]; then mv $DIST/web $DIST/web.old; fi
  mv $DIST/web.new $DIST/web
  rm -rf $DIST/web.old"

echo "== checking what the vault serves"
want="$(sha256sum web/app/og-vault.js | cut -d' ' -f1)"
got="$(curl -fsS --max-time 20 "${URL}og-vault.js" | sha256sum | cut -d' ' -f1)"
if [ "$want" = "$got" ]; then
  echo "-> live: $URL ($ver)"
else
  echo "!! files are in place, but ${URL}og-vault.js differs (is the vault's /paper/ route deployed? og-vault task #791)" >&2
  exit 3
fi
