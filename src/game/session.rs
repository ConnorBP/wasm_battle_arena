use std::collections::HashSet;

use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RosterEntry {
    pub player_id: PlayerId,
    pub handle: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub scores: Vec<PlayerScore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapError {
    InvalidProtocol,
    InvalidPlayerCount,
    DuplicatePlayer,
    InvalidHandles,
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
        mut scores: Vec<PlayerScore>,
    ) -> Result<Self, BootstrapError> {
        roster.sort_by_key(|entry| entry.player_id);
        scores.sort_by_key(|entry| entry.player_id);

        if protocol_version == 0 {
            return Err(BootstrapError::InvalidProtocol);
        }
        let valid_count = match mode {
            GameMode::Duel => roster.len() == 2,
            GameMode::Deathmatch => (2..=4).contains(&roster.len()),
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
            scores,
        )
        .expect("built-in duel bootstrap is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u128, handle: usize) -> RosterEntry {
        RosterEntry { player_id: PlayerId(id), handle }
    }

    fn bootstrap(mode: GameMode, roster: Vec<RosterEntry>, score_ids: &[u128]) -> Result<RoundBootstrap, BootstrapError> {
        RoundBootstrap::new(
            1,
            MatchId(7),
            9,
            SessionEpoch(0),
            RoundNumber(0),
            mode,
            roster,
            score_ids.iter().map(|id| PlayerScore { player_id: PlayerId(*id), score: 0 }).collect(),
        )
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
    fn rejects_invalid_rosters() {
        assert_eq!(bootstrap(GameMode::Duel, vec![entry(1, 0)], &[1]), Err(BootstrapError::InvalidPlayerCount));
        assert_eq!(bootstrap(GameMode::Duel, vec![entry(1, 0), entry(1, 1)], &[1, 1]), Err(BootstrapError::DuplicatePlayer));
        assert_eq!(bootstrap(GameMode::Duel, vec![entry(1, 0), entry(2, 2)], &[1, 2]), Err(BootstrapError::InvalidHandles));
        assert_eq!(bootstrap(GameMode::Duel, vec![entry(1, 0), entry(2, 1)], &[1, 3]), Err(BootstrapError::InvalidScores));
        assert_eq!(bootstrap(GameMode::Deathmatch, (0..5).map(|id| entry(id, id as usize)).collect(), &[0, 1, 2, 3, 4]), Err(BootstrapError::InvalidPlayerCount));
    }
}
