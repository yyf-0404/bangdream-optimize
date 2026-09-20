//! Certified linear envelopes for one fully resolved scoring timeline.
//!
//! Coefficients retain the native f32 level/Fever/skill operands. Outward f64
//! arithmetic encloses the exact real product; per-operation f32 ULP bounds
//! cover rounding, and the two integer casts contribute explicit lower losses.
use super::{CompiledScore, NoteScorer, PowerRange, ScoringError, ScoringPower};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreBounds {
    pub min: u64,
    pub max: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct ScoreEnvelope {
    max_power: ScoringPower,
    coefficient: [f64; 2],
    rounding_error: f64,
    truncation_loss: f64,
}

impl CompiledScore {
    /// Precompute once for a resolved timeline and a finite upper power limit.
    /// The result is not a bound for a different skill assignment/timing model.
    pub fn envelope(&self, max_power: ScoringPower) -> Result<ScoreEnvelope, ScoringError> {
        // Validate every intermediate uint32 conversion at the largest input.
        self.score_scalar(max_power)?;
        let (level, count) = self.chart_parameters();
        let level_rate = 1.0_f32 + (level - 5) as f32 * 0.01_f32;
        let divisor = count as f32;
        let corrected = max_power.value() * level_rate;
        let normalized = corrected / divisor;
        let base = NoteScorer::new(max_power, level, count)?.base();
        let base_coefficient = interval_mul(
            interval_div([f64::from(level_rate); 2], f64::from(divisor)),
            3.0,
        );
        let base_error = up(up(ulp(corrected) / f64::from(divisor)) + ulp(normalized));
        let base_error = up(up(base_error * 3.0) + ulp(base));
        let mut coefficient = [0.0; 2];
        let mut rounding_error = 0.0;
        let mut truncation_loss = 0.0;
        for (note, multiplicity) in self.groups() {
            let combined = note.fever * note.skill;
            // A zero final multiplier contributes exactly zero, including casts.
            if combined == 0.0 || note.judgement == 0.0 || note.combo == 0.0 {
                continue;
            }
            let judged = base * note.judgement;
            let combo = judged * note.combo;
            let after_skill = (combo as u32) as f32 * combined;
            let error = up(up(base_error * f64::from(note.judgement)) + ulp(judged));
            let error = up(up(error * f64::from(note.combo)) + ulp(combo));
            let error = up(up(error * f64::from(combined)) + ulp(after_skill));
            // floor(f32) is itself exactly representable as f32 in the valid
            // uint32 range: casting that integer back adds no extra error.
            let loss = up(f64::from(combined) + 1.0);
            let term = interval_mul(
                interval_mul(
                    interval_mul(base_coefficient, f64::from(note.judgement)),
                    f64::from(note.combo),
                ),
                f64::from(combined),
            );
            let term = interval_mul(term, f64::from(multiplicity));
            coefficient = [down(coefficient[0] + term[0]), up(coefficient[1] + term[1])];
            rounding_error = up(rounding_error + up(error * f64::from(multiplicity)));
            truncation_loss = up(truncation_loss + up(loss * f64::from(multiplicity)));
        }
        Ok(ScoreEnvelope {
            max_power,
            coefficient,
            rounding_error,
            truncation_loss,
        })
    }
}

impl ScoreEnvelope {
    pub fn max_power(self) -> ScoringPower {
        self.max_power
    }
    pub fn coefficient_bounds(self) -> [f64; 2] {
        self.coefficient
    }
    pub fn rounding_error(self) -> f64 {
        self.rounding_error
    }
    pub fn truncation_loss(self) -> f64 {
        self.truncation_loss
    }

    /// Constant-time bounds. Scores outside the construction domain are errors.
    pub fn bounds(self, power: ScoringPower) -> Result<ScoreBounds, ScoringError> {
        if power > self.max_power {
            return Err(ScoringError::InvalidPowerRange);
        }
        if power.value() == 0.0 {
            return Ok(ScoreBounds { min: 0, max: 0 });
        }
        let power = f64::from(power.value());
        let lower = down(
            down(down(power * self.coefficient[0]) - self.rounding_error) - self.truncation_loss,
        );
        let upper = up(up(power * self.coefficient[1]) + self.rounding_error);
        // Rust's saturation at u64::MAX is safe here: chart totals are bounded
        // by u32::MAX notes each contributing at most u32::MAX points.
        Ok(ScoreBounds {
            min: lower.ceil() as u64,
            max: upper.floor() as u64,
        })
    }

    pub fn bounds_for_range(self, powers: PowerRange) -> Result<ScoreBounds, ScoringError> {
        Ok(ScoreBounds {
            min: self.bounds(powers.min())?.min,
            max: self.bounds(powers.max())?.max,
        })
    }
}

// One full ULP at the maximum rounded result encloses every rounding error
// below that magnitude, including subnormals and a binade crossing. Use a full
// ULP rather than half to avoid assumptions at the endpoint of a binade.
fn ulp(value: f32) -> f64 {
    let exponent = ((value.to_bits() >> 23) & 255) as i32;
    2.0_f64.powi(if exponent == 0 { -149 } else { exponent - 150 })
}
fn up(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        f64::from_bits(value.to_bits() + 1)
    }
}
fn down(value: f64) -> f64 {
    if value <= 0.0 {
        0.0
    } else {
        f64::from_bits(value.to_bits() - 1)
    }
}
fn interval_mul(value: [f64; 2], factor: f64) -> [f64; 2] {
    [down(value[0] * factor), up(value[1] * factor)]
}
fn interval_div(value: [f64; 2], divisor: f64) -> [f64; 2] {
    [down(value[0] / divisor), up(value[1] / divisor)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::{normal_combo_multiplier, Crescendo, Judgement, NoteFactors};
    fn p(value: f32) -> ScoringPower {
        ScoringPower::new(value).unwrap()
    }
    fn plan(count: usize) -> CompiledScore {
        let mut crescendo = Crescendo::new(100.0, 0.5, 150).unwrap();
        CompiledScore::new(
            27,
            &(0..count)
                .map(|i| NoteFactors {
                    judgement: if i % 17 == 0 { 0.8 } else { 1.1 },
                    combo: normal_combo_multiplier(i as u32 + 1),
                    skill: crescendo.multiplier_for_note(Judgement::Perfect),
                    fever: if i % 3 == 0 { 2.0 } else { 1.0 },
                })
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn envelopes_enclose_exact_scores_across_float_binades() {
        for count in [1, 17, 121, 760, 2051] {
            let model = plan(count);
            let envelope = model.envelope(p(1000000.0)).unwrap();
            let mut cases = vec![0, 1, 0x007fffff, 0x00800000];
            for value in [1.0_f32, 1024.0, 262144.0, 300040.75, 524288.0, 999999.0] {
                cases.extend(value.to_bits() - 8..=value.to_bits() + 8);
            }
            for bits in cases {
                let power = p(f32::from_bits(bits));
                let bounds = envelope.bounds(power).unwrap();
                let exact = model.score(power).unwrap();
                assert!(
                    bounds.min <= exact && exact <= bounds.max,
                    "count={count} power={} bounds={bounds:?} score={exact}",
                    power.value()
                );
            }
            assert!(envelope.bounds(p(1000001.0)).is_err());
        }
    }
    #[test]
    fn validates_zero_factors_and_uint32_conversion_boundaries() {
        for judgement in [0.0, f32::from_bits(1), 1.0] {
            for skill in [0.0, f32::from_bits(1), 1.0] {
                let model = CompiledScore::new(
                    5,
                    &[NoteFactors {
                        judgement,
                        combo: 1.0,
                        skill,
                        fever: 1.0,
                    }],
                )
                .unwrap();
                let maximum = p(1_000_000_000.0);
                let envelope = model.envelope(maximum).unwrap();
                for power in [p(0.0), p(1.0), p(715827900.0), maximum] {
                    let bounds = envelope.bounds(power).unwrap();
                    let score = model.score(power).unwrap();
                    assert!(bounds.min <= score && score <= bounds.max);
                }
            }
        }
        assert!(plan(1).envelope(p(f32::MAX)).is_err());
    }

    #[test]
    fn randomized_envelopes_cover_rounding_over_many_magnitudes() {
        let mut seed = 0x9344_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 32) as u32
        };
        for _ in 0..300 {
            let count = (next() % 31 + 1) as usize;
            let notes: Vec<_> = (0..count)
                .map(|_| NoteFactors {
                    judgement: [0.0, 0.5, 0.75, 0.8, 1.1][next() as usize % 5],
                    combo: [1.0, 1.01, 1.11, 1.34][next() as usize % 4],
                    skill: [0.0, 1.0, 1.6, 2.205, 2.5][next() as usize % 5],
                    fever: [1.0, 2.0][next() as usize % 2],
                })
                .collect();
            let model = CompiledScore::new((next() % 100 + 1) as i32, &notes).unwrap();
            let maximum = p(f32::from_bits(
                (next() % 150 + 1) << 23 | (next() & 0x7fffff),
            ));
            let Ok(envelope) = model.envelope(maximum) else {
                assert!(model.score(maximum).is_err());
                continue;
            };
            for _ in 0..64 {
                let power = p(f32::from_bits(next() % (maximum.cache_key() + 1)));
                let exact = model.score(power).unwrap();
                let bounds = envelope.bounds(power).unwrap();
                assert!(
                    bounds.min <= exact && exact <= bounds.max,
                    "power={} maximum={} exact={exact} bounds={bounds:?}",
                    power.value(),
                    maximum.value()
                );
            }
        }
    }

    #[test]
    fn member_choice_bounds_and_branch_pruning_match_exhaustive_search() {
        let choices: Vec<Vec<[f32; 3]>> = (0..5)
            .map(|slot| {
                (0..4)
                    .map(|option| {
                        let v = 16000.0 + (slot * 97 + option * 451) as f32 / 16.0;
                        [v, v + 53.125, v - 11.03125]
                    })
                    .collect()
            })
            .collect();
        let refs: Vec<_> = choices.iter().map(Vec::as_slice).collect();
        let root = PowerRange::from_member_choices(&refs).unwrap();
        let model = plan(760);
        let envelope = model.envelope(root.max()).unwrap();
        let mut brute_best = 0;
        let mut pruned_best = 0;
        let mut pruned = 0;
        let mut selected = [[0.0; 3]; 5];
        fn walk(
            choices: &[Vec<[f32; 3]>],
            selected: &mut [[f32; 3]; 5],
            depth: usize,
            model: &CompiledScore,
            envelope: ScoreEnvelope,
            pruning: bool,
            best: &mut u64,
            pruned: &mut usize,
        ) {
            let refs: Vec<&[[f32; 3]]> = (0..5)
                .map(|i| {
                    if i < depth {
                        std::slice::from_ref(&selected[i])
                    } else {
                        choices[i].as_slice()
                    }
                })
                .collect();
            let powers = PowerRange::from_member_choices(&refs).unwrap();
            if pruning && envelope.bounds_for_range(powers).unwrap().max < *best {
                *pruned += 1;
                return;
            }
            if depth == 5 {
                let power = ScoringPower::from_member_parameters(selected).unwrap();
                assert!(powers.contains(power));
                // Every ancestor bounds the leaf, not just the singleton leaf.
                for prefix in 0..5 {
                    let ancestors: Vec<&[[f32; 3]]> = (0..5)
                        .map(|i| {
                            if i < prefix {
                                std::slice::from_ref(&selected[i])
                            } else {
                                choices[i].as_slice()
                            }
                        })
                        .collect();
                    assert!(PowerRange::from_member_choices(&ancestors)
                        .unwrap()
                        .contains(power));
                }
                let value = model.score(power).unwrap();
                assert!(envelope.bounds_for_range(powers).unwrap().max >= value);
                *best = (*best).max(value);
                return;
            }
            for &choice in choices[depth].iter().rev() {
                selected[depth] = choice;
                walk(
                    choices,
                    selected,
                    depth + 1,
                    model,
                    envelope,
                    pruning,
                    best,
                    pruned,
                );
            }
        }
        walk(
            &choices,
            &mut selected,
            0,
            &model,
            envelope,
            false,
            &mut brute_best,
            &mut 0,
        );
        walk(
            &choices,
            &mut selected,
            0,
            &model,
            envelope,
            true,
            &mut pruned_best,
            &mut pruned,
        );
        assert_eq!(brute_best, pruned_best);
        assert!(pruned > 0);
    }
}
