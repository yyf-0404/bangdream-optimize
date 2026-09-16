//! Experimental per-card inverted index in the second/third song's score order.
//! Both loops skip conflicting positions in 64-candidate blocks. The third-song
//! complement for the first team is built lazily and reused for every second team.
use super::*;

#[cfg(feature = "experimental-bitmap-band")]
pub(super) fn enabled() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("BANGDREAM_OPTIMIZE_BAND_BACKEND").as_deref() != Ok("scan")
    }
    #[cfg(target_arch = "wasm32")]
    true
}

struct CardIndex {
    words: Vec<u64>,
    stride: usize,
}

impl CardIndex {
    fn new(order: &[usize], members: &[Vec<usize>], card_count: usize) -> Self {
        let stride = order.len().div_ceil(64);
        let mut words = vec![0; card_count * stride];
        for (position, &candidate) in order.iter().enumerate() {
            for &card in &members[candidate] {
                words[card * stride + position / 64] |= 1u64 << (position % 64);
            }
        }
        Self { words, stride }
    }

    #[inline]
    fn exclude(&self, members: &[usize], block: usize, mut available: u64) -> u64 {
        for &card in members {
            available &= !self.words[card * self.stride + block];
            if available == 0 {
                break;
            }
        }
        available
    }
}

// Compact sparse/high mask bits before allocating rows. No candidate x candidate
// matrix is stored; index space is O(distinct cards * candidates / 64).
fn card_members<'a>(masks: impl Iterator<Item = &'a [u64]>) -> (Vec<Vec<usize>>, usize) {
    let mut slots = std::collections::HashMap::new();
    let members = masks
        .map(|mask| {
            let mut cards = Vec::new();
            for (word, &bits) in mask.iter().enumerate() {
                let mut remaining = bits;
                while remaining != 0 {
                    let bit = word * 64 + remaining.trailing_zeros() as usize;
                    let next = slots.len();
                    cards.push(*slots.entry(bit).or_insert(next));
                    remaining &= remaining - 1;
                }
            }
            cards
        })
        .collect();
    (members, slots.len())
}

#[inline]
fn low_bits(count: usize) -> u64 {
    if count == 64 {
        u64::MAX
    } else {
        (1u64 << count) - 1
    }
}

// The cutoff is inclusive and uses the exact same saturating addition as the
// scalar enumerator. Never subtract the floor: that can overflow at i64 limits.
#[inline]
fn block_end(scores: &[BandScore], start: usize, prefix: BandScore, floor: BandScore) -> usize {
    let end = (start + 64).min(scores.len());
    if prefix.saturating_add(scores[end - 1]) >= floor {
        return end;
    }
    start + scores[start..end].partition_point(|&score| prefix.saturating_add(score) >= floor)
}

