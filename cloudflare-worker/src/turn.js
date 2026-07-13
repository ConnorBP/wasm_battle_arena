const TURN_TTL_SECONDS = 21_600;
const REQUEST_TIMEOUT_MS = 3_000;
const MAX_RESPONSE_BYTES = 16 * 1024;
const MAX_ICE_SERVERS = 8;
const MAX_URLS_PER_SERVER = 8;
const MAX_URL_LENGTH = 256;
const MAX_USERNAME_LENGTH = 512;
const MAX_CREDENTIAL_LENGTH = 512;

export const STUN_ONLY_ICE_SERVERS = Object.freeze([
  Object.freeze({ urls: "stun:stun.cloudflare.com:3478" }),
]);

/**
 * Mint short-lived TURN credentials for one admitted matchmaking identity.
 * Every failure deliberately degrades to public STUN so credential service
 * availability can never make matchmaking fail.
 */
export async function generateIceServers(env, options = {}) {
  const fallback = () => ({ iceServers: STUN_ONLY_ICE_SERVERS, turnExpiresAt: null });
  if (typeof env?.TURN_KEY_ID !== "string" || env.TURN_KEY_ID.length === 0 || env.TURN_KEY_ID.length > 256 ||
      typeof env?.TURN_KEY_API_TOKEN !== "string" || env.TURN_KEY_API_TOKEN.length === 0 || env.TURN_KEY_API_TOKEN.length > 2048) {
    return fallback();
  }

  const fetchImpl = options.fetchImpl ?? globalThis.fetch;
  const now = options.now ?? Date.now();
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), options.timeoutMs ?? REQUEST_TIMEOUT_MS);
  try {
    const keyId = encodeURIComponent(env.TURN_KEY_ID);
    const response = await fetchImpl(
      `https://rtc.live.cloudflare.com/v1/turn/keys/${keyId}/credentials/generate-ice-servers`,
      {
        method: "POST",
        headers: {
          authorization: `Bearer ${env.TURN_KEY_API_TOKEN}`,
          "content-type": "application/json",
        },
        body: JSON.stringify({ ttl: TURN_TTL_SECONDS }),
        signal: controller.signal,
      },
    );
    if (!response?.ok) return fallback();
    const contentLength = response.headers?.get?.("content-length");
    if (contentLength !== null && contentLength !== undefined &&
        (!/^\d+$/.test(contentLength) || Number(contentLength) > MAX_RESPONSE_BYTES)) return fallback();

    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.byteLength === 0 || bytes.byteLength > MAX_RESPONSE_BYTES) return fallback();
    let payload;
    try { payload = JSON.parse(new TextDecoder().decode(bytes)); } catch { return fallback(); }
    const iceServers = validateIceServers(payload?.iceServers);
    if (!iceServers || !iceServers.some((server) => server.username !== undefined) ||
        new TextEncoder().encode(JSON.stringify(iceServers)).byteLength > 12 * 1024) return fallback();
    return { iceServers, turnExpiresAt: now + TURN_TTL_SECONDS * 1000 };
  } catch {
    return fallback();
  } finally {
    clearTimeout(timer);
  }
}

/** Strictly copy an API response; never return unvalidated attacker-controlled fields. */
export function validateIceServers(value) {
  if (!Array.isArray(value) || value.length === 0 || value.length > MAX_ICE_SERVERS) return null;
  const output = [];
  for (const item of value) {
    if (!item || typeof item !== "object" || Array.isArray(item) ||
        Object.keys(item).some((key) => !["urls", "username", "credential"].includes(key))) return null;
    const rawUrls = typeof item.urls === "string" ? [item.urls] : item.urls;
    if (!Array.isArray(rawUrls) || rawUrls.length === 0 || rawUrls.length > MAX_URLS_PER_SERVER) return null;
    const urls = [];
    for (const rawUrl of rawUrls) {
      const url = validateIceUrl(rawUrl);
      if (url === undefined) return null;
      if (url !== null) urls.push(url);
    }
    // Port 53 and any non-allowlisted URL are filtered, not passed to WebRTC.
    if (urls.length === 0) continue;
    const hasTurn = urls.some((url) => url.startsWith("turn:") || url.startsWith("turns:"));
    if (hasTurn) {
      // Cloudflare may group its STUN and TURN URLs in one RTCIceServer;
      // credentials are required because at least one URL is TURN.
      if (typeof item.username !== "string" || item.username.length === 0 ||
          item.username.length > MAX_USERNAME_LENGTH ||
          typeof item.credential !== "string" || item.credential.length === 0 ||
          item.credential.length > MAX_CREDENTIAL_LENGTH) return null;
      output.push({ urls: urls.length === 1 ? urls[0] : urls, username: item.username, credential: item.credential });
    } else {
      if (item.username !== undefined || item.credential !== undefined) return null;
      output.push({ urls: urls.length === 1 ? urls[0] : urls });
    }
  }
  return output.length > 0 ? output : null;
}

function validateIceUrl(value) {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_URL_LENGTH || /[\u0000-\u0020\u007f]/.test(value)) return undefined;
  // ICE URLs are not normal hierarchical URLs. Parse the authority ourselves
  // and require the exact Cloudflare host (no suffix/subdomain tricks).
  const match = /^(stun|turn|turns):([^?]+)(?:\?transport=(udp|tcp))?$/.exec(value);
  if (!match) return undefined;
  const scheme = match[1];
  const authority = match[2];
  const hostPort = /^(stun\.cloudflare\.com|turn\.cloudflare\.com)(?::([0-9]{1,5}))?$/.exec(authority);
  if (!hostPort) return undefined;
  const port = hostPort[2] === undefined ? null : Number(hostPort[2]);
  if (port === 53) return null;
  if (port !== null && (port < 1 || port > 65_535)) return undefined;
  if (scheme === "stun" && match[3] !== undefined) return undefined;
  if (scheme === "stun" && hostPort[1] !== "stun.cloudflare.com") return undefined;
  if ((scheme === "turn" || scheme === "turns") && hostPort[1] !== "turn.cloudflare.com") return undefined;
  return value;
}
