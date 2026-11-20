use super::contribution::{
    contribution_dominance_graph_for_cross_subsets, contribution_dominance_graph_for_models,
    MedleyContributionDominance, SignatureContributionModels,
};
use super::framework::{self, Graphs, Model, Phase};
use super::hard::{
    hard_dominance_graph_for_cross_subsets, hard_dominance_graph_for_indices_with_closure,
    MedleyCardPruneProfile, MedleyPruneUpperBounds,
};
use super::signature::{seed_signatures, signature_can_complete_with_card, MedleyPruneSignature};
use super::stats::{MedleyPruneTrace, SignaturePoolStats};
use crate::medley::team::medley_chart_item_score_upper_bound;
use crate::model::chart::{Chart, TeamCardSkill};
use crate::model::preparation::PreparedCard;
use crate::timing::Timer;
use bangdream_optimize_team_prune::DominanceGraph;
use std::collections::BTreeMap;
use std::convert::Infallible;

const TEAM_SIZE: usize = 5;

const MEDLEY_TEAM_COUNT: usize = 3;

#[derive(Clone, Copy)]
struct JointPointBonusPruneContext {
    teammate_bonus_bounds: [u64; 2],
    fixed_score_equivalent: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct SignatureCandidatePool {
    pub(crate) signature: MedleyPruneSignature,
    pub(crate) active_card_indices: Vec<usize>,
    pub(crate) estimated_candidates: usize,
}

pub(crate) fn signature_candidate_pools(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    current_best: i32,
) -> (
    Vec<SignatureCandidatePool>,
    Vec<SignaturePoolStats>,
    MedleyPruneTrace,
) {
    let trace_start = Timer::start();
    let signatures_start = Timer::start();
    let signatures = seed_signatures(cards);
    let signatures_ms = elapsed_ms(signatures_start);
    let upper_start = Timer::start();
    let best_any_team_scores = charts
        .iter()
        .enumerate()
        .map(|(chart_idx, chart)| {
            medley_chart_item_score_upper_bound(cards, profiles, chart, chart_idx, &signatures)
        })
        .collect::<Vec<_>>();
    let mut upper_bounds = MedleyPruneUpperBounds::with_best_any_team_scores(
        cards,
        charts,
        profiles,
        &best_any_team_scores,
    );
    let chart_eligibility_masks =
        upper_bounds.card_chart_eligibility_masks(&signatures, current_best);
    let mut trace = MedleyPruneTrace {
        upper_bounds_init_ms: elapsed_ms(upper_start),
        signatures_ms,
        signature_count: signatures.len(),
        ..MedleyPruneTrace::default()
    };
    let mut pools = Vec::new();
    let mut stats = Vec::new();

    for signature in signatures.iter().copied() {
        let active_start = Timer::start();
        let (active_card_indices, mut signature_stats) = signature_active_card_indices(
            cards,
            charts,
            profiles,
            current_best,
            signature,
            &mut upper_bounds,
            &best_any_team_scores,
            &chart_eligibility_masks,
            MEDLEY_TEAM_COUNT,
            None,
            0.0,
            None,
            None,
            false,
        );
        let active_ms = elapsed_ms(active_start);
        trace.active_indices_ms += active_ms;
        trace.add(&signature_stats.trace);
        let capacity_start = Timer::start();
        let groups = character_groups_for_raw_indices(cards, &active_card_indices);
        let estimated_candidates = candidate_capacity(&groups, usize::MAX);
        trace.capacity_ms += elapsed_ms(capacity_start);
        signature_stats.estimated_candidates = estimated_candidates;
        stats.push(signature_stats);

        if active_card_indices.len() >= TEAM_SIZE && groups.len() >= TEAM_SIZE {
            pools.push(SignatureCandidatePool {
                signature,
                active_card_indices,
                estimated_candidates,
            });
        }
    }

    pools.sort_by(|left, right| {
        left.estimated_candidates
            .cmp(&right.estimated_candidates)
            .then_with(|| {
                left.active_card_indices
                    .len()
                    .cmp(&right.active_card_indices.len())
            })
    });

    trace.context_ms = elapsed_ms(trace_start);
    (pools, stats, trace)
}

fn signature_active_card_indices(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    current_best: i32,
    signature: MedleyPruneSignature,
    upper_bounds: &mut MedleyPruneUpperBounds<'_>,
    best_any_team_scores: &[f64],
    chart_eligibility_masks: &[u8],
    team_count: usize,
    fixed_teammate_skills: Option<&[TeamCardSkill; 4]>,
    fixed_teammate_effective_stat: f64,
    joint_point_bonus: Option<JointPointBonusPruneContext>,
    replacement_values: Option<&[u64]>,
    preserve_stat: bool,
) -> (Vec<usize>, SignaturePoolStats) {
    let mut stats = SignaturePoolStats {
        signature: Some(signature),
        ..SignaturePoolStats::default()
    };
    let allowed_indices = cards
        .iter()
        .enumerate()
        .filter_map(|(idx, card)| signature.allows(card).then_some(idx))
        .collect::<Vec<_>>();
    stats.allowed_count = allowed_indices.len();

    let same_shape_start = Timer::start();
    let same_shape_indices = if let Some(teammate_skills) = fixed_teammate_skills {
        super::contribution::same_shape_contribution_active_indices_with_fixed_teammate_skills(
            cards,
            charts,
            profiles,
            signature,
            team_count,
            replacement_values,
            teammate_skills,
            fixed_teammate_effective_stat,
            joint_point_bonus.map(|context| context.teammate_bonus_bounds),
            joint_point_bonus
                .map(|context| context.fixed_score_equivalent)
                .unwrap_or_default(),
            preserve_stat,
        )
    } else if let (Some(context), Some(card_bonus_micros)) = (joint_point_bonus, replacement_values)
    {
        super::contribution::same_shape_contribution_active_indices_with_joint_point_bonus(
            cards,
            charts,
            profiles,
            signature,
            team_count,
            card_bonus_micros,
            context.teammate_bonus_bounds,
            context.fixed_score_equivalent,
            preserve_stat,
        )
    } else {
        super::contribution::same_shape_contribution_active_indices(
            cards,
            charts,
            profiles,
            signature,
            team_count,
            replacement_values,
            preserve_stat,
        )
    };
    stats.trace.same_shape_contribution_ms += elapsed_ms(same_shape_start);
    stats.score_contribution_same_pruned += allowed_indices
        .len()
        .saturating_sub(same_shape_indices.len());

    let mut eligible_indices = Vec::new();
    for idx in same_shape_indices {
        let completion_start = Timer::start();
        if !signature_can_complete_with_card(cards, idx, signature) {
            stats.trace.completion_ms += elapsed_ms(completion_start);
            continue;
        }
        stats.trace.completion_ms += elapsed_ms(completion_start);
        let upper_bound_start = Timer::start();
        if current_best > 0
            && charts.len() == MEDLEY_TEAM_COUNT
            && !upper_bounds.signature_can_beat_incumbent(idx, signature, current_best)
        {
            stats.trace.upper_bound_ms += elapsed_ms(upper_bound_start);
            stats.upper_bound_pruned += 1;
            continue;
        }
        stats.trace.upper_bound_ms += elapsed_ms(upper_bound_start);
        eligible_indices.push(idx);
    }

    let fixed_point_start = Timer::start();
    let (active, passes) = framework::fixed_point(eligible_indices, |active, phase| {
        let stage_cards: Vec<_> = active.iter().map(|&i| cards[i].clone()).collect();
        let stage_profiles: Vec<_> = active.iter().map(|&i| profiles[i].clone()).collect();
        let stage_masks: Vec<_> = active.iter().map(|&i| chart_eligibility_masks[i]).collect();
        let stage_values =
            replacement_values.map(|values| active.iter().map(|&i| values[i]).collect::<Vec<_>>());
        let local: Vec<_> = (0..stage_cards.len()).collect();
        let next = match phase {
            Phase::SameCharacter => divided_same_character_survivors(
                &stage_cards,
                charts,
                &stage_profiles,
                signature,
                current_best,
                &local,
                best_any_team_scores,
                fixed_teammate_skills,
                fixed_teammate_effective_stat,
                joint_point_bonus,
                stage_values.as_deref(),
                team_count,
                preserve_stat,
                &mut stats,
            ),
            Phase::CrossCharacter => divided_cross_character_survivors(
                &stage_cards,
                charts,
                &stage_profiles,
                signature,
                current_best,
                &local,
                best_any_team_scores,
                &stage_masks,
                fixed_teammate_skills,
                fixed_teammate_effective_stat,
                joint_point_bonus,
                stage_values.as_deref(),
                team_count,
                preserve_stat,
                &mut stats,
            ),
        };
        Ok::<_, Infallible>(next.into_iter().map(|i| active[i]).collect())
    })
    .unwrap();
    stats.fixed_point_passes += passes;
    stats.trace.fixed_point_ms += elapsed_ms(fixed_point_start);

    stats.active_count = active.len();
    (active, stats)
}

struct SubsetDominanceGraphs {
    hard_graph: DominanceGraph,
    contribution_graph: DominanceGraph,
    hard_graph_ms: f64,
    contribution_graph_ms: f64,
}

#[allow(clippy::too_many_arguments)]
fn subset_dominance_graphs(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    indices: &[usize],
    replacement_values: Option<&[u64]>,
    contribution: &MedleyContributionDominance<'_>,
    contribution_models: &SignatureContributionModels,
    close_transitively: bool,
) -> SubsetDominanceGraphs {
    let hard_start = Timer::start();
    let hard_graph = filter_replacement_value_edges(
        hard_dominance_graph_for_indices_with_closure(
            cards,
            profiles,
            signature,
            indices,
            close_transitively,
        ),
        replacement_values,
    );
    let hard_graph_ms = elapsed_ms(hard_start);
    let contribution_start = Timer::start();
    let contribution_graph = contribution_dominance_graph_for_models(
        cards,
        &hard_graph,
        contribution,
        contribution_models,
        indices,
        close_transitively,
    );
    let contribution_graph = if contribution.has_joint_point_bonus() {
        contribution_graph
    } else {
        filter_replacement_value_edges(contribution_graph, replacement_values)
    };
    let contribution_graph_ms = elapsed_ms(contribution_start);
    SubsetDominanceGraphs {
        hard_graph,
        contribution_graph,
        hard_graph_ms,
        contribution_graph_ms,
    }
}

fn add_subset_graph_trace(stats: &mut SignaturePoolStats, graphs: &SubsetDominanceGraphs) {
    stats.trace.hard_graph_ms += graphs.hard_graph_ms;
    stats.trace.contribution_graph_ms += graphs.contribution_graph_ms;
    stats.trace.contribution_graph_count += 1;
}

#[allow(clippy::too_many_arguments)]
fn divided_same_character_survivors(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    current_best: i32,
    indices: &[usize],
    best_any_team_scores: &[f64],
    fixed_teammate_skills: Option<&[TeamCardSkill; 4]>,
    fixed_teammate_effective_stat: f64,
    joint_point_bonus: Option<JointPointBonusPruneContext>,
    replacement_values: Option<&[u64]>,
    team_count: usize,
    preserve_stat: bool,
    stats: &mut SignaturePoolStats,
) -> Vec<usize> {
    let context_start = Timer::start();
    let mut contribution = MedleyContributionDominance::with_best_any_team_scores(
        cards,
        charts,
        profiles,
        current_best,
        best_any_team_scores,
    );
    if let Some(teammate_skills) = fixed_teammate_skills {
        contribution.set_fixed_teammate_context(teammate_skills, fixed_teammate_effective_stat);
    }
    if preserve_stat {
        contribution.require_non_decreasing_stat();
    }
    if let (Some(context), Some(card_bonus_micros)) = (joint_point_bonus, replacement_values) {
        contribution.set_joint_point_bonus_context(
            card_bonus_micros,
            context.teammate_bonus_bounds,
            context.fixed_score_equivalent,
        );
    }
    let models = contribution.models_for_signature(signature);
    stats.trace.contribution_context_ms += elapsed_ms(context_start);

    let mut adapter = LegacyStage {
        cards,
        profiles,
        signature,
        replacement_values,
        contribution: &contribution,
        models: &models,
        team_count,
        stats,
    };
    let (survivors, trace) =
        framework::same_character(cards, indices, team_count, &mut adapter).unwrap();
    stats.trace.hard_cover_ms += trace.cover_ms;
    survivors
}

#[allow(clippy::too_many_arguments)]
fn divided_cross_character_survivors(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    current_best: i32,
    indices: &[usize],
    best_any_team_scores: &[f64],
    chart_eligibility_masks: &[u8],
    fixed_teammate_skills: Option<&[TeamCardSkill; 4]>,
    fixed_teammate_effective_stat: f64,
    joint_point_bonus: Option<JointPointBonusPruneContext>,
    replacement_values: Option<&[u64]>,
    team_count: usize,
    preserve_stat: bool,
    stats: &mut SignaturePoolStats,
) -> Vec<usize> {
    let context_start = Timer::start();
    let mut contribution = MedleyContributionDominance::with_best_any_team_scores(
        cards,
        charts,
        profiles,
        current_best,
        best_any_team_scores,
    );
    if let Some(teammate_skills) = fixed_teammate_skills {
        contribution.set_fixed_teammate_context(teammate_skills, fixed_teammate_effective_stat);
    }
    if preserve_stat {
        contribution.require_non_decreasing_stat();
    }
    if let (Some(context), Some(card_bonus_micros)) = (joint_point_bonus, replacement_values) {
        contribution.set_joint_point_bonus_context(
            card_bonus_micros,
            context.teammate_bonus_bounds,
            context.fixed_score_equivalent,
        );
    }
    let models = contribution.models_for_signature(signature);
    stats.trace.contribution_context_ms += elapsed_ms(context_start);
    let mut adapter = LegacyStage {
        cards,
        profiles,
        signature,
        replacement_values,
        contribution: &contribution,
        models: &models,
        team_count,
        stats,
    };
    let (survivors, trace) = framework::cross_character(
        cards,
        signature,
        indices,
        chart_eligibility_masks,
        team_count,
        charts.len(),
        &mut adapter,
    )
    .unwrap();
    stats.trace.contribution_cover_ms += trace.cover_ms;
    stats.trace.hard_graph_ms += trace.hard_merge_ms;
    stats.trace.contribution_graph_ms += trace.contribution_merge_ms;
    survivors
}

struct LegacyStage<'a, 'b> {
    cards: &'a [PreparedCard],
    profiles: &'a [MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    replacement_values: Option<&'a [u64]>,
    contribution: &'a MedleyContributionDominance<'b>,
    models: &'a SignatureContributionModels,
    team_count: usize,
    stats: &'a mut SignaturePoolStats,
}

impl Model for LegacyStage<'_, '_> {
    type Error = Infallible;
    fn subset(&mut self, indices: &[usize], close: bool) -> Result<Graphs, Infallible> {
        let graphs = subset_dominance_graphs(
            self.cards,
            self.profiles,
            self.signature,
            indices,
            self.replacement_values,
            self.contribution,
            self.models,
            close,
        );
        add_subset_graph_trace(self.stats, &graphs);
        Ok(Graphs {
            hard: graphs.hard_graph,
            contribution: graphs.contribution_graph,
        })
    }
    fn cross(&mut self, left: &[usize], right: &[usize]) -> Result<Graphs, Infallible> {
        let start = Timer::start();
        let hard = filter_replacement_value_edges(
            hard_dominance_graph_for_cross_subsets(
                self.cards,
                self.profiles,
                self.signature,
                left,
                right,
            ),
            self.replacement_values,
        );
        self.stats.trace.hard_graph_ms += elapsed_ms(start);
        let start = Timer::start();
        let contribution = contribution_dominance_graph_for_cross_subsets(
            self.cards,
            self.contribution,
            self.models,
            left,
            right,
        );
        let contribution = if self.contribution.has_joint_point_bonus() {
            contribution
        } else {
            filter_replacement_value_edges(contribution, self.replacement_values)
        };
        self.stats.trace.contribution_graph_ms += elapsed_ms(start);
        self.stats.trace.contribution_graph_count += 1;
        Ok(Graphs { hard, contribution })
    }
    fn observe(
        &mut self,
        phase: Phase,
        _target: usize,
        _graphs: &Graphs,
        hard: usize,
        combined: usize,
    ) {
        let stats = &mut self.stats;
        match phase {
            Phase::SameCharacter => {
                stats.max_same_character_cover = stats.max_same_character_cover.max(hard);
                stats.max_score_contribution_same_cover =
                    stats.max_score_contribution_same_cover.max(combined);
                if combined >= self.team_count {
                    if hard >= self.team_count {
                        stats.same_character_pruned += 1;
                    } else {
                        stats.score_contribution_same_pruned += 1;
                    }
                }
            }
            Phase::CrossCharacter => {
                stats.max_cross_character_cover = stats.max_cross_character_cover.max(hard);
                stats.max_score_contribution_cross_cover =
                    stats.max_score_contribution_cross_cover.max(combined);
                if combined > 0 {
                    if hard > 0 {
                        stats.cross_character_pruned += 1;
                    } else {
                        stats.score_contribution_cross_pruned += 1;
                    }
                }
            }
        }
    }
}

pub(crate) fn single_team_active_card_indices(
    cards: &[PreparedCard],
    chart: &Chart,
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    replacement_values: Option<&[u64]>,
    preserve_stat: bool,
) -> Vec<usize> {
    single_team_active_card_indices_impl(
        cards,
        chart,
        profiles,
        signature,
        None,
        0.0,
        None,
        replacement_values,
        preserve_stat,
    )
    .0
}

pub(crate) fn single_team_active_card_indices_with_joint_point_bonus(
    cards: &[PreparedCard],
    chart: &Chart,
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    card_bonus_micros: &[u64],
    teammate_bonus_bounds: [u64; 2],
    fixed_score_equivalent: f64,
    preserve_stat: bool,
) -> Vec<usize> {
    single_team_active_card_indices_impl(
        cards,
        chart,
        profiles,
        signature,
        None,
        0.0,
        Some(JointPointBonusPruneContext {
            teammate_bonus_bounds,
            fixed_score_equivalent,
        }),
        Some(card_bonus_micros),
        preserve_stat,
    )
    .0
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn single_team_active_card_indices_with_fixed_teammate_skills_and_trace(
    cards: &[PreparedCard],
    chart: &Chart,
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    teammate_skills: &[TeamCardSkill; 4],
    teammate_effective_stat: f64,
    teammate_bonus_bounds: Option<[u64; 2]>,
    fixed_score_equivalent: f64,
    replacement_values: Option<&[u64]>,
    preserve_stat: bool,
) -> (Vec<usize>, MedleyPruneTrace) {
    single_team_active_card_indices_impl(
        cards,
        chart,
        profiles,
        signature,
        Some(teammate_skills),
        teammate_effective_stat,
        teammate_bonus_bounds.map(|teammate_bonus_bounds| JointPointBonusPruneContext {
            teammate_bonus_bounds,
            fixed_score_equivalent,
        }),
        replacement_values,
        preserve_stat,
    )
}

fn single_team_active_card_indices_impl(
    cards: &[PreparedCard],
    chart: &Chart,
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    fixed_teammate_skills: Option<&[TeamCardSkill; 4]>,
    fixed_teammate_effective_stat: f64,
    joint_point_bonus: Option<JointPointBonusPruneContext>,
    replacement_values: Option<&[u64]>,
    preserve_stat: bool,
) -> (Vec<usize>, MedleyPruneTrace) {
    let active_start = Timer::start();
    let charts = std::slice::from_ref(chart);
    let mut upper_bounds = MedleyPruneUpperBounds::new(cards, charts, profiles);
    let contribution_score_upper_bounds = vec![0.0; charts.len()];
    let chart_eligibility_masks = vec![1_u8; cards.len()];
    let (active, stats) = signature_active_card_indices(
        cards,
        charts,
        profiles,
        0,
        signature,
        &mut upper_bounds,
        &contribution_score_upper_bounds,
        &chart_eligibility_masks,
        1,
        fixed_teammate_skills,
        fixed_teammate_effective_stat,
        joint_point_bonus,
        replacement_values,
        preserve_stat,
    );
    let mut trace = stats.trace;
    trace.active_indices_ms = elapsed_ms(active_start);
    (active, trace)
}

fn filter_replacement_value_edges(
    graph: DominanceGraph,
    replacement_values: Option<&[u64]>,
) -> DominanceGraph {
    let Some(values) = replacement_values else {
        return graph;
    };
    let mut filtered = DominanceGraph::new(values.len());
    for target_idx in 0..values.len() {
        for &dominator_idx in graph.incoming(target_idx) {
            if values[dominator_idx] >= values[target_idx] {
                filtered.add_edge(dominator_idx, target_idx);
            }
        }
    }
    filtered
}

fn elapsed_ms(start: Timer) -> f64 {
    start.elapsed_ms()
}

fn candidate_capacity(groups: &[Vec<usize>], max_candidates: usize) -> usize {
    if groups.len() < TEAM_SIZE {
        return 0;
    }

    let mut capacity = 0usize;
    accumulate_candidate_capacity(groups, 0, 0, 1, max_candidates, &mut capacity);
    capacity.min(max_candidates)
}

fn accumulate_candidate_capacity(
    groups: &[Vec<usize>],
    group_start: usize,
    selected_groups: usize,
    current_product: usize,
    max_candidates: usize,
    capacity: &mut usize,
) {
    if *capacity >= max_candidates {
        return;
    }
    if selected_groups == TEAM_SIZE {
        *capacity = capacity.saturating_add(current_product).min(max_candidates);
        return;
    }

    let remaining_slots = TEAM_SIZE - selected_groups;
    let end = groups.len().saturating_sub(remaining_slots) + 1;
    for group_idx in group_start..end {
        let next_product = current_product.saturating_mul(groups[group_idx].len());
        accumulate_candidate_capacity(
            groups,
            group_idx + 1,
            selected_groups + 1,
            next_product,
            max_candidates,
            capacity,
        );
    }
}

fn character_groups_for_raw_indices(
    cards: &[PreparedCard],
    raw_indices: &[usize],
) -> Vec<Vec<usize>> {
    let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for &raw_idx in raw_indices {
        groups
            .entry(cards[raw_idx].character_id)
            .or_default()
            .push(raw_idx);
    }

    groups.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::super::hard::medley_card_prune_profiles;
    use super::*;
    use crate::medley::team::adjusted_card_stats;
    use crate::medley::test_support::{medley_charts, prepared_card, selected_cool_items};
    use crate::model::chart::{ChartNode, ChartNodeType};
    use crate::model::preparation::{AreaItemPercent, ScoreUp, StatValue};
    use crate::model::schema::Attribute;

    fn active_indices_for_signature(
        cards: &[PreparedCard],
        signature: MedleyPruneSignature,
    ) -> Vec<usize> {
        let charts = medley_charts();
        let card_stats =
            adjusted_card_stats(cards, &AreaItemPercent::empty(), &selected_cool_items());
        let profiles = medley_card_prune_profiles(cards, &charts, &card_stats).unwrap();
        let mut upper_bounds = MedleyPruneUpperBounds::new(cards, &charts, &profiles);
        let contribution_score_upper_bounds = vec![0.0; charts.len()];
        let chart_eligibility_masks = vec![(1_u8 << charts.len()) - 1; cards.len()];

        signature_active_card_indices(
            cards,
            &charts,
            &profiles,
            0,
            signature,
            &mut upper_bounds,
            &contribution_score_upper_bounds,
            &chart_eligibility_masks,
            MEDLEY_TEAM_COUNT,
            None,
            0.0,
            None,
            None,
            false,
        )
        .0
    }

    fn single_active_indices_for_signature(
        cards: &[PreparedCard],
        signature: MedleyPruneSignature,
    ) -> Vec<usize> {
        let chart = medley_charts().remove(0);
        let card_stats =
            adjusted_card_stats(cards, &AreaItemPercent::empty(), &selected_cool_items());
        let profiles =
            medley_card_prune_profiles(cards, std::slice::from_ref(&chart), &card_stats).unwrap();
        single_team_active_card_indices(cards, &chart, &profiles, signature, None, false)
    }

    fn strong_card(card_id: u32, character_id: u32, attribute: Attribute) -> PreparedCard {
        let mut card = prepared_card(card_id, character_id, 1, attribute);
        card.stat = StatValue {
            performance: 2000.0,
            technique: 2000.0,
            visual: 2000.0,
        };
        card.score_up = ScoreUp {
            default: 2.0,
            unification_activate_effect_value: None,
            unification_activate_condition_band_id: None,
            unification_activate_condition_type: None,
        };
        card
    }

    #[test]
    fn preprune_compares_skill_shape_by_chart_meta_points() {
        let mut cards = Vec::new();
        for idx in 0..3 {
            let mut card = prepared_card(idx + 1, 1, 1, Attribute::Cool);
            card.skill.duration = 7.0;
            cards.push(card);
        }
        let mut short_duration = prepared_card(100, 1, 1, Attribute::Cool);
        short_duration.skill.duration = 5.0;
        cards.push(short_duration);
        for character_id in 2..=5 {
            cards.push(prepared_card(
                1000 + character_id,
                character_id,
                1,
                Attribute::Cool,
            ));
        }

        let active = active_indices_for_signature(&cards, MedleyPruneSignature::Mixed);

        assert!(active.contains(&0));
        assert!(active.contains(&1));
        assert!(active.contains(&2));
        assert!(!active.contains(&3));
    }

    #[test]
    fn preprune_uses_unification_skill_high_meta_as_upper_bound() {
        let mut cards = Vec::new();
        for idx in 0..3 {
            cards.push(prepared_card(idx + 1, 1, 1, Attribute::Cool));
        }
        let mut unification_card = prepared_card(100, 1, 1, Attribute::Cool);
        unification_card.score_up = ScoreUp {
            default: 0.5,
            unification_activate_effect_value: Some(2.0),
            unification_activate_condition_band_id: None,
            unification_activate_condition_type: Some(Attribute::Cool),
        };
        cards.push(unification_card);
        for character_id in 2..=5 {
            cards.push(prepared_card(
                1000 + character_id,
                character_id,
                1,
                Attribute::Cool,
            ));
        }

        let active = active_indices_for_signature(
            &cards,
            MedleyPruneSignature::UnifiedAttribute(Attribute::Cool),
        );

        assert!(active.contains(&3));
    }

    #[test]
    fn preprune_keeps_attribute_that_can_preserve_unified_team_context() {
        let mut cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool),
            prepared_card(2, 2, 1, Attribute::Cool),
            prepared_card(3, 3, 1, Attribute::Cool),
            prepared_card(4, 4, 1, Attribute::Cool),
            prepared_card(5, 5, 1, Attribute::Cool),
        ];
        cards[1].score_up = ScoreUp {
            default: 1.0,
            unification_activate_effect_value: Some(3.0),
            unification_activate_condition_band_id: None,
            unification_activate_condition_type: Some(Attribute::Cool),
        };
        for idx in 0..3 {
            cards.push(strong_card(100 + idx, 1, Attribute::Happy));
        }

        let active = active_indices_for_signature(
            &cards,
            MedleyPruneSignature::UnifiedAttribute(Attribute::Cool),
        );

        assert!(active.contains(&0));
    }

