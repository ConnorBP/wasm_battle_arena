export const PROTOCOL_VERSION = 2;
export const EPOCH_PROTOCOL_VERSION = 3;
export const QUEUE_PROTOCOL_VERSION = 4;
export const PLAYER_ID_PATTERN = /^[0-9a-f]{32}$/i;
export const QUEUE_TICKET_PATTERN = /^[0-9a-f]{32}$/i;
export const QUEUE_TOKEN_PATTERN = /^[0-9a-f]{64}$/i;
export const ASSIGNED_ROOM_PATTERN = /^q4_[0-9a-f]{32}$/;
export const MAX_MESSAGE_BYTES = 16 * 1024;
export const MAX_MESSAGES_PER_SECOND = 60;
export const RECONNECT_GRACE_MS = 30_000;
export const MAX_LOBBY_SOCKETS = 32;

const ROOM_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;

export function routePath(pathname) {
  const match = /^\/match\/([A-Za-z0-9_-]{1,64})$/.exec(pathname);
  if (match) return { kind: "match", room: match[1] };
  const lobby = /^\/lobby\/([A-Za-z0-9_-]{1,64})$/.exec(pathname);
  if (lobby) return { kind: "lobby", room: lobby[1] };
  // Protocol 4 intentionally has one public pool. Arbitrary queue room names
  // would create private/fragmented queues and are therefore not routed.
  if (pathname === "/queue/public-v4") return { kind: "queue", room: "public-v4" };
  return null;
}

export function validRoomName(room) {
  return ROOM_PATTERN.test(room);
}

export function parseEpochLobbyQuery(searchParams) {
  const copy = new URLSearchParams(searchParams);
  if (copy.getAll("protocol").length !== 1 || copy.get("protocol") !== "3") return fail("protocol must be 3");
  copy.delete("protocol");

  const assignmentKeys = ["queueTicket", "queueExpires", "queueToken"];
  const present = assignmentKeys.filter((key) => copy.has(key));
  let assignment = null;
  if (present.length !== 0) {
    if (present.length !== assignmentKeys.length || assignmentKeys.some((key) => copy.getAll(key).length !== 1)) {
      return fail("queue assignment fields must be supplied exactly once together");
    }
    const ticket = copy.get("queueTicket");
    const expiresText = copy.get("queueExpires");
    const token = copy.get("queueToken");
    if (!QUEUE_TICKET_PATTERN.test(ticket ?? "")) return fail("invalid queue ticket");
    if (!/^[0-9]{13}$/.test(expiresText ?? "") || !Number.isSafeInteger(Number(expiresText))) {
      return fail("invalid queue assignment expiry");
    }
    if (!QUEUE_TOKEN_PATTERN.test(token ?? "")) return fail("invalid queue assignment token");
    assignment = {
      ticket: ticket.toLowerCase(),
      expiresAt: Number(expiresText),
      token: token.toLowerCase(),
    };
    for (const key of assignmentKeys) copy.delete(key);
  }

  const lobby = parseLobbyQuery(copy);
  return lobby.ok ? { ok: true, value: { ...lobby.value, assignment } } : lobby;
}

export function parseQueueQuery(searchParams) {
  const allowed = new Set(["protocol", "preference", "target"]);
  const seen = new Set();
  for (const key of searchParams.keys()) {
    if (!allowed.has(key)) return fail(`unknown query parameter: ${key}`);
    if (seen.has(key)) return fail(`duplicate query parameter: ${key}`);
    seen.add(key);
  }
  if (searchParams.get("protocol") !== "4") return fail("protocol must be 4");
  const preference = searchParams.get("preference");
  if (!new Set(["any", "duel", "deathmatch"]).has(preference)) {
    return fail("preference must be any, duel, or deathmatch");
  }
  const targetText = searchParams.get("target");
  if (!/^[3-8]$/.test(targetText ?? "")) return fail("target must be between 3 and 8");
  return { ok: true, value: { preference, target: Number(targetText) } };
}

