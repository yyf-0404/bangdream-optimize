//! Maximum-score adapter for the shared second/third-team AVX2 scanner.
use super::*;

pub(super) fn enabled() -> bool {
    #[cfg(feature = "experimental-maximize-reuse")]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var("BANGDREAM_OPTIMIZE_MAXIMIZE_REUSE").as_deref() != Ok("off")
        })
    }
    #[cfg(not(feature = "experimental-maximize-reuse"))]
    true
}

fn scan_parts() -> u8 {
    #[cfg(feature = "experimental-maximize-reuse")]
    {
        static PARTS: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
        return *PARTS.get_or_init(|| {
            u8::from(
                std::env::var("BANGDREAM_OPTIMIZE_MAXIMIZE_SECOND_BATCH").as_deref() != Ok("off"),
            ) | (u8::from(
                std::env::var("BANGDREAM_OPTIMIZE_MAXIMIZE_THIRD_REUSE").as_deref() != Ok("off"),
            ) << 1)
        });
    }
    #[cfg(not(feature = "experimental-maximize-reuse"))]
    3
}

#[target_feature(enable = "avx2")]
unsafe fn solve<const W: usize>(
    scores: &[[Score; 3]],
    masks: &[[u64; W]],
    current_best: Score,
    implementation: MedleySolverImplementation,
    mode: u8,
) -> ExactSearchOutcome {
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
    let visit = |plan, score: i64| {
        // The score domain is the same i32 domain as the original solver.
        best = Score::try_from(score).expect("maximum team score fits i32");
        indices = Some(plan);
        MedleyBandVisit::Continue { floor: score + 1 }
    };
    macro_rules! run {
        ($second:literal, $third:literal) => {
            reuse_band::enumerate_prepared_parts::<W, $second, $third>(
                i64::from(current_best) + 1,
                &scores,
                masks,
                implementation,
                search,
                visit,
            )
        };
    }
    let metrics = match mode {
        0 => run!(false, false),
        1 => run!(true, false),
        2 => run!(false, true),
        _ => run!(true, true),
    };
    // Shared scan metrics count logical candidates, including filtered positions.
    outcome(
        best,
        indices,
        implementation,
        metrics.pair_checks.saturating_add(metrics.third_checks),
    )
}

#[target_feature(enable = "avx2")]
pub(super) unsafe fn narrow(input: &MedleySolverInput) -> ExactSearchOutcome {
    narrow_mode(input, scan_parts())
}

#[target_feature(enable = "avx2")]
unsafe fn narrow_mode(input: &MedleySolverInput, mode: u8) -> ExactSearchOutcome {
    let masks: Vec<_> = input.team_masks.iter().map(|&mask| [mask]).collect();
    solve(
        &input.scores,
        &masks,
        input.current_best,
        MedleySolverImplementation::Avx2,
        mode,
    )
}

#[target_feature(enable = "avx2")]
pub(super) unsafe fn wide(input: &WideMedleySolverInput, meter: WorkMeter) -> ExactSearchOutcome {
    wide_mode(input, meter, scan_parts())
}

#[target_feature(enable = "avx2")]
unsafe fn wide_mode(
    input: &WideMedleySolverInput,
    meter: WorkMeter,
    mode: u8,
) -> ExactSearchOutcome {
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
                MedleySolverImplementation::Avx2Wide,
                mode,
            )
        }};
    }
    match input.team_masks.first().map(Vec::len) {
        Some(1) => run!(1),
        Some(2) => run!(2),
        Some(3) => run!(3),
        Some(4) => run!(4),
        _ => solve_wide_avx2_x86(input, meter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_scan_preserves_maximum_and_original_tie_order() {
        if !avx2_available() {
            return;
        }
        let mut seed = 0x632a72be14a5c31du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for words in [1, 2, 3, 4, 5, 7] {
            for n in [0, 1, 2, 4, 7, 8, 9, 31] {
                for case in 0..12 {
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
                    let scores = (0..n)
                        .map(|_| {
                            std::array::from_fn(|_| {
                                if case == 0 {
                                    0
                                } else if case == 1 {
                                    100
                                } else {
                                    (next() % 101) as i32 - 20
                                }
                            })
                        })
                        .collect();
                    let input = WideMedleySolverInput {
                        current_best: if case == 0 { -1 } else { (next() % 302) as i32 },
                        team_masks: masks,
                        scores,
                    };
                    let scalar = solve_wide_scalar_internal(&input, WorkMeter::unlimited());
                    // SAFETY: AVX2 support was checked above.
                    let (before, after) = unsafe {
                        (
                            solve_wide_avx2_x86(&input, WorkMeter::unlimited()),
                            wide(&input, WorkMeter::unlimited()),
                        )
                    };
                    assert_eq!(
                        (after.best_score, after.best_indices),
                        (scalar.best_score, scalar.best_indices),
                        "words={words}, n={n}, case={case}"
                    );
                    assert_eq!(
                        (after.best_score, after.best_indices),
                        (before.best_score, before.best_indices)
                    );
                    for mode in 0..4 {
                        let variant = unsafe { wide_mode(&input, WorkMeter::unlimited(), mode) };
                        assert_eq!(
                            (variant.best_score, variant.best_indices),
                            (scalar.best_score, scalar.best_indices),
                            "mode={mode}, words={words}, n={n}, case={case}"
                        );
                        if words == 1 {
                            let narrow_input = MedleySolverInput {
                                current_best: input.current_best,
                                scores: input.scores.clone(),
                                team_masks: input.team_masks.iter().map(|m| m[0]).collect(),
                            };
                            let variant = unsafe { narrow_mode(&narrow_input, mode) };
                            assert_eq!(
                                (variant.best_score, variant.best_indices),
                                (scalar.best_score, scalar.best_indices)
                            );
                        }
                    }
                    if words == 1 {
                        let input = MedleySolverInput {
                            current_best: input.current_best,
                            scores: input.scores,
                            team_masks: input.team_masks.iter().map(|m| m[0]).collect(),
                        };
                        let after = unsafe { narrow(&input) };
                        assert_eq!(
                            (after.best_score, after.best_indices),
                            (scalar.best_score, scalar.best_indices)
                        );
                    }
                }
            }
        }
    }
}
