use crate::{
    avx2_available, MedleySolverError, MedleySolverImplementation, MedleySolverInput,
    MedleySolverPlan, MedleySolverQuality, Score, TeamMask, WideMedleySolverInput,
};

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

pub(crate) struct ExactSearchOutcome {
    pub(crate) best_score: Score,
    pub(crate) best_indices: Option<[usize; 3]>,
    pub(crate) implementation: MedleySolverImplementation,
    pub(crate) work: u64,
}

#[cfg(test)]
pub(crate) fn profile_narrow(
    input: &MedleySolverInput,
) -> Result<ExactSearchOutcome, MedleySolverError> {
    if avx2_available() {
        solve_avx2_internal(input, WorkMeter::unlimited())
    } else {
        Ok(solve_scalar_internal(
            input,
            MedleySolverImplementation::ScalarFallbackAvx2Unavailable,
            WorkMeter::unlimited(),
        ))
    }
}

#[cfg(test)]
pub(crate) fn profile_wide(
    input: &WideMedleySolverInput,
) -> Result<ExactSearchOutcome, MedleySolverError> {
    if avx2_available() {
        solve_wide_avx2_internal(input, WorkMeter::unlimited())
    } else {
        Ok(solve_wide_scalar_internal(input, WorkMeter::unlimited()))
    }
}

impl ExactSearchOutcome {
    pub(crate) fn into_plan(
        self,
        quality: MedleySolverQuality,
    ) -> Result<MedleySolverPlan, MedleySolverError> {
        Ok(MedleySolverPlan {
            score: self.best_score,
            indices: self.best_indices.ok_or(MedleySolverError::NoValidPlan)?,
            implementation: self.implementation,
            quality,
            exact_work: self.work,
            auto_route: None,
        })
    }
}

struct WorkMeter {
    used: u64,
}

impl WorkMeter {
    fn unlimited() -> Self {
        Self { used: 0 }
    }

    fn spend(&mut self, amount: u64) {
        self.used = self.used.saturating_add(amount);
    }
}

pub(crate) fn solve_avx2(input: &MedleySolverInput) -> Result<MedleySolverPlan, MedleySolverError> {
    let started = trace_start();
    let outcome = solve_avx2_internal(input, WorkMeter::unlimited())?;
    trace_exact_outcome("avx2", input.scores.len(), &outcome, started);
    outcome.into_plan(MedleySolverQuality::Exact)
}

fn solve_avx2_internal(
    input: &MedleySolverInput,
    meter: WorkMeter,
) -> Result<ExactSearchOutcome, MedleySolverError> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if !avx2_available() {
            return Err(MedleySolverError::Avx2Unavailable);
        }

        // SAFETY: runtime AVX2 support is checked above. The SIMD path compares
        // four u64 masks at a time, so every narrow TeamMask is supported.
        Ok(unsafe { solve_avx2_x86(input, meter) })
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        let _ = input;
        Err(MedleySolverError::Avx2Unavailable)
    }
}

pub(crate) fn solve_wide_avx2(
    input: &WideMedleySolverInput,
) -> Result<MedleySolverPlan, MedleySolverError> {
    let started = trace_start();
    let outcome = solve_wide_avx2_internal(input, WorkMeter::unlimited())?;
    trace_exact_outcome("avx2Wide", input.scores.len(), &outcome, started);
    outcome.into_plan(MedleySolverQuality::Exact)
}

