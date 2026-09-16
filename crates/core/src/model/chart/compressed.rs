use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ScoreFactorRun {
    pub(super) start: usize,
    pub(super) end: usize,
    factor: f64,
}

pub(super) fn push_run(runs: &mut Vec<ScoreFactorRun>, index: usize, factor: f64) {
    if let Some(last) = runs.last_mut() {
        // Bit equality, not an epsilon: even tiny differences can cross a floor.
        if last.factor.to_bits() == factor.to_bits() {
            last.end = index + 1;
            return;
        }
    }
    runs.push(ScoreFactorRun {
        start: index,
        end: index + 1,
        factor,
    });
}

impl ExactScoreScratch {
    pub(crate) fn compressed() -> Self {
        #[cfg(feature = "experimental-compressed-score")]
        if std::env::var("BANGDREAM_OPTIMIZE_COMPRESSED_SCORE").as_deref() == Ok("off") {
            return Self::default();
        }
        Self {
            compressed: true,
            ..Self::default()
        }
    }
}

impl Chart {
    pub(super) fn compressed_independent_skill_score_matrix(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        windows: &[[ExactSkillWindow; 6]; 5],
        scratch: &mut ExactScoreScratch,
    ) -> Option<IndependentSkillScoreMatrix> {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        if native_kernel_enabled() && avx2_available() {
            // SAFETY: AVX2 is checked at runtime. Both paths inline the same
            // operations; no fast-math or fused multiply-add is requested.
            return unsafe {
                self.compressed_skill_matrix_avx2(team, stat, is_medley, windows, scratch)
            };
        }
        self.compressed_skill_matrix_impl(team, stat, is_medley, windows, scratch)
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[target_feature(enable = "avx2")]
    unsafe fn compressed_skill_matrix_avx2(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        windows: &[[ExactSkillWindow; 6]; 5],
        scratch: &mut ExactScoreScratch,
    ) -> Option<IndependentSkillScoreMatrix> {
        self.compressed_skill_matrix_impl(team, stat, is_medley, windows, scratch)
    }

    #[inline(always)]
    fn compressed_skill_matrix_impl(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        windows: &[[ExactSkillWindow; 6]; 5],
        scratch: &mut ExactScoreScratch,
    ) -> Option<IndependentSkillScoreMatrix> {
        if !scratch.compressed || self.fever_enabled || self.score_as_medley != is_medley {
            return None;
        }
        let runs = &self.score_factor_runs;
        if runs.is_empty() {
            return None;
        }
        scratch.base_scores.resize(runs.len(), 0);
        let mut base_score = 0;
        for (run, base) in runs.iter().zip(&mut scratch.base_scores) {
            *base = (stat as f64 * run.factor).floor() as i32;
            base_score += *base * (run.end - run.start) as i32;
        }
        let mut deltas = [[0; 6]; 5];
        for card in 0..5 {
            if let Some(previous) = (0..card).find(|&previous| {
                team[previous].duration.to_bits() == team[card].duration.to_bits()
                    && team[previous].score_up.to_bits() == team[card].score_up.to_bits()
                    && team[previous].rateup == team[card].rateup
            }) {
                deltas[card] = deltas[previous];
                continue;
            }
            let profile = &mut scratch.rateup_profiles[card];
            if team[card].rateup {
                let max_len = windows[card]
                    .iter()
                    .map(|window| window.range_end - window.range_start)
                    .max()
                    .unwrap_or(0);
                profile.prepare(team[card].score_up, max_len.saturating_add(3) & !3);
            }
            for (activation, window) in windows[card].iter().enumerate() {
                let mut delta = 0;
                for index in window.run_start..window.run_end {
                    let run = &runs[index];
                    let start = run.start.max(window.range_start);
                    let end = run.end.min(window.range_end);
                    let count = end - start;
                    let base = scratch.base_scores[index];
                    delta += if window.rateup {
                        // Rate-up changes once per note. Retain the original multiplier
                        // sequence and its floating-point recurrence, including the cap.
                        let offset = start - window.range_start;
                        constant_base_rateup_delta(
                            base,
                            &profile.multipliers[offset..offset + count],
                        )
                    } else {
                        // Both original floors stay inside the multiplication by count.
                        ((base as f64 * (1.0 + window.score_up)).floor() as i32 - base)
                            * count as i32
                    };
                }
                deltas[card][activation] = delta;
            }
        }
        Some(IndependentSkillScoreMatrix { base_score, deltas })
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn native_kernel_enabled() -> bool {
    #[cfg(feature = "experimental-compressed-native")]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var("BANGDREAM_OPTIMIZE_COMPRESSED_NATIVE").as_deref() != Ok("off")
        })
    }
    #[cfg(not(feature = "experimental-compressed-native"))]
    true
}

