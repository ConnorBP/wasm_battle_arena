//! Browser-local casual profile and progression.
//!
//! Reconnect credentials intentionally remain in the networking layer's
//! `sessionStorage`. This module only owns the durable, non-authoritative
//! preferences and casual rewards stored in `localStorage`.

use std::collections::BTreeSet;

use bevy::prelude::*;
use bevy_ggrs::Session;

use super::{
    assets::sounds::AudioConfig,
    networking::{GgrsConfig, LocalPlayerHandle},
    session::{match_winner, MatchId, PlayerProfile, RoundBootstrap, RoundNumber, SessionEpoch},
    PendingPlayerProfile, RollbackState, RoundProgress, Scores,
};

pub const PROFILE_SCHEMA_VERSION: u8 = 1;
pub const PROFILE_STORAGE_KEY: &str = "ghosties.casual-profile.v1";
const PROFILE_MAGIC: &str = "GHOSTIES_PROFILE";
const MAX_COUNTER: u64 = 999_999_999;
const MAX_EVENT_ID_BYTES: usize = 96;
const DEFAULT_NAME: &str = "Ghost";

pub const ROUND_PARTICIPATION_POINTS: u64 = 2;
pub const ROUND_WIN_BONUS_POINTS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CosmeticDefinition {
    pub id: u8,
    pub name: &'static str,
    pub required_points: u64,
}

/// Deliberately small thresholds make the first rewards available in a short
/// casual session. Classic is always available.
pub const COSMETICS: [CosmeticDefinition; 4] = [
    CosmeticDefinition {
        id: 0,
        name: "Classic",
        required_points: 0,
    },
    CosmeticDefinition {
        id: 1,
        name: "Crown",
        required_points: 5,
    },
    CosmeticDefinition {
        id: 2,
        name: "Wizard",
        required_points: 12,
    },
    CosmeticDefinition {
        id: 3,
        name: "Bow",
        required_points: 25,
    },
];

pub fn cosmetic(id: u8) -> Option<&'static CosmeticDefinition> {
    COSMETICS
        .get(id as usize)
        .filter(|definition| definition.id == id)
}

pub fn unlocked_mask(lifetime_points: u64) -> u8 {
    COSMETICS.iter().fold(0, |mask, definition| {
        if lifetime_points >= definition.required_points {
            mask | (1 << definition.id)
        } else {
            mask
        }
    })
}

pub fn equipped_or_default(equipped: u8, unlocked: u8) -> u8 {
    if cosmetic(equipped).is_some() && unlocked & (1 << equipped) != 0 {
        equipped
    } else {
        0
    }
}

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct CasualProfile {
    pub schema_version: u8,
    pub name: String,
    pub music_volume: f64,
    pub effects_volume: f64,
    pub palette_id: u8,
    pub lifetime_points: u64,
    pub matches_played: u64,
    pub rounds_played: u64,
    pub unlocked_cosmetics: u8,
    pub equipped_cosmetic: u8,
    processed_outcomes: BTreeSet<String>,
}

impl Default for CasualProfile {
    fn default() -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            name: DEFAULT_NAME.into(),
            music_volume: 55.0,
            effects_volume: 100.0,
            palette_id: 0,
            lifetime_points: 0,
            matches_played: 0,
            rounds_played: 0,
            unlocked_cosmetics: 1,
            equipped_cosmetic: 0,
            processed_outcomes: BTreeSet::new(),
        }
    }
}

