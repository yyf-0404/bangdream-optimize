use super::contribution::{
    contribution_dominance_graph_for_signature, MedleyContributionDominance,
};
use super::hard::{
    cross_cover, hard_dominance_graph_for_indices, same_character_cover, MedleyCardPruneProfile,
    MedleyPruneUpperBounds,
};
use super::signature::{seed_signatures, signature_can_complete_with_card, MedleyPruneSignature};
use super::stats::{MedleyPruneTrace, SignaturePoolStats};
use crate::model::chart::Chart;
use crate::model::preparation::PreparedCard;
use crate::timing::Timer;
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
    let upper_start = Timer::start();
    let mut upper_bounds = MedleyPruneUpperBounds::new(cards, charts, profiles);
    let mut trace = MedleyPruneTrace {
        upper_bounds_init_ms: elapsed_ms(upper_start),
        ..MedleyPruneTrace::default()
    };
    let mut pools = Vec::new();
    let mut stats = Vec::new();

    let signatures_start = Timer::start();
    let signatures = seed_signatures(cards);
    trace.signatures_ms = elapsed_ms(signatures_start);
    trace.signature_count = signatures.len();

    for signature in signatures {
        let active_start = Timer::start();
        let (active_card_indices, mut signature_stats) = signature_active_card_indices(
            cards,
            charts,
            profiles,
            current_best,
            signature,
            &mut upper_bounds,
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
) -> (Vec<usize>, SignaturePoolStats) {
    let mut active = Vec::new();
    let mut stats = SignaturePoolStats {
        signature: Some(signature),
        ..SignaturePoolStats::default()
    };
    let contribution_context_start = Timer::start();
    let mut contribution_dominance =
        MedleyContributionDominance::new(cards, charts, profiles, current_best);
    stats.trace.contribution_context_ms += elapsed_ms(contribution_context_start);
    let allowed_indices = cards
        .iter()
        .enumerate()
        .filter_map(|(idx, card)| signature.allows(card).then_some(idx))
        .collect::<Vec<_>>();
    stats.allowed_count = allowed_indices.len();
    let hard_graph_start = Timer::start();
    let hard_dominance_graph =
        hard_dominance_graph_for_indices(cards, profiles, signature, &allowed_indices);
    stats.trace.hard_graph_ms += elapsed_ms(hard_graph_start);
    let mut contribution_dominance_graph = None;

    for &idx in &allowed_indices {
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

        let hard_cover_start = Timer::start();
        let same_cover = same_character_cover(&hard_dominance_graph, idx, cards);
        stats.max_same_character_cover = stats.max_same_character_cover.max(same_cover);
        if same_cover >= MEDLEY_TEAM_COUNT {
            stats.trace.hard_cover_ms += elapsed_ms(hard_cover_start);
            stats.same_character_pruned += 1;
            continue;
        }

        let hard_cross_cover = cross_cover(&hard_dominance_graph, idx, cards);
        stats.trace.hard_cover_ms += elapsed_ms(hard_cover_start);
        stats.max_cross_character_cover = stats.max_cross_character_cover.max(hard_cross_cover);
        if hard_cross_cover > 0 {
            stats.cross_character_pruned += 1;
            continue;
        }

        if contribution_dominance_graph.is_none() {
            let contribution_graph_start = Timer::start();
            contribution_dominance_graph = Some(contribution_dominance_graph_for_signature(
                cards,
                signature,
                &hard_dominance_graph,
                &mut contribution_dominance,
            ));
            stats.trace.contribution_graph_ms += elapsed_ms(contribution_graph_start);
            stats.trace.contribution_graph_count += 1;
        }
        let contribution_dominance_graph = contribution_dominance_graph
            .as_ref()
            .expect("contribution graph is initialized before use");
        let contribution_cover_start = Timer::start();
        let contribution_same_cover =
            same_character_cover(contribution_dominance_graph, idx, cards);
        stats.max_score_contribution_same_cover = stats
            .max_score_contribution_same_cover
            .max(contribution_same_cover);
        if contribution_same_cover >= MEDLEY_TEAM_COUNT {
            stats.trace.contribution_cover_ms += elapsed_ms(contribution_cover_start);
            stats.score_contribution_same_pruned += 1;
            continue;
        }

        let contribution_cross_cover = cross_cover(contribution_dominance_graph, idx, cards);
        stats.trace.contribution_cover_ms += elapsed_ms(contribution_cover_start);
        stats.max_score_contribution_cross_cover = stats
            .max_score_contribution_cross_cover
            .max(contribution_cross_cover);
        if contribution_cross_cover > 0 {
            stats.score_contribution_cross_pruned += 1;
            continue;
        }

        active.push(idx);
    }

    stats.active_count = active.len();
    (active, stats)
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

        signature_active_card_indices(cards, &charts, &profiles, 0, signature, &mut upper_bounds).0
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
}
