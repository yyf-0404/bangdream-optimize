//! Upper bounds for the two teams accompanying a candidate. Each bound excludes
//! all five of the candidate's cards. A second bound limits the combined score
//! of two mutually disjoint companions while ignoring the current candidate.
//! The minimum of these relaxations still overestimates a legal three-team plan.

use crate::timing::Timer;

const SONG_COUNT: usize = 3;
const TEAM_SIZE: usize = 5;

// Bound both memory and query work when many top teams share the same cards.
// An unexamined tail contributes its highest score, never a guessed lower bound.
const PREFIX_BYTE_BUDGET: usize = 8 * 1024 * 1024;

pub(crate) fn enabled() -> bool {
    #[cfg(feature = "experimental-compatible-prune")]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var("BANGDREAM_OPTIMIZE_COMPATIBLE_PRUNE").as_deref() != Ok("off")
        })
    }
    #[cfg(not(feature = "experimental-compatible-prune"))]
    true
}

struct Candidate {
    cards: [usize; TEAM_SIZE],
    scores: [i64; SONG_COUNT],
}

struct RankedPrefix {
    scores: Vec<i64>,
    // For every card, set bits mark ranked candidates containing that card.
    conflicts: Vec<u64>,
    words: usize,
    first_open_word: Vec<usize>,
    tail_bound: Option<i64>,
}

impl RankedPrefix {
    fn new(candidates: &[Candidate], song: usize, limit: usize, card_count: usize) -> Self {
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        let length = limit.min(order.len());
        let key = |&index: &usize| (std::cmp::Reverse(candidates[index].scores[song]), index);
        let tail_bound = if length < order.len() {
            order.select_nth_unstable_by_key(length, key);
            Some(candidates[order[length]].scores[song])
        } else {
            None
        };
        order.truncate(length);
        order.sort_unstable_by_key(key);
        let words = length.div_ceil(64);
        let mut conflicts = vec![0; card_count * words];
        for (rank, &index) in order.iter().enumerate() {
            for &card in &candidates[index].cards {
                conflicts[card * words + rank / 64] |= 1 << (rank % 64);
            }
        }
        let first_open_word = (0..card_count)
            .map(|card| {
                (0..words)
                    .find(|&word| conflicts[card * words + word] != u64::MAX)
                    .unwrap_or(words)
            })
            .collect();
        Self {
            scores: order
                .iter()
                .map(|&index| candidates[index].scores[song])
                .collect(),
            conflicts,
            words,
            first_open_word,
            tail_bound,
        }
    }

    fn compatible_upper_bound(&self, cards: &[usize; TEAM_SIZE]) -> Option<i64> {
        let rows = cards.map(|card| card * self.words);
        // Every earlier word is wholly blocked by at least one selected card.
        let start = cards
            .iter()
            .map(|&card| self.first_open_word[card])
            .max()
            .unwrap_or(0);
        for word in start..self.words {
            let blocked = self.conflicts[rows[0] + word]
                | self.conflicts[rows[1] + word]
                | self.conflicts[rows[2] + word]
                | self.conflicts[rows[3] + word]
                | self.conflicts[rows[4] + word];
            let available = !blocked;
            if available != 0 {
                let rank = word * 64 + available.trailing_zeros() as usize;
                // Padding bits in the last word are outside the ranked prefix.
                return self.scores.get(rank).copied().or(self.tail_bound);
            }
        }
        self.tail_bound
    }
}

