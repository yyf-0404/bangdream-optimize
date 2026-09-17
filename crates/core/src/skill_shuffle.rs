//! Five-member native shuffle observed in CN 9.4.2. This models a player's
//! display slots, not the order of five different players in cooperative live.
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::model::chart::{ExactScoreScratch, IndependentSkillScoreMatrix};
use crate::{Chart, ChartError, TeamCardSkill};

#[path = "skill_shuffle_weights.rs"]
mod weights;
use weights::NATIVE_ORDERS;

pub const SHUFFLE_PATH_COUNT: u64 = 1024;
pub(crate) const ORDER_COUNT: usize = 120;
pub(crate) const CAPTAIN_SLOT: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaxScoreTeamOrder {
    pub recommended_team_card_ids: [u32; 5],
    /// Only the first five activations; the sixth remains the captain's skill.
    pub skill_order_card_ids: [u32; 5],
    pub skill_order_path_count: u16,
    /// Sum of ALL possible orders reaching the maximum, including score ties.
    pub max_score_path_count: u16,
    pub optimal_order_count: u8,
    pub total_path_count: u16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WeightedOrder {
    pub index: usize,
    pub count: u16,
}

#[derive(Debug)]
pub(crate) struct DisplayLayout {
    pub cards: [usize; 5],
    pub weighted_orders: Vec<WeightedOrder>,
}

#[derive(Debug)]
pub(crate) struct ShuffleTables {
    pub orders: Vec<[usize; 5]>,
    #[cfg(test)]
    pub path_counts: [u16; ORDER_COUNT],
    pub layouts: Vec<DisplayLayout>,
    /// Marginals are for linear mean-score bounds ONLY, never joint PT odds.
    pub slot_activation_counts: [[u16; 5]; 5],
}

pub(crate) fn tables() -> &'static ShuffleTables {
    static TABLES: OnceLock<ShuffleTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut orders = Vec::with_capacity(ORDER_COUNT);
        for a in 0..5 {
            for b in 0..5 {
                for c in 0..5 {
                    for d in 0..5 {
                        for e in 0..5 {
                            let order = [a, b, c, d, e];
                            if order.iter().fold(0u8, |mask, &x| mask | (1 << x)) == 31 {
                                orders.push(order);
                            }
                        }
                    }
                }
            }
        }
        let mut path_counts = [0; ORDER_COUNT];
        for (order, count) in NATIVE_ORDERS {
            path_counts[order_index(order)] = count;
        }
        let mut slot_activation_counts = [[0; 5]; 5];
        for (order, &count) in orders.iter().zip(&path_counts) {
            for (activation, &slot) in order.iter().enumerate() {
                slot_activation_counts[slot][activation] += count;
            }
        }
        let layouts = orders
            .iter()
            .map(|&cards| DisplayLayout {
                cards,
                weighted_orders: orders
                    .iter()
                    .zip(&path_counts)
                    .filter_map(|(order, &count)| {
                        (count > 0).then(|| WeightedOrder {
                            index: order_index(order.map(|slot| cards[slot])),
                            count,
                        })
                    })
                    .collect(),
            })
            .collect();
        ShuffleTables {
            orders,
            #[cfg(test)]
            path_counts,
            layouts,
            slot_activation_counts,
        }
    })
}

pub(crate) fn order_index(order: [usize; 5]) -> usize {
    let factors = [24, 6, 2, 1, 1];
    (0..5)
        .map(|i| factors[i] * order[i + 1..].iter().filter(|&&x| x < order[i]).count())
        .sum()
}

pub(crate) type OrderScores = [[i32; ORDER_COUNT]; 5];

pub(crate) fn matrix_order_scores(matrix: &IndependentSkillScoreMatrix) -> OrderScores {
    let first_five: [i32; ORDER_COUNT] = std::array::from_fn(|index| {
        tables().orders[index]
            .iter()
            .enumerate()
            .map(|(position, &card)| matrix.deltas[card][position])
            .sum()
    });
    std::array::from_fn(|captain| {
        std::array::from_fn(|index| {
            matrix.base_score + first_five[index] + matrix.deltas[captain][5]
        })
    })
}

pub(crate) fn matrix_best_mean_numerator(matrix: &IndependentSkillScoreMatrix) -> i64 {
    let marginal = &tables().slot_activation_counts;
    let values: [[i64; 5]; 5] = std::array::from_fn(|slot| {
        std::array::from_fn(|card| {
            (0..5)
                .map(|activation| {
                    i64::from(marginal[slot][activation])
                        * i64::from(matrix.deltas[card][activation])
                })
                .sum::<i64>()
                + if slot == CAPTAIN_SLOT {
                    SHUFFLE_PATH_COUNT as i64 * i64::from(matrix.deltas[card][5])
                } else {
                    0
                }
        })
    });
    let mut best = [i64::MIN; 32];
    best[0] = 0;
    for mask in 0usize..31 {
        let slot = mask.count_ones() as usize;
        for card in 0..5 {
            if mask & (1 << card) == 0 {
                best[mask | (1 << card)] =
                    best[mask | (1 << card)].max(best[mask] + values[slot][card]);
            }
        }
    }
    SHUFFLE_PATH_COUNT as i64 * i64::from(matrix.base_score) + best[31]
}

