//! Exact score caching and inversion on the nonnegative f32 lattice.
use super::{CompiledScore, ScoringError, ScoringPower};
use std::collections::BTreeMap;

/// Inclusive bounds in representable f32 values, not integer power points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerRange {
    min: ScoringPower,
    max: ScoringPower,
}

impl PowerRange {
    pub fn new(min: ScoringPower, max: ScoringPower) -> Result<Self, ScoringError> {
        if min > max {
            return Err(ScoringError::InvalidPowerRange);
        }
        Ok(Self { min, max })
    }
    pub fn min(self) -> ScoringPower {
        self.min
    }
    pub fn max(self) -> ScoringPower {
        self.max
    }
    pub fn contains(self, power: ScoringPower) -> bool {
        self.min <= power && power <= self.max
    }
    pub fn intersect(self, other: Self) -> Option<Self> {
        Self::new(self.min.max(other.min), self.max.min(other.max)).ok()
    }

    /// Each slice holds all possible final [P,T,V] rows for that physical slot.
    /// Independent per-axis extrema relax correlations and card uniqueness.
    /// Evaluating the same f32 additions on these extrema therefore encloses
    /// every legal completion, without a guessed error allowance.
    pub fn from_member_choices(choices: &[&[[f32; 3]]]) -> Result<Self, ScoringError> {
        let mut lows = Vec::with_capacity(choices.len());
        let mut highs = Vec::with_capacity(choices.len());
        for slot in choices {
            if slot.is_empty() {
                return Err(ScoringError::InvalidPowerRange);
            }
            let mut low = [f32::MAX; 3];
            let mut high = [0.0_f32; 3];
            for &row in *slot {
                for i in 0..3 {
                    let value = super::nonnegative(row[i], "member parameter")?;
                    low[i] = low[i].min(value);
                    high[i] = high[i].max(value);
                }
            }
            lows.push(low);
            highs.push(high);
        }
        Self::new(
            ScoringPower::from_member_parameters(&lows)?,
            ScoringPower::from_member_parameters(&highs)?,
        )
    }
}

/// Bound to one immutable resolved timeline and one validated power domain.
/// A chart/mode/skill change constructs another cache; there are no raw chart
/// pointer keys that can alias a reused allocation or a reinitialized chart.
pub struct CachedScore<'a> {
    plan: &'a CompiledScore,
    domain: PowerRange,
    values: BTreeMap<ScoringPower, u64>,
    lower_bounds: BTreeMap<u64, Option<ScoringPower>>,
    evaluations: usize,
}

impl<'a> CachedScore<'a> {
    pub fn new(plan: &'a CompiledScore, domain: PowerRange) -> Result<Self, ScoringError> {
        // All factors are nonnegative: a valid maximum validates all smaller
        // powers. Do not interpret overflowing scores as ordinary high scores.
        let maximum = plan.score(domain.max)?;
        Ok(Self {
            plan,
            domain,
            values: BTreeMap::from([(domain.max, maximum)]),
            lower_bounds: BTreeMap::new(),
            evaluations: 1,
        })
    }
    pub fn domain(&self) -> PowerRange {
        self.domain
    }
    pub fn evaluations(&self) -> usize {
        self.evaluations
    }
    pub fn score(&mut self, power: ScoringPower) -> Result<u64, ScoringError> {
        if !self.domain.contains(power) {
            return Err(ScoringError::InvalidPowerRange);
        }
        if let Some(&score) = self.values.get(&power) {
            return Ok(score);
        }
        let score = self.plan.score(power)?;
        self.values.insert(power, score);
        self.evaluations += 1;
        Ok(score)
    }

