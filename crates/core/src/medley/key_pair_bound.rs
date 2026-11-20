//! Shared three-song candidate bound, independent of scoring and timing rules.
//! Project cards onto at most 16 keys, then require both companions to avoid
//! those keys in the fixed team AND each other. Other conflicts are relaxed.
use crate::timing::Timer;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_KEYS: usize = 16;
const MISSING: i128 = i128::MIN;

#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub cards: [usize; 5],
    /// Three additive scores or conservative uppers, in the same unit as floor.
    /// i64 preserves weighted mean-score numerators without rounding/division.
    pub scores: [i64; 3],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("key-card projection supports at most 16 distinct keys")]
    TooManyKeys,
    #[error("key-card projection keys must be distinct")]
    DuplicateKey,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub keys: Vec<usize>,
    pub occupied_masks: usize,
    pub compatible_mask_pairs: u64,
    /// Diagnostic only: sums outside i64 saturate; missing pairs are None.
    pub pair_root_upper: [Option<i64>; 3],
    pub before: usize,
    pub after: usize,
    pub by_song: [usize; 3],
    pub preparation_seconds: f64,
    pub filter_seconds: f64,
}

/// Deterministic fallback for callers without card-price certificates. Cards
/// frequent near each song's best score receive priority. No candidate is
/// deleted here; a poor key choice only makes the later bound less selective.
pub fn select_keys(candidates: &[Candidate], limit: usize) -> Vec<usize> {
    let mut counts = BTreeMap::<usize, usize>::new();
    for song in 0..3 {
        let mut order: Vec<_> = (0..candidates.len()).collect();
        let key = |&i: &usize| (std::cmp::Reverse(candidates[i].scores[song]), i);
        let count = order.len().min(32);
        if count < order.len() {
            order.select_nth_unstable_by_key(count, key);
        }
        order.truncate(count);
        order.sort_unstable_by_key(key);
        for (rank, &i) in order.iter().enumerate() {
            for (slot, &card) in candidates[i].cards.iter().enumerate() {
                if !candidates[i].cards[..slot].contains(&card) {
                    *counts.entry(card).or_default() += count - rank;
                }
            }
        }
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_unstable_by_key(|&(card, count)| (std::cmp::Reverse(count), card));
    ranked.truncate(limit.min(MAX_KEYS));
    ranked.into_iter().map(|v| v.0).collect()
}

struct Table {
    bits: BTreeMap<usize, usize>,
    pair: Vec<[i128; 3]>,
    occupied: usize,
    combinations: u64,
}
impl Table {
    fn mask(&self, cards: &[usize]) -> usize {
        cards
            .iter()
            .fold(0, |m, c| m | self.bits.get(c).copied().unwrap_or(0))
    }
    fn new(candidates: &[Candidate], keys: &[usize]) -> Result<Self, Error> {
        if keys.len() > MAX_KEYS {
            return Err(Error::TooManyKeys);
        }
        if keys.iter().collect::<BTreeSet<_>>().len() != keys.len() {
            return Err(Error::DuplicateKey);
        }
        let mut result = Self {
            bits: keys.iter().enumerate().map(|(i, &c)| (c, 1 << i)).collect(),
            pair: vec![[MISSING; 3]; 1 << keys.len()],
            occupied: 0,
            combinations: 0,
        };
        let mut best = vec![None::<[i64; 3]>; result.pair.len()];
        for candidate in candidates {
            let row = best[result.mask(&candidate.cards)].get_or_insert(candidate.scores);
            for s in 0..3 {
                row[s] = row[s].max(candidate.scores[s]);
            }
        }
        let occupied: Vec<_> = best
            .iter()
            .enumerate()
            .filter_map(|(m, v)| v.map(|v| (m, v)))
            .collect();
        result.occupied = occupied.len();
        for (index, &(a, x)) in occupied.iter().enumerate() {
            for &(b, y) in &occupied[index..] {
                if a & b != 0 {
                    continue;
                }
                result.combinations += 1;
                for s in 0..3 {
                    let j = (s + 1) % 3;
                    let k = (s + 2) % 3;
                    result.pair[a | b][s] = result.pair[a | b][s]
                        .max(i128::from(x[j]) + i128::from(y[k]))
                        .max(i128::from(y[j]) + i128::from(x[k]));
                }
            }
        }
        for bit in 0..keys.len() {
            for mask in 0..result.pair.len() {
                if mask & (1 << bit) == 0 {
                    continue;
                }
                let sub = result.pair[mask ^ (1 << bit)];
                for s in 0..3 {
                    result.pair[mask][s] = result.pair[mask][s].max(sub[s]);
                }
            }
        }
        Ok(result)
    }
    fn companions(&self, cards: &[usize; 5]) -> [i128; 3] {
        self.pair[(self.pair.len() - 1) ^ self.mask(cards)]
    }
}

/// Companion upper for a partially selected team. Build only from a complete
/// domain, or envelopes covering every team that can reach the incumbent.
/// Each envelope must have an EXACT key-card occupancy; non-key identities may
/// be relaxed. No value here is an achievable score or a selected companion.
pub struct PrefixUpper(Table);
impl PrefixUpper {
    pub fn new(envelopes: &[Candidate], keys: &[usize]) -> Result<Self, Error> {
        Table::new(envelopes, keys).map(Self)
    }
    pub fn companions(&self, prefix: &[usize]) -> [Option<i128>; 3] {
        let table = &self.0;
        table.pair[(table.pair.len() - 1) ^ table.mask(prefix)].map(|v| (v != MISSING).then_some(v))
    }
    pub fn occupied_masks(&self) -> usize {
        self.0.occupied
    }
}

/// Return original indices in original order, preserving equality at floor.
/// The input must cover all candidates that may participate in a plan reaching
/// floor. A budget-interrupted partial enumeration is not such a domain.
/// Table values are upper bounds, never achievable scores or chosen teams.
pub fn filter(
    candidates: &[Candidate],
    keys: &[usize],
    floor: i64,
) -> Result<(Vec<usize>, Stats), Error> {
    let start = Timer::start();
    let table = Table::new(candidates, keys)?;
    let mut stats =
        Stats {
            keys: keys.to_vec(),
            occupied_masks: table.occupied,
            compatible_mask_pairs: table.combinations,
            pair_root_upper: table.pair.last().unwrap().map(|v| {
                (v != MISSING).then(|| v.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
            }),
            before: candidates.len(),
            after: 0,
            by_song: [0; 3],
            preparation_seconds: start.elapsed_ms() / 1000.,
            filter_seconds: 0.,
        };
    let start = Timer::start();
    let mut kept = Vec::new();
    for (i, candidate) in candidates.iter().enumerate() {
        let pair = table.companions(&candidate.cards);
        let mut any = false;
        for s in 0..3 {
            if pair[s] != MISSING && i128::from(candidate.scores[s]) + pair[s] >= i128::from(floor)
            {
                stats.by_song[s] += 1;
                any = true;
            }
        }
        if any {
            kept.push(i);
        }
    }
    stats.after = kept.len();
    stats.filter_seconds = start.elapsed_ms() / 1000.;
    Ok((kept, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projected_envelopes_cover_companions_of_every_prefix() {
        let keys = [0, 3, 7, 8, 12];
        let original: Vec<_> = (0usize..243)
            .map(|n| Candidate {
                cards: std::array::from_fn(|g| g * 3 + (n / 3usize.pow(g as u32)) % 3),
                scores: std::array::from_fn(|s| ((n + s * 51) % 77) as i64 - 20),
            })
            .collect();
        // Lose all non-key identities and take coordinatewise maxima, exactly
        // as the relaxed pre-enumeration domain is permitted to do.
        let mut merged: BTreeMap<Vec<usize>, [i64; 3]> = BTreeMap::new();
        for row in &original {
            let key: Vec<_> = row
                .cards
                .iter()
                .copied()
                .filter(|i| keys.contains(i))
                .collect();
            let value = merged.entry(key).or_insert(row.scores);
            for s in 0..3 {
                value[s] = value[s].max(row.scores[s]);
            }
        }
        let envelopes: Vec<_> = merged
            .into_iter()
            .map(|(keys, scores)| {
                let mut cards = [usize::MAX; 5];
                cards[..keys.len()].copy_from_slice(&keys);
                Candidate { cards, scores }
            })
            .collect();
        let table = PrefixUpper::new(&envelopes, &keys).unwrap();
        let mut checked = 0;
        for a in &original {
            for b in &original {
                if a.cards.iter().any(|i| b.cards.contains(i)) {
                    continue;
                }
                let free: Vec<_> = (0..15)
                    .filter(|i| !a.cards.contains(i) && !b.cards.contains(i))
                    .collect();
                for depth in 0..=5 {
                    let bound = table.companions(&free[..depth]);
                    for s in 0..3 {
                        let actual =
                            i128::from(a.scores[(s + 1) % 3]) + i128::from(b.scores[(s + 2) % 3]);
                        assert!(bound[s].unwrap() >= actual);
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 10_000);
        let empty = PrefixUpper::new(&[], &keys).unwrap();
        assert_eq!(empty.companions(&[]), [None; 3]);
    }
    #[test]
    fn projection_preserves_competitive_triples_with_signed_and_weighted_scores() {
        let mut seed = 735_u32;
        let mut checked = 0;
        let mut removed = 0;
        for sample in 0..100 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed >> 8
            };
            let scale = if sample % 2 == 0 { 1 } else { 1024 };
            let candidates: Vec<_> = (0..24)
                .map(|i| Candidate {
                    cards: std::array::from_fn(|g| {
                        g * 3 + if i < 3 { i } else { next() as usize % 3 }
                    }),
                    scores: std::array::from_fn(|_| (next() as i64 % 600 - 200) * scale),
                })
                .collect();
            let keys: Vec<_> = (0..sample % 17).collect();
            let table = Table::new(&candidates, &keys).unwrap();
            let floor = (next() as i64 % 1000 - 250) * scale;
            let (kept, _) = filter(&candidates, &keys, floor).unwrap();
            let selected = select_keys(&candidates, 16);
            assert_eq!(selected, select_keys(&candidates, 16));
            let (auto, _) = filter(&candidates, &selected, floor).unwrap();
            removed += candidates.len() - auto.len();
            let disjoint = |a: usize, b: usize| {
                candidates[a]
                    .cards
                    .iter()
                    .all(|c| !candidates[b].cards.contains(c))
            };
            for a in 0..24 {
                for b in 0..24 {
                    for c in 0..24 {
                        if !disjoint(a, b) || !disjoint(a, c) || !disjoint(b, c) {
                            continue;
                        }
                        checked += 1;
                        let ids = [a, b, c];
                        for s in 0..3 {
                            let actual =
                                i128::from(candidates[ids[(s + 1) % 3]].scores[(s + 1) % 3])
                                    + i128::from(candidates[ids[(s + 2) % 3]].scores[(s + 2) % 3]);
                            assert!(table.companions(&candidates[ids[s]].cards)[s] >= actual);
                        }
                        let total = (0..3)
                            .map(|s| i128::from(candidates[ids[s]].scores[s]))
                            .sum::<i128>();
                        if total >= i128::from(floor) {
                            assert!(ids.iter().all(|i| kept.contains(i) && auto.contains(i)));
                        }
                    }
                }
            }
        }
        assert!(
            checked > 1000,
            "random test must actually contain legal triples"
        );
        assert!(removed > 0, "test must exercise pruning");
    }

    #[test]
    fn sparse_ids_empty_domains_ties_and_i64_limits() {
        let cards: Vec<_> = (0..3)
            .map(|i| Candidate {
                cards: std::array::from_fn(|g| usize::MAX - (i * 5 + g)),
                scores: [i64::MAX, i64::MIN, 7],
            })
            .collect();
        let keys: Vec<_> = cards.iter().flat_map(|v| v.cards).collect();
        // MAX + MIN + 7 = 6. Pair sums must not overflow or lose signed terms.
        assert_eq!(filter(&cards, &keys, 6).unwrap().0, vec![0, 1, 2]);
        assert!(filter(&cards, &keys, 7).unwrap().0.is_empty());
        assert!(filter(&cards[..1], &keys, i64::MIN).unwrap().0.is_empty());
        assert!(filter(&[], &[], 0).unwrap().0.is_empty());
        assert_eq!(filter(&cards, &[1, 1], 0).unwrap_err(), Error::DuplicateKey);
        assert_eq!(
            filter(&cards, &(0..17).collect::<Vec<_>>(), 0).unwrap_err(),
            Error::TooManyKeys
        );
        // Repeated IDs can represent a padded mask with fewer than five cards.
        let padded = [
            Candidate {
                cards: [9; 5],
                scores: [10; 3],
            },
            Candidate {
                cards: [8; 5],
                scores: [10; 3],
            },
            Candidate {
                cards: [7; 5],
                scores: [10; 3],
            },
        ];
        assert_eq!(filter(&padded, &[7, 8, 9], 30).unwrap().0, vec![0, 1, 2]);
    }
}