    #[test]
    fn preprune_drops_card_with_full_medley_cross_character_dominator_cover() {
        let mut cards = vec![prepared_card(1, 99, 1, Attribute::Cool)];
        for character_id in 1..=15 {
            cards.push(strong_card(
                character_id * 100,
                character_id,
                Attribute::Cool,
            ));
        }

        let active = active_indices_for_signature(&cards, MedleyPruneSignature::Mixed);

        assert!(!active.contains(&0));
    }

    #[test]
    fn preprune_keeps_card_when_cross_character_cover_can_be_lost_to_teammate_roles() {
        let mut cards = vec![prepared_card(1, 99, 1, Attribute::Cool)];
        for character_id in 1..=5 {
            for copy_idx in 0..2 {
                cards.push(strong_card(
                    character_id * 100 + copy_idx,
                    character_id,
                    Attribute::Cool,
                ));
            }
        }

        let active = active_indices_for_signature(&cards, MedleyPruneSignature::Mixed);

        assert!(active.contains(&0));
    }

    #[test]
    fn single_team_prunes_after_one_same_character_dominator() {
        let mut cards = vec![prepared_card(1, 1, 1, Attribute::Cool)];
        cards.push(strong_card(2, 1, Attribute::Cool));
        for character_id in 2..=5 {
            cards.push(prepared_card(
                100 + character_id,
                character_id,
                1,
                Attribute::Cool,
            ));
        }

        let active = single_active_indices_for_signature(&cards, MedleyPruneSignature::Mixed);

        assert!(!active.contains(&0));
    }

