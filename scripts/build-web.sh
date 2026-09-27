#!/usr/bin/env bash
# Build the spike for the browser (WebGPU) into web/pkg.
# Needs: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version <lockfile version>
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p og-spike --lib --target wasm32-unknown-unknown
"$(command -v wasm-bindgen || echo "$HOME/.cargo/bin/wasm-bindgen")" --target web --no-typescript --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/og_spike.wasm
if command -v wasm-opt >/dev/null; then wasm-opt -O3 -o web/pkg/og_spike_bg.wasm web/pkg/og_spike_bg.wasm; fi
echo "Built web/pkg. Serve with: scripts/serve-web.py  (http://localhost:8990)"

# The app (Phase 1) at web/app/
cargo build --release -p og-paper --lib --target wasm32-unknown-unknown
"$(command -v wasm-bindgen || echo "$HOME/.cargo/bin/wasm-bindgen")" --target web --no-typescript --out-dir web/app/pkg \
  target/wasm32-unknown-unknown/release/og_paper.wasm
echo "Built web/app/pkg (app at /app/)."
