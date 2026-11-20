//! One conflict traversal over immutable upper bounds. Mutable refinements can
//! only rule out more triples; raising the actual floor never reopens a prefix.
use super::*;

pub(super) fn solve(
    mut upper: WideMedleySolverInput,
    max_stages: usize,
    warm: Option<[usize; 3]>,
    compatible_bounds: bool,
    mut refine: impl FnMut(usize, usize, usize) -> Result<MedleyRefinement, SearchError>,
) -> Result<LazyMedleyResult, LazyMedleyError> {
    let possible_entries = upper.scores.len() * 3;
    let mut stages = vec![[0usize; 3]; upper.scores.len()];
    let mut known = vec![[false; 3]; upper.scores.len()];
    let mut infeasible = vec![[false; 3]; upper.scores.len()];
    let mut scored_entries = 0;
    let mut upper_refinements = 0;
    let mut best = -1;
    let mut winner = None;
    let trace = std::env::var_os("BANGDREAM_OPTIMIZE_TIMING_TRACE").is_some();
    let started = crate::timing::Instant::now();
    let mut visits = 0_u64;
    let mut last_trace = started;

    // No caller score is trusted: the seed passes through the same refinement
    // callback to an exact value before it can exclude another triple.
    if let Some(indices) = warm {
        refine_triple(
            indices,
            max_stages,
            &mut upper.scores,
            &mut stages,
            &mut known,
            &mut infeasible,
            &mut scored_entries,
            &mut upper_refinements,
            &mut best,
            &mut winner,
            &mut refine,
        )?;
    }
    // The snapshot stays immutable, so sorting and score cutoffs remain valid.
    // Refinements below only lower scores; the real incumbent only increases.
    let active = if compatible_bounds {
        let start = crate::timing::Instant::now();
        let indices = compatible_candidates(&upper, i64::from(best) + 1);
        if trace {
            eprintln!(
                "timing companion filter: {} -> {}, {:.3}s",
                upper.scores.len(),
                indices.len(),
                start.elapsed().as_secs_f64()
            );
        }
        indices
    } else {
        (0..upper.scores.len()).collect()
    };
    let band = WideMedleyBandInput {
        floor: i64::from(best) + 1,
        team_masks: active
            .iter()
            .map(|&i| upper.team_masks[i].clone())
            .collect(),
        scores: active
            .iter()
            .map(|&i| upper.scores[i].map(i64::from))
            .collect(),
    };
    let mut failure = None;
    let metrics = enumerate_medley_band_wide(&band, |indices, _| {
        let indices = indices.map(|i| active[i]);
        visits += 1;
        let result = refine_triple(
            indices,
            max_stages,
            &mut upper.scores,
            &mut stages,
            &mut known,
            &mut infeasible,
            &mut scored_entries,
            &mut upper_refinements,
            &mut best,
            &mut winner,
            &mut refine,
        );
        if let Err(error) = result {
            failure = Some(error);
            return MedleyBandVisit::Break;
        }
        if trace && (visits <= 10 || last_trace.elapsed().as_secs() >= 15) {
            eprintln!("timing streaming visit {visits}: incumbent {best}, fine {upper_refinements}, exact {scored_entries}, {:.3}s", started.elapsed().as_secs_f64());
            last_trace = crate::timing::Instant::now();
        }
        MedleyBandVisit::Continue {
            floor: i64::from(best) + 1,
        }
    })?;
    if let Some(error) = failure {
        return Err(error);
    }
    debug_assert!(!metrics.stopped_early);
    let indices = winner.ok_or(MedleySolverError::NoValidPlan)?;
    Ok(LazyMedleyResult {
        plan: MedleySolverPlan {
            indices,
            score: best,
            implementation: metrics.implementation,
            quality: MedleySolverQuality::Exact,
            exact_work: metrics.pair_checks.saturating_add(metrics.third_checks),
            auto_route: None,
        },
        scored_entries,
        possible_entries,
        solver_runs: 1,
        upper_refinements,
    })
}