fn solve_wide_avx2_internal(
    input: &WideMedleySolverInput,
    meter: WorkMeter,
) -> Result<ExactSearchOutcome, MedleySolverError> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if !avx2_available() {
            return Err(MedleySolverError::Avx2Unavailable);
        }

        // SAFETY: runtime AVX2 support is checked above. Wide masks have
        // already been validated to use the same word count.
        Ok(unsafe { solve_wide_avx2_x86(input, meter) })
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        let _ = input;
        Err(MedleySolverError::Avx2Unavailable)
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn solve_avx2_x86(input: &MedleySolverInput, mut meter: WorkMeter) -> ExactSearchOutcome {
    let Some(search) = prepare_search(&input.scores, input.current_best) else {
        return empty_outcome(
            input.current_best,
            MedleySolverImplementation::Avx2,
            meter.used,
        );
    };
    let [song0, song1, song2] = search.song_order;
    let n = search.orders[song2].len();

    let masks = &input.team_masks;
    let song2_masks: Vec<TeamMask> = search.orders[song2].iter().map(|&idx| masks[idx]).collect();
    let song2_scores: Vec<Score> = search.orders[song2]
        .iter()
        .map(|&idx| input.scores[idx][song2])
        .collect();

    let mut best_score = input.current_best;
    let mut best_indices: Option<[usize; 3]> = None;
    let zero = _mm256_setzero_si256();

    for &i in &search.orders[song0] {
        let score_i = input.scores[i][song0];
        if score_i + search.max_scores[song1] + search.max_scores[song2] <= best_score {
            break;
        }

        for &j in &search.orders[song1] {
            let score_ij = score_i + input.scores[j][song1];
            if score_ij + search.max_scores[song2] <= best_score {
                break;
            }
            meter.spend(1);

            if masks[i] & masks[j] != 0 {
                continue;
            }

            let used = masks[i] | masks[j];
            let used_vec = _mm256_set1_epi64x(used as i64);
            let mut k_pos = 0;

            while k_pos + 4 <= n {
                if score_ij + song2_scores[k_pos] <= best_score {
                    break;
                }

                meter.spend(1);
                let mask_vec =
                    _mm256_loadu_si256(song2_masks.as_ptr().add(k_pos) as *const __m256i);
                let overlap = _mm256_and_si256(used_vec, mask_vec);
                let disjoint = _mm256_cmpeq_epi64(overlap, zero);
                let lanes = _mm256_movemask_pd(_mm256_castsi256_pd(disjoint)) as u32;

                if lanes != 0 {
                    let lane = lanes.trailing_zeros() as usize;
                    let k_sorted_pos = k_pos + lane;
                    let score = score_ij + song2_scores[k_sorted_pos];
                    if score > best_score {
                        best_score = score;
                        best_indices = Some(indices_by_song(
                            song0,
                            i,
                            song1,
                            j,
                            song2,
                            search.orders[song2][k_sorted_pos],
                        ));
                    }
                    break;
                }

                k_pos += 4;
            }

            while k_pos < n {
                let score = score_ij + song2_scores[k_pos];
                if score <= best_score {
                    break;
                }

                meter.spend(1);
                if used & song2_masks[k_pos] == 0 {
                    best_score = score;
                    best_indices = Some(indices_by_song(
                        song0,
                        i,
                        song1,
                        j,
                        song2,
                        search.orders[song2][k_pos],
                    ));
                    break;
                }

                k_pos += 1;
            }
        }
    }

    outcome(
        best_score,
        best_indices,
        MedleySolverImplementation::Avx2,
        meter.used,
    )
}

pub(crate) fn solve_scalar(
    input: &MedleySolverInput,
    implementation: MedleySolverImplementation,
) -> Result<MedleySolverPlan, MedleySolverError> {
    let started = trace_start();
    let outcome = solve_scalar_internal(input, implementation, WorkMeter::unlimited());
    trace_exact_outcome("scalar", input.scores.len(), &outcome, started);
    outcome.into_plan(MedleySolverQuality::Exact)
}

fn solve_scalar_internal(
    input: &MedleySolverInput,
    implementation: MedleySolverImplementation,
    mut meter: WorkMeter,
) -> ExactSearchOutcome {
    let Some(search) = prepare_search(&input.scores, input.current_best) else {
        return empty_outcome(input.current_best, implementation, meter.used);
    };
    let [song0, song1, song2] = search.song_order;

    let mut best_score = input.current_best;
    let mut best_indices: Option<[usize; 3]> = None;

    for &i in &search.orders[song0] {
        let score_i = input.scores[i][song0];
        if score_i + search.max_scores[song1] + search.max_scores[song2] <= best_score {
            break;
        }

        for &j in &search.orders[song1] {
            let score_ij = score_i + input.scores[j][song1];
            if score_ij + search.max_scores[song2] <= best_score {
                break;
            }
            meter.spend(1);

            if input.team_masks[i] & input.team_masks[j] != 0 {
                continue;
            }

            let used_ij = input.team_masks[i] | input.team_masks[j];
            for &k in &search.orders[song2] {
                let score = score_ij + input.scores[k][song2];
                if score <= best_score {
                    break;
                }
                meter.spend(1);

                if used_ij & input.team_masks[k] != 0 {
                    continue;
                }

                best_score = score;
                best_indices = Some(indices_by_song(song0, i, song1, j, song2, k));
                break;
            }
        }
    }

    outcome(best_score, best_indices, implementation, meter.used)
}

