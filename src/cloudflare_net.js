// Browser networking bridge imported by wasm-bindgen. Keep the exported names
// and scalar/BigInt return types in sync with the extern block in cloudflare_net.rs.
const networks = new Map();
let nextTransportId = 1;
const MAX_PACKET_BYTES = 64 * 1024;
const MAX_QUEUED_PACKETS = 256;
const MAX_BUFFERED_BYTES = 1024 * 1024;
const MATCHMAKING_TIMEOUT_MS = 2 * 60 * 1000;
const INITIAL_SOCKET_TIMEOUT_MS = 15 * 1000;
const QUEUE_HEARTBEAT_MS = 15 * 1000;
const ASSIGNMENT_HANDOFF_TIMEOUT_MS = 15 * 1000;
const DEFAULT_ICE_SERVERS = Object.freeze([{ urls: "stun:stun.cloudflare.com:3478" }]);
const MAX_ICE_SERVERS = 8;
const MAX_ICE_URLS = 8;
const MAX_ICE_TEXT = 512;

function validIceUrl(value) {
    if (typeof value !== "string" || value.length === 0 || value.length > 256 || /[\u0000-\u0020\u007f]/.test(value)) return false;
    const match = /^(stun|turn|turns):(stun\.cloudflare\.com|turn\.cloudflare\.com)(?::([0-9]{1,5}))?(?:\?transport=(udp|tcp))?$/.exec(value);
    if (!match) return false;
    const port = match[3] === undefined ? null : Number(match[3]);
    if (port === 53 || (port !== null && (port < 1 || port > 65535))) return false;
    return match[1] === "stun" ? match[2] === "stun.cloudflare.com" && match[4] === undefined : match[2] === "turn.cloudflare.com";
}

function validatedIceConfiguration(message) {
    if (!Array.isArray(message?.iceServers) || message.iceServers.length === 0 || message.iceServers.length > MAX_ICE_SERVERS) return null;
    const servers = [];
    let hasTurn = false;
    for (const item of message.iceServers) {
        if (!item || typeof item !== "object" || Array.isArray(item)) return null;
        const raw = typeof item.urls === "string" ? [item.urls] : item.urls;
        if (!Array.isArray(raw) || raw.length === 0 || raw.length > MAX_ICE_URLS || !raw.every(validIceUrl)) return null;
        const turn = raw.some(url => url.startsWith("turn:") || url.startsWith("turns:"));
        if (turn) {
            if (typeof item.username !== "string" || item.username.length === 0 || item.username.length > MAX_ICE_TEXT ||
                typeof item.credential !== "string" || item.credential.length === 0 || item.credential.length > MAX_ICE_TEXT) return null;
            if (Object.keys(item).some(key => !["urls", "username", "credential"].includes(key))) return null;
            servers.push({ urls: Array.isArray(item.urls) ? [...raw] : raw[0], username: item.username, credential: item.credential });
            hasTurn = true;
        } else {
            if (Object.keys(item).some(key => key !== "urls")) return null;
            servers.push({ urls: Array.isArray(item.urls) ? [...raw] : raw[0] });
        }
    }
    if (hasTurn) {
        if (!Number.isSafeInteger(message.turnExpiresAt) || message.turnExpiresAt <= Date.now() || message.turnExpiresAt > Date.now() + 21660 * 1000) return null;
    } else if (message.turnExpiresAt !== null) return null;
    return { iceServers: servers, turnExpiresAt: message.turnExpiresAt, hasTurn };
}

async function recordCandidatePair(session, peer) {
    if (peer.__ghostStatsRecorded) return;
    try {
        const stats = await peer.getStats();
        if (!isCurrent(session) || peer.__ghostStatsRecorded) return;
        let pair = null;
        stats.forEach(report => {
            if (report.type === "transport" && report.selectedCandidatePairId) pair = stats.get(report.selectedCandidatePairId) || pair;
            if (!pair && report.type === "candidate-pair" && report.state === "succeeded" && (report.nominated || report.selected)) pair = report;
        });
        if (!pair) return;
        const local = stats.get(pair.localCandidateId);
        const remote = stats.get(pair.remoteCandidateId);
        const types = [local?.candidateType, remote?.candidateType];
        const kind = types.includes("relay") ? "relay" : types.some(type => type === "srflx" || type === "prflx") ? "srflx" : "host";
        peer.__ghostStatsRecorded = true;
        const index = kind === "relay" ? 10 : kind === "srflx" ? 9 : 8;
        session.telemetry[index]++;
        if (kind === "relay") session.telemetry[6]++;
    } catch (_) { /* Metrics must never affect connectivity. */ }
}

