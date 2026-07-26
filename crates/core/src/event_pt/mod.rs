use crate::EventType;
use thiserror::Error;

pub const POINT_BONUS_BASE_BASIS_POINTS: u64 = 10_000;
pub const MEDLEY_SCORE_DIVISOR: u64 = 18_500;
pub const MEDLEY_THREE_SONG_FIXED_PT: u64 = 100;

const MISSION_FIXED_PT: u64 = 120;
const MISSION_PERSONAL_SCORE_DIVISOR: u64 = 15_000;
const MISSION_OTHER_SCORE_DIVISOR: u64 = 150_000;
const LIVE_TRY_FIXED_PT: u64 = 130;
const LIVE_TRY_PERSONAL_SCORE_DIVISOR: u64 = 26_000;
const LIVE_TRY_OTHER_SCORE_DIVISOR: u64 = 260_000;
const CHALLENGE_FIXED_PT: u64 = 70;
const CHALLENGE_PERSONAL_SCORE_DIVISOR: u64 = 50_000;
const CHALLENGE_OTHER_SCORE_DIVISOR: u64 = 500_000;
const VERSUS_SOLO_FIXED_PT: u64 = 100;
const VERSUS_SOLO_SCORE_DIVISOR: u64 = 9_750;
const FESTIVAL_SOLO_FIXED_PT: u64 = 80;
const FESTIVAL_SOLO_SCORE_DIVISOR: u64 = 14_000;
const MULTIPLAYER_SCORE_DIVISOR: u64 = 6_500;
const FESTIVAL_MULTIPLAYER_FIXED_PT: u64 = 50;
const FESTIVAL_WIN_PT: u64 = 125;
const VERSUS_RANK_PT: [u64; 5] = [200, 173, 146, 123, 100];
const FESTIVAL_RANK_PT: [u64; 5] = [125, 117, 110, 105, 100];
const CHALLENGE_CP_FIXED_PT: u64 = 3_250;
const CHALLENGE_CP_SCORE_DIVISOR: u64 = 450;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EventPtError {
    #[error("event type {event_type:?} is not supported by this event PT formula")]
    UnsupportedEventType { event_type: EventType },

    #[error("team rank {rank} is invalid; expected a zero-based rank in 0..5")]
    InvalidTeamRank { rank: u8 },

    #[error("total score {total_score} is lower than personal score {personal_score}")]
    TotalScoreBelowPersonalScore {
        personal_score: u64,
        total_score: u64,
    },
}

pub fn apply_point_bonus(value: u64, bonus_basis_points: u32) -> u64 {
    value.saturating_mul(POINT_BONUS_BASE_BASIS_POINTS + bonus_basis_points as u64)
        / POINT_BONUS_BASE_BASIS_POINTS
}

pub fn solo_points(
    event_type: EventType,
    personal_score: i32,
    point_bonus_basis_points: u32,
    mission_support_pt_bonus: u64,
) -> Result<u64, EventPtError> {
    let score = non_negative_score(personal_score);
    let points = match event_type {
        EventType::MissionLive => apply_point_bonus(
            MISSION_FIXED_PT + score / MISSION_PERSONAL_SCORE_DIVISOR,
            point_bonus_basis_points,
        )
        .saturating_add(mission_support_pt_bonus),
        EventType::LiveTry => apply_point_bonus(
            LIVE_TRY_FIXED_PT + score / LIVE_TRY_PERSONAL_SCORE_DIVISOR,
            point_bonus_basis_points,
        ),
        EventType::Challenge => apply_point_bonus(
            CHALLENGE_FIXED_PT + score / CHALLENGE_PERSONAL_SCORE_DIVISOR,
            point_bonus_basis_points,
        ),
        EventType::Versus => VERSUS_SOLO_FIXED_PT + score / VERSUS_SOLO_SCORE_DIVISOR,
        EventType::Festival => apply_point_bonus(
            FESTIVAL_SOLO_FIXED_PT + score / FESTIVAL_SOLO_SCORE_DIVISOR,
            point_bonus_basis_points,
        ),
        EventType::Medley => return Err(EventPtError::UnsupportedEventType { event_type }),
    };
    Ok(points)
}