export function parseLobbyQuery(searchParams) {
  const allowed = new Set(["mode", "capacity", "playerId", "reconnectToken"]);
  const seen = new Set();
  for (const key of searchParams.keys()) {
    if (!allowed.has(key)) return fail(`unknown query parameter: ${key}`);
    if (seen.has(key)) return fail(`duplicate query parameter: ${key}`);
    seen.add(key);
  }

  const mode = searchParams.get("mode");
  const capacityText = searchParams.get("capacity");
  if (mode !== "duel" && mode !== "deathmatch") {
    return fail("mode must be duel or deathmatch");
  }
  if (!/^[0-9]+$/.test(capacityText ?? "")) {
    return fail("capacity must be an integer");
  }
  const capacity = Number(capacityText);
  if (mode === "duel" && capacity !== 2) {
    return fail("duel capacity must be 2");
  }
  if (mode === "deathmatch" && (capacity < 3 || capacity > 8)) {
    return fail("deathmatch capacity must be between 3 and 8");
  }

  const rawPlayerId = searchParams.get("playerId");
  const rawToken = searchParams.get("reconnectToken");
  if ((rawPlayerId === null) !== (rawToken === null)) {
    return fail("playerId and reconnectToken must be supplied together");
  }
  if (rawPlayerId !== null && !PLAYER_ID_PATTERN.test(rawPlayerId)) {
    return fail("playerId must be 32 hex characters");
  }
  if (rawToken !== null && !PLAYER_ID_PATTERN.test(rawToken)) {
    return fail("reconnectToken must be 32 hex characters");
  }

  return {
    ok: true,
    value: {
      mode,
      capacity,
      playerId: rawPlayerId?.toLowerCase() ?? null,
      reconnectToken: rawToken?.toLowerCase() ?? null,
    },
  };
}

export function parseEpochClientMessage(text) {
  let message;
  try { message = JSON.parse(text); } catch { return fail("invalid JSON"); }
  if (!isRecord(message) || typeof message.type !== "string") return fail("invalid message");
  if (message.type === "ready" && onlyKeys(message, ["type"])) return { ok: true, value: message };
  if ((message.type === "leave" || message.type === "requeue") && onlyKeys(message, ["type"])) return { ok: true, value: message };
  if (message.type === "rematch_request" && onlyKeys(message, ["type", "generation", "nonce"])) {
    if (!Number.isSafeInteger(message.generation) || message.generation < 1 || typeof message.nonce !== "string" || !PLAYER_ID_PATTERN.test(message.nonce)) return fail("invalid rematch request");
    return { ok: true, value: { ...message, nonce: message.nonce.toLowerCase() } };
  }
  if (message.type === "rematch_response" && onlyKeys(message, ["type", "generation", "nonce", "accept"])) {
    if (!Number.isSafeInteger(message.generation) || message.generation < 1 || typeof message.nonce !== "string" || !PLAYER_ID_PATTERN.test(message.nonce) || typeof message.accept !== "boolean") return fail("invalid rematch response");
    return { ok: true, value: { ...message, nonce: message.nonce.toLowerCase() } };
  }
  if (message.type === "profile" && onlyKeys(message, ["type", "name", "paletteId", "cosmeticId"])) {
    if (typeof message.name !== "string" || byteLength(message.name) === 0 || byteLength(message.name) > 24 || /[\u0000-\u001f\u007f]/.test(message.name) || message.name !== message.name.trim() || !Number.isInteger(message.paletteId) || message.paletteId < 0 || message.paletteId > 3 || !Number.isInteger(message.cosmeticId) || message.cosmeticId < 0 || message.cosmeticId > 3) return fail("invalid profile");
    return { ok: true, value: message };
  }
  if (message.type === "report" && onlyKeys(message, ["type", "epoch", "round", "outcomes"])) {
    if (!Number.isInteger(message.epoch) || message.epoch < 0 || !Number.isInteger(message.round) || message.round < 0 || !Array.isArray(message.outcomes) || message.outcomes.length < 2 || message.outcomes.length > 8) return fail("invalid report");
    for (const outcome of message.outcomes) {
      if (!isRecord(outcome) || !onlyKeys(outcome, ["playerId", "placement", "scoreDelta"]) ||
          typeof outcome.playerId !== "string" || !PLAYER_ID_PATTERN.test(outcome.playerId) ||
          !Number.isInteger(outcome.placement) || outcome.placement < 1 || outcome.placement > 8 ||
          !Number.isSafeInteger(outcome.scoreDelta) || outcome.scoreDelta < 0 || outcome.scoreDelta > 1_000_000) return fail("invalid report outcome");
    }
    return { ok: true, value: { ...message, outcomes: message.outcomes.map((outcome) => ({ ...outcome, playerId: outcome.playerId.toLowerCase() })) } };
  }
  if (message.type === "signal") {
    // Protocol 3 signals are always epoch-scoped. A signal missing `epoch` is
    // not silently downgraded to the legacy v2 schema.
    if (!onlyKeys(message, ["type", "epoch", "to", "data"])) return fail("invalid signal schema");
    if (!Number.isInteger(message.epoch) || message.epoch < 0) return fail("invalid signal epoch");
    const signal = parseClientMessage(JSON.stringify({ type: "signal", to: message.to, data: message.data }));
    return signal.ok ? { ok: true, value: { ...message, to: signal.value.to } } : fail("invalid signal");
  }
  // Only `ping` (and an explicit error for anything else) survives the legacy
  // parser; epoch signals are fully handled above.
  return parseClientMessage(text);
}

