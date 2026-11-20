//! Safe score upper bounds for every frame plan in the declared search domains.
//!
//! Unlike compiled candidate maxima, these bounds do not depend on which plans
//! a heuristic visited. Note choices, Combo rank and skill history are relaxed
//! independently. They are not replacement-card contribution dominance proofs.
#[cfg(test)]
use super::time_judgement;
use super::{
    replay, search, skill::SkillState, NoteWindow, ReplayError, ReplayInput, ScoreEffect,
    REPLAY_FPS,
};
use crate::scoring::{
    CompiledScore, Judgement, NoteFactors, NoteScorer, ScoreEnvelope, ScoringError, ScoringPower,
};
use std::collections::{BTreeMap, HashMap};

mod cache;
pub use cache::{TimingUpperCache, TimingUpperCacheStats};
mod coarse;
mod net;
mod pool;
pub use net::{TimingAdjacentStats, TimingFrontierStats, TimingNetStats, TimingNetUpper,
    TimingNetCache, TimingNetCacheStats, TimingNetPreparationStats};
mod replacement;
pub use coarse::{
    CoarseTimingRoster, CoarseTimingTeam, CoarseTimingUpperCache, CoarseTimingUpperStats,
};
pub use pool::{PoolTimingChoice, PoolTimingLinear, PoolTimingUpper};
pub use replacement::{
    TimingAffineContext, TimingReplacementBounds, TimingReplacementCache, TimingReplacementStats,
};

type Term = [u32; 3];
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingUpperStats {
    pub notes: usize,
    pub unique_terms: usize,
    pub distinct_choices: usize,
    pub choice_terms: usize,
}

pub struct TimingScoreUpper {
    level: i32,
    count: u32,
    terms: Vec<Term>,
    choices: Vec<(Vec<usize>, u32)>,
    componentwise: CompiledScore,
    stats: TimingUpperStats,
}

/// Exposes only the upper side of the relaxed timeline's envelope. Its lower
/// side is NOT an achievable lower bound for any actual frame plan.
pub struct TimingUpperEnvelope(ScoreEnvelope);

impl TimingUpperEnvelope {
    pub fn upper(&self, power: ScoringPower) -> Result<u64, ScoringError> {
        Ok(self.0.bounds(power)?.max)
    }
}

#[derive(Default)]
pub struct TimingUpperScratch {
    terms: Vec<u32>,
}

impl TimingScoreUpper {
    /// Compose exact alternative multiplicities without expanding fixed notes.
    fn from_choice_counts(level: i32, count: u32, counts: BTreeMap<Vec<Term>, u32>)
        -> Result<Self, ReplayError> {
        let mut term_ids = HashMap::new();
        let mut terms = Vec::new();
        let mut choices = BTreeMap::<Vec<usize>, u32>::new();
        let mut componentwise = Vec::new();
        for (variants, n) in counts {
            if n == 0 { continue; }
            if variants.is_empty() { return Err(ReplayError::Input(
                "upper bound has no legal judgement alternative")); }
            let max = std::array::from_fn::<_, 3, _>(|k|
                variants.iter().map(|v| v[k]).max().unwrap());
            componentwise.push((NoteFactors { judgement: f32::from_bits(max[0]),
                combo: f32::from_bits(max[1]), skill: f32::from_bits(max[2]), fever: 1.0 }, n));
            let mut ids = Vec::new();
            for t in variants {
                let next = terms.len();
                ids.push(*term_ids.entry(t).or_insert_with(|| { terms.push(t); next }));
            }
            ids.sort_unstable();
            *choices.entry(ids).or_default() += n;
        }
        let stats = TimingUpperStats { notes: count as usize, unique_terms: terms.len(),
            distinct_choices: choices.len(), choice_terms: choices.keys().map(Vec::len).sum() };
        Ok(Self { level, count, terms, choices: choices.into_iter().collect(),
            componentwise: CompiledScore::from_weighted(level, count, &componentwise)?, stats })
    }

