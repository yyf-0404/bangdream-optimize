//! Prefix bounds for a whole inventory, before a five-card roster is known.
//! All catalogue durations/corrections widen the six windows. A choice may be
//! the componentwise union of alternatives: power and individual slot gains
//! need not be simultaneously attainable. Only an UPPER bound is exposed.
use super::{
    cache::Geometry,
    coarse::{build_gain, ChartGeometry},
    *,
};
use crate::{hit_timing::ReplaySkill, scoring::linear_power_upper};

mod forced;
mod assignments;
pub use assignments::AssignmentStats;

#[derive(Clone, Copy, Debug, Default)]
pub struct PoolTimingChoice {
    skill: Option<usize>,
    assignment_row: u64,
    diagnostic_minimum: bool,
    linear_power: f64,
    gains: [f64; 6],
}
impl PoolTimingChoice {
    /// Resolved profile identity within the originating PoolTimingUpper only.
    /// Includes duration and judgement correction, not merely score percentage.
    /// A union of different profiles has no single resolved identity.
    pub fn skill_profile(&self) -> Option<usize> {
        self.skill
    }

    /// Diagnostic only: bounds evaluated with these minimum components bound
    /// the PREFILTER VALUES from below, NOT achievable scores. This permits
    /// counting a whole Cartesian subtree that this prefilter cannot discard.
    pub fn minimum_components(self, other: Self) -> Self {
        let gains=std::array::from_fn(|i| self.gains[i].min(other.gains[i]));
        Self {
            skill: None,
            assignment_row: self.row_identity(other,&gains),
            diagnostic_minimum: true,
            linear_power: self.linear_power.min(other.linear_power),
            gains,
        }
    }
    /// Alternatives from the SAME PoolTimingUpper only. Merging different
    /// cards, skills or team-condition cases is a relaxation, never a score.
    pub fn union(self, other: Self) -> Self {
        let gains=std::array::from_fn(|i| self.gains[i].max(other.gains[i]));
        Self {
            skill: if self.skill == other.skill {
                self.skill
            } else {
                None
            },
            assignment_row: self.row_identity(other,&gains),
            diagnostic_minimum: self.diagnostic_minimum || other.diagnostic_minimum,
            linear_power: self.linear_power.max(other.linear_power),
            gains,
        }
    }
    fn row_identity(self, other: Self, gains: &[f64;6]) -> u64 {
        let same=|row: &[f64;6]| row.iter().zip(gains).all(|(a,b)|a.to_bits()==b.to_bits());
        if self.assignment_row!=0 && same(&self.gains) {self.assignment_row}
        else if other.assignment_row!=0 && same(&other.gains) {other.assignment_row}
        else {0}
    }
}

#[derive(Clone)]
pub struct PoolTimingUpper {
    adjacent: Option<std::sync::Arc<std::sync::Mutex<super::net::adjacent::Adjacent>>>,
    max_power: ScoringPower,
    baseline: f64,
    error: f64,
    gains: Vec<[f64; 6]>,
    assignment_rows: std::sync::Arc<Vec<u64>>,
    net_stats: Option<TimingNetStats>,
    rounding_profile: Option<std::sync::Arc<CompiledScore>>,
    assignments: std::sync::Arc<std::sync::Mutex<assignments::Assignments>>,
}