#[allow(clippy::too_many_arguments)]
fn refine_triple(
    indices: [usize; 3],
    max_stages: usize,
    scores: &mut [[i32; 3]],
    stages: &mut [[usize; 3]],
    known: &mut [[bool; 3]],
    infeasible: &mut [[bool; 3]],
    scored_entries: &mut usize,
    upper_refinements: &mut usize,
    best: &mut i32,
    winner: &mut Option<[usize; 3]>,
    refine: &mut impl FnMut(usize, usize, usize) -> Result<MedleyRefinement, SearchError>,
) -> Result<(), LazyMedleyError> {
    loop {
        if indices.iter().enumerate().any(|(s, &c)| infeasible[c][s]) { return Ok(()); }
        let total: i32 = indices.iter().enumerate().map(|(s, &c)| scores[c][s]).sum();
        if total <= *best {
            return Ok(());
        }
        if indices.iter().enumerate().all(|(s, &c)| known[c][s]) {
            *best = total;
            *winner = Some(indices);
            return Ok(());
        }
        let stage = indices
            .iter()
            .enumerate()
            .filter(|&(s, &c)| !known[c][s])
            .map(|(s, &c)| stages[c][s])
            .min()
            .unwrap();
        for (song, &candidate) in indices.iter().enumerate() {
            if known[candidate][song] || stages[candidate][song] != stage {
                continue;
            }
            let refinement = refine(candidate, song, stage)?;
            stages[candidate][song] += 1;
            let (actual, exact) = match refinement {
                MedleyRefinement::Upper(v) => (v, false),
                MedleyRefinement::Exact(v) => (v, true),
                MedleyRefinement::Infeasible => {
                    infeasible[candidate][song] = true;
                    known[candidate][song] = true;
                    scores[candidate][song] = 0;
                    *scored_entries += 1;
                    return Ok(());
                }
            };
            if !exact && stages[candidate][song] >= max_stages {
                return Err(LazyMedleyError::MissingExactScore);
            }
            let upper = scores[candidate][song];
            if actual < 0 {
                return Err(LazyMedleyError::InvalidInput);
            }
            if actual > upper {
                return Err(LazyMedleyError::Underestimated {
                    candidate,
                    song,
                    actual,
                    upper,
                });
            }
            scores[candidate][song] = actual;
            known[candidate][song] = exact;
            if exact {
                *scored_entries += 1;
            } else {
                *upper_refinements += 1;
            }
            let total: i32 = indices.iter().enumerate().map(|(s, &c)| scores[c][s]).sum();
            if total <= *best {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_refinement_matches_exhaustive_and_staged_reference() {
        let mut state = 64321_u32;
        for words in [1, 4, 6, 8] {
            for sample in 0..40 {
                let mut next = || {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    state
                };
                let actual: Vec<[i32; 3]> = (0..22)
                    .map(|_| {
                        std::array::from_fn(|_| {
                            if sample == 0 {
                                0
                            } else {
                                (next() % 1000) as i32
                            }
                        })
                    })
                    .collect();
                let fine: Vec<_> = actual
                    .iter()
                    .map(|r| r.map(|v| v + (next() % 40) as i32))
                    .collect();
                let coarse: Vec<_> = fine
                    .iter()
                    .map(|r| r.map(|v| v + (next() % 400) as i32))
                    .collect();
                let masks: Vec<_> = (0..22)
                    .map(|i| {
                        let mut mask = vec![0_u64; words];
                        for part in 0..5 {
                            mask[(i + part) % words] |= 1 << (10 * part + i % (part + 4));
                        }
                        mask
                    })
                    .collect();
                let expected = solve_medley_wide_with(
                    &WideMedleySolverInput {
                        current_best: -1,
                        team_masks: masks.clone(),
                        scores: actual.clone(),
                    },
                    MedleySolverPreference::Scalar,
                )
                .unwrap();
                for policy in [
                    CandidatePolicy::StreamCompetitive,
                    CandidatePolicy::StreamCompetitiveFrom([0, 1, 2]),
                    CandidatePolicy::StreamCompatibleFrom(Some([0, 1, 2])),
                ] {
                    let mut calls = vec![[0; 3]; masks.len()];
                    let result = solve_staged_with_policy(
                        WideMedleySolverInput {
                            current_best: i32::MAX,
                            team_masks: masks.clone(),
                            scores: coarse.clone(),
                        },
                        2,
                        policy,
                        |c, s, stage| {
                            assert_eq!(calls[c][s], stage);
                            calls[c][s] += 1;
                            Ok(if stage == 0 {
                                MedleyRefinement::Upper(fine[c][s])
                            } else {
                                MedleyRefinement::Exact(actual[c][s])
                            })
                        },
                    )
                    .unwrap();
                    assert_eq!(
                        result.plan.score, expected.score,
                        "words={words}, sample={sample}"
                    );
                    assert_eq!(result.plan.quality, MedleySolverQuality::Exact);
                    assert_eq!(result.solver_runs, 1);
                    assert!(result.scored_entries <= actual.len() * 3);
                    assert_eq!(
                        result.plan.score,
                        result
                            .plan
                            .indices
                            .iter()
                            .enumerate()
                            .map(|(s, &c)| actual[c][s])
                            .sum::<i32>()
                    );
                }
            }
        }
    }

    #[test]
    fn streamed_fine_rejection_skips_unneeded_expensive_scores() {
        let coarse = vec![[100; 3], [100; 3], [100; 3], [150; 3], [150; 3], [150; 3]];
        let mut expensive = Vec::new();
        let result = solve_staged_with_policy(
            WideMedleySolverInput {
                current_best: 999999,
                team_masks: (0..6).map(|i| vec![1 << i]).collect(),
                scores: coarse,
            },
            2,
            CandidatePolicy::StreamCompetitiveFrom([0, 1, 2]),
            |c, s, stage| {
                if stage == 1 {
                    expensive.push((c, s));
                }
                let value = if c < 3 { 100 } else { 0 };
                Ok(if stage == 0 {
                    MedleyRefinement::Upper(value)
                } else {
                    MedleyRefinement::Exact(value)
                })
            },
        )
        .unwrap();
        assert_eq!(result.plan.score, 300);
        assert_eq!(expensive, vec![(0, 0), (1, 1), (2, 2)]);
    }

    #[test]
    fn streamed_errors_do_not_return_an_unproved_incumbent() {
        let input = WideMedleySolverInput {
            current_best: -1,
            team_masks: vec![vec![1], vec![2], vec![4]],
            scores: vec![[5; 3]; 3],
        };
        for policy in [
            CandidatePolicy::StreamCompetitive,
            CandidatePolicy::StreamCompetitiveFrom([0, 1, 2]),
        ] {
            assert!(matches!(
                solve_staged_with_policy(input.clone(), 1, policy, |_, _, _| Ok(
                    MedleyRefinement::Upper(4)
                )),
                Err(LazyMedleyError::MissingExactScore)
            ));
            assert!(matches!(
                solve_staged_with_policy(input.clone(), 1, policy, |_, _, _| Ok(
                    MedleyRefinement::Exact(6)
                )),
                Err(LazyMedleyError::Underestimated { .. })
            ));
            assert!(matches!(
                solve_staged_with_policy(input.clone(), 1, policy, |_, _, _| Ok(
                    MedleyRefinement::Exact(-1)
                )),
                Err(LazyMedleyError::InvalidInput)
            ));
        }
        for seed in [[0, 0, 2], [0, 1, 3]] {
            assert!(matches!(
                solve_staged_with_policy(
                    input.clone(),
                    1,
                    CandidatePolicy::StreamCompetitiveFrom(seed),
                    |_, _, _| panic!("bad seed")
                ),
                Err(LazyMedleyError::InvalidInput)
            ));
        }
        let conflicting = WideMedleySolverInput {
            team_masks: vec![vec![1]; 3],
            ..input
        };
        assert!(matches!(
            solve_staged_with_policy(
                conflicting,
                1,
                CandidatePolicy::StreamCompetitive,
                |_, _, _| panic!("no triple")
            ),
            Err(LazyMedleyError::Solver(MedleySolverError::NoValidPlan))
        ));
    }

    #[test]
    fn a_later_scoring_failure_cannot_publish_an_exact_warm_incumbent() {
        let input = WideMedleySolverInput {
            current_best: -1,
            team_masks: (0..6).map(|i| vec![1 << i]).collect(),
            scores: vec![[10; 3]; 6],
        };
        for policy in [
            CandidatePolicy::FirstCompetitiveFrom([0, 1, 2]),
            CandidatePolicy::StreamCompetitiveFrom([0, 1, 2]),
            CandidatePolicy::StreamCompatibleFrom(Some([0, 1, 2])),
        ] {
            let mut calls = Vec::new();
            let result = solve_staged_with_policy(input.clone(), 1, policy, |team, song, _| {
                calls.push((team, song));
                if team >= 3 {
                    return Err(SearchError::Options("injected exact scoring failure"));
                }
                Ok(MedleyRefinement::Exact(5))
            });
            // A feasible exact seed worth 15 is established before another
            // competitive candidate fails. Its existence must not mask failure.
            assert_eq!(&calls[..3], &[(0, 0), (1, 1), (2, 2)]);
            assert!(calls.len() > 3);
            assert!(matches!(
                result,
                Err(LazyMedleyError::Search(SearchError::Options(
                    "injected exact scoring failure"
                )))
            ));
        }
    }
}
