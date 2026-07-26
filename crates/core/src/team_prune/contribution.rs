use super::hard::{
    best_any_team_score_upper_bound, medley_card_dominates_for_signature, MedleyCardPruneProfile,
    MedleyPruneContext,
};
use super::signature::{signature_label, MedleyPruneSignature};
use crate::model::chart::Chart;
use crate::model::preparation::PreparedCard;
use bangdream_optimize_team_prune::{
    dominance_graph_for_index_subset_with_base, dominator_cover_after_worst_teammate_groups,
    dominator_cover_summary_after_worst_teammate_groups, DominanceGraph, DominatorCoverSummary,
};
use std::collections::BTreeMap;

const TEAM_SIZE: usize = 5;
const MEDLEY_TEAM_COUNT: usize = 3;
// Every physical role is either one of the five normal positions with another card as captain,
// or one of the five normal positions with this card activating again as captain. There is no
// captain-only role: the captain is always one of the five normally activated cards.
const CAPTAIN_SCENARIO_START: usize = TEAM_SIZE;
const CONTRIBUTION_SCENARIO_COUNT: usize = TEAM_SIZE * 2;
const SIGNATURE_BREAK_STATE_COUNT: usize = 4;
const SCORE_CONTRIBUTION_EPS: f64 = 1e-7;
const CONTRIBUTION_DIAGNOSTIC_TARGETS: usize = 3;
const CONTRIBUTION_DIAGNOSTIC_NEAR_MISSES: usize = 4;

pub(crate) fn contribution_dominance_graph_for_signature(
    cards: &[PreparedCard],
    signature: MedleyPruneSignature,
    hard_dominance_graph: &DominanceGraph,
    contribution_dominance: &mut MedleyContributionDominance<'_>,
) -> DominanceGraph {
    let models = contribution_dominance.models_for_signature(signature);
    dominance_graph_for_index_subset_with_base(
        cards.len(),
        &models.allowed_indices,
        hard_dominance_graph,
        |dominator_idx, target_idx| {
            contribution_dominance.card_can_replace_with_models(dominator_idx, target_idx, &models)
        },
    )
}

pub(crate) fn same_shape_contribution_active_indices(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    required_cover: usize,
    replacement_values: Option<&[u64]>,
) -> Vec<usize> {
    let mut dominance = MedleyContributionDominance::new(cards, charts, profiles, 0);
    let allowed = cards
        .iter()
        .enumerate()
        .filter_map(|(idx, card)| signature.allows(card).then_some(idx))
        .collect::<Vec<_>>();
    let mut shape_buckets: BTreeMap<(u32, u64, bool), Vec<usize>> = BTreeMap::new();
    for &idx in &allowed {
        let card = &cards[idx];
        shape_buckets
            .entry((
                card.character_id,
                card.skill.duration.to_bits(),
                card.skill.rateup,
            ))
            .or_default()
            .push(idx);
    }

    allowed
        .iter()
        .copied()
        .filter(|&target_idx| {
            let target = &cards[target_idx];
            shape_buckets[&(
                target.character_id,
                target.skill.duration.to_bits(),
                target.skill.rateup,
            )]
                .iter()
                .copied()
                .filter(|&dominator_idx| {
                    if dominator_idx == target_idx {
                        return false;
                    }
                    if replacement_values
                        .is_some_and(|values| values[dominator_idx] < values[target_idx])
                    {
                        return false;
                    }
                    dominance.card_can_replace_for_signature(dominator_idx, target_idx, signature)
                })
                .take(required_cover)
                .count()
                < required_cover
        })
        .collect()
}

pub(crate) fn same_character_score_contribution_cover(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    context: &MedleyPruneContext,
    contribution_dominance: &mut MedleyContributionDominance<'_>,
) -> usize {
    context
        .obligations
        .get(idx)
        .into_iter()
        .flatten()
        .map(|&signature| {
            same_character_score_contribution_cover_for_signature(
                idx,
                card,
                cards,
                signature,
                contribution_dominance,
            )
        })
        .min()
        .unwrap_or_default()
}

pub(crate) fn full_medley_score_contribution_cover(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    context: &MedleyPruneContext,
    contribution_dominance: &mut MedleyContributionDominance<'_>,
) -> usize {
    context
        .obligations
        .get(idx)
        .into_iter()
        .flatten()
        .map(|&signature| {
            full_medley_score_contribution_cover_for_signature(
                idx,
                card,
                cards,
                signature,
                contribution_dominance,
            )
        })
        .min()
        .unwrap_or_default()
}

pub(crate) fn trace_score_contribution_cover_diagnostics(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    current_best: i32,
    signature: MedleyPruneSignature,
    active_card_indices: &[usize],
) {
    let mut contribution_dominance =
        MedleyContributionDominance::new(cards, charts, profiles, current_best);
    let mut diagnostics = Vec::new();

    for &target_idx in active_card_indices {
        let mut counts_by_character = BTreeMap::new();
        let mut near_misses = Vec::new();

        for (candidate_idx, candidate) in cards.iter().enumerate() {
            if candidate_idx == target_idx || !signature.allows(candidate) {
                continue;
            }

            let comparison = contribution_dominance.replacement_comparison_for_signature(
                candidate_idx,
                target_idx,
                signature,
            );
            if comparison.replaces {
                *counts_by_character
                    .entry(candidate.character_id)
                    .or_default() += 1;
                continue;
            }

            near_misses.push(ContributionNearMiss {
                candidate_idx,
                new_cover: DominatorCoverSummary::default(),
                comparison,
                reverse_comparison: contribution_dominance.replacement_comparison_for_signature(
                    target_idx,
                    candidate_idx,
                    signature,
                ),
            });
        }

        let target = &cards[target_idx];
        let cover = dominator_cover_summary_after_worst_teammate_groups(
            &counts_by_character,
            target.character_id,
            TEAM_SIZE,
            MEDLEY_TEAM_COUNT,
        );
        for near_miss in &mut near_misses {
            near_miss.new_cover = dominator_cover_summary_with_extra_character(
                &counts_by_character,
                target.character_id,
                cards[near_miss.candidate_idx].character_id,
            );
        }
        let mut character_buckets: Vec<_> = counts_by_character.into_iter().collect();
        character_buckets.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
        near_misses.sort_by(|left, right| {
            right
                .new_cover
                .free_replacements
                .cmp(&left.new_cover.free_replacements)
                .then_with(|| right.new_cover.dominators.cmp(&left.new_cover.dominators))
                .then_with(|| {
                    right
                        .reverse_comparison
                        .replaces
                        .cmp(&left.reverse_comparison.replaces)
                })
                .then_with(|| right.comparison.margin.total_cmp(&left.comparison.margin))
                .then_with(|| {
                    cards[left.candidate_idx]
                        .card_id
                        .cmp(&cards[right.candidate_idx].card_id)
                })
        });

        diagnostics.push(ContributionCoverDiagnostic {
            target_idx,
            cover,
            character_buckets,
            near_misses,
        });
    }

    diagnostics.sort_by(|left, right| {
        right
            .cover
            .free_replacements
            .cmp(&left.cover.free_replacements)
            .then_with(|| right.cover.dominators.cmp(&left.cover.dominators))
            .then_with(|| {
                cards[left.target_idx]
                    .card_id
                    .cmp(&cards[right.target_idx].card_id)
            })
    });

    for diagnostic in diagnostics
        .into_iter()
        .take(CONTRIBUTION_DIAGNOSTIC_TARGETS)
    {
        let target = &cards[diagnostic.target_idx];
        eprintln!(
            "medley contribution cover detail: signature={} target_card={} target_character={} free_replacements={} dominators={} blocked_teammates={} other_team_capacity={} top_buckets={}",
            signature_label(signature),
            target.card_id,
            target.character_id,
            diagnostic.cover.free_replacements,
            diagnostic.cover.dominators,
            diagnostic.cover.blocked_teammates,
            diagnostic.cover.other_team_capacity,
            format_character_buckets(&diagnostic.character_buckets),
        );
        for near_miss in diagnostic
            .near_misses
            .into_iter()
            .take(CONTRIBUTION_DIAGNOSTIC_NEAR_MISSES)
        {
            let candidate = &cards[near_miss.candidate_idx];
            eprintln!(
                "medley contribution near miss: target_card={} candidate_card={} candidate_character={} new_free_replacements={} new_dominators={} new_blocked_teammates={} new_other_team_capacity={} reason={} margin={:.3} failed_chart={} failed_skill_position={} left_value={:.3} right_value={:.3} reverse_replaces={} reverse_reason={} reverse_margin={:.3}",
                target.card_id,
                candidate.card_id,
                candidate.character_id,
                near_miss.new_cover.free_replacements,
                near_miss.new_cover.dominators,
                near_miss.new_cover.blocked_teammates,
                near_miss.new_cover.other_team_capacity,
                near_miss.comparison.reason.label(),
                near_miss.comparison.margin,
                near_miss
                    .comparison
                    .failed_chart
                    .map(|idx| idx.to_string())
                    .unwrap_or_else(|| "-".to_owned()),
                near_miss
                    .comparison
                    .failed_skill_position
                    .map(skill_position_label)
                    .unwrap_or_else(|| "-".to_owned()),
                near_miss.comparison.left_value,
                near_miss.comparison.right_value,
                near_miss.reverse_comparison.replaces,
                near_miss.reverse_comparison.reason.label(),
                near_miss.reverse_comparison.margin,
            );
            if let Some(endpoint) = near_miss.comparison.endpoint {
                eprintln!(
                    "medley contribution affine detail: target_card={} candidate_card={} chart={} skill_position={} endpoint_x={:.6} endpoint_y={:.3} x_range=[{:.6},{:.6}] y_range=[{:.3},{:.3}] left_fn={:.3}*x+{:.6}*y+{:.3} right_fn={:.3}*x+{:.6}*y+{:.3}",
                    target.card_id,
                    candidate.card_id,
                    near_miss
                        .comparison
                        .failed_chart
                        .map(|idx| idx.to_string())
                        .unwrap_or_else(|| "-".to_owned()),
                    near_miss
                        .comparison
                        .failed_skill_position
                        .map(skill_position_label)
                        .unwrap_or_else(|| "-".to_owned()),
                    endpoint.x,
                    endpoint.y,
                    endpoint.x_low,
                    endpoint.x_high,
                    endpoint.y_low,
                    endpoint.y_high,
                    endpoint.left_a,
                    endpoint.left_b,
                    endpoint.left_c,
                    endpoint.right_a,
                    endpoint.right_b,
                    endpoint.right_c,
                );
            }
            if let Some(endpoint) = near_miss.reverse_comparison.endpoint {
                eprintln!(
                    "medley contribution reverse affine detail: target_card={} candidate_card={} chart={} skill_position={} endpoint_x={:.6} endpoint_y={:.3} x_range=[{:.6},{:.6}] y_range=[{:.3},{:.3}] target_fn={:.3}*x+{:.6}*y+{:.3} candidate_fn={:.3}*x+{:.6}*y+{:.3}",
                    target.card_id,
                    candidate.card_id,
                    near_miss
                        .reverse_comparison
                        .failed_chart
                        .map(|idx| idx.to_string())
                        .unwrap_or_else(|| "-".to_owned()),
                    near_miss
                        .reverse_comparison
                        .failed_skill_position
                        .map(skill_position_label)
                        .unwrap_or_else(|| "-".to_owned()),
                    endpoint.x,
                    endpoint.y,
                    endpoint.x_low,
                    endpoint.x_high,
                    endpoint.y_low,
                    endpoint.y_high,
                    endpoint.left_a,
                    endpoint.left_b,
                    endpoint.left_c,
                    endpoint.right_a,
                    endpoint.right_b,
                    endpoint.right_c,
                );
            }
        }
    }
}