pub(crate) fn solve_wide_scalar(
    input: &WideMedleySolverInput,
) -> Result<MedleySolverPlan, MedleySolverError> {
    let started = trace_start();
    let outcome = solve_wide_scalar_internal(input, WorkMeter::unlimited());
    trace_exact_outcome("scalarWide", input.scores.len(), &outcome, started);
    outcome.into_plan(MedleySolverQuality::Exact)
}

fn trace_start() -> Option<std::time::Instant> {
    std::env::var_os("BANGDREAM_OPTIMIZE_DP_TRACE")
        .is_some()
        .then(std::time::Instant::now)
}

fn trace_exact_outcome(
    kind: &str,
    candidate_count: usize,
    outcome: &ExactSearchOutcome,
    started: Option<std::time::Instant>,
) {
    if let Some(started) = started {
        eprintln!(
            "medley exact detail: kind={kind} candidates={candidate_count} exact_work={} found={} elapsed_ms={:.3}",
            outcome.work,
            outcome.best_indices.is_some(),
            started.elapsed().as_secs_f64() * 1000.0,
        );
    }
}

fn solve_wide_scalar_internal(
    input: &WideMedleySolverInput,
    mut meter: WorkMeter,
) -> ExactSearchOutcome {
    let implementation = MedleySolverImplementation::ScalarWide;
    let Some(search) = prepare_search(&input.scores, input.current_best) else {
        return empty_outcome(input.current_best, implementation, meter.used);
    };
    let [song0, song1, song2] = search.song_order;
    let mask_work = input.team_masks.first().map(Vec::len).unwrap_or(1).max(1) as u64;

    let mut best_score = input.current_best;
    let mut best_indices: Option<[usize; 3]> = None;

    for &i in &search.orders[song0] {
        let score_i = input.scores[i][song0];
        if score_i + search.max_scores[song1] + search.max_scores[song2] <= best_score {
            break;
        }

        for &j in &search.orders[song1] {
            let score_ij = score_i + input.scores[j][song1];
            if score_ij + search.max_scores[song2] <= best_score {
                break;
            }
            meter.spend(mask_work);

            if wide_masks_overlap(&input.team_masks[i], &input.team_masks[j]) {
                continue;
            }

            for &k in &search.orders[song2] {
                let score = score_ij + input.scores[k][song2];
                if score <= best_score {
                    break;
                }
                meter.spend(mask_work);

                if wide_mask_overlaps_pair(
                    &input.team_masks[i],
                    &input.team_masks[j],
                    &input.team_masks[k],
                ) {
                    continue;
                }

                best_score = score;
                best_indices = Some(indices_by_song(song0, i, song1, j, song2, k));
                break;
            }
        }
    }

    outcome(best_score, best_indices, implementation, meter.used)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn solve_wide_avx2_x86(
    input: &WideMedleySolverInput,
    mut meter: WorkMeter,
) -> ExactSearchOutcome {
    let implementation = MedleySolverImplementation::Avx2Wide;
    let Some(search) = prepare_search(&input.scores, input.current_best) else {
        return empty_outcome(input.current_best, implementation, meter.used);
    };
    let [song0, song1, song2] = search.song_order;
    let n = search.orders[song2].len();

    let word_count = input.team_masks.first().map(Vec::len).unwrap_or_default();
    let song2_masks_by_word: Vec<Vec<u64>> = (0..word_count)
        .map(|word_idx| {
            search.orders[song2]
                .iter()
                .map(|&idx| input.team_masks[idx][word_idx])
                .collect()
        })
        .collect();
    let song2_scores: Vec<Score> = search.orders[song2]
        .iter()
        .map(|&idx| input.scores[idx][song2])
        .collect();

    let mut best_score = input.current_best;
    let mut best_indices: Option<[usize; 3]> = None;
    let mut used_words = vec![0u64; word_count];
    let zero = _mm256_setzero_si256();

    for &i in &search.orders[song0] {
        let score_i = input.scores[i][song0];
        if score_i + search.max_scores[song1] + search.max_scores[song2] <= best_score {
            break;
        }

        for &j in &search.orders[song1] {
            let score_ij = score_i + input.scores[j][song1];
            if score_ij + search.max_scores[song2] <= best_score {
                break;
            }
            meter.spend(word_count.max(1) as u64);

            if wide_masks_overlap(&input.team_masks[i], &input.team_masks[j]) {
                continue;
            }

            for (word_idx, used_word) in used_words.iter_mut().enumerate() {
                *used_word = input.team_masks[i][word_idx] | input.team_masks[j][word_idx];
            }

            let mut k_pos = 0;
            let active_word_count =
                used_words.iter().filter(|&&word| word != 0).count().max(1) as u64;
            while k_pos + 4 <= n {
                if score_ij + song2_scores[k_pos] <= best_score {
                    break;
                }

                meter.spend(active_word_count);
                let mut overlap_any = zero;
                for (word_idx, &used_word) in used_words.iter().enumerate() {
                    if used_word == 0 {
                        continue;
                    }
                    let used_vec = _mm256_set1_epi64x(used_word as i64);
                    let mask_vec = _mm256_loadu_si256(
                        song2_masks_by_word[word_idx].as_ptr().add(k_pos) as *const __m256i,
                    );
                    overlap_any =
                        _mm256_or_si256(overlap_any, _mm256_and_si256(used_vec, mask_vec));
                }

                let disjoint = _mm256_cmpeq_epi64(overlap_any, zero);
                let lanes = _mm256_movemask_pd(_mm256_castsi256_pd(disjoint)) as u32;

                if lanes != 0 {
                    let lane = lanes.trailing_zeros() as usize;
                    let k_sorted_pos = k_pos + lane;
                    let score = score_ij + song2_scores[k_sorted_pos];
                    if score > best_score {
                        best_score = score;
                        best_indices = Some(indices_by_song(
                            song0,
                            i,
                            song1,
                            j,
                            song2,
                            search.orders[song2][k_sorted_pos],
                        ));
                    }
                    break;
                }

                k_pos += 4;
            }

            while k_pos < n {
                let score = score_ij + song2_scores[k_pos];
                if score <= best_score {
                    break;
                }

                meter.spend(active_word_count);
                if !wide_mask_overlaps_words(
                    &used_words,
                    &input.team_masks[search.orders[song2][k_pos]],
                ) {
                    best_score = score;
                    best_indices = Some(indices_by_song(
                        song0,
                        i,
                        song1,
                        j,
                        song2,
                        search.orders[song2][k_pos],
                    ));
                    break;
                }

                k_pos += 1;
            }
        }
    }

    outcome(best_score, best_indices, implementation, meter.used)
}

fn wide_masks_overlap(left: &[u64], right: &[u64]) -> bool {
    left.iter()
        .zip(right)
        .any(|(left, right)| left & right != 0)
}

fn wide_mask_overlaps_pair(left: &[u64], right: &[u64], target: &[u64]) -> bool {
    left.iter()
        .zip(right)
        .zip(target)
        .any(|((left, right), target)| (left | right) & target != 0)
}

fn wide_mask_overlaps_words(used_words: &[u64], target: &[u64]) -> bool {
    used_words
        .iter()
        .zip(target)
        .any(|(used, target)| used & target != 0)
}

struct SearchPreparation {
    orders: [Vec<usize>; 3],
    max_scores: [Score; 3],
    song_order: [usize; 3],
}

fn prepare_search(scores: &[[Score; 3]], current_best: Score) -> Option<SearchPreparation> {
    if scores.is_empty() {
        return None;
    }

    let mut orders: [Vec<usize>; 3] = std::array::from_fn(|song_idx| {
        let mut indices: Vec<usize> = (0..scores.len()).collect();
        indices.sort_by_key(|&idx| std::cmp::Reverse(scores[idx][song_idx]));
        indices
    });
    let max_scores = std::array::from_fn(|song_idx| scores[orders[song_idx][0]][song_idx]);
    if max_scores.iter().sum::<Score>() <= current_best {
        return None;
    }

    for song_idx in 0..3 {
        let other_max: Score = (0..3)
            .filter(|&other| other != song_idx)
            .map(|other| max_scores[other])
            .sum();
        orders[song_idx].retain(|&idx| scores[idx][song_idx] + other_max > current_best);
        if orders[song_idx].is_empty() {
            return None;
        }
    }

    const PERMUTATIONS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [1, 0, 2],
        [0, 2, 1],
        [2, 0, 1],
        [1, 2, 0],
        [2, 1, 0],
    ];
    let song_order = *PERMUTATIONS
        .iter()
        .min_by_key(|&&[first, second, query]| {
            (
                viable_pair_upper_bound(
                    scores,
                    &orders[first],
                    first,
                    &orders[second],
                    second,
                    max_scores[query],
                    current_best,
                ),
                orders[query].len(),
                [first, second, query],
            )
        })
        .expect("the six song permutations are non-empty");

    Some(SearchPreparation {
        orders,
        max_scores,
        song_order,
    })
}

