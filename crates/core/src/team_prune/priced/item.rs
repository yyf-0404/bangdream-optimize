//! Conditional bound for an entire item configuration, before team enumeration.
//! A plan reaching F needs score_s >= F - sum(other song upper bounds). With
//! x=lambda*P+M/lambda, score_s <= x^2/4. On [lo,hi], the secant of x^2/4 is an
//! upper bound. The resulting 15 linear slot contributions share one card pool.
//! Relaxing team character/signature requirements makes this bound conservative.
use super::{assignment_bound, top_five, Choice, MedleyCardPruneProfile};
use crate::model::{
    chart::{Chart, TeamCardSkill},
    preparation::PreparedCard,
};
use std::collections::BTreeMap;

pub(crate) struct ItemAnalysis {
    pub(crate) upper: f64,
    pub(crate) allowed_cards: Vec<bool>,
    pub(crate) resource_bound: Option<ResourceBound>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResourceBound {
    pub(crate) upper: f64,
    pub(crate) card_losses: Vec<[f64; 3]>,
    pub(crate) prices: Vec<f64>,
    pub(crate) other_upper_by_song: [f64; 3],
}

impl ItemAnalysis {
    pub(crate) fn unbounded(count: usize) -> Self {
        Self {
            upper: f64::INFINITY,
            allowed_cards: vec![true; count],
            resource_bound: None,
        }
    }
}

pub(crate) fn conditional_item_bound(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    charts: &[Chart],
    song_bounds: &[f64],
    floor: i32,
) -> f64 {
    conditional_item_analysis_internal(cards, profiles, charts, song_bounds, floor, false).upper
}

pub(crate) fn conditional_item_analysis(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    charts: &[Chart],
    song_bounds: &[f64],
    floor: i32,
) -> ItemAnalysis {
    conditional_item_analysis_internal(cards, profiles, charts, song_bounds, floor, true)
}

fn conditional_item_analysis_internal(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    charts: &[Chart],
    song_bounds: &[f64],
    floor: i32,
    filter_cards: bool,
) -> ItemAnalysis {
    if floor <= 0 || charts.len() != 3 || song_bounds.len() != 3 || cards.len() < 15 {
        return ItemAnalysis::unbounded(cards.len());
    }
    if song_bounds.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return ItemAnalysis::unbounded(cards.len());
    }
    let mut choices = Vec::new();
    let mut bases = Vec::new();
    for (song, chart) in charts.iter().enumerate() {
        let inflation = 1.0 + f64::EPSILON * 32.0 * (chart.count as f64 + 64.0);
        let mut rateup_cache = BTreeMap::new();
        let mut song_choices = Vec::new();
        for (card, profile) in cards.iter().zip(profiles) {
            let mut skill: [f64; 6] =
                std::array::from_fn(|slot| profile.skill_meta_bounds[song * 6 + slot].high);
            if card.skill.rateup && chart.warning.is_empty() {
                skill = [0.0; 6];
                for up in [
                    Some(card.score_up.default),
                    card.score_up.unification_activate_effect_value,
                ]
                .into_iter()
                .flatten()
                {
                    let key = (card.skill.duration.to_bits(), up.to_bits());
                    let values = if let Some(&values) = rateup_cache.get(&key) {
                        values
                    } else {
                        let Ok(meta) = chart.conditional_skill_meta(TeamCardSkill {
                            score_up: up,
                            ..card.skill
                        }) else {
                            return ItemAnalysis::unbounded(cards.len());
                        };
                        let values = meta.map(|row| row.into_iter().fold(0.0, f64::max));
                        rateup_cache.insert(key, values);
                        values
                    };
                    for slot in 0..6 {
                        skill[slot] = skill[slot].max(values[slot]);
                    }
                }
            }
            if !profile.stat.is_finite()
                || profile.stat < 0.0
                || skill.iter().any(|v| !v.is_finite() || *v < 0.0)
            {
                return ItemAnalysis::unbounded(cards.len());
            }
            song_choices.push(Choice {
                power: (profile.stat * (1.0 + 64.0 * f64::EPSILON)).next_up(),
                skill: skill.map(|v| (v * inflation).next_up()),
            });
        }
        choices.push(song_choices);
        bases.push((chart.meta.no_skill * inflation).next_up());
    }
    let mut best = f64::INFINITY;
    let mut allowed_cards = vec![true; cards.len()];
    let mut resource_bound = None;
    for scale in [0.8, 1.0, 1.25] {
        let mut weights = Vec::with_capacity(15);
        let mut constant = 0.0;
        let mut constants = [0.0f64; 3];
        for song in 0..3 {
            let mut grouped = BTreeMap::<u32, Vec<Choice>>::new();
            let mut captain: f64 = 0.0;
            for (card, &choice) in cards.iter().zip(&choices[song]) {
                captain = captain.max(choice.skill[5]);
                let mut normal = choice;
                normal.skill[5] = 0.0;
                grouped.entry(card.character_id).or_default().push(normal);
            }
            let groups: Vec<_> = grouped.into_values().collect();
            let power = top_five(
                groups
                    .iter()
                    .map(|g| g.iter().map(|c| c.power).fold(0.0, f64::max)),
            );
            let meta = top_five(groups.iter().map(|g| {
                g.iter()
                    .map(|c| c.skill[..5].iter().copied().fold(0.0, f64::max))
                    .fold(0.0, f64::max)
            })) + bases[song]
                + captain;
            let lambda = (meta / power).sqrt() * scale;
            if !lambda.is_finite() || lambda <= 0.0 {
                return ItemAnalysis::unbounded(cards.len());
            }
            let fixed = ((bases[song] + captain).next_up() / lambda).next_up();
            let high = (assignment_bound(&groups, lambda) + fixed).next_up();
            let others = (song_bounds[(song + 1) % 3] + song_bounds[(song + 2) % 3]).next_up();
            let lower_score = (f64::from(floor) - others).next_down().max(0.0);
            let low = (2.0 * lower_score.sqrt().next_down()).next_down().max(0.0);
            if !high.is_finite() || low > high {
                return ItemAnalysis::unbounded(cards.len());
            }
            let a = ((low + high).next_up() / 4.0).next_up();
            let b = (-(low * high).next_down() / 4.0).next_up();
            constants[song] = ((a * fixed).next_up() + b).next_up();
            constant = (constant + constants[song]).next_up();
            for slot in 0..5 {
                let mut row = Vec::with_capacity(cards.len());
                for choice in &choices[song] {
                    let linear = ((lambda * choice.power).next_up()
                        + (choice.skill[slot] / lambda).next_up())
                    .next_up();
                    let weight = (a * linear).next_up().ceil();
                    if !weight.is_finite() || weight < 0.0 || weight >= i64::MAX as f64 {
                        return ItemAnalysis::unbounded(cards.len());
                    }
                    row.push(weight as i64);
                }
                weights.push(row);
            }
        }
        let dual = assignment_dual(&weights);
        let assignment = dual.upper as f64;
        // Include conversion/accumulation error even if the constant is negative.
        let error = (assignment.abs() + constant.abs()) * 128.0 * f64::EPSILON;
        let upper = ((assignment + constant).next_up() + error).next_up().ceil();
        let improves = upper < best;
        best = best.min(upper);
        if filter_cards {
            // Every selected card occupies one of the 15 normal slots. Its
            // smallest dual slack across rows is an unavoidable loss, including
            // when the card is also captain. Keep equality for tied optima.
            let mut card_losses = Vec::with_capacity(cards.len());
            for (card, allowed) in allowed_cards.iter_mut().enumerate() {
                let losses: [f64; 3] = std::array::from_fn(|song| {
                    let loss = (song * 5..song * 5 + 5)
                        .map(|row| {
                            dual.rows[row] + dual.prices[card] - i128::from(weights[row][card])
                        })
                        .min()
                        .unwrap();
                    debug_assert!(loss >= 0);
                    (loss as f64).next_down().max(0.0)
                });
                let loss = losses.into_iter().fold(f64::INFINITY, f64::min);
                if (upper - loss).next_up() < f64::from(floor) {
                    *allowed = false;
                }
                card_losses.push(losses);
            }
            if improves {
                resource_bound = Some(ResourceBound {
                    upper,
                    card_losses,
                    prices: dual
                        .prices
                        .iter()
                        .map(|&p| (p as f64).next_down().max(0.0))
                        .collect(),
                    other_upper_by_song: std::array::from_fn(|song| {
                        let linear = dual.prices.iter().sum::<i128>()
                            + dual
                                .rows
                                .iter()
                                .enumerate()
                                .filter(|(row, _)| row / 5 != song)
                                .map(|(_, &v)| v)
                                .sum::<i128>();
                        let fixed =
                            (constants[(song + 1) % 3] + constants[(song + 2) % 3]).next_up();
                        let error = ((linear as f64).abs() + fixed.abs()) * 128.0 * f64::EPSILON;
                        (((linear as f64).next_up() + fixed).next_up() + error).next_up()
                    }),
                });
            }
        }
    }
    ItemAnalysis {
        upper: best,
        allowed_cards,
        resource_bound,
    }
}

