use crate::{
    avx2_available, MedleySolverError, MedleySolverImplementation, MedleySolverPlan, Score,
    TeamMask, WideMedleySolverInput, RANDOM_BUCKET_K, RANDOM_BUCKET_ROUNDS,
};

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

const RANDOM_BUCKET_SEED: u64 = 114_514;
const RANDOM_BUCKET_MASK_COUNT: usize = 1 << RANDOM_BUCKET_K;
const RANDOM_BUCKET_FULL_MASK: usize = RANDOM_BUCKET_MASK_COUNT - 1;
const INVALID_INDEX: usize = usize::MAX;
const INVALID_SCORE: Score = Score::MIN / 4;

pub(crate) fn solve_random_bucket_narrow(
    current_best: Score,
    team_masks: &[TeamMask],
    scores: &[[Score; 3]],
) -> Result<MedleySolverPlan, MedleySolverError> {
    let team_cards = team_masks
        .iter()
        .map(|&mask| card_indices_from_u64(mask))
        .collect::<Vec<_>>();
    solve_random_bucket(current_best, &team_cards, scores, RANDOM_BUCKET_ROUNDS)
}

pub(crate) fn solve_random_bucket_wide(
    input: &WideMedleySolverInput,
) -> Result<MedleySolverPlan, MedleySolverError> {
    solve_random_bucket_wide_with_rounds(input, RANDOM_BUCKET_ROUNDS)
}

pub(crate) fn solve_random_bucket_wide_with_rounds(
    input: &WideMedleySolverInput,
    rounds: usize,
) -> Result<MedleySolverPlan, MedleySolverError> {
    let team_cards = input
        .team_masks
        .iter()
        .map(|mask| card_indices_from_words(mask))
        .collect::<Vec<_>>();
    solve_random_bucket(input.current_best, &team_cards, &input.scores, rounds)
}