impl CasualProfile {
    /// Decode the intentionally simple, versioned storage schema. Every field
    /// is validated independently; an unknown schema or malformed envelope
    /// returns a complete safe default.
    pub fn decode(value: &str) -> Self {
        let fields: Vec<_> = value.split('\t').collect();
        if fields.len() != 12
            || fields[0] != PROFILE_MAGIC
            || fields[1].parse::<u8>().ok() != Some(PROFILE_SCHEMA_VERSION)
        {
            return Self::default();
        }

        let defaults = Self::default();
        let mut profile = Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            name: canonical_name(fields[2]),
            music_volume: volume_or_default(fields[3], defaults.music_volume),
            effects_volume: volume_or_default(fields[4], defaults.effects_volume),
            palette_id: fields[5]
                .parse::<u8>()
                .ok()
                .filter(|id| *id < 4)
                .unwrap_or(defaults.palette_id),
            lifetime_points: bounded_counter(fields[6]),
            matches_played: bounded_counter(fields[7]),
            rounds_played: bounded_counter(fields[8]),
            unlocked_cosmetics: fields[9].parse::<u8>().unwrap_or(0),
            equipped_cosmetic: fields[10].parse::<u8>().unwrap_or(0),
            processed_outcomes: fields[11]
                .split(',')
                .filter(|id| valid_event_id(id))
                .map(str::to_owned)
                .collect(),
        };
        profile.normalize();
        profile
    }

    pub fn encode(&self) -> String {
        let mut profile = self.clone();
        profile.normalize();
        let events = profile
            .processed_outcomes
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{PROFILE_MAGIC}\t{PROFILE_SCHEMA_VERSION}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            profile.name,
            profile.music_volume,
            profile.effects_volume,
            profile.palette_id,
            profile.lifetime_points,
            profile.matches_played,
            profile.rounds_played,
            profile.unlocked_cosmetics,
            profile.equipped_cosmetic,
            events,
        )
    }

    pub fn is_unlocked(&self, cosmetic_id: u8) -> bool {
        cosmetic(cosmetic_id).is_some() && self.unlocked_cosmetics & (1 << cosmetic_id) != 0
    }

    pub fn equip(&mut self, cosmetic_id: u8) -> bool {
        if !self.is_unlocked(cosmetic_id) {
            return false;
        }
        let changed = self.equipped_cosmetic != cosmetic_id;
        self.equipped_cosmetic = cosmetic_id;
        changed
    }

    /// Applies one confirmed round result. A stable event ID is retained in
    /// the durable profile, so rollback replays, repeated frames, and reloads
    /// cannot grant the same reward twice.
    pub fn award_confirmed_outcome(
        &mut self,
        event_id: &str,
        local_won_round: bool,
        match_completed: bool,
    ) -> bool {
        if !valid_event_id(event_id) || !self.processed_outcomes.insert(event_id.to_owned()) {
            return false;
        }
        let reward = ROUND_PARTICIPATION_POINTS
            + if local_won_round {
                ROUND_WIN_BONUS_POINTS
            } else {
                0
            };
        self.lifetime_points = self.lifetime_points.saturating_add(reward).min(MAX_COUNTER);
        self.rounds_played = self.rounds_played.saturating_add(1).min(MAX_COUNTER);
        if match_completed {
            self.matches_played = self.matches_played.saturating_add(1).min(MAX_COUNTER);
        }
        self.normalize();
        true
    }

    fn normalize(&mut self) {
        self.schema_version = PROFILE_SCHEMA_VERSION;
        self.name = canonical_name(&self.name);
        self.music_volume = finite_clamped_volume(self.music_volume, Self::default().music_volume);
        self.effects_volume =
            finite_clamped_volume(self.effects_volume, Self::default().effects_volume);
        if self.palette_id >= 4 {
            self.palette_id = 0;
        }
        self.lifetime_points = self.lifetime_points.min(MAX_COUNTER);
        self.matches_played = self.matches_played.min(MAX_COUNTER);
        self.rounds_played = self.rounds_played.min(MAX_COUNTER);
        // Unlocks are earned, not trusted input. Keeping the field in the
        // schema makes unlock state explicit while thresholds remain canonical.
        self.unlocked_cosmetics = unlocked_mask(self.lifetime_points);
        self.equipped_cosmetic =
            equipped_or_default(self.equipped_cosmetic, self.unlocked_cosmetics);
        self.processed_outcomes.retain(|id| valid_event_id(id));
    }
}

fn canonical_name(value: &str) -> String {
    let value = PlayerProfile::sanitized_name(value);
    if value.is_empty() {
        DEFAULT_NAME.into()
    } else {
        value
    }
}

fn bounded_counter(value: &str) -> u64 {
    value.parse::<u64>().unwrap_or_default().min(MAX_COUNTER)
}

fn volume_or_default(value: &str, default: f64) -> f64 {
    value
        .parse::<f64>()
        .ok()
        .map(|value| finite_clamped_volume(value, default))
        .unwrap_or(default)
}

fn finite_clamped_volume(value: f64, default: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        default
    }
}

