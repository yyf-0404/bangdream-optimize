//! Fixed-team / fixed-trigger-order search. Default scoring jointly optimizes
//! all trigger and ordinary hits in the declared replay model. Explicit legacy
//! settings or diagnostic limits retain the old local path, marked incomplete.
mod scorer;
mod windows;
use super::{
    replay, JudgementCorrection, NoteWindow, ReplayError, ReplayInput, ReplayResult, REPLAY_FPS,
};
use crate::scoring::Judgement;
use scorer::Scorer;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SEARCH_REVISION: &str = "joint-single-settlement-search-120-v7";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct SearchOptions {
    /// Zero searches until stable. Nonzero limits are for diagnostic comparisons.
    pub max_rounds: u64,
    /// Zero refines until stable. Nonzero limits are for diagnostic comparisons.
    pub max_refine_passes: u64,
    /// Zero has no evaluation limit. Default candidate preparation uses zero.
    pub max_evaluations: u64,
    /// False retains the legacy ordinary-note local search for explicit audits.
    pub complete_ordinary: bool,
    /// Complete joint trigger/ordinary DP. Disabling either complete flag or
    /// setting a diagnostic work limit explicitly selects the incomplete path.
    pub complete_search: bool,
}
impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_rounds: 0,
            max_refine_passes: 0,
            max_evaluations: 0,
            complete_ordinary: true,
            complete_search: true,
        }
    }
}

