use crate::{
    avx2_available, MedleySolverError, MedleySolverImplementation, MedleySolverInput,
    MedleySolverPlan, Score, TeamMask, WideMedleySolverInput,
};

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

pub(crate) fn solve_avx2(input: &MedleySolverInput) -> Result<MedleySolverPlan, MedleySolverError> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if !avx2_available() {
            return Err(MedleySolverError::Avx2Unavailable);
        }

        // SAFETY: runtime AVX2 support is checked above. The SIMD path compares
        // four u64 masks at a time, so every narrow TeamMask is supported.
        unsafe { solve_avx2_x86(input) }
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
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if !avx2_available() {
            return Err(MedleySolverError::Avx2Unavailable);
        }

        // SAFETY: runtime AVX2 support is checked above. Wide masks have
        // already been validated to use the same word count.
        unsafe { solve_wide_avx2_x86(input) }
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        let _ = input;
        Err(MedleySolverError::Avx2Unavailable)
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn solve_avx2_x86(input: &MedleySolverInput) -> Result<MedleySolverPlan, MedleySolverError> {
    let n = input.scores.len();
    if n == 0 {
        return Err(MedleySolverError::NoValidPlan);
    }

    let order = sorted_orders(input);
    let max_scores = max_scores(input, &order);

    if max_scores[0] + max_scores[1] + max_scores[2] <= input.current_best {
        return Err(MedleySolverError::NoValidPlan);
    }

    let masks = &input.team_masks;
    let song2_masks: Vec<TeamMask> = order[2].iter().map(|&idx| masks[idx]).collect();
    let song2_scores: Vec<Score> = order[2].iter().map(|&idx| input.scores[idx][2]).collect();

    let mut best_score = input.current_best;
    let mut best_indices: Option<[usize; 3]> = None;
    let zero = _mm256_setzero_si256();

    for &i in &order[0] {
        let score_i = input.scores[i][0];
        if score_i + max_scores[1] + max_scores[2] <= best_score {
            break;
        }

        for &j in &order[1] {
            let score_ij = score_i + input.scores[j][1];
            if score_ij + max_scores[2] <= best_score {
                break;
            }

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
                        best_indices = Some([i, j, order[2][k_sorted_pos]]);
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

                if used & song2_masks[k_pos] == 0 {
                    best_score = score;
                    best_indices = Some([i, j, order[2][k_pos]]);
                    break;
                }

                k_pos += 1;
            }
        }
    }

    let indices = best_indices.ok_or(MedleySolverError::NoValidPlan)?;
    Ok(MedleySolverPlan {
        score: best_score,
        indices,
        implementation: MedleySolverImplementation::Avx2,
    })
}

pub(crate) fn solve_scalar(
    input: &MedleySolverInput,
    implementation: MedleySolverImplementation,
) -> Result<MedleySolverPlan, MedleySolverError> {
    let n = input.scores.len();
    if n == 0 {
        return Err(MedleySolverError::NoValidPlan);
    }

    let mut order = sorted_orders(input);
    let max_scores = max_scores(input, &order);

    if max_scores[0] + max_scores[1] + max_scores[2] <= input.current_best {
        return Err(MedleySolverError::NoValidPlan);
    }

    let mut best_score = input.current_best;
    let mut best_indices: Option<[usize; 3]> = None;

    for &i in &order[0] {
        let score_i = input.scores[i][0];
        if score_i + max_scores[1] + max_scores[2] <= best_score {
            break;
        }

        for &j in &order[1] {
            let score_ij = score_i + input.scores[j][1];
            if score_ij + max_scores[2] <= best_score {
                break;
            }

            if input.team_masks[i] & input.team_masks[j] != 0 {
                continue;
            }

            let used_ij = input.team_masks[i] | input.team_masks[j];
            for &k in &order[2] {
                let score = score_ij + input.scores[k][2];
                if score <= best_score {
                    break;
                }

                if used_ij & input.team_masks[k] != 0 {
                    continue;
                }

                best_score = score;
                best_indices = Some([i, j, k]);
                break;
            }
        }
    }

    let indices = best_indices.ok_or(MedleySolverError::NoValidPlan)?;
    order.iter_mut().for_each(Vec::clear);

    Ok(MedleySolverPlan {
        score: best_score,
        indices,
        implementation,
    })
}

