#!/usr/bin/env bash
# Deploy the page server to go: build the image here and load it there.
# (Building on go compiles Rust twice, server and web app, and swamps it.)
#   scripts/deploy-go.sh [host]
set -euo pipefail
cd "$(dirname "$0")/.."
HOST=${1:-go}
docker build -t og-paper:latest .
docker save og-paper:latest | gzip -1 | ssh "$HOST" 'gunzip | docker load'
ssh "$HOST" 'cd ~/og-paper-server && docker compose up -d --no-build og-paper && sleep 3 && docker compose logs --tail 6 og-paper | grep -v "k="'
