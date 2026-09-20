//! Original CN 9.4.4 arithmetic, independent of skill scheduling and team search.
//!
//! Each operation is a separate f32 operation. Do not reassociate products,
//! replace multiply/subtract by `mul_add`, or use UI integers as scoring power.
//! Production search uses the same arithmetic revision and outward bounds.
mod bounds;
#[doc(hidden)]
pub mod cache_audit;
mod compiled;
mod power;
pub mod preparation;
mod search;
mod team;

pub use bounds::{ScoreBounds, ScoreEnvelope};
pub use compiled::{CompiledScore, NoteFactors};
pub use power::{AreaBonus, DeckPower, MemberPower, ScoringPower};
pub(crate) use search::{linear_power_lower, linear_power_upper, linear_score_upper, SCORE_GAMMA};
pub use search::{CachedScore, PowerRange};
pub(crate) use team::TeamPowerGroups;
pub use team::{TeamPower, TeamPowerGroup};
use thiserror::Error;

/// Arithmetic only; this does not identify a device's frame/update policy.
pub const ARITHMETIC_REVISION: &str = "cn-9.4.4-f32-v1";

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ScoringError {
    #[error("{0} must be finite and nonnegative")]
    InvalidValue(&'static str),
    #[error("chart requires a positive note count and a level in 1..=100")]
    InvalidChart,
    #[error("a note score exceeds the supported unsigned 32-bit range")]
    NoteScoreOverflow,
    #[error("display power exceeds the supported signed 32-bit range")]
    DisplayPowerOverflow,
    #[error("total score exceeds the supported signed 32-bit result range")]
    TotalScoreOverflow,
    #[error("crescendo needs a positive increment and a cap at least as high as its base")]
    InvalidCrescendo,
    #[error("power range must be ordered, nonempty and inside the validated scoring domain")]
    InvalidPowerRange,
}

fn nonnegative(value: f32, name: &'static str) -> Result<f32, ScoringError> {
    if value.is_finite() && value >= 0.0 {
        Ok(if value == 0.0 { 0.0 } else { value })
    } else {
        Err(ScoringError::InvalidValue(name))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Judgement {
    Miss = 0,
    Bad = 1,
    Good = 2,
    Great = 3,
    Perfect = 4,
}

impl Judgement {
    pub fn multiplier(self) -> f32 {
        match self {
            Self::Miss | Self::Bad => 0.0,
            Self::Good => 0.5,
            Self::Great => 0.8,
            Self::Perfect => 1.1,
        }
    }
}

/// Regular manual Combo only. Medley and Auto supply their own verified rate.
pub fn normal_combo_multiplier(combo: u32) -> f32 {
    match combo {
        0..=20 => 1.0,
        21..=50 => 1.01,
        51..=100 => 1.02,
        101..=150 => 1.03,
        151..=200 => 1.04,
        201..=250 => 1.05,
        251..=300 => 1.06,
        301..=400 => 1.07,
        401..=500 => 1.08,
        501..=600 => 1.09,
        601..=700 => 1.10,
        _ => 1.11,
    }
}

/// Master-data percent, e.g. 120 -> the native f32 multiplier for 2.2.
pub fn skill_multiplier_from_percent(percent: f32) -> Result<f32, ScoringError> {
    let percent = nonnegative(percent, "skill percent")?;
    let rate = percent / 100.0_f32;
    nonnegative(1.0_f32 + rate, "skill multiplier")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteScore {
    /// Judgment and Combo applied; Fever and skills not applied yet.
    pub before_skill: u32,
    pub final_score: u32,
}

/// Base score shared by all chart notes for one unrounded team power.
#[derive(Debug, Clone, Copy)]
pub struct NoteScorer {
    base: f32,
}

impl NoteScorer {
    pub fn new(power: ScoringPower, level: i32, count: u32) -> Result<Self, ScoringError> {
        if count == 0 || !(1..=100).contains(&level) {
            return Err(ScoringError::InvalidChart);
        }
        let level_delta = (level - 5) as f32 * 0.01_f32;
        let level_rate = 1.0_f32 + level_delta;
        let corrected_power = power.value() * level_rate;
        let per_note = corrected_power / count as f32;
        let base = nonnegative(per_note * 3.0_f32, "base score")?;
        Ok(Self { base })
    }

    pub fn base(self) -> f32 {
        self.base
    }

    pub fn score(
        self,
        judgement: Judgement,
        combo_multiplier: f32,
        skill_multiplier: f32,
        fever_multiplier: f32,
    ) -> Result<NoteScore, ScoringError> {
        self.score_with_judgement_multiplier(
            judgement.multiplier(),
            combo_multiplier,
            skill_multiplier,
            fever_multiplier,
        )
    }

    /// Allows mode-specific judgment/base rates, including Auto. The caller
    /// selects the mode's rate, combo and skill timeline.
    pub fn score_with_judgement_multiplier(
        self,
        judgement_multiplier: f32,
        combo_multiplier: f32,
        skill_multiplier: f32,
        fever_multiplier: f32,
    ) -> Result<NoteScore, ScoringError> {
        let judgement = nonnegative(judgement_multiplier, "judgment multiplier")?;
        let combo = nonnegative(combo_multiplier, "combo multiplier")?;
        let skill = nonnegative(skill_multiplier, "skill multiplier")?;
        let fever = nonnegative(fever_multiplier, "Fever multiplier")?;
        let judged = self.base * judgement;
        let before_skill = note_integer(judged * combo)?;
        let combined = fever * skill;
        let final_score = note_integer(before_skill as f32 * combined)?;
        Ok(NoteScore {
            before_skill,
            final_score,
        })
    }
}

fn note_integer(value: f32) -> Result<u32, ScoringError> {
    // Rust casts saturate; reject invalid scores instead of hiding overflow.
    if value.is_finite() && (0.0..4_294_967_296.0_f32).contains(&value) {
        Ok(value as u32)
    } else {
        Err(ScoringError::NoteScoreOverflow)
    }
}

/// One activation's extra percentage; construct fresh state on actual start.
#[derive(Debug, Clone, Copy)]
pub struct Crescendo {
    base_multiplier: f32,
    increment_percent: f32,
    extra_cap: f32,
    extra_percent: f32,
}

impl Crescendo {
    pub fn new(
        base_percent: f32,
        increment_percent: f32,
        cap_percent: u32,
    ) -> Result<Self, ScoringError> {
        let base_multiplier = skill_multiplier_from_percent(base_percent)?;
        let increment_percent = nonnegative(increment_percent, "crescendo increment")?;
        let cap_percent = cap_percent as f32;
        if increment_percent == 0.0 || cap_percent < base_percent {
            return Err(ScoringError::InvalidCrescendo);
        }
        // Native code derives the ceiling from the rounded ordinary multiplier.
        let original_percent = (base_multiplier - 1.0_f32) * 100.0_f32;
        let extra_cap = cap_percent - original_percent;
        // At an equal base/cap, rounding can make this slightly negative.
        // The native clamp retains that value; forcing zero changes its bits.
        Ok(Self {
            base_multiplier,
            increment_percent,
            extra_cap,
            extra_percent: 0.0,
        })
    }

    /// Effective judgment after correction. P increments before this note's
    /// score; Great retains the extra percentage rather than resetting it.
    pub fn multiplier_for_note(&mut self, judgement: Judgement) -> f32 {
        if judgement == Judgement::Perfect {
            self.extra_percent = (self.extra_percent + self.increment_percent).min(self.extra_cap);
        }
        self.base_multiplier + self.extra_percent / 100.0_f32
    }

    pub fn extra_percent(self) -> f32 {
        self.extra_percent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn perfect_total(power: f32, level: i32, count: u32, fever: f32) -> u32 {
        let scorer = NoteScorer::new(ScoringPower::new(power).unwrap(), level, count).unwrap();
        (1..=count)
            .map(|combo| {
                scorer
                    .score(
                        Judgement::Perfect,
                        normal_combo_multiplier(combo),
                        1.0,
                        fever,
                    )
                    .unwrap()
                    .final_score
            })
            .sum()
    }

    #[test]
    fn original_arm32_regressions() {
        assert_eq!(perfect_total(300_040.75, 27, 760, 1.0), 1_288_520);
        assert_eq!(perfect_total(300_040.0, 27, 760, 1.0), 1_288_470);
        assert_eq!(perfect_total(300_149.0, 27, 760, 1.0), 1_289_040);
        assert_eq!(perfect_total(300_000.0, 27, 500, 2.0), 2_537_240);
        assert_eq!(perfect_total(466_061.0, 27, 760, 1.0), 2_001_650);
    }

    #[test]
    fn crescendo_reaches_exact_cap_and_matches_native_trace() {
        let scorer = NoteScorer::new(ScoringPower::new(400_000.0).unwrap(), 27, 121).unwrap();
        let mut state = Crescendo::new(100.0, 0.5, 150).unwrap();
        let mut total = 0;
        for combo in 1..=121 {
            let multiplier = if combo == 1 {
                1.0
            } else {
                state.multiplier_for_note(Judgement::Perfect)
            };
            let score = scorer
                .score(
                    Judgement::Perfect,
                    normal_combo_multiplier(combo),
                    multiplier,
                    1.0,
                )
                .unwrap();
            if combo == 101 {
                assert_eq!(multiplier, 2.5);
                assert_eq!(
                    score,
                    NoteScore {
                        before_skill: 13708,
                        final_score: 34270
                    }
                );
            }
            total += score.final_score;
        }
        assert_eq!(total, 3_737_735);
        assert_eq!(state.extra_percent(), 50.0);
        assert_eq!(state.multiplier_for_note(Judgement::Great), 2.5);
        assert_eq!(
            Crescendo::new(100.0, 0.5, 150).unwrap().extra_percent(),
            0.0
        );
    }

    #[test]
    fn fever_follows_first_truncation_and_great_keeps_crescendo() {
        let scorer = NoteScorer { base: 1000.5 };
        assert_eq!(
            scorer.score(Judgement::Perfect, 1.0, 1.5, 2.0).unwrap(),
            NoteScore {
                before_skill: 1100,
                final_score: 3300
            }
        );
        let mut state = Crescendo::new(100.0, 0.5, 150).unwrap();
        assert_eq!(state.multiplier_for_note(Judgement::Perfect), 2.005_f32);
        assert_eq!(state.multiplier_for_note(Judgement::Great), 2.005_f32);
        assert_eq!(state.multiplier_for_note(Judgement::Perfect), 2.01_f32);
    }

    #[test]
    fn rejects_invalid_arithmetic_instead_of_saturating() {
        let power = ScoringPower::new(1.0).unwrap();
        assert!(NoteScorer::new(power, 27, 0).is_err());
        assert!(NoteScorer::new(power, 0, 1).is_err());
        let scorer = NoteScorer::new(power, 27, 1).unwrap();
        for value in [f32::NAN, f32::INFINITY, -1.0] {
            assert!(scorer.score(Judgement::Perfect, 1.0, value, 1.0).is_err());
        }
        assert!(note_integer(4_294_967_296.0).is_err());
        assert!(Crescendo::new(100.0, 0.0, 150).is_err());
        assert!(Crescendo::new(100.0, 0.5, 90).is_err());
    }
}

#[cfg(test)]
mod migration_tests;
