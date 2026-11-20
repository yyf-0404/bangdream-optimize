//! Chart geometry and sparse per-window bounds, shared across skill orders.
//! Queue-dependent window limits and the global judgment domain are in the key.
use super::*;
use crate::hit_timing::{JudgementCorrection, ReplaySkill};
use std::sync::Arc;

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingUpperCacheStats {
    pub geometries: usize,
    pub profile_builds: u64,
    pub profile_hits: u64,
    /// Profile payload only; excludes hash buckets, geometry and temporary bounds.
    pub profile_payload_bytes: usize,
    pub baseline_full_replays: u64,
    pub baseline_certified_validations: u64,
    pub baseline_skill_envelopes: u64,
    pub sparse_preparations: u64,
    pub composed_note_visits: u64,
    pub full_note_visits: u64,
    pub disjoint_preparations: u64,
    pub composed_profiles: u64,
}

struct Frame {
    time: u32,
    raw: Judgement,
    combo: f32,
    fever: f32,
}
pub(super) struct Geometry {
    frames: Vec<Vec<Frame>>,
    pub(super) spans: Vec<(u32, u32)>,
    baseline: Vec<Vec<Term>>,
    baseline_counts: BTreeMap<Vec<Term>, u32>,
}
type Profile = Vec<(usize, Vec<Term>)>;

/// A linear change to alternative multiplicities for one window. It can be
/// added to other profiles only after proving their note supports disjoint.
struct Summary {
    profile: Profile,
    support: Vec<u64>,
    changes: Vec<(Vec<Term>, i64)>,
}
impl Summary {
    fn new(geometry: &Geometry, profile: Profile) -> Self {
        let mut support = vec![0; geometry.baseline.len().div_ceil(64)];
        let mut changes = BTreeMap::<Vec<Term>, i64>::new();
        for (i, additions) in &profile {
            support[i / 64] |= 1 << (i % 64);
            let baseline = &geometry.baseline[*i];
            let mut variants = baseline.clone();
            for &term in additions { add(&mut variants, term); }
            variants.sort_unstable();
            *changes.entry(baseline.clone()).or_default() -= 1;
            *changes.entry(variants).or_default() += 1;
        }
        Self { profile, support,
            changes: changes.into_iter().filter(|(_, n)| *n != 0).collect() }
    }
    fn payload_bytes(&self) -> usize {
        self.profile.capacity() * std::mem::size_of::<(usize, Vec<Term>)>()
            + self.profile.iter().map(|(_, v)| v.capacity() * std::mem::size_of::<Term>()).sum::<usize>()
            + self.support.capacity() * std::mem::size_of::<u64>()
            + self.changes.capacity() * std::mem::size_of::<(Vec<Term>, i64)>()
            + self.changes.iter().map(|(v, _)| v.capacity() * std::mem::size_of::<Term>()).sum::<usize>()
    }
}

fn disjoint(profiles: &[Arc<Summary>], words: usize) -> bool {
    let mut union = vec![0; words];
    profiles.iter().all(|p| p.support.iter().zip(&mut union).all(|(&mask, used)| {
        let overlap = mask & *used;
        *used |= mask;
        overlap == 0
    }))
}

/// Fixed chart, Combo/Fever mode, baseline and routing. Each prepare call may
/// change all skills, but never power-dependent scoring or geometry inputs.
/// This experimental cache has no eviction; keep it scoped to one calculation.
pub struct TimingUpperCache {
    input: ReplayInput,
    limits: Option<Vec<Option<[u32; 2]>>>,
    geometry: HashMap<u8, Geometry>,
    profiles: HashMap<Vec<u32>, Arc<Summary>>,
    triggers: HashMap<usize, usize>,
    baseline_validation: crate::hit_timing::baseline::BaselineValidation,
    stats: TimingUpperCacheStats,
}