pub(crate) fn exact_order_scores(
    chart: &Chart,
    skills: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
) -> Result<OrderScores, ChartError> {
    exact_order_scores_with_scratch(
        chart,
        skills,
        stat,
        is_medley,
        &mut ExactScoreScratch::default(),
    )
}

pub(crate) fn exact_order_scores_with_scratch(
    chart: &Chart,
    skills: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
    scratch: &mut ExactScoreScratch,
) -> Result<OrderScores, ChartError> {
    if let Some(matrix) = chart.independent_skill_score_matrix(skills, stat, is_medley, scratch)? {
        return Ok(matrix_order_scores(&matrix));
    }
    if let Some(matrix) = chart.predecessor_skill_score_matrix(skills, stat, is_medley, scratch)? {
        return Ok(std::array::from_fn(|captain| {
            std::array::from_fn(|index| matrix.score(tables().orders[index], captain))
        }));
    }
    if let Some(matrix) = chart.state_skill_scores(skills, stat, is_medley, scratch)? {
        return Ok(std::array::from_fn(|captain| {
            std::array::from_fn(|index| matrix.score(tables().orders[index], captain))
        }));
    }
    let mut scores = [[0; ORDER_COUNT]; 5];
    for (captain, values) in scores.iter_mut().enumerate() {
        for (index, order) in tables().orders.iter().enumerate() {
            let activations =
                std::array::from_fn(|i| skills[if i == 5 { captain } else { order[i] }]);
            values[index] = chart.get_score_for_six_skills(&activations, stat, is_medley)?;
        }
    }
    Ok(scores)
}

pub(crate) fn best_weighted_mean_numerator(scores: &OrderScores) -> i64 {
    tables()
        .layouts
        .iter()
        .map(|layout| {
            let row = &scores[layout.cards[CAPTAIN_SLOT]];
            layout
                .weighted_orders
                .iter()
                .map(|order| i64::from(row[order.index]) * i64::from(order.count))
                .sum()
        })
        .max()
        .expect("there are 120 layouts")
}

/// Specified teams keep both their display slots and their captain. Enumerate
/// the actual shuffled orders with the full scheduler, including chained queues.
pub(crate) fn fixed_captain_order_scores(
    chart: &Chart,
    skills: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
    captain: usize,
) -> Result<[i32; ORDER_COUNT], ChartError> {
    let mut scores = [0; ORDER_COUNT];
    for (index, order) in tables().orders.iter().enumerate() {
        let activations = std::array::from_fn(|i| skills[if i == 5 { captain } else { order[i] }]);
        scores[index] = chart.get_score_for_six_skills(&activations, stat, is_medley)?;
    }
    Ok(scores)
}

pub(crate) fn recommend_max_score(
    chart: &Chart,
    skills: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
    captain: usize,
) -> Result<MaxScoreTeamOrder, ChartError> {
    let scores = exact_order_scores(chart, skills, stat, is_medley)?;
    Ok(recommend_max_score_from_scores(
        skills.map(|card| card.card_id),
        &scores[captain],
        captain,
    ))
}