function peerConfiguration(session) {
    const turnUsable = session.iceHasTurn && Number.isSafeInteger(session.turnExpiresAt) &&
        session.turnExpiresAt > Date.now() + 10 * 60 * 1000;
    if (!turnUsable) session.telemetry[7]++;
    return { iceServers: turnUsable ? session.iceServers : DEFAULT_ICE_SERVERS };
}

function current(id) {
    return networks.get(id) || null;
}

function isCurrent(session) {
    return networks.get(session.id) === session;
}

function closeSession(session, code, reason) {
    window.clearTimeout(session.timeout);
    window.clearInterval(session.heartbeat);
    session.channel?.close();
    session.peer?.close();
    session.ws?.close(code, reason);
}

function fail(session, error) {
    if (current(session.id) !== session || session.status === 2) return;
    session.status = 2;
    session.error = error instanceof Error ? error.message : String(error);
    // WebSocket.close permits 1000 or application codes 3000-4999.
    closeSession(session, 4000, "connection failed");
}

function sameRound(session, epoch, round) {
    return isCurrent(session) && session.epoch === epoch && session.round === round && session.closedRound !== `${epoch}:${round}`;
}

function sameEpochTransport(session, epoch) {
    return isCurrent(session) && session.epoch === epoch && !(session.closedRound?.startsWith(`${epoch}:`));
}

function lobbySendSignal(session, to, data, epoch = session.epoch, round = session.round) {
    if (isCurrent(session) && session.ws.readyState === WebSocket.OPEN) {
        try { session.ws.send(JSON.stringify({ type: "signal", epoch, round, to, data })); }
        catch (error) { fail(session, error); }
    }
}

function lobbyBindChannel(session, peerId, channel, epoch, round) {
    if (!sameRound(session, epoch, round)) { channel.close(); return; }
    if (session.channels.has(peerId)) return fail(session, "duplicate lobby data channel");
    channel.binaryType = "arraybuffer";
    session.channels.set(peerId, channel);
    channel.onmessage = ({ data }) => {
        if (!sameEpochTransport(session, epoch) || !(data instanceof ArrayBuffer) || data.byteLength > MAX_PACKET_BYTES + 8 || data.byteLength < 8) { session.telemetry[2]++; return; }
        const bytes = new Uint8Array(data);
        const view = new DataView(data);
        const packetEpoch = view.getUint32(0, false);
        const packetRound = view.getUint32(4, false);
        if (packetEpoch !== session.epoch || packetRound !== session.round) { session.telemetry[3]++; return; }
        if (session.inbox.length >= MAX_QUEUED_PACKETS) { session.telemetry[2]++; return fail(session, "lobby receive queue overflow"); }
        session.telemetry[1]++;
        session.inbox.push({ epoch: packetEpoch, from: peerId, packet: bytes.slice(8) });
    };
    channel.onclose = () => { if (sameEpochTransport(session, epoch) && session.status === 1) fail(session, "lobby peer disconnected"); };
    channel.onerror = () => { if (sameEpochTransport(session, epoch)) fail(session, "lobby peer data channel failed"); };
    channel.onopen = () => {
        if (!sameEpochTransport(session, epoch)) return channel.close();
        session.openPeers.add(peerId);
        if (session.openPeers.size === session.roster.length - 1) {
            session.status = 1;
            window.clearTimeout(session.timeout);
        }
    };
}