impl TimingUpperCache {
    pub fn new(input: &ReplayInput) -> Self {
        Self {
            input: input.clone(),
            limits: None,
            geometry: HashMap::new(),
            profiles: HashMap::new(),
            triggers: input.notes.iter().enumerate().filter_map(|(i, n)| n.trigger.map(|s| (s, i))).collect(),
            baseline_validation: Default::default(),
            stats: TimingUpperCacheStats::default(),
        }
    }

    pub fn stats(&self) -> &TimingUpperCacheStats {
        &self.stats
    }

    /// One fixed range domain per owner, shared across skill sequences. This
    /// only narrows the numerical relaxation; it never supplies an exact score.
    pub fn with_frame_ranges(mut self, limits: Vec<Option<[u32; 2]>>) -> Self {
        assert!(self.geometry.is_empty() && self.profiles.is_empty());
        self.limits = Some(limits);
        self
    }

    pub fn prepare(&mut self, skills: &[ReplaySkill]) -> Result<TimingScoreUpper, ReplayError> {
        self.prepare_impl(skills,
            std::env::var("BANDORI_TIMING_DISABLE_DISJOINT_UPPER").as_deref() != Ok("1"))
    }

    pub(super) fn prepare_impl(&mut self, skills: &[ReplaySkill], summaries: bool) -> Result<TimingScoreUpper, ReplayError> {
        self.input.skills = skills.to_vec();
        // Keep the uncached constructor's validation contract, including actual
        // baseline judgments/bindings. A cache hit must not hide invalid inputs.
        self.baseline_validation.validate(&self.input)?;
        self.stats.baseline_full_replays = self.baseline_validation.stats.full_replays;
        self.stats.baseline_certified_validations = self.baseline_validation.stats.certified_validations;
        self.stats.baseline_skill_envelopes = self.baseline_validation.stats.skill_envelopes;
        let domain = skills
            .iter()
            .map(|s| correction_key(s.correction).saturating_sub(1))
            .max()
            .unwrap_or(0);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.geometry.entry(domain) {
            entry.insert(Geometry::in_frame_ranges(&self.input, self.limits.as_deref())?);
            self.stats.geometries += 1;
        }
        let geometry = &self.geometry[&domain];
        let sparse = std::env::var("BANDORI_TIMING_DISABLE_SPARSE_UPPER").as_deref() != Ok("1");
        let mut chosen = Vec::with_capacity(skills.len());
        let mut previous: Option<(u32, u32)> = None;
        for (slot, skill) in skills.iter().enumerate() {
            let Some(&trigger) = self.triggers.get(&slot) else {
                continue;
            };
            let (mut first, mut last) = geometry.spans[trigger];
            if let Some((a, b)) = previous {
                first = first.max(a + 1);
                last = last.max(b + 1);
            }
            let playing = countdown(skill.duration) + 1;
            let cooling = countdown(0.75) + 1;
            previous = Some((first + playing + cooling, last + playing + cooling));
            let window = (first + 1, last + playing);
            let mut key = vec![u32::from(domain), slot as u32, window.0, window.1];
            key.extend(skill_key(skill));
            let profile = if let Some(p) = self.profiles.get(&key) {
                self.stats.profile_hits += 1;
                p.clone()
            } else {
                let p = Arc::new(Summary::new(geometry,
                    geometry.profile(&self.input, slot, skill, window)?));
                self.stats.profile_builds += 1;
                self.stats.profile_payload_bytes += key.capacity() * std::mem::size_of::<u32>()
                    + p.payload_bytes();
                self.profiles.insert(key, p.clone());
                p
            };
            chosen.push(profile);
        }
        if sparse && summaries && disjoint(&chosen, geometry.baseline.len().div_ceil(64)) {
            let mut counts = geometry.baseline_counts.clone();
            for profile in &chosen {
                for (variants, delta) in &profile.changes {
                    if *delta < 0 { *counts.get_mut(variants).unwrap() -= (-*delta) as u32; }
                    else { *counts.entry(variants.clone()).or_default() += *delta as u32; }
                }
            }
            self.stats.sparse_preparations += 1;
            self.stats.disjoint_preparations += 1;
            self.stats.composed_profiles += chosen.len() as u64;
            return TimingScoreUpper::from_choice_counts(self.input.play_level,
                self.input.notes.len() as u32, counts);
        }
        let mut alternatives = if sparse { vec![] } else { geometry.baseline.clone() };
        let mut changed = HashMap::<usize, Vec<Term>>::new();
        for profile in chosen {
            for (i, variants) in &profile.profile {
                let target = if sparse {
                    changed.entry(*i).or_insert_with(|| geometry.baseline[*i].clone())
                } else { &mut alternatives[*i] };
                for &term in variants {
                    add(target, term);
                }
            }
        }
        if sparse {
            self.stats.sparse_preparations += 1;
            self.stats.composed_note_visits += changed.len() as u64;
            let mut counts = geometry.baseline_counts.clone();
            for (i, mut variants) in changed {
                *counts.get_mut(&geometry.baseline[i]).unwrap() -= 1;
                variants.sort_unstable();
                *counts.entry(variants).or_default() += 1;
            }
            return TimingScoreUpper::from_choice_counts(self.input.play_level,
                self.input.notes.len() as u32, counts);
        }
        self.stats.full_note_visits += alternatives.len() as u64;
        TimingScoreUpper::from_alternatives(self.input.play_level, alternatives)
    }
}