fn valid_event_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_EVENT_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || matches!(byte, b':' | b'-'))
}

pub fn outcome_event_id(
    match_id: MatchId,
    epoch: SessionEpoch,
    round: RoundNumber,
    confirmed_frame: u32,
) -> String {
    format!(
        "{:032x}:{:08x}:{:08x}:{:08x}",
        match_id.0, epoch.0, round.0, confirmed_frame
    )
}

pub fn load_persistent_profile(
    mut stored: ResMut<CasualProfile>,
    mut pending: ResMut<PendingPlayerProfile>,
    mut audio: ResMut<AudioConfig>,
) {
    let loaded = CasualProfile::decode(&storage_load());
    pending.name = loaded.name.clone();
    pending.palette_id = loaded.palette_id;
    pending.cosmetic_id = loaded.equipped_cosmetic;
    audio.music_volume = loaded.music_volume;
    audio.sfx_volume = loaded.effects_volume;
    // Materialize a canonical v1 profile on first run and repair any partially
    // invalid v1 values that were safely defaulted during decoding.
    storage_save(&loaded.encode());
    *stored = loaded;
}

/// Pull preference edits made by egui into the durable profile. Progression
/// counters remain exclusively owned by `award_confirmed_progression`.
pub fn sync_persistent_preferences(
    pending: Res<PendingPlayerProfile>,
    audio: Res<AudioConfig>,
    mut stored: ResMut<CasualProfile>,
) {
    let name = canonical_name(&pending.name);
    let palette = if pending.palette_id < 4 {
        pending.palette_id
    } else {
        0
    };
    let equipped = equipped_or_default(pending.cosmetic_id, stored.unlocked_cosmetics);
    let music = finite_clamped_volume(audio.music_volume, stored.music_volume);
    let effects = finite_clamped_volume(audio.sfx_volume, stored.effects_volume);
    if stored.name == name
        && stored.palette_id == palette
        && stored.equipped_cosmetic == equipped
        && stored.music_volume == music
        && stored.effects_volume == effects
    {
        return;
    }
    stored.name = name;
    stored.palette_id = palette;
    stored.equipped_cosmetic = equipped;
    stored.music_volume = music;
    stored.effects_volume = effects;
    stored.normalize();
    storage_save(&stored.encode());
}

pub fn award_confirmed_progression(
    session: Res<Session<GgrsConfig>>,
    bootstrap: Option<Res<RoundBootstrap>>,
    local: Option<Res<LocalPlayerHandle>>,
    progress: Res<RoundProgress>,
    scores: Res<Scores>,
    rollback_state: Res<State<RollbackState>>,
    mut stored: ResMut<CasualProfile>,
) {
    // OnExit(InRound) applies the outcome to Scores. Waiting for RoundEnd
    // prevents a confirmed frame from recording the match endpoint against
    // pre-outcome scores.
    if rollback_state.get() != &RollbackState::RoundEnd {
        return;
    }
    let (Some(bootstrap), Some(local), Some(outcome), Some(frame)) = (
        bootstrap,
        local,
        progress.resolved.as_ref(),
        progress.resolved_frame,
    ) else {
        return;
    };
    let Session::P2P(p2p) = session.as_ref() else {
        return;
    };
    if p2p.confirmed_frame() < frame as i32 {
        return;
    }
    let Some(local_id) = bootstrap
        .roster
        .iter()
        .find(|entry| entry.handle == local.0)
        .map(|entry| entry.player_id)
    else {
        return;
    };

    let local_won = outcome.point_winners().contains(&local_id);
    let match_completed = match_winner(scores.entries()).is_some();
    let event_id = outcome_event_id(bootstrap.match_id, bootstrap.epoch, bootstrap.round, frame);
    if stored.award_confirmed_outcome(&event_id, local_won, match_completed) {
        storage_save(&stored.encode());
    }
}

#[cfg(target_arch = "wasm32")]
fn storage_load() -> String {
    profile_storage_load(PROFILE_STORAGE_KEY)
}

#[cfg(not(target_arch = "wasm32"))]
fn storage_load() -> String {
    String::new()
}

#[cfg(target_arch = "wasm32")]
fn storage_save(value: &str) {
    profile_storage_save(PROFILE_STORAGE_KEY, value);
}

