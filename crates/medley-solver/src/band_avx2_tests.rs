use super::*;

fn compare(input: WideMedleyBandInput) {
    if !avx2_available() {
        return;
    }
    for mode in 0..3 {
        let mut reference = None;
        for backend in 0..if input.team_masks.iter().all(|m| m.len() == 1) {
            9
        } else {
            5
        } {
            let mut visits = Vec::new();
            let mut visit = |indices, score: BandScore| {
                visits.push((indices, score));
                match mode {
                    1 => MedleyBandVisit::Continue {
                        floor: score.saturating_add(1),
                    },
                    2 if visits.len() == 3 => MedleyBandVisit::Break,
                    _ => MedleyBandVisit::Continue { floor: input.floor },
                }
            };
            let mut metrics = match backend {
                0 => enumerate_band_wide_scalar(&input, &mut visit),
                // SAFETY: the test checks runtime AVX2 support above.
                1 => unsafe { enumerate_band_wide_avx2_impl::<0>(&input, &mut visit) },
                2 => unsafe { enumerate_band_wide_avx2_impl::<1>(&input, &mut visit) },
                3 => unsafe { enumerate_band_wide_avx2_impl::<2>(&input, &mut visit) },
                4 => unsafe { enumerate_reuse_wide(&input, &mut visit) },
                _ => {
                    let narrow = MedleyBandInput {
                        floor: input.floor,
                        team_masks: input.team_masks.iter().map(|m| m[0]).collect(),
                        scores: input.scores.clone(),
                    };
                    // SAFETY: the test checks runtime AVX2 support above.
                    unsafe {
                        match backend {
                            5 => enumerate_band_avx2_impl::<0>(&narrow, &mut visit),
                            6 => enumerate_band_avx2_impl::<1>(&narrow, &mut visit),
                            7 => enumerate_band_avx2_impl::<2>(&narrow, &mut visit),
                            _ => enumerate_reuse_narrow(&narrow, &mut visit),
                        }
                    }
                }
            };
            metrics.implementation = MedleySolverImplementation::Scalar;
            let actual = (visits, metrics);
            if let Some(ref reference) = reference {
                assert_eq!(&actual, reference, "mode={mode}, backend={backend}");
            } else {
                reference = Some(actual);
            }
        }
    }
}

#[test]
fn common_card_certificate_is_stronger_than_independent_bounds() {
    // i = card 0, j = card 1. High-scoring thirds avoiding i must contain
    // card 1; the high-scoring third avoiding j contains card 0 instead.
    let masks = [1u64, 2, 2 | 4, 1 | 8, 2 | 16, 32];
    let order = [2, 3, 4, 5, 0, 1];
    let scores = [100, 99, 90, 50, 0, 0];
    let view = second_prune::Masks::Narrow(&masks);
    let mut bound = second_prune::Gate::<1>::new(view, &order, &scores);
    let mut common = second_prune::Gate::<2>::new(view, &order, &scores);
    assert_eq!(common.rejected_prefix_limit(0, 1, 0, 50), None);
    assert_eq!(bound.rejected_prefix_limit(0, 1, 0, 90), None);
    assert_eq!(common.rejected_prefix_limit(0, 1, 0, 90), Some(3));
    assert_eq!(bound.rejected_prefix_limit(0, 1, 0, 100), Some(1));
}

#[test]
fn collective_conflicts_without_a_common_card_remain_inconclusive() {
    let masks = [1u64, 2 | 4, 2 | 8, 1 | 32, 4 | 16, 64];
    let order = [2, 3, 4, 5, 0, 1];
    let scores = [100, 99, 90, 50, 0, 0];
    let mut common =
        second_prune::Gate::<2>::new(second_prune::Masks::Narrow(&masks), &order, &scores);
    // All eligible thirds are blocked, but different cards block each one.
    // The gate must defer to the exact AVX2 loop rather than invent a witness.
    assert_eq!(common.rejected_prefix_limit(0, 1, 0, 90), None);
}

#[test]
fn all_conflicting_batches_preserve_inclusive_cutoff_and_tail_counts() {
    let mut scores = vec![[110, 0, 0], [100, 0, 0], [0, 110, 0], [0, 100, 0]];
    scores.extend((0..12).map(|i| [0, 0, 80 - 3 * i]));
    let masks = [vec![1, 1, 2, 2], vec![3; 12]].concat();
    for floor in [
        0, 199, 200, 241, 242, 249, 250, 251, 259, 260, 261, 270, 299, 300, 301,
    ] {
        for wide in [false, true] {
            compare(WideMedleyBandInput {
                floor,
                scores: scores.clone(),
                team_masks: masks
                    .iter()
                    .map(|&mask| {
                        if wide {
                            vec![0, mask, 0, mask << 16]
                        } else {
                            vec![mask]
                        }
                    })
                    .collect(),
            });
        }
    }
}

#[test]
fn avx2_batch_skip_matches_scalar_visits_with_dynamic_floors() {
    let mut seed = 0x492c_39ad_e91f_6681u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for words in [1, 2, 3, 4, 7] {
        for n in [4, 5, 7, 8, 9, 31] {
            for _ in 0..12 {
                let mut masks = Vec::new();
                let mut scores = Vec::new();
                for _ in 0..n {
                    let mut mask = vec![0; words];
                    for _ in 0..5 {
                        let bit = next() as usize % (words * 64);
                        mask[bit / 64] |= 1 << (bit % 64);
                    }
                    masks.push(mask);
                    scores.push(std::array::from_fn(|_| (next() % 101) as i64 - 30));
                }
                compare(WideMedleyBandInput {
                    floor: 75,
                    team_masks: masks,
                    scores,
                });
            }
        }
    }
    compare(WideMedleyBandInput {
        floor: i64::MAX,
        team_masks: (0..17).map(|i| vec![1 << (i % 8)]).collect(),
        scores: (0..17)
            .map(|i| [i64::MAX - i, i64::MAX - i * 2, i64::MAX - i * 3])
            .collect(),
    });
}

#[test]
fn reused_prefix_handles_empty_masks_ties_and_immediate_floor_jumps() {
    for words in [1, 3, 5] {
        for floor in [-1, 0, 1, i64::MAX] {
            compare(WideMedleyBandInput {
                floor,
                team_masks: vec![vec![0; words]; 9],
                scores: vec![[0; 3]; 9],
            });
        }
        compare(WideMedleyBandInput {
            floor: 100,
            team_masks: (0..25)
                .map(|i| {
                    let mut mask = vec![0; words];
                    mask[i % words] = 1 << (i % 7);
                    mask
                })
                .collect(),
            scores: (0..25)
                .map(|i| [100 - i, 120 - i * 2, 130 - i * 3])
                .collect(),
        });
    }
}
