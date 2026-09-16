//! Reuse the third-song prefix compatible with a fixed first team.
//! The second-song traversal and callbacks retain their original order.
use super::*;

#[inline]
fn shrink_end(scores: &[BandScore], end: &mut usize, prefix: BandScore, floor: BandScore) {
    if *end != 0 && prefix.saturating_add(scores[*end - 1]) < floor {
        *end = scores[..*end].partition_point(|&score| prefix.saturating_add(score) >= floor);
    }
}

#[target_feature(enable = "avx2")]
pub(super) unsafe fn enumerate<const W: usize>(
    floor: BandScore,
    scores: &[[BandScore; 3]],
    masks: &[[u64; W]],
    implementation: MedleySolverImplementation,
    visit: impl FnMut([usize; 3], BandScore) -> MedleyBandVisit,
) -> MedleyBandMetrics {
    let Some(search) = prepare_band_search(scores, floor) else {
        return empty_band_metrics(floor, implementation);
    };
    enumerate_prepared(floor, scores, masks, implementation, search, visit)
}

// Maximum-score search supplies its stable tie order and uses best + 1 as floor.
#[target_feature(enable = "avx2")]
pub(super) unsafe fn enumerate_prepared<const W: usize>(
    floor: BandScore,
    scores: &[[BandScore; 3]],
    masks: &[[u64; W]],
    implementation: MedleySolverImplementation,
    search: BandSearchPreparation,
    visit: impl FnMut([usize; 3], BandScore) -> MedleyBandVisit,
) -> MedleyBandMetrics {
    enumerate_prepared_parts::<W, true, true>(floor, scores, masks, implementation, search, visit)
}

// Experimental switches are compile-time specializations: disabled work is not
// merely computed and ignored. The default PT and max paths use both parts.
#[target_feature(enable = "avx2")]
pub(super) unsafe fn enumerate_prepared_parts<
    const W: usize,
    const SECOND: bool,
    const REUSE: bool,