impl Geometry {
    pub(super) fn new(input: &ReplayInput) -> Result<Self, ReplayError> {
        Self::in_frame_ranges(input, None)
    }

    fn in_frame_ranges(input: &ReplayInput, limits: Option<&[Option<[u32; 2]>]>)
        -> Result<Self, ReplayError> {
        let mut domains = search::frame_domains(input)?;
        if let Some(limits) = limits {
            if limits.len() != domains.len() {
                return Err(ReplayError::Input("one upper-bound range is required per note"));
            }
            for (domain, limit) in domains.iter_mut().zip(limits) {
                if let Some([first, last]) = limit {
                    if first > last || *last > 72_000 {
                        return Err(ReplayError::Input("invalid upper-bound frame range"));
                    }
                    domain.retain(|f| first <= f && f <= last);
                }
                if domain.is_empty() {
                    return Err(ReplayError::Input("empty upper-bound frame domain"));
                }
            }
        }
        let spans: Vec<_> = domains
            .iter()
            .map(|d| (*d.iter().min().unwrap(), *d.iter().max().unwrap()))
            .collect();
        let mut rank_first: Vec<_> = spans.iter().map(|p| p.0).collect();
        rank_first.sort_unstable();
        let mut frames = Vec::new();
        let mut baseline = Vec::new();
        for (i, domain) in domains.iter().enumerate() {
            let mut row = Vec::new();
            let mut terms = Vec::new();
            for &time in domain {
                let raw = input.notes[i].raw_judgement(time)?;
                let rank = rank_first.partition_point(|&f| f <= time) as u32;
                let combo = input.combo_multiplier(
                    input
                        .initial_combo
                        .checked_add(rank)
                        .ok_or(ReplayError::Input("combo overflow"))?,
                );
                let fever: f32 = if input
                    .fever_frames
                    .is_some_and(|[a, b]| a <= time && time < b)
                {
                    2.0
                } else {
                    1.0
                };
                if raw >= Judgement::Great {
                    add(
                        &mut terms,
                        [raw.multiplier().to_bits(), combo.to_bits(), fever.to_bits()],
                    );
                }
                row.push(Frame {
                    time,
                    raw,
                    combo,
                    fever,
                });
            }
            frames.push(row);
            baseline.push(terms);
        }
        let mut baseline_counts = BTreeMap::new();
        for terms in &mut baseline {
            terms.sort_unstable();
            *baseline_counts.entry(terms.clone()).or_default() += 1;
        }
        Ok(Self {
            frames,
            spans,
            baseline,
            baseline_counts,
        })
    }