async function lobbyCreatePeer(session, peerId, offerer, epoch, round) {
    if (!sameRound(session, epoch, round)) return;
    const peer = new RTCPeerConnection(peerConfiguration(session));
    session.peers.set(peerId, peer);
    session.pendingIce.set(peerId, []);
    peer.onicecandidate = ({ candidate }) => { if (sameRound(session, epoch, round)) lobbySendSignal(session, peerId, { type: "ice", candidate }, epoch, round); };
    peer.ondatachannel = ({ channel }) => lobbyBindChannel(session, peerId, channel, epoch, round);
    peer.onconnectionstatechange = () => {
        if (!sameEpochTransport(session, epoch)) return;
        if (peer.connectionState === "connected") recordCandidatePair(session, peer);
        if (peer.connectionState === "failed") fail(session, "lobby WebRTC connection failed");
    };
    if (offerer) {
        lobbyBindChannel(session, peerId, peer.createDataChannel("ggrs", { ordered: false, maxRetransmits: 0 }), epoch, round);
        await peer.setLocalDescription(await peer.createOffer());
        if (sameRound(session, epoch, round)) lobbySendSignal(session, peerId, { type: "offer", sdp: peer.localDescription.sdp }, epoch, round);
    }
}

function validLobbySignal(session, message, start) {
    return message && typeof message === "object" && Number.isInteger(message.epoch) &&
        message.epoch === start.epoch && message.round === start.round && typeof message.from === "string" &&
        start.roster.some(entry => entry.playerId === message.from) && message.from !== session.localPlayerId &&
        message.data && typeof message.data === "object" && ["offer", "answer", "ice"].includes(message.data.type);
}

async function lobbyHandleSignal(session, message) {
    const epoch = session.epoch, round = session.round, from = message.from;
    if (!validLobbySignal(session, message, { epoch, round, roster: session.roster })) throw new Error("invalid lobby signal source");
    const peer = session.peers.get(from);
    if (!peer) throw new Error("lobby signal before peer setup");
    const data = message.data;
    if (data.type === "offer") {
        if (session.localPlayerId < from) throw new Error("unexpected lobby offer");
        await peer.setRemoteDescription({ type: "offer", sdp: data.sdp });
        if (!sameRound(session, epoch, round)) return;
        for (const candidate of session.pendingIce.get(from).splice(0)) if (candidate) await peer.addIceCandidate(candidate);
        await peer.setLocalDescription(await peer.createAnswer());
        if (sameRound(session, epoch, round)) lobbySendSignal(session, from, { type: "answer", sdp: peer.localDescription.sdp }, epoch, round);
    } else if (data.type === "answer") {
        if (session.localPlayerId > from) throw new Error("unexpected lobby answer");
        await peer.setRemoteDescription({ type: "answer", sdp: data.sdp });
        if (!sameRound(session, epoch, round)) return;
        for (const candidate of session.pendingIce.get(from).splice(0)) if (candidate) await peer.addIceCandidate(candidate);
    } else if (data.type === "ice") {
        if (peer.remoteDescription) { if (data.candidate) await peer.addIceCandidate(data.candidate); }
        else session.pendingIce.get(from).push(data.candidate);
    }
}

function validLobbyStart(session, message) {
    if (message.protocol !== 3 || !Number.isInteger(message.epoch) || message.epoch < 0 || message.epoch > 0xffffffff ||
        !Number.isInteger(message.round) || message.round < 0 || message.round > 0xffffffff || !/^[0-9a-f]{32}$/.test(message.seed) ||
        !Array.isArray(message.roster) || message.roster.length !== session.capacity ||
        !Number.isInteger(message.matchGeneration ?? 0) || (message.matchGeneration ?? 0) < 0 || (message.matchGeneration ?? 0) > 0xffffffff) return null;
    const roster = [...message.roster].sort((a,b) => a.playerId.localeCompare(b.playerId));
    if (roster.some((entry,index) => entry.index !== index || !/^[0-9a-f]{32}$/.test(entry.playerId) ||
        !Number.isSafeInteger(entry.score) || entry.score < 0 || entry.score > 0xffffffff) ||
        !roster.some(entry => entry.playerId === session.localPlayerId)) return null;
    return { ...message, roster, matchGeneration: message.matchGeneration ?? session.matchGeneration };
}

