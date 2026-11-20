//! Portable reuse of the third-song prefix disjoint from a fixed first team.
//! Only one prefix is retained: extra storage is linear in the candidate count.
use super::*;

#[inline]
fn disjoint<const W: usize>(left: &[u64; W], right: &[u64; W]) -> bool {
    (0..W).all(|word| left[word] & right[word] == 0)
}

#[inline]
fn shrink_end(scores: &[BandScore], end: &mut usize, prefix: BandScore, floor: BandScore) {
    if *end != 0 && prefix.saturating_add(scores[*end - 1]) < floor {
        *end = scores[..*end].partition_point(|&score| prefix.saturating_add(score) >= floor);
    }
}

struct Third<const W: usize> {
    position: usize,
    mask: [u64; W],
}

pub(super) fn enumerate<const W: usize>(
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

fn enumerate_prepared<const W: usize>(
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
    let mut filtered = Vec::<Third<W>>::new();

    for &i in &search.orders[song0] {
        let score_i = scores[i][song0];
        if sum3(score_i, search.max_scores[song1], search.max_scores[song2]) < metrics.final_floor {
            break;
        }
        let mut ready = false;
        let mut end = third_scores.len();
        for &j in &search.orders[song1] {
            let prefix = score_i.saturating_add(scores[j][song1]);
            if prefix.saturating_add(search.max_scores[song2]) < metrics.final_floor {
                break;
            }
            metrics.pair_checks = metrics.pair_checks.saturating_add(1);
            if !disjoint(&masks[i], &masks[j]) {
                continue;
            }
            // For fixed i, j scores decrease and the visitor's floor only rises.
            // The first eligible prefix contains every later eligible prefix.
            shrink_end(&third_scores, &mut end, prefix, metrics.final_floor);
            if !ready {
                filtered.clear();
                for (position, &k) in search.orders[song2][..end].iter().enumerate() {
                    if disjoint(&masks[i], &masks[k]) {
                        filtered.push(Third {
                            position,
                            mask: masks[k],
                        });
                    }
                }
                ready = true;
            }
            let mut counted_to = 0;
            for third in &filtered {
                if third.position >= end {
                    break;
                }
                // No per-position score addition is needed until compatibility
                // is known: end already certifies the inclusive score cutoff.
                if !disjoint(&masks[j], &third.mask) {
                    continue;
                }
                metrics.third_checks = metrics
                    .third_checks
                    .saturating_add((third.position + 1 - counted_to) as u64);
                counted_to = third.position + 1;
                metrics.compatible_triples = metrics.compatible_triples.saturating_add(1);
                let score = prefix.saturating_add(third_scores[third.position]);
                let k = search.orders[song2][third.position];
                if apply_band_visit(
                    &mut metrics,
                    visit(indices_by_song(song0, i, song1, j, song2, k), score),
                ) {
                    return metrics;
                }
                shrink_end(&third_scores, &mut end, prefix, metrics.final_floor);
            }
            // Diagnostics retain the old sequential scan's logical positions,
            // including conflicts removed by the fixed first team.
            metrics.third_checks = metrics
                .third_checks
                .saturating_add(end.saturating_sub(counted_to) as u64);
        }
    }
    metrics
}

pub(super) fn enumerate_wide(
    input: &WideMedleyBandInput,
    visit: impl FnMut([usize; 3], BandScore) -> MedleyBandVisit,
) -> MedleyBandMetrics {
    macro_rules! run {
        ($words:literal) => {{
            let masks: Vec<[u64; $words]> = input
                .team_masks
                .iter()
                .map(|mask| std::array::from_fn(|word| mask[word]))
                .collect();
            enumerate(
                input.floor,
                &input.scores,
                &masks,
                MedleySolverImplementation::ScalarWide,
                visit,
            )
        }};
    }
    match input.team_masks.first().map(Vec::len) {
        Some(1) => run!(1),
        Some(2) => run!(2),
        Some(3) => run!(3),
        Some(4) => run!(4),
        _ => enumerate_band_wide_scalar(input, visit),
    }
}

pub(super) fn solve<const W: usize>(
    scores: &[[Score; 3]],
    masks: &[[u64; W]],
    current_best: Score,
    implementation: MedleySolverImplementation,
) -> ExactSearchOutcome {
    // Use maximum search's stable ties and song order, rather than re-sorting
    // with the band preparation's different tie semantics.
    let Some(search) = prepare_search(scores, current_best) else {
        return empty_outcome(current_best, implementation, 0);
    };
    let search = BandSearchPreparation {
        orders: search.orders,
        max_scores: search.max_scores.map(i64::from),
        song_order: search.song_order,
    };
    let scores: Vec<_> = scores.iter().map(|s| s.map(i64::from)).collect();
    let mut best = current_best;
    let mut indices = None;
    let metrics = enumerate_prepared(
        i64::from(current_best) + 1,
        &scores,
        masks,
        implementation,
        search,
        |plan, score| {
            best = Score::try_from(score).expect("maximum team score fits i32");
            indices = Some(plan);
            MedleyBandVisit::Continue { floor: score + 1 }
        },
    );
    outcome(
        best,
        indices,
        implementation,
        metrics
            .pair_checks
            .saturating_add(metrics.third_checks)
            .saturating_mul(W as u64),
    )
}

pub(super) fn solve_wide(input: &WideMedleySolverInput) -> ExactSearchOutcome {
    macro_rules! run {
        ($words:literal) => {{
            let masks: Vec<[u64; $words]> = input
                .team_masks
                .iter()
                .map(|mask| std::array::from_fn(|word| mask[word]))
                .collect();
            solve(
                &input.scores,
                &masks,
                input.current_best,
                MedleySolverImplementation::ScalarWide,
            )
        }};
    }
    match input.team_masks.first().map(Vec::len) {
        Some(1) => run!(1),
        Some(2) => run!(2),
        Some(3) => run!(3),
        Some(4) => run!(4),
        _ => solve_wide_scalar_internal(input, WorkMeter::unlimited()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_scan_preserves_callbacks_cutoffs_and_logical_counters() {
        let mut state = 0x41ac_0316_b593_892du64;
        for words in [1, 2, 3, 4, 6] {
            for n in [0, 1, 2, 3, 7, 31, 65] {
                for case in 0..10 {
                    let mut next = || {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        state
                    };
                    let masks: Vec<_> = (0..n)
                        .map(|_| {
                            let mut mask = vec![0; words];
                            if case != 0 {
                                for _ in 0..5 {
                                    let bit = next() as usize % (words * 64);
                                    mask[bit / 64] |= 1 << (bit % 64);
                                }
                            }
                            mask
                        })
                        .collect();
                    let input = WideMedleyBandInput {
                        floor: if case == 0 { i64::MIN } else { 80 },
                        team_masks: masks,
                        scores: (0..n)
                            .map(|_| {
                                std::array::from_fn(|_| {
                                    if case == 0 {
                                        i64::MAX
                                    } else {
                                        (next() % 101) as i64 - 20
                                    }
                                })
                            })
                            .collect(),
                    };
                    for mode in 0..3 {
                        let action = |count, score: i64| match mode {
                            1 => MedleyBandVisit::Continue {
                                floor: score.saturating_add(1),
                            },
                            2 if count == 3 => MedleyBandVisit::Break,
                            _ => MedleyBandVisit::Continue { floor: input.floor },
                        };
                        let mut reference = Vec::new();
                        let expected = enumerate_band_wide_scalar(&input, |indices, score| {
                            reference.push((indices, score));
                            action(reference.len(), score)
                        });
                        let mut actual = Vec::new();
                        let metrics = enumerate_wide(&input, |indices, score| {
                            actual.push((indices, score));
                            action(actual.len(), score)
                        });
                        assert_eq!(
                            actual, reference,
                            "words={words}, n={n}, case={case}, mode={mode}"
                        );
                        assert_eq!(metrics, expected);
                    }
                }
            }
        }
    }

    #[test]
    fn portable_maximum_preserves_the_original_winner_including_ties() {
        let mut state = 0x4981_01da_0183_990fu64;
        for words in [1, 2, 3, 4, 6] {
            for n in [0, 1, 2, 3, 7, 31, 65] {
                for case in 0..20 {
                    let mut next = || {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        state
                    };
                    let masks = (0..n)
                        .map(|_| {
                            let mut mask = vec![0; words];
                            if case != 0 {
                                for _ in 0..5 {
                                    let bit = next() as usize % (words * 64);
                                    mask[bit / 64] |= 1 << (bit % 64);
                                }
                            }
                            mask
                        })
                        .collect();
                    let input = WideMedleySolverInput {
                        current_best: if case < 2 { -1 } else { (next() % 302) as i32 },
                        team_masks: masks,
                        scores: (0..n)
                            .map(|_| {
                                std::array::from_fn(|_| {
                                    if case < 2 {
                                        0
                                    } else {
                                        (next() % 101) as i32 - 20
                                    }
                                })
                            })
                            .collect(),
                    };
                    let expected = solve_wide_scalar_internal(&input, WorkMeter::unlimited());
                    let actual = solve_wide(&input);
                    assert_eq!(
                        (actual.best_score, actual.best_indices),
                        (expected.best_score, expected.best_indices),
                        "words={words}, n={n}, case={case}"
                    );
                    assert_eq!(actual.work, expected.work);
                }
            }
        }
    }
}