fn same_character_score_contribution_cover_for_signature(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    signature: MedleyPruneSignature,
    contribution_dominance: &mut MedleyContributionDominance<'_>,
) -> usize {
    cards
        .iter()
        .enumerate()
        .filter(|&(other_idx, other)| {
            other_idx != idx
                && other.character_id == card.character_id
                && contribution_dominance.card_can_replace_for_signature(other_idx, idx, signature)
        })
        .take(MEDLEY_TEAM_COUNT)
        .count()
}

fn full_medley_score_contribution_cover_for_signature(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    signature: MedleyPruneSignature,
    contribution_dominance: &mut MedleyContributionDominance<'_>,
) -> usize {
    let mut counts_by_character: BTreeMap<u32, usize> = BTreeMap::new();

    for (other_idx, other) in cards.iter().enumerate() {
        if other_idx == idx
            || !contribution_dominance.card_can_replace_for_signature(other_idx, idx, signature)
        {
            continue;
        }

        *counts_by_character.entry(other.character_id).or_default() += 1;
    }

    dominator_cover_after_worst_teammate_groups(
        counts_by_character,
        card.character_id,
        TEAM_SIZE,
        MEDLEY_TEAM_COUNT,
    )
}

fn dominator_cover_summary_with_extra_character(
    counts_by_character: &BTreeMap<u32, usize>,
    target_character_id: u32,
    character_id: u32,
) -> DominatorCoverSummary {
    let mut counts = counts_by_character.clone();
    *counts.entry(character_id).or_default() += 1;
    dominator_cover_summary_after_worst_teammate_groups(
        &counts,
        target_character_id,
        TEAM_SIZE,
        MEDLEY_TEAM_COUNT,
    )
}

