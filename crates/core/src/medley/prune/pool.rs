use super::contribution::{
    contribution_dominance_graph_for_signature, MedleyContributionDominance,
};
use super::hard::{
    cross_cover, hard_dominance_graph_for_indices, same_character_cover, MedleyCardPruneProfile,
    MedleyPruneUpperBounds,
};
use super::signature::{seed_signatures, signature_can_complete_with_card, MedleyPruneSignature};
use super::stats::{MedleyPruneTrace, SignaturePoolStats};
use crate::medley::team::medley_chart_item_score_upper_bound;
use crate::model::chart::Chart;
use crate::model::preparation::PreparedCard;
use crate::timing::Timer;
use bangdream_optimize_team_prune::DominanceGraph;
use std::collections::BTreeMap;

const TEAM_SIZE: usize = 5;
const MEDLEY_TEAM_COUNT: usize = 3;

#[derive(Debug, Clone)]
pub(in crate::medley) struct SignatureCandidatePool {
    pub(in crate::medley) signature: MedleyPruneSignature,
    pub(in crate::medley) active_card_indices: Vec<usize>,
    pub(in crate::medley) estimated_candidates: usize,
}

pub(in crate::medley) fn signature_candidate_pools(
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

    let same_shape_indices = super::contribution::same_shape_contribution_active_indices(
        cards, charts, profiles, signature, team_count,
    );
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

    let mut active = eligible_indices;
    loop {
        stats.fixed_point_passes += 1;
        let pass_input_count = active.len();
        let same_stage = staged_dominance_graphs(
            cards,
            charts,
            profiles,
            signature,
            current_best,
            &active,
            best_any_team_scores,
            chart_eligibility_masks,
        );
        stats.trace.hard_graph_ms += same_stage.hard_graph_ms;
        stats.trace.contribution_context_ms += same_stage.contribution_context_ms;
        stats.trace.contribution_graph_ms += same_stage.contribution_graph_ms;
        stats.trace.contribution_graph_count += 1;
        let mut same_survivors = Vec::new();
        for (local_idx, &raw_idx) in active.iter().enumerate() {
            let cover_start = Timer::start();
            let hard_cover = same_character_cover(
                &same_stage.hard_graph,
                local_idx,
                &same_stage.cards,
                team_count,
            );
            let combined_cover = same_character_cover(
                &same_stage.contribution_graph,
                local_idx,
                &same_stage.cards,
                team_count,
            );
            stats.trace.hard_cover_ms += elapsed_ms(cover_start);
            stats.max_same_character_cover = stats.max_same_character_cover.max(hard_cover);
            stats.max_score_contribution_same_cover =
                stats.max_score_contribution_same_cover.max(combined_cover);
            if combined_cover >= team_count {
                if hard_cover >= team_count {
                    stats.same_character_pruned += 1;
                } else {
                    stats.score_contribution_same_pruned += 1;
                }
            } else {
                same_survivors.push(raw_idx);
            }
        }

        let cross_stage = staged_dominance_graphs(
            cards,
            charts,
            profiles,
            signature,
            current_best,
            &same_survivors,
            best_any_team_scores,
            chart_eligibility_masks,
        );
        stats.trace.hard_graph_ms += cross_stage.hard_graph_ms;
        stats.trace.contribution_context_ms += cross_stage.contribution_context_ms;
        stats.trace.contribution_graph_ms += cross_stage.contribution_graph_ms;
        stats.trace.contribution_graph_count += 1;
        let mut next_active = Vec::new();
        for (local_idx, &raw_idx) in same_survivors.iter().enumerate() {
            let cover_start = Timer::start();
            let hard_cover = cross_cover(
                &cross_stage.hard_graph,
                local_idx,
                &cross_stage.cards,
                signature,
                team_count,
                &cross_stage.chart_eligibility_masks,
                charts.len(),
            );
            let combined_cover = cross_cover(
                &cross_stage.contribution_graph,
                local_idx,
                &cross_stage.cards,
                signature,
                team_count,
                &cross_stage.chart_eligibility_masks,
                charts.len(),
            );
            stats.trace.contribution_cover_ms += elapsed_ms(cover_start);
            stats.max_cross_character_cover = stats.max_cross_character_cover.max(hard_cover);
            stats.max_score_contribution_cross_cover =
                stats.max_score_contribution_cross_cover.max(combined_cover);
            if combined_cover > 0 {
                if hard_cover > 0 {
                    stats.cross_character_pruned += 1;
                } else {
                    stats.score_contribution_cross_pruned += 1;
                }
            } else {
                next_active.push(raw_idx);
            }
        }

        let reached_fixed_point = next_active.len() == pass_input_count;
        active = next_active;
        if reached_fixed_point {
            break;
        }
    }

    stats.active_count = active.len();
    (active, stats)
}