fn constant_base_rateup_delta(base: i32, multipliers: &[f64]) -> i32 {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if avx2_available() {
        // SAFETY: runtime AVX2 detection; the vector loop reads complete groups of four.
        return unsafe { constant_base_rateup_delta_avx2(base, multipliers) };
    }
    multipliers
        .iter()
        .map(|&multiplier| (base as f64 * multiplier).floor() as i32 - base)
        .sum()
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn constant_base_rateup_delta_avx2(base: i32, multipliers: &[f64]) -> i32 {
    let base_pd = _mm256_set1_pd(base as f64);
    let base_epi = _mm_set1_epi32(base);
    let mut sum = _mm_setzero_si128();
    let mut index = 0;
    while index + 4 <= multipliers.len() {
        let values = _mm256_loadu_pd(multipliers.as_ptr().add(index));
        let scored = _mm256_cvttpd_epi32(_mm256_floor_pd(_mm256_mul_pd(base_pd, values)));
        sum = _mm_add_epi32(sum, _mm_sub_epi32(scored, base_epi));
        index += 4;
    }
    let mut lanes = [0; 4];
    _mm_storeu_si128(lanes.as_mut_ptr().cast(), sum);
    lanes.into_iter().sum::<i32>()
        + multipliers[index..]
            .iter()
            .map(|&multiplier| (base as f64 * multiplier).floor() as i32 - base)
            .sum::<i32>()
}

impl IndependentSkillScoreMatrix {
    pub(super) fn max_order(&self, seed_order: [usize; 5], seed_captain: usize) -> ExactSkillOrder {
        let (best_delta, order_indices, captain_index) = max_independent_skill_delta(&self.deltas);
        let seed_delta = seed_order
            .iter()
            .enumerate()
            .map(|(activation, &card)| self.deltas[card][activation])
            .sum::<i32>()
            + self.deltas[seed_captain][5];
        if seed_delta == best_delta {
            ExactSkillOrder {
                score: self.base_score + seed_delta,
                order_indices: seed_order,
                captain_index: seed_captain,
            }
        } else {
            ExactSkillOrder {
                score: self.base_score + best_delta,
                order_indices,
                captain_index,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart(overlap: bool, combo: i32, fever: bool) -> Chart {
        let triggers = if overlap {
            [10, 20, 30, 40, 50, 60]
        } else {
            [10, 220, 430, 640, 850, 1060]
        };
        let nodes = (0..1450)
            .map(|index| ChartNode {
                time: index as f64 * 0.06,
                node_type: if triggers.contains(&index) {
                    ChartNodeType::Skill
                } else {
                    ChartNodeType::Node
                },
            })
            .collect();
        let mut chart = Chart::new_with_fever_section(27, nodes, Some(30.0), Some(50.0));
        chart
            .init_with_rule_and_fever(combo, true, ScoreRule::STANDARD, fever)
            .unwrap();
        chart
    }

    fn skills() -> [TeamCardSkill; 5] {
        std::array::from_fn(|index| TeamCardSkill {
            card_id: index as u32,
            duration: [5.0, 7.0, 3.5, 8.0, 5.0][index],
            score_up: [1.0, 1.5, 1.2, 1.65, 1.0][index],
            rateup: index == 0 || index == 1 || index == 4,
        })
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn native_matrix_matches_portable_with_changing_scratch_and_floor_edges() {
        if !avx2_available() {
            return;
        }
        let mut portable = ExactScoreScratch {
            compressed: true,
            ..Default::default()
        };
        let mut native = ExactScoreScratch {
            compressed: true,
            ..Default::default()
        };
        let mut seed = 0x3249_eb61u32;
        for overlap in [false, true] {
            for combo in [0, 680, 1980] {
                let chart = chart(overlap, combo, false);
                for iteration in 0..1000 {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let mut team = skills();
                    for (index, card) in team.iter_mut().enumerate() {
                        card.rateup = (iteration + index) % 3 == 0;
                        card.score_up =
                            [0.0, 0.5, 1.0, 1.495, 1.5, 1.505, 1.7][(iteration + index) % 7];
                        card.duration = if card.rateup {
                            RATEUP_DURATIONS[(iteration + index) % RATEUP_DURATIONS.len()]
                        } else {
                            SKILL_DURATIONS[(iteration + index) % SKILL_DURATIONS.len()]
                        };
                    }
                    if iteration % 4 == 0 {
                        team[4] = team[0];
                    }
                    let stat = if iteration < 3 {
                        iteration as i32
                    } else {
                        (seed % 2_000_001) as i32
                    };
                    let windows =
                        team.map(|skill| chart.compile_exact_skill_windows(skill).unwrap());
                    let expected = chart.compressed_skill_matrix_impl(
                        &team,
                        stat,
                        true,
                        &windows,
                        &mut portable,
                    );
                    // SAFETY: runtime support checked above.
                    let actual = unsafe {
                        chart.compressed_skill_matrix_avx2(&team, stat, true, &windows, &mut native)
                    };
                    assert_eq!(
                        actual, expected,
                        "overlap={overlap} combo={combo} stat={stat}"
                    );
                    assert_eq!(native.base_scores, portable.base_scores);
                    for card in 0..5 {
                        assert_eq!(
                            native.rateup_profiles[card].multipliers,
                            portable.rateup_profiles[card].multipliers
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn compressed_matrix_and_order_match_per_note_with_floor_boundaries() {
        let mut plain = ExactScoreScratch::default();
        let mut compressed = ExactScoreScratch {
            compressed: true,
            ..Default::default()
        };
        let mut rng = 0x19af_304eu32;
        for overlap in [false, true] {
            for combo in [0, 680, 1980] {
                let chart = chart(overlap, combo, false);
                assert!(chart.score_factor_runs.len() * 20 < chart.nodes.len());
                let mut team = skills();
                for iteration in 0..120 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    let stat = match iteration {
                        0 => 0,
                        1 => 1,
                        _ => (rng % 2_000_000) as i32,
                    };
                    // Exercise duplicate rows, ordinary skills, rate-up below/at/above cap.
                    team[0].score_up = [1.0, 1.495, 1.5, 1.505, 1.7][iteration % 5];
                    team[4] = TeamCardSkill {
                        card_id: 999,
                        ..team[0]
                    };
                    let windows =
                        team.map(|skill| chart.compile_exact_skill_windows(skill).unwrap());
                    let expected = chart
                        .independent_skill_score_matrix_from_windows(
                            &team, stat, true, &windows, &mut plain,
                        )
                        .unwrap();
                    let actual = chart
                        .independent_skill_score_matrix_from_windows(
                            &team,
                            stat,
                            true,
                            &windows,
                            &mut compressed,
                        )
                        .unwrap();
                    assert_eq!(
                        actual, expected,
                        "overlap={overlap} combo={combo} stat={stat}"
                    );
                    let mut seed = [0, 1, 2, 3, 4];
                    seed.rotate_left(iteration % 5);
                    let captain = iteration % 5;
                    let expected = chart
                        .get_independent_medley_score_order_from_exact_windows(
                            &team, stat, true, seed, captain, &windows, &mut plain,
                        )
                        .unwrap();
                    let actual = chart
                        .get_independent_medley_score_order_from_exact_windows(
                            &team,
                            stat,
                            true,
                            seed,
                            captain,
                            &windows,
                            &mut compressed,
                        )
                        .unwrap();
                    assert_eq!(actual, expected);
                }
            }
        }
    }

    #[test]
    fn compression_falls_back_for_fever_and_mode_mismatch_and_survives_reinit() {
        let mut compressed = ExactScoreScratch {
            compressed: true,
            ..Default::default()
        };
        let mut plain = ExactScoreScratch::default();
        let team = skills();
        for fever in [false, true] {
            let mut chart = chart(false, 0, fever);
            for is_medley in [true, false] {
                let windows = team.map(|skill| chart.compile_exact_skill_windows(skill).unwrap());
                for stat in [1, 123_456, 2_000_001] {
                    assert_eq!(
                        chart
                            .independent_skill_score_matrix_from_windows(
                                &team, stat, is_medley, &windows, &mut plain
                            )
                            .unwrap(),
                        chart
                            .independent_skill_score_matrix_from_windows(
                                &team,
                                stat,
                                is_medley,
                                &windows,
                                &mut compressed
                            )
                            .unwrap(),
                    );
                }
            }
            chart.init_auto_with_base_multiplier(0.75).unwrap();
            assert_eq!(chart.score_factor_runs.len(), 1);
            let windows = team.map(|skill| chart.compile_exact_skill_windows(skill).unwrap());
            assert_eq!(
                chart
                    .independent_skill_score_matrix_from_windows(
                        &team, 333_333, false, &windows, &mut plain
                    )
                    .unwrap(),
                chart
                    .independent_skill_score_matrix_from_windows(
                        &team,
                        333_333,
                        false,
                        &windows,
                        &mut compressed
                    )
                    .unwrap(),
            );
        }
    }

    #[test]
    fn rateup_vector_matches_scalar_at_every_tail_length() {
        let mut profile = RateUpProfileScratch::default();
        for score_up in [1.0, 1.499, 1.5, 1.7] {
            profile.prepare(score_up, 300);
            for count in 0..300 {
                let values = &profile.multipliers[..count];
                for base in [0, 1, 32_767, 10_003] {
                    let expected: i32 = values
                        .iter()
                        .map(|&value| (base as f64 * value).floor() as i32 - base)
                        .sum();
                    assert_eq!(constant_base_rateup_delta(base, values), expected);
                }
            }
        }
    }
}
