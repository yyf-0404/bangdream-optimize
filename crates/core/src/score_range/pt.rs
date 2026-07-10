use super::FIRE_MULTIPLIERS;
use crate::EventType;
use thiserror::Error;

const POINT_BONUS_BASE_BASIS_POINTS: u64 = 10_000;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ScoreRangePtError {
    #[error("event type {event_type:?} is not supported by score range")]
    UnsupportedEventType { event_type: EventType },

    #[error("fire multiplier {value} is invalid")]
    InvalidFireMultiplier { value: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreInterval {
    pub min_score: u64,
    pub max_score: u64,
}

pub fn score_interval_for_points(
    event_type: EventType,
    points: u64,
    point_bonus_basis_points: u32,
    fire_multiplier: u32,
) -> Result<Option<ScoreInterval>, ScoreRangePtError> {
    score_interval_for_points_with_support(
        event_type,
        points,
        point_bonus_basis_points,
        fire_multiplier,
        0,
    )
}

pub fn score_interval_for_points_with_support(
    event_type: EventType,
    points: u64,
    point_bonus_basis_points: u32,
    fire_multiplier: u32,
    mission_support_pt_bonus: u64,
) -> Result<Option<ScoreInterval>, ScoreRangePtError> {
    if !FIRE_MULTIPLIERS.contains(&fire_multiplier) {
        return Err(ScoreRangePtError::InvalidFireMultiplier {
            value: fire_multiplier,
        });
    }
    let fire = fire_multiplier as u64;
    if points % fire != 0 {
        return Ok(None);
    }
    let base_points = points / fire;
    let interval = match event_type {
        EventType::Versus => simple_interval(base_points, 100, 9_750),
        EventType::Medley => simple_interval(base_points, 30, 18_500),
        EventType::Festival => simple_interval(base_points, 80, 14_000),
        EventType::LiveTry => bonus_interval(base_points, 130, 26_000, point_bonus_basis_points),
        EventType::Challenge => bonus_interval(base_points, 70, 50_000, point_bonus_basis_points),
        EventType::MissionLive => base_points
            .checked_sub(mission_support_pt_bonus)
            .and_then(|points| bonus_interval(points, 120, 15_000, point_bonus_basis_points)),
    };
    Ok(interval)
}

fn simple_interval(points: u64, fixed: u64, divisor: u64) -> Option<ScoreInterval> {
    let quotient = points.checked_sub(fixed)?;
    Some(ScoreInterval {
        min_score: quotient.saturating_mul(divisor),
        max_score: quotient
            .saturating_add(1)
            .saturating_mul(divisor)
            .saturating_sub(1),
    })
}

fn bonus_interval(
    points: u64,
    fixed: u64,
    divisor: u64,
    bonus_basis_points: u32,
) -> Option<ScoreInterval> {
    let multiplier = POINT_BONUS_BASE_BASIS_POINTS + bonus_basis_points as u64;
    let inner_min = div_ceil(
        points.saturating_mul(POINT_BONUS_BASE_BASIS_POINTS),
        multiplier,
    );
    let inner_max_exclusive = div_ceil(
        points
            .saturating_add(1)
            .saturating_mul(POINT_BONUS_BASE_BASIS_POINTS),
        multiplier,
    );
    let inner_max = inner_max_exclusive.saturating_sub(1);
    if inner_max < fixed {
        return None;
    }
    let inner_min = inner_min.max(fixed);
    if inner_min > inner_max {
        return None;
    }
    Some(ScoreInterval {
        min_score: inner_min.saturating_sub(fixed).saturating_mul(divisor),
        max_score: inner_max
            .saturating_sub(fixed)
            .saturating_add(1)
            .saturating_mul(divisor)
            .saturating_sub(1),
    })
}

fn div_ceil(value: u64, divisor: u64) -> u64 {
    value / divisor + u64::from(value % divisor != 0)
}

pub fn points_for_score(
    event_type: EventType,
    score: i32,
    point_bonus_basis_points: u32,
    fire_multiplier: u32,
) -> Result<u64, ScoreRangePtError> {
    points_for_score_with_support(
        event_type,
        score,
        point_bonus_basis_points,
        fire_multiplier,
        0,
    )
}

pub fn points_for_score_with_support(
    event_type: EventType,
    score: i32,
    point_bonus_basis_points: u32,
    fire_multiplier: u32,
    mission_support_pt_bonus: u64,
) -> Result<u64, ScoreRangePtError> {
    if !FIRE_MULTIPLIERS.contains(&fire_multiplier) {
        return Err(ScoreRangePtError::InvalidFireMultiplier {
            value: fire_multiplier,
        });
    }

    let score = score.max(0) as u64;
    let base_pt = match event_type {
        EventType::LiveTry => apply_point_bonus(130 + score / 26_000, point_bonus_basis_points),
        EventType::Challenge => apply_point_bonus(70 + score / 50_000, point_bonus_basis_points),
        EventType::Versus => 100 + score / 9_750,
        EventType::Medley => 30 + score / 18_500,
        EventType::Festival => 80 + score / 14_000,
        EventType::MissionLive => apply_point_bonus(
            120 + score / 15_000,
            point_bonus_basis_points,
        )
        .saturating_add(mission_support_pt_bonus),
    };

    Ok(base_pt.saturating_mul(fire_multiplier as u64))
}

fn apply_point_bonus(value: u64, bonus_basis_points: u32) -> u64 {
    value.saturating_mul(POINT_BONUS_BASE_BASIS_POINTS + bonus_basis_points as u64)
        / POINT_BONUS_BASE_BASIS_POINTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_live_try_point_bonus_after_score_floor() {
        assert_eq!(
            points_for_score(EventType::LiveTry, 2_600_000, 5_000, 5).unwrap(),
            1_725,
        );
    }

    #[test]
    fn challenge_uses_coop_formula() {
        assert_eq!(
            points_for_score(EventType::Challenge, 1_000_000, 2_000, 1).unwrap(),
            108,
        );
    }

    #[test]
    fn versus_keeps_fixed_hundred() {
        assert_eq!(
            points_for_score(EventType::Versus, 975_000, 0, 1).unwrap(),
            200
        );
        assert_eq!(points_for_score(EventType::Versus, 0, 0, 1).unwrap(), 100);
    }

    #[test]
    fn festival_uses_single_player_fixed_eighty_formula() {
        assert_eq!(
            points_for_score(EventType::Festival, 1_400_000, 0, 5).unwrap(),
            900,
        );
        assert_eq!(
            score_interval_for_points(EventType::Festival, 180, 0, 1).unwrap(),
            Some(ScoreInterval {
                min_score: 1_400_000,
                max_score: 1_413_999,
            }),
        );
    }

    #[test]
    fn mission_support_bonus_is_added_after_event_multiplier() {
        let points = points_for_score_with_support(
            EventType::MissionLive,
            1_500_000,
            5_000,
            5,
            97,
        )
        .unwrap();
        assert_eq!(points, (330 + 97) * 5);
        let interval = score_interval_for_points_with_support(
            EventType::MissionLive,
            points,
            5_000,
            5,
            97,
        )
        .unwrap()
        .unwrap();
        assert!(interval.min_score <= 1_500_000);
        assert!(interval.max_score >= 1_500_000);
    }

    #[test]
    fn medley_uses_first_song_base_and_single_song_fire_mapping() {
        assert_eq!(
            points_for_score(EventType::Medley, 185_000, 0, 10).unwrap(),
            400
        );
    }

    #[test]
    fn reverses_versus_floor_to_inclusive_score_interval() {
        assert_eq!(
            score_interval_for_points(EventType::Versus, 200, 0, 1).unwrap(),
            Some(ScoreInterval {
                min_score: 975_000,
                max_score: 984_749,
            })
        );
    }

    #[test]
    fn reverses_nested_point_bonus_floors() {
        let interval = score_interval_for_points(EventType::LiveTry, 345, 5_000, 1)
            .unwrap()
            .unwrap();
        assert!(interval.min_score <= 2_600_000);
        assert!(interval.max_score >= 2_600_000);
        assert_eq!(
            points_for_score(EventType::LiveTry, interval.min_score as i32, 5_000, 1).unwrap(),
            345
        );
        assert_eq!(
            points_for_score(EventType::LiveTry, interval.max_score as i32, 5_000, 1).unwrap(),
            345
        );
    }

    #[test]
    fn every_supported_single_player_formula_round_trips_to_a_score_interval() {
        for event_type in [
            EventType::MissionLive,
            EventType::LiveTry,
            EventType::Challenge,
            EventType::Versus,
            EventType::Festival,
            EventType::Medley,
        ] {
            let bonus = if matches!(
                event_type,
                EventType::MissionLive | EventType::LiveTry | EventType::Challenge
            ) {
                3_750
            } else {
                0
            };
            let support = u64::from(event_type == EventType::MissionLive) * 97;
            for fire in FIRE_MULTIPLIERS {
                for score in [0, 1, 9_749, 500_000, 2_345_678] {
                    let points = points_for_score_with_support(
                        event_type,
                        score,
                        bonus,
                        fire,
                        support,
                    )
                    .unwrap();
                    let interval = score_interval_for_points_with_support(
                        event_type,
                        points,
                        bonus,
                        fire,
                        support,
                    )
                    .unwrap()
                    .unwrap();
                    assert!(interval.min_score <= score as u64, "{event_type:?}");
                    assert!(score as u64 <= interval.max_score, "{event_type:?}");
                }
            }
        }
    }
}
