//! Continuous NET upper gains: baseline and boosted value use the same frame
//! before subtraction, and all notes share one window start. Scoring reuses
//! the existing delayed-window domain; independent replacement research has
//! an explicit separate constructor with no neighbor propagation or guards.
use super::super::ReplaySkill;
use super::cache::Geometry;
use super::replacement::frames::{corrected, FrameBounds};
use super::*;
use crate::timing::Instant;
pub(super) mod adjacent;
mod frontier;
pub use adjacent::TimingAdjacentStats;
mod state;
pub(super) mod intervals;
mod runs;
mod profiles;
mod preparation;
mod stateless;
pub use preparation::Stats as TimingNetPreparationStats;
pub use profiles::{TimingNetCache, TimingNetCacheStats};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingFrontierStats {
    pub revision: &'static str,
    pub baseline_coefficient: f64,
    /// Same-reference old upper retained when tighter, including budget cases.
    pub reference_capped_slots: usize,
    pub max_cap_reduction: f64,
    pub windows: super::super::independent::WindowStats,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingNetStats {
    pub stateless_profiles: usize,
    pub stateless_rows: usize,
    pub stateless_evaluations: u64,
    pub coefficient_hits: usize,
    pub frame_runs: bool,
    pub option_evaluations: u64,
    pub adjacent_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontier: Option<TimingFrontierStats>,
    pub stateful: bool,
    pub state_rows: u64,
    pub state_skipped_rows: u64,
    pub state_solved_rows: u64,
    pub state_budget_rows: u64,
    pub state_infeasible_rows: u64,
    pub mandatory_note_rows: u64,
    pub state_tightened_rows: u64,
    pub state_transitions: u64,
    /// Actual attempts after repeated closures have been omitted.
    pub state_evaluated_transitions: u64,
    pub state_skipped_frames: u64,
    pub state_max_frontier: usize,
    pub delayed_windows: bool,
    pub build_seconds: f64,
    pub geometries: usize,
    pub profiles: usize,
    pub request_rows: usize,
    pub note_options: usize,
    pub candidate_note_visits: u64,
    pub exhaustive_note_visits: u64,
}
struct OptionValue {
    grade: usize,
    weight: f64,
    previous: usize,
}
struct NoteOptions {
    baseline: f64,
    outside: bool,
    options: Vec<OptionValue>,
    index: usize,
}
type Rows = [Vec<Vec<NoteOptions>>; 6];

pub struct TimingNetUpper {
    pub(super) adjacent: Option<std::sync::Arc<std::sync::Mutex<adjacent::Adjacent>>>,
    pub(super) baseline: f64,
    pub(super) error: f64,
    pub(super) gains: Vec<[f64; 6]>,
    pub(super) rounding_profile: std::sync::Arc<CompiledScore>,
    keys: HashMap<[u32; 6], usize>,
    max_power: ScoringPower,
    stats: TimingNetStats,
}
impl TimingNetUpper {
    pub fn new(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
    ) -> Result<Self, ReplayError> {
        Self::build(input, skills, max_power, true, false)
    }
    pub fn new_independent(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
    ) -> Result<Self, ReplayError> {
        Self::build(input, skills, max_power, false, false)
    }
    pub fn new_stateful(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
    ) -> Result<Self, ReplayError> {
        Self::build(input, skills, max_power, true, true)
    }
    pub fn new_independent_stateful(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
    ) -> Result<Self, ReplayError> {
        Self::build(input, skills, max_power, false, true)
    }
    fn build(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
        delayed_windows: bool,
        stateful: bool,
    ) -> Result<Self, ReplayError> {
        Self::build_indexed(input,skills,max_power,delayed_windows,stateful,
            std::env::var("BANDORI_TIMING_DISABLE_NET_INTERVAL_INDEX").as_deref() != Ok("1"))
    }
    fn build_indexed(input: &ReplayInput, skills: &[ReplaySkill], max_power: ScoringPower,
        delayed_windows: bool, stateful: bool, indexed: bool) -> Result<Self, ReplayError> {
        Self::build_runs(input, skills, max_power, delayed_windows, stateful, indexed,
            std::env::var("BANDORI_TIMING_DISABLE_NET_FRAME_RUNS").as_deref() != Ok("1"))
    }
    fn build_runs(input: &ReplayInput, skills: &[ReplaySkill], max_power: ScoringPower,
        delayed_windows: bool, stateful: bool, indexed: bool, compressed: bool)
        -> Result<Self, ReplayError> {
        Self::build_cached(input, skills, max_power, delayed_windows, stateful, indexed, compressed,
            std::env::var("BANDORI_TIMING_DISABLE_LAZY_NET_ENVELOPES").as_deref() != Ok("1"), None, None)
    }
    fn build_cached(input: &ReplayInput, skills: &[ReplaySkill], max_power: ScoringPower,
        delayed_windows: bool, stateful: bool, indexed: bool, compressed: bool,
        lazy_envelopes: bool, profiles: Option<&mut profiles::Profiles>,
        preparation: Option<&mut preparation::Preparation>)
        -> Result<Self, ReplayError> {
        Self::build_cached_controlled(input,skills,max_power,delayed_windows,stateful,indexed,compressed,
            lazy_envelopes,std::env::var("BANDORI_TIMING_DISABLE_STATELESS_NET_ROWS").as_deref()!=Ok("1"),
            profiles,preparation)
    }
    fn build_cached_controlled(input: &ReplayInput, skills: &[ReplaySkill], max_power: ScoringPower,
        delayed_windows: bool, stateful: bool, indexed: bool, compressed: bool,
        lazy_envelopes: bool, constant_rows: bool, mut profiles: Option<&mut profiles::Profiles>,
        mut preparation: Option<&mut preparation::Preparation>)
        -> Result<Self, ReplayError> {
        let start = Instant::now();
        replay(input)?;
        if skills.is_empty() || input.skills.len() != 6 {
            return Err(ReplayError::Input(
                "net table needs a catalogue and six requests",
            ));
        }
        for (i, s) in skills.iter().enumerate() {
            s.validate(i)?;
        }
        let domain_key = profiles.as_ref().map(|_| {
            assert!(delayed_windows && stateful, "coefficient owner uses stateful delayed windows");
            [skills.iter().map(|s| s.cache_key()[1]).max().unwrap(),
                skills.iter().map(|s| countdown(s.duration)+1).min().unwrap(),
                skills.iter().map(|s| countdown(s.duration)+1).max().unwrap()]
        });
        let mut frames = if let Some(preparation) = preparation.as_mut() {
            preparation.frames(input,skills)?
        } else { FrameBounds::new(input, skills)? };
        if frames.requests.iter().any(Vec::is_empty) {
            return Err(ReplayError::Input("net table needs all six requests"));
        }
        if delayed_windows {
            // Exactly the existing pool/coarse upper's start-range recurrence;
            // no risk detection, eligibility changes or skipped profiles.
            let shortest = skills
                .iter()
                .map(|s| countdown(s.duration) + 1)
                .min()
                .unwrap();
            let longest = skills
                .iter()
                .map(|s| countdown(s.duration) + 1)
                .max()
                .unwrap();
            let cooling = countdown(0.75) + 1;
            let mut previous: Option<(u32, u32)> = None;
            for requests in &mut frames.requests {
                let mut first = *requests.iter().min().unwrap();
                let mut last = *requests.iter().max().unwrap();
                if let Some((a, b)) = previous {
                    first = first.max(a + 1);
                    last = last.max(b + 1);
                }
                previous = Some((first + shortest + cooling, last + longest + cooling));
                *requests = (first..=last).collect();
            }
        }
        // A real reference coefficient per note, not an achievable baseline.
        // Subtract this SAME value when computing the note's skill net gain.
        let base = frames.note_baselines();
        let baseline = base.iter().fold(0_f64, |v, &b| (v + b).next_up());
        let mut stats = TimingNetStats {
            frame_runs: compressed,
            delayed_windows,
            stateful,
            ..TimingNetStats::default()
        };
        let mut geometry = BTreeMap::<(u32, u32), Rows>::new();
        let mut keys = HashMap::new();
        let mut gains = Vec::new();
        let mut max_multiplier = 1_f32;
        let event_ranks = crate::hit_timing::routing::event_ranks(input);
        let interval_index = &frames.interval_index;
        let mut runs = None;
        for skill in skills {
            let key = skill.cache_key();
            let playing = countdown(skill.duration) + 1;
            let profile_key = domain_key.map(|d| std::array::from_fn(|i| if i<3 { d[i] } else { key[i-3] }));
            if let Some((profile, maximum)) = profiles.as_mut()
                .and_then(|p| p.get(profile_key.unwrap())) {
                max_multiplier = max_multiplier.max(maximum);
                keys.insert(key, gains.len());
                gains.push(profile);
                stats.profiles += 1;
                stats.coefficient_hits += 1;
                continue;
            }
            // Cached coefficients consume neither row geometry nor note runs.
            // Build runs at the first real row request; the immutable interval
            // index is already present in FrameBounds.
            if compressed && runs.is_none() {
                runs = Some(frames.notes.iter().map(|n| runs::NoteRuns::new(n))
                    .collect::<Vec<_>>());
            }
            if constant_rows && stateful && compressed
                && !matches!(skill.score,ScoreEffect::Continuous {..}|ScoreEffect::Crescendo {..}) {
                let state=SkillState::new(skill)?;
                let values=[state.clone().multiplier(Judgement::Great),
                    state.clone().multiplier(Judgement::Perfect)];
                let maximum=values.into_iter().fold(1.,f32::max);
                let profile=stateless::profile(&frames,runs.as_ref().unwrap(),&base,
                    playing,key[1],values,indexed,&mut stats);
                keys.insert(key,gains.len());gains.push(profile);
                stats.profiles+=1;stats.stateless_profiles+=1;
                max_multiplier=max_multiplier.max(maximum);
                if let Some(p)=profiles.as_mut() {p.store(profile_key.unwrap(),profile,maximum);}
                continue;
            }
            let rows = geometry.entry((playing, key[1])).or_insert_with(|| {
                stats.geometries += 1;
                std::array::from_fn(|slot| {
                    frames.requests[slot]
                        .iter()
                        .map(|&request| {
                            stats.request_rows += 1;
                            let candidates = if indexed {
                                interval_index.query(request + 1, request + playing)
                            } else { (0..frames.notes.len()).collect() };
                            if indexed { stats.candidate_note_visits += candidates.len() as u64; }
                            else { stats.exhaustive_note_visits += candidates.len() as u64; }
                            let possible: Vec<_> = candidates.iter()
                                .map(|&i| {
                                    let n = &frames.notes[i];
                                    if frames.triggers[i].is_some_and(|s| s <= slot) {
                                        return None;
                                    }
                                    if let Some(runs) = &runs {
                                        return runs[i].possible(request, request + playing, key[1]);
                                    }
                                    n.iter()
                                        .filter(|f| {
                                            request < f.time
                                                && f.time <= request + playing
                                                && corrected(f.raw, key[1]).is_some()
                                        })
                                        .map(|f| f.time)
                                        .min()
                                })
                                .collect();
                            let mut first: Vec<_> = possible.iter().flatten().copied().collect();
                            first.sort_unstable();
                            candidates.into_iter().zip(possible)
                                .filter_map(|(i, possible)| {
                                    let n = &frames.notes[i];
                                    if frames.triggers[i].is_some_and(|s| s <= slot) {
                                        return None;
                                    }
                                    // Keep even an impossible mandatory note: an
                                    // empty inside domain invalidates this row.
                                    // Any external frame stays allowed here;
                                    // another skill may correct its raw grade.
                                    let outside = runs.as_ref().map_or_else(
                                        || n.iter().any(|f| f.time <= request || f.time > request + playing),
                                        |runs| runs[i].outside(request, request + playing));
                                    if possible.is_none() && outside {
                                        return None;
                                    }
                                    let options = if let Some(runs) = &runs {
                                        runs[i].options(request, request + playing, key[1],
                                            &first, &mut stats.option_evaluations)
                                    } else {
                                        let mut options = Vec::<OptionValue>::new();
                                        for f in n {
                                        if !(request < f.time && f.time <= request + playing) {
                                            continue;
                                        }
                                        let Some(grade) = corrected(f.raw, key[1]) else {
                                            continue;
                                        };
                                        stats.option_evaluations += 1;
                                        let judged = if grade == 0 {
                                            Judgement::Great
                                        } else {
                                            Judgement::Perfect
                                        };
                                        let weight = (f.weight[1] * f64::from(judged.multiplier()))
                                            .next_up();
                                        // Potential prior accepted notes, ties included,
                                        // may all be Perfect in this upper; exclude self.
                                        let previous = first
                                            .partition_point(|&t| t <= f.time)
                                            .saturating_sub(1);
                                        if let Some(o) = options
                                            .iter_mut()
                                            .find(|o| o.grade == grade && o.previous == previous)
                                        {
                                            o.weight = o.weight.max(weight);
                                        } else {
                                            options.push(OptionValue {
                                                grade,
                                                weight,
                                                previous,
                                            });
                                        }
                                        }
                                        options
                                    };
                                    stats.note_options += options.len();
                                    stats.mandatory_note_rows += u64::from(!outside);
                                    Some(NoteOptions {
                                        baseline: base[i],
                                        outside,
                                        options,
                                        index: i,
                                    })
                                })
                                .collect()
                        })
                        .collect()
                })
            });
            let mut state = SkillState::new(skill)?;
            let mut multipliers = Vec::with_capacity(input.notes.len() + 1);
            let mut profile_maximum = 1_f32;
            for _ in 0..=input.notes.len() {
                // Clean Continuous state is an upper. Crescendo uses native
                // f32 progression, capped by possible prior accepted notes.
                let g = state.clone().multiplier(Judgement::Great);
                let p = state.clone().multiplier(Judgement::Perfect);
                profile_maximum = profile_maximum.max(g).max(p);
                multipliers.push([g, p]);
                state.multiplier(Judgement::Perfect);
            }
            // Include the initial state even if a native crescendo step rounds down.
            for i in 1..multipliers.len() {
                for g in 0..2 {
                    multipliers[i][g] = multipliers[i][g].max(multipliers[i - 1][g]);
                }
            }
            let machine = if stateful {
                state::Machine::new(skill, input.notes.len())?
            } else {
                None
            };
            let profile = std::array::from_fn(|slot| {
                // Stateless stateful rows use their signed mandatory-note
                // envelope below. Their first, unsigned envelope was unused.
                let envelopes = (!lazy_envelopes || machine.is_some() || !stateful).then(|| rows[slot]
                    .iter()
                    .map(|notes| {
                        let envelope = notes.iter().fold(0_f64, |sum, n| {
                            let gain = n
                                .options
                                .iter()
                                .map(|o| {
                                    ((o.weight * f64::from(multipliers[o.previous][o.grade]))
                                        .next_up()
                                        - n.baseline)
                                        .next_up()
                                })
                                .fold(0., f64::max);
                            (sum + gain).next_up()
                        });
                        envelope
                    })
                    .collect::<Vec<_>>());
                if let Some(machine) = &machine {
                    state::maximum_profile(envelopes.as_ref().unwrap(), machine, &mut stats, |row| {
                        let request = frames.requests[slot][row];
                        rows[slot][row]
                            .iter()
                            .map(|note| state::StateNote {
                                baseline: note.baseline,
                                outside: note.outside,
                                events: frames.notes[note.index]
                                    .iter()
                                    .filter_map(|f| {
                                        if !(request < f.time && f.time <= request + playing) {
                                            return None;
                                        }
                                        let grade = corrected(f.raw, key[1])?;
                                        let judged = if grade == 0 {
                                            Judgement::Great
                                        } else {
                                            Judgement::Perfect
                                        };
                                        Some(state::Event {
                                            time: f.time,
                                            rank: event_ranks[note.index],
                                            grade,
                                            weight: (f.weight[1] * f64::from(judged.multiplier()))
                                                .next_up(),
                                        })
                                    })
                                    .collect(),
                            })
                            .collect()
                    })
                } else if stateful {
                    // Stateless effects need no DP, but a mandatory note must
                    // still pay its best inside value, even when it is negative.
                    rows[slot]
                        .iter()
                        .map(|notes| {
                            notes.iter().fold(0_f64, |sum, n| {
                                let gain = n
                                    .options
                                    .iter()
                                    .map(|o| {
                                        ((o.weight * f64::from(multipliers[o.previous][o.grade]))
                                            .next_up()
                                            - n.baseline)
                                            .next_up()
                                    })
                                    .fold(if n.outside { 0. } else { f64::NEG_INFINITY }, f64::max);
                                (sum + gain).next_up()
                            })
                        })
                        // Same nonnegative power-linearization relaxation as DP.
                        .fold(0., f64::max)
                } else {
                    envelopes.unwrap().into_iter().fold(0., f64::max)
                }
            });
            keys.insert(key, gains.len());
            gains.push(profile);
            stats.profiles += 1;
            max_multiplier = max_multiplier.max(profile_maximum);
            if let Some(p) = profiles.as_mut() {
                p.store(profile_key.unwrap(), profile, profile_maximum);
            }
        }
        // Componentwise maxima bound each native f32 operation's rounding
        // error, even though their joint continuous value is unattainable.
        // Both casts round down, so no truncation loss is added to this upper.
        let rounding_key = [skills.iter().map(|s| s.cache_key()[1]).max().unwrap(),max_multiplier.to_bits()];
        let cached_rounding = preparation.as_mut().and_then(|p| p.rounding(rounding_key));
        let rounding_profile = if let Some(profile) = cached_rounding { profile } else {
            let mut widened = input.clone();
            let strongest = skills.iter().max_by_key(|s| s.cache_key()[1]).unwrap();
            widened.skills = vec![strongest.clone(); 6];
            let geometry = Geometry::new(&widened)?;
            let high: Vec<_> = geometry
                .ideal_factors()
                .into_iter()
                .map(|n| NoteFactors {
                    skill: max_multiplier,
                    ..n
                })
                .collect();
            let rounding_profile = std::sync::Arc::new(CompiledScore::new(input.play_level, &high)?);
            if let Some(p) = preparation.as_mut() { p.store_rounding(rounding_key,rounding_profile.clone()); }
            rounding_profile
        };
        let error = rounding_profile.envelope(max_power)?.rounding_error();
        stats.build_seconds = start.elapsed().as_secs_f64();
        Ok(Self {
            adjacent: None,
            baseline,
            error,
            gains,
            rounding_profile,
            keys,
            max_power,
            stats,
        })
    }
    pub fn stats(&self) -> &TimingNetStats {
        &self.stats
    }
    fn score(&self, c: f64, p: ScoringPower) -> Result<u64, ReplayError> {
        if p > self.max_power {
            return Err(ScoringError::InvalidPowerRange.into());
        }
        Ok(((f64::from(p) * c).next_up() + self.error)
            .next_up()
            .floor() as u64)
    }
    fn row(&self, s: &ReplaySkill) -> Result<[f64; 6], ReplayError> {
        self.keys
            .get(&s.cache_key())
            .map(|&i| self.gains[i])
            .ok_or(ReplayError::Input("skill outside net catalogue"))
    }
    pub fn upper_sequence(
        &self,
        skills: &[ReplaySkill; 6],
        power: ScoringPower,
    ) -> Result<u64, ReplayError> {
        let mut c = self.baseline;
        for (s, skill) in skills.iter().enumerate() {
            c = (c + self.row(skill)?[s]).next_up();
        }
        if let Some(adjacent) = &self.adjacent {
            let sequence: [usize; 6] = skills
                .iter()
                .map(|s| {
                    self.keys
                        .get(&s.cache_key())
                        .copied()
                        .ok_or(ReplayError::Input("skill outside net catalogue"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .unwrap();
            let gain = adjacent
                .lock()
                .map_err(|_| ReplayError::Input("adjacent cache poisoned"))?
                .sequence(sequence)?;
            c = c.min((self.baseline + gain).next_up());
        }
        self.score(c, power)
    }
    pub fn upper_roster(
        &self,
        members: &[ReplaySkill; 5],
        power: ScoringPower,
    ) -> Result<u64, ReplayError> {
        let rows: Vec<_> = members
            .iter()
            .map(|s| self.row(s))
            .collect::<Result<_, _>>()?;
        let mut dp = [f64::NEG_INFINITY; 32];
        dp[0] = 0.;
        for mask in 0usize..31 {
            let slot = mask.count_ones() as usize;
            for (i, row) in rows.iter().enumerate() {
                if mask & (1 << i) == 0 {
                    let next = mask | (1 << i);
                    dp[next] = dp[next].max((dp[mask] + row[slot]).next_up());
                }
            }
        }
        let encore = rows.iter().map(|r| r[5]).fold(0., f64::max);
        let mut coefficient = ((self.baseline + dp[31]).next_up() + encore).next_up();
        if let Some(adjacent) = &self.adjacent {
            let members: [usize; 5] = members
                .iter()
                .map(|s| {
                    self.keys
                        .get(&s.cache_key())
                        .copied()
                        .ok_or(ReplayError::Input("skill outside net catalogue"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .unwrap();
            let gain = adjacent
                .lock()
                .map_err(|_| ReplayError::Input("adjacent cache poisoned"))?
                .roster(members)?;
            coefficient = coefficient.min((self.baseline + gain).next_up());
        }
        self.score(coefficient, power)
    }
}

#[cfg(test)]
mod tests;
