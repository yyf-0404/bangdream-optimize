use super::{geometry::Factor, *};
use crate::hit_timing::{skill::SkillState, ScoreEffect};
use crate::scoring::{Judgement, NoteScorer, ScoringError};
use std::{cmp::Ordering, collections::HashMap};

pub(super) struct Machine {
    pub values: Vec<[f32; 2]>,
    pub next: Vec<[usize; 2]>,
    pub monotone: bool,
    pub clean_dominance: bool,
    pub maximum: [f32; 2],
    pub baseline_dominance: bool,
}
impl Machine {
    pub fn new(skill: &ReplaySkill, notes: usize) -> Result<Self, ReplayError> {
        let mut s = SkillState::new(skill)?;
        let (values, next) = match skill.score {
            ScoreEffect::Continuous { .. } => {
                let p = s.clone().multiplier(Judgement::Perfect);
                let g = s.multiplier(Judgement::Great);
                (vec![[g, p], [g, g]], vec![[1, 0], [1, 1]])
            }
            ScoreEffect::Crescendo { .. } => {
                let mut values = vec![];
                let mut next = vec![];
                for k in 0..=notes {
                    let before = s.snapshot().extra_percent.to_bits();
                    let g = s.clone().multiplier(Judgement::Great);
                    let p = s.multiplier(Judgement::Perfect);
                    let absorbing = before == s.snapshot().extra_percent.to_bits() || k == notes;
                    values.push([g, p]);
                    next.push([k, if absorbing { k } else { k + 1 }]);
                    if absorbing {
                        break;
                    }
                }
                (values, next)
            }
            _ => (
                vec![[
                    s.clone().multiplier(Judgement::Great),
                    s.multiplier(Judgement::Perfect),
                ]],
                vec![[0, 0]],
            ),
        };
        // Check actual f32 progression, including equal-base/cap rounding down.
        // Continuous has clean->dirty numbering, and its declared high >= low.
        let monotone = !matches!(skill.score, ScoreEffect::Crescendo { .. })
            || values
                .windows(2)
                .all(|v| (0..2).all(|g| v[0][g] <= v[1][g]));
        let maximum = std::array::from_fn(|g| values.iter().map(|v| v[g]).fold(0., f32::max));
        // Clean Perfect stays clean; Great makes both histories dirty. Check
        // the actual native operands rather than deriving them from percentages.
        let clean_dominance = matches!(skill.score, ScoreEffect::Continuous { .. })
            && values.len() == 2 && next == [[1, 0], [1, 1]]
            && (0..2).all(|g| values[0][g].is_finite() && values[1][g].is_finite()
                && values[0][g] >= values[1][g]);
        let baseline_dominance = values.iter().flatten().all(|&v| v.is_finite() && v >= 1.);
        Ok(Self {
            values,
            next,
            monotone,
            clean_dominance,
            maximum,
            baseline_dominance,
        })
    }

