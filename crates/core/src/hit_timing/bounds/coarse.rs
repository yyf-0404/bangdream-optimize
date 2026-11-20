//! Cheap, certified upper bounds across every native order of a fixed layout.
//! For each note, max(B, U1, ..., U6) <= B + sum(max(Uj-B, 0)). We relax
//! judgment/Combo/Fever to one ideal baseline B and every skill to its maximum
//! state. Each affected Uj >= B; two certified envelopes bound Uj-B without
//! moving either native integer cast or treating an unattainable lower as a plan.
use super::cache::{correction_key, Geometry};
use super::*;
use crate::hit_timing::ReplaySkill;

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoarseTimingUpperStats {
    pub geometries: usize,
    pub profile_builds: u64,
    pub profile_hits: u64,
}

#[derive(Clone, Copy)]
pub(super) struct Gain {
    pub(super) normal: ScoreEnvelope,
    pub(super) boosted: ScoreEnvelope,
}
impl Gain {
    fn upper(self, power: ScoringPower) -> Result<u64, ScoringError> {
        Ok(self
            .boosted
            .bounds(power)?
            .max
            .saturating_sub(self.normal.bounds(power)?.min))
    }
}
pub(super) struct ChartGeometry {
    pub(super) geometry: Geometry,
    pub(super) ideal: Vec<NoteFactors>,
    pub(super) baseline: ScoreEnvelope,
}

type SkillKey = [u32; 6];
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ProfileKey {
    domain: u8,
    slot: u8,
    first: u32,
    last: u32,
    skill: SkillKey,
}

fn skill_key(skill: &ReplaySkill) -> SkillKey {
    skill.cache_key()
}

/// Fixed chart, six requested skill slots and one finite upper power limit.
/// Profiles are scoped to one calculation; this is not an unbounded global cache.
pub struct CoarseTimingUpperCache {
    input: ReplayInput,
    max_power: ScoringPower,
    triggers: [usize; 6],
    geometry: HashMap<u8, ChartGeometry>,
    profiles: HashMap<ProfileKey, Gain>,
    /// All effect/correction bits are keyed; invalid descriptors are never cached.
    playing: HashMap<SkillKey, u32>,
    cooling: u32,
    stats: CoarseTimingUpperStats,
}

/// Prepared for the five PHYSICAL positions (center index 2 is the captain).
/// All fields are private: no artificial baseline lower bound is exposed.
pub struct CoarseTimingTeam {
    baseline: ScoreEnvelope,
    first_five: [[Gain; 5]; 5],
    encore: Gain,
}

/// Covers all physical layouts and all captains of the same unordered roster.
/// Query at a certified upper power covering those layouts, not display power.
pub struct CoarseTimingRoster {
    baseline: ScoreEnvelope,
    first_five: [[Gain; 5]; 5],
    encores: [Option<Gain>; 5],
}

impl CoarseTimingUpperCache {
    pub fn new(input: &ReplayInput, max_power: ScoringPower) -> Result<Self, ReplayError> {
        replay(input)?;
        let mut triggers = [None; 6];
        if input.skills.len() != 6 {
            return Err(ReplayError::Input(
                "coarse native-order bound needs six skill slots",
            ));
        }
        for (i, note) in input.notes.iter().enumerate() {
            if let Some(slot) = note.trigger {
                triggers[slot] = Some(i);
            }
        }
        if triggers.iter().any(Option::is_none) {
            return Err(ReplayError::Input(
                "coarse native-order bound needs all six requests",
            ));
        }
        Ok(Self {
            input: input.clone(),
            max_power,
            triggers: triggers.map(Option::unwrap),
            geometry: HashMap::new(),
            profiles: HashMap::new(),
            playing: HashMap::new(),
            cooling: countdown(0.75) + 1,
            stats: CoarseTimingUpperStats::default(),
        })
    }

    pub fn stats(&self) -> &CoarseTimingUpperStats {
        &self.stats
    }

