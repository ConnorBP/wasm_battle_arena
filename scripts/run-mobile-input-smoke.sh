#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
site="$root/.smoke-mobile-input"
port="${SMOKE_PORT:-4174}"
server_pid=""
cleanup() { if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; fi; rm -rf "$site"; }
trap cleanup EXIT
cd "$root"
cargo build --locked --release --target wasm32-unknown-unknown --features auto_mobile_input_test
rm -rf "$site" && mkdir -p "$site/out"
wasm-bindgen --out-dir "$site/out" --target web target/wasm32-unknown-unknown/release/wasm_battle_arena.wasm
cp index.html "$site/" && cp -r assets "$site/"
python -m http.server "$port" --bind 127.0.0.1 --directory "$site" > "$site/server.log" 2>&1 &
server_pid=$!
SMOKE_URL="http://127.0.0.1:$port" npm run smoke:mobile-input
