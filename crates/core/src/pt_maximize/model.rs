use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ChartError, DpModelError, EventPtError, EventType, SelectedAreaItems, SongSelection};

pub const RANDOM_SKILL_ORDER_COUNT: u64 = 120;
pub const CHALLENGE_CP_COST: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveVariant {
    Solo,
    Cooperative,
    Versus,
    Festival,
    Medley,
    ChallengeCp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventBonusApplication {
    TeamStat,
    PointMultiplier,
}

pub fn supports_live_variant(event_type: EventType, live_variant: LiveVariant) -> bool {
    matches!(
        (event_type, live_variant),
        (
            EventType::MissionLive,
            LiveVariant::Solo | LiveVariant::Cooperative
        ) | (
            EventType::LiveTry,
            LiveVariant::Solo | LiveVariant::Cooperative
        ) | (
            EventType::Challenge,
            LiveVariant::Solo | LiveVariant::Cooperative
        ) | (EventType::Challenge, LiveVariant::ChallengeCp)
            | (EventType::Versus, LiveVariant::Solo | LiveVariant::Versus)
            | (
                EventType::Festival,
                LiveVariant::Solo | LiveVariant::Festival
            )
            | (EventType::Medley, LiveVariant::Medley)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AveragePt {
    pub pt_sum: u128,
    pub sample_count: u64,
}

impl AveragePt {
    pub fn new(pt_sum: u128, sample_count: u64) -> Result<Self, PtMaximizeError> {
        if sample_count == 0 {
            return Err(PtMaximizeError::EmptyDistribution);
        }
        Ok(Self {
            pt_sum,
            sample_count,
        })
    }

    pub fn as_f64(self) -> f64 {
        self.pt_sum as f64 / self.sample_count as f64
    }
}

impl PartialOrd for AveragePt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AveragePt {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_positive_fractions(
            self.pt_sum,
            self.sample_count as u128,
            other.pt_sum,
            other.sample_count as u128,
        )
    }
}

fn compare_positive_fractions(
    mut left_numerator: u128,
    mut left_denominator: u128,
    mut right_numerator: u128,
    mut right_denominator: u128,
) -> Ordering {
    debug_assert!(left_denominator > 0 && right_denominator > 0);
    let mut reversed = false;
    loop {
        let left_quotient = left_numerator / left_denominator;
        let right_quotient = right_numerator / right_denominator;
        let quotient_order = left_quotient.cmp(&right_quotient);
        if quotient_order != Ordering::Equal {
            return if reversed {
                quotient_order.reverse()
            } else {
                quotient_order
            };
        }

        let left_remainder = left_numerator % left_denominator;
        let right_remainder = right_numerator % right_denominator;
        match (left_remainder == 0, right_remainder == 0) {
            (true, true) => return Ordering::Equal,
            (true, false) => {
                return if reversed {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
            }
            (false, true) => {
                return if reversed {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            (false, false) => {}
        }

        left_numerator = left_denominator;
        left_denominator = left_remainder;
        right_numerator = right_denominator;
        right_denominator = right_remainder;
        reversed = !reversed;
    }
}

pub(crate) fn compare_nonnegative_averages(
    left_sum: i128,
    left_count: u64,
    right_sum: i128,
    right_count: u64,
) -> Ordering {
    debug_assert!(left_sum >= 0 && right_sum >= 0);
    compare_positive_fractions(
        left_sum as u128,
        u128::from(left_count),
        right_sum as u128,
        u128::from(right_count),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreHistogram {
    /// Sorted `(score, occurrence_count)` pairs.
    pub entries: Vec<(i32, u64)>,
    pub score_sum: i64,
    pub min_score: i32,
    pub max_score: i32,
    pub sample_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptainScoreDistribution {
    pub captain_index: usize,
    pub captain_card_id: u32,
    pub distribution: ScoreHistogram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "liveVariant",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FixedTeamPtScenario {
    Solo {
        event_type: EventType,
        point_bonus_basis_points: u32,
        mission_support_pt_bonus: u64,
    },
    Versus {
        team_rank: u8,
    },
    Festival {
        other_players_score: i64,
        team_rank: u8,
        won: bool,
    },
    ChallengeCp,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CooperativeTeammate {
    pub expected_stat: i32,
    pub leader_score_up: f64,
    pub leader_skill_duration: f64,
}

impl CooperativeTeammate {
    pub(crate) fn skill(self, player_index: usize) -> crate::TeamCardSkill {
        crate::TeamCardSkill {
            card_id: u32::MAX.saturating_sub(player_index as u32),
            duration: self.leader_skill_duration,
            score_up: self.leader_score_up,
            rateup: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CooperativePtScenario {
    pub event_type: EventType,
    pub teammates: [CooperativeTeammate; 4],
    pub leader_selection: CooperativeLeaderSelection,
    pub point_bonus_basis_points: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum CooperativeLeaderSelection {
    #[default]
    MaxStat,
    Specified {
        #[serde(rename = "playerIndex")]
        player_index: u8,
    },
    Random,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TeammateInput<T> {
    Uniform(T),
    Individual([T; 4]),
}

impl<T: Clone> TeammateInput<T> {
    pub fn expand(&self) -> [T; 4] {
        match self {
            Self::Uniform(value) => std::array::from_fn(|_| value.clone()),
            Self::Individual(values) => values.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CooperativeInput {
    pub teammates: TeammateInput<CooperativeTeammate>,
    #[serde(default)]
    pub leader_selection: CooperativeLeaderSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FestivalInput {
    pub teammate_scores: TeammateInput<i32>,
    pub team_rank: u8,
    pub won: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersusInput {
    pub team_rank: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtMaximizeRequest {
    pub event_type: EventType,
    pub live_variant: LiveVariant,
    pub songs: Vec<SongSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_personal_stat: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_support_pt_bonus: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooperative: Option<CooperativeInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versus: Option<VersusInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub festival: Option<FestivalInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtMaximizeScenarioSummary {
    pub includes_fever: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_personal_stat: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_support_pt_bonus: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooperative: Option<CooperativeInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versus: Option<VersusInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub festival: Option<FestivalInput>,
}

impl PtMaximizeRequest {
    pub fn scenario_summary(&self) -> PtMaximizeScenarioSummary {
        PtMaximizeScenarioSummary {
            includes_fever: matches!(
                self.live_variant,
                LiveVariant::Cooperative | LiveVariant::Festival
            ),
            minimum_personal_stat: self.minimum_personal_stat,
            mission_support_pt_bonus: self.mission_support_pt_bonus,
            cooperative: self.cooperative.clone(),
            versus: self.versus,
            festival: self.festival.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PtMaximizeSearchScenario {
    FullTeam { scenario: FixedTeamPtScenario },
    Cooperative { scenario: CooperativePtScenario },
    Medley,
}

impl PtMaximizeSearchScenario {
    pub fn event_type(self) -> EventType {
        match self {
            Self::FullTeam { scenario } => scenario.event_type(),
            Self::Cooperative { scenario } => scenario.event_type,
            Self::Medley => EventType::Medley,
        }
    }

    pub fn live_variant(self) -> LiveVariant {
        match self {
            Self::FullTeam { scenario } => scenario.live_variant(),
            Self::Cooperative { .. } => LiveVariant::Cooperative,
            Self::Medley => LiveVariant::Medley,
        }
    }

    pub(crate) fn with_point_bonus(self, point_bonus_basis_points: u32) -> Self {
        match self {
            Self::FullTeam {
                scenario:
                    FixedTeamPtScenario::Solo {
                        event_type,
                        mission_support_pt_bonus,
                        ..
                    },
            } => Self::FullTeam {
                scenario: FixedTeamPtScenario::Solo {
                    event_type,
                    point_bonus_basis_points,
                    mission_support_pt_bonus,
                },
            },
            Self::Cooperative { mut scenario } => {
                scenario.point_bonus_basis_points = point_bonus_basis_points;
                Self::Cooperative { scenario }
            }
            other => other,
        }
    }
}

impl PtMaximizeRequest {
    pub fn search_scenario(&self) -> Result<PtMaximizeSearchScenario, PtMaximizeError> {
        if self.minimum_personal_stat.is_some_and(|value| value < 0) {
            return Err(PtMaximizeError::InvalidMinimumPersonalStat);
        }
        if !supports_live_variant(self.event_type, self.live_variant) {
            return Err(PtMaximizeError::UnsupportedLiveVariant {
                event_type: self.event_type,
                live_variant: self.live_variant,
            });
        }
        let mission_support_pt_bonus = if self.event_type == EventType::MissionLive
            && self.live_variant == LiveVariant::Solo
        {
            self.mission_support_pt_bonus
                .ok_or(PtMaximizeError::MissingMissionSupportPtBonus)?
        } else {
            0
        };
        Ok(match self.live_variant {
            LiveVariant::Solo => PtMaximizeSearchScenario::FullTeam {
                scenario: FixedTeamPtScenario::Solo {
                    event_type: self.event_type,
                    point_bonus_basis_points: 0,
                    mission_support_pt_bonus,
                },
            },
            LiveVariant::Cooperative => {
                let input =
                    self.cooperative
                        .as_ref()
                        .ok_or(PtMaximizeError::MissingVariantInput {
                            live_variant: self.live_variant,
                        })?;
                for (index, teammate) in input.teammates.expand().into_iter().enumerate() {
                    if teammate.expected_stat < 0
                        || !teammate.leader_score_up.is_finite()
                        || teammate.leader_score_up < 0.0
                        || !teammate.leader_skill_duration.is_finite()
                        || teammate.leader_skill_duration < 0.0
                    {
                        return Err(PtMaximizeError::InvalidCooperativeTeammate { index });
                    }
                }
                if let CooperativeLeaderSelection::Specified { player_index } =
                    input.leader_selection
                {
                    if player_index >= 5 {
                        return Err(PtMaximizeError::InvalidCooperativeLeaderIndex {
                            index: player_index,
                        });
                    }
                }
                PtMaximizeSearchScenario::Cooperative {
                    scenario: CooperativePtScenario {
                        event_type: self.event_type,
                        teammates: input.teammates.expand(),
                        leader_selection: input.leader_selection,
                        point_bonus_basis_points: 0,
                    },
                }
            }
            LiveVariant::Versus => PtMaximizeSearchScenario::FullTeam {
                scenario: FixedTeamPtScenario::Versus {
                    team_rank: validated_team_rank(
                        self.versus
                            .ok_or(PtMaximizeError::MissingVariantInput {
                                live_variant: self.live_variant,
                            })?
                            .team_rank,
                    )?,
                },
            },
            LiveVariant::Festival => {
                let input = self
                    .festival
                    .as_ref()
                    .ok_or(PtMaximizeError::MissingVariantInput {
                        live_variant: self.live_variant,
                    })?;
                PtMaximizeSearchScenario::FullTeam {
                    scenario: FixedTeamPtScenario::Festival {
                        other_players_score: input
                            .teammate_scores
                            .expand()
                            .into_iter()
                            .enumerate()
                            .map(|(index, score)| {
                                if score < 0 {
                                    Err(PtMaximizeError::InvalidFestivalTeammateScore { index })
                                } else {
                                    Ok(i64::from(score))
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()?
                            .into_iter()
                            .sum(),
                        team_rank: validated_team_rank(input.team_rank)?,
                        won: input.won,
                    },
                }
            }
            LiveVariant::ChallengeCp => PtMaximizeSearchScenario::FullTeam {
                scenario: FixedTeamPtScenario::ChallengeCp,
            },
            LiveVariant::Medley => PtMaximizeSearchScenario::Medley,
        })
    }
}

fn validated_team_rank(rank: u8) -> Result<u8, PtMaximizeError> {
    if rank < 5 {
        Ok(rank)
    } else {
        Err(EventPtError::InvalidTeamRank { rank }.into())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtMaximizeTeamResult {
    pub team_card_ids: Vec<u32>,
    pub captain_card_id: u32,
    pub total_stat: i32,
    pub point_bonus_basis_points: u32,
    pub items: SelectedAreaItems,
    pub evaluation: FixedTeamPtEvaluation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtMaximizeResult {
    pub event_id: u32,
    pub event_type: EventType,
    pub live_variant: LiveVariant,
    pub songs: Vec<SongSelection>,
    pub scenario: PtMaximizeScenarioSummary,
    pub metrics: PtMaximizeMetrics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<PtMaximizeTeamResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medley: Option<PtMaximizeMedleyResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtMaximizeMetrics {
    pub core_version: String,
    pub card_count: usize,
    pub song_count: usize,
    pub total_elapsed_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single: Option<PtMaximizeSingleMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medley: Option<PtMaximizeMedleyMetrics>,
}

impl PtMaximizeMetrics {
    pub fn single(card_count: usize, metrics: PtMaximizeSingleMetrics) -> Self {
        Self {
            core_version: env!("BANGDREAM_OPTIMIZE_APP_VERSION").to_owned(),
            card_count,
            song_count: 1,
            total_elapsed_ms: metrics.total_elapsed_ms,
            single: Some(metrics),
            medley: None,
        }
    }

    pub fn medley(card_count: usize, metrics: PtMaximizeMedleyMetrics) -> Self {
        Self {
            core_version: env!("BANGDREAM_OPTIMIZE_APP_VERSION").to_owned(),
            card_count,
            song_count: 3,
            total_elapsed_ms: metrics.total_elapsed_ms,
            single: None,
            medley: Some(metrics),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PtMaximizeSingleMetrics {
    pub item_count: usize,
    pub mode_count: usize,
    pub mode_search_count: u64,
    pub retained_card_count: u64,
    pub planned_team_count: u128,
    pub explored_team_count: u64,
    pub exact_evaluation_count: u64,
    pub candidate_build_ms: f64,
    pub solve_ms: f64,
    pub exact_evaluation_ms: f64,
    pub total_elapsed_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PtMaximizeMedleyMetrics {
    pub item_count: usize,
    pub item_upper_bound_ms: f64,
    pub raw_candidate_count: usize,
    pub retained_candidate_count: usize,
    pub candidate_build_ms: f64,
    pub seed_ms: f64,
    pub solve_ms: f64,
    pub pair_check_count: u64,
    pub third_check_count: u64,
    pub compatible_plan_count: u64,
    pub exact_distribution_count: usize,
    pub total_elapsed_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtMaximizeMedleyTeamResult {
    pub team_card_ids: Vec<u32>,
    pub captain_card_id: u32,
    pub total_stat: i32,
    pub items: SelectedAreaItems,
    pub score_distribution: ScoreHistogram,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtMaximizeMedleyResult {
    pub teams: Vec<PtMaximizeMedleyTeamResult>,
    pub average_pt: AveragePt,
    pub min_pt: u64,
    pub max_pt: u64,
    pub total_score_sum: i128,
    pub sample_count: u64,
}

impl FixedTeamPtScenario {
    pub fn event_type(self) -> EventType {
        match self {
            Self::Solo { event_type, .. } => event_type,
            Self::Versus { .. } => EventType::Versus,
            Self::Festival { .. } => EventType::Festival,
            Self::ChallengeCp => EventType::Challenge,
        }
    }

    pub fn live_variant(self) -> LiveVariant {
        match self {
            Self::Solo { .. } => LiveVariant::Solo,
            Self::Versus { .. } => LiveVariant::Versus,
            Self::Festival { .. } => LiveVariant::Festival,
            Self::ChallengeCp => LiveVariant::ChallengeCp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedTeamPtEvaluation {
    pub event_type: EventType,
    pub live_variant: LiveVariant,
    pub captain_index: usize,
    pub captain_card_id: u32,
    pub score_distribution: ScoreHistogram,
    pub average_pt: AveragePt,
    pub min_pt: u64,
    pub max_pt: u64,
    pub average_cp_gain: Option<AveragePt>,
    pub challenge_cp_cost: Option<u32>,
}

#[derive(Debug, Error)]
pub enum PtMaximizeError {
    #[error(transparent)]
    Chart(#[from] ChartError),

    #[error(transparent)]
    EventPt(#[from] EventPtError),

    #[error(transparent)]
    Model(#[from] DpModelError),

    #[error("event type {event_type:?} does not support live variant {live_variant:?}")]
    UnsupportedLiveVariant {
        event_type: EventType,
        live_variant: LiveVariant,
    },

    #[error("score distribution is empty")]
    EmptyDistribution,

    #[error("cooperative calculation is not supported for event type {event_type:?}")]
    UnsupportedCooperativeEvent { event_type: EventType },

    #[error("at least five distinct characters are required")]
    NotEnoughDistinctCharacters,

    #[error("no valid PT-maximizing team was found")]
    NoResult,

    #[error("missionSupportPtBonus must be provided for mission_live solo")]
    MissingMissionSupportPtBonus,

    #[error("minimumPersonalStat must be non-negative")]
    InvalidMinimumPersonalStat,

    #[error("cooperative teammate {index} has invalid stat or leader skill parameters")]
    InvalidCooperativeTeammate { index: usize },

    #[error("cooperative leader player index must be between 0 and 4, got {index}")]
    InvalidCooperativeLeaderIndex { index: u8 },

    #[error("festival teammate {index} has a negative expected score")]
    InvalidFestivalTeammateScore { index: usize },

    #[error("input for live variant {live_variant:?} is missing")]
    MissingVariantInput { live_variant: LiveVariant },

    #[error("single-song variants require exactly one song, got {count}")]
    InvalidSingleSongCount { count: usize },

    #[error("Medley PT-maximize requires exactly three songs, got {count}")]
    InvalidMedleySongCount { count: usize },

    #[error("Medley candidate generation failed: {0}")]
    MedleyCandidate(String),

    #[error("Medley solver failed: {0}")]
    MedleySolver(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_matrix_matches_the_design() {
        assert!(supports_live_variant(
            EventType::MissionLive,
            LiveVariant::Cooperative
        ));
        assert!(supports_live_variant(
            EventType::Challenge,
            LiveVariant::ChallengeCp
        ));
        assert!(supports_live_variant(
            EventType::Medley,
            LiveVariant::Medley
        ));
        assert!(!supports_live_variant(EventType::Medley, LiveVariant::Solo));
        assert!(!supports_live_variant(
            EventType::Festival,
            LiveVariant::Cooperative
        ));
    }

    #[test]
    fn average_pt_compares_exact_rationals() {
        let left = AveragePt::new(2, 3).unwrap();
        let right = AveragePt::new(3, 5).unwrap();
        assert!(left > right);
        assert_eq!(left, AveragePt::new(2, 3).unwrap());

        let near_limit = AveragePt::new(u128::MAX - 1, u64::MAX).unwrap();
        let limit = AveragePt::new(u128::MAX, u64::MAX).unwrap();
        assert!(near_limit < limit);
    }

    #[test]
    fn teammate_input_accepts_uniform_and_individual_json() {
        let uniform: TeammateInput<i32> = serde_json::from_str("123").unwrap();
        assert_eq!(uniform.expand(), [123; 4]);
        let individual: TeammateInput<i32> = serde_json::from_str("[1,2,3,4]").unwrap();
        assert_eq!(individual.expand(), [1, 2, 3, 4]);
    }

    #[test]
    fn cooperative_leader_selection_uses_tagged_camel_case_json() {
        let specified: CooperativeLeaderSelection =
            serde_json::from_str(r#"{"mode":"specified","playerIndex":4}"#).unwrap();
        assert_eq!(
            specified,
            CooperativeLeaderSelection::Specified { player_index: 4 }
        );
        assert_eq!(
            serde_json::to_string(&CooperativeLeaderSelection::MaxStat).unwrap(),
            r#"{"mode":"max_stat"}"#
        );
    }

    #[test]
    fn request_rejects_invalid_rank_before_team_search() {
        let request = PtMaximizeRequest {
            event_type: EventType::Versus,
            live_variant: LiveVariant::Versus,
            songs: vec![SongSelection {
                song_id: 1,
                difficulty: 3,
            }],
            minimum_personal_stat: None,
            mission_support_pt_bonus: None,
            cooperative: None,
            versus: Some(VersusInput { team_rank: 5 }),
            festival: None,
        };
        assert!(matches!(
            request.search_scenario(),
            Err(PtMaximizeError::EventPt(EventPtError::InvalidTeamRank {
                rank: 5
            }))
        ));
    }

    #[test]
    fn mission_live_support_bonus_is_required_only_for_solo() {
        let solo = PtMaximizeRequest {
            event_type: EventType::MissionLive,
            live_variant: LiveVariant::Solo,
            songs: vec![],
            minimum_personal_stat: None,
            mission_support_pt_bonus: None,
            cooperative: None,
            versus: None,
            festival: None,
        };
        assert!(matches!(
            solo.search_scenario(),
            Err(PtMaximizeError::MissingMissionSupportPtBonus)
        ));

        let cooperative = PtMaximizeRequest {
            event_type: EventType::MissionLive,
            live_variant: LiveVariant::Cooperative,
            songs: vec![],
            minimum_personal_stat: Some(0),
            mission_support_pt_bonus: Some(999),
            cooperative: Some(CooperativeInput {
                teammates: TeammateInput::Uniform(CooperativeTeammate {
                    expected_stat: 0,
                    leader_score_up: 0.0,
                    leader_skill_duration: 0.0,
                }),
                leader_selection: CooperativeLeaderSelection::MaxStat,
            }),
            versus: None,
            festival: None,
        };
        assert!(matches!(
            cooperative.search_scenario().unwrap(),
            PtMaximizeSearchScenario::Cooperative {
                scenario: CooperativePtScenario {
                    event_type: EventType::MissionLive,
                    point_bonus_basis_points: 0,
                    ..
                }
            }
        ));
    }
}