/// Separable upper for five member/trigger assignments at a declared maximum
/// sum of member powers. A captain row includes the sixth trigger as well.
pub struct PoolTimingLinear {
    power_coefficient: f64,
    gain_coefficient: f64,
    error: f64,
}
impl PoolTimingLinear {
    /// Rows are first-five trigger slots; columns are normal/captain.
    pub fn weights(&self, choice: PoolTimingChoice) -> [[f64; 2]; 5] {
        let base = (self.power_coefficient * choice.linear_power).next_up();
        std::array::from_fn(|slot| {
            std::array::from_fn(|captain| {
                let gain = if captain == 0 {
                    choice.gains[slot]
                } else {
                    (choice.gains[slot] + choice.gains[5]).next_up()
                };
                (base + (self.gain_coefficient * gain).next_up()).next_up()
            })
        })
    }
    pub fn error(&self) -> f64 {
        self.error
    }
}
impl PoolTimingUpper {
    /// Reuse the SAME chart, full ordered skill catalogue and window options
    /// at another power cap. Native error is recomputed, not scaled. Adjacent
    /// window/clock caches contain power-independent coefficients and are
    /// shared; their diagnostic counters are cumulative across clones.
    /// The older non-NET envelope has power-dependent gains and is rejected.
    pub fn with_power_cap(&self, max_power: ScoringPower) -> Result<Self, ReplayError> {
        let profile = self.rounding_profile.as_ref().ok_or(ReplayError::Input(
            "power recap requires a NET coefficient model",
        ))?;
        let mut model = self.clone();
        model.error = profile.envelope(max_power)?.rounding_error();
        model.max_power = max_power;
        Ok(model)
    }
    pub fn new_adjacent_net(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
        options: super::super::independent::WindowOptions,
    ) -> Result<Self, ReplayError> {
        Self::from_net(
            TimingNetUpper::new_adjacent_net(input, skills, max_power, options)?,
            max_power,
        )
    }
    pub fn adjacent_stats(&self) -> Option<TimingAdjacentStats> {
        self.adjacent.as_ref().map(|a| a.lock().unwrap().stats())
    }
    /// Opt-in full-frontier contribution bound with a shared per-note baseline.
    pub fn new_frontier_net(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
        options: super::super::independent::WindowOptions,
    ) -> Result<Self, ReplayError> {
        Self::from_net(
            TimingNetUpper::new_frontier_net(input, skills, max_power, options)?,
            max_power,
        )
    }
    /// Net-gain upper on the existing scoring window domains. Replacement
    /// contribution models remain independent; the default upper is unchanged.
    pub fn new_net(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
    ) -> Result<Self, ReplayError> {
        Self::from_net(TimingNetUpper::new(input, skills, max_power)?, max_power)
    }
    pub fn new_stateful_net(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
    ) -> Result<Self, ReplayError> {
        Self::from_net(
            TimingNetUpper::new_stateful(input, skills, max_power)?,
            max_power,
        )
    }
    fn from_net(table: TimingNetUpper, max_power: ScoringPower) -> Result<Self, ReplayError> {
        Ok(Self {
            adjacent: table.adjacent.clone(),
            max_power,
            baseline: table.baseline,
            error: table.error,
            net_stats: Some(table.stats().clone()),
            assignment_rows: assignments::identities::rows(&table.gains),
            gains: table.gains,
            rounding_profile: Some(table.rounding_profile),
            assignments: Default::default(),
        })
    }
    pub fn net_stats(&self) -> Option<&TimingNetStats> {
        self.net_stats.as_ref()
    }
    pub fn new_cached_adjacent_net(cache: &mut TimingNetCache, skills: &[ReplaySkill],
        max_power: ScoringPower, options: super::super::independent::WindowOptions)
        -> Result<Self, ReplayError> {
        Self::from_net(cache.adjacent(skills, max_power, options)?, max_power)
    }
    pub fn assignment_stats(&self) -> AssignmentStats {
        self.assignments.lock().unwrap().stats.clone()
    }
    /// For sum p <= U: gamma*sum(p)*(B+sum(g)) is bounded by
    /// sum(gamma*B*p + gamma*U*g). Native summation, envelope arithmetic and
    /// integer casts remain covered. This is an upper, never an actual score.
    pub fn linearize(&self, max_sum: f64) -> Result<PoolTimingLinear, ScoringError> {
        if !max_sum.is_finite() || max_sum < 0. {
            return Err(ScoringError::InvalidPowerRange);
        }
        // Extra f64 margin covers the outward additions in the pool envelope.
        let gamma = (linear_power_upper(1.) * (1. + 128. * f64::EPSILON)).next_up();
        let power = (gamma * max_sum).next_up();
        if power > f64::from(self.max_power) {
            return Err(ScoringError::InvalidPowerRange);
        }
        Ok(PoolTimingLinear {
            power_coefficient: (gamma * self.baseline).next_up(),
            gain_coefficient: power,
            error: self.error,
        })
    }
    /// Catalogue must contain EVERY resolved skill variant allowed by a team.
    /// max_power must also cover the relaxed power used by prefix queries.
    pub fn new(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
    ) -> Result<Self, ReplayError> {
        replay(input)?;
        if skills.is_empty() || input.skills.len() != 6 {
            return Err(ReplayError::Input(
                "pool bound needs a skill catalogue and six requests",
            ));
        }
        for (i, s) in skills.iter().enumerate() {
            s.validate(i)?;
        }
        let strongest = skills
            .iter()
            .max_by_key(|s| super::cache::correction_key(s.correction))
            .unwrap();
        let mut geometry_input = input.clone();
        geometry_input.skills = vec![strongest.clone(); 6];
        let geometry = Geometry::new(&geometry_input)?;
        let ideal = geometry.ideal_factors();
        let baseline = CompiledScore::new(input.play_level, &ideal)?.envelope(max_power)?;
        let chart = ChartGeometry {
            geometry,
            ideal,
            baseline,
        };
        let mut triggers = [None; 6];
        for (i, n) in input.notes.iter().enumerate() {
            if let Some(s) = n.trigger {
                triggers[s] = Some(i);
            }
        }
        if triggers.iter().any(Option::is_none) {
            return Err(ReplayError::Input("pool bound needs all six requests"));
        }
        let playing: Vec<_> = skills.iter().map(|s| countdown(s.duration) + 1).collect();
        let shortest = *playing.iter().min().unwrap();
        let longest = *playing.iter().max().unwrap();
        let cooling = countdown(0.75) + 1;
        let mut previous = None;
        let mut gains = vec![[0.; 6]; skills.len()];
        let mut error = baseline.rounding_error();
        for slot in 0..6 {
            let (mut first, mut last) = chart.geometry.spans[triggers[slot].unwrap()];
            if let Some((a, b)) = previous {
                first = first.max(a + 1);
                last = last.max(b + 1);
            }
            previous = Some((first + shortest + cooling, last + longest + cooling));
            let mut max_error = 0.0_f64;
            for (i, skill) in skills.iter().enumerate() {
                let gain = build_gain(
                    input,
                    &chart,
                    slot,
                    skill,
                    (first + 1, last + playing[i]),
                    max_power,
                )?;
                gains[i][slot] = (gain.boosted.coefficient_bounds()[1]
                    - gain.normal.coefficient_bounds()[0])
                    .next_up()
                    .max(0.);
                let e = (gain.boosted.rounding_error() + gain.normal.rounding_error()).next_up();
                max_error = max_error.max((e + gain.normal.truncation_loss()).next_up());
            }
            error = (error + max_error).next_up();
        }
        Ok(Self {
            adjacent: None,
            max_power,
            baseline: baseline.coefficient_bounds()[1],
            error,
            assignment_rows: assignments::identities::rows(&gains),
            gains,
            net_stats: None,
            rounding_profile: None,
            assignments: Default::default(),
        })
    }