pub(crate) fn solve_wide_scalar(
    input: &WideMedleySolverInput,
) -> Result<MedleySolverPlan, MedleySolverError> {
    let n = input.scores.len();
    if n == 0 {
        return Err(MedleySolverError::NoValidPlan);
    }

    let mut order = sorted_orders_for_scores(&input.scores);
    let max_scores = max_scores_for_scores(&input.scores, &order);

    if max_scores[0] + max_scores[1] + max_scores[2] <= input.current_best {
        return Err(MedleySolverError::NoValidPlan);
    }

    let mut best_score = input.current_best;
    let mut best_indices: Option<[usize; 3]> = None;

    for &i in &order[0] {
        let score_i = input.scores[i][0];
        if score_i + max_scores[1] + max_scores[2] <= best_score {
            break;
        }

        for &j in &order[1] {
            let score_ij = score_i + input.scores[j][1];
            if score_ij + max_scores[2] <= best_score {
                break;
            }

            if wide_masks_overlap(&input.team_masks[i], &input.team_masks[j]) {
                continue;
            }

            for &k in &order[2] {
                let score = score_ij + input.scores[k][2];
                if score <= best_score {
                    break;
                }

                if wide_mask_overlaps_pair(
                    &input.team_masks[i],
                    &input.team_masks[j],
                    &input.team_masks[k],
                ) {
                    continue;
                }

                best_score = score;
                best_indices = Some([i, j, k]);
                break;
            }
        }
    }

    let indices = best_indices.ok_or(MedleySolverError::NoValidPlan)?;
    order.iter_mut().for_each(Vec::clear);

    Ok(MedleySolverPlan {
        score: best_score,
        indices,
        implementation: MedleySolverImplementation::ScalarWide,
    })
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn solve_wide_avx2_x86(
    input: &WideMedleySolverInput,
) -> Result<MedleySolverPlan, MedleySolverError> {
    let n = input.scores.len();
    if n == 0 {
        return Err(MedleySolverError::NoValidPlan);
    }

    let order = sorted_orders_for_scores(&input.scores);
    let max_scores = max_scores_for_scores(&input.scores, &order);

    if max_scores[0] + max_scores[1] + max_scores[2] <= input.current_best {
        return Err(MedleySolverError::NoValidPlan);
    }

    let word_count = input.team_masks.first().map(Vec::len).unwrap_or_default();
    let song2_masks_by_word: Vec<Vec<u64>> = (0..word_count)
        .map(|word_idx| {
            order[2]
                .iter()
                .map(|&idx| input.team_masks[idx][word_idx])
                .collect()
        })
        .collect();
    let song2_scores: Vec<Score> = order[2].iter().map(|&idx| input.scores[idx][2]).collect();

    let mut best_score = input.current_best;
    let mut best_indices: Option<[usize; 3]> = None;
    let mut used_words = vec![0u64; word_count];
    let zero = _mm256_setzero_si256();

    for &i in &order[0] {
        let score_i = input.scores[i][0];
        if score_i + max_scores[1] + max_scores[2] <= best_score {
            break;
        }

        for &j in &order[1] {
            let score_ij = score_i + input.scores[j][1];
            if score_ij + max_scores[2] <= best_score {
                break;
            }

            if wide_masks_overlap(&input.team_masks[i], &input.team_masks[j]) {
                continue;
            }

            for (word_idx, used_word) in used_words.iter_mut().enumerate() {
                *used_word = input.team_masks[i][word_idx] | input.team_masks[j][word_idx];
            }

            let mut k_pos = 0;
            while k_pos + 4 <= n {
                if score_ij + song2_scores[k_pos] <= best_score {
                    break;
                }

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
                        best_indices = Some([i, j, order[2][k_sorted_pos]]);
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

                if !wide_mask_overlaps_words(&used_words, &input.team_masks[order[2][k_pos]]) {
                    best_score = score;
                    best_indices = Some([i, j, order[2][k_pos]]);
                    break;
                }

                k_pos += 1;
            }
        }
    }

    let indices = best_indices.ok_or(MedleySolverError::NoValidPlan)?;
    Ok(MedleySolverPlan {
        score: best_score,
        indices,
        implementation: MedleySolverImplementation::Avx2Wide,
    })
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

fn sorted_orders(input: &MedleySolverInput) -> [Vec<usize>; 3] {
    sorted_orders_for_scores(&input.scores)
}

fn max_scores(input: &MedleySolverInput, order: &[Vec<usize>; 3]) -> [Score; 3] {
    max_scores_for_scores(&input.scores, order)
}

fn sorted_orders_for_scores(scores: &[[Score; 3]]) -> [Vec<usize>; 3] {
    std::array::from_fn(|song_idx| {
        let mut indices: Vec<usize> = (0..scores.len()).collect();
        indices.sort_by_key(|&idx| std::cmp::Reverse(scores[idx][song_idx]));
        indices
    })
}

fn max_scores_for_scores(scores: &[[Score; 3]], order: &[Vec<usize>; 3]) -> [Score; 3] {
    std::array::from_fn(|song_idx| {
        order[song_idx]
            .first()
            .map(|&idx| scores[idx][song_idx])
            .unwrap_or_default()
    })
}
