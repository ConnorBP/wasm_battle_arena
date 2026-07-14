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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LobbyControlEvent {
    RematchPending {
        generation: u32,
        nonce: String,
        deadline_ms: u64,
        accepted: u8,
        required: u8,
    },
    RematchAccepted {
        generation: u32,
        nonce: String,
    },
    Ignored,
    ReturnToMenu {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueStatus {
    Searching,
    HoldingForThird,
    Staging {
        count: u8,
        votes: u8,
        votes_required: u8,
        deadline_ms: u64,
        voted: bool,
    },
    Assigned,
}

#[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
fn queue_status_from_scalars(
    phase: u32,
    count: u32,
    votes: u32,
    votes_required: u32,
    deadline_ms: u64,
    voted: bool,
) -> Option<QueueStatus> {
    match phase {
        1 => Some(QueueStatus::Searching),
        2 => Some(QueueStatus::HoldingForThird),
        3 if (3..=8).contains(&count)
            && votes <= count
            && votes_required == count / 2 + 1
            && deadline_ms > 0
            && deadline_ms <= 9_007_199_254_740_991
            && (!voted || votes > 0) =>
        {
            Some(QueueStatus::Staging {
                count: count as u8,
                votes: votes as u8,
                votes_required: votes_required as u8,
                deadline_ms,
                voted,
            })
        }
        4 => Some(QueueStatus::Assigned),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum QueueTimeoutPhase {
    SocketOpen,
    Waiting,
    AssignmentHandoff,
    WebRtc,
}

/// `None` is deliberate for ordinary queue age; liveness is instead enforced
/// by heartbeat watchdog. All transition/connectivity phases stay bounded.
#[cfg_attr(not(test), allow(dead_code))]
pub fn queue_timeout_ms(phase: QueueTimeoutPhase) -> Option<u64> {
    match phase {
        QueueTimeoutPhase::Waiting => None,
        QueueTimeoutPhase::SocketOpen | QueueTimeoutPhase::AssignmentHandoff => Some(15_000),
        QueueTimeoutPhase::WebRtc => Some(120_000),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct QueueAssignmentScalars {
    pub protocol: u32,
    pub room: String,
    pub mode: String,
    pub capacity: u32,
    pub ticket: String,
    pub expires_at: u64,
    pub token: String,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn valid_queue_assignment(
    value: &QueueAssignmentScalars,
    expected_ticket: &str,
    now: u64,
) -> bool {
    value.protocol == 4
        && value.ticket == expected_ticket
        && is_lower_hex(&value.ticket, 32)
        && value.room.len() == 35
        && value.room.starts_with("q4_")
        && is_lower_hex(&value.room[3..], 32)
        && is_lower_hex(&value.token, 64)
        && value.expires_at > now
        && value.expires_at <= now.saturating_add(60_000)
        && ((value.mode == "duel" && value.capacity == 2)
            || (value.mode == "deathmatch" && (3..=8).contains(&value.capacity)))
}

#[cfg_attr(not(test), allow(dead_code))]
fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub struct LobbyMatchInfo {
    pub local_player: PlayerId,
    pub mode: u32,
    pub seed: u64,
    pub match_id: u128,
    pub epoch: u32,
    pub round: u32,
    pub match_generation: u32,
    pub roster: Vec<(PlayerId, usize)>,
    /// Server-committed scores from the immutable start snapshot.
    pub scores: Vec<(PlayerId, u32)>,
}

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NetworkTelemetry {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_dropped: u64,
    pub stale_epoch_packets: u64,
    pub reconnects: u64,
    pub reports_sent: u64,
    /// Selected ICE candidate pairs using a TURN relay.
    pub relay_connections: u64,
    /// Peer connections created with STUN-only fallback configuration.
    pub stun_fallbacks: u64,
    pub candidate_pair_host: u64,
    pub candidate_pair_srflx: u64,
    pub candidate_pair_relay: u64,
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
    round: u32,
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

    pub fn connect_queue(
        &mut self,
        signaling_url: &str,
        compatibility_room: &str,
        preference: &str,
        profile_name: &str,
        palette_id: u8,
        cosmetic_id: u8,
    ) {
        self.close();
        if !matches!(preference, "any" | "duel" | "deathmatch") {
            self.native_error = Some("invalid public queue preference".into());
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.transport_id = cloudflare_connect_queue(
                signaling_url,
                compatibility_room,
                preference,
                profile_name,
                palette_id as u32,
                cosmetic_id as u32,
            );
            // Protocol 4 hands this same owning handle to an exact protocol-3
            // lobby. It is therefore a lobby transport once assigned.
            self.mode = TransportMode::Lobby;
            self.epoch = 0;
            self.round = 0;
            self.owns_transport = true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (
                signaling_url,
                compatibility_room,
                preference,
                profile_name,
                palette_id,
                cosmetic_id,
            );
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
        if !((mode == 0 && capacity == 2) || (mode == 1 && (3..=8).contains(&capacity))) {
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
            self.round = 0;
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

    pub fn is_waiting_in_queue(&self) -> bool {
        matches!(
            self.queue_status(),
            Some(
                QueueStatus::Searching | QueueStatus::HoldingForThird | QueueStatus::Staging { .. }
            )
        )
    }

    pub fn queue_status(&self) -> Option<QueueStatus> {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
            return queue_status_from_scalars(
                cloudflare_queue_phase(self.transport_id),
                cloudflare_queue_count(self.transport_id),
                cloudflare_queue_votes(self.transport_id),
                cloudflare_queue_votes_required(self.transport_id),
                cloudflare_queue_deadline(self.transport_id).parse().ok()?,
                cloudflare_queue_voted(self.transport_id),
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            None
        }
        #[cfg(target_arch = "wasm32")]
        None
    }

    /// Casts this queue ticket's start vote. Voting is deliberately unavailable
    /// before a dynamic LGS group reaches staging or after assignment.
    pub fn vote_start(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0
            && matches!(
                self.queue_status(),
                Some(QueueStatus::Staging { voted: false, .. })
            )
        {
            return cloudflare_queue_vote_start(self.transport_id);
        }
        false
    }

    /// Withdraws this queue ticket's vote while its LGS group is staging.
    pub fn withdraw_start_vote(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0
            && matches!(
                self.queue_status(),
                Some(QueueStatus::Staging { voted: true, .. })
            )
        {
            return cloudflare_queue_withdraw_start_vote(self.transport_id);
        }
        false
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
            let mode = cloudflare_lobby_mode(self.transport_id);
            if !((mode == 0 && len == 2) || (mode == 1 && (3..=8).contains(&len))) {
                return None;
            }
            let mut snapshot = Vec::with_capacity(len);
            for index in 0..len {
                snapshot.push((
                    parse_player_id(&cloudflare_lobby_roster_id(self.transport_id, index as u32))?,
                    cloudflare_lobby_roster_score(self.transport_id, index as u32),
                ));
            }
            snapshot.sort_by_key(|entry| entry.0);
            let roster = snapshot
                .iter()
                .enumerate()
                .map(|(handle, entry)| (entry.0, handle))
                .collect::<Vec<_>>();
            if !roster.iter().any(|entry| entry.0 == local_player) {
                return None;
            }
            let scores = snapshot.into_iter().collect();
            let epoch = cloudflare_lobby_epoch(self.transport_id);
            Some(LobbyMatchInfo {
                local_player,
                mode,
                seed,
                match_id,
                epoch,
                round: cloudflare_lobby_round(self.transport_id),
                match_generation: cloudflare_lobby_generation(self.transport_id),
                roster,
                scores,
            })
        }
        #[cfg(not(target_arch = "wasm32"))]
        None
    }

    pub fn request_rematch(&self, generation: u32, nonce: &str) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby && self.transport_id != 0 {
            return cloudflare_lobby_rematch_request(self.transport_id, generation, nonce);
        }
        false
    }

    pub fn respond_rematch(&self, generation: u32, nonce: &str, accept: bool) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby && self.transport_id != 0 {
            return cloudflare_lobby_rematch_response(self.transport_id, generation, nonce, accept);
        }
        false
    }

    pub fn leave_lobby(&self, requeue: bool) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby && self.transport_id != 0 {
            return cloudflare_lobby_leave(self.transport_id, requeue);
        }
        false
    }

    pub fn match_generation(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby && self.transport_id != 0 {
            return Some(cloudflare_lobby_generation(self.transport_id));
        }
        None
    }

    pub fn poll_control(&self) -> Option<LobbyControlEvent> {
        #[cfg(target_arch = "wasm32")]
        {
            let value = cloudflare_lobby_control(self.transport_id);
            if value.is_null() || value.is_undefined() {
                return None;
            }
            let kind = js_sys::Reflect::get(&value, &"type".into())
                .ok()?
                .as_string()?;
            let number = |key: &str| {
                js_sys::Reflect::get(&value, &key.into())
                    .ok()?
                    .as_f64()
                    .map(|v| v as u64)
            };
            return match kind.as_str() {
                "rematch_pending" => Some(LobbyControlEvent::RematchPending {
                    generation: number("generation")? as u32,
                    nonce: js_sys::Reflect::get(&value, &"nonce".into())
                        .ok()?
                        .as_string()?,
                    deadline_ms: number("deadline")?,
                    accepted: js_sys::Reflect::get(&value, &"accepted".into())
                        .ok()
                        .map(|v| js_sys::Array::from(&v).length() as u8)
                        .unwrap_or(0),
                    required: number("required")? as u8,
                }),
                "rematch_accepted" => Some(LobbyControlEvent::RematchAccepted {
                    generation: number("generation")? as u32,
                    nonce: js_sys::Reflect::get(&value, &"nonce".into())
                        .ok()?
                        .as_string()?,
                }),
                "rematch_denied" | "match_exit" => Some(LobbyControlEvent::ReturnToMenu {
                    reason: js_sys::Reflect::get(&value, &"reason".into())
                        .ok()
                        .and_then(|v| v.as_string())
                        .unwrap_or_else(|| "match ended".into()),
                }),
                _ => Some(LobbyControlEvent::Ignored),
            };
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

    pub fn set_epoch_round(&mut self, epoch: u32, round: u32) {
        self.epoch = epoch;
        self.round = round;
    }

    /// Returns the validated newer immutable start without exposing it as the
    /// active transport snapshot.
    pub fn pending_epoch_round(&self) -> Option<(u32, u32)> {
        Some((self.pending_epoch()?, self.pending_round()?))
    }

    pub fn pending_epoch(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby
            && self.transport_id != 0
            && cloudflare_lobby_has_pending(self.transport_id)
        {
            return Some(cloudflare_lobby_pending_epoch(self.transport_id));
        }
        None
    }

    pub fn pending_round(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby
            && self.transport_id != 0
            && cloudflare_lobby_has_pending(self.transport_id)
        {
            return Some(cloudflare_lobby_pending_round(self.transport_id));
        }
        None
    }

    /// Retires exactly the old round and starts peer formation for the pending
    /// immutable start. A stale cleanup cannot affect the promoted transport.
    pub fn promote_pending(&mut self, old_epoch: u32, old_round: u32) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            if self.mode == TransportMode::Lobby && self.transport_id != 0 {
                let Some((epoch, round)) = self.pending_epoch_round() else {
                    return false;
                };
                if cloudflare_lobby_promote_pending(
                    self.transport_id,
                    old_epoch,
                    old_round,
                    epoch,
                    round,
                ) {
                    self.epoch = epoch;
                    self.round = round;
                    return true;
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (old_epoch, old_round);
        false
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
                relay_connections: cloudflare_telemetry(self.transport_id, 6),
                stun_fallbacks: cloudflare_telemetry(self.transport_id, 7),
                candidate_pair_host: cloudflare_telemetry(self.transport_id, 8),
                candidate_pair_srflx: cloudflare_telemetry(self.transport_id, 9),
                candidate_pair_relay: cloudflare_telemetry(self.transport_id, 10),
            };
        }
        NetworkTelemetry::default()
    }

    pub fn close_epoch_transport(&self, epoch: u32, round: u32) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.mode == TransportMode::Lobby && self.transport_id != 0 {
            return cloudflare_lobby_close_epoch(self.transport_id, epoch, round);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (epoch, round);
        false
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
            round: self.round,
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
        self.round = 0;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment() -> QueueAssignmentScalars {
        QueueAssignmentScalars {
            protocol: 4,
            room: format!("q4_{}", "a".repeat(32)),
            mode: "deathmatch".into(),
            capacity: 5,
            ticket: "b".repeat(32),
            expires_at: 20_000,
            token: "c".repeat(64),
        }
    }

    #[test]
    fn coordinator_phases_skip_lobby_control_polling() {
        assert!(matches!(
            queue_status_from_scalars(1, 0, 0, 0, 0, false),
            Some(QueueStatus::Searching)
        ));
        assert!(matches!(
            queue_status_from_scalars(2, 0, 0, 0, 0, false),
            Some(QueueStatus::HoldingForThird)
        ));
        assert!(matches!(
            queue_status_from_scalars(3, 3, 0, 2, 40_000, false),
            Some(QueueStatus::Staging { .. })
        ));
        assert!(matches!(
            queue_status_from_scalars(4, 0, 0, 0, 0, false),
            Some(QueueStatus::Assigned)
        ));
    }

    #[test]
    fn queue_status_scalars_are_strict_and_bounded() {
        assert_eq!(
            queue_status_from_scalars(1, 0, 0, 0, 0, false),
            Some(QueueStatus::Searching)
        );
        assert_eq!(
            queue_status_from_scalars(2, 0, 0, 0, 0, false),
            Some(QueueStatus::HoldingForThird)
        );
        assert_eq!(
            queue_status_from_scalars(3, 4, 2, 3, 40_000, true),
            Some(QueueStatus::Staging {
                count: 4,
                votes: 2,
                votes_required: 3,
                deadline_ms: 40_000,
                voted: true,
            })
        );
        assert_eq!(queue_status_from_scalars(3, 2, 0, 2, 40_000, false), None);
        assert_eq!(queue_status_from_scalars(3, 9, 0, 5, 40_000, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 6, 3, 40_000, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 2, 2, 40_000, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 0, 3, 0, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 0, 3, u64::MAX, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 0, 3, 40_000, true), None);
        for count in 3..=8 {
            let required = count / 2 + 1;
            assert!(matches!(
                queue_status_from_scalars(3, count, required - 1, required, 40_000, false),
                Some(QueueStatus::Staging { count: parsed, votes_required: parsed_required, .. })
                    if parsed == count as u8 && parsed_required == required as u8
            ));
        }
        assert_eq!(
            queue_status_from_scalars(4, 0, 0, 0, 0, false),
            Some(QueueStatus::Assigned)
        );
    }

    #[test]
    fn ordinary_queue_wait_has_no_age_timeout_but_handoffs_are_bounded() {
        assert_eq!(queue_timeout_ms(QueueTimeoutPhase::Waiting), None);
        assert_eq!(
            queue_timeout_ms(QueueTimeoutPhase::SocketOpen),
            Some(15_000)
        );
        assert_eq!(
            queue_timeout_ms(QueueTimeoutPhase::AssignmentHandoff),
            Some(15_000)
        );
        assert_eq!(queue_timeout_ms(QueueTimeoutPhase::WebRtc), Some(120_000));
    }

    #[test]
    fn assignment_validation_rejects_tampering_expiry_and_unbounded_scalars() {
        let valid = assignment();
        assert!(valid_queue_assignment(&valid, &"b".repeat(32), 10_000));
        for invalid in [
            QueueAssignmentScalars {
                protocol: 3,
                ..valid.clone()
            },
            QueueAssignmentScalars {
                room: "q4_bad".into(),
                ..valid.clone()
            },
            QueueAssignmentScalars {
                capacity: 2,
                ..valid.clone()
            },
            QueueAssignmentScalars {
                token: "C".repeat(64),
                ..valid.clone()
            },
            QueueAssignmentScalars {
                expires_at: 10_000,
                ..valid.clone()
            },
            QueueAssignmentScalars {
                expires_at: 70_001,
                ..valid.clone()
            },
        ] {
            assert!(!valid_queue_assignment(&invalid, &"b".repeat(32), 10_000));
        }
        let duel = QueueAssignmentScalars {
            mode: "duel".into(),
            capacity: 2,
            ..valid
        };
        assert!(valid_queue_assignment(&duel, &"b".repeat(32), 10_000));
        assert!(!valid_queue_assignment(&duel, &"d".repeat(32), 10_000));
    }

    #[test]
    fn queue_connect_rejects_invalid_preference_before_transport() {
        let mut socket = CloudflareSocket::default();
        socket.connect_queue("", "battle-0-7-0", "surprise", "Ghost", 0, 0);
        assert_eq!(
            socket.state(),
            ConnectionState::Failed("invalid public queue preference".into())
        );
    }

    #[test]
    fn voting_is_safe_outside_browser_staging() {
        let socket = CloudflareSocket::default();
        assert!(!socket.vote_start());
        assert!(!socket.withdraw_start_vote());
    }

    #[test]
    fn wasm_u64_exports_use_javascript_bigint() {
        let source = include_str!("cloudflare_net.rs");
        assert!(source.contains("export function cloudflare_telemetry"));
        assert!(source
            .contains("return BigInt(Number.isSafeInteger(value) && value >= 0 ? value : 0);"));
        assert!(source.contains("fn cloudflare_telemetry(id: u32, counter: u32) -> u64;"));
    }

    #[test]
    fn browser_rollover_contract_is_two_phase_and_identity_guarded() {
        let source = include_str!("cloudflare_net.rs");
        assert!(source.contains("pendingStart: null, pendingSignals: []"));
        assert!(source.contains("session.pendingStart = start"));
        assert!(source.contains("session.pendingSignals.length >= MAX_QUEUED_PACKETS"));
        assert!(source.contains("export function cloudflare_lobby_promote_pending"));
        assert!(source.contains("session.epoch !== oldEpoch || session.round !== oldRound"));
        assert!(source.contains("if (!closeLobbyRound(session, oldEpoch, oldRound)) return false"));
        assert!(source.contains("installLobbyStart(session, start, signals)"));
    }

    #[test]
    fn public_client_source_has_no_obsolete_roster_size_parameter() {
        let source = include_str!("cloudflare_net.rs");
        for forbidden in [
            ["queue", "Target"].concat(),
            ["&tar", "get="].concat(),
            ["message.tar", "get"].concat(),
        ] {
            assert!(
                !source.contains(&forbidden),
                "obsolete public queue field remains"
            );
        }
        let networking = include_str!("game/networking.rs");
        assert!(!networking.contains(&["pub tar", "get:"].concat()));
        let gui = include_str!("game/gui.rs");
        assert!(!gui.contains(&["Target ", "ghosts"].concat()));
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

// The browser treats even Worker messages as untrusted. Return a fresh,
// minimal RTCConfiguration and never persist TURN material in browser storage.
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
    // A later epoch on a long-lived control socket cannot safely refresh
    // without a server-pushed protocol message. Do not start ICE with TURN
    // credentials inside the final ten minutes; reconnect obtains a fresh set.
    const turnUsable = session.iceHasTurn && Number.isSafeInteger(session.turnExpiresAt) &&
        session.turnExpiresAt > Date.now() + 10 * 60 * 1000;
    if (!turnUsable) session.telemetry[7]++;
    return { iceServers: turnUsable ? session.iceServers : DEFAULT_ICE_SERVERS };
}

function current(id) {
    return networks.get(id) || (network?.id === id ? network : null);
}

function isCurrent(session) {
    return networks.get(session.id) === session || network === session;
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
    // Browser WebSocket.close only permits 1000 or application codes 3000-4999.
    closeSession(session, 4000, "connection failed");
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
        const ice = validatedIceConfiguration(message);
        session.iceServers = ice?.iceServers || DEFAULT_ICE_SERVERS;
        session.turnExpiresAt = ice?.turnExpiresAt ?? null;
        session.iceHasTurn = ice?.hasTurn ?? false;
        session.peer = new RTCPeerConnection(peerConfiguration(session));
        const peer = session.peer;
        peer.onicecandidate = ({ candidate }) => sendSignal(session, "ice", candidate);
        peer.onconnectionstatechange = () => {
            if (network === session && session.peer === peer && peer.connectionState === "connected") recordCandidatePair(session, peer);
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
        iceServers: DEFAULT_ICE_SERVERS,
        turnExpiresAt: null,
        iceHasTurn: false,
        telemetry: [0,0,0,0,0,0,0,0,0,0,0],
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

function sameRound(session, epoch, round) {
    return isCurrent(session) && session.epoch === epoch && session.round === round &&
        session.closedRound !== `${epoch}:${round}`;
}

// Peer connections and data channels live for the roster epoch. Individual
// GGRS sessions are round-scoped by the 8-byte packet header below.
function sameEpochTransport(session, epoch) {
    return isCurrent(session) && session.epoch === epoch &&
        !(session.closedRound?.startsWith(`${epoch}:`));
}

function lobbySendSignal(session, to, data, epoch = session.epoch, round = session.round) {
    if (isCurrent(session) && session.ws.readyState === WebSocket.OPEN) {
        session.ws.send(JSON.stringify({ type: "signal", epoch, round, to, data }));
    }
}

function lobbyBindChannel(session, peerId, channel, epoch, round) {
    if (!sameRound(session, epoch, round)) { channel.close(); return; }
    if (session.channels.has(peerId)) return fail(session, "duplicate lobby data channel");
    channel.binaryType = "arraybuffer";
    session.channels.set(peerId, channel);
    channel.onmessage = ({ data }) => {
        if (!sameEpochTransport(session, epoch) || !(data instanceof ArrayBuffer) || data.byteLength > MAX_PACKET_BYTES + 8 || data.byteLength < 8) { session.telemetry[2]++; return; }
        const bytes = new Uint8Array(data); const view = new DataView(data); const packetEpoch = view.getUint32(0, false); const packetRound = view.getUint32(4, false);
        if (packetEpoch !== session.epoch || packetRound !== session.round) { session.telemetry[3]++; return; }
        if (session.inbox.length >= MAX_QUEUED_PACKETS) { session.telemetry[2]++; return fail(session, "lobby receive queue overflow"); }
        session.telemetry[1]++; session.inbox.push({ epoch: packetEpoch, from: peerId, packet: bytes.slice(8) });
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
        start.roster.some(entry => entry.playerId === message.from) &&
        message.from !== session.localPlayerId && message.data && typeof message.data === "object" &&
        ["offer", "answer", "ice"].includes(message.data.type);
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
    if (message.protocol !== 3 || !Number.isInteger(message.epoch) || message.epoch < 0 ||
        !Number.isInteger(message.round) || message.round < 0 || !/^[0-9a-f]{32}$/.test(message.seed) ||
        !Array.isArray(message.roster) || message.roster.length !== session.capacity ||
        !Number.isInteger(message.matchGeneration ?? 0) || (message.matchGeneration ?? 0) < 0) return null;
    const roster = [...message.roster].sort((a,b) => a.playerId.localeCompare(b.playerId));
    if (roster.some((entry,index) => entry.index !== index || !/^[0-9a-f]{32}$/.test(entry.playerId) ||
        !Number.isSafeInteger(entry.score) || entry.score < 0) ||
        !roster.some(entry => entry.playerId === session.localPlayerId)) return null;
    return { ...message, roster, matchGeneration: message.matchGeneration ?? session.matchGeneration };
}

function closeLobbyRound(session, epoch, round) {
    if (session.epoch !== epoch || session.round !== round || session.closedRound === `${epoch}:${round}`) return false;
    session.closedRound = `${epoch}:${round}`;
    session.status = 0;
    for (const channel of session.channels.values()) channel.close();
    for (const peer of session.peers.values()) peer.close();
    session.channels.clear(); session.peers.clear(); session.pendingIce.clear(); session.openPeers.clear(); session.inbox.length = 0;
    return true;
}

async function installLobbyStart(session, start, bufferedSignals = []) {
    session.roster = start.roster; session.seed = start.seed; session.epoch = start.epoch; session.round = start.round;
    session.matchGeneration = start.matchGeneration; session.status = 0; session.closedRound = null;
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
    // Queue tickets are one-use admission credentials, so an assigned handoff
    // never combines one with a stale reconnect identity.
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
                    // Signed admission succeeded. Peer formation remains
                    // independently bounded by the existing WebRTC timeout.
                    session.timeout = window.setTimeout(() => fail(session, "lobby WebRTC timed out"), MATCHMAKING_TIMEOUT_MS);
                }
                // Store identity only. TURN usernames/credentials remain solely
                // in this in-memory session and are refreshed by reconnect.
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
                    // Phase one: active GGRS and peers remain untouched until
                    // Rust OnExit cleanup explicitly promotes this snapshot.
                    session.pendingStart = start;
                    session.pendingSignals.length = 0;

                }
            } else if (message.type === "signal") {
                if (session.pendingStart && message.epoch === session.pendingStart.epoch &&
                    message.round === session.pendingStart.round) {
                    if (!validLobbySignal(session, message, session.pendingStart)) throw new Error("invalid pending lobby signal");
                    if (session.pendingSignals.length >= MAX_QUEUED_PACKETS) throw new Error("pending lobby signal overflow");
                    session.pendingSignals.push(message);
                    return;
                }
                if (!Number.isInteger(message.epoch) || message.epoch !== session.epoch || message.round !== session.round) return;
                await lobbyHandleSignal(session, message);
            } else if (message.type === "rematch_pending" || message.type === "rematch_accepted" || message.type === "rematch_denied" || message.type === "match_exit" || message.type === "match_over" || message.type === "requeue") {
                if (session.control.length >= 32) session.control.shift();
                session.control.push(message);
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
        Number.isSafeInteger(message.deadline) && message.deadline > 0 && typeof message.voted === "boolean" &&
        (!message.voted || message.votes > 0)) {
        session.queuePhase = 3; session.queueCount = message.count; session.queueVotes = message.votes;
        session.queueVotesRequired = message.votesRequired; session.queueDeadline = message.deadline;
        session.queueVoted = message.voted; return true;
    }
    return false;
}

// Pure timeout policy kept explicit for source/reducer harnesses: public queue
// waiting has no ordinary age timeout. Only opening, assignment handoff, and
// WebRTC/lobby formation retain bounded failure windows.
function timeoutForPhase(phase) {
    if (phase === "queue_wait") return null;
    if (phase === "socket_open") return INITIAL_SOCKET_TIMEOUT_MS;
    if (phase === "assignment_handoff") return ASSIGNMENT_HANDOFF_TIMEOUT_MS;
    return MATCHMAKING_TIMEOUT_MS;
}

export function cloudflare_connect_queue(baseUrl, compatibilityRoom, preference, profileName, paletteId, cosmeticId) {
    const endpoint = (baseUrl || `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/queue`).replace(/\/match\/?$/, "/queue").replace(/\/lobby\/?$/, "/queue");
    const url = `${endpoint.replace(/\/$/, "")}/${encodeURIComponent(compatibilityRoom)}?protocol=4&preference=${encodeURIComponent(preference)}`;
    const ws = new WebSocket(url);
    const id = nextTransportId++ || nextTransportId++;
    const session = { id, ws, queue: true, status: 0, error: "", queuePhase: 1, queueCount: 0, queueVotes: 0, queueVotesRequired: 0, queueDeadline: 0, queueVoted: false, ticket: "", control: [], inbox: [], roster: [], channels: new Map(), peers: new Map(), pendingIce: new Map(), openPeers: new Set(), timeout: 0, heartbeat: 0, signalChain: Promise.resolve(), telemetry: [0,0,0,0,0,0,0,0,0,0,0] };
    networks.set(id, session);
    session.timeout = window.setTimeout(() => fail(session, "could not open public queue"), timeoutForPhase("socket_open"));
    ws.onopen = () => {
        if (!isCurrent(session)) return;
        // Keep the initial timer until the protocol-4 queued acknowledgement;
        // an open socket which never admits a ticket is not a healthy queue.
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
                session.ticket = message.ticket; session.queuePhase = 1;
                window.clearTimeout(session.timeout);
                session.timeout = 0; // Ordinary admitted queue waiting is unbounded.
                return;
            }
            if (message?.type === "status" && queueStatus(session, message)) return;
            if (message?.type === "heartbeat_ack") return;
            if (message?.type === "assigned") {
                if (!validAssignment(message, session.ticket)) throw new Error("invalid queue assignment");
                session.queuePhase = 4;
                session.handingOff = true;
                window.clearInterval(session.heartbeat);
                // Remove the queue object before closing it so a synchronous or
                // queued close callback cannot fail the newly installed lobby.
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
    try { session.ws.send(JSON.stringify({ type: "vote_start" })); return true; } catch (error) { fail(session, error); return false; }
}
export function cloudflare_queue_withdraw_start_vote(id) {
    const session = current(id);
    if (!session?.queue || session.queuePhase !== 3 || !session.queueVoted || session.ws.readyState !== WebSocket.OPEN) return false;
    try { session.ws.send(JSON.stringify({ type: "withdraw_start_vote" })); return true; } catch (error) { fail(session, error); return false; }
}

export function cloudflare_lobby_local_id(id) { return current(id)?.localPlayerId || ""; }
export function cloudflare_lobby_mode(id) { return current(id)?.mode ?? 0; }
export function cloudflare_lobby_generation(id) { return current(id)?.matchGeneration ?? 0; }
export function cloudflare_lobby_control(id) { return current(id)?.control?.shift() ?? null; }
export function cloudflare_lobby_rematch_request(id, generation, nonce) { const session=current(id); if (!session || session.ws.readyState!==WebSocket.OPEN) return false; session.ws.send(JSON.stringify({type:"rematch_request",generation,nonce})); return true; }
export function cloudflare_lobby_rematch_response(id, generation, nonce, accept) { const session=current(id); if (!session || session.ws.readyState!==WebSocket.OPEN) return false; session.ws.send(JSON.stringify({type:"rematch_response",generation,nonce,accept})); return true; }
export function cloudflare_lobby_leave(id, requeue) { const session=current(id); if (!session || session.ws.readyState!==WebSocket.OPEN) return false; session.ws.send(JSON.stringify({type:requeue?"requeue":"leave"})); return true; }
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
    const session = current(id); const channel = session?.channels.get(to);
    if (session?.status !== 1 || epoch !== session.epoch || channel?.readyState !== "open" || packet.length > MAX_PACKET_BYTES || channel.bufferedAmount > MAX_BUFFERED_BYTES) { if (session) session.telemetry[2]++; return; }
    try { const framed = new Uint8Array(packet.length + 8); const view = new DataView(framed.buffer); view.setUint32(0, epoch, false); view.setUint32(4, session.round, false); framed.set(packet, 8); channel.send(framed); session.telemetry[0]++; } catch (error) { fail(session, error); }
}
export function cloudflare_lobby_receive(id) {
    const item = current(id)?.inbox?.shift();
    return item ? [item.epoch, item.from, item.packet] : null;
}
export function cloudflare_lobby_report(id, epoch, round, winners) {
    const session = current(id);
    if (!session || session.ws.readyState !== WebSocket.OPEN || epoch !== session.epoch || round !== session.round) return false;
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
export function cloudflare_lobby_close_epoch(id, epoch, round) {
    const session = current(id); if (!session || session.queue) return false;
    return closeLobbyRound(session, epoch, round);
}
export function cloudflare_lobby_promote_pending(id, oldEpoch, oldRound, pendingEpoch, pendingRound) {
    const session = current(id);
    if (!session || session.queue || !session.pendingStart || session.epoch !== oldEpoch || session.round !== oldRound ||
        session.pendingStart.epoch !== pendingEpoch || session.pendingStart.round !== pendingRound) return false;
    const start = session.pendingStart;
    if (pendingEpoch === oldEpoch) {
        // Same-roster rounds reuse the established epoch mesh. GGRS packets
        // carry round in their header, so delayed old-round packets are
        // rejected without racing another browser's teardown.
        const activeIds = session.roster.map(entry => entry.playerId).join(",");
        const pendingIds = start.roster.map(entry => entry.playerId).join(",");
        if (activeIds !== pendingIds || session.status !== 1) return false;
        session.roster = start.roster; session.seed = start.seed; session.round = start.round;
        session.matchGeneration = start.matchGeneration; session.inbox.length = 0;
        session.pendingStart = null; session.pendingSignals.length = 0;
        return true;
    }
    // Changed-roster epochs receive a fresh mesh after the exact old mesh is
    // retired. This is the only path that closes peer connections on rollover.
    if (!closeLobbyRound(session, oldEpoch, oldRound)) return false;
    const signals = session.pendingSignals.splice(0);
    session.pendingStart = null;
    session.signalChain = session.signalChain.then(() => installLobbyStart(session, start, signals)).catch(error => fail(session, error));
    return true;
}
export function cloudflare_close_lobby(id) {
    const session = current(id); if (!session) return;
    networks.delete(id); window.clearTimeout(session.timeout); window.clearInterval(session.heartbeat);
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
    fn cloudflare_connect_queue(
        base_url: &str,
        compatibility_room: &str,
        preference: &str,
        profile_name: &str,
        palette_id: u32,
        cosmetic_id: u32,
    ) -> u32;
    fn cloudflare_queue_phase(id: u32) -> u32;
    fn cloudflare_queue_count(id: u32) -> u32;
    fn cloudflare_queue_votes(id: u32) -> u32;
    fn cloudflare_queue_votes_required(id: u32) -> u32;
    fn cloudflare_queue_deadline(id: u32) -> String;
    fn cloudflare_queue_voted(id: u32) -> bool;
    fn cloudflare_queue_vote_start(id: u32) -> bool;
    fn cloudflare_queue_withdraw_start_vote(id: u32) -> bool;
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
    fn cloudflare_lobby_mode(id: u32) -> u32;
    fn cloudflare_lobby_generation(id: u32) -> u32;
    fn cloudflare_lobby_control(id: u32) -> wasm_bindgen::JsValue;
    fn cloudflare_lobby_rematch_request(id: u32, generation: u32, nonce: &str) -> bool;
    fn cloudflare_lobby_rematch_response(
        id: u32,
        generation: u32,
        nonce: &str,
        accept: bool,
    ) -> bool;
    fn cloudflare_lobby_leave(id: u32, requeue: bool) -> bool;
    fn cloudflare_lobby_seed(id: u32) -> String;
    fn cloudflare_lobby_epoch(id: u32) -> u32;
    fn cloudflare_lobby_round(id: u32) -> u32;
    fn cloudflare_lobby_has_pending(id: u32) -> bool;
    fn cloudflare_lobby_pending_epoch(id: u32) -> u32;
    fn cloudflare_lobby_pending_round(id: u32) -> u32;
    fn cloudflare_lobby_roster_len(id: u32) -> u32;
    fn cloudflare_lobby_roster_id(id: u32, index: u32) -> String;
    fn cloudflare_lobby_roster_score(id: u32, index: u32) -> u32;
    fn cloudflare_lobby_send(id: u32, epoch: u32, to: &str, packet: &[u8]);
    fn cloudflare_lobby_report(
        id: u32,
        epoch: u32,
        round: u32,
        winners: wasm_bindgen::JsValue,
    ) -> bool;
    fn cloudflare_lobby_receive(id: u32) -> wasm_bindgen::JsValue;
    fn cloudflare_lobby_close_epoch(id: u32, epoch: u32, round: u32) -> bool;
    fn cloudflare_lobby_promote_pending(
        id: u32,
        old_epoch: u32,
        old_round: u32,
        pending_epoch: u32,
        pending_round: u32,
    ) -> bool;
    fn cloudflare_close_lobby(id: u32);
}