function closeLobbyRound(session, epoch, round) {
    if (session.epoch !== epoch || session.round !== round || session.closedRound === `${epoch}:${round}`) return false;
    session.closedRound = `${epoch}:${round}`;
    session.status = 0;
    for (const channel of session.channels.values()) channel.close();
    for (const peer of session.peers.values()) peer.close();
    session.channels.clear();
    session.peers.clear();
    session.pendingIce.clear();
    session.openPeers.clear();
    session.inbox.length = 0;
    return true;
}

async function installLobbyStart(session, start, bufferedSignals = []) {
    session.roster = start.roster;
    session.seed = start.seed;
    session.epoch = start.epoch;
    session.round = start.round;
    session.matchGeneration = start.matchGeneration;
    session.status = 0;
    session.closedRound = null;
    window.clearTimeout(session.timeout);
    session.timeout = window.setTimeout(() => fail(session, "lobby WebRTC timed out"), MATCHMAKING_TIMEOUT_MS);
    const epoch = session.epoch, round = session.round;
    for (const entry of session.roster) if (entry.playerId !== session.localPlayerId) {
        await lobbyCreatePeer(session, entry.playerId, session.localPlayerId < entry.playerId, epoch, round);
    }
    for (const signal of bufferedSignals) {
        if (!sameRound(session, epoch, round)) return;
        await lobbyHandleSignal(session, signal);
    }
}

