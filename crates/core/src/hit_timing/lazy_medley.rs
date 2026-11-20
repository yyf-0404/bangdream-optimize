//! Refine optimistic three-team solutions instead of preparing every candidate.
//! Exact over the caller's finite candidate pool and deterministic scoring
//! callback. This is not a proof of globally optimal hit timing or team coverage.
use super::search::SearchError;
mod companions;
mod streamed;
use bangdream_optimize_medley_solver::{
    enumerate_medley_band_wide, solve_medley_wide_with, MedleyBandVisit, MedleySolverError,
    MedleySolverPlan, MedleySolverPreference, MedleySolverQuality, WideMedleyBandInput,
    WideMedleySolverInput,
};
pub use companions::compatible_candidates;

#[derive(Debug, thiserror::Error)]
pub enum LazyMedleyError {
    #[error(transparent)]
    Solver(#[from] MedleySolverError),
    #[error(transparent)]
    Search(#[from] SearchError),
    #[error("lazy medley requires nonempty masks and scores in 0..=i32::MAX/3")]
    InvalidInput,
    #[error("the final refinement stage must return an exact score")]
    MissingExactScore,
    #[error(
        "candidate {candidate}, song {song}: score {actual} exceeds declared upper bound {upper}"
    )]
    Underestimated {
        candidate: usize,
        song: usize,
        actual: i32,
        upper: i32,
    },
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LazyMedleyResult {
    pub plan: MedleySolverPlan,
    pub scored_entries: usize,
    pub possible_entries: usize,
    pub solver_runs: usize,
    pub upper_refinements: usize,
}

/// A tighter certified upper bound, or the final deterministic candidate score.
pub enum MedleyRefinement {
    Upper(i32),
    Exact(i32),
    /// Supported by the exhaustive streaming policies. Not a zero score.
    Infeasible,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CandidatePolicy {
    HighestUpper,
    /// Stop at the first competitive upper triple, but certify exhaustion
    /// against the actual incumbent before returning a final result.
    FirstCompetitive,
    /// Only indices are accepted, never a trusted score. All seed entries are
    /// refined to exact values by the same callback before setting a floor.
    FirstCompetitiveFrom([usize; 3]),
    /// Traverse a fixed certified upper snapshot once. Callback scores may
    /// tighten individual entries and raise the actual incumbent, never lower it.
    StreamCompetitive,
    StreamCompetitiveFrom([usize; 3]),
    StreamCompatibleFrom(Option<[usize; 3]>),
}

/// The input scores MUST upper-bound the callback under the same chart, skill
/// order, power and objective. Only the upper-bound inputs are sent to the
/// existing strict solver; approximate solver routes cannot certify termination.
/// Entries not chosen by an optimistic winning triple are never scored.
pub fn solve(
    upper: WideMedleySolverInput,
    mut score: impl FnMut(usize, usize) -> Result<i32, SearchError>,
) -> Result<LazyMedleyResult, LazyMedleyError> {
    solve_staged(upper, 1, |c, s, _| score(c, s).map(MedleyRefinement::Exact))
}

/// Optional coarse -> fine -> exact stages. An optimistic triple is never an
/// incumbent until all three entries are exact. Equal bounds may advance one
/// stage, but the bounded final stage must return Exact, guaranteeing termination.
pub fn solve_staged(
    upper: WideMedleySolverInput,
    max_stages: usize,
    refine: impl FnMut(usize, usize, usize) -> Result<MedleyRefinement, SearchError>,
) -> Result<LazyMedleyResult, LazyMedleyError> {
    solve_staged_with_policy(upper, max_stages, CandidatePolicy::HighestUpper, refine)
}

pub fn solve_staged_with_policy(
    mut upper: WideMedleySolverInput,
    max_stages: usize,
    policy: CandidatePolicy,
    mut refine: impl FnMut(usize, usize, usize) -> Result<MedleyRefinement, SearchError>,
) -> Result<LazyMedleyResult, LazyMedleyError> {
    let warm = match policy {
        CandidatePolicy::FirstCompetitiveFrom(indices)
        | CandidatePolicy::StreamCompetitiveFrom(indices) => Some(indices),
        CandidatePolicy::StreamCompatibleFrom(indices) => indices,
        _ => None,
    };
    if max_stages == 0
        || upper.team_masks.len() != upper.scores.len()
        || upper.team_masks.iter().any(|m| !m.iter().any(|&w| w != 0))
        || upper
            .scores
            .iter()
            .flatten()
            .any(|&v| !(0..=i32::MAX / 3).contains(&v))
    {
        return Err(LazyMedleyError::InvalidInput);
    }
    if let Some(indices) = warm {
        if indices.iter().any(|&i| i >= upper.scores.len())
            || (0..3).any(|a| {
                (a + 1..3).any(|b| {
                    let x = &upper.team_masks[indices[a]];
                    let y = &upper.team_masks[indices[b]];
                    x.len() != y.len() || x.iter().zip(y).any(|(x, y)| x & y != 0)
                })
            })
        {
            return Err(LazyMedleyError::InvalidInput);
        }
    }
    if matches!(
        policy,
        CandidatePolicy::StreamCompetitive
            | CandidatePolicy::StreamCompetitiveFrom(_)
            | CandidatePolicy::StreamCompatibleFrom(_)
    ) {
        return streamed::solve(
            upper,
            max_stages,
            warm,
            matches!(policy, CandidatePolicy::StreamCompatibleFrom(_)),
            refine,
        );
    }
    // No unverified caller incumbent can exclude a real solution. Negative one
    // lets the strict solver find a valid zero-score triple as well.
    upper.current_best = -1;
    let mut band = (policy != CandidatePolicy::HighestUpper).then(|| WideMedleyBandInput {
        floor: 0,
        team_masks: upper.team_masks.clone(),
        scores: upper.scores.iter().map(|r| r.map(i64::from)).collect(),
    });
    let trace = std::env::var_os("BANGDREAM_OPTIMIZE_TIMING_TRACE").is_some();
    let possible_entries = upper.scores.len() * 3;
    let mut known = vec![[false; 3]; upper.scores.len()];
    let mut stages = vec![[0usize; 3]; upper.scores.len()];
    let mut scored_entries = 0;
    let mut solver_runs = 0;
    let mut upper_refinements = 0;
    let mut incumbent: Option<MedleySolverPlan> = None;
    let mut warm = warm;
    let mut last_search = None;
    loop {
        let force_exact = warm.is_some();
        if !force_exact {
            solver_runs += 1;
        }
        let started = crate::timing::Instant::now();
        let next = if let Some(indices) = warm.take() {
            Ok(MedleySolverPlan {
                indices,
                score: indices
                    .iter()
                    .enumerate()
                    .map(|(s, &i)| upper.scores[i][s])
                    .sum(),
                // This seed selection does not perform a search. Final quality
                // comes only from exhaustive verification after real scoring.
                implementation:
                    bangdream_optimize_medley_solver::MedleySolverImplementation::Scalar,
                quality: MedleySolverQuality::Approximate,
                exact_work: 0,
                auto_route: None,
            })
        } else if let Some(band) = &mut band {
            band.floor = i64::from(upper.current_best) + 1;
            let mut found = None;
            let metrics = enumerate_medley_band_wide(band, |indices, score| {
                found = Some((indices, score));
                MedleyBandVisit::Break
            })?;
            last_search = Some((
                metrics.implementation,
                metrics.pair_checks.saturating_add(metrics.third_checks),
            ));
            found
                .map(|(indices, score)| MedleySolverPlan {
                    indices,
                    score: score as i32,
                    implementation: metrics.implementation,
                    quality: MedleySolverQuality::Approximate,
                    exact_work: metrics.pair_checks.saturating_add(metrics.third_checks),
                    auto_route: None,
                })
                .ok_or(MedleySolverError::NoValidPlan)
        } else {
            solve_medley_wide_with(&upper, MedleySolverPreference::StrictExact)
        };
        if trace {
            eprintln!(
                "timing triple query {solver_runs}: incumbent {}, found {}, {:.3}s",
                upper.current_best,
                next.is_ok(),
                started.elapsed().as_secs_f64()
            );
        }
        let optimistic = match next {
            Ok(plan) => plan,
            Err(MedleySolverError::NoValidPlan) if incumbent.is_some() => {
                let mut plan = incumbent.unwrap();
                // A first-hit traversal is not optimal on its own. The final
                // exhaustive miss above is the certificate for this incumbent.
                plan.quality = MedleySolverQuality::Exact;
                if let Some((implementation, work)) = last_search {
                    plan.implementation = implementation;
                    plan.exact_work = work;
                }
                return Ok(LazyMedleyResult {
                    plan,
                    scored_entries,
                    possible_entries,
                    solver_runs,
                    upper_refinements,
                });
            }
            Err(e) => return Err(e.into()),
        };
        let mut changed = false;
        for (song, &candidate) in optimistic.indices.iter().enumerate() {
            while !known[candidate][song] {
                let refinement = refine(candidate, song, stages[candidate][song])?;
                stages[candidate][song] += 1;
                let (actual, exact) = match refinement {
                    MedleyRefinement::Upper(value) => (value, false),
                    MedleyRefinement::Exact(value) => (value, true),
                    MedleyRefinement::Infeasible => return Err(SearchError::Options(
                        "infeasible candidates require a streaming policy").into()),
                };
                if !exact && stages[candidate][song] >= max_stages {
                    return Err(LazyMedleyError::MissingExactScore);
                }
                let bound = upper.scores[candidate][song];
                if actual < 0 {
                    return Err(LazyMedleyError::InvalidInput);
                }
                if actual > bound {
                    return Err(LazyMedleyError::Underestimated {
                        candidate,
                        song,
                        actual,
                        upper: bound,
                    });
                }
                upper.scores[candidate][song] = actual;
                if let Some(band) = &mut band {
                    band.scores[candidate][song] = i64::from(actual);
                }
                known[candidate][song] = exact;
                if exact {
                    scored_entries += 1;
                } else {
                    upper_refinements += 1;
                }
                changed = true;
                if !force_exact {
                    break;
                }
            }
        }
        if optimistic
            .indices
            .iter()
            .enumerate()
            .any(|(song, &c)| !known[c][song])
        {
            continue;
        }
        let actual = optimistic
            .indices
            .iter()
            .enumerate()
            .map(|(song, &c)| upper.scores[c][song])
            .sum();
        let plan = MedleySolverPlan {
            score: actual,
            ..optimistic
        };
        if !changed && policy == CandidatePolicy::HighestUpper {
            return Ok(LazyMedleyResult {
                plan,
                scored_entries,
                possible_entries,
                solver_runs,
                upper_refinements,
            });
        }
        if incumbent.as_ref().is_none_or(|p| actual > p.score) {
            upper.current_best = actual;
            incumbent = Some(plan);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_hint_is_fully_scored_before_search_and_never_trusts_a_caller_score() {
        let input = WideMedleySolverInput {
            current_best: 999999,
            team_masks: (0..6).map(|i| vec![1 << i]).collect(),
            scores: vec![[1000; 3]; 6],
        };
        let actual = vec![[10; 3], [10; 3], [10; 3], [100; 3], [100; 3], [100; 3]];
        let mut calls = Vec::new();
        let result = solve_staged_with_policy(
            input.clone(),
            2,
            CandidatePolicy::FirstCompetitiveFrom([0, 1, 2]),
            |c, s, stage| {
                calls.push((c, s, stage));
                Ok(if stage == 0 {
                    MedleyRefinement::Upper(actual[c][s] + 1)
                } else {
                    MedleyRefinement::Exact(actual[c][s])
                })
            },
        )
        .unwrap();
        assert_eq!(
            &calls[..6],
            &[
                (0, 0, 0),
                (0, 0, 1),
                (1, 1, 0),
                (1, 1, 1),
                (2, 2, 0),
                (2, 2, 1)
            ]
        );
        assert_eq!(result.plan.score, 300);
        assert_eq!(result.plan.quality, MedleySolverQuality::Exact);
        for seed in [[0, 0, 2], [0, 1, 6]] {
            assert!(matches!(
                solve_staged_with_policy(
                    input.clone(),
                    1,
                    CandidatePolicy::FirstCompetitiveFrom(seed),
                    |_, _, _| panic!("invalid seed must not be scored")
                ),
                Err(LazyMedleyError::InvalidInput)
            ));
        }
    }

    #[test]
    fn first_competitive_is_not_finished_at_the_first_scored_triple() {
        let actual = vec![[10; 3], [10; 3], [10; 3], [100; 3], [100; 3], [100; 3]];
        let result = solve_staged_with_policy(
            WideMedleySolverInput {
                current_best: 99999,
                team_masks: (0..6).map(|i| vec![1 << i]).collect(),
                scores: vec![
                    [1000; 3], [1000; 3], [1000; 3], [100; 3], [100; 3], [100; 3],
                ],
            },
            1,
            CandidatePolicy::FirstCompetitive,
            |c, s, _| Ok(MedleyRefinement::Exact(actual[c][s])),
        )
        .unwrap();
        assert_eq!(result.plan.score, 300);
        assert_eq!(result.plan.quality, MedleySolverQuality::Exact);
        assert!(result.scored_entries >= 6);
    }

    #[test]
    fn first_competitive_matches_exhaustive_scores_in_wide_conflicting_pools() {
        let mut seed = 944_u32;
        for words in [1, 4, 6, 8] {
            for sample in 0..30 {
                let mut next = || {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    seed
                };
                let actual: Vec<[i32; 3]> = (0..20)
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
                    .map(|row| row.map(|v| v + (next() % 80) as i32))
                    .collect();
                let coarse: Vec<_> = fine
                    .iter()
                    .map(|row| row.map(|v| v + (next() % 1000) as i32))
                    .collect();
                let masks: Vec<_> = (0..20)
                    .map(|i| {
                        let mut m = vec![0_u64; words];
                        m[i % words] |= 1 << (i % 7);
                        m[words - 1] |= 1 << (i % 5);
                        m
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
                let result = solve_staged_with_policy(
                    WideMedleySolverInput {
                        current_best: 9999999,
                        team_masks: masks,
                        scores: coarse,
                    },
                    2,
                    if sample % 2 == 0 {
                        CandidatePolicy::FirstCompetitiveFrom([0, 1, 2])
                    } else {
                        CandidatePolicy::FirstCompetitive
                    },
                    |c, s, stage| {
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
                assert!(result.scored_entries <= 60);
            }
        }
    }

    #[test]
    fn lazy_scores_match_complete_solver_in_conflicting_pools() {
        let mut seed = 944u32;
        for sample in 0..50 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            };
            let mut actual = Vec::new();
            let mut bounds = Vec::new();
            let mut masks = Vec::new();
            for i in 0..20 {
                masks.push(vec![1u64 << (next() % 20), 1u64 << (i % 5)]);
                let row = std::array::from_fn(|_| (next() % 1000) as i32);
                bounds.push(row.map(|v| v + (next() % 150) as i32));
                actual.push(row);
            }
            let full = WideMedleySolverInput {
                current_best: -1,
                team_masks: masks.clone(),
                scores: actual.clone(),
            };
            let expected =
                solve_medley_wide_with(&full, MedleySolverPreference::StrictExact).unwrap();
            let result = solve(
                WideMedleySolverInput {
                    current_best: 999999,
                    team_masks: masks,
                    scores: bounds,
                },
                |c, s| Ok(actual[c][s]),
            )
            .unwrap();
            assert_eq!(result.plan.score, expected.score, "pool {sample}");
            assert!(result.scored_entries <= 60);
        }
    }

    #[test]
    fn does_not_discard_a_low_scoring_team_needed_for_disjointness() {
        let masks = vec![vec![1], vec![2], vec![4], vec![1 | 2 | 4], vec![8]];
        let actual = vec![[100; 3], [90; 3], [80; 3], [1000; 3], [1; 3]];
        let result = solve(
            WideMedleySolverInput {
                current_best: 0,
                team_masks: masks,
                scores: actual.clone(),
            },
            |c, s| Ok(actual[c][s]),
        )
        .unwrap();
        assert_eq!(result.plan.score, 270);
        assert_eq!(result.scored_entries, 3);
    }

    #[test]
    fn handles_zero_scores_and_rejects_underestimates() {
        let input = WideMedleySolverInput {
            current_best: 0,
            team_masks: vec![vec![1], vec![2], vec![4]],
            scores: vec![[0; 3]; 3],
        };
        assert_eq!(solve(input.clone(), |_, _| Ok(0)).unwrap().plan.score, 0);
        assert!(matches!(
            solve(input, |_, _| Ok(1)),
            Err(LazyMedleyError::Underestimated { .. })
        ));
    }

    #[test]
    fn staged_bounds_never_become_incumbents_and_match_eager_scores() {
        let mut seed = 944u32;
        for _ in 0..50 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            };
            let actual: Vec<[i32; 3]> = (0..20)
                .map(|_| std::array::from_fn(|_| (next() % 1000) as i32))
                .collect();
            let fine: Vec<_> = actual
                .iter()
                .map(|row| row.map(|v| v + (next() % 50) as i32))
                .collect();
            let coarse: Vec<_> = fine
                .iter()
                .map(|row| row.map(|v| v + (next() % 1000) as i32))
                .collect();
            let masks: Vec<_> = (0..20)
                .map(|i| vec![1u64 << (i % 15), 1u64 << (i % 7)])
                .collect();
            let exact = solve_medley_wide_with(
                &WideMedleySolverInput {
                    current_best: -1,
                    team_masks: masks.clone(),
                    scores: actual.clone(),
                },
                MedleySolverPreference::StrictExact,
            )
            .unwrap();
            let result = solve_staged(
                WideMedleySolverInput {
                    current_best: 999999,
                    team_masks: masks,
                    scores: coarse,
                },
                2,
                |c, s, stage| {
                    Ok(if stage == 0 {
                        MedleyRefinement::Upper(fine[c][s])
                    } else {
                        MedleyRefinement::Exact(actual[c][s])
                    })
                },
            )
            .unwrap();
            assert_eq!(result.plan.score, exact.score);
            assert!(result.upper_refinements >= result.scored_entries);
        }
        let zero = WideMedleySolverInput {
            current_best: 0,
            team_masks: vec![vec![1], vec![2], vec![4]],
            scores: vec![[0; 3]; 3],
        };
        assert_eq!(
            solve_staged(zero.clone(), 2, |_, _, stage| Ok(if stage == 0 {
                MedleyRefinement::Upper(0)
            } else {
                MedleyRefinement::Exact(0)
            }))
            .unwrap()
            .plan
            .score,
            0
        );
        assert!(matches!(
            solve_staged(zero, 2, |_, _, _| Ok(MedleyRefinement::Upper(0))),
            Err(LazyMedleyError::MissingExactScore)
        ));
    }
}
