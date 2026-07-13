# Performance profiling

Optimization changes require measurements from the production WebAssembly build.

## Reproducible scenario

1. Open two Chromium windows at `https://ghost.segfault.site`.
2. Start the same private match and play for five minutes.
3. Record Chrome Performance traces during normal movement, sustained firing, pickup collection, death, and round regeneration.
4. Capture p50/p95 frame duration, long tasks, memory growth, entity count, and GGRS interruption/rollback events.
5. Repeat on one named mid-range mobile device in portrait and landscape.

## Current budgets

- Simulation: 60 Hz (16.67 ms frame budget).
- Production WASM: recorded by the Pages workflow summary and verified with `scripts/check-deployment.sh`.
- Runtime assets: PNG/OGG allowlist only.
- Multiplayer: full-mesh work is capped at four players and requires separate browser profiling before release.

Do not rewrite deterministic simulation from suspicion. Optimize only a measured p95 bottleneck and compare before/after traces using the same match seed and scenario.