    /// Team conditions/levels must already be resolved. This prepares an upper
    /// bound only; it does not certify a baseline or a playable timing strategy.
    pub fn prepare_team(
        &mut self,
        members: &[ReplaySkill; 5],
    ) -> Result<CoarseTimingTeam, ReplayError> {
        let roster = self.prepare_inner(members, false)?;
        Ok(CoarseTimingTeam {
            baseline: roster.baseline,
            first_five: roster.first_five,
            encore: roster.encores[2].unwrap(),
        })
    }

    pub fn prepare_roster(
        &mut self,
        members: &[ReplaySkill; 5],
    ) -> Result<CoarseTimingRoster, ReplayError> {
        self.prepare_inner(members, true)
    }

    fn prepare_inner(
        &mut self,
        members: &[ReplaySkill; 5],
        all_captains: bool,
    ) -> Result<CoarseTimingRoster, ReplayError> {
        let keys = members.each_ref().map(skill_key);
        let mut playing = [0; 5];
        for (i, skill) in members.iter().enumerate() {
            if let Some(&frames) = self.playing.get(&keys[i]) {
                playing[i] = frames;
            } else {
                skill.validate(i)?;
                playing[i] = countdown(skill.duration) + 1;
                self.playing.insert(keys[i], playing[i]);
            }
        }
        let domain = members
            .iter()
            .map(|s| correction_key(s.correction).saturating_sub(1))
            .max()
            .unwrap();
        if !self.geometry.contains_key(&domain) {
            let mut input = self.input.clone();
            input.skills = members
                .iter()
                .cloned()
                .chain(std::iter::once(members[2].clone()))
                .collect();
            let geometry = Geometry::new(&input)?;
            let ideal = geometry.ideal_factors();
            let baseline =
                CompiledScore::new(input.play_level, &ideal)?.envelope(self.max_power)?;
            self.geometry.insert(
                domain,
                ChartGeometry {
                    geometry,
                    ideal,
                    baseline,
                },
            );
            self.stats.geometries += 1;
        }
        let chart = &self.geometry[&domain];
        let shortest = *playing.iter().min().unwrap();
        let longest = *playing.iter().max().unwrap();
        let cooling = self.cooling;
        let mut previous: Option<(u32, u32)> = None;
        let mut rows = [[None; 5]; 5];
        let mut encores = [None; 5];
        for slot in 0..6 {
            let (mut first, mut last) = chart.geometry.spans[self.triggers[slot]];
            if let Some((a, b)) = previous {
                first = first.max(a + 1);
                last = last.max(b + 1);
            }
            // Every possible preceding duration lies between these limits.
            previous = Some((first + shortest + cooling, last + longest + cooling));
            for member in 0..5 {
                if slot == 5 && !all_captains && member != 2 {
                    continue;
                }
                let window = (first + 1, last + playing[member]);
                let skill = &members[member];
                let key = ProfileKey {
                    domain,
                    slot: slot as u8,
                    first: window.0,
                    last: window.1,
                    skill: keys[member],
                };
                let gain = if let Some(&gain) = self.profiles.get(&key) {
                    self.stats.profile_hits += 1;
                    gain
                } else {
                    let gain = build_gain(&self.input, chart, slot, skill, window, self.max_power)?;
                    self.profiles.insert(key, gain);
                    self.stats.profile_builds += 1;
                    gain
                };
                if slot < 5 {
                    rows[slot][member] = Some(gain);
                } else {
                    encores[member] = Some(gain);
                }
            }
        }
        Ok(CoarseTimingRoster {
            baseline: chart.baseline,
            first_five: rows.map(|row| row.map(Option::unwrap)),
            encores,
        })
    }
}

impl CoarseTimingTeam {
    /// No chart scan, replay, timing search or allocation. A five-member subset
    /// DP covers all native orders: physical member 4 cannot activate fifth.
    /// Every other first-five permutation is reachable, followed by member 2.
    pub fn upper(&self, power: ScoringPower) -> Result<u64, ScoringError> {
        add_bound(
            self.baseline,
            assignment(&self.first_five, power, Some(4))?,
            self.encore.upper(power)?,
            power,
        )
    }
}