function connectLobbyInternal(baseUrl, room, mode, capacity, profileName, paletteId, cosmeticId, assignment = null, existingId = 0) {
    const endpoint = (baseUrl || `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/lobby`).replace(/\/match\/?$/, "/lobby").replace(/\/queue\/?$/, "/lobby");
    const modeName = mode === 0 ? "duel" : "deathmatch";
    const identityKey = `ghost-lobby-v3:${room}`;
    let credentials = null;
    if (!assignment) {
        try { credentials = JSON.parse(sessionStorage.getItem(identityKey) || "null"); } catch (_) {}
    }
    const reconnect = credentials && /^[0-9a-f]{32}$/.test(credentials.playerId) && /^[0-9a-f]{32}$/.test(credentials.reconnectToken)
        ? `&playerId=${credentials.playerId}&reconnectToken=${credentials.reconnectToken}` : "";
    const handoff = assignment ? `&queueTicket=${assignment.ticket}&queueExpires=${assignment.expiresAt}&queueToken=${assignment.token}` : "";
    const url = `${endpoint.replace(/\/$/, "")}/${encodeURIComponent(room)}?protocol=3&mode=${modeName}&capacity=${capacity}${reconnect}${handoff}`;
    const ws = new WebSocket(url);
    const id = existingId || nextTransportId++ || nextTransportId++;
    const session = { id, ws, identityKey, status: 0, error: "", lobby: true, assignmentHandoff: !!assignment, mode, capacity, inbox: [], peers: new Map(), channels: new Map(), pendingIce: new Map(), openPeers: new Set(), roster: [], localPlayerId: "", seed: "", epoch: 0, round: 0, matchGeneration: 0, pendingStart: null, pendingSignals: [], closedRound: null, control: [], signalChain: Promise.resolve(), timeout: 0, heartbeat: 0, queuePhase: assignment ? 4 : 0, queueCount: 0, profileName, paletteId, cosmeticId, iceServers: DEFAULT_ICE_SERVERS, turnExpiresAt: null, iceHasTurn: false, telemetry: [0,0,0,0,reconnect ? 1 : 0,0,0,0,0,0,0] };
    networks.set(id, session);
    session.timeout = window.setTimeout(() => fail(session, assignment ? "assignment handoff timed out" : "lobby matchmaking timed out"), assignment ? ASSIGNMENT_HANDOFF_TIMEOUT_MS : MATCHMAKING_TIMEOUT_MS);
    ws.onopen = () => {};
    ws.onmessage = ({ data }) => {
        if (!isCurrent(session) || typeof data !== "string" || data.length > 16384) return;
        session.signalChain = session.signalChain.then(async () => {
            const message = JSON.parse(data);
            if (!message || typeof message !== "object" || typeof message.type !== "string") throw new Error("invalid lobby message");
            if (message.type === "welcome") {
                if (message.protocol !== 3 || !/^[0-9a-f]{32}$/.test(message.playerId) || !/^[0-9a-f]{32}$/.test(message.reconnectToken)) throw new Error("invalid lobby welcome");
                const ice = validatedIceConfiguration(message);
                session.iceServers = ice?.iceServers || DEFAULT_ICE_SERVERS;
                session.turnExpiresAt = ice?.turnExpiresAt ?? null;
                session.iceHasTurn = ice?.hasTurn ?? false;
                session.localPlayerId = message.playerId;
                if (session.assignmentHandoff) {
                    session.assignmentHandoff = false;
                    window.clearTimeout(session.timeout);
                    session.timeout = window.setTimeout(() => fail(session, "lobby WebRTC timed out"), MATCHMAKING_TIMEOUT_MS);
                }
                sessionStorage.setItem(identityKey, JSON.stringify({ playerId: message.playerId, reconnectToken: message.reconnectToken }));
                session.ws.send(JSON.stringify({ type: "profile", name: profileName, paletteId, cosmeticId }));
                session.ws.send(JSON.stringify({ type: "ready" }));
            } else if (message.type === "start") {
                const start = validLobbyStart(session, message);
                if (!start) throw new Error("invalid lobby start");
                const next = [start.epoch, start.round], active = [session.epoch, session.round];
                if (session.roster.length && next[0] === active[0] && next[1] === active[1]) return;
                if (session.roster.length && (next[0] < active[0] || (next[0] === active[0] && next[1] < active[1]))) return;
                if (!session.roster.length) await installLobbyStart(session, start);
                else {
                    if (session.pendingStart) {
                        const pending = [session.pendingStart.epoch, session.pendingStart.round];
                        if (next[0] === pending[0] && next[1] === pending[1]) return;
                        throw new Error("multiple pending lobby starts");
                    }
                    session.pendingStart = start;
                    session.pendingSignals.length = 0;
                }
            } else if (message.type === "signal") {
                if (session.pendingStart && message.epoch === session.pendingStart.epoch && message.round === session.pendingStart.round) {
                    if (!validLobbySignal(session, message, session.pendingStart)) throw new Error("invalid pending lobby signal");
                    if (session.pendingSignals.length >= MAX_QUEUED_PACKETS) throw new Error("pending lobby signal overflow");
                    session.pendingSignals.push(message);
                    return;
                }
                if (!Number.isInteger(message.epoch) || message.epoch !== session.epoch || message.round !== session.round) return;
                await lobbyHandleSignal(session, message);
            } else if (["rematch_pending", "rematch_accepted", "rematch_denied", "match_exit", "match_over", "requeue"].includes(message.type)) {
                if (session.control.length >= 32) session.control.shift();
                session.control.push(message);
            } else if (["round_commit", "round_abort", "presence", "status", "profile_accepted", "report_ack", "pong"].includes(message.type)) {
                return;
            } else if (message.type === "error") {
                throw new Error(typeof message.error === "string" ? message.error : "lobby error");
            } else throw new Error("unsupported lobby message");
        }).catch(error => fail(session, error));
    };
    ws.onerror = () => fail(session, "could not reach lobby service");
    ws.onclose = () => { if (isCurrent(session) && session.status !== 2) fail(session, "lobby service disconnected"); };
    return id;
}

export function cloudflare_connect_lobby(baseUrl, room, mode, capacity, profileName, paletteId, cosmeticId) {
    return connectLobbyInternal(baseUrl, room, mode, capacity, profileName, paletteId, cosmeticId);
}

function validAssignment(message, ticket) {
    if (!message || typeof message !== "object" || Array.isArray(message)) return false;
    if (!Object.keys(message).every(key => ["type","protocol","room","mode","capacity","ticket","expiresAt","token"].includes(key))) return false;
    if (message.type !== "assigned" || message.protocol !== 4 || message.ticket !== ticket || !/^[0-9a-f]{32}$/.test(message.ticket)) return false;
    if (!/^q4_[0-9a-f]{32}$/.test(message.room) || !/^[0-9a-f]{64}$/.test(message.token)) return false;
    if (!Number.isSafeInteger(message.expiresAt) || message.expiresAt <= Date.now() || message.expiresAt > Date.now() + 60_000) return false;
    return (message.mode === "duel" && message.capacity === 2) ||
        (message.mode === "deathmatch" && Number.isInteger(message.capacity) && message.capacity >= 3 && message.capacity <= 8);
}