fn format_character_buckets(character_buckets: &[(u32, usize)]) -> String {
    character_buckets
        .iter()
        .take(8)
        .map(|(character_id, count)| format!("{character_id}:{count}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn skill_position_label(position: usize) -> String {
    if position < CAPTAIN_SCENARIO_START {
        position.to_string()
    } else {
        format!("{}+captain", position - CAPTAIN_SCENARIO_START)
    }
}

#[derive(Debug)]
struct ContributionCoverDiagnostic {
    target_idx: usize,
    cover: DominatorCoverSummary,
    character_buckets: Vec<(u32, usize)>,
    near_misses: Vec<ContributionNearMiss>,
}

#[derive(Debug)]
struct ContributionNearMiss {
    candidate_idx: usize,
    new_cover: DominatorCoverSummary,
    comparison: ContributionReplacementComparison,
    reverse_comparison: ContributionReplacementComparison,
}

pub(crate) struct MedleyContributionDominance<'a> {
    cards: &'a [PreparedCard],
    charts: &'a [Chart],
    profiles: &'a [MedleyCardPruneProfile],
    seed_score_floor_by_chart: Vec<f64>,
    signature_context_cache: Vec<SignatureContributionContextCacheEntry>,
}

#[derive(Debug, Clone)]
struct SignatureContributionContextCacheEntry {
    signature: MedleyPruneSignature,
    bounds: Option<SignatureContributionContextBounds>,
}

#[derive(Debug, Clone)]
struct SignatureContributionContextBounds {
    stat_high: f64,
    normal_meta_low_by_chart: Vec<f64>,
    normal_meta_high_by_chart: Vec<f64>,
    normal_plus_captain_meta_low_by_chart: Vec<f64>,
    normal_plus_captain_meta_high_by_chart: Vec<f64>,
    teammate_stat_low_by_chart_scenario: Vec<f64>,
}

#[derive(Debug, Clone)]
struct ForcedTeammateBounds {
    stat_low: f64,
    normal_meta_high_by_chart: Vec<f64>,
    normal_plus_captain_meta_high_by_chart: Vec<f64>,
}

#[derive(Debug, Clone)]
struct ForcedCharacterOptions {
    character_id: u32,
    stat_by_break: [f64; SIGNATURE_BREAK_STATE_COUNT],
    normal_meta_by_chart_break: Vec<[f64; SIGNATURE_BREAK_STATE_COUNT]>,
    captain_card_meta_by_chart_break: Vec<[f64; SIGNATURE_BREAK_STATE_COUNT]>,
}

#[derive(Debug, Clone)]
struct ForcedTeammateDp {
    stat: [[f64; SIGNATURE_BREAK_STATE_COUNT]; TEAM_SIZE],
    normal_meta_by_chart: Vec<[[f64; SIGNATURE_BREAK_STATE_COUNT]; TEAM_SIZE]>,
    normal_plus_captain_meta_by_chart: Vec<[[[f64; 2]; SIGNATURE_BREAK_STATE_COUNT]; TEAM_SIZE]>,
}

#[derive(Debug, Clone, Copy)]
struct ValueRange {
    low: f64,
    high: f64,
}

#[derive(Debug, Clone, Copy)]
struct CaptainMetaCandidate {
    character_id: u32,
    normal_meta: ValueRange,
    captain_meta: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ContributionAffineModel {
    stat: f64,
    meta: f64,
    constant: f64,
    x_low: f64,
    x_high: f64,
    y_low: f64,
    y_high: f64,
}

impl ContributionAffineModel {
    fn value_at(self, x: f64, y: f64) -> f64 {
        self.stat * x + self.meta * y + self.constant
    }
}

#[derive(Debug)]
struct SignatureContributionModels {
    allowed_indices: Vec<usize>,
    complete_by_card: Vec<bool>,
    models_by_card_skill_position: Vec<ContributionAffineModel>,
    models_per_card: usize,
}

impl SignatureContributionModels {
    fn models_for_card(&self, card_idx: usize) -> Option<&[ContributionAffineModel]> {
        if !self
            .complete_by_card
            .get(card_idx)
            .copied()
            .unwrap_or(false)
        {
            return None;
        }

        let start = card_idx.checked_mul(self.models_per_card)?;
        self.models_by_card_skill_position
            .get(start..start + self.models_per_card)
    }
}

#[derive(Debug, Clone, Copy)]
struct ContributionEndpointComparison {
    margin: f64,
    left_value: f64,
    right_value: f64,
    x: f64,
    y: f64,
    x_low: f64,
    x_high: f64,
    y_low: f64,
    y_high: f64,
    left_a: f64,
    left_b: f64,
    left_c: f64,
    right_a: f64,
    right_b: f64,
    right_c: f64,
}

#[derive(Debug, Clone, Copy)]
struct ContributionReplacementComparison {
    replaces: bool,
    reason: ContributionReplacementReason,
    margin: f64,
    failed_chart: Option<usize>,
    failed_skill_position: Option<usize>,
    left_value: f64,
    right_value: f64,
    endpoint: Option<ContributionEndpointComparison>,
}

#[derive(Debug, Clone, Copy)]
enum ContributionReplacementReason {
    SameCard,
    Signature,
    StrictDominance,
    Score,
    MissingBounds,
    Tie,
}

impl ContributionReplacementReason {
    fn label(self) -> &'static str {
        match self {
            Self::SameCard => "sameCard",
            Self::Signature => "signature",
            Self::StrictDominance => "strictDominance",
            Self::Score => "score",
            Self::MissingBounds => "missingBounds",
            Self::Tie => "tie",
        }
    }
}

fn contribution_replacement_reject(
    reason: ContributionReplacementReason,
    margin: f64,
    failed_chart: Option<usize>,
    failed_skill_position: Option<usize>,
    left_value: f64,
    right_value: f64,
) -> ContributionReplacementComparison {
    ContributionReplacementComparison {
        replaces: false,
        reason,
        margin,
        failed_chart,
        failed_skill_position,
        left_value,
        right_value,
        endpoint: None,
    }
}

impl<'a> MedleyContributionDominance<'a> {
    pub(crate) fn new(
        cards: &'a [PreparedCard],
        charts: &'a [Chart],
        profiles: &'a [MedleyCardPruneProfile],
        current_best: i32,
    ) -> Self {
        Self {
            cards,
            charts,
            profiles,
            seed_score_floor_by_chart: seed_score_floor_by_chart(
                cards,
                charts,
                profiles,
                current_best,
            ),
            signature_context_cache: Vec::new(),
        }
    }

    pub(crate) fn with_best_any_team_scores(
        cards: &'a [PreparedCard],
        charts: &'a [Chart],
        profiles: &'a [MedleyCardPruneProfile],
        current_best: i32,
        best_any_team_scores: &[f64],
    ) -> Self {
        debug_assert_eq!(charts.len(), best_any_team_scores.len());
        Self {
            cards,
            charts,
            profiles,
            seed_score_floor_by_chart: seed_score_floor_by_chart_from_upper_bounds(
                charts.len(),
                current_best,
                best_any_team_scores,
            ),
            signature_context_cache: Vec::new(),
        }
    }

    fn card_can_replace_for_signature(
        &mut self,
        left_idx: usize,
        right_idx: usize,
        signature: MedleyPruneSignature,
    ) -> bool {
        self.replacement_comparison_for_signature(left_idx, right_idx, signature)
            .replaces
    }

    fn replacement_comparison_for_signature(
        &mut self,
        left_idx: usize,
        right_idx: usize,
        signature: MedleyPruneSignature,
    ) -> ContributionReplacementComparison {
        if left_idx == right_idx {
            return contribution_replacement_reject(
                ContributionReplacementReason::SameCard,
                f64::NEG_INFINITY,
                None,
                None,
                0.0,
                0.0,
            );
        }

        {
            let left = &self.cards[left_idx];
            let right = &self.cards[right_idx];
            if !signature.allows(left) || !signature.allows(right) {
                return contribution_replacement_reject(
                    ContributionReplacementReason::Signature,
                    f64::NEG_INFINITY,
                    None,
                    None,
                    0.0,
                    0.0,
                );
            }
            if medley_card_dominates_for_signature(
                left,
                &self.profiles[left_idx],
                right,
                &self.profiles[right_idx],
                signature,
            ) {
                return ContributionReplacementComparison {
                    replaces: true,
                    reason: ContributionReplacementReason::StrictDominance,
                    margin: f64::INFINITY,
                    failed_chart: None,
                    failed_skill_position: None,
                    left_value: 0.0,
                    right_value: 0.0,
                    endpoint: None,
                };
            }
        }

        let left_card_id = self.cards[left_idx].card_id;
        let right_card_id = self.cards[right_idx].card_id;
        let mut strictly_better = false;
        let mut min_margin = f64::INFINITY;
        let mut min_margin_chart = None;
        let mut min_margin_skill_position = None;
        let mut min_margin_left_value = 0.0;
        let mut min_margin_right_value = 0.0;
        let mut min_margin_endpoint = None;
        let Some(context) = self.signature_context_bounds(signature) else {
            return contribution_replacement_reject(
                ContributionReplacementReason::MissingBounds,
                f64::NEG_INFINITY,
                None,
                None,
                0.0,
                0.0,
            );
        };

        for chart_idx in 0..self.charts.len() {
            for skill_position in 0..CONTRIBUTION_SCENARIO_COUNT {
                let Some(left_model) = signature_card_affine_model_for_chart_skill_position(
                    &self.cards[left_idx],
                    &self.profiles[left_idx],
                    signature,
                    chart_idx,
                    skill_position,
                    &context,
                    &self.charts[chart_idx],
                ) else {
                    return contribution_replacement_reject(
                        ContributionReplacementReason::MissingBounds,
                        f64::NEG_INFINITY,
                        Some(chart_idx),
                        Some(skill_position),
                        0.0,
                        0.0,
                    );
                };
                let Some(right_model) = signature_card_affine_model_for_chart_skill_position(
                    &self.cards[right_idx],
                    &self.profiles[right_idx],
                    signature,
                    chart_idx,
                    skill_position,
                    &context,
                    &self.charts[chart_idx],
                ) else {
                    return contribution_replacement_reject(
                        ContributionReplacementReason::MissingBounds,
                        f64::NEG_INFINITY,
                        Some(chart_idx),
                        Some(skill_position),
                        0.0,
                        0.0,
                    );
                };

                let endpoint = affine_contribution_endpoint_comparison(left_model, right_model);
                let margin = endpoint.margin;
                if margin < min_margin {
                    min_margin = margin;
                    min_margin_chart = Some(chart_idx);
                    min_margin_skill_position = Some(skill_position);
                    min_margin_left_value = endpoint.left_value;
                    min_margin_right_value = endpoint.right_value;
                    min_margin_endpoint = Some(endpoint);
                }
                if margin < 0.0 {
                    return ContributionReplacementComparison {
                        replaces: false,
                        reason: ContributionReplacementReason::Score,
                        margin,
                        failed_chart: Some(chart_idx),
                        failed_skill_position: Some(skill_position),
                        left_value: endpoint.left_value,
                        right_value: endpoint.right_value,
                        endpoint: Some(endpoint),
                    };
                }
                strictly_better |= margin > SCORE_CONTRIBUTION_EPS;
            }
        }

        if strictly_better || left_card_id < right_card_id {
            ContributionReplacementComparison {
                replaces: true,
                reason: ContributionReplacementReason::Score,
                margin: min_margin,
                failed_chart: min_margin_chart,
                failed_skill_position: min_margin_skill_position,
                left_value: min_margin_left_value,
                right_value: min_margin_right_value,
                endpoint: min_margin_endpoint,
            }
        } else {
            ContributionReplacementComparison {
                replaces: false,
                reason: ContributionReplacementReason::Tie,
                margin: min_margin,
                failed_chart: min_margin_chart,
                failed_skill_position: min_margin_skill_position,
                left_value: min_margin_left_value,
                right_value: min_margin_right_value,
                endpoint: min_margin_endpoint,
            }
        }
    }

    fn models_for_signature(
        &mut self,
        signature: MedleyPruneSignature,
    ) -> SignatureContributionModels {
        let context = self.signature_context_bounds(signature);
        let models_per_card = self.charts.len() * CONTRIBUTION_SCENARIO_COUNT;
        let mut allowed_indices = Vec::new();
        let mut complete_by_card = vec![false; self.cards.len()];
        let mut models_by_card_skill_position =
            vec![ContributionAffineModel::default(); self.cards.len() * models_per_card];

        for (idx, card) in self.cards.iter().enumerate() {
            if !signature.allows(card) {
                continue;
            }
            allowed_indices.push(idx);
            let Some(context) = context.as_ref() else {
                continue;
            };

            let mut complete = true;
            for chart_idx in 0..self.charts.len() {
                for skill_position in 0..CONTRIBUTION_SCENARIO_COUNT {
                    let Some(model) = signature_card_affine_model_for_chart_skill_position(
                        card,
                        &self.profiles[idx],
                        signature,
                        chart_idx,
                        skill_position,
                        context,
                        &self.charts[chart_idx],
                    ) else {
                        complete = false;
                        break;
                    };
                    let offset = idx * models_per_card
                        + chart_idx * CONTRIBUTION_SCENARIO_COUNT
                        + skill_position;
                    models_by_card_skill_position[offset] = model;
                }
                if !complete {
                    break;
                }
            }

            if complete {
                complete_by_card[idx] = true;
            }
        }

        SignatureContributionModels {
            allowed_indices,
            complete_by_card,
            models_by_card_skill_position,
            models_per_card,
        }
    }

    fn card_can_replace_with_models(
        &self,
        left_idx: usize,
        right_idx: usize,
        models: &SignatureContributionModels,
    ) -> bool {
        if left_idx == right_idx {
            return false;
        }

        let left = &self.cards[left_idx];
        let right = &self.cards[right_idx];

        let Some(left_models) = models.models_for_card(left_idx) else {
            return false;
        };
        let Some(right_models) = models.models_for_card(right_idx) else {
            return false;
        };

        let mut strictly_better = false;
        for (&left_model, &right_model) in left_models.iter().zip(right_models) {
            let margin = affine_contribution_min_margin(left_model, right_model);
            if margin < 0.0 {
                return false;
            }
            strictly_better |= margin > SCORE_CONTRIBUTION_EPS;
        }

        strictly_better || left.card_id < right.card_id
    }

    fn signature_context_bounds(
        &mut self,
        signature: MedleyPruneSignature,
    ) -> Option<SignatureContributionContextBounds> {
        if let Some(entry) = self
            .signature_context_cache
            .iter()
            .find(|entry| entry.signature == signature)
        {
            return entry.bounds.clone();
        }

        let bounds = self.build_signature_context_bounds(signature);
        self.signature_context_cache
            .push(SignatureContributionContextCacheEntry {
                signature,
                bounds: bounds.clone(),
            });
        bounds
    }

    fn build_signature_context_bounds(
        &self,
        signature: MedleyPruneSignature,
    ) -> Option<SignatureContributionContextBounds> {
        let stat_ranges = character_value_ranges(self.cards, |idx, card| {
            signature.allows(card).then_some(ValueRange {
                low: self.profiles[idx].stat,
                high: self.profiles[idx].stat,
            })
        });
        let coarse_stat_low =
            bottom_value_range_sum::<{ TEAM_SIZE - 1 }, _>(&stat_ranges, |range| range.low)?;
        let stat_high =
            top_value_range_sum::<{ TEAM_SIZE - 1 }, _>(&stat_ranges, |range| range.high)?;
        let max_card_stat = stat_ranges
            .iter()
            .map(|range| range.high)
            .fold(f64::NEG_INFINITY, f64::max);
        if !max_card_stat.is_finite() {
            return None;
        }

        let mut normal_meta_low_by_chart = Vec::with_capacity(self.charts.len());
        let mut normal_meta_high_by_chart = Vec::with_capacity(self.charts.len());
        let mut normal_plus_captain_meta_low_by_chart = Vec::with_capacity(self.charts.len());
        let mut normal_plus_captain_meta_high_by_chart = Vec::with_capacity(self.charts.len());
        for chart_idx in 0..self.charts.len() {
            let normal_ranges = character_value_ranges(self.cards, |idx, card| {
                signature
                    .allows(card)
                    .then(|| {
                        signature_card_normal_meta_bounds_for_chart(
                            card,
                            &self.profiles[idx],
                            signature,
                            chart_idx,
                        )
                    })
                    .flatten()
            });
            let normal_low =
                bottom_value_range_sum::<{ TEAM_SIZE - 1 }, _>(&normal_ranges, |range| range.low)?;
            let normal_high =
                top_value_range_sum::<{ TEAM_SIZE - 1 }, _>(&normal_ranges, |range| range.high)?;
            let normal_plus_captain = signature_normal_plus_captain_meta_bounds(
                self.cards,
                self.profiles,
                signature,
                chart_idx,
                None,
                TEAM_SIZE - 1,
            )?;
            normal_meta_low_by_chart.push(normal_low);
            normal_meta_high_by_chart.push(normal_high);
            normal_plus_captain_meta_low_by_chart.push(normal_plus_captain.low);
            normal_plus_captain_meta_high_by_chart.push(normal_plus_captain.high);
        }

        // All cards must be compared on one shared rectangle for ordinary graph transitivity to
        // remain valid. Tighten that rectangle by first deriving a card-forced lower bound and
        // then taking the minimum over every card that can occur in this exact signature.
        let need_forced_meta = self
            .seed_score_floor_by_chart
            .iter()
            .any(|&score| score > 0.0);
        // Mixed is the large shared pool and the only signature where the extra fixed-card DP has
        // paid for itself in fixture candidate counts. Keep the previous coarse score floor for
        // the smaller unified pools to avoid rebuilding category envelopes with no solver benefit.
        let use_forced_envelope =
            need_forced_meta && matches!(signature, MedleyPruneSignature::Mixed);
        let mut teammate_stat_low_by_chart_scenario = vec![
            if use_forced_envelope {
                f64::INFINITY
            } else {
                coarse_stat_low
            };
            self.charts.len()
                * CONTRIBUTION_SCENARIO_COUNT
        ];
        if need_forced_meta && !use_forced_envelope {
            for chart_idx in 0..self.charts.len() {
                let team_meta_upper = self.signature_team_meta_upper_bound(signature, chart_idx)?;
                let y_low = coarse_stat_low.max(
                    signature_team_stat_floor(
                        self.seed_score_floor_by_chart[chart_idx],
                        team_meta_upper,
                    ) - max_card_stat,
                );
                teammate_stat_low_by_chart_scenario[chart_idx * CONTRIBUTION_SCENARIO_COUNT
                    ..(chart_idx + 1) * CONTRIBUTION_SCENARIO_COUNT]
                    .fill(y_low);
            }
        }
        if use_forced_envelope {
            let mut category_options = Vec::<(
                u32,
                crate::model::schema::Attribute,
                Vec<(u32, Option<ForcedTeammateBounds>)>,
            )>::new();
            for card in self.cards.iter().filter(|card| signature.allows(card)) {
                if category_options.iter().any(|(band_id, attribute, _)| {
                    *band_id == card.band_id && *attribute == card.attribute
                }) {
                    continue;
                }
                let options = forced_character_options(
                    self.cards,
                    self.profiles,
                    self.charts.len(),
                    signature,
                    card.band_id,
                    card.attribute,
                )?;
                let bounds = forced_teammate_bounds_by_excluded_character(
                    &options,
                    self.charts.len(),
                    signature,
                );
                category_options.push((card.band_id, card.attribute, bounds));
            }
            for (card_idx, card) in self.cards.iter().enumerate() {
                if !signature.allows(card) {
                    continue;
                }
                let teammates = category_options
                    .iter()
                    .find(|(band_id, attribute, _)| {
                        *band_id == card.band_id && *attribute == card.attribute
                    })?
                    .2
                    .iter()
                    .find(|(character_id, _)| *character_id == card.character_id)?
                    .1
                    .clone();
                let Some(teammates) = teammates else {
                    continue;
                };
                let stat = self.profiles[card_idx].stat;
                for chart_idx in 0..self.charts.len() {
                    let skill_meta = signature_skill_meta_values_for_chart(
                        card,
                        &self.profiles[card_idx],
                        signature,
                        chart_idx,
                    )?;
                    let no_skill = self.charts[chart_idx].meta.no_skill;
                    let seed_score_floor = self.seed_score_floor_by_chart[chart_idx];
                    for normal_position in 0..TEAM_SIZE {
                        let normal_meta_upper = no_skill
                            + skill_meta[normal_position]
                            + teammates.normal_plus_captain_meta_high_by_chart[chart_idx];
                        let normal_stat_low = teammates.stat_low.max(
                            signature_team_stat_floor(seed_score_floor, normal_meta_upper) - stat,
                        );
                        let normal_scenario =
                            chart_idx * CONTRIBUTION_SCENARIO_COUNT + normal_position;
                        teammate_stat_low_by_chart_scenario[normal_scenario] =
                            teammate_stat_low_by_chart_scenario[normal_scenario]
                                .min(normal_stat_low);

                        let captain_meta_upper = no_skill
                            + skill_meta[normal_position]
                            + skill_meta[TEAM_SIZE]
                            + teammates.normal_meta_high_by_chart[chart_idx];
                        let captain_stat_low = teammates.stat_low.max(
                            signature_team_stat_floor(seed_score_floor, captain_meta_upper) - stat,
                        );
                        let captain_scenario = chart_idx * CONTRIBUTION_SCENARIO_COUNT
                            + CAPTAIN_SCENARIO_START
                            + normal_position;
                        teammate_stat_low_by_chart_scenario[captain_scenario] =
                            teammate_stat_low_by_chart_scenario[captain_scenario]
                                .min(captain_stat_low);
                    }
                }
            }
        }
        if teammate_stat_low_by_chart_scenario
            .iter()
            .any(|value| !value.is_finite())
        {
            return None;
        }

        Some(SignatureContributionContextBounds {
            stat_high,
            normal_meta_low_by_chart,
            normal_meta_high_by_chart,
            normal_plus_captain_meta_low_by_chart,
            normal_plus_captain_meta_high_by_chart,
            teammate_stat_low_by_chart_scenario,
        })
    }

    fn signature_team_meta_upper_bound(
        &self,
        signature: MedleyPruneSignature,
        chart_idx: usize,
    ) -> Option<f64> {
        self.signature_selected_normal_plus_captain_upper_bound(
            signature, chart_idx, None, TEAM_SIZE,
        )
        .map(|meta| self.charts[chart_idx].meta.no_skill + meta)
    }

    fn signature_selected_normal_plus_captain_upper_bound(
        &self,
        signature: MedleyPruneSignature,
        chart_idx: usize,
        excluded_character_id: Option<u32>,
        team_size: usize,
    ) -> Option<f64> {
        signature_normal_plus_captain_meta_bounds(
            self.cards,
            self.profiles,
            signature,
            chart_idx,
            excluded_character_id,
            team_size,
        )
        .map(|range| range.high)
    }
}

fn forced_teammate_bounds_by_excluded_character(
    character_options: &[ForcedCharacterOptions],
    chart_count: usize,
    signature: MedleyPruneSignature,
) -> Vec<(u32, Option<ForcedTeammateBounds>)> {
    let initial = forced_teammate_dp_initial(chart_count);
    let mut prefix = Vec::with_capacity(character_options.len() + 1);
    prefix.push(initial.clone());
    for options in character_options {
        prefix.push(forced_teammate_dp_advance(
            prefix.last().expect("prefix contains initial state"),
            options,
        ));
    }
    let mut suffix = vec![initial; character_options.len() + 1];
    for idx in (0..character_options.len()).rev() {
        suffix[idx] = forced_teammate_dp_advance(&suffix[idx + 1], &character_options[idx]);
    }

    character_options
        .iter()
        .enumerate()
        .map(|(idx, options)| {
            (
                options.character_id,
                combine_forced_teammate_dp(&prefix[idx], &suffix[idx + 1], signature),
            )
        })
        .collect()
}

fn forced_teammate_dp_initial(chart_count: usize) -> ForcedTeammateDp {
    let mut state = ForcedTeammateDp {
        stat: [[f64::INFINITY; SIGNATURE_BREAK_STATE_COUNT]; TEAM_SIZE],
        normal_meta_by_chart: vec![
            [[f64::NEG_INFINITY; SIGNATURE_BREAK_STATE_COUNT]; TEAM_SIZE];
            chart_count
        ],
        normal_plus_captain_meta_by_chart: vec![
            [[[f64::NEG_INFINITY; 2]; SIGNATURE_BREAK_STATE_COUNT];
                TEAM_SIZE];
            chart_count
        ],
    };
    state.stat[0][0] = 0.0;
    for chart_idx in 0..chart_count {
        state.normal_meta_by_chart[chart_idx][0][0] = 0.0;
        state.normal_plus_captain_meta_by_chart[chart_idx][0][0][0] = 0.0;
    }
    state
}

fn forced_teammate_dp_advance(
    current: &ForcedTeammateDp,
    options: &ForcedCharacterOptions,
) -> ForcedTeammateDp {
    let mut next = current.clone();
    for break_mask in 0..SIGNATURE_BREAK_STATE_COUNT {
        for selected in 0..TEAM_SIZE - 1 {
            for breaks in 0..SIGNATURE_BREAK_STATE_COUNT {
                let next_breaks = breaks | break_mask;
                let stat = current.stat[selected][breaks];
                if stat.is_finite() && options.stat_by_break[break_mask].is_finite() {
                    next.stat[selected + 1][next_breaks] = next.stat[selected + 1][next_breaks]
                        .min(stat + options.stat_by_break[break_mask]);
                }
                for chart_idx in 0..current.normal_meta_by_chart.len() {
                    let selected_normal = options.normal_meta_by_chart_break[chart_idx][break_mask];
                    let normal = current.normal_meta_by_chart[chart_idx][selected][breaks];
                    if normal.is_finite() && selected_normal.is_finite() {
                        next.normal_meta_by_chart[chart_idx][selected + 1][next_breaks] = next
                            .normal_meta_by_chart[chart_idx][selected + 1][next_breaks]
                            .max(normal + selected_normal);
                    }
                    for captain_selected in 0..2 {
                        let value = current.normal_plus_captain_meta_by_chart[chart_idx][selected]
                            [breaks][captain_selected];
                        if !value.is_finite() {
                            continue;
                        }
                        if selected_normal.is_finite() {
                            next.normal_plus_captain_meta_by_chart[chart_idx][selected + 1]
                                [next_breaks][captain_selected] = next
                                .normal_plus_captain_meta_by_chart[chart_idx][selected + 1]
                                [next_breaks][captain_selected]
                                .max(value + selected_normal);
                        }
                        let selected_captain =
                            options.captain_card_meta_by_chart_break[chart_idx][break_mask];
                        if captain_selected == 0 && selected_captain.is_finite() {
                            next.normal_plus_captain_meta_by_chart[chart_idx][selected + 1]
                                [next_breaks][1] = next.normal_plus_captain_meta_by_chart
                                [chart_idx][selected + 1][next_breaks][1]
                                .max(value + selected_captain);
                        }
                    }
                }
            }
        }
    }
    next
}

fn combine_forced_teammate_dp(
    left: &ForcedTeammateDp,
    right: &ForcedTeammateDp,
    signature: MedleyPruneSignature,
) -> Option<ForcedTeammateBounds> {
    let required_breaks = contribution_signature_required_break_mask(signature);
    let chart_count = left.normal_meta_by_chart.len();
    let mut stat_low = f64::INFINITY;
    let mut normal_meta_high_by_chart = vec![f64::NEG_INFINITY; chart_count];
    let mut normal_plus_captain_meta_high_by_chart = vec![f64::NEG_INFINITY; chart_count];
    for left_selected in 0..TEAM_SIZE {
        let right_selected = TEAM_SIZE - 1 - left_selected;
        for left_breaks in 0..SIGNATURE_BREAK_STATE_COUNT {
            for right_breaks in 0..SIGNATURE_BREAK_STATE_COUNT {
                if (left_breaks | right_breaks) & required_breaks != required_breaks {
                    continue;
                }
                let left_stat = left.stat[left_selected][left_breaks];
                let right_stat = right.stat[right_selected][right_breaks];
                if left_stat.is_finite() && right_stat.is_finite() {
                    stat_low = stat_low.min(left_stat + right_stat);
                }
                for chart_idx in 0..chart_count {
                    let left_normal =
                        left.normal_meta_by_chart[chart_idx][left_selected][left_breaks];
                    let right_normal =
                        right.normal_meta_by_chart[chart_idx][right_selected][right_breaks];
                    if left_normal.is_finite() && right_normal.is_finite() {
                        normal_meta_high_by_chart[chart_idx] =
                            normal_meta_high_by_chart[chart_idx].max(left_normal + right_normal);
                    }
                    for left_captain in 0..2 {
                        let right_captain = 1 - left_captain;
                        let left_value = left.normal_plus_captain_meta_by_chart[chart_idx]
                            [left_selected][left_breaks][left_captain];
                        let right_value = right.normal_plus_captain_meta_by_chart[chart_idx]
                            [right_selected][right_breaks][right_captain];
                        if left_value.is_finite() && right_value.is_finite() {
                            normal_plus_captain_meta_high_by_chart[chart_idx] =
                                normal_plus_captain_meta_high_by_chart[chart_idx]
                                    .max(left_value + right_value);
                        }
                    }
                }
            }
        }
    }
    if !stat_low.is_finite()
        || normal_meta_high_by_chart
            .iter()
            .chain(normal_plus_captain_meta_high_by_chart.iter())
            .any(|value| !value.is_finite())
    {
        return None;
    }
    Some(ForcedTeammateBounds {
        stat_low,
        normal_meta_high_by_chart,
        normal_plus_captain_meta_high_by_chart,
    })
}

fn forced_character_options(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    chart_count: usize,
    signature: MedleyPruneSignature,
    target_band_id: u32,
    target_attribute: crate::model::schema::Attribute,
) -> Option<Vec<ForcedCharacterOptions>> {
    let mut options_by_character = BTreeMap::<u32, ForcedCharacterOptions>::new();
    for (card_idx, card) in cards.iter().enumerate() {
        if !signature.allows(card) {
            continue;
        }
        let break_mask = contribution_category_break_mask(target_band_id, target_attribute, card);
        let options = options_by_character
            .entry(card.character_id)
            .or_insert_with(|| ForcedCharacterOptions {
                character_id: card.character_id,
                stat_by_break: [f64::INFINITY; SIGNATURE_BREAK_STATE_COUNT],
                normal_meta_by_chart_break: vec![
                    [f64::NEG_INFINITY; SIGNATURE_BREAK_STATE_COUNT];
                    chart_count
                ],
                captain_card_meta_by_chart_break: vec![
                    [f64::NEG_INFINITY;
                        SIGNATURE_BREAK_STATE_COUNT];
                    chart_count
                ],
            });
        options.stat_by_break[break_mask] =
            options.stat_by_break[break_mask].min(profiles[card_idx].stat);
        for chart_idx in 0..chart_count {
            let normal_meta = signature_card_normal_meta_bounds_for_chart(
                card,
                &profiles[card_idx],
                signature,
                chart_idx,
            )?
            .high;
            let captain_meta = signature_card_captain_meta_for_chart(
                card,
                &profiles[card_idx],
                signature,
                chart_idx,
            )?;
            options.normal_meta_by_chart_break[chart_idx][break_mask] =
                options.normal_meta_by_chart_break[chart_idx][break_mask].max(normal_meta);
            options.captain_card_meta_by_chart_break[chart_idx][break_mask] = options
                .captain_card_meta_by_chart_break[chart_idx][break_mask]
                .max(normal_meta + captain_meta);
        }
    }
    Some(options_by_character.into_values().collect())
}

fn contribution_category_break_mask(
    target_band_id: u32,
    target_attribute: crate::model::schema::Attribute,
    card: &PreparedCard,
) -> usize {
    usize::from(card.band_id != target_band_id)
        | (usize::from(card.attribute != target_attribute) << 1)
}

fn contribution_signature_required_break_mask(signature: MedleyPruneSignature) -> usize {
    match signature {
        MedleyPruneSignature::Mixed => 0b11,
        MedleyPruneSignature::UnifiedBand(_) => 0b10,
        MedleyPruneSignature::UnifiedAttribute(_) => 0b01,
        MedleyPruneSignature::UnifiedBandAttribute(_, _) => 0,
    }
}

fn signature_team_stat_floor(seed_score_floor: f64, team_meta_upper: f64) -> f64 {
    if seed_score_floor <= 0.0 || team_meta_upper <= 0.0 {
        return 0.0;
    }

    // Exact note-by-note scoring is bounded by ceil(stat * continuous_meta). Therefore a team
    // reaching integer score S only proves stat * meta > S - 1, not stat * meta >= S.
    (seed_score_floor - 1.0).max(0.0) / team_meta_upper
}

fn seed_score_floor_by_chart(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    current_best: i32,
) -> Vec<f64> {
    if current_best <= 0 || charts.len() != MEDLEY_TEAM_COUNT {
        return vec![0.0; charts.len()];
    }

    let best_any_team_scores: Vec<_> = (0..charts.len())
        .map(|chart_idx| best_any_team_score_upper_bound(cards, charts, profiles, chart_idx))
        .collect();
    seed_score_floor_by_chart_from_upper_bounds(charts.len(), current_best, &best_any_team_scores)
}

fn seed_score_floor_by_chart_from_upper_bounds(
    chart_count: usize,
    current_best: i32,
    best_any_team_scores: &[f64],
) -> Vec<f64> {
    if current_best <= 0 || chart_count != MEDLEY_TEAM_COUNT {
        return vec![0.0; chart_count];
    }

    (0..chart_count)
        .map(|chart_idx| {
            let other_song_upper_bound = best_any_team_scores
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != chart_idx)
                .map(|(_, &score)| score)
                .sum::<f64>();
            (current_best as f64 - other_song_upper_bound).max(0.0)
        })
        .collect()
}