struct StagedDominanceGraphs {
    cards: Vec<PreparedCard>,
    chart_eligibility_masks: Vec<u8>,
    hard_graph: DominanceGraph,
    contribution_graph: DominanceGraph,
    hard_graph_ms: f64,
    contribution_context_ms: f64,
    contribution_graph_ms: f64,
}

fn staged_dominance_graphs(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    current_best: i32,
    indices: &[usize],
    best_any_team_scores: &[f64],
    chart_eligibility_masks: &[u8],
) -> StagedDominanceGraphs {
    let stage_cards = indices
        .iter()
        .map(|&idx| cards[idx].clone())
        .collect::<Vec<_>>();
    let stage_profiles = indices
        .iter()
        .map(|&idx| profiles[idx].clone())
        .collect::<Vec<_>>();
    let stage_chart_eligibility_masks = indices
        .iter()
        .map(|&idx| chart_eligibility_masks[idx])
        .collect::<Vec<_>>();
    let local_indices = (0..stage_cards.len()).collect::<Vec<_>>();
    let hard_start = Timer::start();
    let hard_graph =
        hard_dominance_graph_for_indices(&stage_cards, &stage_profiles, signature, &local_indices);
    let hard_graph_ms = elapsed_ms(hard_start);
    let context_start = Timer::start();
    let mut contribution = MedleyContributionDominance::with_best_any_team_scores(
        &stage_cards,
        charts,
        &stage_profiles,
        current_best,
        best_any_team_scores,
    );
    let contribution_context_ms = elapsed_ms(context_start);
    let contribution_start = Timer::start();
    let contribution_graph = contribution_dominance_graph_for_signature(
        &stage_cards,
        signature,
        &hard_graph,
        &mut contribution,
    );
    let contribution_graph_ms = elapsed_ms(contribution_start);
    StagedDominanceGraphs {
        cards: stage_cards,
        chart_eligibility_masks: stage_chart_eligibility_masks,
        hard_graph,
        contribution_graph,
        hard_graph_ms,
        contribution_context_ms,
        contribution_graph_ms,
    }
}

pub(in crate::medley) fn single_team_active_card_indices(
    cards: &[PreparedCard],
    chart: &Chart,
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
) -> Vec<usize> {
    let charts = std::slice::from_ref(chart);
    let mut upper_bounds = MedleyPruneUpperBounds::new(cards, charts, profiles);
    let contribution_score_upper_bounds = vec![0.0; charts.len()];
    let chart_eligibility_masks = vec![1_u8; cards.len()];
    signature_active_card_indices(
        cards,
        charts,
        profiles,
        0,
        signature,
        &mut upper_bounds,
        &contribution_score_upper_bounds,
        &chart_eligibility_masks,
        1,
    )
    .0
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
        single_team_active_card_indices(cards, &chart, &profiles, signature)
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
    fn overlap_warning_keeps_fast_additive_meta_pruning() {
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
        )
        .0;

        assert!(!active.contains(&0));
    }
}
