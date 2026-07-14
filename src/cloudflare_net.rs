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

#[cfg(any(test, target_arch = "wasm32"))]
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

pub struct LobbyMatchInfo {
    pub local_player: PlayerId,
    pub mode: u32,
    pub seed: u64,
    pub match_id: u128,
    pub epoch: u32,
    pub round: u32,
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
    pub relay_connections: u64,
    pub stun_fallbacks: u64,
    pub candidate_pair_host: u64,
    pub candidate_pair_srflx: u64,
    pub candidate_pair_relay: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Ready,
    Failed(String),
}

#[derive(Resource, Default)]
pub struct CloudflareSocket {
    transport_id: u32,
    native_error: Option<String>,
    epoch: u32,
    round: u32,
    /// Only the Bevy resource owns the persistent lobby control transport.
    owns_transport: bool,
}

impl CloudflareSocket {
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
        return match cloudflare_status(self.transport_id) {
            1 => ConnectionState::Ready,
            2 => ConnectionState::Failed(cloudflare_error(self.transport_id)),
            _ => ConnectionState::Connecting,
        };
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
        None
    }

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

    pub fn lobby_match_info(&self) -> Option<LobbyMatchInfo> {
        #[cfg(target_arch = "wasm32")]
        {
            if self.state() != ConnectionState::Ready {
                return None;
            }
            let local_player = parse_player_id(&cloudflare_lobby_local_id(self.transport_id))?;
            let seed_hex = cloudflare_lobby_seed(self.transport_id);
            let match_id = u128::from_str_radix(&seed_hex, 16).ok()?;
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
            return Some(LobbyMatchInfo {
                local_player,
                mode,
                seed: match_id as u64,
                match_id,
                epoch,
                round: cloudflare_lobby_round(self.transport_id),
                roster,
                scores,
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        None
    }

    pub fn match_generation(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
            return Some(cloudflare_lobby_generation(self.transport_id));
        }
        None
    }

    pub fn request_rematch(&self, generation: u32, nonce: &str) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
            return cloudflare_lobby_rematch_request(self.transport_id, generation, nonce);
        }
        false
    }

    pub fn respond_rematch(&self, generation: u32, nonce: &str, accept: bool) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
            return cloudflare_lobby_rematch_response(self.transport_id, generation, nonce, accept);
        }
        false
    }

    pub fn leave_lobby(&self, requeue: bool) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
            return cloudflare_lobby_leave(self.transport_id, requeue);
        }
        false
    }

    pub fn poll_control(&self) -> Option<LobbyControlEvent> {
        #[cfg(target_arch = "wasm32")]
        {
            if self.transport_id == 0 {
                return None;
            }
            let value = cloudflare_lobby_control(self.transport_id);
            if value.is_null() || value.is_undefined() {
                return None;
            }
            let kind = js_sys::Reflect::get(&value, &"type".into())
                .ok()?
                .as_string()?;
            let number = |key: &str| {
                let value = js_sys::Reflect::get(&value, &key.into()).ok()?.as_f64()?;
                (value.is_finite()
                    && value.fract() == 0.0
                    && (0.0..=9_007_199_254_740_991.0).contains(&value))
                .then_some(value as u64)
            };
            return match kind.as_str() {
                "rematch_pending" => Some(LobbyControlEvent::RematchPending {
                    generation: u32::try_from(number("generation")?).ok()?,
                    nonce: js_sys::Reflect::get(&value, &"nonce".into())
                        .ok()?
                        .as_string()?,
                    deadline_ms: number("deadline")?,
                    accepted: {
                        let accepted = js_sys::Reflect::get(&value, &"accepted".into()).ok()?;
                        if !js_sys::Array::is_array(&accepted) {
                            return Some(LobbyControlEvent::Ignored);
                        }
                        let len = js_sys::Array::from(&accepted).length();
                        if len > 8 {
                            return Some(LobbyControlEvent::Ignored);
                        }
                        len as u8
                    },
                    required: u8::try_from(number("required")?).ok()?,
                }),
                "rematch_accepted" => Some(LobbyControlEvent::RematchAccepted {
                    generation: u32::try_from(number("generation")?).ok()?,
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
        if self.transport_id != 0 {
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
        if self.transport_id != 0 {
            return Some(cloudflare_lobby_epoch(self.transport_id));
        }
        None
    }

    pub fn lobby_round(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
            return Some(cloudflare_lobby_round(self.transport_id));
        }
        None
    }

    pub fn set_epoch_round(&mut self, epoch: u32, round: u32) {
        self.epoch = epoch;
        self.round = round;
    }

    pub fn pending_epoch_round(&self) -> Option<(u32, u32)> {
        Some((self.pending_epoch()?, self.pending_round()?))
    }

    pub fn pending_epoch(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 && cloudflare_lobby_has_pending(self.transport_id) {
            return Some(cloudflare_lobby_pending_epoch(self.transport_id));
        }
        None
    }

    pub fn pending_round(&self) -> Option<u32> {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 && cloudflare_lobby_has_pending(self.transport_id) {
            return Some(cloudflare_lobby_pending_round(self.transport_id));
        }
        None
    }

    pub fn promote_pending(&mut self, old_epoch: u32, old_round: u32) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
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
        if self.transport_id != 0 {
            return cloudflare_lobby_close_epoch(self.transport_id, epoch, round);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (epoch, round);
        false
    }

    pub fn take_transport(&mut self) -> Self {
        Self {
            transport_id: self.transport_id,
            native_error: self.native_error.clone(),
            epoch: self.epoch,
            round: self.round,
            owns_transport: false,
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
            cloudflare_close_lobby(self.transport_id);
        }
        self.transport_id = 0;
        self.native_error = None;
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
    fn send_to(&mut self, message: &Message, address: &PlayerId) {
        #[cfg(target_arch = "wasm32")]
        if self.transport_id != 0 {
            if let Ok(packet) = codec().serialize(message) {
                cloudflare_lobby_send(
                    self.transport_id,
                    self.epoch,
                    &format!("{:032x}", address.0),
                    &packet,
                );
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (message, address);
    }

    fn receive_all_messages(&mut self) -> Vec<(PlayerId, Message)> {
        #[cfg(target_arch = "wasm32")]
        {
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
                let Some(packet_epoch) = array.get(0).as_f64().and_then(|value| {
                    (value.is_finite()
                        && value.fract() == 0.0
                        && (0.0..=u32::MAX as f64).contains(&value))
                    .then_some(value as u32)
                }) else {
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
#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/cloudflare_net.js")]
extern "C" {
    fn cloudflare_status(id: u32) -> u32;
    fn cloudflare_telemetry(id: u32, counter: u32) -> u64;
    fn cloudflare_error(id: u32) -> String;
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

#[cfg(test)]
mod tests {
    use super::*;

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
                voted: true
            })
        );
        assert_eq!(queue_status_from_scalars(3, 2, 0, 2, 40_000, false), None);
        assert_eq!(queue_status_from_scalars(3, 9, 0, 5, 40_000, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 6, 3, 40_000, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 2, 2, 40_000, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 0, 3, 0, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 0, 3, u64::MAX, false), None);
        assert_eq!(queue_status_from_scalars(3, 5, 0, 3, 40_000, true), None);
        assert_eq!(
            queue_status_from_scalars(4, 0, 0, 0, 0, false),
            Some(QueueStatus::Assigned)
        );
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
}