fn signature_card_affine_model_for_chart_skill_position(
    card: &PreparedCard,
    profile: &MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
    chart_idx: usize,
    skill_position: usize,
    context: &SignatureContributionContextBounds,
    chart: &Chart,
) -> Option<ContributionAffineModel> {
    let skill_meta_values =
        signature_skill_meta_values_for_chart(card, profile, signature, chart_idx)?;
    let (skill_meta, captain_coupled) = if skill_position < CAPTAIN_SCENARIO_START {
        (*skill_meta_values.get(skill_position)?, false)
    } else {
        let normal_position = skill_position - CAPTAIN_SCENARIO_START;
        (
            *skill_meta_values.get(normal_position)? + *skill_meta_values.get(TEAM_SIZE)?,
            true,
        )
    };
    let stat = profile.stat;
    let y_low = *context
        .teammate_stat_low_by_chart_scenario
        .get(chart_idx * CONTRIBUTION_SCENARIO_COUNT + skill_position)?;
    let y_high = context.stat_high;
    if y_low > y_high {
        return None;
    }
    let (teammate_meta_low, teammate_meta_high) = if captain_coupled {
        (
            context.normal_meta_low_by_chart[chart_idx],
            context.normal_meta_high_by_chart[chart_idx],
        )
    } else {
        (
            context.normal_plus_captain_meta_low_by_chart[chart_idx],
            context.normal_plus_captain_meta_high_by_chart[chart_idx],
        )
    };
    let x_low = chart.meta.no_skill + teammate_meta_low;
    let x_high = chart.meta.no_skill + teammate_meta_high;

    Some(ContributionAffineModel {
        stat,
        meta: skill_meta,
        constant: stat * skill_meta,
        x_low,
        x_high,
        y_low,
        y_high,
    })
}

