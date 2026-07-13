export const PROTOCOL_VERSION = 2;
export const PLAYER_ID_PATTERN = /^[0-9a-f]{32}$/i;
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
  return null;
}

export function validRoomName(room) {
  return ROOM_PATTERN.test(room);
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
  if (mode === "deathmatch" && (capacity < 2 || capacity > 4)) {
    return fail("deathmatch capacity must be between 2 and 4");
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