pub(crate) fn candidate_indices(
    indices: Vec<usize>,
    cards: impl Fn(usize) -> [usize; TEAM_SIZE],
    scores: impl Fn(usize) -> [i64; SONG_COUNT],
    floor: i64,
    stage: &str,
) -> Vec<usize> {
    let mode = maximum_bound_parts(stage);
    if mode == 0 {
        return indices;
    }
    let start = Timer::start();
    let mut candidates = indices
        .iter()
        .map(|&index| Candidate {
            cards: cards(index),
            scores: scores(index),
        })
        .collect::<Vec<_>>();
    let raw_card_count = candidates
        .iter()
        .flat_map(|candidate| candidate.cards)
        .max()
        .map_or(0, |card| card + 1);
    let mut positions = vec![usize::MAX; raw_card_count];
    let mut card_count = 0;
    for candidate in &mut candidates {
        for card in &mut candidate.cards {
            if positions[*card] == usize::MAX {
                positions[*card] = card_count;
                card_count += 1;
            }
            *card = positions[*card];
        }
    }
    let budget_limit = (PREFIX_BYTE_BUDGET / std::mem::size_of::<u64>() / card_count.max(1)) * 64;
    #[cfg(feature = "experimental-compatible-prune")]
    let budget_limit = std::env::var("BANGDREAM_OPTIMIZE_COMPATIBLE_PREFIX_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map_or(budget_limit, |limit| limit.min(budget_limit));
    let active = match mode {
        1 => compatible_flags_parts::<true, false>(&candidates, floor, budget_limit),
        2 => compatible_flags_parts::<false, true>(&candidates, floor, budget_limit),
        _ => compatible_flags(&candidates, floor, budget_limit),
    };
    let before = indices.len();
    let kept = indices
        .into_iter()
        .zip(active)
        .filter_map(|(index, keep)| keep.then_some(index))
        .collect::<Vec<_>>();
    if std::env::var_os("BANGDREAM_OPTIMIZE_PT_TRACE").is_some()
        || std::env::var_os("BANGDREAM_OPTIMIZE_DP_TRACE").is_some()
    {
        eprintln!(
            "PT medley compatible bound: stage={} before={} after={} elapsed_ms={:.3}",
            stage,
            before,
            kept.len(),
            start.elapsed_ms()
        );
    }
    kept
}

fn maximum_bound_parts(_stage: &str) -> u8 {
    #[cfg(feature = "experimental-maximize-optimizations")]
    if _stage.starts_with("max-") {
        static PARTS: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
        return *PARTS.get_or_init(|| {
            u8::from(
                std::env::var("BANGDREAM_OPTIMIZE_MAXIMIZE_TEAM_BOUND").as_deref() != Ok("off"),
            ) | (u8::from(
                std::env::var("BANGDREAM_OPTIMIZE_MAXIMIZE_PAIR_BOUND").as_deref() != Ok("off"),
            ) << 1)
        });
    }
    3
}

fn compatible_flags(candidates: &[Candidate], floor: i64, limit: usize) -> Vec<bool> {
    compatible_flags_parts::<true, true>(candidates, floor, limit)
}

fn compatible_flags_parts<const TEAM: bool, const PAIR: bool>(
    candidates: &[Candidate],
    floor: i64,
    limit: usize,
) -> Vec<bool> {
    let card_count = candidates
        .iter()
        .flat_map(|candidate| candidate.cards)
        .max()
        .map_or(0, |card| card + 1);
    let ranked: [RankedPrefix; SONG_COUNT] =
        std::array::from_fn(|song| RankedPrefix::new(candidates, song, limit, card_count));
    let bounds = candidates
        .iter()
        .map(|candidate| {
            ranked
                .each_ref()
                .map(|prefix| prefix.compatible_upper_bound(&candidate.cards))
        })
        .collect::<Vec<_>>();
    // Entry s bounds the other two songs. For every possible first team, take
    // its score plus an upper bound for a non-overlapping second team. This
    // remains conservative even if a memory-limited index uses its tail bound.
    let mut pair_bounds: [Option<i64>; SONG_COUNT] = [None; SONG_COUNT];
    if PAIR {
        for (candidate, bound) in candidates.iter().zip(&bounds) {
            for first_song in 0..SONG_COUNT {
                if let Some(second) = bound[(first_song + 1) % SONG_COUNT] {
                    let missing_song = (first_song + 2) % SONG_COUNT;
                    let pair = candidate.scores[first_song].saturating_add(second);
                    pair_bounds[missing_song] = pair_bounds[missing_song].max(Some(pair));
                }
            }
        }
    }
    candidates
        .iter()
        .zip(bounds)
        .map(|(candidate, bounds)| {
            (0..SONG_COUNT).any(|song| {
                let mut upper = i64::MAX;
                if TEAM {
                    let (Some(first), Some(second)) = (
                        bounds[(song + 1) % SONG_COUNT],
                        bounds[(song + 2) % SONG_COUNT],
                    ) else {
                        return false;
                    };
                    upper = first.saturating_add(second);
                }
                if PAIR {
                    let Some(pair) = pair_bounds[song] else {
                        return false;
                    };
                    upper = upper.min(pair);
                }
                candidate.scores[song].saturating_add(upper) >= floor
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disjoint(a: &Candidate, b: &Candidate) -> bool {
        a.cards.iter().all(|card| !b.cards.contains(card))
    }

    #[test]
    fn shared_best_cards_no_longer_inflate_companion_bounds() {
        let candidates = [
            Candidate {
                cards: [0, 1, 2, 3, 4],
                scores: [100; 3],
            },
            Candidate {
                cards: [0, 5, 6, 7, 8],
                scores: [99; 3],
            },
            Candidate {
                cards: [9, 10, 11, 12, 13],
                scores: [10; 3],
            },
        ];
        assert_eq!(
            compatible_flags(&candidates, 210, 4096),
            [false, false, false]
        );
        // Equality is retained; the best pair may overlap the current team.
        assert_eq!(
            compatible_flags(&candidates, 120, 4096),
            [true, false, true]
        );
    }

    #[test]
    fn no_companions_empty_prefix_and_padding_are_safe() {
        let candidates = [Candidate {
            cards: [0, 1, 2, 3, 4],
            scores: [100; 3],
        }];
        assert_eq!(compatible_flags(&candidates, 0, 4096), [false]);
        assert_eq!(compatible_flags(&candidates, 300, 0), [true]);
        assert!(compatible_flags(&[], 0, 4096).is_empty());
    }

    #[test]
    fn skips_wholly_blocked_words_without_skipping_a_compatible_team() {
        let candidates = (0..140)
            .map(|index| Candidate {
                cards: [if index < 128 { 0 } else { 1 }, 2, 3, 4, 5],
                scores: [1000 - index; SONG_COUNT],
            })
            .collect::<Vec<_>>();
        for limit in [63, 64, 127, 128, 129, 140] {
            let ranked = RankedPrefix::new(&candidates, 0, limit, 10);
            let bound = ranked.compatible_upper_bound(&[0, 6, 7, 8, 9]).unwrap();
            assert!(bound >= 872);
            if limit >= 128 {
                assert_eq!(bound, 872);
            }
            if limit == 140 {
                assert_eq!(ranked.first_open_word[0], 2);
                assert_eq!(ranked.compatible_upper_bound(&[0, 1, 7, 8, 9]), None);
            }
        }
    }

    #[test]
    fn sparse_card_positions_and_original_candidate_order_are_preserved() {
        let cards = [
            [2000, 7, 999, 345, 401],
            [0, 8, 1000, 346, 402],
            [1, 9, 1001, 347, 403],
            [2000, 8, 1001, 348, 404],
        ];
        let scores = [
            [100; SONG_COUNT],
            [100; SONG_COUNT],
            [100; SONG_COUNT],
            [1; SONG_COUNT],
        ];
        assert_eq!(
            candidate_indices(vec![2, 0, 3, 1], |i| cards[i], |i| scores[i], 300, "test"),
            [2, 0, 1]
        );
    }

    #[test]
    fn individual_bounds_and_their_intersection_preserve_feasible_triples() {
        let mut seed = 91547u64;
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..48 {
            let candidates: Vec<_> = (0..18)
                .map(|_| Candidate {
                    cards: std::array::from_fn(|_| random() as usize % 28),
                    scores: std::array::from_fn(|_| (random() % 120) as i64 - 10),
                })
                .collect();
            let floor = (random() % 321) as i64;
            for limit in [0, 7, 18] {
                let team = compatible_flags_parts::<true, false>(&candidates, floor, limit);
                let pair = compatible_flags_parts::<false, true>(&candidates, floor, limit);
                let both = compatible_flags(&candidates, floor, limit);
                // Each candidate may qualify on a different song under the two
                // bounds, so combined must be a subset of their intersection.
                for i in 0..candidates.len() {
                    assert!(!both[i] || (team[i] && pair[i]));
                }
                for a in 0..candidates.len() {
                    for b in 0..candidates.len() {
                        for c in 0..candidates.len() {
                            if disjoint(&candidates[a], &candidates[b])
                                && disjoint(&candidates[a], &candidates[c])
                                && disjoint(&candidates[b], &candidates[c])
                                && candidates[a].scores[0]
                                    + candidates[b].scores[1]
                                    + candidates[c].scores[2]
                                    >= floor
                            {
                                for active in [&team, &pair, &both] {
                                    assert!(active[a] && active[b] && active[c]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn bounded_queries_and_pruning_preserve_every_feasible_triple() {
        let mut seed = 123456789u64;
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..64 {
            let candidates = (0..70)
                .map(|_| {
                    let mut cards = [usize::MAX; TEAM_SIZE];
                    for slot in 0..TEAM_SIZE {
                        loop {
                            let card = random() as usize % 35;
                            if !cards.contains(&card) {
                                cards[slot] = card;
                                break;
                            }
                        }
                    }
                    Candidate {
                        cards,
                        scores: std::array::from_fn(|_| (random() % 1000) as i64),
                    }
                })
                .collect::<Vec<_>>();
            let floor = (random() % 3001) as i64;
            for limit in [0, 1, 7, 63, 64, 65, 70] {
                let active = compatible_flags(&candidates, floor, limit);
                for song in 0..SONG_COUNT {
                    let ranked = RankedPrefix::new(&candidates, song, limit, 35);
                    for candidate in &candidates {
                        let actual = candidates
                            .iter()
                            .filter(|other| disjoint(candidate, other))
                            .map(|other| other.scores[song])
                            .max();
                        let bound = ranked.compatible_upper_bound(&candidate.cards);
                        assert!(bound >= actual, "limit={limit}");
                        if limit >= candidates.len() {
                            assert_eq!(bound, actual);
                        }
                    }
                }
                for a in 0..candidates.len() {
                    for b in 0..candidates.len() {
                        if !disjoint(&candidates[a], &candidates[b]) {
                            continue;
                        }
                        for c in 0..candidates.len() {
                            if candidates[a].scores[0]
                                + candidates[b].scores[1]
                                + candidates[c].scores[2]
                                >= floor
                                && disjoint(&candidates[a], &candidates[c])
                                && disjoint(&candidates[b], &candidates[c])
                            {
                                assert!(active[a] && active[b] && active[c]);
                            }
                        }
                    }
                }
            }
        }
    }
}