impl CoarseTimingRoster {
    pub fn upper(&self, power: ScoringPower) -> Result<u64, ScoringError> {
        let mut encore = 0;
        for profile in self.encores.iter().flatten() {
            encore = encore.max(profile.upper(power)?);
        }
        add_bound(
            self.baseline,
            assignment(&self.first_five, power, None)?,
            encore,
            power,
        )
    }
}

fn add_bound(
    baseline: ScoreEnvelope,
    first: u64,
    encore: u64,
    power: ScoringPower,
) -> Result<u64, ScoringError> {
    baseline
        .bounds(power)?
        .max
        .checked_add(first)
        .and_then(|v| v.checked_add(encore))
        .ok_or(ScoringError::TotalScoreOverflow)
}

fn assignment(
    profiles: &[[Gain; 5]; 5],
    power: ScoringPower,
    forbidden_last: Option<usize>,
) -> Result<u64, ScoringError> {
    let mut gains = [[0; 5]; 5];
    for (row, profiles) in gains.iter_mut().zip(profiles) {
        for (value, profile) in row.iter_mut().zip(profiles) {
            *value = profile.upper(power)?;
        }
    }
    let mut dp = [None; 32];
    dp[0] = Some(0u64);
    for mask in 0usize..31 {
        let Some(previous) = dp[mask] else { continue };
        let slot = mask.count_ones() as usize;
        for member in 0..5 {
            if mask & (1 << member) != 0 || (slot == 4 && Some(member) == forbidden_last) {
                continue;
            }
            let next = mask | (1 << member);
            let value = previous
                .checked_add(gains[slot][member])
                .ok_or(ScoringError::TotalScoreOverflow)?;
            dp[next] = Some(dp[next].map_or(value, |old| old.max(value)));
        }
    }
    Ok(dp[31].unwrap())
}

pub(super) fn build_gain(
    input: &ReplayInput,
    chart: &ChartGeometry,
    slot: usize,
    skill: &ReplaySkill,
    (first, last): (u32, u32),
    max_power: ScoringPower,
) -> Result<Gain, ReplayError> {
    let mut state = SkillState::new(skill)?;
    let mut peak = vec![state.multiplier(Judgement::Great).max(1.0)];
    if matches!(skill.score, ScoreEffect::Crescendo { .. }) {
        // Preserve all f32 increments and the untouched state (equal base/cap).
        for _ in &input.notes {
            peak.push(
                peak.last()
                    .copied()
                    .unwrap()
                    .max(state.multiplier(Judgement::Perfect)),
            );
        }
    } else {
        peak[0] = peak[0].max(SkillState::new(skill)?.multiplier(Judgement::Perfect));
    }
    let mut possible: Vec<_> = chart
        .geometry
        .spans
        .iter()
        .filter(|&&(_, b)| b >= first)
        .map(|&(a, _)| a)
        .collect();
    possible.sort_unstable();
    // Zero factors retain the FULL chart note count while restricting each
    // envelope's rounding loss to the affected subset of notes.
    let zero = NoteFactors {
        judgement: 0.,
        combo: 1.,
        skill: 1.,
        fever: 1.,
    };
    let mut normal = vec![zero; input.notes.len()];
    let mut boosted = normal.clone();
    for (i, &(a, b)) in chart.geometry.spans.iter().enumerate() {
        if b < first || a > last || input.notes[i].trigger.is_some_and(|s| slot >= s) {
            continue;
        }
        let multiplier = if peak.len() == 1 {
            peak[0]
        } else {
            peak[possible.partition_point(|&f| f <= b)]
        };
        normal[i] = chart.ideal[i];
        boosted[i] = NoteFactors {
            skill: multiplier,
            ..normal[i]
        };
    }
    Ok(Gain {
        normal: CompiledScore::new(input.play_level, &normal)?.envelope(max_power)?,
        boosted: CompiledScore::new(input.play_level, &boosted)?.envelope(max_power)?,
    })
}

#[cfg(test)]
mod tests;
