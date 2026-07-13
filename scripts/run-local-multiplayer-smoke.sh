#!/usr/bin/env bash
# Bounded local-only multiplayer validation. This script intentionally starts
# Wrangler and the static site on loopback and never targets a deployed Worker.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
worker_dir="$root/cloudflare-worker"
site="$root/.local-multiplayer-smoke-site"
artifact_dir="${ARTIFACT_DIR:-$root/artifacts/local-multiplayer-smoke}"
worker_port="${WORKER_PORT:-8787}"
site_port="${SITE_PORT:-4173}"
local_origin="http://127.0.0.1:${site_port}"
worker_url="ws://127.0.0.1:${worker_port}"
worker_pid=""
site_pid=""

mkdir -p "$artifact_dir"
rm -rf "$site"
mkdir -p "$site/out"

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ -n "$site_pid" ]]; then kill "$site_pid" 2>/dev/null || true; fi
  if [[ -n "$worker_pid" ]]; then
    kill "$worker_pid" 2>/dev/null || true
    command -v taskkill.exe >/dev/null && cmd.exe //c taskkill //F //T //PID "$worker_pid" >/dev/null 2>&1 || true
  fi
  wait "$site_pid" 2>/dev/null || true
  wait "$worker_pid" 2>/dev/null || true
  sleep 1
  if [[ $status -eq 0 && "${KEEP_LOCAL_SMOKE_SITE:-0}" != "1" ]]; then rm -rf "$site" 2>/dev/null || true; fi
  exit "$status"
}
trap cleanup EXIT INT TERM

cd "$root"
# `local` selects the loopback signaling URL; `dev_net` keeps local rooms away
# from normal room names while retaining the real public protocol and UI.
cargo build --locked --release --target wasm32-unknown-unknown --features local,dev_net
wasm-bindgen --out-dir "$site/out" --target web \
  "$root/target/wasm32-unknown-unknown/release/wasm_battle_arena.wasm"
cp "$root/index.html" "$site/"
cp -r "$root/assets" "$site/"

(
  cd "$worker_dir"
  exec npx --no-install wrangler dev --local --ip 127.0.0.1 --port "$worker_port" \
    --config wrangler.local.jsonc \
    --persist-to "$site/wrangler-state"
) >"$artifact_dir/wrangler.log" 2>&1 &
worker_pid=$!

python -m http.server "$site_port" --bind 127.0.0.1 --directory "$site" \
  >"$artifact_dir/site-server.log" 2>&1 &
site_pid=$!

wait_http() {
  local url="$1" name="$2" pid="$3"
  local attempt
  for attempt in $(seq 1 120); do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "$name exited during startup; see $artifact_dir" >&2
      return 1
    fi
    # Wrangler returns 426 for this ordinary HTTP request when ready; Python
    # returns 200. Either proves the loopback listener is accepting requests.
    if curl --silent --output /dev/null --max-time 1 "$url"; then return 0; fi
    sleep 0.25
  done
  echo "timed out waiting for $name at $url; see $artifact_dir" >&2
  return 1
}
wait_http "http://127.0.0.1:${worker_port}/lobby/readiness?protocol=3&mode=duel&capacity=2" "Wrangler" "$worker_pid"
wait_http "${local_origin}/" "game site" "$site_pid"

ARTIFACT_DIR="$artifact_dir" WORKER_URL="$worker_url" ORIGIN="$local_origin" \
  npm run smoke:multiplayer:protocol
ARTIFACT_DIR="$artifact_dir" GAME_URL="$local_origin" \
  npm run smoke:multiplayer:browser

echo "PASS: local multiplayer smoke suites (artifacts: $artifact_dir)"
