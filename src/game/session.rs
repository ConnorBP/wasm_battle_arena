use std::collections::{BTreeSet, HashSet};

use bevy::prelude::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Reflect)]
pub struct PlayerId(pub u128);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MatchId(pub u128);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionEpoch(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundNumber(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    Duel,
    Deathmatch,
}

pub const MATCH_POINTS_TO_WIN: u32 = 3;

/// Public-facing mode copy. The internal `Deathmatch` variant is a round-based
/// last-survivor mode, not an unlimited respawn deathmatch.
pub fn mode_label(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Duel => "Duel — First to 3",
        GameMode::Deathmatch => "Last Ghost Standing — First to 3",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchWinner {
    pub player_id: PlayerId,
    pub score: u32,
}

/// Returns the winner only after the first-to-three endpoint is reached.
/// Canonical identity ordering makes malformed ties deterministic.
pub fn match_winner(scores: &[PlayerScore]) -> Option<MatchWinner> {
    scores
        .iter()
        .filter(|entry| entry.score >= MATCH_POINTS_TO_WIN)
        .min_by_key(|entry| entry.player_id)
        .map(|entry| MatchWinner { player_id: entry.player_id, score: entry.score })
}

/// The deterministic result of applying eliminations to a round roster.
#[derive(Debug, Clone, PartialEq, Eq, Reflect)]
pub enum RoundOutcome {
    InProgress,
    Complete { point_winners: Vec<PlayerId> },
}

impl RoundOutcome {
    pub fn point_winners(&self) -> &[PlayerId] {
        match self {
            Self::InProgress => &[],
            Self::Complete { point_winners } => point_winners,
        }
    }
}

/// Resolve a round without depending on entity or network iteration order.
///
/// Disconnected players are treated exactly like eliminated players. Unknown
/// IDs are ignored, and the returned winners are in canonical `PlayerId`
/// order.
pub fn round_outcome(
    mode: GameMode,
    roster: &[PlayerId],
    eliminated: &[PlayerId],
    disconnected: &[PlayerId],
) -> RoundOutcome {
    let roster: BTreeSet<_> = roster.iter().copied().collect();
    let unavailable: BTreeSet<_> = eliminated
        .iter()
        .chain(disconnected)
        .copied()
        .filter(|player_id| roster.contains(player_id))
        .collect();

    match mode {
        GameMode::Duel if roster.len() == 2 => {
            if unavailable.is_empty() {
                return RoundOutcome::InProgress;
            }

            // Preserve duel rules: each eliminated player awards one point to
            // the opponent, including one point each on a simultaneous KO.
            let point_winners = roster
                .iter()
                .copied()
                .filter(|player_id| {
                    roster
                        .iter()
                        .any(|opponent| opponent != player_id && unavailable.contains(opponent))
                })
                .collect();
            RoundOutcome::Complete { point_winners }
        }
        GameMode::Deathmatch if roster.len() == 4 => {
            let survivors: Vec<_> = roster.difference(&unavailable).copied().collect();
            if survivors.len() > 1 {
                RoundOutcome::InProgress
            } else {
                // A sole survivor scores. A simultaneous wipe has no winner.
                RoundOutcome::Complete { point_winners: survivors }
            }
        }
        _ => RoundOutcome::InProgress,
    }
}

pub const MAX_PLAYER_NAME_BYTES: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerProfile {
    pub player_id: PlayerId,
    pub name: String,
    pub palette_id: u8,
    pub cosmetic_id: u8,
}

impl PlayerProfile {
    pub fn sanitized_name(value: &str) -> String {
        let mut output = String::new();
        let mut pending_space = false;
        for character in value.trim().chars().filter(|character| !character.is_control()) {
            if character.is_whitespace() {
                pending_space = !output.is_empty();
                continue;
            }
            if pending_space && output.len() < MAX_PLAYER_NAME_BYTES { output.push(' '); }
            pending_space = false;
            if output.len() + character.len_utf8() > MAX_PLAYER_NAME_BYTES { break; }
            output.push(character);
        }
        output
    }

    pub fn is_canonical(&self) -> bool {
        !self.name.is_empty()
            && self.name == Self::sanitized_name(&self.name)
            && self.palette_id < 4
            && self.cosmetic_id < 4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RosterEntry {
    pub player_id: PlayerId,
    pub handle: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub struct PlayerScore {
    pub player_id: PlayerId,
    pub score: u32,
}

#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct RoundBootstrap {
    pub protocol_version: u16,
    pub match_id: MatchId,
    pub match_seed: u64,
    pub epoch: SessionEpoch,
    pub round: RoundNumber,
    pub mode: GameMode,
    pub roster: Vec<RosterEntry>,
    pub profiles: Vec<PlayerProfile>,
    pub scores: Vec<PlayerScore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapError {
    InvalidProtocol,
    InvalidPlayerCount,
    DuplicatePlayer,
    InvalidHandles,
    InvalidProfiles,
    InvalidScores,
}

impl RoundBootstrap {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        protocol_version: u16,
        match_id: MatchId,
        match_seed: u64,
        epoch: SessionEpoch,
        round: RoundNumber,
        mode: GameMode,
        mut roster: Vec<RosterEntry>,
        mut profiles: Vec<PlayerProfile>,
        mut scores: Vec<PlayerScore>,
    ) -> Result<Self, BootstrapError> {
        roster.sort_by_key(|entry| entry.player_id);
        profiles.sort_by_key(|entry| entry.player_id);
        scores.sort_by_key(|entry| entry.player_id);

        if protocol_version == 0 {
            return Err(BootstrapError::InvalidProtocol);
        }
        let valid_count = match mode {
            GameMode::Duel => roster.len() == 2,
            // Competitive selection intentionally supports only a duel or a
            // full four-ghost Last Ghost Standing roster.
            GameMode::Deathmatch => roster.len() == 4,
        };
        if !valid_count {
            return Err(BootstrapError::InvalidPlayerCount);
        }
        if roster
            .iter()
            .map(|entry| entry.player_id)
            .collect::<HashSet<_>>()
            .len()
            != roster.len()
        {
            return Err(BootstrapError::DuplicatePlayer);
        }

        let mut handles: Vec<_> = roster.iter().map(|entry| entry.handle).collect();
        handles.sort_unstable();
        if handles != (0..roster.len()).collect::<Vec<_>>() {
            return Err(BootstrapError::InvalidHandles);
        }

        if profiles.len() != roster.len()
            || profiles.iter().any(|profile| !profile.is_canonical())
            || profiles.iter().map(|profile| profile.player_id).collect::<Vec<_>>()
                != roster.iter().map(|entry| entry.player_id).collect::<Vec<_>>()
        {
            return Err(BootstrapError::InvalidProfiles);
        }

        if scores.len() != roster.len()
            || scores
                .iter()
                .map(|score| score.player_id)
                .collect::<Vec<_>>()
                != roster
                    .iter()
                    .map(|entry| entry.player_id)
                    .collect::<Vec<_>>()
        {
            return Err(BootstrapError::InvalidScores);
        }

        Ok(Self {
            protocol_version,
            match_id,
            match_seed,
            epoch,
            round,
            mode,
            roster,
            profiles,
            scores,
        })
    }

    pub fn handle(&self, handle: usize) -> Result<usize, BootstrapError> {
        self.roster
            .iter()
            .find(|entry| entry.handle == handle)
            .map(|entry| entry.handle)
            .ok_or(BootstrapError::InvalidHandles)
    }

    pub fn duel(match_seed: u64) -> Self {
        let roster: Vec<_> = (0..2)
            .map(|handle| RosterEntry {
                player_id: PlayerId(((match_seed as u128) << 8) | handle as u128),
                handle,
            })
            .collect();
        let profiles = roster.iter().map(|entry| PlayerProfile {
            player_id: entry.player_id,
            name: format!("Player {}", entry.handle + 1),
            palette_id: entry.handle as u8,
            cosmetic_id: 0,
        }).collect();
        let scores = roster
            .iter()
            .map(|entry| PlayerScore {
                player_id: entry.player_id,
                score: 0,
            })
            .collect();
        Self::new(
            1,
            MatchId(match_seed as u128),
            match_seed,
            SessionEpoch(0),
            RoundNumber(0),
            GameMode::Duel,
            roster,
            profiles,
            scores,
        )
        .expect("built-in duel bootstrap is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::SoundIdSeed;

    fn entry(id: u128, handle: usize) -> RosterEntry {
        RosterEntry { player_id: PlayerId(id), handle }
    }

    fn bootstrap(mode: GameMode, roster: Vec<RosterEntry>, score_ids: &[u128]) -> Result<RoundBootstrap, BootstrapError> {
        let profiles = roster.iter().map(|entry| PlayerProfile {
            player_id: entry.player_id,
            name: format!("Player {}", entry.player_id.0),
            palette_id: entry.handle as u8,
            cosmetic_id: 0,
        }).collect();
        RoundBootstrap::new(
            1,
            MatchId(7),
            9,
            SessionEpoch(0),
            RoundNumber(0),
            mode,
            roster,
            profiles,
            score_ids.iter().map(|id| PlayerScore { player_id: PlayerId(*id), score: 0 }).collect(),
        )
    }

    #[test]
    fn profile_names_are_sanitized_and_validated() {
        assert_eq!(PlayerProfile::sanitized_name("  Ghost\n  Rider  "), "Ghost Rider");
        assert!(PlayerProfile {
            player_id: PlayerId(1), name: "Ghost Rider".into(), palette_id: 0, cosmetic_id: 0,
        }.is_canonical());
        assert!(!PlayerProfile {
            player_id: PlayerId(1), name: " Ghost ".into(), palette_id: 0, cosmetic_id: 0,
        }.is_canonical());
    }

    #[test]
    fn canonicalizes_roster_and_scores_without_conflating_order_and_handles() {
        let value = bootstrap(GameMode::Duel, vec![entry(1, 1), entry(2, 0)], &[2, 1]).unwrap();
        assert_eq!(value.roster, vec![entry(1, 1), entry(2, 0)]);
        assert_eq!(value.handle(0), Ok(0));
        assert_eq!(value.handle(1), Ok(1));
        assert_eq!(value.scores.iter().map(|score| score.player_id).collect::<Vec<_>>(), vec![PlayerId(1), PlayerId(2)]);
    }

    #[test]
    fn sound_streams_preserve_duel_seeds_and_advance_independently() {
        let mut streams = SoundIdSeed::new(10, 4);
        assert_eq!(streams.0, vec![
            crate::game::SoundSeed::from_seed(11),
            crate::game::SoundSeed::from_seed(12),
            crate::game::SoundSeed::from_seed(13),
            crate::game::SoundSeed::from_seed(14),
        ]);
        let untouched = streams.0[1];
        streams.next(0);
        assert_eq!(streams.0[1], untouched);
    }

    fn ids(values: &[u128]) -> Vec<PlayerId> {
        values.iter().copied().map(PlayerId).collect()
    }

    fn winners(outcome: RoundOutcome) -> Vec<PlayerId> {
        match outcome {
            RoundOutcome::Complete { point_winners } => point_winners,
            RoundOutcome::InProgress => panic!("expected a completed round"),
        }
    }

    #[test]
    fn last_ghost_standing_scores_the_sole_survivor() {
        let roster = ids(&[1, 2, 3, 4]);
        let eliminated = roster[..roster.len() - 1].to_vec();
        assert_eq!(
            winners(round_outcome(GameMode::Deathmatch, &roster, &eliminated, &[])),
            vec![*roster.last().unwrap()],
        );
    }

    #[test]
    fn match_endpoint_is_first_to_three_and_tie_safe() {
        let scores = |values: &[(u128, u32)]| values.iter().map(|(id, score)| PlayerScore {
            player_id: PlayerId(*id), score: *score,
        }).collect::<Vec<_>>();
        assert_eq!(match_winner(&scores(&[(1, 2), (2, 2)])), None);
        assert_eq!(match_winner(&scores(&[(1, 3), (2, 2)])), Some(MatchWinner { player_id: PlayerId(1), score: 3 }));
        assert_eq!(match_winner(&scores(&[(9, 3), (2, 3)])), Some(MatchWinner { player_id: PlayerId(2), score: 3 }));
        assert_eq!(mode_label(GameMode::Deathmatch), "Last Ghost Standing — First to 3");
    }

    #[test]
    fn simultaneous_outcomes_keep_duel_semantics_but_last_ghost_standing_has_no_winner() {
        let roster = ids(&[10, 20]);
        assert_eq!(
            winners(round_outcome(GameMode::Duel, &roster, &roster, &[])),
            roster,
        );
        let four = ids(&[1, 2, 3, 4]);
        assert!(winners(round_outcome(GameMode::Deathmatch, &four, &four, &[])).is_empty());
    }

    #[test]
    fn round_policy_is_input_order_independent_and_disconnects_are_eliminations() {
        let roster = ids(&[40, 10, 30, 20]);
        let first = round_outcome(
            GameMode::Deathmatch,
            &roster,
            &ids(&[30, 10]),
            &ids(&[20]),
        );
        let second = round_outcome(
            GameMode::Deathmatch,
            &ids(&[20, 30, 10, 40]),
            &ids(&[10]),
            &ids(&[30, 20]),
        );
        assert_eq!(first, second);
        assert_eq!(winners(first), ids(&[40]));

        assert_eq!(
            winners(round_outcome(GameMode::Duel, &ids(&[2, 1]), &[], &ids(&[1]))),
            ids(&[2]),
        );
    }

    #[test]
    fn last_ghost_standing_stays_in_progress_while_multiple_players_survive() {
        assert_eq!(
            round_outcome(GameMode::Deathmatch, &ids(&[1, 2, 3, 4]), &ids(&[2]), &[]),
            RoundOutcome::InProgress,
        );
    }

    #[test]
    fn rejects_invalid_rosters() {
        assert_eq!(bootstrap(GameMode::Duel, vec![entry(1, 0)], &[1]), Err(BootstrapError::InvalidPlayerCount));
        assert_eq!(bootstrap(GameMode::Duel, vec![entry(1, 0), entry(1, 1)], &[1, 1]), Err(BootstrapError::DuplicatePlayer));
        assert_eq!(bootstrap(GameMode::Duel, vec![entry(1, 0), entry(2, 2)], &[1, 2]), Err(BootstrapError::InvalidHandles));
        assert_eq!(bootstrap(GameMode::Duel, vec![entry(1, 0), entry(2, 1)], &[1, 3]), Err(BootstrapError::InvalidScores));
        assert_eq!(bootstrap(GameMode::Deathmatch, (0..3).map(|id| entry(id, id as usize)).collect(), &[0, 1, 2]), Err(BootstrapError::InvalidPlayerCount));
        assert_eq!(bootstrap(GameMode::Deathmatch, (0..5).map(|id| entry(id, id as usize)).collect(), &[0, 1, 2, 3, 4]), Err(BootstrapError::InvalidPlayerCount));
    }
}
