//! Default preparation reuses joint input geometry and reoptimizes native scores
//! at each queried power. Explicit legacy/limited options retain a finite plan
//! family, reported as incomplete; it must never be mistaken for an upper bound.
use super::independent::{JointTimeline, JointPreparation, JointPreparationStats};
use super::{
    replay,
    search::{self, SearchError, SearchOptions, SearchResult},
    ReplayError, ReplayInput, ReplayResult, REPLAY_FPS,
};
use crate::scoring::{NoteScorer, ScoringError, ScoringPower};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

mod orders;
pub use orders::{CompiledTimingOrders, TimingOrderScratch, TimingOrderStats};
mod pool;
pub use pool::{TimingCompileCachePool, TimingGeometryPoolStats};

// Already-rounded f32 operands: judgment, Combo, (Fever * skill).
pub(super) type Term = [u32; 3];

#[derive(Clone)]
struct RawVariant {
    terms: Vec<Term>,
    frames: Vec<u32>,
    order: Vec<usize>,
}
#[derive(Clone)]
struct RawGroup {
    indices: Vec<usize>,
    variants: Vec<RawVariant>,
    last_frames: Vec<u32>,
    last_order: Vec<usize>,
}
pub(super) struct Recorder {
    groups: Vec<RawGroup>,
    captured: u64,
    dominated: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileStats {
    pub complete: bool,
    pub evaluated_plans_captured: u64,
    pub dominated_or_equal_variants: u64,
    pub independent_groups: usize,
    pub variants: usize,
    pub unique_score_terms: usize,
    pub term_additions_per_score: usize,
}

struct Variant {
    terms: Vec<(usize, u32)>,
    frames: Vec<u32>,
    order: Vec<usize>,
}
struct Group {
    indices: Vec<usize>,
    variants: Vec<Variant>,
}

/// Immutable, reusable with arbitrary valid floating-point powers. The chart,
/// all resolved skills, their trigger order, and Combo mode are fixed at compile.
pub struct CompiledTimingCandidates {
    exact: Option<Arc<JointTimeline>>,
    restricted: bool,
    input: ReplayInput,
    groups: Vec<Group>,
    terms: Vec<Term>,
    common: Vec<(usize, u32)>,
    stats: CompileStats,
}

/// Reuse one exactly equivalent complete score, plus legacy arithmetic scratch.
/// Keeping only one model bounds retention independently of the roster count.
pub struct TimingScoreScratch {
    terms: Vec<u32>,
    choices: Vec<usize>,
    reuse_equivalent_scores: bool,
    exact_score: Option<ExactScoreMemo>,
    exact_stats: ExactScoreReuseStats,
}

struct ExactScoreMemo {
    model: Arc<JointTimeline>,
    signature: Vec<u32>,
    score: u64,
}

#[derive(Clone, Copy, Default, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactScoreReuseStats {
    pub dp_queries: u64,
    pub equivalent_score_hits: u64,
}

impl Default for TimingScoreScratch {
    fn default() -> Self {
        Self::with_equivalent_score_reuse(
            std::env::var("BANDORI_TIMING_DISABLE_SCORE_EQUIVALENCE_CACHE").as_deref() != Ok("1"),
        )
    }
}

impl TimingScoreScratch {
    /// Explicit policy for application callers; it does not change exact DP.
    pub fn with_equivalent_score_reuse(reuse_equivalent_scores: bool) -> Self {
        Self {
            terms: Vec::new(),
            choices: Vec::new(),
            reuse_equivalent_scores,
            exact_score: None,
            exact_stats: ExactScoreReuseStats::default(),
        }
    }

    pub fn exact_score_stats(&self) -> ExactScoreReuseStats {
        self.exact_stats
    }
}

