use crate::game::session::PlayerId;
use bevy::prelude::*;
#[cfg(target_arch = "wasm32")]
use bincode::Options;
use ggrs::{Message, NonBlockingSocket};

#[cfg(target_arch = "wasm32")]
const MAX_PACKET_BYTES: usize = 64 * 1024;

pub struct CloudflareNetPlugin;

impl Plugin for CloudflareNetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CloudflareSocket>()
            .init_resource::<NetworkTelemetry>();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MatchInfo {
    pub player_index: u8,
    pub seed: u64,
}

#[derive(Debug, Clone)]
pub struct LobbyMatchInfo {
    pub local_player: PlayerId,
    pub seed: u64,
    pub match_id: u128,
    pub epoch: u32,
    pub round: u32,
    pub roster: Vec<(PlayerId, usize)>,
}

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NetworkTelemetry {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_dropped: u64,
    pub stale_epoch_packets: u64,
    pub reconnects: u64,
    pub reports_sent: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    #[allow(dead_code)]
    Connecting,
    Ready,
    Failed(String),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum TransportMode {
    #[default]
    Legacy,
    Lobby,
}

#[derive(Resource, Default)]
pub struct CloudflareSocket {
    transport_id: u32,
    native_error: Option<String>,
    legacy_remote: Option<PlayerId>,
    mode: TransportMode,
    epoch: u32,
    owns_transport: bool,
}

impl CloudflareSocket {
    pub fn connect(&mut self, signaling_url: &str, room: &str) {
        self.close();

        if room.is_empty()
            || room.len() > 64
            || !room
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            self.native_error = Some("invalid matchmaking room".into());
            return;
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.transport_id = cloudflare_connect(signaling_url, room);
            self.legacy_remote = None;
            self.mode = TransportMode::Legacy;
            self.owns_transport = true;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = signaling_url;
            self.native_error = Some("online play is only supported in browser builds".into());
        }
    }

    pub fn connect_lobby(
        &mut self,
        signaling_url: &str,
        room: &str,
        mode: u32,
        capacity: u32,
        profile_name: &str,
        palette_id: u8,
        cosmetic_id: u8,
    ) {
        self.close();
        if room.is_empty()
            || room.len() > 64
            || !room
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            self.native_error = Some("invalid lobby room".into());
            return;
        }
        if !((mode == 0 && capacity == 2) || (mode == 1 && (2..=4).contains(&capacity))) {
            self.native_error = Some("invalid lobby mode or capacity".into());
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.transport_id = cloudflare_connect_lobby(
                signaling_url,
                room,
                mode,
                capacity,
                profile_name,
                palette_id as u32,
                cosmetic_id as u32,
            );
            self.mode = TransportMode::Lobby;
            self.epoch = 0;
            self.owns_transport = true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (
                signaling_url,
                mode,
                capacity,
                profile_name,
                palette_id,
                cosmetic_id,
            );
            self.native_error = Some("online play is only supported in browser builds".into());
        }
    }

    pub fn state(&self) -> ConnectionState {
        if let Some(error) = &self.native_error {
            return ConnectionState::Failed(error.clone());
        }
        if self.transport_id == 0 {
            return ConnectionState::Disconnected;
        }

        #[cfg(target_arch = "wasm32")]
        match cloudflare_status(self.transport_id) {
            1 => ConnectionState::Ready,
            2 => ConnectionState::Failed(cloudflare_error(self.transport_id)),
            _ => ConnectionState::Connecting,
        }

        #[cfg(not(target_arch = "wasm32"))]
        ConnectionState::Disconnected
    }

    pub fn match_info(&self) -> Option<MatchInfo> {
        if self.state() != ConnectionState::Ready {
            return None;
        }

        #[cfg(target_arch = "wasm32")]
        {
            let player_index = cloudflare_player_index(self.transport_id) as u8;
            if player_index > 1 {
                return None;
            }
            return Some(MatchInfo {
                player_index,
                seed: ((cloudflare_seed_high(self.transport_id) as u64) << 32)
                    | cloudflare_seed_low(self.transport_id) as u64,
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        None
    }

    pub fn lobby_match_info(&self) -> Option<LobbyMatchInfo> {
        #[cfg(target_arch = "wasm32")]
        {
            if self.mode != TransportMode::Lobby || self.state() != ConnectionState::Ready {
                return None;
            }
            let local_player = parse_player_id(&cloudflare_lobby_local_id(self.transport_id))?;
            let seed_hex = cloudflare_lobby_seed(self.transport_id);
            let match_id = u128::from_str_radix(&seed_hex, 16).ok()?;
            let seed = match_id as u64;
            let len = cloudflare_lobby_roster_len(self.transport_id) as usize;
            if len != 2 && len != 4 {
                return None;
            }
            let mut roster = Vec::with_capacity(len);
            for index in 0..len {
                roster.push((
                    parse_player_id(&cloudflare_lobby_roster_id(self.transport_id, index as u32))?,
                    index,
                ));
            }
            roster.sort_by_key(|entry| entry.0);
            if roster
                .iter()
                .enumerate()
                .any(|(handle, entry)| entry.1 != handle)
                || !roster.iter().any(|entry| entry.0 == local_player)
            {
                return None;
            }
            let epoch = cloudflare_lobby_epoch(self.transport_id);
            Some(LobbyMatchInfo {
                local_player,
                seed,
                match_id,
                epoch,
                round: cloudflare_lobby_round(self.transport_id),
                roster,
            })
        }
        #[cfg(not(target_arch = "wasm32"))]
        None
    }

    pub fn report_round(&self, epoch: u32, round: u32, winners: &[PlayerId]) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby && self.transport_id != 0 {
            let array = js_sys::Array::new();
            for winner in winners {
                array.push(&wasm_bindgen::JsValue::from_str(&format!(
                    "{:032x}",
                    winner.0
                )));
            }
            return cloudflare_lobby_report(self.transport_id, epoch, round, array.into());
        }
        false
    }

    pub fn lobby_epoch(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby && self.transport_id != 0 {
            return Some(cloudflare_lobby_epoch(self.transport_id));
        }
        None
    }

    pub fn lobby_round(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby && self.transport_id != 0 {
            return Some(cloudflare_lobby_round(self.transport_id));
        }
        None
    }

    pub fn set_epoch(&mut self, epoch: u32) {
        self.epoch = epoch;
    }

    pub fn telemetry(&self) -> NetworkTelemetry {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
            return NetworkTelemetry {
                packets_sent: cloudflare_telemetry(self.transport_id, 0),
                packets_received: cloudflare_telemetry(self.transport_id, 1),
                packets_dropped: cloudflare_telemetry(self.transport_id, 2),
                stale_epoch_packets: cloudflare_telemetry(self.transport_id, 3),
                reconnects: cloudflare_telemetry(self.transport_id, 4),
                reports_sent: cloudflare_telemetry(self.transport_id, 5),
            };
        }
        NetworkTelemetry::default()
    }

    pub fn close_epoch_transport(&self) {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby && self.transport_id != 0 {
            cloudflare_lobby_close_epoch(self.transport_id);
        }
    }

    /// Creates the epoch packet adapter while retaining the owning lobby
    /// control handle in the Bevy resource. Dropping a GGRS epoch adapter must
    /// never close the persistent control WebSocket.
    pub fn take_transport(&mut self) -> Self {
        let owns_transport = self.mode == TransportMode::Legacy;
        if owns_transport {
            self.owns_transport = false;
        }
        let transport_id = if owns_transport {
            std::mem::take(&mut self.transport_id)
        } else {
            self.transport_id
        };
        Self {
            transport_id,
            native_error: self.native_error.clone(),
            legacy_remote: self.legacy_remote,
            mode: self.mode,
            epoch: self.epoch,
            owns_transport,
        }
    }

    pub fn disconnect(&mut self) {
        self.close();
    }

    pub fn has_transport(&self) -> bool {
        self.transport_id != 0
    }

    fn close(&mut self) {
        if self.transport_id != 0 && self.owns_transport {
            #[cfg(target_arch = "wasm32")]
            match self.mode {
                TransportMode::Legacy => cloudflare_close(self.transport_id),
                TransportMode::Lobby => cloudflare_close_lobby(self.transport_id),
            }
        }
        self.transport_id = 0;
        self.native_error = None;
        self.legacy_remote = None;
        self.mode = TransportMode::Legacy;
        self.epoch = 0;
        self.owns_transport = false;
    }
}

