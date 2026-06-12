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
const SCORE_CONTRIBUTION_EPS: f64 = 1e-7;
const CONTRIBUTION_DIAGNOSTIC_TARGETS: usize = 3;
const CONTRIBUTION_DIAGNOSTIC_NEAR_MISSES: usize = 4;

pub(in crate::medley) fn contribution_dominance_graph_for_signature(
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

pub(in crate::medley) fn same_character_score_contribution_cover(
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

pub(in crate::medley) fn full_medley_score_contribution_cover(
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

pub(in crate::medley) fn trace_score_contribution_cover_diagnostics(
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
                "medley contribution near miss: target_card={} candidate_card={} candidate_character={} new_free_replacements={} new_dominators={} new_blocked_teammates={} new_other_team_capacity={} reason={} margin={:.3} failed_chart={} left_value={:.3} right_value={:.3} reverse_replaces={} reverse_reason={} reverse_margin={:.3}",
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
                near_miss.comparison.left_value,
                near_miss.comparison.right_value,
                near_miss.reverse_comparison.replaces,
                near_miss.reverse_comparison.reason.label(),
                near_miss.reverse_comparison.margin,
            );
            if let Some(endpoint) = near_miss.comparison.endpoint {
                eprintln!(
                    "medley contribution affine detail: target_card={} candidate_card={} chart={} endpoint_x={:.6} endpoint_y={:.3} x_range=[{:.6},{:.6}] y_range=[{:.3},{:.3}] left_fn={:.3}*x+{:.6}*y+{:.3} right_fn={:.3}*x+{:.6}*y+{:.3}",
                    target.card_id,
                    candidate.card_id,
                    near_miss
                        .comparison
                        .failed_chart
                        .map(|idx| idx.to_string())
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
                    "medley contribution reverse affine detail: target_card={} candidate_card={} chart={} endpoint_x={:.6} endpoint_y={:.3} x_range=[{:.6},{:.6}] y_range=[{:.3},{:.3}] target_fn={:.3}*x+{:.6}*y+{:.3} candidate_fn={:.3}*x+{:.6}*y+{:.3}",
                    target.card_id,
                    candidate.card_id,
                    near_miss
                        .reverse_comparison
                        .failed_chart
                        .map(|idx| idx.to_string())
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

pub(in crate::medley) struct MedleyContributionDominance<'a> {
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
    stat_low: f64,
    stat_high: f64,
    max_card_stat: f64,
    normal_meta_low_by_chart: Vec<f64>,
    normal_meta_high_by_chart: Vec<f64>,
    captain_est_by_chart: Vec<f64>,
    team_stat_floor_by_chart: Vec<f64>,
}

#[derive(Debug, Clone, Copy)]
struct ValueRange {
    low: f64,
    high: f64,
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
    models_by_card_chart: Vec<ContributionAffineModel>,
    chart_count: usize,
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

        let start = card_idx.checked_mul(self.chart_count)?;
        self.models_by_card_chart
            .get(start..start + self.chart_count)
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
    left_value: f64,
    right_value: f64,
) -> ContributionReplacementComparison {
    ContributionReplacementComparison {
        replaces: false,
        reason,
        margin,
        failed_chart,
        left_value,
        right_value,
        endpoint: None,
    }
}

impl<'a> MedleyContributionDominance<'a> {
    pub(in crate::medley) fn new(
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
        let mut min_margin_left_value = 0.0;
        let mut min_margin_right_value = 0.0;
        let mut min_margin_endpoint = None;

        for chart_idx in 0..self.charts.len() {
            let Some(left_model) =
                self.card_affine_model_for_signature(left_idx, signature, chart_idx)
            else {
                return contribution_replacement_reject(
                    ContributionReplacementReason::MissingBounds,
                    f64::NEG_INFINITY,
                    Some(chart_idx),
                    0.0,
                    0.0,
                );
            };
            let Some(right_model) =
                self.card_affine_model_for_signature(right_idx, signature, chart_idx)
            else {
                return contribution_replacement_reject(
                    ContributionReplacementReason::MissingBounds,
                    f64::NEG_INFINITY,
                    Some(chart_idx),
                    0.0,
                    0.0,
                );
            };

            let endpoint = affine_contribution_endpoint_comparison(left_model, right_model);
            let margin = endpoint.margin;
            if margin < min_margin {
                min_margin = margin;
                min_margin_chart = Some(chart_idx);
                min_margin_left_value = endpoint.left_value;
                min_margin_right_value = endpoint.right_value;
                min_margin_endpoint = Some(endpoint);
            }
            if margin + SCORE_CONTRIBUTION_EPS < 0.0 {
                return ContributionReplacementComparison {
                    replaces: false,
                    reason: ContributionReplacementReason::Score,
                    margin,
                    failed_chart: Some(chart_idx),
                    left_value: endpoint.left_value,
                    right_value: endpoint.right_value,
                    endpoint: Some(endpoint),
                };
            }
            strictly_better |= margin > SCORE_CONTRIBUTION_EPS;
        }

        if strictly_better || left_card_id < right_card_id {
            ContributionReplacementComparison {
                replaces: true,
                reason: ContributionReplacementReason::Score,
                margin: min_margin,
                failed_chart: min_margin_chart,
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
        let chart_count = self.charts.len();
        let mut allowed_indices = Vec::new();
        let mut complete_by_card = vec![false; self.cards.len()];
        let mut models_by_card_chart =
            vec![ContributionAffineModel::default(); self.cards.len() * chart_count];

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
                let Some(model) = signature_card_affine_model_for_chart(
                    card,
                    &self.profiles[idx],
                    signature,
                    chart_idx,
                    context,
                    &self.charts[chart_idx],
                ) else {
                    complete = false;
                    break;
                };
                models_by_card_chart[idx * chart_count + chart_idx] = model;
            }

            if complete {
                complete_by_card[idx] = true;
            }
        }

        SignatureContributionModels {
            allowed_indices,
            complete_by_card,
            models_by_card_chart,
            chart_count,
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
            if margin + SCORE_CONTRIBUTION_EPS < 0.0 {
                return false;
            }
            strictly_better |= margin > SCORE_CONTRIBUTION_EPS;
        }

        strictly_better || left.card_id < right.card_id
    }

    fn card_affine_model_for_signature(
        &mut self,
        card_idx: usize,
        signature: MedleyPruneSignature,
        chart_idx: usize,
    ) -> Option<ContributionAffineModel> {
        self.cards.get(card_idx)?;
        let context = self.signature_context_bounds(signature)?;
        signature_card_affine_model_for_chart(
            &self.cards[card_idx],
            &self.profiles[card_idx],
            signature,
            chart_idx,
            &context,
            &self.charts[chart_idx],
        )
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
        let stat_low =
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
        let mut captain_est_by_chart = Vec::with_capacity(self.charts.len());
        let mut team_stat_floor_by_chart = Vec::with_capacity(self.charts.len());
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
            let captain_est = self.signature_seed_captain_estimate(signature, chart_idx)?;
            normal_meta_low_by_chart.push(normal_low);
            normal_meta_high_by_chart.push(normal_high);
            captain_est_by_chart.push(captain_est);
            let team_meta_upper = self.signature_team_meta_upper_bound(signature, chart_idx)?;
            team_stat_floor_by_chart.push(signature_team_stat_floor(
                self.seed_score_floor_by_chart[chart_idx],
                team_meta_upper,
            ));
        }

        Some(SignatureContributionContextBounds {
            stat_low,
            stat_high,
            max_card_stat,
            normal_meta_low_by_chart,
            normal_meta_high_by_chart,
            captain_est_by_chart,
            team_stat_floor_by_chart,
        })
    }

    fn signature_seed_captain_estimate(
        &self,
        signature: MedleyPruneSignature,
        chart_idx: usize,
    ) -> Option<f64> {
        let mut candidates = Vec::new();
        for (idx, card) in self.cards.iter().enumerate() {
            if !signature.allows(card) {
                continue;
            }
            let normal = signature_card_normal_meta_estimate_for_chart(
                card,
                &self.profiles[idx],
                signature,
                chart_idx,
            )?;
            let captain = signature_card_captain_meta_for_chart(
                card,
                &self.profiles[idx],
                signature,
                chart_idx,
            )?;
            let value =
                self.profiles[idx].stat * (self.charts[chart_idx].meta.no_skill + normal + captain);
            candidates.push((idx, value, captain));
        }
        candidates.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| self.cards[left.0].card_id.cmp(&self.cards[right.0].card_id))
        });

        let mut characters = Vec::with_capacity(TEAM_SIZE);
        let mut captain_est = f64::NEG_INFINITY;
        for (idx, _, captain) in candidates {
            let character_id = self.cards[idx].character_id;
            if characters.contains(&character_id) {
                continue;
            }
            characters.push(character_id);
            captain_est = captain_est.max(captain);
            if characters.len() == TEAM_SIZE {
                break;
            }
        }

        (characters.len() == TEAM_SIZE && captain_est.is_finite()).then_some(captain_est)
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
        let normal_by_character = signature_normal_meta_high_by_character(
            self.cards,
            self.profiles,
            signature,
            chart_idx,
            excluded_character_id,
        );
        if normal_by_character.len() < team_size {
            return None;
        }

        let mut best = f64::NEG_INFINITY;
        for (idx, card) in self.cards.iter().enumerate() {
            if !signature.allows(card) || excluded_character_id == Some(card.character_id) {
                continue;
            }
            let normal = signature_card_normal_meta_bounds_for_chart(
                card,
                &self.profiles[idx],
                signature,
                chart_idx,
            )?
            .high;
            let captain = signature_card_captain_meta_for_chart(
                card,
                &self.profiles[idx],
                signature,
                chart_idx,
            )?;
            let other_normal = top_n_character_value_sum_excluding(
                &normal_by_character,
                team_size.saturating_sub(1),
                card.character_id,
            )?;
            best = best.max(normal + captain + other_normal);
        }

        best.is_finite().then_some(best)
    }
}