// Rectangular Hungarian assignment supplies column prices. Reconstruct a
// feasible dual directly: sum prices + sum_rows max_col(weight - price).
// Consequently pruning depends on this verified bound, not on a chosen matching.
struct AssignmentDual {
    upper: i128,
    prices: Vec<i128>,
    rows: Vec<i128>,
}

fn assignment_dual(weights: &[Vec<i64>]) -> AssignmentDual {
    let rows = weights.len();
    let cols = weights[0].len();
    assert!(rows <= cols);
    let mut u = vec![0i128; rows + 1];
    let mut v = vec![0i128; cols + 1];
    let mut matched = vec![0usize; cols + 1];
    let mut previous = vec![0usize; cols + 1];
    for row in 1..=rows {
        matched[0] = row;
        let mut column = 0;
        let mut minima = vec![i128::MAX; cols + 1];
        let mut used = vec![false; cols + 1];
        loop {
            used[column] = true;
            let active = matched[column];
            let mut delta = i128::MAX;
            let mut next = 0;
            for j in 1..=cols {
                if used[j] {
                    continue;
                }
                let reduced = -i128::from(weights[active - 1][j - 1]) - u[active] - v[j];
                if reduced < minima[j] {
                    minima[j] = reduced;
                    previous[j] = column;
                }
                if minima[j] < delta {
                    delta = minima[j];
                    next = j;
                }
            }
            for j in 0..=cols {
                if used[j] {
                    u[matched[j]] += delta;
                    v[j] -= delta;
                } else if j != 0 {
                    minima[j] -= delta;
                }
            }
            column = next;
            if matched[column] == 0 {
                break;
            }
        }
        loop {
            let next = previous[column];
            matched[column] = matched[next];
            column = next;
            if column == 0 {
                break;
            }
        }
    }
    let prices: Vec<_> = v[1..].iter().map(|&v| (-v).max(0)).collect();
    let rows: Vec<_> = weights
        .iter()
        .map(|row| {
            row.iter()
                .zip(&prices)
                .map(|(&weight, &price)| i128::from(weight) - price)
                .max()
                .unwrap()
        })
        .collect();
    AssignmentDual {
        upper: prices.iter().sum::<i128>() + rows.iter().sum::<i128>(),
        prices,
        rows,
    }
}