    pub fn new(input: &ReplayInput) -> Result<Self, ReplayError> {
        replay(input)?;
        let domains = search::frame_domains(input)?;
        let spans: Vec<_> = domains
            .iter()
            .map(|d| (*d.iter().min().unwrap(), *d.iter().max().unwrap()))
            .collect();
        let mut trigger_indices = vec![None; input.skills.len()];
        for (i, n) in input.notes.iter().enumerate() {
            if let Some(s) = n.trigger {
                trigger_indices[s] = Some(i);
            }
        }
        let mut windows = Vec::new();
        let mut previous: Option<(u32, u32)> = None;
        for (&i, skill) in trigger_indices.iter().zip(&input.skills) {
            let Some(i) = i else {
                windows.push(None);
                continue;
            };
            let (mut start_min, mut start_max) = spans[i];
            if let Some((ready_min, ready_max)) = previous {
                start_min = start_min.max(ready_min + 1);
                start_max = start_max.max(ready_max + 1);
            }
            let playing = countdown(skill.duration) + 1;
            windows.push(Some((start_min + 1, start_max + playing)));
            let cooling = countdown(0.75) + 1;
            previous = Some((start_min + playing + cooling, start_max + playing + cooling));
        }
        let mut rank_first: Vec<_> = spans.iter().map(|&(a, _)| a).collect();
        rank_first.sort_unstable();
        let possible_first: Vec<Vec<u32>> = windows
            .iter()
            .map(|&window| {
                let Some((first, _)) = window else {
                    return Vec::new();
                };
                let mut times: Vec<_> = spans
                    .iter()
                    .filter(|&&(_, b)| b >= first)
                    .map(|&(a, _)| a)
                    .collect();
                times.sort_unstable();
                times
            })
            .collect();
        let mut crescendo = Vec::new();
        for skill in &input.skills {
            if matches!(skill.score, ScoreEffect::Crescendo { .. }) {
                let mut s = SkillState::new(skill)?;
                // Include the untouched state: equal base/cap may round the
                // first Perfect slightly DOWN. Great preserves current state.
                let mut upper = vec![s.multiplier(Judgement::Great)];
                for _ in &input.notes {
                    upper.push(
                        upper
                            .last()
                            .copied()
                            .unwrap()
                            .max(s.multiplier(Judgement::Perfect)),
                    );
                }
                crescendo.push(upper);
            } else {
                crescendo.push(Vec::new());
            }
        }
        let mut alternatives = Vec::new();
        for (i, domain) in domains.iter().enumerate() {
            let mut variants: Vec<Term> = Vec::new();
            for &frame in domain {
                let raw = input.notes[i].raw_judgement(frame)?;
                // Every other note with any frame <= this one may be before it.
                // Ties deliberately ignore dispatch restrictions (safe relaxation).
                let rank = rank_first.partition_point(|&first| first <= frame) as u32;
                let combo = input
                    .initial_combo
                    .checked_add(rank)
                    .ok_or(ReplayError::Input("combo overflow"))?;
                let rate = input.combo_multiplier(combo);
                let fever = if input
                    .fever_frames
                    .is_some_and(|[a, b]| a <= frame && frame < b)
                {
                    2.0_f32
                } else {
                    1.0
                };
                let mut add = |grade: Judgement, multiplier: f32| {
                    if grade < Judgement::Great {
                        return;
                    }
                    let t = [
                        grade.multiplier().to_bits(),
                        rate.to_bits(),
                        (fever * multiplier).to_bits(),
                    ];
                    if variants.iter().any(|v| dominates(*v, t)) {
                        return;
                    }
                    variants.retain(|&v| !dominates(t, v));
                    variants.push(t);
                };
                add(raw, 1.0);
                for (slot, (&window, skill)) in windows.iter().zip(&input.skills).enumerate() {
                    let Some((first, last)) = window else {
                        continue;
                    };
                    if frame < first
                        || frame > last
                        || input.notes[i].trigger.is_some_and(|s| slot >= s)
                    {
                        continue;
                    }
                    let mut state = SkillState::new(skill)?;
                    let grade = state.correct(raw);
                    let multiplier = if crescendo[slot].is_empty() {
                        // Perfect is evaluated before any previous Great has
                        // lowered Continuous. Current Great still lowers it.
                        state.multiplier(grade)
                    } else {
                        let possible = possible_first[slot].partition_point(|&a| a <= frame);
                        crescendo[slot][possible]
                    };
                    add(grade, multiplier);
                }
            }
            alternatives.push(variants);
        }
        Self::from_alternatives(input.play_level, alternatives)
    }