    /// Sum the three already-rounded native member axes in f64 before calling.
    pub fn choice(&self, skill: usize, linear_power: f64) -> Result<PoolTimingChoice, ReplayError> {
        if !linear_power.is_finite() || linear_power < 0. {
            return Err(ReplayError::Input("invalid linear member power"));
        }
        let gains = *self
            .gains
            .get(skill)
            .ok_or(ReplayError::Input("unknown catalogue skill"))?;
        Ok(PoolTimingChoice {
            skill: Some(skill),
            assignment_row: self.assignment_rows.get(skill).copied().unwrap_or(0),
            diagnostic_minimum: false,
            linear_power,
            gains,
        })
    }

    /// Five distinct member alternatives; all 120 first-five assignments and
    /// all captains are relaxed, including impossible native physical layouts.
    pub fn upper(&self, choices: &[PoolTimingChoice; 5]) -> Result<u64, ReplayError> {
        // The componentwise minimum was a lower bound on a separable filter,
        // not on this temporal one. Zero safely disables that census shortcut.
        if self.adjacent.is_some() && choices.iter().any(|c| c.diagnostic_minimum) {
            return Ok(0);
        }
        let (power, mut coefficient) = self.separable_components(choices)?;
        if let Some(adjacent) = &self.adjacent {
            if let Some(members) = choices.iter().map(|c| c.skill).collect::<Option<Vec<_>>>() {
                // All componentwise unions with unknown skills retain the
                // separable upper. Fixed skills can use the coupled clock.
                let mut adjacent = adjacent
                    .lock()
                    .map_err(|_| ReplayError::Input("adjacent cache poisoned"))?;
                let gain = adjacent.roster(members.try_into().unwrap())?;
                coefficient = coefficient.min((self.baseline + gain).next_up());
            }
        }
        Ok(self.round_upper(power, coefficient))
    }

    /// Cheap first-stage bound using independent trigger gains and the same
    /// native rounding envelope as `upper`, without querying adjacent windows.
    /// Diagnostic minimum choices retain only their separable-filter meaning.
    pub fn separable_upper(&self, choices: &[PoolTimingChoice; 5]) -> Result<u64, ReplayError> {
        let (power, coefficient) = self.separable_components(choices)?;
        Ok(self.round_upper(power, coefficient))
    }

    /// Whether this query can refine the separable bound with an adjacent
    /// roster. Inspecting the choice metadata does not query or lock caches.
    pub fn has_adjacent_roster(&self, choices: &[PoolTimingChoice; 5]) -> bool {
        self.adjacent.is_some()
            && choices
                .iter()
                .all(|c| c.skill.is_some() && !c.diagnostic_minimum)
    }

    fn separable_components(
        &self,
        choices: &[PoolTimingChoice; 5],
    ) -> Result<(f64, f64), ReplayError> {
        let power = choices
            .iter()
            .fold(0., |sum, c| (sum + c.linear_power).next_up());
        let power = linear_power_upper(power).next_up();
        if power > f64::from(self.max_power) {
            return Err(ScoringError::InvalidPowerRange.into());
        }
        let (gain,encore) = self.assignments.lock().unwrap().sum(choices);
        let coefficient = ((self.baseline + gain).next_up() + encore).next_up();
        Ok((power, coefficient))
    }

    fn round_upper(&self, power: f64, coefficient: f64) -> u64 {
        ((power * coefficient).next_up() + self.error)
            .next_up()
            .floor() as u64
    }
}

#[cfg(test)]
mod tests;
