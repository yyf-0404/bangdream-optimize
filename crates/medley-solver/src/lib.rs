use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(not(target_arch = "wasm32"))]
mod capture;
mod exact;
mod random_bucket;
#[cfg(test)]
mod threshold_benchmark;

pub type TeamMask = u64;
pub type Score = i32;
pub type WideTeamMask = Vec<u64>;

pub const AUTO_EXACT_CANDIDATE_THRESHOLD: usize = 196_608;
pub const RANDOM_BUCKET_K: usize = 10;
pub const RANDOM_BUCKET_ROUNDS: usize = 15_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MedleySolverInput {
    pub current_best: Score,
    pub team_masks: Vec<TeamMask>,
    pub scores: Vec<[Score; 3]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WideMedleySolverInput {
    pub current_best: Score,
    pub team_masks: Vec<WideTeamMask>,
    pub scores: Vec<[Score; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MedleySolverPreference {
    Auto,
    StrictExact,
    FastApproximate,
    Scalar,
    RandomBucket,
    Avx2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum MedleySolverQuality {
    #[default]
    Exact,
    Approximate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MedleySolverAutoRoute {
    ExactCandidateCount,
    RandomBucketCandidateCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MedleySolverImplementation {
    Scalar,
    ScalarWide,
    RandomBucket,
    RandomBucketAvx2,
    Avx2,
    Avx2Wide,
    ScalarFallbackAvx2Unavailable,
    ScalarFallbackAvx2Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MedleySolverPlan {
    pub score: Score,
    pub indices: [usize; 3],
    pub implementation: MedleySolverImplementation,
    #[serde(default)]
    pub quality: MedleySolverQuality,
    #[serde(default)]
    pub exact_work: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_route: Option<MedleySolverAutoRoute>,
}

#[derive(Debug, Error)]
pub enum MedleySolverError {
    #[error("team_masks length {team_masks} does not match scores length {scores}")]
    LengthMismatch { team_masks: usize, scores: usize },

    #[error("wide team mask {index} has {actual} words, expected {expected}")]
    WideMaskWordCountMismatch {
        index: usize,
        expected: usize,
        actual: usize,
    },

    #[error("no valid medley plan found")]
    NoValidPlan,

    #[error("AVX2 is not available on this target")]
    Avx2Unavailable,
}

pub fn avx2_available() -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        std::is_x86_feature_detected!("avx2")
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        false
    }
}

pub fn solve_medley(input: &MedleySolverInput) -> Result<MedleySolverPlan, MedleySolverError> {
    solve_medley_with(input, MedleySolverPreference::Auto)
}

pub fn solve_medley_with(
    input: &MedleySolverInput,
    preference: MedleySolverPreference,
) -> Result<MedleySolverPlan, MedleySolverError> {
    validate(input)?;
    #[cfg(not(target_arch = "wasm32"))]
    capture::maybe_capture_narrow(input);

    match preference {
        MedleySolverPreference::Scalar => {
            exact::solve_scalar(input, MedleySolverImplementation::Scalar)
        }
        MedleySolverPreference::RandomBucket | MedleySolverPreference::FastApproximate => {
            random_bucket::solve_random_bucket_narrow(
                input.current_best,
                &input.team_masks,
                &input.scores,
            )
        }
        MedleySolverPreference::StrictExact if avx2_available() => exact::solve_avx2(input),
        MedleySolverPreference::StrictExact => exact::solve_scalar(
            input,
            MedleySolverImplementation::ScalarFallbackAvx2Unavailable,
        ),
        MedleySolverPreference::Auto => solve_medley_auto(input),
        MedleySolverPreference::Avx2 => exact::solve_avx2(input),
    }
}

pub fn solve_medley_wide_with(
    input: &WideMedleySolverInput,
    preference: MedleySolverPreference,
) -> Result<MedleySolverPlan, MedleySolverError> {
    validate_wide(input)?;
    #[cfg(not(target_arch = "wasm32"))]
    capture::maybe_capture_wide(input);
    match preference {
        MedleySolverPreference::Scalar => exact::solve_wide_scalar(input),
        MedleySolverPreference::Avx2 => exact::solve_wide_avx2(input),
        MedleySolverPreference::RandomBucket | MedleySolverPreference::FastApproximate => {
            random_bucket::solve_random_bucket_wide(input)
        }
        MedleySolverPreference::StrictExact if avx2_available() => exact::solve_wide_avx2(input),
        MedleySolverPreference::StrictExact => exact::solve_wide_scalar(input),
        MedleySolverPreference::Auto => solve_medley_wide_auto(input),
    }
}

fn solve_medley_auto(input: &MedleySolverInput) -> Result<MedleySolverPlan, MedleySolverError> {
    let route = select_auto_route(input.scores.len());
    match route {
        MedleySolverAutoRoute::RandomBucketCandidateCount => annotate_auto_plan(
            random_bucket::solve_random_bucket_narrow(
                input.current_best,
                &input.team_masks,
                &input.scores,
            )?,
            route,
        ),
        MedleySolverAutoRoute::ExactCandidateCount => {
            let plan = if avx2_available() {
                exact::solve_avx2(input)?
            } else {
                exact::solve_scalar(
                    input,
                    MedleySolverImplementation::ScalarFallbackAvx2Unavailable,
                )?
            };
            annotate_auto_plan(plan, route)
        }
    }
}

fn solve_medley_wide_auto(
    input: &WideMedleySolverInput,
) -> Result<MedleySolverPlan, MedleySolverError> {
    let route = select_auto_route(input.scores.len());
    match route {
        MedleySolverAutoRoute::RandomBucketCandidateCount => {
            annotate_auto_plan(random_bucket::solve_random_bucket_wide(input)?, route)
        }
        MedleySolverAutoRoute::ExactCandidateCount => {
            let plan = if avx2_available() {
                exact::solve_wide_avx2(input)?
            } else {
                exact::solve_wide_scalar(input)?
            };
            annotate_auto_plan(plan, route)
        }
    }
}

fn select_auto_route(candidate_count: usize) -> MedleySolverAutoRoute {
    if candidate_count > AUTO_EXACT_CANDIDATE_THRESHOLD {
        MedleySolverAutoRoute::RandomBucketCandidateCount
    } else {
        MedleySolverAutoRoute::ExactCandidateCount
    }
}

fn annotate_auto_plan(
    mut plan: MedleySolverPlan,
    route: MedleySolverAutoRoute,
) -> Result<MedleySolverPlan, MedleySolverError> {
    plan.auto_route = Some(route);
    Ok(plan)
}

fn validate(input: &MedleySolverInput) -> Result<(), MedleySolverError> {
    if input.team_masks.len() != input.scores.len() {
        return Err(MedleySolverError::LengthMismatch {
            team_masks: input.team_masks.len(),
            scores: input.scores.len(),
        });
    }

    Ok(())
}

fn validate_wide(input: &WideMedleySolverInput) -> Result<(), MedleySolverError> {
    if input.team_masks.len() != input.scores.len() {
        return Err(MedleySolverError::LengthMismatch {
            team_masks: input.team_masks.len(),
            scores: input.scores.len(),
        });
    }

    let expected = input.team_masks.first().map(Vec::len).unwrap_or_default();
    for (index, mask) in input.team_masks.iter().enumerate() {
        if mask.len() != expected {
            return Err(MedleySolverError::WideMaskWordCountMismatch {
                index,
                expected,
                actual: mask.len(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_selects_non_overlapping_best_plan() {
        let input = MedleySolverInput {
            current_best: 0,
            team_masks: vec![0b00001, 0b00010, 0b00100, 0b00011, 0b01000],
            scores: vec![
                [100, 1, 1],
                [1, 100, 1],
                [1, 1, 100],
                [98, 98, 1],
                [80, 80, 80],
            ],
        };

        let plan = solve_medley_with(&input, MedleySolverPreference::Scalar).unwrap();

        assert_eq!(plan.score, 300);
        assert_eq!(plan.indices, [0, 1, 2]);
        assert_eq!(plan.implementation, MedleySolverImplementation::Scalar);
        assert_eq!(plan.quality, MedleySolverQuality::Exact);
        assert!(plan.exact_work > 0);
        assert_eq!(plan.auto_route, None);
    }

    #[test]
    fn scalar_rejects_overlapping_best_scores() {
        let input = MedleySolverInput {
            current_best: 0,
            team_masks: vec![0b00001, 0b00001, 0b00010, 0b00100],
            scores: vec![[100, 100, 100], [90, 90, 90], [1, 80, 1], [1, 1, 80]],
        };

        let plan = solve_medley_with(&input, MedleySolverPreference::Scalar).unwrap();

        assert_eq!(plan.indices, [0, 2, 3]);
        assert_eq!(plan.score, 260);
    }

    #[test]
    fn avx2_matches_scalar_when_available() {
        let input = MedleySolverInput {
            current_best: 0,
            team_masks: vec![
                0b0000000001,
                0b0000000010,
                0b0000000100,
                0b0000001000,
                0b0000010000,
                0b0000100000,
                0b0001000000,
                0b0010000000,
                0b0100000000,
                0b1000000000,
                0b0000000011,
                0b0000001100,
            ],
            scores: vec![
                [1000, 5, 7],
                [4, 900, 8],
                [3, 6, 800],
                [980, 820, 5],
                [970, 4, 790],
                [2, 880, 780],
                [950, 860, 760],
                [940, 840, 740],
                [930, 830, 720],
                [920, 810, 700],
                [990, 910, 9],
                [8, 905, 795],
            ],
        };

        let scalar = solve_medley_with(&input, MedleySolverPreference::Scalar).unwrap();

        if avx2_available() {
            let avx2 = solve_medley_with(&input, MedleySolverPreference::Avx2).unwrap();
            assert_eq!(avx2.score, scalar.score);
            assert_eq!(avx2.indices, scalar.indices);
            assert_eq!(avx2.implementation, MedleySolverImplementation::Avx2);
        } else {
            assert!(matches!(
                solve_medley_with(&input, MedleySolverPreference::Avx2),
                Err(MedleySolverError::Avx2Unavailable)
            ));
        }
    }

    #[test]
    fn auto_uses_avx2_for_u64_masks() {
        let input = MedleySolverInput {
            current_best: 0,
            team_masks: vec![1 << 40, 0b10, 0b100],
            scores: vec![[100, 1, 1], [1, 100, 1], [1, 1, 100]],
        };

        let plan = solve_medley(&input).unwrap();

        assert_eq!(plan.score, 300);
        assert_eq!(plan.indices, [0, 1, 2]);
        assert_eq!(plan.quality, MedleySolverQuality::Exact);
        assert_eq!(
            plan.auto_route,
            Some(MedleySolverAutoRoute::ExactCandidateCount)
        );
        if avx2_available() {
            assert_eq!(plan.implementation, MedleySolverImplementation::Avx2);
        } else {
            assert_eq!(
                plan.implementation,
                MedleySolverImplementation::ScalarFallbackAvx2Unavailable
            );
        }
    }

    #[test]
    fn strict_exact_is_additive_to_legacy_kernel_preferences() {
        let input = MedleySolverInput {
            current_best: 0,
            team_masks: vec![1, 2, 4],
            scores: vec![[100, 1, 1], [1, 100, 1], [1, 1, 100]],
        };

        let plan = solve_medley_with(&input, MedleySolverPreference::StrictExact).unwrap();

        assert_eq!(plan.score, 300);
        assert_eq!(plan.quality, MedleySolverQuality::Exact);
        assert_eq!(plan.auto_route, None);
    }

    #[test]
    fn auto_route_uses_the_candidate_count_boundary() {
        assert_eq!(
            select_auto_route(AUTO_EXACT_CANDIDATE_THRESHOLD),
            MedleySolverAutoRoute::ExactCandidateCount
        );
        assert_eq!(
            select_auto_route(AUTO_EXACT_CANDIDATE_THRESHOLD + 1),
            MedleySolverAutoRoute::RandomBucketCandidateCount
        );
    }

    #[test]
    fn wide_auto_uses_avx2_when_available() {
        let input = WideMedleySolverInput {
            current_best: 0,
            team_masks: vec![
                vec![0, 0b0001],
                vec![0, 0b0010],
                vec![0, 0b0100],
                vec![0, 0b0001],
            ],
            scores: vec![[100, 1, 1], [1, 100, 1], [1, 1, 100], [90, 90, 90]],
        };

        let plan = solve_medley_wide_with(&input, MedleySolverPreference::Auto).unwrap();

        assert_eq!(plan.score, 300);
        assert_eq!(plan.indices, [0, 1, 2]);
        if avx2_available() {
            assert_eq!(plan.implementation, MedleySolverImplementation::Avx2Wide);
        } else {
            assert_eq!(plan.implementation, MedleySolverImplementation::ScalarWide);
        }
    }

    #[test]
    fn wide_avx2_matches_scalar_when_available() {
        let input = WideMedleySolverInput {
            current_best: 0,
            team_masks: vec![
                vec![0b00001, 0, 0],
                vec![0b00010, 0, 0],
                vec![0b00100, 0, 0],
                vec![0b01000, 1 << 12, 0],
                vec![0b10000, 1 << 13, 0],
                vec![0b00011, 0, 1 << 8],
                vec![0, 1 << 40, 0],
                vec![0, 0, 1 << 35],
            ],
            scores: vec![
                [1000, 5, 7],
                [4, 900, 8],
                [3, 6, 800],
                [980, 820, 5],
                [970, 4, 790],
                [990, 910, 9],
                [2, 880, 780],
                [950, 860, 760],
            ],
        };

        let scalar = solve_medley_wide_with(&input, MedleySolverPreference::Scalar).unwrap();
        assert_eq!(
            scalar.implementation,
            MedleySolverImplementation::ScalarWide
        );

        if avx2_available() {
            let avx2 = solve_medley_wide_with(&input, MedleySolverPreference::Avx2).unwrap();
            assert_eq!(avx2.score, scalar.score);
            assert_eq!(avx2.indices, scalar.indices);
            assert_eq!(avx2.implementation, MedleySolverImplementation::Avx2Wide);
        } else {
            assert!(matches!(
                solve_medley_wide_with(&input, MedleySolverPreference::Avx2),
                Err(MedleySolverError::Avx2Unavailable)
            ));
        }
    }

    #[test]
    fn random_bucket_supports_wide_masks() {
        let input = WideMedleySolverInput {
            current_best: 0,
            team_masks: vec![
                vec![0, 0b0001],
                vec![0, 0b0010],
                vec![0, 0b0100],
                vec![0, 0b0001],
            ],
            scores: vec![[100, 1, 1], [1, 100, 1], [1, 1, 100], [90, 90, 90]],
        };

        let plan = random_bucket::solve_random_bucket_wide_with_rounds(&input, 128).unwrap();

        assert_eq!(plan.score, 300);
        assert_eq!(plan.indices, [0, 1, 2]);
        if avx2_available() {
            assert_eq!(
                plan.implementation,
                MedleySolverImplementation::RandomBucketAvx2
            );
        } else {
            assert_eq!(
                plan.implementation,
                MedleySolverImplementation::RandomBucket
            );
        }
    }
}