pub struct CompiledTimingResult {
    pub reference_search: SearchResult,
    pub candidates: CompiledTimingCandidates,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingCacheStats {
    pub group_builds: u64,
    pub group_hits: u64,
    pub search_evaluations: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingCacheFootprint {
    pub groups: usize,
    pub variants: usize,
    /// Owned vector capacity plus inline group values/keys. Excludes HashMap
    /// buckets, allocator bookkeeping, temporary banks, and the input snapshot.
    pub payload_bytes: usize,
    pub geometry_owners: usize,
    pub geometry_owner_payload_bytes: usize,
}

/// One cache per chart, baseline geometry, Combo/Fever policy and reference power.
/// Keys contain full frame domains and resolved skill definitions for each
/// interacting region, NOT card IDs or rounded powers. Unrelated skills share
/// existing regions; delayed windows stay in the same region.
pub struct TimingCompileCache {
    input: ReplayInput,
    limits: Option<Vec<Option<[u32; 2]>>>,
    options: SearchOptions,
    groups: HashMap<Vec<u64>, RawGroup>,
    exact: HashMap<(bool, Vec<[u32; 6]>), Arc<JointTimeline>>,
    exact_stamps: HashMap<(bool, Vec<[u32; 6]>), u64>,
    exact_clock: u64,
    score_durations: Vec<f32>,
    max_geometry_bytes: usize,
    // Geometry ownership is independent of the 64 resolved-skill bindings.
    // These owners contain no value DP or power-specific score result.
    geometry_owners: HashMap<Vec<(u8, u32, u32)>, (Arc<JointTimeline>, u64)>,
    max_owner_bytes: usize,
    preparation: JointPreparation,
    stats: TimingCacheStats,
}

impl TimingCompileCache {
    pub fn new(input: &ReplayInput, options: SearchOptions) -> Self {
        Self {
            input: input.clone(),
            limits: None,
            options,
            groups: HashMap::new(),
            exact: HashMap::new(),
            exact_stamps: HashMap::new(),
            exact_clock: 0,
            score_durations: Vec::new(),
            geometry_owners: HashMap::new(),
            // The complete benchmark showed no useful preparation improvement
            // and higher retained memory; keep independent owners opt-in.
            max_owner_bytes: if std::env::var("BANDORI_TIMING_DISABLE_GEOMETRY_OWNERS")
                .as_deref() != Ok("0") || std::env::var("BANDORI_TIMING_DISABLE_JOINT_LOCAL_GEOMETRY")
                .as_deref() == Ok("1") { 0 } else { 32 * 1024 * 1024 },
            preparation: JointPreparation::default(),
            // Local masks can create more distinct graphs than correction
            // alone. Bound retained graph payload, never exploration. Callers
            // holding returned banks own their separate lifetimes.
            max_geometry_bytes: if std::env::var("BANDORI_TIMING_DISABLE_JOINT_LOCAL_GEOMETRY")
                .as_deref() == Ok("1") { usize::MAX } else { 256 * 1024 * 1024 },
            stats: TimingCacheStats::default(),
        }
    }

    pub fn stats(&self) -> &TimingCacheStats {
        &self.stats
    }

    pub fn with_frame_ranges(mut self, limits: Vec<Option<[u32; 2]>>) -> Self {
        assert!(self.exact.is_empty() && self.groups.is_empty());
        assert!(self.geometry_owners.is_empty());
        self.preparation=JointPreparation::default();
        self.limits = limits.iter().any(Option::is_some).then_some(limits);
        self
    }

    pub fn preparation_stats(&self) -> JointPreparationStats { self.preparation.stats() }

    fn exact_geometry_bytes(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        self.exact.values().chain(self.geometry_owners.values().map(|(m, _)| m))
            .filter(|m| seen.insert(m.geometry_identity()))
            .map(|m| m.footprint()).sum()
    }

    fn owner_bytes(&self) -> usize {
        self.geometry_owners.values().map(|(m, _)| m.footprint()).sum()
    }
    fn evict_owner(&mut self) {
        let key = self.geometry_owners.iter().min_by_key(|(_, (_, stamp))| *stamp)
            .unwrap().0.clone();
        self.geometry_owners.remove(&key);
    }
    fn retain_geometry(&mut self, model: &Arc<JointTimeline>) {
        if self.max_owner_bytes == 0 || model.footprint() > self.max_owner_bytes { return; }
        self.geometry_owners.insert(model.geometry_key().to_vec(), (Arc::clone(model), self.exact_clock));
        while self.geometry_owners.len() > 16 || self.owner_bytes() > self.max_owner_bytes {
            self.evict_owner();
        }
    }
    fn enforce_geometry_budget(&mut self) {
        while self.exact_geometry_bytes() > self.max_geometry_bytes {
            if !self.geometry_owners.is_empty() { self.evict_owner(); }
            else if !self.exact.is_empty() { self.evict_exact(); }
            else { break; }
        }
    }

    fn evict_exact(&mut self) {
        let old = if self.max_geometry_bytes == usize::MAX {
            self.exact.keys().next().unwrap().clone()
        } else {
            self.exact_stamps.iter().min_by_key(|(_, stamp)| *stamp).unwrap().0.clone()
        };
        self.exact.remove(&old);
        self.exact_stamps.remove(&old);
    }

    pub fn footprint(&self) -> TimingCacheFootprint {
        let mut bytes = 0;
        let mut variants = 0;
        for (key, group) in &self.groups {
            bytes += std::mem::size_of_val(key) + key.capacity() * std::mem::size_of::<u64>();
            bytes += std::mem::size_of_val(group)
                + group.indices.capacity() * std::mem::size_of::<usize>()
                + group.last_frames.capacity() * std::mem::size_of::<u32>()
                + group.variants.capacity() * std::mem::size_of::<RawVariant>();
            variants += group.variants.len();
            for v in &group.variants {
                bytes += v.terms.capacity() * std::mem::size_of::<Term>()
                    + v.frames.capacity() * std::mem::size_of::<u32>();
            }
        }
        TimingCacheFootprint {
            groups: self.groups.len() + self.exact.len(),
            variants,
            payload_bytes: bytes + self.exact_geometry_bytes(),
            geometry_owners: self.geometry_owners.len(),
            geometry_owner_payload_bytes: self.owner_bytes(),
        }
    }

    pub fn prepare(
        &mut self,
        skills: &[super::ReplaySkill],
    ) -> Result<CompiledTimingCandidates, SearchError> {
        self.prepare_impl(skills, false)
    }

    /// Numeric search can keep more notes explicit to share geometry across
    /// skill durations. Use `prepare` for the canonical tied frame witness.
    pub fn prepare_for_score(
        &mut self,
        skills: &[super::ReplaySkill],
    ) -> Result<CompiledTimingCandidates, SearchError> {
        self.prepare_impl(skills, true)
    }

    fn share_playing_windows(&mut self, skills: &[super::ReplaySkill]) -> bool {
        if !self.preparation.enabled()
            || !self.preparation.stats.bad_interval_scan_enabled
            || self.preparation.stats.bad_interval_audit_enabled
        {
            return false;
        }
        // With at most two durations the six slots have at most 64 duration
        // patterns. The precise graphs are cheap to reuse in that case.
        if self.score_durations.len() < 3 {
            for skill in skills {
                if !self.score_durations.contains(&skill.duration) {
                    self.score_durations.push(skill.duration);
                    if self.score_durations.len() == 3 {
                        break;
                    }
                }
            }
        }
        if self.score_durations.len() < 3 {
            return false;
        }
        let (shortest, longest) = skills.iter().fold((f32::INFINITY, 0.0_f32), |(lo, hi), skill| {
            (lo.min(skill.duration), hi.max(skill.duration))
        });
        // Avoid expanding a short custom skill to a much longer interval.
        shortest < longest && longest <= 2.0 * shortest
    }

    fn prepare_impl(
        &mut self,
        skills: &[super::ReplaySkill],
        score_only: bool,
    ) -> Result<CompiledTimingCandidates, SearchError> {
        let mut input = self.input.clone();
        input.skills = skills.to_vec();
        if let Some(limits) = &self.limits {
            if !self.options.exhaustive() {
                return Err(SearchError::Options("frame ranges require exhaustive scoring"));
            }
            self.exact_clock += 1;
            let key = (false, skills.iter().map(super::ReplaySkill::cache_key).collect());
            let model = if let Some(model) = self.exact.get(&key) {
                self.stats.group_hits += 1;
                self.exact_stamps.insert(key.clone(), self.exact_clock);
                Some(Arc::clone(model))
            } else {
                let layout=self.preparation.geometry(&input, false)?;
                let existing=self.geometry_owners.get(&layout.key).map(|(m,_)|m)
                    .or_else(||self.exact.values().find(|m|m.geometry_key()==layout.key));
                let model=if let Some(existing)=existing {
                    self.preparation.stats.geometry_reuses+=1;
                    Some(existing.with_validated_skills_key(skills,layout.key)?)
                } else if self.preparation.enabled() {
                    self.preparation.build_in_frame_ranges(&input,layout,limits)?
                } else {JointTimeline::in_frame_ranges(&input,limits)?};
                let model=model.map(Arc::new);
                if let Some(model) = &model {
                    if self.exact.len() >= 64 { self.evict_exact(); }
                    self.exact.insert(key.clone(), Arc::clone(model));
                    self.exact_stamps.insert(key, self.exact_clock);
                    self.retain_geometry(model);
                    self.enforce_geometry_budget();
                }
                self.stats.group_builds += 1;
                model
            };
            return Ok(CompiledTimingCandidates::restricted(input, model));
        }
        if self.options.exhaustive() {
            self.exact_clock += 1;
            let share = score_only && self.share_playing_windows(skills);
            let key = (share, skills.iter().map(super::ReplaySkill::cache_key).collect());
            let model = if let Some(model) = self.exact.get(&key) {
                self.stats.group_hits += 1;
                self.exact_stamps.insert(key.clone(), self.exact_clock);
                Arc::clone(model)
            } else {
                let key_start = crate::timing::Instant::now();
                let layout = self.preparation.geometry(&input, share)?;
                self.preparation.stats.geometry_key_seconds += key_start.elapsed().as_secs_f64();
                let existing = self.geometry_owners.get(&layout.key).map(|(m, _)| m)
                    .or_else(|| self.exact.values().find(|m| m.geometry_key() == layout.key));
                let model = Arc::new(
                    if let Some(existing) = existing
                    {
                        if self.preparation.enabled() {
                            self.preparation.stats.geometry_reuses += 1;
                            let start = crate::timing::Instant::now();
                            let model = if self.preparation.stats.bad_interval_scan_enabled {
                                existing.with_validated_skills_key(skills, layout.key)?
                            } else { existing.with_skills_key(skills, layout.key)? };
                            self.preparation.stats.geometry_rebind_seconds += start.elapsed().as_secs_f64();
                            model
                        } else { existing.with_skills(skills)? }
                    } else if self.preparation.enabled() {
                        self.preparation.build(&input, layout)?
                    } else {
                        JointTimeline::new(&input)?
                    },
                );
                // Evicting cached models never truncates the search. Returned
                // banks own Arcs and remain valid after their cache entry leaves.
                if self.exact.len() >= 64 {
                    self.evict_exact();
                }
                self.exact.insert(key.clone(), Arc::clone(&model));
                self.exact_stamps.insert(key, self.exact_clock);
                self.retain_geometry(&model);
                self.enforce_geometry_budget();
                self.stats.group_builds += 1;
                model
            };
            if let Some((_, stamp)) = self.geometry_owners.get_mut(model.geometry_key()) {
                *stamp = self.exact_clock;
            }
            return Ok(CompiledTimingCandidates::exact(input, model));
        }
        // Structural validation before domain/partition indexing.
        let baseline = replay(&input)?;
        let domains = search::frame_domains(&input)?;
        let partitions = partitions(&input, &domains);
        let mut baseline_terms = vec![[0; 3]; input.notes.len()];
        for t in &baseline.trace {
            baseline_terms[t.note_index] = [
                t.judgement.multiplier().to_bits(),
                input.combo_multiplier(t.combo).to_bits(),
                (t.fever_multiplier * t.skill_multiplier).to_bits(),
            ];
        }
        let mut assembled = Recorder {
            groups: Vec::new(),
            captured: 0,
            dominated: 0,
        };
        for indices in partitions {
            let key = group_key(&input, &domains, &indices);
            if let Some(group) = self.groups.get(&key) {
                assembled.groups.push(group.clone());
                self.stats.group_hits += 1;
                continue;
            }
            // A region unreachable by any skill, with all-P baseline and one
            // fixed Fever multiplier, cannot improve by changing hit order or
            // taking Greats. Its exact constant factors need no timing search.
            let first_fever = fever(&input, input.hit_frames[indices[0]]);
            if indices.iter().all(|&i| {
                input.notes[i].trigger.is_none()
                    && baseline_terms[i][0] == 1.1_f32.to_bits()
                    && domains[i].iter().all(|&f| fever(&input, f) == first_fever)
            }) {
                let mut terms: Vec<_> = indices.iter().map(|&i| baseline_terms[i]).collect();
                terms.sort_unstable();
                let frames = indices.iter().map(|&i| input.hit_frames[i]).collect();
                let order = input_order(&input, &indices);
                let group = RawGroup {
                    indices,
                    variants: vec![RawVariant {
                        terms,
                        frames,
                        order,
                    }],
                    last_frames: vec![],
                    last_order: vec![],
                };
                self.stats.group_builds += 1;
                self.groups.insert(key, group.clone());
                assembled.groups.push(group);
                continue;
            }
            let (search, recorder) = search::optimize_component(&input, self.options, &indices)?;
            self.stats.group_builds += 1;
            self.stats.search_evaluations += search.evaluations;
            let recorder = recorder.unwrap();
            assembled.captured += recorder.captured;
            assembled.dominated += recorder.dominated;
            let group = recorder
                .groups
                .into_iter()
                .find(|g| g.indices == indices)
                .expect("same original domain partition");
            self.groups.insert(key, group.clone());
            assembled.groups.push(group);
        }
        assembled.record_perfect_witnesses(&input, &domains)?;
        let bank = assembled.finish(&input)?;
        let result = bank.materialize(ScoringPower::new(input.power)?)?;
        if result.total < baseline.total {
            return Err(SearchError::Mismatch {
                cached: result.total,
                replayed: baseline.total,
            });
        }
        Ok(bank)
    }
}

fn fever(input: &ReplayInput, frame: u32) -> bool {
    input
        .fever_frames
        .is_some_and(|[a, b]| a <= frame && frame < b)
}

fn group_key(input: &ReplayInput, domains: &[Vec<u32>], indices: &[usize]) -> Vec<u64> {
    use super::ScoreEffect;
    let mut key = vec![indices.len() as u64];
    for &i in indices {
        key.extend([i as u64, domains[i].len() as u64]);
        key.extend(domains[i].iter().map(|&f| u64::from(f)));
        if let Some(slot) = input.notes[i].trigger {
            let skill = &input.skills[slot];
            key.extend([
                slot as u64 + 1,
                u64::from(skill.duration.to_bits()),
                match skill.correction {
                    None => 0,
                    Some(super::JudgementCorrection::Great) => 1,
                    Some(super::JudgementCorrection::Good) => 2,
                    Some(super::JudgementCorrection::Bad) => 3,
                },
            ]);
            let bits = |n: f32| u64::from(n.to_bits());
            match skill.score {
                ScoreEffect::Plain { percent } => key.extend([0, bits(percent)]),
                ScoreEffect::PerfectOnly { percent } => key.extend([1, bits(percent)]),
                ScoreEffect::PerfectOrZero { percent } => key.extend([2, bits(percent)]),
                ScoreEffect::PerfectOrHalf { percent } => key.extend([3, bits(percent)]),
                ScoreEffect::Continuous { high, low } => key.extend([4, bits(high), bits(low)]),
                ScoreEffect::Crescendo { percent, step, cap } => {
                    key.extend([5, bits(percent), bits(step), u64::from(cap)])
                }
            }
        } else {
            key.push(0);
        }
    }
    key
}

pub fn compile(
    input: &ReplayInput,
    options: SearchOptions,
) -> Result<CompiledTimingResult, SearchError> {
    if options.exhaustive() {
        let reference_search = search::optimize(input, options)?;
        let candidates =
            CompiledTimingCandidates::exact(input.clone(), Arc::new(JointTimeline::new(input)?));
        return Ok(CompiledTimingResult {
            reference_search,
            candidates,
        });
    }
    let (reference_search, recorder) = search::optimize_recorded(input, options, true)?;
    let mut recorder = recorder.expect("recording enabled");
    recorder.record_perfect_witnesses(input, &search::frame_domains(input)?)?;
    let candidates = recorder.finish(input)?;
    let mut scratch = TimingScoreScratch::default();
    let best = candidates.score(ScoringPower::new(input.power)?, &mut scratch)?;
    if best < reference_search.result.total {
        return Err(SearchError::Mismatch {
            cached: best,
            replayed: reference_search.result.total,
        });
    }
    // Composition is audited independently before exposing the compiled bank.
    candidates.materialize(ScoringPower::new(input.power)?)?;
    Ok(CompiledTimingResult {
        reference_search,
        candidates,
    })
}

impl CompiledTimingCandidates {
    fn exact(input: ReplayInput, model: Arc<JointTimeline>) -> Self {
        Self {
            exact: Some(model),
            restricted: false,
            input,
            groups: vec![],
            terms: vec![],
            common: vec![],
            stats: CompileStats {
                complete: true,
                evaluated_plans_captured: 0,
                dominated_or_equal_variants: 0,
                independent_groups: 1,
                variants: 0,
                unique_score_terms: 0,
                term_additions_per_score: 0,
            },
        }
    }
    pub fn stats(&self) -> &CompileStats {
        &self.stats
    }

    fn restricted(input: ReplayInput, model: Option<Arc<JointTimeline>>) -> Self {
        Self { exact: model, restricted: true, input, groups: vec![], terms: vec![], common: vec![],
            stats: CompileStats { complete: true, evaluated_plans_captured: 0,
                dominated_or_equal_variants: 0, independent_groups: 1, variants: 0,
                unique_score_terms: 0, term_additions_per_score: 0 } }
    }

    pub fn score_optional(&self, power: ScoringPower, scratch: &mut TimingScoreScratch)
        -> Result<Option<u64>, SearchError> {
        if !self.restricted { return self.score(power, scratch).map(Some); }
        scratch.exact_stats.dp_queries += 1;
        match &self.exact { Some(model) => model.score_optional(power), None => Ok(None) }
    }

    /// Fresh work diagnostics for the exact joint model. Legacy candidate banks
    /// have no joint value DP and return None. This never reads a score memo.
    pub fn native_score_with_stats(&self,power:ScoringPower)
        -> Result<Option<(u64, super::independent::WindowStats)>,SearchError> {
        self.exact.as_ref().map(|model|model.score_with_stats(power)).transpose()
    }

    /// With stats.complete, query the entire joint graph at this actual power.
    /// Explicit legacy banks maximize only their stored feasible plan family;
    /// those incomplete values are lower bounds, not pruning upper bounds.
    pub fn score(
        &self,
        power: ScoringPower,
        scratch: &mut TimingScoreScratch,
    ) -> Result<u64, SearchError> {
        if self.restricted {
            return self.score_optional(power, scratch)?.ok_or_else(||
                SearchError::Options("no feasible hit plan within frame ranges"));
        }
        if let Some(model) = &self.exact {
            let signature = self.reuse_signature(model, power, scratch);
            if let (Some(signature), Some(previous)) = (&signature, &scratch.exact_score) {
                if Arc::ptr_eq(&previous.model, model) && previous.signature == *signature {
                    scratch.exact_stats.equivalent_score_hits += 1;
                    return Ok(previous.score);
                }
            }
            scratch.exact_stats.dp_queries += 1;
            let score = model.score(power)?;
            scratch.exact_score = signature.map(|signature| ExactScoreMemo {
                model: Arc::clone(model),
                signature,
                score,
            });
            return Ok(score);
        }
        let scorer = NoteScorer::new(power, self.input.play_level, self.input.notes.len() as u32)?;
        scratch.terms.resize(self.terms.len(), 0);
        scratch.choices.resize(self.groups.len(), 0);
        for (term, value) in self.terms.iter().zip(&mut scratch.terms) {
            *value = scorer
                .score_with_judgement_multiplier(
                    f32::from_bits(term[0]),
                    f32::from_bits(term[1]),
                    f32::from_bits(term[2]),
                    1.0,
                )?
                .final_score;
        }
        let mut total: u64 = self
            .common
            .iter()
            .map(|&(t, n)| u64::from(scratch.terms[t]) * u64::from(n))
            .sum();
        for (index, group) in self.groups.iter().enumerate() {
            if group.variants.len() == 1 {
                scratch.choices[index] = 0;
                continue;
            }
            let mut best = 0;
            let mut chosen = 0;
            for (v, variant) in group.variants.iter().enumerate() {
                let value: u64 = variant
                    .terms
                    .iter()
                    .map(|&(t, count)| u64::from(scratch.terms[t]) * u64::from(count))
                    .sum();
                if value > best {
                    best = value;
                    chosen = v;
                }
            }
            scratch.choices[index] = chosen;
            total = total
                .checked_add(best)
                .ok_or(ScoringError::TotalScoreOverflow)?;
        }
        Ok(total)
    }

    fn reuse_signature(
        &self,
        model: &JointTimeline,
        power: ScoringPower,
        scratch: &TimingScoreScratch,
    ) -> Option<Vec<u32>> {
        scratch.reuse_equivalent_scores.then(|| model.score_signature(power)).flatten()
    }

    /// Reconstruct the chosen inputs only for finalists / display, then verify
    /// them with the original frame replay. Not needed in the candidate loop.
    pub fn materialize(&self, power: ScoringPower) -> Result<ReplayResult, SearchError> {
        if self.restricted {
            return self.exact.as_ref().ok_or(SearchError::Options("infeasible frame ranges"))?
                .maximum_optional(power)?.map(|w| w.result)
                .ok_or(SearchError::Options("infeasible frame ranges"));
        }
        if let Some(model) = &self.exact {
            return Ok(model.maximum(power)?.result);
        }
        let mut scratch = TimingScoreScratch::default();
        let expected = self.score(power, &mut scratch)?;
        let mut request = self.input.clone();
        request.power = power.value();
        request.hit_order.clear();
        for (group, &choice) in self.groups.iter().zip(&scratch.choices) {
            request
                .hit_order
                .extend_from_slice(&group.variants[choice].order);
            for (&i, &frame) in group.indices.iter().zip(&group.variants[choice].frames) {
                request.hit_frames[i] = frame;
            }
        }
        let result = replay(&request)?;
        if result.total != expected {
            return Err(SearchError::Mismatch {
                cached: expected,
                replayed: result.total,
            });
        }
        Ok(result)
    }
}

impl Recorder {
    pub fn new(input: &ReplayInput, choices: &[Vec<u32>]) -> Self {
        let indices = partitions(input, choices);
        Self {
            groups: indices
                .into_iter()
                .map(|indices| RawGroup {
                    indices,
                    variants: Vec::new(),
                    last_frames: Vec::new(),
                    last_order: Vec::new(),
                })
                .collect(),
            captured: 0,
            dominated: 0,
        }
    }

    pub fn record(&mut self, input: &ReplayInput, frames: &[u32], terms: &[Term]) {
        // Search can score an intermediate explicit-constraint violation while
        // rearranging a whole group. Such a plan cannot enter the reusable bank.
        let ranks = input
            .constraints
            .iter()
            .any(|c| c.require_dispatch_order)
            .then(|| super::routing::event_ranks(input));
        if input.constraints.iter().any(|c| {
            !c.allows_frames(frames[c.before], frames[c.after])
                || (c.require_dispatch_order
                    && ranks.as_ref().is_some_and(|r| {
                        (frames[c.before], r[c.before]) >= (frames[c.after], r[c.after])
                    }))
        }) {
            return;
        }
        self.captured += 1;
        for group in &mut self.groups {
            let order = input_order(input, &group.indices);
            if order == group.last_order
                && group.indices.len() == group.last_frames.len()
                && group
                    .indices
                    .iter()
                    .zip(&group.last_frames)
                    .all(|(&i, &f)| frames[i] == f)
            {
                continue;
            }
            group.last_frames = group.indices.iter().map(|&i| frames[i]).collect();
            group.last_order = order;
            let mut candidate: Vec<_> = group.indices.iter().map(|&i| terms[i]).collect();
            candidate.sort_unstable();
            if group
                .variants
                .iter()
                .any(|v| dominates(&v.terms, &candidate))
            {
                self.dominated += 1;
                continue;
            }
            group.variants.retain(|v| {
                let remove = dominates(&candidate, &v.terms);
                self.dominated += u64::from(remove);
                !remove
            });
            group.variants.push(RawVariant {
                terms: candidate,
                frames: group.last_frames.clone(),
                order: group.last_order.clone(),
            });
        }
    }

    fn record_perfect_witnesses(
        &mut self,
        input: &ReplayInput,
        domains: &[Vec<u32>],
    ) -> Result<(), ReplayError> {
        // Replacement certificates can choose any of these whole plans. Make
        // them available to final scoring too, including cached preparation.
        for witness in super::witness::perfect(input, domains) {
            let mut actual = input.clone();
            actual.hit_frames = witness.hit_frames;
            let result = replay(&actual)?;
            let mut terms = vec![[0; 3]; input.notes.len()];
            for t in result.trace {
                terms[t.note_index] = [
                    t.judgement.multiplier().to_bits(),
                    input.combo_multiplier(t.combo).to_bits(),
                    (t.fever_multiplier * t.skill_multiplier).to_bits(),
                ];
            }
            self.record(input, &actual.hit_frames, &terms);
        }
        Ok(())
    }

    fn finish(self, input: &ReplayInput) -> Result<CompiledTimingCandidates, ReplayError> {
        let mut terms = Vec::new();
        let mut ids = HashMap::new();
        let mut groups = Vec::new();
        for raw in self.groups {
            if raw.variants.is_empty() {
                return Err(ReplayError::Input(
                    "compiled timing group has no legal plan",
                ));
            }
            let variants = raw
                .variants
                .into_iter()
                .map(|v| {
                    let mut counts = BTreeMap::new();
                    for term in v.terms {
                        let next = terms.len();
                        let id = *ids.entry(term).or_insert_with(|| {
                            terms.push(term);
                            next
                        });
                        *counts.entry(id).or_insert(0u32) += 1;
                    }
                    Variant {
                        terms: counts.into_iter().collect(),
                        frames: v.frames,
                        order: v.order,
                    }
                })
                .collect();
            groups.push(Group {
                indices: raw.indices,
                variants,
            });
        }
        // Hoist factors shared by all variants. They are scored and summed once
        // even when hundreds of regions/alternatives contain the same notes.
        let mut common = BTreeMap::<usize, u32>::new();
        for group in &mut groups {
            let mut intersection: BTreeMap<_, _> =
                group.variants[0].terms.iter().copied().collect();
            for v in &group.variants[1..] {
                let counts: BTreeMap<_, _> = v.terms.iter().copied().collect();
                for (t, n) in &mut intersection {
                    *n = (*n).min(*counts.get(t).unwrap_or(&0));
                }
            }
            intersection.retain(|_, n| *n > 0);
            for (&t, &n) in &intersection {
                *common.entry(t).or_default() += n;
            }
            for v in &mut group.variants {
                for (t, n) in &mut v.terms {
                    *n -= intersection.get(t).copied().unwrap_or(0);
                }
                v.terms.retain(|(_, n)| *n > 0);
            }
        }
        let stats = CompileStats {
            complete: false,
            evaluated_plans_captured: self.captured,
            dominated_or_equal_variants: self.dominated,
            independent_groups: groups.len(),
            variants: groups.iter().map(|g| g.variants.len()).sum(),
            unique_score_terms: terms.len(),
            term_additions_per_score: common.len()
                + groups
                    .iter()
                    .flat_map(|g| &g.variants)
                    .map(|v| v.terms.len())
                    .sum::<usize>(),
        };
        Ok(CompiledTimingCandidates {
            exact: None,
            restricted: false,
            input: input.clone(),
            groups,
            terms,
            common: common.into_iter().collect(),
            stats,
        })
    }
}

/// A sufficient certificate, not a comparison at the compilation power.
/// Every paired f32 operand is >=; native multiplication/casts are monotone.
/// Positive finite operand bits have the same order as their values.
fn dominates(a: &[Term], b: &[Term]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (0..3).all(|i| a[i] >= b[i]))
}

struct Union(Vec<usize>);
impl Union {
    fn root(&mut self, i: usize) -> usize {
        if self.0[i] != i {
            self.0[i] = self.root(self.0[i]);
        }
        self.0[i]
    }
    fn join(&mut self, a: usize, b: usize) {
        let a = self.root(a);
        let b = self.root(b);
        self.0[b] = a;
    }
}

/// No input order, skill state, or request delay may cross a partition.
/// Arbitrary supplied native coordinates use one whole-plan frontier until a
/// spatial independence certificate is available. Chart-time routing can be
/// partitioned by overlapping candidate lifetimes on the same lane.
fn partitions(input: &ReplayInput, choices: &[Vec<u32>]) -> Vec<Vec<usize>> {
    let n = choices.len();
    if input.native_time_routing.as_ref().is_some_and(|r| {
        r.distance_mode == super::CandidateDistanceMode::NativeMusicPos
    }) {
        return vec![(0..n).collect()];
    }
    let spans: Vec<_> = choices
        .iter()
        .map(|v| (*v.iter().min().unwrap(), *v.iter().max().unwrap()))
        .collect();
    let mut union = Union((0..n).collect());
    for i in 0..n {
        for j in 0..i {
            if spans[i].0 <= spans[j].1 && spans[j].0 <= spans[i].1 {
                union.join(i, j);
            }
        }
    }
    if let Some(r) = &input.native_time_routing {
        for i in 0..n {
            let a = r.targets[i].as_ref().unwrap();
            for j in 0..i {
                let b = r.targets[j].as_ref().unwrap();
                if a.button == b.button
                    && a.active_from_frame <= b.active_until_frame
                    && b.active_from_frame <= a.active_until_frame
                {
                    union.join(i, j);
                }
            }
        }
    }
    for c in &input.constraints {
        union.join(c.before, c.after);
    }
    let mut triggers: Vec<_> = input
        .notes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| n.trigger.map(|s| (s, i)))
        .collect();
    triggers.sort_unstable();
    let mut previous: Option<(usize, u32, u32)> = None;
    for (slot, i) in triggers {
        let (mut earliest, mut latest) = spans[i];
        if let Some((previous_index, ready_min, ready_max)) = previous {
            if earliest <= ready_max {
                union.join(i, previous_index);
            }
            earliest = earliest.max(ready_min + 1);
            latest = latest.max(ready_max + 1);
        }
        let playing = countdown(input.skills[slot].duration) + 1;
        for (j, &(a, b)) in spans.iter().enumerate() {
            if a <= latest + playing && earliest < b {
                union.join(i, j);
            }
        }
        let cooling = countdown(0.75) + 1;
        previous = Some((i, earliest + playing + cooling, latest + playing + cooling));
    }
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for i in 0..n {
        groups.entry(union.root(i)).or_default().push(i);
    }
    groups.into_values().collect()
}

fn countdown(mut remaining: f32) -> u32 {
    let mut frames = 0;
    while remaining > 0.0 {
        remaining -= 1.0_f32 / REPLAY_FPS as f32;
        frames += 1;
    }
    frames
}

#[cfg(test)]
mod tests;

fn input_order(input: &ReplayInput, indices: &[usize]) -> Vec<usize> {
    let ranks = super::routing::event_ranks(input);
    let mut order = indices.to_vec();
    order.sort_unstable_by_key(|&i| ranks[i]);
    order
}