fn solve_random_bucket(
    current_best: Score,
    team_cards: &[Vec<usize>],
    scores: &[[Score; 3]],
    rounds: usize,
) -> Result<MedleySolverPlan, MedleySolverError> {
    let n = scores.len();
    if n == 0 {
        return Err(MedleySolverError::NoValidPlan);
    }

    let max_scores = max_scores_without_order(scores);
    if max_scores[0] + max_scores[1] + max_scores[2] <= current_best {
        return Err(MedleySolverError::NoValidPlan);
    }

    let card_count = team_cards
        .iter()
        .flat_map(|cards| cards.iter())
        .copied()
        .max()
        .map(|idx| idx + 1)
        .unwrap_or_default();
    if card_count == 0 {
        return Err(MedleySolverError::NoValidPlan);
    }

    let bucket_pairs = RandomBucketPairs::new();
    let use_avx2 = avx2_available();
    let mut rng = SplitMix64::new(RANDOM_BUCKET_SEED);
    let mut buckets_by_card = vec![0u16; card_count];
    let mut best_score_by_song = [[INVALID_SCORE; RANDOM_BUCKET_MASK_COUNT]; 3];
    let mut best_index_by_song = [[INVALID_INDEX; RANDOM_BUCKET_MASK_COUNT]; 3];
    let mut best2_score = [INVALID_SCORE; RANDOM_BUCKET_MASK_COUNT];
    let mut best2_index = [INVALID_INDEX; RANDOM_BUCKET_MASK_COUNT];

    let mut best_score = current_best;
    let mut best_indices = None;

    for _ in 0..rounds {
        for bucket in &mut buckets_by_card {
            *bucket = 1u16 << (rng.next_bounded(RANDOM_BUCKET_K as u64) as u16);
        }

        for song_idx in 0..3 {
            best_score_by_song[song_idx].fill(INVALID_SCORE);
            best_index_by_song[song_idx].fill(INVALID_INDEX);
        }

        for (candidate_idx, cards) in team_cards.iter().enumerate() {
            let mut bucket_mask = 0u16;
            for &card_idx in cards {
                bucket_mask |= buckets_by_card[card_idx];
            }
            if bucket_mask == 0 {
                continue;
            }

            let mask_idx = bucket_mask as usize;
            for song_idx in 0..3 {
                let score = scores[candidate_idx][song_idx];
                if score > best_score_by_song[song_idx][mask_idx] {
                    best_score_by_song[song_idx][mask_idx] = score;
                    best_index_by_song[song_idx][mask_idx] = candidate_idx;
                }
            }
        }

        best2_score.copy_from_slice(&best_score_by_song[2]);
        best2_index.copy_from_slice(&best_index_by_song[2]);
        maximize_song2_submasks(&mut best2_score, &mut best2_index);

        update_best_from_bucket_pairs(
            &bucket_pairs,
            &best_score_by_song,
            &best_index_by_song,
            &best2_score,
            &best2_index,
            team_cards,
            &mut best_score,
            &mut best_indices,
            use_avx2,
        );
    }

    let indices = best_indices.ok_or(MedleySolverError::NoValidPlan)?;
    Ok(MedleySolverPlan {
        score: best_score,
        indices,
        implementation: if use_avx2 {
            MedleySolverImplementation::RandomBucketAvx2
        } else {
            MedleySolverImplementation::RandomBucket
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn update_best_from_bucket_pairs(
    pairs: &RandomBucketPairs,
    best_score_by_song: &[[Score; RANDOM_BUCKET_MASK_COUNT]; 3],
    best_index_by_song: &[[usize; RANDOM_BUCKET_MASK_COUNT]; 3],
    best2_score: &[Score; RANDOM_BUCKET_MASK_COUNT],
    best2_index: &[usize; RANDOM_BUCKET_MASK_COUNT],
    team_cards: &[Vec<usize>],
    best_score: &mut Score,
    best_indices: &mut Option<[usize; 3]>,
    use_avx2: bool,
) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if use_avx2 {
            // SAFETY: runtime AVX2 support is checked by avx2_available().
            unsafe {
                update_best_from_bucket_pairs_avx2(
                    pairs,
                    best_score_by_song,
                    best_index_by_song,
                    best2_score,
                    best2_index,
                    team_cards,
                    best_score,
                    best_indices,
                );
            }
            return;
        }
    }

    let _ = use_avx2;
    update_best_from_bucket_pairs_scalar(
        pairs,
        best_score_by_song,
        best_index_by_song,
        best2_score,
        best2_index,
        team_cards,
        best_score,
        best_indices,
    );
}

#[allow(clippy::too_many_arguments)]
fn update_best_from_bucket_pairs_scalar(
    pairs: &RandomBucketPairs,
    best_score_by_song: &[[Score; RANDOM_BUCKET_MASK_COUNT]; 3],
    best_index_by_song: &[[usize; RANDOM_BUCKET_MASK_COUNT]; 3],
    best2_score: &[Score; RANDOM_BUCKET_MASK_COUNT],
    best2_index: &[usize; RANDOM_BUCKET_MASK_COUNT],
    team_cards: &[Vec<usize>],
    best_score: &mut Score,
    best_indices: &mut Option<[usize; 3]>,
) {
    for pair_idx in 0..pairs.len() {
        try_bucket_pair(
            pair_idx,
            pairs,
            best_score_by_song,
            best_index_by_song,
            best2_score,
            best2_index,
            team_cards,
            best_score,
            best_indices,
        );
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
#[allow(clippy::too_many_arguments)]
unsafe fn update_best_from_bucket_pairs_avx2(
    pairs: &RandomBucketPairs,
    best_score_by_song: &[[Score; RANDOM_BUCKET_MASK_COUNT]; 3],
    best_index_by_song: &[[usize; RANDOM_BUCKET_MASK_COUNT]; 3],
    best2_score: &[Score; RANDOM_BUCKET_MASK_COUNT],
    best2_index: &[usize; RANDOM_BUCKET_MASK_COUNT],
    team_cards: &[Vec<usize>],
    best_score: &mut Score,
    best_indices: &mut Option<[usize; 3]>,
) {
    let mut pair_idx = 0usize;
    while pair_idx + 8 <= pairs.len() {
        let song0_masks = _mm256_loadu_si256(pairs.song0.as_ptr().add(pair_idx) as *const __m256i);
        let song1_masks = _mm256_loadu_si256(pairs.song1.as_ptr().add(pair_idx) as *const __m256i);
        let song2_masks =
            _mm256_loadu_si256(pairs.song2_available.as_ptr().add(pair_idx) as *const __m256i);

        let score0 = _mm256_i32gather_epi32(best_score_by_song[0].as_ptr(), song0_masks, 4);
        let score1 = _mm256_i32gather_epi32(best_score_by_song[1].as_ptr(), song1_masks, 4);
        let score2 = _mm256_i32gather_epi32(best2_score.as_ptr(), song2_masks, 4);
        let score = _mm256_add_epi32(_mm256_add_epi32(score0, score1), score2);
        let better = _mm256_cmpgt_epi32(score, _mm256_set1_epi32(*best_score));
        let lanes = _mm256_movemask_ps(_mm256_castsi256_ps(better)) as u32;

        if lanes != 0 {
            let mut scores = [0i32; 8];
            _mm256_storeu_si256(scores.as_mut_ptr() as *mut __m256i, score);
            for lane in 0..8 {
                if (lanes & (1 << lane)) == 0 || scores[lane] <= *best_score {
                    continue;
                }
                try_bucket_pair_with_score(
                    pair_idx + lane,
                    scores[lane],
                    pairs,
                    best_score_by_song,
                    best_index_by_song,
                    best2_score,
                    best2_index,
                    team_cards,
                    best_score,
                    best_indices,
                );
            }
        }

        pair_idx += 8;
    }

    while pair_idx < pairs.len() {
        try_bucket_pair(
            pair_idx,
            pairs,
            best_score_by_song,
            best_index_by_song,
            best2_score,
            best2_index,
            team_cards,
            best_score,
            best_indices,
        );
        pair_idx += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn try_bucket_pair(
    pair_idx: usize,
    pairs: &RandomBucketPairs,
    best_score_by_song: &[[Score; RANDOM_BUCKET_MASK_COUNT]; 3],
    best_index_by_song: &[[usize; RANDOM_BUCKET_MASK_COUNT]; 3],
    best2_score: &[Score; RANDOM_BUCKET_MASK_COUNT],
    best2_index: &[usize; RANDOM_BUCKET_MASK_COUNT],
    team_cards: &[Vec<usize>],
    best_score: &mut Score,
    best_indices: &mut Option<[usize; 3]>,
) {
    let song0_mask = pairs.song0[pair_idx] as usize;
    let song1_mask = pairs.song1[pair_idx] as usize;
    let song2_available_mask = pairs.song2_available[pair_idx] as usize;
    let score0 = best_score_by_song[0][song0_mask];
    let score1 = best_score_by_song[1][song1_mask];
    let score2 = best2_score[song2_available_mask];

    if score0 == INVALID_SCORE || score1 == INVALID_SCORE || score2 == INVALID_SCORE {
        return;
    }

    let score = score0 + score1 + score2;
    try_bucket_pair_with_score(
        pair_idx,
        score,
        pairs,
        best_score_by_song,
        best_index_by_song,
        best2_score,
        best2_index,
        team_cards,
        best_score,
        best_indices,
    );
}

#[allow(clippy::too_many_arguments)]
fn try_bucket_pair_with_score(
    pair_idx: usize,
    score: Score,
    pairs: &RandomBucketPairs,
    best_score_by_song: &[[Score; RANDOM_BUCKET_MASK_COUNT]; 3],
    best_index_by_song: &[[usize; RANDOM_BUCKET_MASK_COUNT]; 3],
    best2_score: &[Score; RANDOM_BUCKET_MASK_COUNT],
    best2_index: &[usize; RANDOM_BUCKET_MASK_COUNT],
    team_cards: &[Vec<usize>],
    best_score: &mut Score,
    best_indices: &mut Option<[usize; 3]>,
) {
    if score <= *best_score {
        return;
    }

    let song0_mask = pairs.song0[pair_idx] as usize;
    let song1_mask = pairs.song1[pair_idx] as usize;
    let song2_available_mask = pairs.song2_available[pair_idx] as usize;
    if best_score_by_song[0][song0_mask] == INVALID_SCORE
        || best_score_by_song[1][song1_mask] == INVALID_SCORE
        || best2_score[song2_available_mask] == INVALID_SCORE
    {
        return;
    }

    let indices = [
        best_index_by_song[0][song0_mask],
        best_index_by_song[1][song1_mask],
        best2_index[song2_available_mask],
    ];
    if indices.iter().any(|&idx| idx == INVALID_INDEX)
        || !candidate_card_sets_are_disjoint(team_cards, indices)
    {
        return;
    }

    *best_score = score;
    *best_indices = Some(indices);
}

fn maximize_song2_submasks(scores: &mut [Score; RANDOM_BUCKET_MASK_COUNT], indices: &mut [usize]) {
    for bit in 0..RANDOM_BUCKET_K {
        let bit_mask = 1usize << bit;
        for mask in 0..RANDOM_BUCKET_MASK_COUNT {
            if mask & bit_mask == 0 {
                continue;
            }
            let submask = mask ^ bit_mask;
            if scores[submask] > scores[mask] {
                scores[mask] = scores[submask];
                indices[mask] = indices[submask];
            }
        }
    }
}

struct RandomBucketPairs {
    song0: Vec<i32>,
    song1: Vec<i32>,
    song2_available: Vec<i32>,
}

impl RandomBucketPairs {
    fn new() -> Self {
        let mut song0 = Vec::with_capacity(3usize.pow(RANDOM_BUCKET_K as u32));
        let mut song1 = Vec::with_capacity(3usize.pow(RANDOM_BUCKET_K as u32));
        let mut song2_available = Vec::with_capacity(3usize.pow(RANDOM_BUCKET_K as u32));
        for song0_mask in 1..RANDOM_BUCKET_MASK_COUNT {
            let remaining = RANDOM_BUCKET_FULL_MASK ^ song0_mask;
            let mut song1_mask = remaining;
            loop {
                if song1_mask != 0 {
                    song0.push(song0_mask as i32);
                    song1.push(song1_mask as i32);
                    song2_available.push((remaining ^ song1_mask) as i32);
                }
                if song1_mask == 0 {
                    break;
                }
                song1_mask = (song1_mask - 1) & remaining;
            }
        }

        Self {
            song0,
            song1,
            song2_available,
        }
    }

    fn len(&self) -> usize {
        self.song0.len()
    }
}

fn candidate_card_sets_are_disjoint(team_cards: &[Vec<usize>], indices: [usize; 3]) -> bool {
    pair_card_sets_are_disjoint(&team_cards[indices[0]], &team_cards[indices[1]])
        && pair_card_sets_are_disjoint(&team_cards[indices[0]], &team_cards[indices[2]])
        && pair_card_sets_are_disjoint(&team_cards[indices[1]], &team_cards[indices[2]])
}

fn pair_card_sets_are_disjoint(left: &[usize], right: &[usize]) -> bool {
    left.iter().all(|card_idx| !right.contains(card_idx))
}

fn card_indices_from_u64(mut mask: u64) -> Vec<usize> {
    let mut indices = Vec::new();
    while mask != 0 {
        let bit_idx = mask.trailing_zeros() as usize;
        indices.push(bit_idx);
        mask &= mask - 1;
    }
    indices
}

fn card_indices_from_words(words: &[u64]) -> Vec<usize> {
    let mut indices = Vec::new();
    for (word_idx, &word) in words.iter().enumerate() {
        let mut word = word;
        while word != 0 {
            let bit_idx = word.trailing_zeros() as usize;
            indices.push(word_idx * u64::BITS as usize + bit_idx);
            word &= word - 1;
        }
    }
    indices
}

fn max_scores_without_order(scores: &[[Score; 3]]) -> [Score; 3] {
    let mut max_scores = [0; 3];
    for score in scores {
        for song_idx in 0..3 {
            max_scores[song_idx] = max_scores[song_idx].max(score[song_idx]);
        }
    }
    max_scores
}

#[derive(Debug, Clone, Copy)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn next_bounded(&mut self, upper_bound: u64) -> u64 {
        self.next_u64() % upper_bound
    }
}
