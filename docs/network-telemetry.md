# Network telemetry

`NetworkTelemetry` exposes local, aggregate counters only:

* packet send/receive/drop and stale-epoch counts
* reconnect and report counts
* `relay_connections` / `candidate_pair_relay`
* `stun_fallbacks`
* `candidate_pair_host` and `candidate_pair_srflx`

When an `RTCPeerConnection` reaches `connected`, the browser calls `getStats()`, resolves the selected candidate pair and its local/remote candidate records, then classifies it as:

1. `relay` if either candidate has `candidateType: "relay"`;
2. `srflx` if either has `srflx` or `prflx` and neither is relay;
3. `host` otherwise.

Each peer connection is counted at most once. Failures to read stats are ignored and never affect the connection. These counters intentionally contain no TURN username, credential, URL, SDP, candidate text, address, or token. Do not add those fields to logs or analytics.
