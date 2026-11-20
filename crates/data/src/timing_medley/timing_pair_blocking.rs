//! Single-card exclusion certificates, prepared once and queried on prefixes.
//! No assignment or conflict scanner is changed. A fixed team's cards cannot
//! be used by either companion, even if their old dual prices were zero.
use super::*;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Bounds {
    singles: [Arc<Vec<f64>>; 2],
    price_sum: f64,
    keys: Arc<Vec<u16>>,
    subsets: Option<[Arc<Vec<f64>>; 2]>,
}
impl Bounds {
    pub(super) fn upper(&self, blocked: &[usize], paid: f64, base: [f64; 2]) -> f64 {
        let mask = blocked.iter().fold(0_u16, |m, &i| m | self.keys[i]) as usize;
        let single: [f64; 2] = std::array::from_fn(|song| {
            let individual = blocked
                .iter()
                .fold(base[song], |v, &i| v.min(self.singles[song][i]));
            self.subsets
                .as_ref()
                .map_or(individual, |v| individual.min(v[song][mask]))
        });
        (((single[0] + single[1]).next_up() + self.price_sum).next_up() - paid).next_up()
    }
}

impl Oracle {
    /// Exactly remove each legal subset of a small key-card universe. Other
    /// blocked cards are intentionally ignored here, so this stays an upper.
    /// If removing another key leaves the previous maximizer legal, its value
    /// is still the maximum and the 64-state assignment need not be rerun.
    fn excluding_subsets(&self, prices: &[f64], bits: &[u16], valid: &[bool]) -> Vec<f64> {
        let mut result = vec![f64::NEG_INFINITY; valid.len()];
        for p in &self.partitions {
            assert!(p.power_range.is_none());
            let rows: Vec<[[Vec<(f64, u16)>; 2]; 5]> = p
                .groups
                .iter()
                .map(|group| {
                    std::array::from_fn(|s| {
                        std::array::from_fn(|c| {
                            let mut row: Vec<_> = group
                                .iter()
                                .map(|v| {
                                    ((v.weights[s][c] - prices[v.card]).next_up(), bits[v.card])
                                })
                                .collect();
                            row.sort_by(|a, b| b.0.total_cmp(&a.0));
                            // A legal fixed team has at most five cards to exclude.
                            row.truncate(6);
                            row
                        })
                    })
                })
                .collect();
            let mut states = vec![(f64::NEG_INFINITY, 0_u16); valid.len()];
            for mask in 0..valid.len() {
                if !valid[mask] {
                    continue;
                }
                if mask != 0 {
                    let bit = 1_usize << mask.trailing_zeros();
                    let parent = states[mask ^ bit];
                    if parent.1 as usize & bit == 0 {
                        states[mask] = parent;
                        result[mask] = result[mask].max((parent.0 + p.error).next_up());
                        continue;
                    }
                }
                let mut dp = [(f64::NEG_INFINITY, 0_u16); 64];
                dp[0].0 = 0.;
                for group in &rows {
                    let row: [[(f64, u16); 2]; 5] = std::array::from_fn(|s| {
                        std::array::from_fn(|c| {
                            group[s][c]
                                .iter()
                                .copied()
                                .find(|v| v.1 as usize & mask == 0)
                                .unwrap_or((f64::NEG_INFINITY, 0))
                        })
                    });
                    let old = dp;
                    for state in 0..64 {
                        if !old[state].0.is_finite() {
                            continue;
                        }
                        let used = state >> 5;
                        for slot in 0..5 {
                            if state & (1 << slot) != 0 {
                                continue;
                            }
                            for captain in 0..=(1 - used) {
                                let target = ((state & 31) | (1 << slot)) | ((used + captain) << 5);
                                let value = (old[state].0 + row[slot][captain].0).next_up();
                                if value > dp[target].0 {
                                    dp[target] = (value, old[state].1 | row[slot][captain].1);
                                }
                            }
                        }
                    }
                }
                states[mask] = dp[63];
                result[mask] = result[mask].max((dp[63].0 + p.error).next_up());
            }
        }
        // Unprepared masks are never used to rule out a team.
        for (i, &legal) in valid.iter().enumerate() {
            if !legal {
                result[i] = f64::INFINITY;
            }
        }
        result
    }
    /// An upper for every catalogue after excluding each card individually.
    /// Within each character group keep the best two card identities per slot.
    /// Removing one card only changes rows in which it is the current winner.
    fn excluding_one(&self, prices: &[f64]) -> Vec<f64> {
        let mut result = vec![f64::NEG_INFINITY; prices.len()];
        for p in &self.partitions {
            assert!(
                p.power_range.is_none(),
                "exclusion table uses unrestricted linear oracles"
            );
            let full = p.best_at_slope(prices, 0.).value;
            let mut values = vec![full; prices.len()];
            let mut leaders = Vec::new();
            let rows: Vec<Matrix> = p
                .groups
                .iter()
                .map(|group| {
                    let mut best = [[[(f64::NEG_INFINITY, usize::MAX); 2]; 2]; 5];
                    for c in group {
                        for slot in 0..5 {
                            for captain in 0..2 {
                                let value = (c.weights[slot][captain] - prices[c.card]).next_up();
                                let row = &mut best[slot][captain];
                                if value > row[0].0 {
                                    row[1] = row[0];
                                    row[0] = (value, c.card);
                                } else if value > row[1].0 {
                                    row[1] = (value, c.card);
                                }
                            }
                        }
                    }
                    let row = std::array::from_fn(|s| std::array::from_fn(|c| best[s][c][0].0));
                    leaders.push(best);
                    row
                })
                .collect();
            for omitted in 0..p.groups.len() {
                let mut dp = [f64::NEG_INFINITY; 64];
                dp[0] = 0.;
                for (i, row) in rows.iter().enumerate() {
                    if i == omitted {
                        continue;
                    }
                    let old = dp;
                    for state in 0..64 {
                        if !old[state].is_finite() {
                            continue;
                        }
                        let used = state >> 5;
                        for slot in 0..5 {
                            if state & (1 << slot) != 0 {
                                continue;
                            }
                            for captain in 0..=(1 - used) {
                                let target = ((state & 31) | (1 << slot)) | ((used + captain) << 5);
                                dp[target] =
                                    dp[target].max((old[state] + row[slot][captain]).next_up());
                            }
                        }
                    }
                }
                let mut affected = std::collections::BTreeSet::new();
                for row in &leaders[omitted] {
                    for top in row {
                        affected.insert(top[0].1);
                    }
                }
                for card in affected {
                    if card == usize::MAX {
                        continue;
                    }
                    // The omitted character may be unused altogether.
                    let mut value = dp[63];
                    for slot in 0..5 {
                        for captain in 0..2 {
                            let best = &leaders[omitted][slot][captain];
                            let weight = best[usize::from(best[0].1 == card)].0;
                            let rest = (31 ^ (1 << slot)) | ((1 - captain) << 5);
                            value = value.max((dp[rest] + weight).next_up());
                        }
                    }
                    values[card] = (value + p.error).next_up();
                }
            }
            for (r, value) in result.iter_mut().zip(values) {
                *r = r.max(value);
            }
        }
        result
    }
}

