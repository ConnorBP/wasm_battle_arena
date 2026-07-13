# Lobby protocol v2

Lobby v2 is a fixed-roster WebSocket control and WebRTC-signaling protocol. It is served at:

```
wss://<worker>/lobby/<room>?mode=<mode>&capacity=<n>
```

Room names are 1–64 ASCII letters, digits, `_`, or `-`. The existing `/match/<room>` duel matcher is unchanged. Requests must be `GET` WebSocket upgrades from an origin in `ALLOWED_ORIGINS`; the Worker-level matchmaking rate limiter applies to both routes.

## Room configuration

The first connection fixes a room's mode and capacity. Later connections must provide the same values.

| Mode | Capacity |
|---|---:|
| `duel` | exactly 2 |
| `deathmatch` | 2–4 |

Unknown query parameters are rejected. A fresh connection omits identity parameters. The server creates a lowercase 32-hex player ID and a lowercase 32-hex reconnect token.

## Server messages

A successful connection first receives:

```json
{
  "type": "welcome",
  "protocol": 2,
  "playerId": "0123456789abcdef0123456789abcdef",
  "reconnectToken": "fedcba9876543210fedcba9876543210",
  "reconnectGraceMs": 30000
}
```

The reconnect token is disclosed only in `welcome`; persistent room state contains its SHA-256 hash. Clients must replace stored credentials after every successful reconnect because tokens rotate.

Before the roster fills, a player receives:

```json
{"type":"status","status":"waiting","mode":"duel","capacity":2,"started":false,"epoch":null}
```

When capacity is reached, the Durable Object freezes canonical epoch 0. Roster entries are sorted by player ID and assigned stable indices. Every roster player receives the same immutable message:

```json
{
  "type": "start",
  "epoch": 0,
  "mode": "duel",
  "capacity": 2,
  "seed": "32 lowercase hex characters",
  "roster": [
    {"playerId":"...","index":0},
    {"playerId":"...","index":1}
  ]
}
```

Connections arriving after start remain waiting and receive `started: true, epoch: 0`. A disconnect or expiry never promotes a waiting player, changes the roster, or creates epoch 1.

## Signaling and control

Control sockets stay open after `start`. Roster members can send directed SDP/ICE messages to another epoch-0 roster ID:

```json
{"type":"signal","to":"<player-id>","data":{"type":"offer","sdp":"..."}}
{"type":"signal","to":"<player-id>","data":{"type":"answer","sdp":"..."}}
{"type":"signal","to":"<player-id>","data":{"type":"ice","candidate":null}}
```

An ICE candidate may instead be an object containing `candidate` and optional `sdpMid`, `sdpMLineIndex`, and `usernameFragment`. The recipient sees:

```json
{"type":"signal","epoch":0,"from":"<player-id>","data":{}}
```

Waiting players cannot signal. Targets must be different, online members of the frozen roster. Applications may keep a control socket alive with `{"type":"ping","nonce":"optional"}`; the reply is `pong`.

## Reconnection

On an unintentional close, an identity is reconnectable for 30 seconds:

```
wss://<worker>/lobby/<room>?mode=duel&capacity=2&playerId=<id>&reconnectToken=<token>
```

Both credentials are required and validated. A successful reconnect rotates the token and closes any previous live socket for that identity, so at most one socket is authoritative. Roster positions and the seed do not change. After grace expiry, roster slots remain frozen and unavailable; non-roster waiting identities are removed. Durable Object alarms enforce grace deadlines even while idle.

## Limits and errors

* inbound message size: 16 KiB UTF-8
* inbound rate: 60 messages/second/socket
* SDP: 12 KiB
* ICE candidate string: 2 KiB
* up to 32 live control sockets per room
* strict JSON schemas; unknown fields and binary frames are rejected

Protocol violations send an `error` when possible and close with WebSocket code 1008. Recoverable signaling-state errors (for example an offline target) send `error` without closing. HTTP validation failures use 400/401/409/410/426 as appropriate.