    /// First representable power that reaches target; None if unreachable.
    /// IEEE nonnegative finite bit order is numerical order, so this takes at
    /// most 31 midpoint evaluations even when the interval includes subnormals.
    pub fn lower_bound(&mut self, target: u64) -> Result<Option<ScoringPower>, ScoringError> {
        if let Some(&cached) = self.lower_bounds.get(&target) {
            return Ok(cached);
        }
        let result = if self.score(self.domain.max)? < target {
            None
        } else if self.score(self.domain.min)? >= target {
            Some(self.domain.min)
        } else {
            let mut low = self.domain.min.cache_key();
            let mut high = self.domain.max.cache_key();
            while low < high {
                let mid = low + (high - low) / 2;
                if self.score(power_from_bits(mid))? < target {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }
            Some(power_from_bits(low))
        };
        self.lower_bounds.insert(target, result);
        Ok(result)
    }

    /// All representable powers whose final score is in [minimum, maximum].
    /// This is a score preimage, not a promise that a real team has every power.
    pub fn powers_for_scores(
        &mut self,
        minimum: u64,
        maximum: u64,
    ) -> Result<Option<PowerRange>, ScoringError> {
        if minimum > maximum {
            return Ok(None);
        }
        let Some(low) = self.lower_bound(minimum)? else {
            return Ok(None);
        };
        let high = match maximum.checked_add(1) {
            Some(target) => match self.lower_bound(target)? {
                Some(end) if end <= low => return Ok(None),
                Some(end) => power_from_bits(end.cache_key() - 1),
                None => self.domain.max,
            },
            None => self.domain.max,
        };
        Ok(PowerRange::new(low, high).ok())
    }
}

fn power_from_bits(bits: u32) -> ScoringPower {
    // Only reached with bits between validated, finite, nonnegative endpoints.
    ScoringPower::new(f32::from_bits(bits)).expect("validated f32 lattice")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::NoteFactors;

    fn plan(count: usize, skill: f32) -> CompiledScore {
        CompiledScore::new(
            27,
            &(0..count)
                .map(|i| NoteFactors {
                    judgement: 1.1,
                    combo: crate::scoring::normal_combo_multiplier(i as u32 + 1),
                    skill,
                    fever: 1.0,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    fn p(v: f32) -> ScoringPower {
        ScoringPower::new(v).unwrap()
    }

    #[test]
    fn fractional_scores_are_distinct_cache_entries_and_domains_are_immutable() {
        let a = plan(760, 1.0);
        let b = plan(760, 2.2);
        let domain = PowerRange::new(p(300040.0), p(300041.0)).unwrap();
        let mut cache = CachedScore::new(&a, domain).unwrap();
        assert_ne!(
            cache.score(p(300040.0)).unwrap(),
            cache.score(p(300040.75)).unwrap()
        );
        let count = cache.evaluations();
        assert_eq!(
            cache.score(p(300040.75)).unwrap(),
            a.score(p(300040.75)).unwrap()
        );
        assert_eq!(count, cache.evaluations());
        let mut other = CachedScore::new(&b, domain).unwrap();
        assert_ne!(
            cache.score(p(300040.75)).unwrap(),
            other.score(p(300040.75)).unwrap()
        );
        assert!(cache.score(p(300039.0)).is_err());
    }

    #[test]
    fn inversion_matches_exhaustive_float_lattice_including_plateaus_and_gaps() {
        for (low, width) in [
            (0_u32, 96),
            (p(300040.0).cache_key(), 512),
            (p(524288.0).cache_key() - 128, 512),
        ] {
            let model = plan(127, 2.5);
            let domain =
                PowerRange::new(power_from_bits(low), power_from_bits(low + width)).unwrap();
            let values: Vec<_> = (low..=low + width)
                .map(|bits| model.score(power_from_bits(bits)).unwrap())
                .collect();
            let mut cache = CachedScore::new(&model, domain).unwrap();
            for target in values[0].saturating_sub(1)..=values[width as usize] + 1 {
                let first = values
                    .iter()
                    .position(|&s| s >= target)
                    .map(|i| power_from_bits(low + i as u32));
                assert_eq!(cache.lower_bound(target).unwrap(), first);
                let range = cache.powers_for_scores(target, target).unwrap();
                for (i, &value) in values.iter().enumerate() {
                    assert_eq!(
                        range.is_some_and(|r| r.contains(power_from_bits(low + i as u32))),
                        value == target
                    );
                }
            }
            let before = cache.evaluations();
            cache.lower_bound(values[0]).unwrap();
            assert_eq!(before, cache.evaluations());
            assert_eq!(cache.powers_for_scores(0, u64::MAX).unwrap(), Some(domain));
        }
    }

    #[test]
    fn validates_overflow_and_finds_boundary_across_the_full_nonnegative_domain() {
        let model = plan(760, 1.0);
        assert!(CachedScore::new(&model, PowerRange::new(p(0.0), p(f32::MAX)).unwrap()).is_err());
        let mut cache =
            CachedScore::new(&model, PowerRange::new(p(0.0), p(1000000.0)).unwrap()).unwrap();
        let before = cache.evaluations();
        let result = cache.lower_bound(1288520).unwrap().unwrap();
        assert!(cache.evaluations() - before <= 32);
        assert!(cache.score(result).unwrap() >= 1288520);
        assert!(
            cache
                .score(power_from_bits(result.cache_key() - 1))
                .unwrap()
                < 1288520
        );
    }
}
/// At most six rounded additions occur on an axis-to-team path. This bound
/// applies to nonnegative, normal member parameters; zero is exact.
pub(crate) const POWER_GAMMA: f64 = (6.0 / 16_777_216.0) / (1.0 - 6.0 / 16_777_216.0);

/// Covers member-axis accumulation, native score products/division and the
/// integer-to-float stage. Coefficients already use native f32 operands. Floors
/// only decrease an upper bound. 32 operations exceeds every dependency path.
pub(crate) const SCORE_GAMMA: f64 = (32.0 / 16_777_216.0) / (1.0 - 32.0 / 16_777_216.0);

pub(crate) fn linear_score_upper(value: f64) -> f64 {
    (value.max(0.0) * (1.0 + SCORE_GAMMA) + value.abs() * 32.0 * f64::EPSILON).ceil()
}

pub(crate) fn linear_power_lower(value: f64) -> f64 {
    (value / (1.0 + POWER_GAMMA + 64.0 * f64::EPSILON)).max(0.0)
}
pub(crate) fn linear_power_upper(value: f64) -> f64 {
    value / (1.0 - POWER_GAMMA - 64.0 * f64::EPSILON)
}