    #[test]
    fn single_team_prunes_with_cross_character_cover_for_one_team() {
        let mut cards = vec![prepared_card(1, 99, 1, Attribute::Cool)];
        for character_id in 1..=5 {
            cards.push(strong_card(
                character_id * 100,
                character_id,
                Attribute::Cool,
            ));
        }

        let active = single_active_indices_for_signature(&cards, MedleyPruneSignature::Mixed);

        assert!(!active.contains(&0));
    }

    #[test]
    fn single_team_divided_graph_prunes_large_cross_character_pool() {
        let cards = (1..=160)
            .map(|id| prepared_card(id, id, 1, Attribute::Cool))
            .collect::<Vec<_>>();

        let active = single_active_indices_for_signature(&cards, MedleyPruneSignature::Mixed);
        let active_card_ids = active
            .into_iter()
            .map(|idx| cards[idx].card_id)
            .collect::<Vec<_>>();

        assert_eq!(active_card_ids, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn potential_long_skill_warning_keeps_pruning_when_current_skills_do_not_queue() {
        let mut cards = vec![prepared_card(1, 1, 1, Attribute::Cool)];
        for idx in 0..3 {
            cards.push(strong_card(10 + idx, 1, Attribute::Cool));
        }
        for character_id in 2..=5 {
            cards.push(prepared_card(
                100 + character_id,
                character_id,
                1,
                Attribute::Cool,
            ));
        }

        let nodes = (0..6)
            .flat_map(|idx| {
                [
                    ChartNode {
                        node_type: ChartNodeType::Skill,
                        time: idx as f64 * 8.0,
                    },
                    ChartNode {
                        node_type: ChartNodeType::Node,
                        time: idx as f64 * 8.0 + 1.0,
                    },
                ]
            })
            .collect();
        let mut chart = Chart::new(5, nodes);
        chart.init(0, false).unwrap();
        assert!(!chart.warning.is_empty());
        let charts = vec![chart.clone(), chart.clone(), chart];
        let card_stats =
            adjusted_card_stats(&cards, &AreaItemPercent::empty(), &selected_cool_items());
        let profiles = medley_card_prune_profiles(&cards, &charts, &card_stats).unwrap();
        let mut upper_bounds = MedleyPruneUpperBounds::new(&cards, &charts, &profiles);
        let contribution_score_upper_bounds = vec![0.0; charts.len()];
        let chart_eligibility_masks = vec![(1_u8 << charts.len()) - 1; cards.len()];

        let active = signature_active_card_indices(
            &cards,
            &charts,
            &profiles,
            i32::MAX,
            MedleyPruneSignature::Mixed,
            &mut upper_bounds,
            &contribution_score_upper_bounds,
            &chart_eligibility_masks,
            MEDLEY_TEAM_COUNT,
            None,
            0.0,
            None,
            None,
            false,
        )
        .0;

        assert!(!active.contains(&0));
    }
}
