#!/usr/bin/env bash
set -euo pipefail
base="${1:-https://ghost.segfault.site}"
wasm="$base/out/wasm_battle_arena_bg.wasm"
echo "Checking $base"
curl -fsSI "$wasm" | tr -d '\r' | grep -Ei 'HTTP/|content-type|content-length|last-modified'
for asset in assets/music/menu.ogg assets/textures/character/ghost_base.png; do
  curl -fsS -o /dev/null "$base/$asset"
done
for excluded in assets/music/menu.mp3 assets/sfx/laser_shoot.wav assets/maps/map1.bmp; do
  code="$(curl -sS -o /dev/null -w '%{http_code}' "$base/$excluded")"
  test "$code" = 404 || { echo "unexpected deployed source asset: $excluded ($code)"; exit 1; }
done