    /// Deliberately idealizes all grades and independently maximizes Combo and
    /// Fever. This is a numeric relaxation, not an executable baseline plan.
    pub(super) fn ideal_factors(&self) -> Vec<NoteFactors> {
        self.frames
            .iter()
            .map(|frames| NoteFactors {
                judgement: Judgement::Perfect.multiplier(),
                combo: frames.iter().map(|f| f.combo).fold(0.0, f32::max),
                fever: frames.iter().map(|f| f.fever).fold(0.0, f32::max),
                skill: 1.0,
            })
            .collect()
    }

    fn profile(
        &self,
        input: &ReplayInput,
        slot: usize,
        skill: &ReplaySkill,
        (first, last): (u32, u32),
    ) -> Result<Profile, ReplayError> {
        let state = SkillState::new(skill)?;
        let mut crescendo = Vec::new();
        let mut possible_first = Vec::new();
        if matches!(skill.score, ScoreEffect::Crescendo { .. }) {
            let mut s = state.clone();
            crescendo.push(s.multiplier(Judgement::Great));
            for _ in &input.notes {
                crescendo.push(
                    crescendo
                        .last()
                        .copied()
                        .unwrap()
                        .max(s.multiplier(Judgement::Perfect)),
                );
            }
            possible_first = self
                .spans
                .iter()
                .filter(|&&(_, b)| b >= first)
                .map(|&(a, _)| a)
                .collect();
            possible_first.sort_unstable();
        }
        let mut profile = Vec::new();
        for (i, frames) in self.frames.iter().enumerate() {
            if input.notes[i].trigger.is_some_and(|s| slot >= s)
                || self.spans[i].1 < first
                || self.spans[i].0 > last
            {
                continue;
            }
            let mut terms = Vec::new();
            for frame in frames.iter().filter(|f| first <= f.time && f.time <= last) {
                let grade = state.correct(frame.raw);
                if grade < Judgement::Great {
                    continue;
                }
                let multiplier = if crescendo.is_empty() {
                    state.clone().multiplier(grade)
                } else {
                    crescendo[possible_first.partition_point(|&f| f <= frame.time)]
                };
                add(
                    &mut terms,
                    [
                        grade.multiplier().to_bits(),
                        frame.combo.to_bits(),
                        (frame.fever * multiplier).to_bits(),
                    ],
                );
            }
            if !terms.is_empty() {
                profile.push((i, terms));
            }
        }
        Ok(profile)
    }
}

fn add(variants: &mut Vec<Term>, term: Term) {
    if !variants.iter().any(|v| dominates(*v, term)) {
        variants.retain(|&v| !dominates(term, v));
        variants.push(term);
    }
}
pub(super) fn correction_key(c: Option<JudgementCorrection>) -> u8 {
    match c {
        None => 0,
        Some(JudgementCorrection::Great) => 1,
        Some(JudgementCorrection::Good) => 2,
        Some(JudgementCorrection::Bad) => 3,
    }
}
pub(super) fn skill_key(s: &ReplaySkill) -> Vec<u32> {
    let mut key = vec![
        s.duration.to_bits(),
        u32::from(correction_key(s.correction)),
    ];
    let bits = f32::to_bits;
    match s.score {
        ScoreEffect::Plain { percent } => key.extend([0, bits(percent)]),
        ScoreEffect::PerfectOnly { percent } => key.extend([1, bits(percent)]),
        ScoreEffect::PerfectOrZero { percent } => key.extend([2, bits(percent)]),
        ScoreEffect::PerfectOrHalf { percent } => key.extend([3, bits(percent)]),
        ScoreEffect::Continuous { high, low } => key.extend([4, bits(high), bits(low)]),
        ScoreEffect::Crescendo { percent, step, cap } => {
            key.extend([5, bits(percent), bits(step), cap])
        }
    }
    key
}
