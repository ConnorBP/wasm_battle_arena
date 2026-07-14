#!/usr/bin/env bash
# Local-only real WASM/WebRTC/GGRS lifecycle harness. This script is the build
# wrapper; network-transition-smoke.mjs itself never starts services or retries.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
worker_dir="$root/cloudflare-worker"
site="$root/.network-transition-smoke-site"
artifact_dir="${ARTIFACT_DIR:-$root/artifacts/network-transition}"
worker_port="${WORKER_PORT:-8787}"
site_port="${SITE_PORT:-4173}"
origin="http://127.0.0.1:${site_port}"
worker_pid=""
site_pid=""

mkdir -p "$artifact_dir"
# Use a unique persistence directory so a prior Windows workerd process cannot
# lock the next scenario's startup cleanup.
site="$site-$$-${RANDOM}"
mkdir -p "$site/out"
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  [[ -z "$site_pid" ]] || kill "$site_pid" 2>/dev/null || true
  [[ -z "$worker_pid" ]] || {
    kill "$worker_pid" 2>/dev/null || true
    command -v taskkill.exe >/dev/null && cmd.exe //c taskkill //F //T //PID "$worker_pid" >/dev/null 2>&1 || true
  }
  wait "$site_pid" 2>/dev/null || true
  wait "$worker_pid" 2>/dev/null || true
  sleep 1
  rm -rf "$site" 2>/dev/null || true
  exit "$status"
}
trap cleanup EXIT INT TERM

cd "$root"
cargo build --locked --release --target wasm32-unknown-unknown --features network_transition_test
wasm-bindgen --out-dir "$site/out" --target web target/wasm32-unknown-unknown/release/wasm_battle_arena.wasm
cp index.html "$site/"
cp -r assets "$site/"
(
  cd "$worker_dir"
  exec npx --no-install wrangler dev --local --ip 127.0.0.1 --port "$worker_port" \
    --config wrangler.local.jsonc --persist-to "$site/wrangler-state"
) >"$artifact_dir/wrangler.log" 2>&1 &
worker_pid=$!
python -m http.server "$site_port" --bind 127.0.0.1 --directory "$site" >"$artifact_dir/site-server.log" 2>&1 &
site_pid=$!

# Bounded listener readiness is infrastructure setup, not a scenario retry.
for target in "http://127.0.0.1:${worker_port}/lobby/readiness?protocol=3&mode=duel&capacity=2" "$origin/"; do
  ready=0
  for _ in $(seq 1 120); do
    if curl --silent --output /dev/null --max-time 1 "$target"; then ready=1; break; fi
    sleep 0.25
  done
  [[ "$ready" == 1 ]] || { echo "listener did not become ready: $target" >&2; exit 1; }
done

if [[ -n "${1:-}" ]]; then
  scenarios=("$1")
elif [[ -n "${TRANSITION_SCENARIO:-}" ]]; then
  scenarios=("$TRANSITION_SCENARIO")
else
  scenarios=(rollover active_disconnect rollover_disconnect reconnect rematch requeue changed_roster)
fi
for scenario in "${scenarios[@]}"; do
  # Unique rooms make every scenario independent without rerunning a failure.
  room="GT${scenario//_/}"
  ARTIFACT_DIR="$artifact_dir" GAME_URL="$origin" TRANSITION_SCENARIO="$scenario" TRANSITION_ROOM="$room" \
    node scripts/network-transition-smoke.mjs
done