impl Drop for CloudflareSocket {
    fn drop(&mut self) {
        self.close();
    }
}

fn parse_player_id(value: &str) -> Option<PlayerId> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u128::from_str_radix(value, 16).ok().map(PlayerId)
}

impl NonBlockingSocket<PlayerId> for CloudflareSocket {
    fn send_to(&mut self, message: &Message, _address: &PlayerId) {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
            if let Ok(packet) = codec().serialize(message) {
                match self.mode {
                    TransportMode::Legacy => cloudflare_send(self.transport_id, &packet),
                    TransportMode::Lobby => cloudflare_lobby_send(
                        self.transport_id,
                        self.epoch,
                        &format!("{:032x}", _address.0),
                        &packet,
                    ),
                }
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        let _ = message;
    }

    fn receive_all_messages(&mut self) -> Vec<(PlayerId, Message)> {
        #[cfg(target_arch = "wasm32")]
        {
            if self.mode == TransportMode::Lobby {
                let mut messages = Vec::new();
                loop {
                    let value = cloudflare_lobby_receive(self.transport_id);
                    if value.is_null() || value.is_undefined() {
                        break;
                    }
                    let array = js_sys::Array::from(&value);
                    if array.length() != 3 {
                        continue;
                    }
                    let Some(packet_epoch) = array.get(0).as_f64().map(|value| value as u32) else {
                        continue;
                    };
                    if packet_epoch != self.epoch {
                        continue;
                    }
                    let Some(from) = parse_player_id(&array.get(1).as_string().unwrap_or_default())
                    else {
                        continue;
                    };
                    let packet = js_sys::Uint8Array::new(&array.get(2)).to_vec();
                    if packet.len() <= MAX_PACKET_BYTES {
                        if let Ok(message) = codec().deserialize(&packet) {
                            messages.push((from, message));
                        }
                    }
                }
                return messages;
            }
            let Some(info) = self.match_info() else {
                return Vec::new();
            };
            let remote = *self.legacy_remote.get_or_insert_with(|| {
                crate::game::session::RoundBootstrap::duel(info.seed)
                    .roster
                    .into_iter()
                    .find(|entry| entry.handle != info.player_index as usize)
                    .expect("legacy duel has remote")
                    .player_id
            });
            let mut messages = Vec::new();

            loop {
                let value = cloudflare_receive(self.transport_id);
                if value.is_null() || value.is_undefined() {
                    break;
                }
                let packet = js_sys::Uint8Array::new(&value).to_vec();
                if packet.len() <= MAX_PACKET_BYTES {
                    if let Ok(message) = codec().deserialize(&packet) {
                        messages.push((remote, message));
                    }
                }
            }
            messages
        }

        #[cfg(not(target_arch = "wasm32"))]
        Vec::new()
    }
}

#[cfg(target_arch = "wasm32")]
fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .reject_trailing_bytes()
        .with_limit(MAX_PACKET_BYTES as u64)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
const networks = new Map();
let network = null; // legacy /match/v2 transport; protocol 3 uses networks
let nextTransportId = 1;
const MAX_PACKET_BYTES = 64 * 1024;
const MAX_QUEUED_PACKETS = 256;
const MAX_BUFFERED_BYTES = 1024 * 1024;
const MATCHMAKING_TIMEOUT_MS = 2 * 60 * 1000;

function current(id) {
    return networks.get(id) || (network?.id === id ? network : null);
}

function isCurrent(session) {
    return networks.get(session.id) === session || network === session;
}

function closeSession(session, code, reason) {
    window.clearTimeout(session.timeout);
    session.channel?.close();
    session.peer?.close();
    session.ws?.close(code, reason);
}

function fail(session, error) {
    if (current(session.id) !== session || session.status === 2) return;
    session.status = 2;
    session.error = error instanceof Error ? error.message : String(error);
    closeSession(session, 1011, "connection failed");
}

function sendSignal(session, type, data) {
    if (network !== session || session.status !== 0 || session.ws.readyState !== WebSocket.OPEN) return;
    try {
        session.ws.send(JSON.stringify({ type, data }));
    } catch (error) {
        fail(session, error);
    }
}

function bindDataChannel(session, channel) {
    if (network !== session || session.channel) return fail(session, "duplicate data channel");
    channel.binaryType = "arraybuffer";
    session.channel = channel;

    channel.onopen = () => {
        if (network !== session || session.channel !== channel || session.status !== 0) return;
        session.status = 1;
        window.clearTimeout(session.timeout);
        if (session.ws.readyState === WebSocket.OPEN) {
            try { session.ws.send(JSON.stringify({ type: "connected", data: null })); }
            catch (_) { /* The data channel is now authoritative. */ }
        }
    };
    channel.onclose = () => {
        if (network === session && session.channel === channel && session.status === 1) {
            fail(session, "peer disconnected");
        }
    };
    channel.onerror = () => fail(session, "peer data channel failed");
    channel.onmessage = ({ data }) => {
        if (network !== session || session.channel !== channel || session.status !== 1) return;
        if (!(data instanceof ArrayBuffer) || data.byteLength > MAX_PACKET_BYTES) return;
        if (session.inbox.length >= MAX_QUEUED_PACKETS) session.inbox.shift();
        session.inbox.push(new Uint8Array(data));
    };
}

async function setRemoteDescription(session, description) {
    await session.peer.setRemoteDescription(description);
    if (network !== session || session.status !== 0) return false;
    for (const candidate of session.pendingIce.splice(0)) {
        await session.peer.addIceCandidate(candidate);
        if (network !== session || session.status !== 0) return false;
    }
    return true;
}

async function handleSignal(session, message) {
    if (network !== session || session.status !== 0 || !message || typeof message !== "object") return;
    if (message.type === "waiting") return;
    if (message.type === "peer-left") throw new Error("peer left matchmaking");
    if (message.type === "error") throw new Error(message.data || "signaling failed");

    if (message.type === "matched") {
        if (
            session.peer ||
            (message.index !== 0 && message.index !== 1) ||
            typeof message.seed !== "string" ||
            !/^[0-9a-fA-F]{16}$/.test(message.seed)
        ) {
            throw new Error("invalid match assignment");
        }

        session.playerIndex = message.index;
        session.seedHigh = Number.parseInt(message.seed.slice(0, 8), 16) >>> 0;
        session.seedLow = Number.parseInt(message.seed.slice(8), 16) >>> 0;
        session.phase = "matched";
        session.peer = new RTCPeerConnection({
            iceServers: [{ urls: "stun:stun.cloudflare.com:3478" }],
        });
        const peer = session.peer;
        peer.onicecandidate = ({ candidate }) => sendSignal(session, "ice", candidate);
        peer.onconnectionstatechange = () => {
            if (network === session && session.peer === peer && peer.connectionState === "failed") {
                fail(session, "WebRTC connection failed");
            }
        };
        peer.ondatachannel = ({ channel }) => bindDataChannel(session, channel);

        if (message.index === 0) {
            bindDataChannel(session, peer.createDataChannel("ggrs", {
                ordered: false,
                maxRetransmits: 0,
            }));
            const offer = await peer.createOffer();
            if (network !== session || session.status !== 0) return;
            await peer.setLocalDescription(offer);
            if (network !== session || session.status !== 0) return;
            session.phase = "offer-sent";
            sendSignal(session, "offer", peer.localDescription);
        }
        return;
    }

    if (!session.peer) throw new Error("signal received before match assignment");
    if (message.type === "offer") {
        if (session.playerIndex !== 1 || session.phase !== "matched") throw new Error("unexpected offer");
        if (!await setRemoteDescription(session, message.data)) return;
        const answer = await session.peer.createAnswer();
        if (network !== session || session.status !== 0) return;
        await session.peer.setLocalDescription(answer);
        if (network !== session || session.status !== 0) return;
        session.phase = "answer-sent";
        sendSignal(session, "answer", session.peer.localDescription);
    } else if (message.type === "answer") {
        if (session.playerIndex !== 0 || session.phase !== "offer-sent") throw new Error("unexpected answer");
        if (await setRemoteDescription(session, message.data)) session.phase = "negotiated";
    } else if (message.type === "ice" && message.data) {
        if (session.peer.remoteDescription) await session.peer.addIceCandidate(message.data);
        else session.pendingIce.push(message.data);
    }
}

export function cloudflare_connect(baseUrl, room) {
    if (network) cloudflare_close(network.id);
    const endpoint = baseUrl || `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/match`;
    const ws = new WebSocket(`${endpoint.replace(/\/$/, "")}/${encodeURIComponent(room)}`);
    const id = nextTransportId++ || nextTransportId++;
    const session = {
        id,
        ws,
        peer: null,
        channel: null,
        inbox: [],
        pendingIce: [],
        signalChain: Promise.resolve(),
        status: 0,
        error: "",
        phase: "waiting",
        playerIndex: 0,
        seedHigh: 0,
        seedLow: 0,
        timeout: 0,
    };
    network = session;
    session.timeout = window.setTimeout(() => fail(session, "matchmaking timed out"), MATCHMAKING_TIMEOUT_MS);

    ws.onmessage = ({ data }) => {
        if (network !== session || session.status !== 0) return;
        if (typeof data !== "string" || data.length > 16384) return fail(session, "invalid signaling message");
        session.signalChain = session.signalChain
            .then(() => handleSignal(session, JSON.parse(data)))
            .catch((error) => fail(session, error));
    };
    ws.onerror = () => {
        if (session.status === 0) fail(session, "could not reach matchmaking service");
    };
    ws.onclose = ({ code, reason }) => {
        if (network === session && session.status === 0 && !(code === 1000 && reason === "connected")) {
            fail(session, "matchmaking service disconnected");
        }
    };
    return id;
}

function lobbySendSignal(session, to, data) {
    if (isCurrent(session) && session.ws.readyState === WebSocket.OPEN) {
        session.ws.send(JSON.stringify({ type: "signal", epoch: session.epoch, to, data }));
    }
}

function lobbyBindChannel(session, peerId, channel) {
    if (session.channels.has(peerId)) return fail(session, "duplicate lobby data channel");
    channel.binaryType = "arraybuffer";
    session.channels.set(peerId, channel);
    channel.onmessage = ({ data }) => {
        if (!isCurrent(session) || !(data instanceof ArrayBuffer) || data.byteLength > MAX_PACKET_BYTES + 4 || data.byteLength < 4) { session.telemetry[2]++; return; }
        const bytes = new Uint8Array(data); const epoch = new DataView(data).getUint32(0, false);
        if (epoch !== session.epoch) { session.telemetry[3]++; return; }
        if (session.inbox.length >= MAX_QUEUED_PACKETS) { session.telemetry[2]++; return fail(session, "lobby receive queue overflow"); }
        session.telemetry[1]++; session.inbox.push({ epoch, from: peerId, packet: bytes.slice(4) });
    };
    channel.onclose = () => { if (isCurrent(session) && session.status === 1) fail(session, "lobby peer disconnected"); };
    channel.onerror = () => fail(session, "lobby peer data channel failed");
    channel.onopen = () => {
        if (!isCurrent(session)) return;
        session.openPeers.add(peerId);
        if (session.openPeers.size === session.roster.length - 1) {
            session.status = 1;
            window.clearTimeout(session.timeout);
        }
    };
}

async function lobbyCreatePeer(session, peerId, offerer) {
    const peer = new RTCPeerConnection({ iceServers: [{ urls: "stun:stun.cloudflare.com:3478" }] });
    session.peers.set(peerId, peer);
    session.pendingIce.set(peerId, []);
    peer.onicecandidate = ({ candidate }) => lobbySendSignal(session, peerId, { type: "ice", candidate });
    peer.ondatachannel = ({ channel }) => lobbyBindChannel(session, peerId, channel);
    peer.onconnectionstatechange = () => { if (peer.connectionState === "failed") fail(session, "lobby WebRTC connection failed"); };
    if (offerer) {
        lobbyBindChannel(session, peerId, peer.createDataChannel("ggrs", { ordered: false, maxRetransmits: 0 }));
        await peer.setLocalDescription(await peer.createOffer());
        lobbySendSignal(session, peerId, { type: "offer", sdp: peer.localDescription.sdp });
    }
}

async function lobbyHandleSignal(session, message) {
    const from = message.from;
    if (!session.roster.some(entry => entry.playerId === from) || from === session.localPlayerId || message.epoch !== session.epoch) {
        throw new Error("invalid lobby signal source");
    }
    const peer = session.peers.get(from);
    if (!peer) throw new Error("lobby signal before peer setup");
    const data = message.data;
    if (data.type === "offer") {
        if (session.localPlayerId < from) throw new Error("unexpected lobby offer");
        await peer.setRemoteDescription({ type: "offer", sdp: data.sdp });
        for (const candidate of session.pendingIce.get(from).splice(0)) if (candidate) await peer.addIceCandidate(candidate);
        await peer.setLocalDescription(await peer.createAnswer());
        lobbySendSignal(session, from, { type: "answer", sdp: peer.localDescription.sdp });
    } else if (data.type === "answer") {
        if (session.localPlayerId > from) throw new Error("unexpected lobby answer");
        await peer.setRemoteDescription({ type: "answer", sdp: data.sdp });
        for (const candidate of session.pendingIce.get(from).splice(0)) if (candidate) await peer.addIceCandidate(candidate);
    } else if (data.type === "ice") {
        if (peer.remoteDescription) { if (data.candidate) await peer.addIceCandidate(data.candidate); }
        else session.pendingIce.get(from).push(data.candidate);
    }
}

export function cloudflare_connect_lobby(baseUrl, room, mode, capacity, profileName, paletteId, cosmeticId) {
    const endpoint = (baseUrl || `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/lobby`).replace(/\/match\/?$/, "/lobby");
    const modeName = mode === 0 ? "duel" : "deathmatch";
    const identityKey = `ghost-lobby-v3:${room}`;
    let credentials = null;
    try { credentials = JSON.parse(sessionStorage.getItem(identityKey) || "null"); } catch (_) {}
    const reconnect = credentials && /^[0-9a-f]{32}$/.test(credentials.playerId) && /^[0-9a-f]{32}$/.test(credentials.reconnectToken)
        ? `&playerId=${credentials.playerId}&reconnectToken=${credentials.reconnectToken}` : "";
    const url = `${endpoint.replace(/\/$/, "")}/${encodeURIComponent(room)}?protocol=3&mode=${modeName}&capacity=${capacity}${reconnect}`;
    const ws = new WebSocket(url);
    const id = nextTransportId++ || nextTransportId++;
    const session = { id, ws, status: 0, error: "", lobby: true, inbox: [], peers: new Map(), channels: new Map(), pendingIce: new Map(), openPeers: new Set(), roster: [], localPlayerId: "", seed: "", epoch: 0, round: 0, signalChain: Promise.resolve(), timeout: 0, profileName, paletteId, cosmeticId, telemetry: [0,0,0,0,reconnect ? 1 : 0,0] };
    networks.set(id, session);
    session.timeout = window.setTimeout(() => fail(session, "lobby matchmaking timed out"), MATCHMAKING_TIMEOUT_MS);
    ws.onopen = () => {};
    ws.onmessage = ({ data }) => {
        if (!isCurrent(session) || typeof data !== "string" || data.length > 16384) return;
        session.signalChain = session.signalChain.then(async () => {
            const message = JSON.parse(data);
            if (!message || typeof message !== "object" || typeof message.type !== "string") throw new Error("invalid lobby message");
            if (message.type === "welcome") {
                if (message.protocol !== 3 || !/^[0-9a-f]{32}$/.test(message.playerId) || !/^[0-9a-f]{32}$/.test(message.reconnectToken)) throw new Error("invalid lobby welcome");
                session.localPlayerId = message.playerId;
                sessionStorage.setItem(identityKey, JSON.stringify({ playerId: message.playerId, reconnectToken: message.reconnectToken }));
                session.ws.send(JSON.stringify({ type: "profile", name: profileName, paletteId, cosmeticId }));
                session.ws.send(JSON.stringify({ type: "ready" }));
            } else if (message.type === "start") {
                if (message.protocol !== 3 || !Number.isInteger(message.epoch) || message.epoch < 0 || !Number.isInteger(message.round) || message.round < 0 || !/^[0-9a-f]{32}$/.test(message.seed) || !Array.isArray(message.roster) || message.roster.length !== capacity) throw new Error("invalid lobby start");
                if (session.roster.length && message.epoch === session.epoch && message.round === session.round) return;
                if (session.roster.length && (message.epoch < session.epoch || (message.epoch === session.epoch && message.round < session.round))) return;
                const roster = [...message.roster].sort((a,b) => a.playerId.localeCompare(b.playerId));
                if (roster.some((entry,index) => entry.index !== index || !/^[0-9a-f]{32}$/.test(entry.playerId))) throw new Error("invalid lobby roster");
                session.roster = roster; session.seed = message.seed; session.epoch = message.epoch; session.round = message.round;
                for (const channel of session.channels.values()) channel.close();
                for (const peer of session.peers.values()) peer.close();
                session.channels.clear(); session.peers.clear(); session.pendingIce.clear(); session.openPeers.clear(); session.inbox.length = 0;
                for (const entry of roster) if (entry.playerId !== session.localPlayerId) await lobbyCreatePeer(session, entry.playerId, session.localPlayerId < entry.playerId);
            } else if (message.type === "signal") {
                if (!Number.isInteger(message.epoch) || message.epoch !== session.epoch) return;
                await lobbyHandleSignal(session, message);
            } else if (message.type === "round_commit" || message.type === "round_abort" || message.type === "presence" || message.type === "status" || message.type === "profile_accepted" || message.type === "report_ack" || message.type === "pong") {
                return; // validated type-specific state is consumed only when needed
            } else if (message.type === "error") {
                throw new Error(typeof message.error === "string" ? message.error : "lobby error");
            } else throw new Error("unsupported lobby message");
        }).catch(error => fail(session, error));
    };
    ws.onerror = () => fail(session, "could not reach lobby service");
    ws.onclose = () => { if (isCurrent(session) && session.status !== 2) fail(session, "lobby service disconnected"); };
    return id;
}

export function cloudflare_lobby_local_id(id) { return current(id)?.localPlayerId || ""; }
export function cloudflare_lobby_seed(id) { return current(id)?.seed || ""; }
export function cloudflare_lobby_epoch(id) { return current(id)?.epoch ?? 0; }
export function cloudflare_lobby_round(id) { return current(id)?.round ?? 0; }
export function cloudflare_lobby_roster_len(id) { return current(id)?.roster.length ?? 0; }
export function cloudflare_lobby_roster_id(id, index) { return current(id)?.roster[index]?.playerId || ""; }
export function cloudflare_lobby_send(id, epoch, to, packet) {
    const session = current(id); const channel = session?.channels.get(to);
    if (session?.status !== 1 || epoch !== session.epoch || channel?.readyState !== "open" || packet.length > MAX_PACKET_BYTES || channel.bufferedAmount > MAX_BUFFERED_BYTES) { if (session) session.telemetry[2]++; return; }
    try { const framed = new Uint8Array(packet.length + 4); new DataView(framed.buffer).setUint32(0, epoch, false); framed.set(packet, 4); channel.send(framed); session.telemetry[0]++; } catch (error) { fail(session, error); }
}
export function cloudflare_lobby_receive(id) {
    const item = current(id)?.inbox.shift();
    return item ? [item.epoch, item.from, item.packet] : null;
}
export function cloudflare_lobby_report(id, epoch, round, winners) {
    const session = current(id);
    if (!session || session.ws.readyState !== WebSocket.OPEN || epoch !== session.epoch) return false;
    const winnerSet = new Set(Array.from(winners));
    const outcomes = session.roster.map((entry, index) => ({
        playerId: entry.playerId,
        placement: winnerSet.has(entry.playerId) ? 1 : index + 1,
        scoreDelta: winnerSet.has(entry.playerId) ? 1 : 0,
    }));
    if (session.reported?.has(`${epoch}:${round}`)) return true;
    try { session.ws.send(JSON.stringify({ type: "report", epoch, round, outcomes })); (session.reported ??= new Set()).add(`${epoch}:${round}`); session.telemetry[5]++; return true; }
    catch (error) { fail(session, error); return false; }
}
export function cloudflare_lobby_close_epoch(id) {
    const session = current(id); if (!session) return;
    for (const channel of session.channels.values()) channel.close();
    for (const peer of session.peers.values()) peer.close();
    session.channels.clear(); session.peers.clear(); session.pendingIce.clear(); session.openPeers.clear(); session.inbox.length = 0;
    session.status = 0;
}
export function cloudflare_close_lobby(id) {
    const session = current(id); if (!session) return;
    networks.delete(id); window.clearTimeout(session.timeout);
    for (const channel of session.channels.values()) channel.close();
    for (const peer of session.peers.values()) peer.close();
    session.ws.close(1000, "client closed");
}

export function cloudflare_telemetry(id, counter) { return current(id)?.telemetry?.[counter] ?? 0; }
export function cloudflare_status(id) { return current(id)?.status ?? 2; }
export function cloudflare_error(id) { return current(id)?.error || "network connection failed"; }
export function cloudflare_player_index(id) { return current(id)?.playerIndex ?? 255; }
export function cloudflare_seed_high(id) { return current(id)?.seedHigh ?? 0; }
export function cloudflare_seed_low(id) { return current(id)?.seedLow ?? 0; }
export function cloudflare_send(id, packet) {
    const session = current(id);
    if (
        !session ||
        session.status !== 1 ||
        session.channel?.readyState !== "open" ||
        packet.length > MAX_PACKET_BYTES ||
        session.channel.bufferedAmount > MAX_BUFFERED_BYTES
    ) return;
    try { session.channel.send(packet); }
    catch (error) { fail(session, error); }
}
export function cloudflare_receive(id) { return current(id)?.inbox.shift() ?? null; }
export function cloudflare_close(id) {
    const session = current(id);
    if (!session) return;
    if (network?.id === id) network = null; else networks.delete(id);
    closeSession(session, 1000, "client closed");
}
"#)]
extern "C" {
    fn cloudflare_connect(base_url: &str, room: &str) -> u32;
    fn cloudflare_status(id: u32) -> u32;
    fn cloudflare_telemetry(id: u32, counter: u32) -> u64;
    fn cloudflare_error(id: u32) -> String;
    fn cloudflare_player_index(id: u32) -> u32;
    fn cloudflare_seed_high(id: u32) -> u32;
    fn cloudflare_seed_low(id: u32) -> u32;
    fn cloudflare_send(id: u32, packet: &[u8]);
    fn cloudflare_receive(id: u32) -> wasm_bindgen::JsValue;
    fn cloudflare_close(id: u32);
    fn cloudflare_connect_lobby(
        base_url: &str,
        room: &str,
        mode: u32,
        capacity: u32,
        profile_name: &str,
        palette_id: u32,
        cosmetic_id: u32,
    ) -> u32;
    fn cloudflare_lobby_local_id(id: u32) -> String;
    fn cloudflare_lobby_seed(id: u32) -> String;
    fn cloudflare_lobby_epoch(id: u32) -> u32;
    fn cloudflare_lobby_round(id: u32) -> u32;
    fn cloudflare_lobby_roster_len(id: u32) -> u32;
    fn cloudflare_lobby_roster_id(id: u32, index: u32) -> String;
    fn cloudflare_lobby_send(id: u32, epoch: u32, to: &str, packet: &[u8]);
    fn cloudflare_lobby_report(
        id: u32,
        epoch: u32,
        round: u32,
        winners: wasm_bindgen::JsValue,
    ) -> bool;
    fn cloudflare_lobby_receive(id: u32) -> wasm_bindgen::JsValue;
    fn cloudflare_lobby_close_epoch(id: u32);
    fn cloudflare_close_lobby(id: u32);
}