impl Pair {
    /// Keep the pre-existing root/joint-power bounds, intersecting them with
    /// stronger exclusions only after at least one member has been fixed.
    pub fn prepare_blocking(
        pairs: &mut [Self],
        oracles: &[Oracle],
        characters: &[u32],
        key_limit: usize,
    ) -> usize {
        assert!(key_limit <= 16);
        let count = characters.len();
        let mut priority: Vec<_> = (0..count)
            .map(|i| {
                let price = pairs
                    .iter()
                    .flat_map(|p| &p.certificates)
                    .map(|c| c.prices[i])
                    .fold(0_f64, f64::max);
                (i, price)
            })
            .filter(|v| v.1 > 0.)
            .collect();
        priority.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        priority.truncate(key_limit);
        let mut bits = vec![0_u16; count];
        for (n, &(i, _)) in priority.iter().enumerate() {
            bits[i] = 1 << n;
        }
        let bits = Arc::new(bits);
        let valid: Vec<_> = (0_usize..1 << priority.len())
            .map(|mask| {
                if mask.count_ones() > 5 {
                    return false;
                }
                let mut seen = std::collections::BTreeSet::new();
                priority
                    .iter()
                    .enumerate()
                    .all(|(n, &(i, _))| mask & (1 << n) == 0 || seen.insert(characters[i]))
            })
            .collect();
        type SongTables = [Arc<Vec<f64>>; 3];
        let mut cache = BTreeMap::<Vec<u64>, (SongTables, Option<SongTables>)>::new();
        for (song, pair) in pairs.iter_mut().enumerate() {
            let mut affected = vec![false; count];
            for c in &mut pair.certificates {
                let key = c.prices.iter().map(|v| v.to_bits()).collect();
                let tables = cache.entry(key).or_insert_with(|| {
                    (
                        std::array::from_fn(|s| Arc::new(oracles[s].excluding_one(&c.prices))),
                        (!priority.is_empty()).then(|| {
                            std::array::from_fn(|s| {
                                Arc::new(oracles[s].excluding_subsets(&c.prices, &bits, &valid))
                            })
                        }),
                    )
                });
                let bounds = Bounds {
                    singles: [
                        tables.0[(song + 1) % 3].clone(),
                        tables.0[(song + 2) % 3].clone(),
                    ],
                    price_sum: c.prices.iter().fold(0_f64, |sum, &v| (sum + v).next_up()),
                    keys: bits.clone(),
                    subsets: tables
                        .1
                        .as_ref()
                        .map(|v| [v[(song + 1) % 3].clone(), v[(song + 2) % 3].clone()]),
                };
                for (card, active) in affected.iter_mut().enumerate() {
                    let old = (c.upper - c.prices[card]).next_up();
                    let new = bounds.upper(&[card], c.prices[card], c.singles);
                    if new < old {
                        *active = true;
                        pair.stats.blocking_max_drop = pair.stats.blocking_max_drop.max(old - new);
                    }
                }
                c.blocking = Some(bounds);
            }
            pair.stats.blocking_cards = affected.iter().filter(|&&v| v).count();
            pair.stats.blocking_key_cards = priority.iter().map(|v| v.0).collect();
            pair.stats.blocking_key_subsets = if priority.is_empty() {
                0
            } else {
                valid.iter().filter(|&&v| v).count()
            };
        }
        cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusion_table_matches_removed_card_oracle() {
        let mut seed = 71_u32;
        for sample in 0..30 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            };
            let count = 24;
            let prices: Vec<_> = (0..count).map(|_| (next() % 300) as f64 * 0.25).collect();
            let groups: Vec<Vec<_>> = (0..8)
                .map(|g| {
                    (0..3)
                        .map(|j| Candidate {
                            card: g * 3 + j,
                            power: 0.,
                            weights: std::array::from_fn(|_| {
                                std::array::from_fn(|_| {
                                    if sample == 0 {
                                        100.
                                    } else {
                                        (next() % 1000) as f64 * 0.25
                                    }
                                })
                            }),
                        })
                        .collect()
                })
                .collect();
            let oracle = Oracle {
                partitions: vec![Partition {
                    groups,
                    error: 0.25,
                    power_range: None,
                }],
            };
            let table = oracle.excluding_one(&prices);
            for omitted in 0..count {
                let removed = Oracle {
                    partitions: vec![Partition {
                        groups: oracle.partitions[0]
                            .groups
                            .iter()
                            .map(|g| g.iter().copied().filter(|c| c.card != omitted).collect())
                            .collect(),
                        error: 0.25,
                        power_range: None,
                    }],
                };
                let expected = removed.best(&prices).value;
                assert!(
                    (table[omitted] - expected).abs() < 1e-8,
                    "sample={sample} omitted={omitted}"
                );
            }
            let bits: Vec<_> = (0..count)
                .map(|i| if i < 6 { 1_u16 << i } else { 0 })
                .collect();
            let valid: Vec<_> = (0_usize..64).map(|m| m.count_ones() <= 5).collect();
            let table = oracle.excluding_subsets(&prices, &bits, &valid);
            for mask in 0..64 {
                if !valid[mask] {
                    assert!(table[mask].is_infinite());
                    continue;
                }
                let removed = Oracle {
                    partitions: vec![Partition {
                        groups: oracle.partitions[0]
                            .groups
                            .iter()
                            .map(|g| {
                                g.iter()
                                    .copied()
                                    .filter(|c| bits[c.card] as usize & mask == 0)
                                    .collect()
                            })
                            .collect(),
                        error: 0.25,
                        power_range: None,
                    }],
                };
                let expected = removed.best(&prices).value;
                assert!(
                    (table[mask] - expected).abs() < 1e-8,
                    "sample={sample} mask={mask}"
                );
            }
        }
    }
}