    fn from_alternatives(level: i32, alternatives: Vec<Vec<Term>>) -> Result<Self, ReplayError> {
        let count = alternatives.len();
        let mut term_ids = HashMap::new();
        let mut terms = Vec::new();
        let mut choices = BTreeMap::<Vec<usize>, u32>::new();
        let mut componentwise = Vec::new();
        for variants in alternatives {
            if variants.is_empty() {
                return Err(ReplayError::Input(
                    "upper bound has no legal judgement alternative",
                ));
            }
            let max =
                std::array::from_fn::<_, 3, _>(|k| variants.iter().map(|v| v[k]).max().unwrap());
            componentwise.push(NoteFactors {
                judgement: f32::from_bits(max[0]),
                combo: f32::from_bits(max[1]),
                skill: f32::from_bits(max[2]),
                fever: 1.0,
            });
            let mut ids = Vec::new();
            for t in variants {
                let next = terms.len();
                ids.push(*term_ids.entry(t).or_insert_with(|| {
                    terms.push(t);
                    next
                }));
            }
            ids.sort_unstable();
            *choices.entry(ids).or_default() += 1;
        }
        let stats = TimingUpperStats {
            notes: count,
            unique_terms: terms.len(),
            distinct_choices: choices.len(),
            choice_terms: choices.keys().map(Vec::len).sum(),
        };
        Ok(Self {
            level,
            count: count as u32,
            terms,
            choices: choices.into_iter().collect(),
            componentwise: CompiledScore::new(level, &componentwise)?,
            stats,
        })
    }

    pub fn stats(&self) -> &TimingUpperStats {
        &self.stats
    }

    /// Constant-time outward envelope, looser than `score` because it also
    /// relaxes judgment/Combo/skill correlation within each note's alternatives.
    pub fn upper_envelope(
        &self,
        max_power: ScoringPower,
    ) -> Result<TimingUpperEnvelope, ScoringError> {
        Ok(TimingUpperEnvelope(self.componentwise.envelope(max_power)?))
    }

    /// Pointwise upper score for ANY legal plan in the fixed configuration.
    /// It must only be compared to an achievable incumbent under the same target.
    pub fn score(
        &self,
        power: ScoringPower,
        scratch: &mut TimingUpperScratch,
    ) -> Result<u64, ScoringError> {
        let scorer = NoteScorer::new(power, self.level, self.count)?;
        scratch.terms.resize(self.terms.len(), 0);
        for (t, v) in self.terms.iter().zip(&mut scratch.terms) {
            *v = scorer
                .score_with_judgement_multiplier(
                    f32::from_bits(t[0]),
                    f32::from_bits(t[1]),
                    f32::from_bits(t[2]),
                    1.0,
                )?
                .final_score;
        }
        Ok(self
            .choices
            .iter()
            .map(|(ids, n)| {
                u64::from(ids.iter().map(|&i| scratch.terms[i]).max().unwrap()) * u64::from(*n)
            })
            .sum())
    }
}
fn dominates(a: Term, b: Term) -> bool {
    (0..3).all(|i| a[i] >= b[i])
}
pub(super) fn countdown(mut t: f32) -> u32 {
    let mut n = 0;
    while t > 0.0 {
        t -= 1.0_f32 / REPLAY_FPS as f32;
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests;