fn recommend_max_score_from_scores(
    ids: [u32; 5],
    scores: &[i32; ORDER_COUNT],
    captain: usize,
) -> MaxScoreTeamOrder {
    let max_score = *scores.iter().max().unwrap();
    let mut best = None;
    for layout in tables()
        .layouts
        .iter()
        .filter(|layout| layout.cards[CAPTAIN_SLOT] == captain)
    {
        let mut max_paths = 0;
        let mut optimal_count = 0;
        let mut best_order = None;
        let mut score_sum = 0i64;
        for weighted in &layout.weighted_orders {
            score_sum += i64::from(scores[weighted.index]) * i64::from(weighted.count);
            if scores[weighted.index] != max_score {
                continue;
            }
            max_paths += weighted.count;
            optimal_count += 1;
            let order_ids = tables().orders[weighted.index].map(|card| ids[card]);
            let key = (weighted.count, std::cmp::Reverse(order_ids));
            if best_order.is_none_or(|current| key > current) {
                best_order = Some(key);
            }
        }
        let Some((order_paths, std::cmp::Reverse(order_ids))) = best_order else {
            continue;
        };
        let layout_ids = layout.cards.map(|card| ids[card]);
        let key = (
            max_paths,
            order_paths,
            score_sum,
            std::cmp::Reverse(layout_ids),
        );
        if best.as_ref().is_none_or(|(current, _)| key > *current) {
            best = Some((
                key,
                MaxScoreTeamOrder {
                    recommended_team_card_ids: layout_ids,
                    skill_order_card_ids: order_ids,
                    skill_order_path_count: order_paths,
                    max_score_path_count: max_paths,
                    optimal_order_count: optimal_count,
                    total_path_count: SHUFFLE_PATH_COUNT as u16,
                },
            ));
        }
    }
    // Every skill permutation is reachable in some layout with its captain in slot 2.
    best.expect("a maximum-score order is reachable").1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_paths_match_independent_reference_and_documented_marginals() {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/skill_shuffle_order_distribution.json"
        ))
        .unwrap();
        for entry in reference.as_array().unwrap() {
            let order = std::array::from_fn(|i| entry["order"][i].as_u64().unwrap() as usize);
            assert_eq!(
                u64::from(tables().path_counts[order_index(order)]),
                entry["path_count"].as_u64().unwrap()
            );
        }
        assert_eq!(
            tables()
                .path_counts
                .iter()
                .map(|&x| u64::from(x))
                .sum::<u64>(),
            1024
        );
        assert_eq!(tables().path_counts.iter().filter(|&&x| x != 0).count(), 96);
        assert_eq!(
            tables()
                .path_counts
                .iter()
                .copied()
                .filter(|&x| x != 0)
                .min(),
            Some(4)
        );
        assert_eq!(tables().path_counts.iter().max(), Some(&19));
        assert_eq!(
            tables().slot_activation_counts.map(|row| row[4]),
            [256, 136, 288, 344, 0]
        );
        for (index, order) in tables().orders.iter().enumerate() {
            assert_eq!(index, order_index(*order));
            assert_eq!(tables().path_counts[index] == 0, order[4] == 4);
        }
    }

    #[test]
    fn hardcoded_weights_match_all_1024_native_shuffle_paths() {
        let mut counts = [0u16; ORDER_COUNT];
        for mut choices in 0..SHUFFLE_PATH_COUNT {
            let mut list = vec![0, 1, 2, 3, 4];
            for i in 0..5 {
                let member = list.remove(i);
                list.insert((choices % 4) as usize, member);
                choices /= 4;
            }
            counts[order_index(list.try_into().unwrap())] += 1;
        }
        assert_eq!(counts, tables().path_counts);
    }

    #[test]
    fn marginal_assignment_bound_matches_all_weighted_layouts() {
        for seed in 0..17 {
            let matrix = IndependentSkillScoreMatrix {
                base_score: 500_000,
                deltas: std::array::from_fn(|card| {
                    std::array::from_fn(|position| {
                        ((card * 317 + position * 199 + seed * 17 + card * position * 131) % 997)
                            as i32
                    })
                }),
            };
            let scores = matrix_order_scores(&matrix);
            let expected = tables()
                .layouts
                .iter()
                .map(|layout| {
                    layout
                        .weighted_orders
                        .iter()
                        .map(|order| {
                            i64::from(scores[layout.cards[2]][order.index]) * i64::from(order.count)
                        })
                        .sum::<i64>()
                })
                .max()
                .unwrap();
            assert_eq!(matrix_best_mean_numerator(&matrix), expected);
        }
    }

    #[test]
    fn tied_maximum_reports_total_probability_separately_from_one_sequence() {
        let recommendation =
            recommend_max_score_from_scores([10, 20, 30, 40, 50], &[100; ORDER_COUNT], 2);
        assert_eq!(recommendation.recommended_team_card_ids[2], 30);
        assert_eq!(recommendation.max_score_path_count, 1024);
        assert_eq!(recommendation.optimal_order_count, 96);
        assert_eq!(recommendation.skill_order_path_count, 19);
    }

    #[test]
    fn recommended_slots_make_a_unique_maximum_reachable_without_moving_captain() {
        let ids = [10, 20, 30, 40, 50];
        let mut scores = [0; ORDER_COUNT];
        scores[order_index([0, 1, 2, 3, 4])] = 100;
        let recommendation = recommend_max_score_from_scores(ids, &scores, 2);
        assert_eq!(recommendation.skill_order_card_ids, ids);
        assert_eq!(recommendation.recommended_team_card_ids[2], 30);
        assert_ne!(recommendation.recommended_team_card_ids[4], 50);
        assert!(recommendation.skill_order_path_count > 0);
        assert_eq!(
            recommendation.max_score_path_count,
            recommendation.skill_order_path_count
        );
    }
}