impl SearchOptions {
    pub(crate) fn exhaustive(self) -> bool {
        self.complete_search
            && self.complete_ordinary
            && self.max_rounds == 0
            && self.max_refine_passes == 0
            && self.max_evaluations == 0
    }
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error(transparent)]
    Replay(#[from] ReplayError),
    #[error("invalid timing search options: {0}")]
    Options(&'static str),
    #[error("timing search score {cached} differs from full replay {replayed}")]
    Mismatch { cached: u64, replayed: u64 },
}

impl From<crate::scoring::ScoringError> for SearchError {
    fn from(error: crate::scoring::ScoringError) -> Self {
        Self::Replay(ReplayError::Scoring(error))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchStop {
    Complete,
    Stable,
    RoundLimit,
    EvaluationLimit,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HitChange {
    pub note_index: usize,
    pub from_frame: u32,
    pub to_frame: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedMove {
    pub round: u64,
    /// None denotes rearranging the current windows without moving a trigger.
    pub anchor: Option<usize>,
    pub gain: u64,
    pub score: u64,
    pub changes: Vec<HitChange>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Completeness is scoped to the input's declared timing/routing domains.
    pub complete: bool,
    pub search_revision: String,
    pub baseline: ReplayResult,
    /// Legacy research JSON name: packing with the initial skill triggers fixed.
    pub coordinate_result: ReplayResult,
    pub result: ReplayResult,
    pub stop: SearchStop,
    pub rounds: u64,
    pub inner_pass_limit_count: u64,
    pub evaluations: u64,
    pub timeline_builds: u64,
    pub timeline_cache_hits: u64,
    pub delta_evaluations: u64,
    pub notes_scored: u64,
    pub boundary_note_visits: u64,
    pub candidate_frames: u64,
    pub representative_frames: u64,
    pub ordinary_dp: super::independent::WindowStats,
    pub accepted: Vec<AcceptedMove>,
}

/// `hit_frames` is the explicit baseline. No timing, node type or physical
/// playability assumption is inferred here. Unknown nodes must remain pinned.
pub fn optimize(input: &ReplayInput, options: SearchOptions) -> Result<SearchResult, SearchError> {
    Ok(optimize_recorded(input, options, false)?.0)
}

pub(super) fn optimize_recorded(
    input: &ReplayInput,
    options: SearchOptions,
    record_candidates: bool,
) -> Result<(SearchResult, Option<super::compiled::Recorder>), SearchError> {
    optimize_internal(input, options, record_candidates, None)
}

pub(super) fn frame_domains(input: &ReplayInput) -> Result<Vec<Vec<u32>>, ReplayError> {
    Ok(Search::new(input, SearchOptions::default())?.choices)
}

pub(super) fn optimize_component(
    input: &ReplayInput,
    options: SearchOptions,
    indices: &[usize],
) -> Result<(SearchResult, Option<super::compiled::Recorder>), SearchError> {
    optimize_internal(input, options, true, Some(indices))
}

fn optimize_internal(
    input: &ReplayInput,
    options: SearchOptions,
    record_candidates: bool,
    only_notes: Option<&[usize]>,
) -> Result<(SearchResult, Option<super::compiled::Recorder>), SearchError> {
    if options.exhaustive() && only_notes.is_none() {
        return optimize_joint(input, record_candidates);
    }
    let baseline = replay(input)?;
    let mut search = Search::new(input, options)?;
    if record_candidates {
        search.scorer.recorder = Some(super::compiled::Recorder::new(input, &search.choices));
    }
    if let Some(indices) = only_notes {
        let mut active = vec![false; input.notes.len()];
        for &i in indices {
            active[i] = true;
        }
        search.anchors.retain(|&i| active[i]);
        search.ordinary.retain(|&i| active[i]);
        for (i, choices) in search.choices.iter_mut().enumerate() {
            if !active[i] {
                *choices = vec![input.hit_frames[i]];
            }
        }
    }
    let (mut frames, mut current) = search
        .refine(input.hit_frames.clone())?
        .expect("validated baseline and available evaluation");
    let coordinate_result = search.audit(&frames, current)?;
    let mut accepted = Vec::new();
    let mut rounds = 0;
    let mut stable = false;
    loop {
        if search.exhausted || (options.max_rounds != 0 && rounds >= options.max_rounds) {
            break;
        }
        rounds += 1;
        let round = rounds;
        let before_round = current;
        for anchor in search.anchors.clone() {
            if search.exhausted {
                break;
            }
            let mut best_frames = frames.clone();
            let mut best_score = current;
            for frame in search.choices[anchor].clone() {
                if search.exhausted {
                    break;
                }
                let mut seeds = vec![frames.clone()];
                let mut reset = frames.clone();
                for &i in &search.ordinary {
                    reset[i] = input.hit_frames[i];
                }
                if !options.complete_ordinary && reset != frames {
                    seeds.push(reset);
                }
                for mut trial in seeds {
                    if search.exhausted {
                        break;
                    }
                    trial[anchor] = frame;
                    // Ordinary notes may be temporarily on the wrong side of
                    // this anchor. Validate the whole group after refinement.
                    if let Some((refined, score)) = search.refine(trial)? {
                        if score > best_score && search.constraints_hold(&refined) {
                            best_frames = refined;
                            best_score = score;
                        }
                    }
                }
            }
            if best_score > current {
                search.audit(&best_frames, best_score)?;
                accepted.push(record(
                    round,
                    Some(anchor),
                    &frames,
                    &best_frames,
                    current,
                    best_score,
                ));
                frames = best_frames;
                current = best_score;
            }
        }
        if !search.exhausted {
            if let Some((final_frames, score)) = search.refine(frames.clone())? {
                if score > current {
                    search.audit(&final_frames, score)?;
                    accepted.push(record(round, None, &frames, &final_frames, current, score));
                    frames = final_frames;
                    current = score;
                }
            }
        }
        if !search.exhausted && current == before_round {
            stable = true;
            break;
        }
    }
    let result = search.audit(&frames, current)?;
    let report = SearchResult {
        complete: false,
        search_revision: SEARCH_REVISION.to_owned(),
        baseline,
        coordinate_result,
        result,
        stop: if search.exhausted {
            SearchStop::EvaluationLimit
        } else if stable {
            SearchStop::Stable
        } else {
            SearchStop::RoundLimit
        },
        rounds,
        inner_pass_limit_count: search.inner_limits,
        evaluations: search.scorer.calls,
        timeline_builds: search.scorer.builds,
        timeline_cache_hits: search.scorer.hits,
        delta_evaluations: search.scorer.delta_calls,
        notes_scored: search.scorer.notes_scored,
        boundary_note_visits: search.boundary_note_visits,
        candidate_frames: search.candidate_frames,
        representative_frames: search.representative_frames,
        ordinary_dp: search.ordinary_dp,
        accepted,
    };
    Ok((report, search.scorer.recorder.take()))
}

fn optimize_joint(
    input: &ReplayInput,
    record_candidates: bool,
) -> Result<(SearchResult, Option<super::compiled::Recorder>), SearchError> {
    use super::independent::{FixedTimeline, JointTimeline};
    let baseline = replay(input)?;
    let power = crate::scoring::ScoringPower::new(input.power)?;
    let fixed = FixedTimeline::new(input)?;
    let coordinate = fixed
        .prepare(&input.hit_frames)?
        .expect("valid baseline")
        .maximum(power)?
        .expect("valid baseline");
    let mut request = input.clone();
    request.hit_frames = coordinate.hit_frames;
    request.hit_order = coordinate.hit_order;
    let coordinate_result = replay(&request)?;
    let joint = JointTimeline::new(input)?.maximum(power)?;
    if joint.result.total < coordinate_result.total {
        return Err(SearchError::Mismatch {
            cached: joint.result.total,
            replayed: coordinate_result.total,
        });
    }
    let mut accepted = vec![];
    let result = if joint.result.total == coordinate_result.total {
        coordinate_result.clone()
    } else {
        accepted.push(record(
            1,
            None,
            &coordinate_result.hit_frames,
            &joint.result.hit_frames,
            coordinate_result.total,
            joint.result.total,
        ));
        joint.result
    };
    let mut recorder = record_candidates.then(|| {
        super::compiled::Recorder::new(input, &frame_domains(input).expect("validated input"))
    });
    if let Some(recorder) = &mut recorder {
        for r in [&baseline, &coordinate_result, &result] {
            let mut terms = vec![[0; 3]; input.notes.len()];
            for t in &r.trace {
                terms[t.note_index] = [
                    t.judgement.multiplier().to_bits(),
                    input.combo_multiplier(t.combo).to_bits(),
                    (t.fever_multiplier * t.skill_multiplier).to_bits(),
                ];
            }
            let mut witnessed_input = input.clone();
            witnessed_input.hit_order = r.hit_order.clone();
            recorder.record(&witnessed_input, &r.hit_frames, &terms);
        }
    }
    Ok((
        SearchResult {
            complete: true,
            search_revision: SEARCH_REVISION.into(),
            baseline,
            coordinate_result,
            result,
            stop: SearchStop::Complete,
            rounds: 1,
            inner_pass_limit_count: 0,
            evaluations: 2,
            timeline_builds: 0,
            timeline_cache_hits: 0,
            delta_evaluations: 0,
            notes_scored: 0,
            boundary_note_visits: 0,
            candidate_frames: joint.stats.prepared_frames as u64,
            representative_frames: 0,
            ordinary_dp: joint.stats,
            accepted,
        },
        recorder,
    ))
}

fn record(
    round: u64,
    anchor: Option<usize>,
    before: &[u32],
    after: &[u32],
    old: u64,
    new: u64,
) -> AcceptedMove {
    AcceptedMove {
        round,
        anchor,
        gain: new - old,
        score: new,
        changes: before
            .iter()
            .zip(after)
            .enumerate()
            .filter_map(|(note_index, (&a, &b))| {
                (a != b).then_some(HitChange {
                    note_index,
                    from_frame: a,
                    to_frame: b,
                })
            })
            .collect(),
    }
}

struct Search<'a> {
    input: &'a ReplayInput,
    options: SearchOptions,
    scorer: Scorer<'a>,
    choices: Vec<Vec<u32>>,
    anchors: Vec<usize>,
    ordinary: Vec<usize>,
    incident: Vec<Vec<usize>>,
    exhausted: bool,
    inner_limits: u64,
    event_ranks: Vec<usize>,
    dependency_groups: Vec<usize>,
    boundary_note_visits: u64,
    candidate_frames: u64,
    representative_frames: u64,
    fixed_timeline: Option<super::independent::FixedTimeline>,
    ordinary_dp: super::independent::WindowStats,
}

impl<'a> Search<'a> {
    fn new(input: &'a ReplayInput, options: SearchOptions) -> Result<Self, ReplayError> {
        let minimum = input.skills.iter().filter_map(|s| s.correction).fold(
            Judgement::Great,
            |grade, correction| {
                grade.min(match correction {
                    JudgementCorrection::Great => Judgement::Great,
                    JudgementCorrection::Good => Judgement::Good,
                    JudgementCorrection::Bad => Judgement::Bad,
                })
            },
        );
        let mut choices = Vec::with_capacity(input.notes.len());
        let mut anchors = Vec::new();
        let mut ordinary = Vec::new();
        for (i, note) in input.notes.iter().enumerate() {
            let mut frames = note.frame_domain(minimum)?;
            if let Some(r) = &input.native_time_routing {
                if let Some(target) = &r.targets[i] {
                    frames.retain(|&f| {
                        target.active_from_frame <= f && f <= target.active_until_frame
                    });
                }
            }
            if !frames.contains(&input.hit_frames[i]) {
                return Err(ReplayError::Input("baseline lies outside search domain"));
            }
            frames.sort_unstable_by(|&a, &b| {
                (f64::from(a) / f64::from(REPLAY_FPS) - note.time)
                    .abs()
                    .total_cmp(&(f64::from(b) / f64::from(REPLAY_FPS) - note.time).abs())
                    .then(a.cmp(&b))
            });
            choices.push(frames);
            if !matches!(note.window, NoteWindow::PinnedPerfect { .. }) {
                if note.trigger.is_some() {
                    anchors.push(i);
                } else {
                    ordinary.push(i);
                }
            }
        }
        let mut incident = vec![vec![]; input.notes.len()];
        for (i, c) in input.constraints.iter().enumerate() {
            incident[c.before].push(i);
            incident[c.after].push(i);
        }
        let dependency_groups = windows::dependency_groups(input, &choices, &incident);
        Ok(Self {
            input,
            options,
            scorer: Scorer::new(input)?,
            choices,
            anchors,
            ordinary,
            incident,
            exhausted: false,
            inner_limits: 0,
            event_ranks: super::routing::event_ranks(input),
            dependency_groups,
            boundary_note_visits: 0,
            candidate_frames: 0,
            representative_frames: 0,
            fixed_timeline: None,
            ordinary_dp: super::independent::WindowStats::default(),
        })
    }

    fn constraint_holds(&self, index: usize, frames: &[u32]) -> bool {
        let c = &self.input.constraints[index];
        c.allows_frames(frames[c.before], frames[c.after])
            && (!c.require_dispatch_order
                || (frames[c.before], self.event_ranks[c.before])
                    < (frames[c.after], self.event_ranks[c.after]))
    }
    fn constraints_hold(&self, frames: &[u32]) -> bool {
        (0..self.input.constraints.len()).all(|i| self.constraint_holds(i, frames))
    }

    fn audit(&self, frames: &[u32], expected: u64) -> Result<ReplayResult, SearchError> {
        let mut input = self.input.clone();
        input.hit_frames = frames.to_vec();
        let result = replay(&input)?;
        if result.total != expected {
            return Err(SearchError::Mismatch {
                cached: expected,
                replayed: result.total,
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