function queueStatus(session, message) {
    if (!message || typeof message !== "object" || Array.isArray(message) || message.type !== "status" ||
        !Object.keys(message).every(key => ["type","status","count","votes","votesRequired","deadline","voted"].includes(key))) return false;
    if (message.status === "searching" && Object.keys(message).length === 2) { session.queuePhase = 1; return true; }
    if (message.status === "holding_for_third" && Object.keys(message).length === 2) { session.queuePhase = 2; return true; }
    if (message.status === "staging" && Object.keys(message).length === 7 && Number.isInteger(message.count) && message.count >= 3 && message.count <= 8 &&
        Number.isInteger(message.votes) && message.votes >= 0 && message.votes <= message.count &&
        Number.isInteger(message.votesRequired) && message.votesRequired === Math.floor(message.count / 2) + 1 &&
        Number.isSafeInteger(message.deadline) && message.deadline > 0 && typeof message.voted === "boolean" && (!message.voted || message.votes > 0)) {
        session.queuePhase = 3;
        session.queueCount = message.count;
        session.queueVotes = message.votes;
        session.queueVotesRequired = message.votesRequired;
        session.queueDeadline = message.deadline;
        session.queueVoted = message.voted;
        return true;
    }
    return false;
}

export function cloudflare_connect_queue(baseUrl, compatibilityRoom, preference, profileName, paletteId, cosmeticId) {
    const endpoint = (baseUrl || `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/queue`).replace(/\/match\/?$/, "/queue").replace(/\/lobby\/?$/, "/queue");
    const url = `${endpoint.replace(/\/$/, "")}/${encodeURIComponent(compatibilityRoom)}?protocol=4&preference=${encodeURIComponent(preference)}`;
    const ws = new WebSocket(url);
    const id = nextTransportId++ || nextTransportId++;
    const session = { id, ws, queue: true, status: 0, error: "", queuePhase: 1, queueCount: 0, queueVotes: 0, queueVotesRequired: 0, queueDeadline: 0, queueVoted: false, ticket: "", control: [], inbox: [], roster: [], channels: new Map(), peers: new Map(), pendingIce: new Map(), openPeers: new Set(), timeout: 0, heartbeat: 0, signalChain: Promise.resolve(), telemetry: [0,0,0,0,0,0,0,0,0,0,0] };
    networks.set(id, session);
    session.timeout = window.setTimeout(() => fail(session, "could not open public queue"), INITIAL_SOCKET_TIMEOUT_MS);
    ws.onopen = () => {
        if (!isCurrent(session)) return;
        session.heartbeat = window.setInterval(() => {
            if (isCurrent(session) && ws.readyState === WebSocket.OPEN) {
                try { ws.send(JSON.stringify({ type: "heartbeat" })); } catch (error) { fail(session, error); }
            }
        }, QUEUE_HEARTBEAT_MS);
    };
    ws.onmessage = ({ data }) => {
        if (!isCurrent(session) || typeof data !== "string" || data.length > 2048) return fail(session, "invalid queue message");
        session.signalChain = session.signalChain.then(() => {
            const message = JSON.parse(data);
            if (message?.type === "queued") {
                if (message.protocol !== 4 || !/^[0-9a-f]{32}$/.test(message.ticket) || message.preference !== preference || !Object.keys(message).every(key => ["type","protocol","ticket","preference"].includes(key))) throw new Error("invalid queue acknowledgement");
                session.ticket = message.ticket;
                session.queuePhase = 1;
                window.clearTimeout(session.timeout);
                session.timeout = 0;
                return;
            }
            if (message?.type === "status" && queueStatus(session, message)) return;
            if (message?.type === "heartbeat_ack") return;
            if (message?.type === "assigned") {
                if (!validAssignment(message, session.ticket)) throw new Error("invalid queue assignment");
                session.queuePhase = 4;
                session.handingOff = true;
                window.clearInterval(session.heartbeat);
                networks.delete(id);
                ws.close(1000, "assignment accepted");
                const mode = message.mode === "duel" ? 0 : 1;
                connectLobbyInternal(baseUrl, message.room, mode, message.capacity, profileName, paletteId, cosmeticId, message, id);
                return;
            }
            if (message?.type === "error") throw new Error(typeof message.error === "string" ? message.error : "queue error");
            throw new Error("unsupported queue message");
        }).catch(error => fail(current(id) || session, error));
    };
    ws.onerror = () => { if (isCurrent(session)) fail(session, "could not reach public queue"); };
    ws.onclose = () => { if (isCurrent(session) && !session.handingOff && session.status !== 2) fail(session, "public queue disconnected"); };
    return id;
}