pub fn cooperative_points(
    event_type: EventType,
    personal_score: i32,
    total_score: i64,
    point_bonus_basis_points: u32,
    mission_support_pt_bonus: u64,
) -> Result<u64, EventPtError> {
    let personal_score = non_negative_score(personal_score);
    let total_score = total_score.max(0) as u64;
    let other_score = total_score.checked_sub(personal_score).ok_or(
        EventPtError::TotalScoreBelowPersonalScore {
            personal_score,
            total_score,
        },
    )?;
    let base = match event_type {
        EventType::MissionLive => {
            MISSION_FIXED_PT
                + personal_score / MISSION_PERSONAL_SCORE_DIVISOR
                + other_score / MISSION_OTHER_SCORE_DIVISOR
        }
        EventType::LiveTry => {
            LIVE_TRY_FIXED_PT
                + personal_score / LIVE_TRY_PERSONAL_SCORE_DIVISOR
                + other_score / LIVE_TRY_OTHER_SCORE_DIVISOR
        }
        EventType::Challenge => {
            CHALLENGE_FIXED_PT
                + personal_score / CHALLENGE_PERSONAL_SCORE_DIVISOR
                + other_score / CHALLENGE_OTHER_SCORE_DIVISOR
        }
        EventType::Medley | EventType::Versus | EventType::Festival => {
            return Err(EventPtError::UnsupportedEventType { event_type });
        }
    };
    let points = apply_point_bonus(base, point_bonus_basis_points);
    Ok(if event_type == EventType::MissionLive {
        points.saturating_add(mission_support_pt_bonus)
    } else {
        points
    })
}

pub fn versus_multiplayer_points(score: i32, team_rank: u8) -> Result<u64, EventPtError> {
    let rank_points = rank_points(VERSUS_RANK_PT, team_rank)?;
    Ok(non_negative_score(score) / MULTIPLAYER_SCORE_DIVISOR + rank_points)
}

pub fn festival_multiplayer_points(
    score: i32,
    team_rank: u8,
    won: bool,
) -> Result<u64, EventPtError> {
    let rank_points = rank_points(FESTIVAL_RANK_PT, team_rank)?;
    Ok(FESTIVAL_MULTIPLAYER_FIXED_PT
        + non_negative_score(score) / MULTIPLAYER_SCORE_DIVISOR
        + rank_points
        + u64::from(won) * FESTIVAL_WIN_PT)
}

pub fn medley_three_song_points(total_score: i64) -> u64 {
    MEDLEY_THREE_SONG_FIXED_PT + total_score.max(0) as u64 / MEDLEY_SCORE_DIVISOR
}

pub fn challenge_cp_points(score: i32) -> u64 {
    CHALLENGE_CP_FIXED_PT + non_negative_score(score) / CHALLENGE_CP_SCORE_DIVISOR
}

pub fn challenge_cp_gain(event_points: u64) -> u64 {
    event_points / 20 + u64::from(event_points % 20 != 0)
}

fn non_negative_score(score: i32) -> u64 {
    score.max(0) as u64
}

fn rank_points<const N: usize>(values: [u64; N], rank: u8) -> Result<u64, EventPtError> {
    values
        .get(rank as usize)
        .copied()
        .ok_or(EventPtError::InvalidTeamRank { rank })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_formula_workbook_examples() {
        assert_eq!(
            cooperative_points(EventType::MissionLive, 4_100_000, 20_000_000, 35_600, 97).unwrap(),
            2_372,
        );
        assert_eq!(
            cooperative_points(EventType::LiveTry, 157_000, 157_000, 16_600, 0).unwrap(),
            361,
        );
        assert_eq!(
            cooperative_points(EventType::Challenge, 0, 0, 30_600, 0).unwrap(),
            284,
        );
        assert_eq!(versus_multiplayer_points(3_020_000, 0).unwrap(), 664);
        assert_eq!(
            festival_multiplayer_points(3_654_786, 4, false).unwrap(),
            712,
        );
        assert_eq!(challenge_cp_points(1_700_000), 7_027);
        assert_eq!(medley_three_song_points(4_500_000), 343);
    }

    #[test]
    fn mission_support_is_added_after_point_bonus() {
        let without_support =
            cooperative_points(EventType::MissionLive, 4_100_000, 20_000_000, 35_600, 0).unwrap();
        assert_eq!(without_support, 2_275);
        assert_eq!(
            cooperative_points(EventType::MissionLive, 4_100_000, 20_000_000, 35_600, 97).unwrap(),
            without_support + 97,
        );
    }

    #[test]
    fn festival_solo_applies_event_bonus_as_point_multiplier() {
        assert_eq!(solo_points(EventType::Festival, 140_000, 5_000, 0), Ok(135));
    }

    #[test]
    fn rejects_invalid_multiplayer_inputs() {
        assert_eq!(
            versus_multiplayer_points(0, 5),
            Err(EventPtError::InvalidTeamRank { rank: 5 }),
        );
        assert!(matches!(
            cooperative_points(EventType::LiveTry, 100, 99, 0, 0),
            Err(EventPtError::TotalScoreBelowPersonalScore { .. })
        ));
    }
}