fn affine_contribution_endpoint_comparison(
    left: ContributionAffineModel,
    right: ContributionAffineModel,
) -> ContributionEndpointComparison {
    let x_low = left.x_low.min(right.x_low);
    let x_high = left.x_high.max(right.x_high);
    let y_low = left.y_low.min(right.y_low);
    let y_high = left.y_high.max(right.y_high);
    let mut result = ContributionEndpointComparison {
        margin: f64::INFINITY,
        left_value: 0.0,
        right_value: 0.0,
        x: 0.0,
        y: 0.0,
        x_low,
        x_high,
        y_low,
        y_high,
        left_a: left.stat,
        left_b: left.meta,
        left_c: left.constant,
        right_a: right.stat,
        right_b: right.meta,
        right_c: right.constant,
    };

    for (x, y) in [
        (x_low, y_low),
        (x_low, y_high),
        (x_high, y_low),
        (x_high, y_high),
    ] {
        let left_value = left.value_at(x, y);
        let right_value = right.value_at(x, y);
        let margin = left_value - right_value;
        if margin < result.margin {
            result = ContributionEndpointComparison {
                margin,
                left_value,
                right_value,
                x,
                y,
                x_low,
                x_high,
                y_low,
                y_high,
                left_a: left.stat,
                left_b: left.meta,
                left_c: left.constant,
                right_a: right.stat,
                right_b: right.meta,
                right_c: right.constant,
            };
        }
    }

    result
}

