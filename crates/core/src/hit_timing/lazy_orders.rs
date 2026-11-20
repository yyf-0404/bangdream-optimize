//! Bound-guided timing scoring for one fixed physical layout and center captain.
//! All 96 native orders are considered; probability sums every maximum-score tie.
use super::search::SearchError;
use crate::skill_shuffle::{tables, CAPTAIN_SLOT, SHUFFLE_PATH_COUNT};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTimingOrder {
    /// Indices into the five physical card positions, including the encore.
    pub slots: [usize; 6],
    pub paths: u16,
}

pub fn reachable_orders() -> Vec<NativeTimingOrder> {
    let tables = tables();
    let layout = tables
        .layouts
        .iter()
        .find(|l| l.cards == [0, 1, 2, 3, 4])
        .unwrap();
    layout
        .weighted_orders
        .iter()
        .map(|o| {
            let mut slots = [CAPTAIN_SLOT; 6];
            slots[..5].copy_from_slice(&tables.orders[o.index]);
            NativeTimingOrder {
                slots,
                paths: o.count,
            }
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingOrderMaximum {
    pub score: u64,
    /// Index in reachable_orders(); ties prefer higher path count, then lower index.
    pub chosen_order: usize,
    /// Native path weight of all tied per-order candidate maxima. These plans
    /// may differ before their random future is known; this alone does NOT prove
    /// that one causal/adaptive playing policy attains this hit probability.
    pub max_score_path_count: u16,
    pub optimal_order_count: usize,
    pub total_path_count: u16,
    pub scored_orders: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum TimingOrderError {
    #[error("expected one bound for each of the 96 reachable native orders")]
    InvalidInput,
    #[error("order {order}: score {score} exceeds upper bound {upper}")]
    Underestimated {
        order: usize,
        score: u64,
        upper: u64,
    },
    #[error(transparent)]
    Search(#[from] SearchError),
}

/// Exact over callback values, not a claim of globally optimal timing. The
/// callback must be deterministic for each order. No per-order maxima are
/// averaged into achievable average PT: that requires an information policy.
pub fn maximize(
    upper: &[u64],
    mut score: impl FnMut(usize) -> Result<u64, SearchError>,
) -> Result<TimingOrderMaximum, TimingOrderError> {
    let orders = reachable_orders();
    if upper.len() != orders.len() {
        return Err(TimingOrderError::InvalidInput);
    }
    let mut pending: Vec<_> = (0..orders.len()).collect();
    pending.sort_unstable_by_key(|&i| {
        (
            std::cmp::Reverse(upper[i]),
            std::cmp::Reverse(orders[i].paths),
            i,
        )
    });
    let mut result: Option<TimingOrderMaximum> = None;
    for i in pending {
        // Equality MUST be evaluated: it may add paths attaining the maximum.
        if result.as_ref().is_some_and(|r| upper[i] < r.score) {
            break;
        }
        let actual = score(i)?;
        if actual > upper[i] {
            return Err(TimingOrderError::Underestimated {
                order: i,
                score: actual,
                upper: upper[i],
            });
        }
        let r = result.get_or_insert(TimingOrderMaximum {
            score: actual,
            chosen_order: i,
            max_score_path_count: 0,
            optimal_order_count: 0,
            total_path_count: SHUFFLE_PATH_COUNT as u16,
            scored_orders: 0,
        });
        r.scored_orders += 1;
        if actual > r.score {
            r.score = actual;
            r.chosen_order = i;
            r.max_score_path_count = 0;
            r.optimal_order_count = 0;
        }
        if actual == r.score {
            r.max_score_path_count += orders[i].paths;
            r.optimal_order_count += 1;
            if (orders[i].paths, std::cmp::Reverse(i))
                > (
                    orders[r.chosen_order].paths,
                    std::cmp::Reverse(r.chosen_order),
                )
            {
                r.chosen_order = i;
            }
        }
    }
    Ok(result.unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_native_weights_and_encores_are_preserved() {
        let orders = reachable_orders();
        assert_eq!(orders.len(), 96);
        assert_eq!(orders.iter().map(|o| u32::from(o.paths)).sum::<u32>(), 1024);
        let mut unique = std::collections::BTreeSet::new();
        for o in orders {
            assert_eq!(o.slots[5], 2);
            assert_ne!(o.slots[4], 4);
            let mut first = o.slots[..5].to_vec();
            first.sort();
            assert_eq!(first, [0, 1, 2, 3, 4]);
            assert!(unique.insert(o.slots));
        }
    }

    #[test]
    fn zero_and_equal_upper_bounds_include_all_ties() {
        for value in [0, 1234] {
            let r = maximize(&[value; 96], |_| Ok(value)).unwrap();
            assert_eq!(r.max_score_path_count, 1024);
            assert_eq!(r.optimal_order_count, 96);
            assert_eq!(r.scored_orders, 96);
        }
    }

    #[test]
    fn matches_eager_maximum_and_probability_across_random_bounds() {
        let orders = reachable_orders();
        let mut bounds = [0; 96];
        bounds[0] = 50;
        bounds[1] = 50;
        let ties = maximize(&bounds, |i| Ok(bounds[i])).unwrap();
        assert_eq!(ties.scored_orders, 2);
        assert_eq!(ties.max_score_path_count, orders[0].paths + orders[1].paths);
        let mut seed = 944u32;
        for _ in 0..100 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            };
            let actual: Vec<_> = (0..96).map(|_| u64::from(next() % 30)).collect();
            let upper: Vec<_> = actual.iter().map(|&v| v + u64::from(next() % 10)).collect();
            let r = maximize(&upper, |i| Ok(actual[i])).unwrap();
            let maximum = *actual.iter().max().unwrap();
            assert_eq!(r.score, maximum);
            assert_eq!(
                r.max_score_path_count,
                orders
                    .iter()
                    .zip(&actual)
                    .filter(|(_, v)| **v == maximum)
                    .map(|(o, _)| o.paths)
                    .sum::<u16>()
            );
            assert_eq!(actual[r.chosen_order], maximum);
        }
        assert!(matches!(
            maximize(&[1; 96], |_| Ok(2)),
            Err(TimingOrderError::Underestimated { .. })
        ));
        assert!(matches!(
            maximize(&[], |_| Ok(2)),
            Err(TimingOrderError::InvalidInput)
        ));
    }
}