pub(super) fn enumerate<'a>(
    floor: BandScore,
    scores: &[[BandScore; 3]],
    masks: impl Iterator<Item = &'a [u64]>,
    mut visit: impl FnMut([usize; 3], BandScore) -> MedleyBandVisit,
) -> MedleyBandMetrics {
    // The bitmap implementation is portable scalar u64 code, including on WASM.
    let implementation = MedleySolverImplementation::Scalar;
    let mut metrics = empty_band_metrics(floor, implementation);
    let Some(search) = prepare_band_search(scores, floor) else {
        return metrics;
    };
    let [song0, song1, song2] = search.song_order;
    let (members, card_count) = card_members(masks);
    let second = CardIndex::new(&search.orders[song1], &members, card_count);
    let third = CardIndex::new(&search.orders[song2], &members, card_count);
    let second_scores: Vec<_> = search.orders[song1]
        .iter()
        .map(|&j| scores[j][song1])
        .collect();
    let third_scores: Vec<_> = search.orders[song2]
        .iter()
        .map(|&k| scores[k][song2])
        .collect();
    let mut third_without_i = vec![0; third.stride];

    for &i in &search.orders[song0] {
        let score_i = scores[i][song0];
        if sum3(score_i, search.max_scores[song1], search.max_scores[song2]) < metrics.final_floor {
            break;
        }
        let mut built_third_blocks = 0;
        'second: for j_block in 0..second.stride {
            let start = j_block * 64;
            // Preserve addition order even for saturating scores.
            let mut end = (start + 64).min(second_scores.len());
            if sum3(score_i, second_scores[end - 1], search.max_scores[song2]) < metrics.final_floor
            {
                end = start
                    + second_scores[start..end].partition_point(|&s| {
                        sum3(score_i, s, search.max_scores[song2]) >= metrics.final_floor
                    });
            }
            if end == start {
                break;
            }
            let mut js = second.exclude(&members[i], j_block, low_bits(end - start));
            let mut counted_to = start;
            while js != 0 {
                let j_pos = start + js.trailing_zeros() as usize;
                let j = search.orders[song1][j_pos];
                let score_ij = score_i.saturating_add(second_scores[j_pos]);
                if score_ij.saturating_add(search.max_scores[song2]) < metrics.final_floor {
                    // A callback raised the floor: count only positions the old
                    // sequential scan would still have reached.
                    let remaining = second_scores[counted_to..end].partition_point(|&s| {
                        sum3(score_i, s, search.max_scores[song2]) >= metrics.final_floor
                    });
                    metrics.pair_checks += remaining as u64;
                    break 'second;
                }
                metrics.pair_checks += (j_pos + 1 - counted_to) as u64;
                counted_to = j_pos + 1;
                js &= js - 1;

                for k_block in 0..third.stride {
                    let k_start = k_block * 64;
                    if score_ij.saturating_add(third_scores[k_start]) < metrics.final_floor {
                        break;
                    }
                    let k_end = block_end(&third_scores, k_start, score_ij, metrics.final_floor);
                    if k_block == built_third_blocks {
                        third_without_i[k_block] = third.exclude(&members[i], k_block, u64::MAX);
                        built_third_blocks += 1;
                    }
                    let mut ks = third.exclude(
                        &members[j],
                        k_block,
                        third_without_i[k_block] & low_bits(k_end - k_start),
                    );
                    let mut k_counted_to = k_start;
                    while ks != 0 {
                        let k_pos = k_start + ks.trailing_zeros() as usize;
                        let score = score_ij.saturating_add(third_scores[k_pos]);
                        if score < metrics.final_floor {
                            break;
                        }
                        metrics.third_checks += (k_pos + 1 - k_counted_to) as u64;
                        k_counted_to = k_pos + 1;
                        ks &= ks - 1;
                        metrics.compatible_triples += 1;
                        let k = search.orders[song2][k_pos];
                        if apply_band_visit(
                            &mut metrics,
                            visit(indices_by_song(song0, i, song1, j, song2, k), score),
                        ) {
                            return metrics;
                        }
                    }
                    let final_end =
                        block_end(&third_scores, k_start, score_ij, metrics.final_floor);
                    // The floor may exclude positions already visited; don't
                    // subtract those checks from the diagnostic counters.
                    metrics.third_checks += final_end.saturating_sub(k_counted_to) as u64;
                    if final_end < (k_start + 64).min(third_scores.len()) {
                        break;
                    }
                }
            }
            let final_end = counted_to
                + second_scores[counted_to..end].partition_point(|&s| {
                    sum3(score_i, s, search.max_scores[song2]) >= metrics.final_floor
                });
            metrics.pair_checks += (final_end - counted_to) as u64;
            if final_end < (start + 64).min(second_scores.len()) {
                break;
            }
        }
    }
    metrics
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(input: WideMedleyBandInput) {
        for mode in 0..3 {
            let action = |count: usize, score: BandScore| match mode {
                1 => MedleyBandVisit::Continue { floor: score },
                2 if count == 7 => MedleyBandVisit::Break,
                _ => MedleyBandVisit::Continue { floor: input.floor },
            };
            let mut expected = Vec::new();
            let mut reference = enumerate_band_wide_scalar(&input, |indices, score| {
                expected.push((indices, score));
                action(expected.len(), score)
            });
            let mut actual = Vec::new();
            let metrics = enumerate(
                input.floor,
                &input.scores,
                input.team_masks.iter().map(Vec::as_slice),
                |indices, score| {
                    actual.push((indices, score));
                    action(actual.len(), score)
                },
            );
            assert_eq!(actual, expected, "callback order, mode={mode}");
            reference.implementation = metrics.implementation;
            assert_eq!(
                metrics, reference,
                "logical counters and cutoff, mode={mode}"
            );
        }
    }

    #[test]
    fn bitmap_matches_scalar_across_word_boundaries_and_dynamic_floors() {
        let mut state = 0x4d59_5df4_d0f3_3173u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for n in [0, 1, 2, 3, 63, 64, 65, 127, 128, 129] {
            for _ in 0..4 {
                let mut masks = Vec::new();
                let mut scores = Vec::new();
                for _ in 0..n {
                    let mut mask = vec![0; 4];
                    for _ in 0..5 {
                        let bit = next() as usize % 200;
                        mask[bit / 64] |= 1 << (bit % 64);
                    }
                    masks.push(mask);
                    scores.push(std::array::from_fn(|_| (next() % 101) as i64 - 30));
                }
                compare(WideMedleyBandInput {
                    floor: 145,
                    team_masks: masks,
                    scores,
                });
            }
        }
    }

    #[test]
    fn bitmap_handles_empty_masks_all_conflicts_ties_and_saturation() {
        for masks in [vec![vec![0; 33]; 65], vec![vec![1; 33]; 65]] {
            compare(WideMedleyBandInput {
                floor: 30,
                team_masks: masks,
                scores: vec![[10; 3]; 65],
            });
        }
        for floor in [i64::MIN, -1, 0, i64::MAX] {
            compare(WideMedleyBandInput {
                floor,
                team_masks: (0..65)
                    .map(|i| {
                        let mut mask = vec![0; 33];
                        mask[32] = 1 << (i % 64);
                        mask
                    })
                    .collect(),
                scores: (0..65)
                    .map(|i| [i64::MAX - i, i64::MAX - i * 2, i64::MAX - i * 3])
                    .collect(),
            });
        }
    }
}