export function cloudflare_queue_phase(id) { return current(id)?.queuePhase ?? 0; }
export function cloudflare_queue_count(id) { return current(id)?.queueCount ?? 0; }
export function cloudflare_queue_votes(id) { return current(id)?.queueVotes ?? 0; }
export function cloudflare_queue_votes_required(id) { return current(id)?.queueVotesRequired ?? 0; }
export function cloudflare_queue_deadline(id) { return String(current(id)?.queueDeadline ?? 0); }
export function cloudflare_queue_voted(id) { return current(id)?.queueVoted === true; }
export function cloudflare_queue_vote_start(id) {
    const session = current(id);
    if (!session?.queue || session.queuePhase !== 3 || session.queueVoted || session.ws.readyState !== WebSocket.OPEN) return false;
    try { session.ws.send(JSON.stringify({ type: "vote_start" })); return true; }
    catch (error) { fail(session, error); return false; }
}
export function cloudflare_queue_withdraw_start_vote(id) {
    const session = current(id);
    if (!session?.queue || session.queuePhase !== 3 || !session.queueVoted || session.ws.readyState !== WebSocket.OPEN) return false;
    try { session.ws.send(JSON.stringify({ type: "withdraw_start_vote" })); return true; }
    catch (error) { fail(session, error); return false; }
}