export function parseClientMessage(text) {
  let message;
  try {
    message = JSON.parse(text);
  } catch {
    return fail("invalid JSON");
  }
  if (!isRecord(message) || typeof message.type !== "string") {
    return fail("message must be an object with a type");
  }

  if (message.type === "ping") {
    if (!onlyKeys(message, ["type", "nonce"])) return fail("invalid ping schema");
    if (message.nonce !== undefined &&
        (typeof message.nonce !== "string" || message.nonce.length > 64)) {
      return fail("invalid ping nonce");
    }
    return { ok: true, value: message };
  }

  if (message.type !== "signal" || !onlyKeys(message, ["type", "to", "data"])) {
    return fail("unsupported message schema");
  }
  if (typeof message.to !== "string" || !PLAYER_ID_PATTERN.test(message.to)) {
    return fail("signal target must be a 32-hex player ID");
  }
  const dataResult = validateSignalData(message.data);
  if (!dataResult.ok) return dataResult;
  return {
    ok: true,
    value: { type: "signal", to: message.to.toLowerCase(), data: message.data },
  };
}

export function validateSignalData(data) {
  if (!isRecord(data) || typeof data.type !== "string") {
    return fail("invalid signal data");
  }
  if (data.type === "offer" || data.type === "answer") {
    if (!onlyKeys(data, ["type", "sdp"]) ||
        typeof data.sdp !== "string" || byteLength(data.sdp) === 0 || byteLength(data.sdp) > 12 * 1024) {
      return fail("invalid SDP signal");
    }
    return { ok: true, value: data };
  }
  if (data.type !== "ice" || !onlyKeys(data, ["type", "candidate"])) {
    return fail("invalid signal type");
  }
  if (data.candidate === null) return { ok: true, value: data };
  const candidate = data.candidate;
  if (!isRecord(candidate) ||
      !onlyKeys(candidate, ["candidate", "sdpMid", "sdpMLineIndex", "usernameFragment"]) ||
      typeof candidate.candidate !== "string" || byteLength(candidate.candidate) > 2048 ||
      !optionalString(candidate.sdpMid, 64) ||
      !optionalInteger(candidate.sdpMLineIndex, 0, 255) ||
      !optionalString(candidate.usernameFragment, 256)) {
    return fail("invalid ICE candidate");
  }
  return { ok: true, value: data };
}

export function applyMessageRateLimit(rate, now) {
  const next = {
    windowStarted: rate?.windowStarted ?? now,
    windowMessages: rate?.windowMessages ?? 0,
  };
  if (now - next.windowStarted >= 1000) {
    next.windowStarted = now;
    next.windowMessages = 0;
  }
  next.windowMessages += 1;
  return { allowed: next.windowMessages <= MAX_MESSAGES_PER_SECOND, rate: next };
}

export function canonicalRoster(records) {
  return [...records]
    .sort((a, b) => a.playerId.localeCompare(b.playerId))
    .map((record, index) => ({ ...record, index }));
}

export function randomHex(bytes = 16, cryptoImpl = globalThis.crypto) {
  const value = cryptoImpl.getRandomValues(new Uint8Array(bytes));
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function byteLength(value) {
  return new TextEncoder().encode(value).byteLength;
}

function onlyKeys(value, allowed) {
  const keys = Object.keys(value);
  return keys.every((key) => allowed.includes(key)) && allowed.every((key) =>
    key === "nonce" || key === "sdpMid" || key === "sdpMLineIndex" ||
    key === "usernameFragment" || Object.hasOwn(value, key));
}

function optionalString(value, maxLength) {
  return value == null || (typeof value === "string" && value.length <= maxLength);
}

function optionalInteger(value, minimum, maximum) {
  return value == null || (Number.isInteger(value) && value >= minimum && value <= maximum);
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function fail(error) {
  return { ok: false, error };
}