fn affine_contribution_min_margin(
    left: ContributionAffineModel,
    right: ContributionAffineModel,
) -> f64 {
    let x_low = left.x_low.min(right.x_low);
    let x_high = left.x_high.max(right.x_high);
    let y_low = left.y_low.min(right.y_low);
    let y_high = left.y_high.max(right.y_high);
    let stat_delta = left.stat - right.stat;
    let meta_delta = left.meta - right.meta;
    let constant_delta = left.constant - right.constant;
    let x = if stat_delta >= 0.0 { x_low } else { x_high };
    let y = if meta_delta >= 0.0 { y_low } else { y_high };

    stat_delta * x + meta_delta * y + constant_delta
}

fn signature_card_normal_meta_bounds_for_chart(
    card: &PreparedCard,
    profile: &MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
    chart_idx: usize,
) -> Option<ValueRange> {
    let values = signature_skill_meta_values_for_chart(card, profile, signature, chart_idx)?;
    let normal_values = values.get(..TEAM_SIZE)?;
    let low = normal_values.iter().copied().fold(f64::INFINITY, f64::min);
    let high = normal_values
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    Some(ValueRange { low, high })
}

fn signature_card_captain_meta_for_chart(
    card: &PreparedCard,
    profile: &MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
    chart_idx: usize,
) -> Option<f64> {
    signature_skill_meta_values_for_chart(card, profile, signature, chart_idx)
        .and_then(|values| values.get(TEAM_SIZE).copied())
}

fn signature_skill_meta_values_for_chart<'a>(
    card: &PreparedCard,
    profile: &'a MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
    chart_idx: usize,
) -> Option<&'a [f64]> {
    let score_up = card
        .score_up
        .resolve(signature.team_band_id(), signature.team_attribute());
    let values = profile.skill_meta_for_score_up(score_up)?;
    let start = chart_idx * (TEAM_SIZE + 1);
    values.get(start..start + TEAM_SIZE + 1)
}

fn character_value_ranges<F>(cards: &[PreparedCard], mut value: F) -> Vec<ValueRange>
where
    F: FnMut(usize, &PreparedCard) -> Option<ValueRange>,
{
    let mut ranges_by_character: BTreeMap<u32, ValueRange> = BTreeMap::new();
    for (idx, card) in cards.iter().enumerate() {
        let Some(range) = value(idx, card) else {
            continue;
        };
        ranges_by_character
            .entry(card.character_id)
            .and_modify(|existing| {
                existing.low = existing.low.min(range.low);
                existing.high = existing.high.max(range.high);
            })
            .or_insert(range);
    }

    ranges_by_character.into_values().collect()
}

fn signature_normal_plus_captain_meta_bounds(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    chart_idx: usize,
    excluded_character_id: Option<u32>,
    selected_card_count: usize,
) -> Option<ValueRange> {
    let mut normal_by_character = BTreeMap::new();
    let mut captain_candidates = Vec::new();
    for (idx, card) in cards.iter().enumerate() {
        if !signature.allows(card) || excluded_character_id == Some(card.character_id) {
            continue;
        }
        let normal_meta = signature_card_normal_meta_bounds_for_chart(
            card,
            &profiles[idx],
            signature,
            chart_idx,
        )?;
        let captain_meta =
            signature_card_captain_meta_for_chart(card, &profiles[idx], signature, chart_idx)?;
        normal_by_character
            .entry(card.character_id)
            .and_modify(|range: &mut ValueRange| {
                range.low = range.low.min(normal_meta.low);
                range.high = range.high.max(normal_meta.high);
            })
            .or_insert(normal_meta);
        captain_candidates.push(CaptainMetaCandidate {
            character_id: card.character_id,
            normal_meta,
            captain_meta,
        });
    }

    selected_normal_plus_captain_meta_bounds(
        &normal_by_character,
        &captain_candidates,
        selected_card_count,
    )
}

fn selected_normal_plus_captain_meta_bounds(
    normal_by_character: &BTreeMap<u32, ValueRange>,
    captain_candidates: &[CaptainMetaCandidate],
    selected_card_count: usize,
) -> Option<ValueRange> {
    let other_card_count = selected_card_count.checked_sub(1)?;
    if normal_by_character.len() < selected_card_count {
        return None;
    }

    let normal_low_by_character = normal_by_character
        .iter()
        .map(|(&character_id, range)| (character_id, range.low))
        .collect();
    let normal_high_by_character = normal_by_character
        .iter()
        .map(|(&character_id, range)| (character_id, range.high))
        .collect();
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for candidate in captain_candidates {
        let other_low = bottom_n_character_value_sum_excluding(
            &normal_low_by_character,
            other_card_count,
            candidate.character_id,
        )?;
        let other_high = top_n_character_value_sum_excluding(
            &normal_high_by_character,
            other_card_count,
            candidate.character_id,
        )?;
        low = low.min(candidate.normal_meta.low + candidate.captain_meta + other_low);
        high = high.max(candidate.normal_meta.high + candidate.captain_meta + other_high);
    }

    (low.is_finite() && high.is_finite() && low <= high).then_some(ValueRange { low, high })
}

