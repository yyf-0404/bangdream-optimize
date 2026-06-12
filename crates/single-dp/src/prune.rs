use crate::{
    seed_single_incumbent_state, single_bound_from_indices, single_state_upper_bound, SingleDpCard,
    SingleDpInput, SingleDpTerm, SingleState, SCORE_EPSILON, TEAM_SIZE,
};
use bangdream_optimize_team_prune::{
    cross_group_dominator_cover, dominance_graph_for_items, same_group_dominator_cover,
};
use std::collections::BTreeSet;

const SINGLE_TEAM_COUNT: usize = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SinglePruneStats {
    pub(crate) raw_count: usize,
    pub(crate) active_count: usize,
    pub(crate) group_count: usize,
    pub(crate) same_group_pruned: usize,
    pub(crate) cross_group_pruned: usize,
    pub(crate) upper_bound_pruned: usize,
    pub(crate) max_same_group_cover: usize,
    pub(crate) max_cross_group_cover: usize,
    pub(crate) incumbent_present: bool,
}

pub(crate) fn prune_single_cards_with_stats(
    input: &SingleDpInput,
) -> (Vec<SingleDpCard>, SinglePruneStats) {
    let cards = &input.cards;
    let incumbent = seed_single_incumbent_state(cards, input)
        .map(|state| input.score_raw(state.sa, state.sb, state.c));
    let graph = dominance_graph_for_items(
        cards,
        |_, _| true,
        |_, dominator, _, target| single_card_dominates(dominator, target),
    );
    let mut stats = SinglePruneStats {
        raw_count: cards.len(),
        group_count: cards
            .iter()
            .map(|card| card.group_id)
            .collect::<BTreeSet<_>>()
            .len(),
        incumbent_present: incumbent.is_some(),
        ..SinglePruneStats::default()
    };
    let mut result = Vec::with_capacity(cards.len());

    for (idx, card) in cards.iter().enumerate() {
        let same_cover = same_group_dominator_cover(&graph, idx, cards, |card| card.group_id, 1);
        stats.max_same_group_cover = stats.max_same_group_cover.max(same_cover);
        if same_cover > 0 {
            stats.same_group_pruned += 1;
            continue;
        }

        let cross_cover = cross_group_dominator_cover(
            &graph,
            idx,
            cards,
            |card| card.group_id,
            TEAM_SIZE,
            SINGLE_TEAM_COUNT,
        );
        stats.max_cross_group_cover = stats.max_cross_group_cover.max(cross_cover);
        if cross_cover > 0 {
            stats.cross_group_pruned += 1;
            continue;
        }

        if incumbent.is_some_and(|incumbent| {
            forced_card_upper_bound(input, cards, idx) + SCORE_EPSILON < incumbent
        }) {
            stats.upper_bound_pruned += 1;
            continue;
        }

        result.push(card.clone());
    }

    stats.active_count = result.len();
    (result, stats)
}

pub(crate) fn trace_single_prune_stats(stats: &SinglePruneStats) {
    eprintln!(
        "single prune: raw_cards={} active_cards={} groups={} same_pruned={} cross_pruned={} bound_pruned={} max_same_cover={} max_cross_cover={} incumbent={}",
        stats.raw_count,
        stats.active_count,
        stats.group_count,
        stats.same_group_pruned,
        stats.cross_group_pruned,
        stats.upper_bound_pruned,
        stats.max_same_group_cover,
        stats.max_cross_group_cover,
        stats.incumbent_present,
    );
}

fn forced_card_upper_bound(
    input: &SingleDpInput,
    cards: &[SingleDpCard],
    target_idx: usize,
) -> f64 {
    let target = &cards[target_idx];
    let remaining_bound = single_bound_from_indices(
        cards,
        cards
            .iter()
            .enumerate()
            .filter_map(|(idx, card)| (card.group_id != target.group_id).then_some(idx)),
    );
    if remaining_bound.remaining_groups < TEAM_SIZE - 1 {
        return f64::NEG_INFINITY;
    }

    let mut best = f64::NEG_INFINITY;
    for position in 0..TEAM_SIZE {
        let normal = target.normal_terms[position];
        let mask = 1usize << position;
        let state = SingleState {
            sa: target.stat,
            sb: normal.sb,
            c: normal.c,
            positions: std::array::from_fn(|idx| if idx == position { target.card_id } else { 0 }),
            captain_card_id: 0,
        };

        best = best.max(single_state_upper_bound(
            input,
            &state,
            mask,
            false,
            &remaining_bound,
        ));

        let captain = target.captain_term;
        let captain_state = SingleState {
            sb: state.sb + captain.sb,
            c: state.c + captain.c,
            captain_card_id: target.card_id,
            ..state
        };
        best = best.max(single_state_upper_bound(
            input,
            &captain_state,
            mask,
            true,
            &remaining_bound,
        ));
    }

    best
}

