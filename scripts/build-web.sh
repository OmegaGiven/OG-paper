#!/usr/bin/env bash
# Build the spike for the browser (WebGPU) into web/pkg.
# Needs: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version <lockfile version>
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p og-spike --lib --target wasm32-unknown-unknown
"$(command -v wasm-bindgen || echo "$HOME/.cargo/bin/wasm-bindgen")" --target web --no-typescript --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/og_spike.wasm
if command -v wasm-opt >/dev/null; then wasm-opt -O3 -o web/pkg/og_spike_bg.wasm web/pkg/og_spike_bg.wasm; fi
echo "Built web/pkg. Serve with: python3 -m http.server -d web 8990 --bind 127.0.0.1"
