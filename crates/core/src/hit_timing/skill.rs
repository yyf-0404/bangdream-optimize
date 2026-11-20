use super::{JudgementCorrection, ReplayError, ReplaySkill, ScoreEffect, SkillStateSnapshot};
use crate::scoring::{skill_multiplier_from_percent, Crescendo, Judgement, ScoringError};

impl ReplaySkill {
    /// Complete descriptor bits for calculation-scoped caches. Chart, reference
    /// power and search settings must be scoped separately; not a persisted ID.
    pub fn cache_key(&self) -> [u32; 6] {
        let [kind, a, b, c] = match self.score {
            ScoreEffect::Plain { percent } => [0, percent.to_bits(), 0, 0],
            ScoreEffect::PerfectOnly { percent } => [1, percent.to_bits(), 0, 0],
            ScoreEffect::PerfectOrZero { percent } => [2, percent.to_bits(), 0, 0],
            ScoreEffect::PerfectOrHalf { percent } => [3, percent.to_bits(), 0, 0],
            ScoreEffect::Continuous { high, low } => [4, high.to_bits(), low.to_bits(), 0],
            ScoreEffect::Crescendo { percent, step, cap } => {
                [5, percent.to_bits(), step.to_bits(), cap]
            }
        };
        let correction = match self.correction {
            None => 0,
            Some(JudgementCorrection::Great) => 1,
            Some(JudgementCorrection::Good) => 2,
            Some(JudgementCorrection::Bad) => 3,
        };
        [self.duration.to_bits(), correction, kind, a, b, c]
    }

    pub(super) fn validate(&self, index: usize) -> Result<(), ReplayError> {
        let error = |reason| ReplayError::Skill { index, reason };
        if !self.duration.is_finite() || self.duration <= 0.0 || self.duration > 60.0 {
            return Err(error("duration must be finite and in (0, 60]"));
        }
        let base = match self.score {
            ScoreEffect::Plain { percent }
            | ScoreEffect::PerfectOnly { percent }
            | ScoreEffect::PerfectOrZero { percent }
            | ScoreEffect::PerfectOrHalf { percent } => percent,
            ScoreEffect::Continuous { high, low } => {
                if !low.is_finite() || low < 0.0 || low > high {
                    return Err(error(
                        "continuous effect requires a finite lower percentage",
                    ));
                }
                high
            }
            ScoreEffect::Crescendo { percent, step, cap } => {
                Crescendo::new(percent, step, cap)?;
                percent
            }
        };
        skill_multiplier_from_percent(base)?;
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct SkillState {
    skill: ReplaySkill,
    worst: Judgement,
    crescendo: Option<Crescendo>,
}

impl SkillState {
    pub fn new(skill: &ReplaySkill) -> Result<Self, ScoringError> {
        let crescendo = match skill.score {
            ScoreEffect::Crescendo { percent, step, cap } => {
                Some(Crescendo::new(percent, step, cap)?)
            }
            _ => None,
        };
        Ok(Self {
            skill: skill.clone(),
            worst: Judgement::Perfect,
            crescendo,
        })
    }

    pub fn correct(&self, raw: Judgement) -> Judgement {
        let threshold = self.skill.correction.map(|c| match c {
            JudgementCorrection::Great => Judgement::Great,
            JudgementCorrection::Good => Judgement::Good,
            JudgementCorrection::Bad => Judgement::Bad,
        });
        if threshold.is_some_and(|min| raw >= min) {
            Judgement::Perfect
        } else {
            raw
        }
    }

    pub fn multiplier(&mut self, grade: Judgement) -> f32 {
        let percent = match self.skill.score {
            ScoreEffect::Plain { percent } => percent,
            ScoreEffect::PerfectOnly { percent } => {
                if grade != Judgement::Perfect {
                    return 1.0;
                }
                percent
            }
            ScoreEffect::PerfectOrZero { percent } => {
                if grade != Judgement::Perfect {
                    return 0.0;
                }
                percent
            }
            ScoreEffect::PerfectOrHalf { percent } => {
                if grade != Judgement::Perfect {
                    return 0.5;
                }
                percent
            }
            ScoreEffect::Continuous { high, low } => {
                self.worst = self.worst.min(grade);
                if self.worst < Judgement::Perfect {
                    low
                } else {
                    high
                }
            }
            ScoreEffect::Crescendo { .. } => {
                return self.crescendo.as_mut().unwrap().multiplier_for_note(grade)
            }
        };
        // Definition was validated before any replay or state transition.
        skill_multiplier_from_percent(percent).unwrap()
    }

    pub fn snapshot(&self) -> SkillStateSnapshot {
        SkillStateSnapshot {
            worst_judgement: self.worst,
            extra_percent: self.crescendo.map_or(0.0, |s| s.extra_percent()),
        }
    }
}