export function cloudflare_lobby_local_id(id) { return current(id)?.localPlayerId || ""; }
export function cloudflare_lobby_mode(id) { return current(id)?.mode ?? 0; }
export function cloudflare_lobby_generation(id) { return current(id)?.matchGeneration ?? 0; }
export function cloudflare_lobby_control(id) { return current(id)?.control?.shift() ?? null; }
export function cloudflare_lobby_rematch_request(id, generation, nonce) { const session=current(id); if (!session || session.ws.readyState!==WebSocket.OPEN) return false; try { session.ws.send(JSON.stringify({type:"rematch_request",generation,nonce})); return true; } catch (error) { fail(session,error); return false; } }
export function cloudflare_lobby_rematch_response(id, generation, nonce, accept) { const session=current(id); if (!session || session.ws.readyState!==WebSocket.OPEN) return false; try { session.ws.send(JSON.stringify({type:"rematch_response",generation,nonce,accept})); return true; } catch (error) { fail(session,error); return false; } }
export function cloudflare_lobby_leave(id, requeue) { const session=current(id); if (!session || session.ws.readyState!==WebSocket.OPEN) return false; try { session.ws.send(JSON.stringify({type:requeue?"requeue":"leave"})); return true; } catch (error) { fail(session,error); return false; } }
export function cloudflare_lobby_seed(id) { return current(id)?.seed || ""; }
export function cloudflare_lobby_epoch(id) { return current(id)?.epoch ?? 0; }
export function cloudflare_lobby_round(id) { return current(id)?.round ?? 0; }
export function cloudflare_lobby_has_pending(id) { return current(id)?.pendingStart != null; }
export function cloudflare_lobby_pending_epoch(id) { return current(id)?.pendingStart?.epoch ?? 0; }
export function cloudflare_lobby_pending_round(id) { return current(id)?.pendingStart?.round ?? 0; }
export function cloudflare_lobby_roster_len(id) { return current(id)?.roster?.length ?? 0; }
export function cloudflare_lobby_roster_id(id, index) { return current(id)?.roster?.[index]?.playerId || ""; }
export function cloudflare_lobby_roster_score(id, index) { return current(id)?.roster?.[index]?.score ?? 0; }
export function cloudflare_lobby_send(id, epoch, to, packet) {
    const session = current(id);
    const channel = session?.channels?.get?.(to);
    if (session?.status !== 1 || epoch !== session.epoch || channel?.readyState !== "open" || packet.length > MAX_PACKET_BYTES || channel.bufferedAmount > MAX_BUFFERED_BYTES) {
        if (session) session.telemetry[2]++;
        return;
    }
    try {
        const framed = new Uint8Array(packet.length + 8);
        const view = new DataView(framed.buffer);
        view.setUint32(0, epoch, false);
        view.setUint32(4, session.round, false);
        framed.set(packet, 8);
        channel.send(framed);
        session.telemetry[0]++;
    } catch (error) { fail(session, error); }
}
export function cloudflare_lobby_receive(id) {
    const item = current(id)?.inbox?.shift();
    return item ? [item.epoch, item.from, item.packet] : null;
}
export function cloudflare_lobby_report(id, epoch, round, winners) {
    const session = current(id);
    if (!session || session.ws.readyState !== WebSocket.OPEN || epoch !== session.epoch || round !== session.round) return false;
    const winnerSet = new Set(Array.from(winners));
    const outcomes = session.roster.map((entry, index) => ({ playerId: entry.playerId, placement: winnerSet.has(entry.playerId) ? 1 : index + 1, scoreDelta: winnerSet.has(entry.playerId) ? 1 : 0 }));
    if (session.reported?.has(`${epoch}:${round}`)) return true;
    try {
        session.ws.send(JSON.stringify({ type: "report", epoch, round, outcomes }));
        (session.reported ??= new Set()).add(`${epoch}:${round}`);
        session.telemetry[5]++;
        return true;
    } catch (error) { fail(session, error); return false; }
}
export function cloudflare_lobby_close_epoch(id, epoch, round) {
    const session = current(id);
    if (!session || session.queue) return false;
    return closeLobbyRound(session, epoch, round);
}
export function cloudflare_lobby_promote_pending(id, oldEpoch, oldRound, pendingEpoch, pendingRound) {
    const session = current(id);
    if (!session || session.queue || !session.pendingStart || session.epoch !== oldEpoch || session.round !== oldRound ||
        session.pendingStart.epoch !== pendingEpoch || session.pendingStart.round !== pendingRound) return false;
    const start = session.pendingStart;
    if (pendingEpoch === oldEpoch) {
        const activeIds = session.roster.map(entry => entry.playerId).join(",");
        const pendingIds = start.roster.map(entry => entry.playerId).join(",");
        if (activeIds !== pendingIds || session.status !== 1) return false;
        session.roster = start.roster;
        session.seed = start.seed;
        session.round = start.round;
        session.matchGeneration = start.matchGeneration;
        session.inbox.length = 0;
        session.pendingStart = null;
        session.pendingSignals.length = 0;
        return true;
    }
    if (!closeLobbyRound(session, oldEpoch, oldRound)) return false;
    const signals = session.pendingSignals.splice(0);
    session.pendingStart = null;
    session.signalChain = session.signalChain.then(() => installLobbyStart(session, start, signals)).catch(error => fail(session, error));
    return true;
}
export function cloudflare_close_lobby(id) {
    const session = current(id);
    if (!session) return;
    networks.delete(id);
    window.clearTimeout(session.timeout);
    window.clearInterval(session.heartbeat);
    try { if (session.identityKey) sessionStorage.removeItem(session.identityKey); } catch (_) {}
    if (session.queue && session.ws.readyState === WebSocket.OPEN && session.ticket) {
        try { session.ws.send(JSON.stringify({ type: "cancel" })); } catch (_) {}
    }
    for (const channel of session.channels?.values?.() ?? []) channel.close();
    for (const peer of session.peers?.values?.() ?? []) peer.close();
    session.ws.close(1000, "client closed");
}

export function cloudflare_telemetry(id, counter) {
    const value = current(id)?.telemetry?.[counter] ?? 0;
    return BigInt(Number.isSafeInteger(value) && value >= 0 ? value : 0);
}
export function cloudflare_status(id) { const session=current(id); return session ? session.status : 0; }
export function cloudflare_error(id) { return current(id)?.error || "network connection failed"; }