>(
    floor: BandScore,
    scores: &[[BandScore; 3]],
    masks: &[[u64; W]],
    implementation: MedleySolverImplementation,
    search: BandSearchPreparation,
    mut visit: impl FnMut([usize; 3], BandScore) -> MedleyBandVisit,
) -> MedleyBandMetrics {
    let mut metrics = empty_band_metrics(floor, implementation);
    let [song0, song1, song2] = search.song_order;
    let third_scores: Vec<_> = search.orders[song2]
        .iter()
        .map(|&k| scores[k][song2])
        .collect();
    let second_scores: Vec<_> = search.orders[song1]
        .iter()
        .map(|&j| scores[j][song1])
        .collect();
    let second_masks: [Vec<u64>; W] = std::array::from_fn(|word| {
        if !SECOND {
            return Vec::new();
        }
        search.orders[song1]
            .iter()
            .map(|&j| masks[j][word])
            .collect()
    });
    let third_masks: [Vec<u64>; W] = std::array::from_fn(|word| {
        if REUSE {
            return Vec::new();
        }
        search.orders[song2]
            .iter()
            .map(|&k| masks[k][word])
            .collect()
    });
    let mut second_positions = Vec::new();
    let mut second_end = second_scores.len();
    let mut positions = Vec::new();
    let mut filtered_masks: [Vec<u64>; W] = std::array::from_fn(|_| Vec::new());
    let zero = _mm256_setzero_si256();

    for &i in &search.orders[song0] {
        let score_i = scores[i][song0];
        if sum3(score_i, search.max_scores[song1], search.max_scores[song2]) < metrics.final_floor {
            break;
        }
        let mut ready = false;
        let mut end = third_scores.len();
        if second_end != 0
            && sum3(
                score_i,
                second_scores[second_end - 1],
                search.max_scores[song2],
            ) < metrics.final_floor
        {
            second_end = second_scores[..second_end].partition_point(|&s| {
                sum3(score_i, s, search.max_scores[song2]) >= metrics.final_floor
            });
        }
        if SECOND {
            second_positions.clear();
            let mut position = 0;
            while position + 4 <= second_end {
                let mut overlap = zero;
                for word in 0..W {
                    let used = _mm256_set1_epi64x(masks[i][word] as i64);
                    let candidates = _mm256_loadu_si256(
                        second_masks[word].as_ptr().add(position) as *const __m256i
                    );
                    overlap = _mm256_or_si256(overlap, _mm256_and_si256(used, candidates));
                }
                let mut lanes =
                    _mm256_movemask_pd(_mm256_castsi256_pd(_mm256_cmpeq_epi64(overlap, zero)))
                        as u32;
                while lanes != 0 {
                    second_positions.push(position + lanes.trailing_zeros() as usize);
                    lanes &= lanes - 1;
                }
                position += 4;
            }
            while position < second_end {
                if (0..W).all(|word| masks[i][word] & second_masks[word][position] == 0) {
                    second_positions.push(position);
                }
                position += 1;
            }
        }
        let mut pair_counted_to = 0;
        let second_length = if SECOND {
            second_positions.len()
        } else {
            second_end
        };
        for cursor in 0..second_length {
            let j_position = if SECOND {
                second_positions[cursor]
            } else {
                cursor
            };
            let j = search.orders[song1][j_position];
            let prefix = score_i.saturating_add(scores[j][song1]);
            if prefix.saturating_add(search.max_scores[song2]) < metrics.final_floor {
                break;
            }
            metrics.pair_checks = metrics
                .pair_checks
                .saturating_add((j_position + 1 - pair_counted_to) as u64);
            pair_counted_to = j_position + 1;
            if !SECOND && (0..W).any(|word| masks[i][word] & masks[j][word] != 0) {
                continue;
            }
            // For fixed i, j scores only decrease and the floor only rises.
            // Cache the cutoff, and build only the first (largest) needed prefix.
            shrink_end(&third_scores, &mut end, prefix, metrics.final_floor);
            if REUSE && !ready {
                positions.clear();
                for words in &mut filtered_masks {
                    words.clear();
                }
                for position in 0..end {
                    let k = search.orders[song2][position];
                    if masks[i].iter().zip(&masks[k]).any(|(&a, &b)| a & b != 0) {
                        continue;
                    }
                    positions.push(position);
                    for word in 0..W {
                        filtered_masks[word].push(masks[k][word]);
                    }
                }
                ready = true;
            }
            let mut cursor = 0;
            let mut counted_to = 0;
            let position_at = |index: usize| if REUSE { positions[index] } else { index };
            let third_length = if REUSE {
                positions.len()
            } else {
                third_scores.len()
            };
            let scan_masks = if REUSE { &filtered_masks } else { &third_masks };
            'third: while cursor < third_length && position_at(cursor) < end {
                let width = (third_length - cursor).min(4);
                let mut lanes;
                if width == 4 {
                    let mut overlap = zero;
                    for word in 0..W {
                        let used = _mm256_set1_epi64x(
                            (masks[j][word] | if REUSE { 0 } else { masks[i][word] }) as i64,
                        );
                        let candidates = _mm256_loadu_si256(
                            scan_masks[word].as_ptr().add(cursor) as *const __m256i
                        );
                        overlap = _mm256_or_si256(overlap, _mm256_and_si256(used, candidates));
                    }
                    lanes =
                        _mm256_movemask_pd(_mm256_castsi256_pd(_mm256_cmpeq_epi64(overlap, zero)))
                            as u32;
                } else {
                    lanes = 0;
                    for lane in 0..width {
                        if (0..W).all(|word| {
                            (masks[j][word] | if REUSE { 0 } else { masks[i][word] })
                                & scan_masks[word][cursor + lane]
                                == 0
                        }) {
                            lanes |= 1 << lane;
                        }
                    }
                }
                while lanes != 0 {
                    let position = position_at(cursor + lanes.trailing_zeros() as usize);
                    if position >= end {
                        break 'third;
                    }
                    lanes &= lanes - 1;
                    let score = prefix.saturating_add(third_scores[position]);
                    metrics.third_checks = metrics
                        .third_checks
                        .saturating_add((position + 1 - counted_to) as u64);
                    counted_to = position + 1;
                    metrics.compatible_triples = metrics.compatible_triples.saturating_add(1);
                    let k = search.orders[song2][position];
                    if apply_band_visit(
                        &mut metrics,
                        visit(indices_by_song(song0, i, song1, j, song2, k), score),
                    ) {
                        return metrics;
                    }
                    shrink_end(&third_scores, &mut end, prefix, metrics.final_floor);
                }
                cursor += width;
            }
            // Include positions eliminated by i, and preserve the baseline's
            // count when a callback raises the floor behind the current cursor.
            metrics.third_checks = metrics
                .third_checks
                .saturating_add(end.saturating_sub(counted_to) as u64);
        }
        if second_end != 0
            && sum3(
                score_i,
                second_scores[second_end - 1],
                search.max_scores[song2],
            ) < metrics.final_floor
        {
            second_end = second_scores[..second_end].partition_point(|&s| {
                sum3(score_i, s, search.max_scores[song2]) >= metrics.final_floor
            });
        }
        metrics.pair_checks = metrics
            .pair_checks
            .saturating_add(second_end.saturating_sub(pair_counted_to) as u64);
    }
    metrics
}