    /// A strength ordering that simulates every effective Great/Perfect hit.
    /// Clocks and geometry are compared separately by the caller.
    pub fn progress_strength(&self, effect: &ScoreEffect, progress: usize,
        continuous: bool) -> Option<usize> {
        match effect {
            ScoreEffect::Crescendo { .. } if self.monotone => Some(progress),
            ScoreEffect::Continuous { .. } if continuous && self.clean_dominance =>
                1usize.checked_sub(progress),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Value {
    pub coefficient: f64,
    pub score: u64,
}
impl Value {
    pub fn add(self, other: Self) -> Result<Self, ReplayError> {
        Ok(Self {
            coefficient: self.coefficient + other.coefficient,
            score: self
                .score
                .checked_add(other.score)
                .ok_or(ScoringError::TotalScoreOverflow)?,
        })
    }
    fn times(self, count: usize) -> Result<Self, ReplayError> {
        Ok(Self {
            coefficient: self.coefficient * count as f64,
            score: self
                .score
                .checked_mul(count as u64)
                .ok_or(ScoringError::TotalScoreOverflow)?,
        })
    }
    fn difference(self, other: Self) -> Self {
        Self {
            coefficient: self.coefficient - other.coefficient,
            score: self.score - other.score,
        }
    }
}

pub(super) type PrefixCache = HashMap<(u32, bool), Vec<Value>>;

/// A normal-form clock whose ordinary hits in this interval cannot expire a
/// queue entry or clear ready. Equal classes have the same active skill and
/// progression, so an action that preserves one state preserves them all.
#[derive(Clone, Copy)]
pub(super) struct NativeUniformState {
    pub class: (usize, usize),
    pub first: u32,
    pub last: u32,
}

/// Geometry and trace reconstruction are shared by independent windows and
/// complete fixed timelines. The latter encode the active slot in the state.
pub(super) trait ActionCosts {
    /// Prepared timelines may fix runtime switches once. Other cost models
    /// retain the original dynamic reads, including their controlled tests.
    fn value_controls(&self) -> Controls { Controls::dynamic() }
    /// A second independent future simulation for value-only queries. Higher
    /// strength and an equal prefix score may dominate; callers preserving
    /// witness ties must leave this disabled.
    fn dominance_rank_extra(&self, _state: usize) -> Option<(usize,u32,usize)> { None }
    /// Opt in only when values have nonnegative integer scores, zero
    /// coefficients, and action results do not depend on the prefix score.
    fn supports_native_blocks(&self) -> bool { false }
    /// Equal keys must determine the full ordinary action on every input
    /// state. Trigger and aggregate actions may return None and run directly.
    fn native_block_operator(&self, _index: usize) -> Option<(u32, u8, usize)> { None }
    /// Within this interval equal raw/band operator keys may differ in frame
    /// but must yield the same full transition on this immutable state.
    fn native_uniform_state(&self, _state: usize) -> Option<NativeUniformState> { None }
    /// On ordinary actions, equal raw/band operands have the same complete
    /// transition on this immutable input state throughout the closed cell.
    /// The output clock may differ from the input, e.g. after FIFO expiry.
    fn native_frame_interval(&self, _state: usize, _frame: u32) -> Option<(u32,u32)> { None }
    fn finish_stats(&self, _stats: &mut WindowStats) {}
    fn supports_temporal_intervals(&self) -> bool { false }
    /// On a geometry-certified ordinary-event cell, every hit sequence must
    /// have the same costs and output state at all frames in [first, last].
    fn stable_interval(&self, _state: usize, _first: u32, _last: u32) -> bool { false }
    fn supports_dominance(&self) -> bool {
        false
    }
    /// Opaque future-clock group and strength. Within one geometry node and
    /// group, larger strength must preserve every future legal action and yield
    /// no smaller native increments for every continuation. None disables it.
    fn dominance_rank(&self, _state: usize) -> Option<(usize, u32, usize)> {
        None
    }
    /// Within one geometry node/group, larger values in BOTH coordinates must
    /// simulate every continuation with no smaller increments. This proof
    /// includes the one-coordinate ranks for states that return Some.
    fn dominance_plane(&self, _state: usize) -> Option<(usize, usize, usize)> {
        None
    }
    /// A dynamic schedule may reject a raw Good/Bad hit without an active
    /// correcting skill. Rejection removes the edge, never scores it as zero.
    fn action(
        &mut self,
        action: &geometry::Action,
        state: usize,
    ) -> Result<Option<(usize, Value)>, ReplayError> {
        Ok(Some(if action.hits.len() > 1 {
            self.run(action.factor, state, action.hits.len())?
        } else {
            self.hit(action.factor, state)?
        }))
    }
    /// The stable graph action index lets complete timelines prepare immutable
    /// hit data once, instead of recomputing it for every live skill state.
    fn action_at(
        &mut self,
        _index: usize,
        action: &geometry::Action,
        state: usize,
    ) -> Result<Option<(usize, Value)>, ReplayError> {
        self.action(action, state)
    }
    fn hit(&mut self, factor: Factor, state: usize) -> Result<(usize, Value), ReplayError>;
    fn run(
        &mut self,
        factor: Factor,
        state: usize,
        count: usize,
    ) -> Result<(usize, Value), ReplayError>;
    fn compare(&self, a: Value, b: Value, objective: WindowObjective) -> Ordering {
        match objective {
            WindowObjective::Continuous => a.coefficient.total_cmp(&b.coefficient),
            WindowObjective::Native => a.score.cmp(&b.score),
        }
    }
}

impl ActionCosts for Costs<'_> {
    fn hit(&mut self, f: Factor, state: usize) -> Result<(usize, Value), ReplayError> {
        Costs::hit(self, f, state)
    }
    fn run(
        &mut self,
        f: Factor,
        state: usize,
        count: usize,
    ) -> Result<(usize, Value), ReplayError> {
        Costs::run(self, f, state, count)
    }
}

enum Prefixes<'a> {
    Owned(PrefixCache),
    Reused(&'a mut PrefixCache),
}
impl std::ops::Deref for Prefixes<'_> {
    type Target = PrefixCache;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(p) => p,
            Self::Reused(p) => p,
        }
    }
}
impl std::ops::DerefMut for Prefixes<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Owned(p) => p,
            Self::Reused(p) => p,
        }
    }
}

