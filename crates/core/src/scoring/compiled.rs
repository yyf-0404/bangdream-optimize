//! Compress equal states only after resolving Combo, Fever and skill state.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use super::note_integer;
use super::{nonnegative, NoteScorer, ScoringError, ScoringPower};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct NoteFactors {
    pub judgement: f32,
    pub combo: f32,
    pub skill: f32,
    pub fever: f32,
}

/// Power-independent timeline. No continuous score factor crosses a rounding
/// boundary; groups are keyed by the complete native f32 operand bits.
#[derive(Debug, Clone)]
pub struct CompiledScore {
    level: i32,
    count: u32,
    judgement: Vec<f32>,
    combo: Vec<f32>,
    skill: Vec<f32>,
    fever: Vec<f32>,
    multiplicity: Vec<u32>,
}

impl CompiledScore {
    pub(super) fn chart_parameters(&self) -> (i32, u32) {
        (self.level, self.count)
    }

    pub(super) fn groups(&self) -> impl Iterator<Item = (NoteFactors, u32)> + '_ {
        (0..self.group_count()).map(|i| {
            (
                NoteFactors {
                    judgement: self.judgement[i],
                    combo: self.combo[i],
                    skill: self.skill[i],
                    fever: self.fever[i],
                },
                self.multiplicity[i],
            )
        })
    }

    pub fn new(level: i32, notes: &[NoteFactors]) -> Result<Self, ScoringError> {
        let count = u32::try_from(notes.len()).map_err(|_| ScoringError::InvalidChart)?;
        NoteScorer::new(ScoringPower::new(0.0)?, level, count)?;
        let mut groups = BTreeMap::<[u32; 4], u32>::new();
        for note in notes {
            let key = [
                nonnegative(note.judgement, "judgment multiplier")?.to_bits(),
                nonnegative(note.combo, "combo multiplier")?.to_bits(),
                nonnegative(note.skill, "skill multiplier")?.to_bits(),
                nonnegative(note.fever, "Fever multiplier")?.to_bits(),
            ];
            *groups.entry(key).or_default() += 1;
        }
        let mut result = Self {
            level,
            count,
            judgement: Vec::new(),
            combo: Vec::new(),
            skill: Vec::new(),
            fever: Vec::new(),
            multiplicity: Vec::new(),
        };
        for (key, count) in groups {
            result.judgement.push(f32::from_bits(key[0]));
            result.combo.push(f32::from_bits(key[1]));
            result.skill.push(f32::from_bits(key[2]));
            result.fever.push(f32::from_bits(key[3]));
            result.multiplicity.push(count);
        }
        Ok(result)
    }

    pub fn group_count(&self) -> usize {
        self.multiplicity.len()
    }
    pub fn note_count(&self) -> u32 {
        self.count
    }

    pub fn score(&self, power: ScoringPower) -> Result<u64, ScoringError> {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: checked CPU support; all parallel arrays have equal length.
            return unsafe { self.score_avx2(power) };
        }
        self.score_scalar(power)
    }

    pub fn score_scalar(&self, power: ScoringPower) -> Result<u64, ScoringError> {
        let scorer = NoteScorer::new(power, self.level, self.count)?;
        self.scalar_tail(scorer, 0)
    }

    fn scalar_tail(&self, scorer: NoteScorer, start: usize) -> Result<u64, ScoringError> {
        let mut total = 0;
        for i in start..self.group_count() {
            let note = scorer.score_with_judgement_multiplier(
                self.judgement[i],
                self.combo[i],
                self.skill[i],
                self.fever[i],
            )?;
            total += u64::from(note.final_score) * u64::from(self.multiplicity[i]);
        }
        Ok(total)
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[target_feature(enable = "avx2")]
    unsafe fn score_avx2(&self, power: ScoringPower) -> Result<u64, ScoringError> {
        #[cfg(target_arch = "x86")]
        use std::arch::x86::*;
        #[cfg(target_arch = "x86_64")]
        use std::arch::x86_64::*;
        let scorer = NoteScorer::new(power, self.level, self.count)?;
        let base = _mm256_set1_ps(scorer.base());
        let mut total = 0;
        let mut i = 0;
        while i + 8 <= self.group_count() {
            let judged = _mm256_mul_ps(base, _mm256_loadu_ps(self.judgement.as_ptr().add(i)));
            let before = _mm256_floor_ps(_mm256_mul_ps(
                judged,
                _mm256_loadu_ps(self.combo.as_ptr().add(i)),
            ));
            let combined = _mm256_mul_ps(
                _mm256_loadu_ps(self.fever.as_ptr().add(i)),
                _mm256_loadu_ps(self.skill.as_ptr().add(i)),
            );
            let final_value = _mm256_mul_ps(before, combined);
            let mut before_values = [0.0; 8];
            let mut final_values = [0.0; 8];
            _mm256_storeu_ps(before_values.as_mut_ptr(), before);
            _mm256_storeu_ps(final_values.as_mut_ptr(), final_value);
            for lane in 0..8 {
                // Keep the same uint32 checks as the scalar path. f32 integers
                // above 2^24 remain exactly representable by their original bits.
                note_integer(before_values[lane])?;
                let value = note_integer(final_values[lane])?;
                total += u64::from(value) * u64::from(self.multiplicity[i + lane]);
            }
            i += 8;
        }
        Ok(total + self.scalar_tail(scorer, i)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::{normal_combo_multiplier, Crescendo, Judgement};
    #[test]
    fn compressed_and_simd_match_uncompressed_at_fractional_boundaries() {
        let mut crescendo = Crescendo::new(100.0, 0.5, 150).unwrap();
        let notes: Vec<_> = (1..=1000)
            .map(|i| {
                let grade = if i % 29 == 0 {
                    Judgement::Great
                } else {
                    Judgement::Perfect
                };
                NoteFactors {
                    judgement: grade.multiplier(),
                    combo: normal_combo_multiplier(i),
                    skill: if (50..600).contains(&i) {
                        crescendo.multiplier_for_note(grade)
                    } else {
                        1.0
                    },
                    fever: if (100..800).contains(&i) { 2.0 } else { 1.0 },
                }
            })
            .collect();
        let compiled = CompiledScore::new(27, &notes).unwrap();
        assert!(compiled.group_count() < notes.len());
        for i in 0..257 {
            let power = ScoringPower::new(300040.0 + i as f32 / 32.0).unwrap();
            let scorer = NoteScorer::new(power, 27, notes.len() as u32).unwrap();
            let direct: u64 = notes
                .iter()
                .map(|n| {
                    scorer
                        .score_with_judgement_multiplier(n.judgement, n.combo, n.skill, n.fever)
                        .unwrap()
                        .final_score as u64
                })
                .sum();
            assert_eq!(compiled.score_scalar(power).unwrap(), direct);
            assert_eq!(compiled.score(power).unwrap(), direct);
        }
    }
    #[test]
    fn rejects_overflow_even_when_followed_by_zero_skill() {
        let notes: Vec<_> = (0..8)
            .map(|i| NoteFactors {
                judgement: 1.1,
                combo: 1.0 + i as f32,
                skill: 0.0,
                fever: 1.0,
            })
            .collect();
        let compiled = CompiledScore::new(27, &notes).unwrap();
        let power = ScoringPower::new(1e20).unwrap();
        assert_eq!(compiled.score(power), Err(ScoringError::NoteScoreOverflow));
        assert_eq!(
            compiled.score_scalar(power),
            Err(ScoringError::NoteScoreOverflow)
        );
    }

    #[test]
    fn vector_groups_and_scalar_tail_preserve_unsigned_scores() {
        let notes: Vec<_> = (0..19)
            .map(|i| NoteFactors {
                judgement: 1.0,
                combo: 1.0 + i as f32 / 1000.0,
                skill: 1.0,
                fever: 1.0,
            })
            .collect();
        let compiled = CompiledScore::new(5, &notes).unwrap();
        let power = ScoringPower::new(19_000_000_000.0).unwrap();
        let scorer = NoteScorer::new(power, 5, 19).unwrap();
        let direct: u64 = notes
            .iter()
            .map(|n| {
                let score = scorer
                    .score_with_judgement_multiplier(n.judgement, n.combo, n.skill, n.fever)
                    .unwrap();
                assert!(score.before_skill > i32::MAX as u32);
                u64::from(score.final_score)
            })
            .sum();
        assert_eq!(compiled.group_count(), 19);
        assert_eq!(compiled.score_scalar(power).unwrap(), direct);
        assert_eq!(compiled.score(power).unwrap(), direct);
    }
}