fn signature_team_stat_floor(seed_score_floor: f64, team_meta_upper: f64) -> f64 {
    if seed_score_floor <= 0.0 || team_meta_upper <= 0.0 {
        return 0.0;
    }

    seed_score_floor / team_meta_upper
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
    (0..charts.len())
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

fn signature_card_affine_model_for_chart(
    card: &PreparedCard,
    profile: &MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
    chart_idx: usize,
    context: &SignatureContributionContextBounds,
    chart: &Chart,
) -> Option<ContributionAffineModel> {
    let normal_meta =
        signature_card_normal_meta_estimate_for_chart(card, profile, signature, chart_idx)?;
    let captain_meta = signature_card_captain_meta_for_chart(card, profile, signature, chart_idx)?;
    let captain_est = context.captain_est_by_chart[chart_idx];
    let effective_meta = normal_meta + (captain_meta - captain_est).max(0.0);
    let stat = profile.stat;
    let y_low = context
        .stat_low
        .max(context.team_stat_floor_by_chart[chart_idx] - context.max_card_stat);
    let y_high = context.stat_high;
    if y_low > y_high + SCORE_CONTRIBUTION_EPS {
        return None;
    }
    let x_low = chart.meta.no_skill + context.normal_meta_low_by_chart[chart_idx] + captain_est;
    let x_high = chart.meta.no_skill + context.normal_meta_high_by_chart[chart_idx] + captain_est;

    Some(ContributionAffineModel {
        stat,
        meta: effective_meta,
        constant: (stat * effective_meta).round(),
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

fn signature_card_normal_meta_estimate_for_chart(
    card: &PreparedCard,
    profile: &MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
    chart_idx: usize,
) -> Option<f64> {
    let values = signature_skill_meta_values_for_chart(card, profile, signature, chart_idx)?;
    let normal_values = values.get(..TEAM_SIZE)?;
    Some(normal_values.iter().sum::<f64>() / TEAM_SIZE as f64)
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

fn signature_normal_meta_high_by_character(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    chart_idx: usize,
    excluded_character_id: Option<u32>,
) -> BTreeMap<u32, f64> {
    let mut result = BTreeMap::new();
    for (idx, card) in cards.iter().enumerate() {
        if !signature.allows(card) || excluded_character_id == Some(card.character_id) {
            continue;
        }
        let Some(normal) =
            signature_card_normal_meta_bounds_for_chart(card, &profiles[idx], signature, chart_idx)
        else {
            continue;
        };
        result
            .entry(card.character_id)
            .and_modify(|value: &mut f64| *value = value.max(normal.high))
            .or_insert(normal.high);
    }

    result
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
