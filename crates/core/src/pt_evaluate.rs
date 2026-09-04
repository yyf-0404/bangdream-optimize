//! Exact event-PT evaluation for user-specified teams.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::pt_maximize::distribution::{
    evaluate_fixed_distribution, fixed_captain_score_distribution,
};
use crate::{
    event_bonus_application, floor_team_stat, medley_three_song_points, AreaItemPercent, AveragePt,
    EventBonusApplication, EventType, FixedTeamPtScenario, LiveVariant, PreparedCard,
    PtMaximizeError, PtMaximizeMedleyResult, PtMaximizeMedleyTeamResult, PtMaximizeScenarioSummary,
    PtMaximizeTeamResult, ScoreHistogram, SelectedAreaItems, SongMode, SongSelection, VersusInput,
};

pub const FIXED_CAPTAIN_INDEX: usize = 2;
const TEAM_SIZE: usize = 5;
const MEDLEY_SONG_COUNT: usize = 3;
const POINT_BONUS_MICROS_PER_BASIS_POINT: u64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "mode",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PtEvaluateScoreMode {
    Manual,
    Auto { base_multiplier: f64 },
}

impl Default for PtEvaluateScoreMode {
    fn default() -> Self {
        Self::Manual
    }
}

impl PtEvaluateScoreMode {
    pub fn validate(self) -> Result<Self, PtEvaluateError> {
        match self {
            Self::Manual => Ok(self),
            Self::Auto { base_multiplier }
                if base_multiplier.is_finite() && matches!(base_multiplier, 0.5 | 0.75) =>
            {
                Ok(self)
            }
            Self::Auto { base_multiplier } => {
                Err(PtEvaluateError::InvalidAutoBaseMultiplier { base_multiplier })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecifiedTeam {
    pub card_ids: [u32; TEAM_SIZE],
    pub captain_card_id: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtEvaluateRequest {
    pub event_type: EventType,
    pub live_variant: LiveVariant,
    pub songs: Vec<SongSelection>,
    pub teams: Vec<SpecifiedTeam>,
    pub items: SelectedAreaItems,
    #[serde(default)]
    pub score_mode: PtEvaluateScoreMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_support_pt_bonus: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versus: Option<VersusInput>,
}

impl PtEvaluateRequest {
    pub fn scenario_summary(&self) -> PtMaximizeScenarioSummary {
        PtMaximizeScenarioSummary {
            includes_fever: false,
            minimum_personal_stat: None,
            mission_support_pt_bonus: (self.event_type == EventType::MissionLive)
                .then_some(self.mission_support_pt_bonus.unwrap_or_default()),
            cooperative: None,
            versus: self.versus,
            festival: None,
        }
    }

    pub fn validate_shape(&self) -> Result<(), PtEvaluateError> {
        self.score_mode.validate()?;
        if !supports_pt_evaluate_variant(self.event_type, self.live_variant) {
            return Err(PtEvaluateError::UnsupportedLiveVariant {
                event_type: self.event_type,
                live_variant: self.live_variant,
            });
        }
        if matches!(self.score_mode, PtEvaluateScoreMode::Auto { .. })
            && !supports_pt_evaluate_auto(self.live_variant)
        {
            return Err(PtEvaluateError::AutoUnsupportedLiveVariant {
                live_variant: self.live_variant,
            });
        }
        let expected = if self.live_variant == LiveVariant::Medley {
            MEDLEY_SONG_COUNT
        } else {
            1
        };
        if self.songs.len() != expected {
            return Err(PtEvaluateError::InvalidSongCount {
                expected,
                actual: self.songs.len(),
            });
        }
        if self.teams.len() != expected {
            return Err(PtEvaluateError::InvalidTeamCount {
                expected,
                actual: self.teams.len(),
            });
        }
        if self.event_type == EventType::MissionLive && self.mission_support_pt_bonus.is_none() {
            return Err(PtEvaluateError::MissingMissionSupportPtBonus);
        }
        if self.live_variant == LiveVariant::Versus {
            let rank = self
                .versus
                .ok_or(PtEvaluateError::MissingVersusInput)?
                .team_rank;
            if rank >= 5 {
                return Err(PtEvaluateError::InvalidTeamRank { rank });
            }
        }
        for (team_index, team) in self.teams.iter().enumerate() {
            if team.captain_card_id != team.card_ids[FIXED_CAPTAIN_INDEX] {
                return Err(PtEvaluateError::CaptainMustBeThird {
                    team_index,
                    captain_card_id: team.captain_card_id,
                    third_card_id: team.card_ids[FIXED_CAPTAIN_INDEX],
                });
            }
            let unique = team.card_ids.iter().copied().collect::<BTreeSet<_>>();
            if unique.len() != TEAM_SIZE {
                return Err(PtEvaluateError::DuplicateCard { team_index });
            }
        }
        if self.live_variant == LiveVariant::Medley {
            let unique = self
                .teams
                .iter()
                .flat_map(|team| team.card_ids)
                .collect::<BTreeSet<_>>();
            if unique.len() != MEDLEY_SONG_COUNT * TEAM_SIZE {
                return Err(PtEvaluateError::MedleyCardConflict);
            }
        }
        Ok(())
    }
}

pub const fn supports_pt_evaluate_auto(live_variant: LiveVariant) -> bool {
    matches!(live_variant, LiveVariant::Solo | LiveVariant::Medley)
}

pub fn supports_pt_evaluate_variant(event_type: EventType, live_variant: LiveVariant) -> bool {
    matches!(
        (event_type, live_variant),
        (EventType::MissionLive, LiveVariant::Solo)
            | (EventType::LiveTry, LiveVariant::Solo)
            | (
                EventType::Challenge,
                LiveVariant::Solo | LiveVariant::ChallengeCp
            )
            | (EventType::Versus, LiveVariant::Solo | LiveVariant::Versus)
            | (EventType::Festival, LiveVariant::Solo)
            | (EventType::Medley, LiveVariant::Medley)
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtEvaluateMetrics {
    pub core_version: String,
    pub total_elapsed_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtEvaluateResult {
    pub event_id: u32,
    pub event_type: EventType,
    pub live_variant: LiveVariant,
    pub songs: Vec<SongSelection>,
    pub scenario: PtMaximizeScenarioSummary,
    pub score_mode: PtEvaluateScoreMode,
    pub metrics: PtEvaluateMetrics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<PtMaximizeTeamResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medley: Option<PtMaximizeMedleyResult>,
}

pub fn evaluate_specified_teams(
    cards: &[PreparedCard],
    charts: &[crate::Chart],
    area_item_percent: &AreaItemPercent,
    point_bonus_micros: &BTreeMap<u32, u64>,
    request: &PtEvaluateRequest,
) -> Result<(Option<PtMaximizeTeamResult>, Option<PtMaximizeMedleyResult>), PtEvaluateError> {
    request.validate_shape()?;
    validate_selected_items(area_item_percent, &request.items)?;

    let application = event_bonus_application(request.event_type, request.live_variant);
    let mut resolved = Vec::with_capacity(request.teams.len());
    for (team_index, team) in request.teams.iter().enumerate() {
        let selected = resolve_team(cards, team, team_index)?;
        let character_count = selected
            .iter()
            .map(|card| card.character_id)
            .collect::<BTreeSet<_>>()
            .len();
        if character_count != TEAM_SIZE {
            return Err(PtEvaluateError::DuplicateCharacter { team_index });
        }
        let mode = selected_team_mode(&selected);
        let skills = selected
            .map(|card| mode.resolve_skill(card))
            .transpose_array()?;
        let total_stat = floor_team_stat(selected.iter().map(|card| {
            card.add_up_stat(
                area_item_percent,
                &request.items.band,
                &request.items.attribute,
                request.items.magazine.as_str(),
            )
        }));
        let point_bonus_basis_points = if application == EventBonusApplication::PointMultiplier {
            point_bonus_basis_points(selected.iter().map(|card| {
                point_bonus_micros
                    .get(&card.card_id)
                    .copied()
                    .unwrap_or_default()
            }))
        } else {
            0
        };
        resolved.push((skills, total_stat, point_bonus_basis_points));
    }

    if request.live_variant != LiveVariant::Medley {
        let (skills, total_stat, point_bonus_basis_points) = resolved[0];
        let scenario = single_scenario(request, point_bonus_basis_points)?;
        let distribution = fixed_captain_score_distribution(
            &charts[0],
            &skills,
            total_stat,
            false,
            FIXED_CAPTAIN_INDEX,
        )?;
        let evaluation = evaluate_fixed_distribution(distribution, scenario)?;
        return Ok((
            Some(PtMaximizeTeamResult {
                team_card_ids: request.teams[0].card_ids.to_vec(),
                captain_card_id: request.teams[0].captain_card_id,
                total_stat,
                point_bonus_basis_points,
                items: request.items.clone(),
                evaluation,
            }),
            None,
        ));
    }

    let mut team_results = Vec::with_capacity(MEDLEY_SONG_COUNT);
    for song in 0..MEDLEY_SONG_COUNT {
        let (skills, total_stat, _) = resolved[song];
        let distribution = fixed_captain_score_distribution(
            &charts[song],
            &skills,
            total_stat,
            true,
            FIXED_CAPTAIN_INDEX,
        )?;
        team_results.push(PtMaximizeMedleyTeamResult {
            team_card_ids: request.teams[song].card_ids.to_vec(),
            captain_card_id: request.teams[song].captain_card_id,
            total_stat,
            items: request.items.clone(),
            score_distribution: distribution.distribution,
        });
    }
    let medley = fixed_medley_result(team_results)?;
    Ok((None, Some(medley)))
}

pub fn evaluate_specified_teams_with_elapsed(
    cards: &[PreparedCard],
    charts: &[crate::Chart],
    area_item_percent: &AreaItemPercent,
    point_bonus_micros: &BTreeMap<u32, u64>,
    request: &PtEvaluateRequest,
) -> Result<
    (
        Option<PtMaximizeTeamResult>,
        Option<PtMaximizeMedleyResult>,
        f64,
    ),
    PtEvaluateError,
> {
    let started = crate::timing::Timer::start();
    let (team, medley) = evaluate_specified_teams(
        cards,
        charts,
        area_item_percent,
        point_bonus_micros,
        request,
    )?;
    Ok((team, medley, started.elapsed_ms()))
}

fn resolve_team<'a>(
    cards: &'a [PreparedCard],
    team: &SpecifiedTeam,
    team_index: usize,
) -> Result<[&'a PreparedCard; TEAM_SIZE], PtEvaluateError> {
    team.card_ids
        .map(|card_id| {
            cards
                .iter()
                .find(|card| card.card_id == card_id)
                .ok_or(PtEvaluateError::MissingCard {
                    team_index,
                    card_id,
                })
        })
        .transpose_array()
}

fn selected_team_mode(cards: &[&PreparedCard; TEAM_SIZE]) -> SongMode {
    let band_id = cards[0].band_id;
    let attribute = cards[0].attribute;
    let unified_band = cards.iter().all(|card| card.band_id == band_id);
    let unified_attribute = cards.iter().all(|card| card.attribute == attribute);
    match (unified_band, unified_attribute) {
        (true, true) => SongMode::UnifiedBandAttribute(band_id, attribute),
        (true, false) => SongMode::UnifiedBand(band_id),
        (false, true) => SongMode::UnifiedAttribute(attribute),
        (false, false) => SongMode::Mixed,
    }
}

fn single_scenario(
    request: &PtEvaluateRequest,
    point_bonus_basis_points: u32,
) -> Result<FixedTeamPtScenario, PtEvaluateError> {
    Ok(match request.live_variant {
        LiveVariant::Solo => FixedTeamPtScenario::Solo {
            event_type: request.event_type,
            point_bonus_basis_points,
            mission_support_pt_bonus: request.mission_support_pt_bonus.unwrap_or_default(),
        },
        LiveVariant::Versus => FixedTeamPtScenario::Versus {
            team_rank: request
                .versus
                .ok_or(PtEvaluateError::MissingVersusInput)?
                .team_rank,
        },
        LiveVariant::ChallengeCp => FixedTeamPtScenario::ChallengeCp,
        other => {
            return Err(PtEvaluateError::UnsupportedLiveVariant {
                event_type: request.event_type,
                live_variant: other,
            });
        }
    })
}

fn point_bonus_basis_points(values: impl IntoIterator<Item = u64>) -> u32 {
    let micros = values
        .into_iter()
        .fold(0u64, |sum, value| sum.saturating_add(value));
    ((micros.saturating_add(POINT_BONUS_MICROS_PER_BASIS_POINT / 2))
        / POINT_BONUS_MICROS_PER_BASIS_POINT)
        .min(u64::from(u32::MAX)) as u32
}

fn validate_selected_items(
    area: &AreaItemPercent,
    items: &SelectedAreaItems,
) -> Result<(), PtEvaluateError> {
    let checks = [
        ("band", area.band.get(&items.band)),
        ("attribute", area.attribute.get(&items.attribute)),
        ("magazine", area.magazine.get(items.magazine.as_str())),
    ];
    for (kind, value) in checks {
        let enabled = value.is_some_and(|rate| {
            rate.performance > 0.0 || rate.technique > 0.0 || rate.visual > 0.0
        });
        if !enabled {
            return Err(PtEvaluateError::UnavailableAreaItem {
                kind,
                key: match kind {
                    "band" => items.band.clone(),
                    "attribute" => items.attribute.clone(),
                    _ => items.magazine.as_str().to_owned(),
                },
            });
        }
    }
    Ok(())
}

fn fixed_medley_result(
    teams: Vec<PtMaximizeMedleyTeamResult>,
) -> Result<PtMaximizeMedleyResult, PtEvaluateError> {
    let histograms: [&ScoreHistogram; MEDLEY_SONG_COUNT] =
        std::array::from_fn(|index| &teams[index].score_distribution);
    let (pt_sum, sample_count) = medley_pt_sum(histograms)?;
    let min_score = histograms
        .iter()
        .map(|value| i64::from(value.min_score))
        .sum();
    let max_score = histograms
        .iter()
        .map(|value| i64::from(value.max_score))
        .sum();
    let total_score_sum = histograms
        .iter()
        .map(|value| i128::from(value.score_sum) * i128::from(sample_count / value.sample_count))
        .sum();
    Ok(PtMaximizeMedleyResult {
        teams,
        average_pt: AveragePt::new(pt_sum, sample_count)?,
        min_pt: medley_three_song_points(min_score),
        max_pt: medley_three_song_points(max_score),
        total_score_sum,
        sample_count,
    })
}

fn medley_pt_sum(
    histograms: [&ScoreHistogram; MEDLEY_SONG_COUNT],
) -> Result<(u128, u64), PtEvaluateError> {
    const DIVISOR: i32 = 18_500;
    if histograms.iter().any(|value| value.sample_count == 0) {
        return Err(PtEvaluateError::EmptyDistribution);
    }
    let sample_count = histograms
        .iter()
        .try_fold(1u64, |product, value| {
            product.checked_mul(value.sample_count)
        })
        .ok_or(PtEvaluateError::EmptyDistribution)?;
    let total_samples = u128::from(sample_count);
    let mut quotient_sums = [0u128; MEDLEY_SONG_COUNT];
    let remainders: [Vec<(i32, u64)>; MEDLEY_SONG_COUNT] = std::array::from_fn(|song| {
        let mut values = BTreeMap::new();
        for &(score, count) in &histograms[song].entries {
            quotient_sums[song] += (score.max(0) / DIVISOR) as u128 * u128::from(count);
            *values.entry(score.max(0) % DIVISOR).or_insert(0) += count;
        }
        values.into_iter().collect()
    });
    let mut pt_sum = 100u128 * total_samples;
    for song in 0..MEDLEY_SONG_COUNT {
        pt_sum += quotient_sums[song] * (total_samples / u128::from(histograms[song].sample_count));
    }
    let mut pair_remainders = BTreeMap::<i32, u64>::new();
    for &(left, left_count) in &remainders[0] {
        for &(right, right_count) in &remainders[1] {
            *pair_remainders.entry(left + right).or_insert(0) += left_count * right_count;
        }
    }
    for (pair, pair_count) in pair_remainders {
        for &(third, third_count) in &remainders[2] {
            pt_sum += ((pair + third) / DIVISOR) as u128
                * u128::from(pair_count)
                * u128::from(third_count);
        }
    }
    Ok((pt_sum, sample_count))
}

trait TransposeArray<T, E, const N: usize> {
    fn transpose_array(self) -> Result<[T; N], E>;
}

impl<T, E, const N: usize> TransposeArray<T, E, N> for [Result<T, E>; N] {
    fn transpose_array(self) -> Result<[T; N], E> {
        self.into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| unreachable!("array length is preserved"))
    }
}

#[derive(Debug, Error)]
pub enum PtEvaluateError {
    #[error(transparent)]
    PtMaximize(#[from] PtMaximizeError),
    #[error(transparent)]
    Model(#[from] crate::DpModelError),
    #[error("event type {event_type:?} does not support fixed-team live variant {live_variant:?}")]
    UnsupportedLiveVariant {
        event_type: EventType,
        live_variant: LiveVariant,
    },
    #[error("expected {expected} song(s), got {actual}")]
    InvalidSongCount { expected: usize, actual: usize },
    #[error("expected {expected} team(s), got {actual}")]
    InvalidTeamCount { expected: usize, actual: usize },
    #[error("team {team_index} contains duplicate cards")]
    DuplicateCard { team_index: usize },
    #[error("team {team_index} contains duplicate characters")]
    DuplicateCharacter { team_index: usize },
    #[error("card {card_id} in team {team_index} is missing from the player configuration")]
    MissingCard { team_index: usize, card_id: u32 },
    #[error(
        "team {team_index} captain {captain_card_id} must match third slot card {third_card_id}"
    )]
    CaptainMustBeThird {
        team_index: usize,
        captain_card_id: u32,
        third_card_id: u32,
    },
    #[error("Medley teams must not reuse the same physical card")]
    MedleyCardConflict,
    #[error("selected {kind} area-item group {key} is missing or contains a level-0 item")]
    UnavailableAreaItem { kind: &'static str, key: String },
    #[error("Auto base multiplier must be 0.5 or 0.75, got {base_multiplier}")]
    InvalidAutoBaseMultiplier { base_multiplier: f64 },
    #[error("Auto scoring is only supported for solo and medley fixed-team evaluation, got {live_variant:?}")]
    AutoUnsupportedLiveVariant { live_variant: LiveVariant },
    #[error("missionSupportPtBonus must be provided for mission_live fixed-team evaluation")]
    MissingMissionSupportPtBonus,
    #[error("versus input is required for versus live")]
    MissingVersusInput,
    #[error("team rank must be between 0 and 4, got {rank}")]
    InvalidTeamRank { rank: u8 },
    #[error("score distribution is empty")]
    EmptyDistribution,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Magazine;

    fn team(start: u32) -> SpecifiedTeam {
        let card_ids = [start, start + 1, start + 2, start + 3, start + 4];
        SpecifiedTeam {
            captain_card_id: card_ids[FIXED_CAPTAIN_INDEX],
            card_ids,
        }
    }

    fn request(event_type: EventType, live_variant: LiveVariant) -> PtEvaluateRequest {
        let count = if live_variant == LiveVariant::Medley {
            3
        } else {
            1
        };
        PtEvaluateRequest {
            event_type,
            live_variant,
            songs: (0..count)
                .map(|index| SongSelection {
                    song_id: index as u32 + 1,
                    difficulty: 3,
                })
                .collect(),
            teams: (0..count)
                .map(|index| team(index as u32 * 10 + 1))
                .collect(),
            items: SelectedAreaItems {
                band: "1".to_owned(),
                attribute: "happy".to_owned(),
                magazine: Magazine::Performance,
            },
            score_mode: PtEvaluateScoreMode::Manual,
            mission_support_pt_bonus: (event_type == EventType::MissionLive).then_some(100),
            versus: (live_variant == LiveVariant::Versus).then_some(VersusInput { team_rank: 0 }),
        }
    }

    #[test]
    fn supported_variants_exclude_cooperative_and_festival_multiplayer() {
        assert!(request(EventType::LiveTry, LiveVariant::Solo)
            .validate_shape()
            .is_ok());
        assert!(request(EventType::Challenge, LiveVariant::ChallengeCp)
            .validate_shape()
            .is_ok());
        assert!(request(EventType::Versus, LiveVariant::Versus)
            .validate_shape()
            .is_ok());
        assert!(request(EventType::Medley, LiveVariant::Medley)
            .validate_shape()
            .is_ok());
        assert!(matches!(
            request(EventType::LiveTry, LiveVariant::Cooperative).validate_shape(),
            Err(PtEvaluateError::UnsupportedLiveVariant { .. })
        ));
        assert!(matches!(
            request(EventType::Festival, LiveVariant::Festival).validate_shape(),
            Err(PtEvaluateError::UnsupportedLiveVariant { .. })
        ));
    }

    #[test]
    fn captain_must_be_the_third_display_slot() {
        let mut value = request(EventType::Challenge, LiveVariant::Solo);
        value.teams[0].captain_card_id = value.teams[0].card_ids[0];
        assert!(matches!(
            value.validate_shape(),
            Err(PtEvaluateError::CaptainMustBeThird { team_index: 0, .. })
        ));
    }

    #[test]
    fn medley_rejects_reusing_a_physical_card_across_teams() {
        let mut value = request(EventType::Medley, LiveVariant::Medley);
        value.teams[1].card_ids[0] = value.teams[0].card_ids[0];
        assert!(matches!(
            value.validate_shape(),
            Err(PtEvaluateError::MedleyCardConflict)
        ));
    }

    #[test]
    fn auto_multiplier_is_limited_to_known_server_rules() {
        let mut value = request(EventType::Challenge, LiveVariant::Solo);
        value.score_mode = PtEvaluateScoreMode::Auto {
            base_multiplier: 0.6,
        };
        assert!(matches!(
            value.validate_shape(),
            Err(PtEvaluateError::InvalidAutoBaseMultiplier { .. })
        ));
    }

    #[test]
    fn auto_is_only_supported_for_solo_and_medley() {
        for (event_type, live_variant) in [
            (EventType::Challenge, LiveVariant::Solo),
            (EventType::Medley, LiveVariant::Medley),
        ] {
            let mut value = request(event_type, live_variant);
            value.score_mode = PtEvaluateScoreMode::Auto {
                base_multiplier: 0.5,
            };
            assert!(value.validate_shape().is_ok());
        }

        for (event_type, live_variant) in [
            (EventType::Challenge, LiveVariant::ChallengeCp),
            (EventType::Versus, LiveVariant::Versus),
        ] {
            let mut value = request(event_type, live_variant);
            value.score_mode = PtEvaluateScoreMode::Auto {
                base_multiplier: 0.5,
            };
            assert!(matches!(
                value.validate_shape(),
                Err(PtEvaluateError::AutoUnsupportedLiveVariant {
                    live_variant: actual,
                }) if actual == live_variant
            ));
        }
    }
}