fn single_card_dominates(left: &SingleDpCard, right: &SingleDpCard) -> bool {
    if left.stat < right.stat {
        return false;
    }
    let mut strictly_better = left.stat > right.stat;

    for position in 0..TEAM_SIZE {
        let left_term = left.normal_terms[position];
        let right_term = right.normal_terms[position];
        if !model_term_dominates(left_term, right_term) {
            return false;
        }
        strictly_better |= model_term_strictly_better(left_term, right_term);
    }
    if !model_term_dominates(left.captain_term, right.captain_term) {
        return false;
    }
    strictly_better |= model_term_strictly_better(left.captain_term, right.captain_term);

    strictly_better || left.card_id < right.card_id
}

fn model_term_dominates(left: SingleDpTerm, right: SingleDpTerm) -> bool {
    left.sb >= right.sb && left.c >= right.c
}

fn model_term_strictly_better(left: SingleDpTerm, right: SingleDpTerm) -> bool {
    left.sb > right.sb || left.c > right.c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prunes_dominated_same_group_card() {
        let cards = vec![
            card(1, 1, 1000, 1.0),
            card(2, 1, 1200, 1.0),
            card(3, 2, 1000, 1.0),
            card(4, 3, 1000, 1.0),
            card(5, 4, 1000, 1.0),
            card(6, 5, 1000, 1.0),
        ];

        let pruned = pruned_cards(cards);

        assert!(pruned.iter().any(|card| card.card_id == 2));
        assert!(!pruned.iter().any(|card| card.card_id == 1));
    }

    #[test]
    fn prunes_cross_group_card_when_one_replacement_survives_teammate_blocking() {
        let mut cards = vec![card(1, 99, 1000, 1.0)];
        for group_id in 1..=5 {
            cards.push(card(100 + group_id, group_id, 1200, 2.0));
        }

        let pruned = pruned_cards(cards);

        assert!(!pruned.iter().any(|card| card.card_id == 1));
    }

    #[test]
    fn keeps_cross_group_card_when_all_replacements_can_be_blocked_by_teammates() {
        let mut cards = vec![card(1, 99, 1000, 1.0)];
        for group_id in 1..=4 {
            cards.push(card(100 + group_id, group_id, 1200, 2.0));
        }

        let pruned = pruned_cards(cards);

        assert!(pruned.iter().any(|card| card.card_id == 1));
    }

    #[test]
    fn prunes_card_when_forced_upper_bound_cannot_reach_seed_incumbent() {
        let mut cards = (1..=5)
            .map(|card_id| card(card_id, card_id, 1000, 1.0))
            .collect::<Vec<_>>();
        cards.push(card(99, 99, 1, 2.0));

        let pruned = pruned_cards(cards);

        assert!(!pruned.iter().any(|card| card.card_id == 99));
    }

    #[test]
    fn reports_prune_reason_counts() {
        let mut cards = vec![
            card(1, 1, 1000, 1.0),
            card(2, 1, 1200, 1.0),
            card(99, 99, 1, 2.0),
        ];
        for group_id in 2..=6 {
            cards.push(card(100 + group_id, group_id, 1200, 2.0));
        }

        let (_, stats) = prune_single_cards_with_stats(&SingleDpInput {
            d: 10.0,
            c: 0.0,
            cards,
        });

        assert_eq!(stats.raw_count, 8);
        assert_eq!(stats.same_group_pruned, 1);
        assert!(stats.cross_group_pruned > 0 || stats.upper_bound_pruned > 0);
        assert!(stats.active_count < stats.raw_count);
    }

    fn card(card_id: u32, group_id: u32, stat: i32, weight: f64) -> SingleDpCard {
        SingleDpCard {
            card_id,
            group_id,
            stat,
            normal_terms: std::array::from_fn(|position| SingleDpTerm {
                sb: weight * (position as f64 + 1.0) * 0.01,
                c: weight * position as f64,
            }),
            captain_term: SingleDpTerm {
                sb: weight * 0.03,
                c: weight,
            },
        }
    }

    fn pruned_cards(cards: Vec<SingleDpCard>) -> Vec<SingleDpCard> {
        prune_single_cards_with_stats(&SingleDpInput {
            d: 10.0,
            c: 0.0,
            cards,
        })
        .0
    }
}