fn top_n_character_value_sum_excluding(
    values_by_character: &BTreeMap<u32, f64>,
    count: usize,
    excluded_character_id: u32,
) -> Option<f64> {
    if count == 0 {
        return Some(0.0);
    }

    let mut values: Vec<_> = values_by_character
        .iter()
        .filter(|(character_id, _)| **character_id != excluded_character_id)
        .map(|(_, &value)| value)
        .collect();
    if values.len() < count {
        return None;
    }
    values.sort_by(|left, right| right.total_cmp(left));
    Some(values.into_iter().take(count).sum())
}

fn bottom_n_character_value_sum_excluding(
    values_by_character: &BTreeMap<u32, f64>,
    count: usize,
    excluded_character_id: u32,
) -> Option<f64> {
    if count == 0 {
        return Some(0.0);
    }

    let mut values: Vec<_> = values_by_character
        .iter()
        .filter(|(character_id, _)| **character_id != excluded_character_id)
        .map(|(_, &value)| value)
        .collect();
    if values.len() < count {
        return None;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    Some(values.into_iter().take(count).sum())
}

fn top_value_range_sum<const N: usize, F>(ranges: &[ValueRange], mut value: F) -> Option<f64>
where
    F: FnMut(ValueRange) -> f64,
{
    if ranges.len() < N {
        return None;
    }

    let mut values: Vec<_> = ranges.iter().copied().map(&mut value).collect();
    values.sort_by(|left, right| right.total_cmp(left));
    Some(values.iter().take(N).sum())
}

fn bottom_value_range_sum<const N: usize, F>(ranges: &[ValueRange], mut value: F) -> Option<f64>
where
    F: FnMut(ValueRange) -> f64,
{
    if ranges.len() < N {
        return None;
    }

    let mut values: Vec<_> = ranges.iter().copied().map(&mut value).collect();
    values.sort_by(|left, right| left.total_cmp(right));
    Some(values.iter().take(N).sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medley::team::adjusted_card_stats;
    use crate::medley::test_support::{medley_charts, prepared_card, selected_cool_items};
    use crate::model::chart::{ChartNode, ChartNodeType, TeamCardSkill};
    use crate::model::preparation::{AreaItemPercent, ScoreUp, StatValue};
    use crate::model::schema::Attribute;
    use crate::team_prune::hard::medley_card_prune_profiles;

    #[test]
    fn same_shape_prefilter_uses_contribution_tradeoff_only_within_shape() {
        let mut dominator = prepared_card(1, 1, 1, Attribute::Cool);
        dominator.stat = StatValue {
            performance: 10_000.0,
            technique: 10_000.0,
            visual: 10_000.0,
        };
        dominator.score_up = ScoreUp {
            default: 0.99,
            unification_activate_effect_value: None,
            unification_activate_condition_band_id: None,
            unification_activate_condition_type: None,
        };
        let mut target = prepared_card(2, 1, 1, Attribute::Cool);
        target.stat = StatValue {
            performance: 1_000.0,
            technique: 1_000.0,
            visual: 1_000.0,
        };
        target.score_up.default = 1.0;
        let mut different_shape = target.clone();
        different_shape.card_id = 3;
        different_shape.skill.duration = 7.0;
        let mut cards = vec![dominator, target, different_shape];
        for character_id in 2..=5 {
            let (band_id, attribute) = if character_id == 2 {
                (2, Attribute::Happy)
            } else {
                (1, Attribute::Cool)
            };
            cards.push(prepared_card(
                100 + character_id,
                character_id,
                band_id,
                attribute,
            ));
        }
        let chart = medley_charts().remove(0);
        let stats = adjusted_card_stats(&cards, &AreaItemPercent::empty(), &selected_cool_items());
        let profiles =
            medley_card_prune_profiles(&cards, std::slice::from_ref(&chart), &stats).unwrap();

        let active = same_shape_contribution_active_indices(
            &cards,
            std::slice::from_ref(&chart),
            &profiles,
            MedleyPruneSignature::Mixed,
            1,
            None,
        );

        assert!(active.contains(&0));
        assert!(!active.contains(&1));
        assert!(active.contains(&2));
    }

    #[test]
    fn captain_context_selects_captain_from_normal_teammates() {
        let exact = |value| ValueRange {
            low: value,
            high: value,
        };
        let normal_by_character = BTreeMap::from([
            (1, exact(100.0)),
            (2, exact(90.0)),
            (3, exact(80.0)),
            (4, exact(70.0)),
            (5, exact(1.0)),
        ]);
        let captain_candidates = [
            CaptainMetaCandidate {
                character_id: 1,
                normal_meta: exact(100.0),
                captain_meta: 0.0,
            },
            CaptainMetaCandidate {
                character_id: 2,
                normal_meta: exact(90.0),
                captain_meta: 500.0,
            },
            CaptainMetaCandidate {
                character_id: 3,
                normal_meta: exact(80.0),
                captain_meta: 500.0,
            },
            CaptainMetaCandidate {
                character_id: 4,
                normal_meta: exact(70.0),
                captain_meta: 500.0,
            },
            CaptainMetaCandidate {
                character_id: 5,
                normal_meta: exact(1.0),
                captain_meta: 1_000.0,
            },
        ];

        let joint = selected_normal_plus_captain_meta_bounds(
            &normal_by_character,
            &captain_candidates,
            TEAM_SIZE - 1,
        )
        .unwrap();
        let independent_low = 1.0 + 70.0 + 80.0 + 90.0;
        let independent_high = 100.0 + 90.0 + 80.0 + 70.0 + 1_000.0;

        assert_eq!(joint.low, 251.0);
        assert_eq!(joint.high, 1_271.0);
        assert!(joint.low > independent_low);
        assert!(joint.high < independent_high);
    }

    #[test]
    fn forced_teammate_dp_respects_exact_mode_and_excluded_character() {
        let option = |character_id, break_mask, stat, normal, captain_extra| {
            let mut stat_by_break = [f64::INFINITY; SIGNATURE_BREAK_STATE_COUNT];
            stat_by_break[break_mask] = stat;
            let mut normal_by_break = [f64::NEG_INFINITY; SIGNATURE_BREAK_STATE_COUNT];
            normal_by_break[break_mask] = normal;
            let mut captain_by_break = [f64::NEG_INFINITY; SIGNATURE_BREAK_STATE_COUNT];
            captain_by_break[break_mask] = normal + captain_extra;
            ForcedCharacterOptions {
                character_id,
                stat_by_break,
                normal_meta_by_chart_break: vec![normal_by_break],
                captain_card_meta_by_chart_break: vec![captain_by_break],
            }
        };
        let options = vec![
            option(1, 0, 1.0, 1.0, 1.0),
            option(2, 0b01, 10.0, 1.0, 10.0),
            option(3, 0b10, 20.0, 2.0, 20.0),
            option(4, 0, 30.0, 3.0, 30.0),
            option(5, 0, 40.0, 4.0, 40.0),
            option(6, 0b11, 100.0, 100.0, 100.0),
        ];

        let bounds =
            forced_teammate_bounds_by_excluded_character(&options, 1, MedleyPruneSignature::Mixed)
                .into_iter()
                .find(|(character_id, _)| *character_id == 1)
                .and_then(|(_, bounds)| bounds)
                .expect("four exact Mixed teammates should exist");

        assert_eq!(bounds.stat_low, 100.0);
        assert_eq!(bounds.normal_meta_high_by_chart, vec![109.0]);
        assert_eq!(bounds.normal_plus_captain_meta_high_by_chart, vec![209.0]);
    }

    #[test]
    fn contribution_dominance_requires_every_skill_position() {
        let mut chart_nodes = Vec::new();
        for (activation, note_count) in [1, 8, 3, 7, 3, 12].into_iter().enumerate() {
            let start = activation as f64 * 10.0;
            chart_nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: start,
            });
            for note in 0..note_count {
                chart_nodes.push(ChartNode {
                    node_type: ChartNodeType::Node,
                    time: start + (note + 1) as f64 * 8.0 / (note_count + 1) as f64,
                });
            }
        }
        let mut chart = crate::Chart::new(20, chart_nodes);
        chart.init(0, false).unwrap();

        let mut target = prepared_card(1, 1, 1, Attribute::Cool);
        target.stat = StatValue {
            performance: 638.580_950_465_604_7,
            technique: 638.580_950_465_604_7,
            visual: 638.580_950_465_604_7,
        };
        target.skill.duration = 8.0;
        target.skill.score_up = 1.002_647_146_662_322;
        target.score_up.default = target.skill.score_up;

        let mut high_stat = prepared_card(2, 1, 1, Attribute::Cool);
        high_stat.stat = StatValue {
            performance: 976.589_946_124_396_8,
            technique: 976.589_946_124_396_8,
            visual: 976.589_946_124_396_8,
        };
        high_stat.skill.duration = 8.0;
        high_stat.skill.score_up = 0.448_601_122_368_907_53;
        high_stat.score_up.default = high_stat.skill.score_up;

        let mut cards = vec![target, high_stat];
        for (card_id, character_id, stat, duration, score_up) in [
            (3, 2, 949.442_462_628_881_3, 7.0, 0.150_911_270_350_869_54),
            (4, 3, 468.835_329_161_082_54, 6.0, 0.587_418_832_302_039_3),
            (5, 4, 1_614.865_235_288_985_3, 3.0, 0.617_717_406_683_017_6),
            (6, 5, 1_669.423_883_172_874_4, 6.0, 1.367_775_627_222_917_6),
        ] {
            let (band_id, attribute) = if character_id == 2 {
                (2, Attribute::Happy)
            } else {
                (1, Attribute::Cool)
            };
            let mut card = prepared_card(card_id, character_id, band_id, attribute);
            card.stat = StatValue {
                performance: stat,
                technique: stat,
                visual: stat,
            };
            card.skill.duration = duration;
            card.skill.score_up = score_up;
            card.score_up.default = score_up;
            cards.push(card);
        }

        let stats = adjusted_card_stats(&cards, &AreaItemPercent::empty(), &selected_cool_items());
        let profiles =
            medley_card_prune_profiles(&cards, std::slice::from_ref(&chart), &stats).unwrap();
        let comparison =
            MedleyContributionDominance::new(&cards, std::slice::from_ref(&chart), &profiles, 0)
                .replacement_comparison_for_signature(1, 0, MedleyPruneSignature::Mixed);
        let active = same_shape_contribution_active_indices(
            &cards,
            std::slice::from_ref(&chart),
            &profiles,
            MedleyPruneSignature::Mixed,
            1,
            None,
        );

        assert!(!comparison.replaces);
        assert!(comparison.failed_skill_position.is_some());
        assert!(active.contains(&0));
        assert!(active.contains(&1));
    }

    #[test]
    fn contribution_dominance_accounts_for_captain_reusing_a_normal_card() {
        let model = |stat: f64, meta: f64| ContributionAffineModel {
            stat,
            meta,
            constant: stat * meta,
            x_low: 10.0,
            x_high: 10.0,
            y_low: 30.0,
            y_high: 30.0,
        };
        let mut left_models = vec![model(15.0, 1.0); CAPTAIN_SCENARIO_START];
        left_models.extend(vec![model(15.0, 2.0); TEAM_SIZE]);
        let mut right_models = vec![model(10.0, 2.0); CAPTAIN_SCENARIO_START];
        right_models.extend(vec![model(10.0, 4.0); TEAM_SIZE]);

        assert!(left_models[..CAPTAIN_SCENARIO_START]
            .iter()
            .zip(&right_models[..CAPTAIN_SCENARIO_START])
            .all(|(&left, &right)| affine_contribution_min_margin(left, right) >= 0.0));
        assert!(left_models[CAPTAIN_SCENARIO_START..]
            .iter()
            .zip(&right_models[CAPTAIN_SCENARIO_START..])
            .any(|(&left, &right)| affine_contribution_min_margin(left, right) < 0.0));

        let cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool),
            prepared_card(2, 1, 1, Attribute::Cool),
        ];
        let mut all_models = left_models;
        all_models.extend(right_models);
        let models = SignatureContributionModels {
            allowed_indices: vec![0, 1],
            complete_by_card: vec![true, true],
            models_by_card_skill_position: all_models,
            models_per_card: CONTRIBUTION_SCENARIO_COUNT,
        };
        let dominance = MedleyContributionDominance::new(&cards, &[], &[], 0);

        assert!(!dominance.card_can_replace_with_models(0, 1, &models));
    }

    #[test]
    fn contribution_scenarios_are_exactly_the_ten_physical_roles() {
        let labels = (0..CONTRIBUTION_SCENARIO_COUNT)
            .map(skill_position_label)
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            [
                "0",
                "1",
                "2",
                "3",
                "4",
                "0+captain",
                "1+captain",
                "2+captain",
                "3+captain",
                "4+captain",
            ]
        );
    }

    #[test]
    fn negative_margin_inside_epsilon_never_creates_dominance_edge() {
        let right = ContributionAffineModel {
            stat: 10.0,
            meta: 2.0,
            constant: 20.0,
            x_low: 5.0,
            x_high: 5.0,
            y_low: 30.0,
            y_high: 30.0,
        };
        let left = ContributionAffineModel {
            constant: right.constant - SCORE_CONTRIBUTION_EPS / 2.0,
            ..right
        };
        assert!(affine_contribution_min_margin(left, right) < 0.0);

        let cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool),
            prepared_card(2, 1, 1, Attribute::Cool),
        ];
        let mut all_models = vec![left; CONTRIBUTION_SCENARIO_COUNT];
        all_models.extend(vec![right; CONTRIBUTION_SCENARIO_COUNT]);
        let models = SignatureContributionModels {
            allowed_indices: vec![0, 1],
            complete_by_card: vec![true, true],
            models_by_card_skill_position: all_models,
            models_per_card: CONTRIBUTION_SCENARIO_COUNT,
        };
        let dominance = MedleyContributionDominance::new(&cards, &[], &[], 0);

        assert!(!dominance.card_can_replace_with_models(0, 1, &models));
    }

    #[test]
    fn signature_team_meta_upper_bound_covers_exact_integer_scores() {
        let mut cards = (1..=6)
            .map(|character_id| prepared_card(character_id, character_id, 1, Attribute::Cool))
            .collect::<Vec<_>>();
        cards[0].score_up = ScoreUp {
            default: 0.8,
            unification_activate_effect_value: Some(1.8),
            unification_activate_condition_band_id: Some(1),
            unification_activate_condition_type: Some(Attribute::Cool),
        };
        let mut alternative = prepared_card(100, 1, 2, Attribute::Happy);
        alternative.skill.duration = 7.0;
        alternative.score_up.default = 1.35;
        cards.push(alternative);

        let charts = medley_charts();
        let card_stats =
            adjusted_card_stats(&cards, &AreaItemPercent::empty(), &selected_cool_items());
        let profiles = medley_card_prune_profiles(&cards, &charts, &card_stats).unwrap();
        let dominance = MedleyContributionDominance::new(&cards, &charts, &profiles, 0);

        let signatures = [
            MedleyPruneSignature::Mixed,
            MedleyPruneSignature::UnifiedBand(1),
            MedleyPruneSignature::UnifiedAttribute(Attribute::Cool),
            MedleyPruneSignature::UnifiedBandAttribute(1, Attribute::Cool),
        ];
        for signature in signatures {
            for chart_idx in 0..charts.len() {
                let upper = dominance
                    .signature_team_meta_upper_bound(signature, chart_idx)
                    .unwrap();
                for mask in 0usize..(1 << cards.len()) {
                    if mask.count_ones() != TEAM_SIZE as u32 {
                        continue;
                    }
                    let indices = (0..cards.len())
                        .filter(|idx| mask & (1 << idx) != 0)
                        .collect::<Vec<_>>();
                    if indices.iter().any(|&idx| !signature.allows(&cards[idx])) {
                        continue;
                    }
                    let mut characters = indices
                        .iter()
                        .map(|&idx| cards[idx].character_id)
                        .collect::<Vec<_>>();
                    characters.sort_unstable();
                    characters.dedup();
                    if characters.len() != TEAM_SIZE {
                        continue;
                    }

                    let stat =
                        crate::floor_team_stat(indices.iter().map(|&idx| profiles[idx].stat));
                    let team = indices
                        .iter()
                        .map(|&idx| TeamCardSkill {
                            score_up: cards[idx]
                                .score_up
                                .resolve(signature.team_band_id(), signature.team_attribute()),
                            ..cards[idx].skill
                        })
                        .collect::<Vec<_>>();
                    let exact = charts[chart_idx]
                        .get_max_score_order(&team, stat, false)
                        .unwrap();
                    let continuous_upper = (stat as f64 * upper).ceil() as i32;
                    assert!(
                        exact.score <= continuous_upper,
                        "signature={signature:?} chart={chart_idx} cards={indices:?} exact={} upper={continuous_upper}",
                        exact.score
                    );
                    let stat_floor = signature_team_stat_floor(exact.score as f64, upper);
                    assert!(stat as f64 >= stat_floor);
                }
            }
        }
    }

    #[test]
    fn affine_min_margin_matches_endpoint_scan() {
        let cases = [
            (
                ContributionAffineModel {
                    stat: 10.0,
                    meta: 1.5,
                    constant: 7.0,
                    x_low: 3.0,
                    x_high: 9.0,
                    y_low: 20.0,
                    y_high: 80.0,
                },
                ContributionAffineModel {
                    stat: 8.0,
                    meta: 2.5,
                    constant: 5.0,
                    x_low: 4.0,
                    x_high: 12.0,
                    y_low: 10.0,
                    y_high: 70.0,
                },
            ),
            (
                ContributionAffineModel {
                    stat: 6.0,
                    meta: 4.0,
                    constant: 1.0,
                    x_low: 1.0,
                    x_high: 5.0,
                    y_low: 2.0,
                    y_high: 6.0,
                },
                ContributionAffineModel {
                    stat: 11.0,
                    meta: 3.0,
                    constant: 9.0,
                    x_low: 0.5,
                    x_high: 7.0,
                    y_low: 1.0,
                    y_high: 8.0,
                },
            ),
        ];

        for (left, right) in cases {
            let endpoint = affine_contribution_endpoint_comparison(left, right);
            let margin = affine_contribution_min_margin(left, right);
            assert!((endpoint.margin - margin).abs() < f64::EPSILON);
        }
    }
}