pub(super) struct Costs<'a> {
    input: &'a ReplayInput,
    scorer: NoteScorer,
    scale: f64,
    machine: &'a Machine,
    net: bool,
    reference_total: f64,
    // Prefixes contain already-native f32 skill/Fever operands and integer scores.
    prefixes: Prefixes<'a>,
    initial_prefixes: usize,
}

pub(super) fn coefficient(input: &ReplayInput, replay: &crate::hit_timing::ReplayResult) -> f64 {
    let scale = f64::from(1.0_f32 + (input.play_level - 5) as f32 * 0.01_f32)
        / f64::from(input.notes.len() as f32)
        * 3.;
    replay
        .trace
        .iter()
        .map(|n| {
            scale
                * f64::from(n.judgement.multiplier())
                * f64::from(input.combo_multiplier(n.combo))
                * f64::from(n.fever_multiplier * n.skill_multiplier)
        })
        .sum()
}
impl<'a> Costs<'a> {
    pub fn new(
        input: &'a ReplayInput,
        power: ScoringPower,
        machine: &'a Machine,
    ) -> Result<Self, ReplayError> {
        Ok(Self {
            input,
            scorer: NoteScorer::new(power, input.play_level, input.notes.len() as u32)?,
            scale: f64::from(1.0_f32 + (input.play_level - 5) as f32 * 0.01_f32)
                / f64::from(input.notes.len() as f32)
                * 3.,
            machine,
            net: false,
            reference_total: 0.,
            prefixes: Prefixes::Owned(HashMap::new()),
            initial_prefixes: 0,
        })
    }
    /// Signed continuous contribution only. It has no standalone native score:
    /// the game's two integer casts apply to the complete note score.
    pub fn net(
        input: &'a ReplayInput,
        machine: &'a Machine,
        reference_total: f64,
    ) -> Result<Self, ReplayError> {
        let mut result = Self::new(input, ScoringPower::new(0.)?, machine)?;
        result.net = true;
        result.reference_total = reference_total;
        Ok(result)
    }
    pub fn prefix_count(&self) -> usize {
        self.prefixes.len() - self.initial_prefixes
    }
    /// Reuse only for the same chart/Combo/Fever and complete skill machine.
    /// Net prefixes contain no power-dependent integer scores or references.
    pub fn net_cached(
        input: &'a ReplayInput,
        machine: &'a Machine,
        reference_total: f64,
        prefixes: &'a mut PrefixCache,
    ) -> Result<Self, ReplayError> {
        let mut result = Self::net(input, machine, reference_total)?;
        result.initial_prefixes = prefixes.len();
        result.prefixes = Prefixes::Reused(prefixes);
        Ok(result)
    }
    pub fn trace_value(&self, replay: &crate::hit_timing::ReplayResult) -> Value {
        Value {
            score: replay.total,
            coefficient: coefficient(self.input, replay),
        }
    }
    pub fn compare(&self, a: Value, b: Value, objective: WindowObjective) -> Ordering {
        match objective {
            WindowObjective::Continuous => a.coefficient.total_cmp(&b.coefficient),
            WindowObjective::Native => a.score.cmp(&b.score),
        }
    }
    fn unit(
        &self,
        perfect: bool,
        combo: f32,
        fever: bool,
        multiplier: f32,
    ) -> Result<Value, ReplayError> {
        let grade = if perfect {
            Judgement::Perfect
        } else {
            Judgement::Great
        };
        let fever = if fever { 2. } else { 1. };
        let score = if self.net {
            0
        } else {
            u64::from(
                self.scorer
                    .score(grade, combo, multiplier, fever)?
                    .final_score,
            )
        };
        // Match the rounded combined native operand, then remove power and casts.
        Ok(Value {
            coefficient: self.scale
                * f64::from(grade.multiplier())
                * f64::from(combo)
                * f64::from(fever * multiplier),
            score,
        })
    }
    fn combo(&self, rank: usize) -> f32 {
        self.input
            .combo_multiplier(self.input.initial_combo + rank as u32)
    }
    pub fn hit(&self, f: Factor, state: usize) -> Result<(usize, Value), ReplayError> {
        if self.net && !f.inside {
            return Ok((0, Value::default()));
        }
        let grade = usize::from(f.perfect);
        let (next, multiplier) = if f.inside {
            (
                self.machine.next[state][grade],
                self.machine.values[state][grade],
            )
        } else {
            // Before a single contiguous window the state is initial; after
            // it, history can never affect a later hit. Merge the entire tail.
            (0, 1.)
        };
        let mut value = self.unit(f.perfect, self.combo(f.rank), f.fever, multiplier)?;
        if self.net {
            value.coefficient = if f.inside {
                value.coefficient - f.reference
            } else {
                0.
            };
        }
        Ok((next, value))
    }
    pub fn run(
        &mut self,
        f: Factor,
        mut state: usize,
        count: usize,
    ) -> Result<(usize, Value), ReplayError> {
        if self.net && !f.inside {
            return Ok((0, Value::default()));
        }
        let mut result = Value::default();
        let mut offset = 0;
        while offset < count {
            let combo = self.combo(f.rank + offset);
            let mut length = 1;
            while offset + length < count
                && self.combo(f.rank + offset + length).to_bits() == combo.to_bits()
            {
                length += 1;
            }
            if !f.inside {
                state = 0;
                result = result.add(self.unit(true, combo, f.fever, 1.)?.times(length)?)?;
            } else if self.machine.next[state][1] == state {
                result = result.add(
                    self.unit(true, combo, f.fever, self.machine.values[state][1])?
                        .times(length)?,
                )?;
            } else {
                let key = (combo.to_bits(), f.fever);
                if !self.prefixes.contains_key(&key) {
                    let mut prefix = vec![Value::default()];
                    for v in &self.machine.values {
                        prefix.push(
                            prefix
                                .last()
                                .copied()
                                .unwrap()
                                .add(self.unit(true, combo, f.fever, v[1])?)?,
                        );
                    }
                    self.prefixes.insert(key, prefix);
                }
                let last = self.machine.values.len() - 1;
                let advancing = length.min(last - state);
                let prefix = &self.prefixes[&key];
                result = result.add(prefix[state + advancing].difference(prefix[state]))?;
                state += advancing;
                if advancing < length {
                    result = result.add(
                        self.unit(true, combo, f.fever, self.machine.values[state][1])?
                            .times(length - advancing)?,
                    )?;
                }
            }
            offset += length;
        }
        if self.net {
            result.coefficient -= f.reference;
        }
        Ok((state, result))
    }
    pub fn upper(&self, notes: &[Vec<Factor>]) -> Result<Value, ReplayError> {
        let mut sum = Value::default();
        for choices in notes {
            let mut best = Value::default();
            for &f in choices {
                if self.net && !f.inside {
                    continue;
                }
                let multiplier = if f.inside {
                    self.machine.maximum[usize::from(f.perfect)]
                } else {
                    1.
                };
                let mut v = self.unit(f.perfect, self.combo(f.rank), f.fever, multiplier)?;
                if self.net {
                    v.coefficient -= f.reference;
                }
                // A fixed subtrahend preserves the optimistic Combo upper.
                // This cheap envelope may relax negative mandatory terms to 0;
                // the complete graph below must still settle and charge them.
                best.coefficient = best.coefficient.max(v.coefficient);
                best.score = best.score.max(v.score);
            }
            sum = sum.add(best)?;
        }
        sum.coefficient += self.coefficient_error();
        sum.coefficient = sum.coefficient.next_up();
        Ok(sum)
    }

    /// Absolute (not relative-to-net-gain) error allowance. Covers cancellation
    /// in signed increments, prefix subtraction, run sums and DP comparisons.
    /// With N chart notes, K<=N+1 machine states and A the absolute contribution
    /// plus reference mass, a complete continuous path has error <=
    /// 128*u*(N+1)*(K+1)*A (u=2^-53). The 256*EPSILON factor is 512*u;
    /// its own rounding still leaves more than that allowance.
    /// Continuous evaluation merges only identical geometry/skill states;
    /// native-only cross-state dominance/temporal skipping is not used here.
    pub fn coefficient_error(&self) -> f64 {
        let n = self.input.notes.len();
        let combo = self.combo(n).max(1.);
        let multiplier = self.machine.maximum[0].max(self.machine.maximum[1]).max(1.);
        let absolute = self.scale.abs()
            * f64::from(Judgement::Perfect.multiplier())
            * f64::from(combo)
            * 2.
            * f64::from(multiplier)
            * n as f64
            + self.reference_total;
        (absolute * 256. * f64::EPSILON * (n + 1) as f64 * (self.machine.values.len() + 1) as f64)
            .next_up()
    }
}

#[cfg(test)]
mod tests;