#[cfg(not(target_arch = "wasm32"))]
fn storage_save(_value: &str) {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function profile_storage_load(key) {
  try { return window.localStorage.getItem(key) || ""; }
  catch (_) { return ""; }
}
export function profile_storage_save(key, value) {
  try { window.localStorage.setItem(key, value); }
  catch (_) { /* Storage may be disabled or full; gameplay remains available. */ }
}
"#)]
extern "C" {
    fn profile_storage_load(key: &str) -> String;
    fn profile_storage_save(key: &str, value: &str);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_round_trip_and_unknown_schema_default_safely() {
        let mut profile = CasualProfile::default();
        profile.name = "  Ghost\n Rider ".into();
        profile.music_volume = 33.5;
        profile.effects_volume = 72.0;
        profile.palette_id = 2;
        assert!(profile.award_confirmed_outcome("01:02:03:04", true, false));
        let decoded = CasualProfile::decode(&profile.encode());
        assert_eq!(decoded.name, "Ghost Rider");
        assert_eq!(decoded.music_volume, 33.5);
        assert_eq!(decoded.effects_volume, 72.0);
        assert_eq!(decoded.palette_id, 2);
        assert_eq!(decoded.lifetime_points, 5);
        assert_eq!(
            CasualProfile::decode("GHOSTIES_PROFILE\t99\tGhost"),
            CasualProfile::default()
        );
        assert_eq!(
            CasualProfile::decode("not a profile"),
            CasualProfile::default()
        );
    }

    #[test]
    fn invalid_values_are_defaulted_clamped_and_sanitized() {
        let value = "GHOSTIES_PROFILE\t1\t  \n Bad\tNaN\t500\t90\t18446744073709551615\tbad\t4\t255\t99\tbad event,abc:123";
        let profile = CasualProfile::decode(value);
        assert_eq!(profile.name, "Bad");
        assert_eq!(profile.music_volume, 55.0);
        assert_eq!(profile.effects_volume, 100.0);
        assert_eq!(profile.palette_id, 0);
        assert_eq!(profile.lifetime_points, MAX_COUNTER);
        assert_eq!(profile.matches_played, 0);
        assert_eq!(profile.rounds_played, 4);
        assert_eq!(profile.unlocked_cosmetics, 0b1111);
        assert_eq!(profile.equipped_cosmetic, 0);
        assert_eq!(
            profile.processed_outcomes,
            BTreeSet::from(["abc:123".into()])
        );
    }

    #[test]
    fn thresholds_are_small_monotonic_and_exact() {
        assert_eq!(unlocked_mask(0), 0b0001);
        assert_eq!(unlocked_mask(4), 0b0001);
        assert_eq!(unlocked_mask(5), 0b0011);
        assert_eq!(unlocked_mask(11), 0b0011);
        assert_eq!(unlocked_mask(12), 0b0111);
        assert_eq!(unlocked_mask(24), 0b0111);
        assert_eq!(unlocked_mask(25), 0b1111);
        assert!(COSMETICS
            .windows(2)
            .all(|pair| pair[0].required_points < pair[1].required_points));
    }

    #[test]
    fn duplicate_stable_ids_never_award_twice_even_after_round_trip() {
        let mut profile = CasualProfile::default();
        let id = outcome_event_id(MatchId(7), SessionEpoch(2), RoundNumber(3), 99);
        assert!(profile.award_confirmed_outcome(&id, true, true));
        assert!(!profile.award_confirmed_outcome(&id, true, true));
        let mut reloaded = CasualProfile::decode(&profile.encode());
        assert!(!reloaded.award_confirmed_outcome(&id, true, true));
        assert_eq!(
            (
                reloaded.lifetime_points,
                reloaded.rounds_played,
                reloaded.matches_played
            ),
            (5, 1, 1)
        );
    }

    #[test]
    fn equipped_cosmetic_must_exist_and_be_unlocked() {
        assert_eq!(equipped_or_default(1, 0b0001), 0);
        assert_eq!(equipped_or_default(9, 0b1111), 0);
        assert_eq!(equipped_or_default(2, 0b0111), 2);
        let locked = "GHOSTIES_PROFILE\t1\tGhost\t55\t100\t0\t4\t0\t0\t15\t3\t";
        assert_eq!(CasualProfile::decode(locked).equipped_cosmetic, 0);
    }
}