#[allow(clippy::too_many_arguments)]
fn viable_pair_upper_bound(
    scores: &[[Score; 3]],
    first_order: &[usize],
    first_song: usize,
    second_order: &[usize],
    second_song: usize,
    query_max: Score,
    current_best: Score,
) -> u64 {
    let mut viable_second = second_order.len();
    let mut count = 0u64;
    for &first_idx in first_order {
        while viable_second > 0
            && scores[first_idx][first_song]
                + scores[second_order[viable_second - 1]][second_song]
                + query_max
                <= current_best
        {
            viable_second -= 1;
        }
        count = count.saturating_add(viable_second as u64);
    }
    count
}

fn indices_by_song(
    song0: usize,
    index0: usize,
    song1: usize,
    index1: usize,
    song2: usize,
    index2: usize,
) -> [usize; 3] {
    let mut indices = [0; 3];
    indices[song0] = index0;
    indices[song1] = index1;
    indices[song2] = index2;
    indices
}

fn empty_outcome(
    current_best: Score,
    implementation: MedleySolverImplementation,
    work: u64,
) -> ExactSearchOutcome {
    outcome(current_best, None, implementation, work)
}

fn outcome(
    best_score: Score,
    best_indices: Option<[usize; 3]>,
    implementation: MedleySolverImplementation,
    work: u64,
) -> ExactSearchOutcome {
    ExactSearchOutcome {
        best_score,
        best_indices,
        implementation,
        work,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_permutation_is_mapped_back_to_physical_song_positions() {
        let input = MedleySolverInput {
            current_best: 250,
            team_masks: vec![1, 2, 4, 8, 16],
            scores: vec![
                [100, 60, 0],
                [60, 100, 0],
                [60, 60, 100],
                [90, 90, 0],
                [80, 80, 0],
            ],
        };

        let search = prepare_search(&input.scores, input.current_best).unwrap();
        assert_ne!(search.song_order, [0, 1, 2]);
        let plan = solve_scalar(&input, MedleySolverImplementation::Scalar).unwrap();

        assert_eq!(plan.score, 300);
        assert_eq!(plan.indices, [0, 1, 2]);
    }

    #[test]
    fn filtered_permuted_scalar_matches_bruteforce() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            state ^= state << 7;
            state ^= state >> 9;
            state
        };

        for _ in 0..128 {
            let mut team_masks = Vec::new();
            let mut scores = Vec::new();
            for _ in 0..9 {
                let first = (next() % 12) as u32;
                let mut second = (next() % 12) as u32;
                if second == first {
                    second = (second + 1) % 12;
                }
                team_masks.push((1u64 << first) | (1u64 << second));
                scores.push([
                    (next() % 500) as Score,
                    (next() % 500) as Score,
                    (next() % 500) as Score,
                ]);
            }
            let current_best = (next() % 900) as Score;
            let input = MedleySolverInput {
                current_best,
                team_masks,
                scores,
            };

            let mut brute_score = current_best;
            for i in 0..input.scores.len() {
                for j in 0..input.scores.len() {
                    if input.team_masks[i] & input.team_masks[j] != 0 {
                        continue;
                    }
                    for k in 0..input.scores.len() {
                        if (input.team_masks[i] | input.team_masks[j]) & input.team_masks[k] != 0 {
                            continue;
                        }
                        brute_score = brute_score
                            .max(input.scores[i][0] + input.scores[j][1] + input.scores[k][2]);
                    }
                }
            }

            let result = solve_scalar(&input, MedleySolverImplementation::Scalar);
            if brute_score == current_best {
                assert!(matches!(result, Err(MedleySolverError::NoValidPlan)));
                if avx2_available() {
                    assert!(matches!(
                        solve_avx2(&input),
                        Err(MedleySolverError::NoValidPlan)
                    ));
                }
            } else {
                assert_eq!(result.unwrap().score, brute_score);
                if avx2_available() {
                    assert_eq!(solve_avx2(&input).unwrap().score, brute_score);
                }
            }
        }
    }
}
