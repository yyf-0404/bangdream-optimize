use super::super::compiled::{Recorder, Term};
use super::super::{
    clock::Timeline, skill::SkillState, NoteWindow, ReplayError, ReplayInput, MAX_SECONDS,
    REPLAY_FPS,
};
use crate::scoring::{Judgement, NoteScorer, ScoringError, ScoringPower};
use std::{collections::HashMap, rc::Rc};

type Key = Vec<(u32, usize, usize)>;

/// Exact prefix checkpoints for one fixed skill schedule. A changed ordinary
/// hit replays only until the unchanged suffix has the same future skill state.
pub(super) struct PreparedPlan {
    frames: Vec<u32>,
    order: Vec<usize>,
    prefix: Vec<u64>,
    states: Vec<Vec<SkillState>>,
    future_slots: Vec<u8>,
    timeline: Rc<Timeline>,
    pub total: u64,
    terms: Option<Vec<Term>>,
}
impl PreparedPlan {
    pub fn rank_after_move(&self, i: usize, frame: u32, ranks: &[usize]) -> usize {
        let key = (frame, ranks[i]);
        let rank = self
            .order
            .partition_point(|&j| (self.frames[j], ranks[j]) < key);
        rank - usize::from((self.frames[i], ranks[i]) < key)
    }
}
/// A search-local cache: definitions/power/Fever never change during its life.
/// Timelines and exact score checkpoints are reusable only under their fixed
/// schedule. Changed judgments/order/state are replayed without approximation.
pub(super) struct Scorer<'a> {
    input: &'a ReplayInput,
    scorer: NoteScorer,
    triggers: Vec<(usize, usize)>,
    cache: HashMap<Key, (Rc<Timeline>, u64)>,
    tick: u64,
    pub calls: u64,
    pub builds: u64,
    pub hits: u64,
    pub delta_calls: u64,
    pub notes_scored: u64,
    router: super::super::routing::Router<'a>,
    event_ranks: Vec<usize>,
    pub recorder: Option<Recorder>,
    last_term: Term,
}

impl<'a> Scorer<'a> {
    pub fn new(input: &'a ReplayInput) -> Result<Self, ReplayError> {
        // Caller has already accepted a full replay, including structural checks.
        let scorer = NoteScorer::new(
            ScoringPower::new(input.power)?,
            input.play_level,
            input.notes.len() as u32,
        )?;
        Ok(Self {
            input,
            scorer,
            triggers: input
                .notes
                .iter()
                .enumerate()
                .filter_map(|(i, note)| note.trigger.map(|s| (i, s)))
                .collect(),
            cache: HashMap::new(),
            tick: 0,
            calls: 0,
            builds: 0,
            hits: 0,
            delta_calls: 0,
            notes_scored: 0,
            router: super::super::routing::Router::new(input),
            event_ranks: super::super::routing::event_ranks(input),
            recorder: None,
            last_term: [0; 3],
        })
    }

    pub fn schedule(&mut self, frames: &[u32]) -> Result<Rc<Timeline>, ReplayError> {
        let key = Timeline::key(self.input, frames)?;
        self.tick += 1;
        if let Some((timeline, used)) = self.cache.get_mut(&key) {
            *used = self.tick;
            self.hits += 1;
            return Ok(Rc::clone(timeline));
        }
        let timeline = Rc::new(Timeline::from_requests(&self.input.skills, &key)?);
        if self.cache.len() >= 128 {
            let oldest = self
                .cache
                .iter()
                .min_by_key(|(_, (_, used))| used)
                .unwrap()
                .0
                .clone();
            self.cache.remove(&oldest);
        }
        self.cache.insert(key, (Rc::clone(&timeline), self.tick));
        self.builds += 1;
        Ok(timeline)
    }

    /// Constraints are checked before adopting a plan, not against the old
    /// ordinary-note positions while constructing a joint proposal.
    pub fn score(&mut self, frames: &[u32]) -> Result<u64, ReplayError> {
        self.calls += 1;
        let timeline = self.schedule(frames)?;
        let mut states = self
            .input
            .skills
            .iter()
            .map(SkillState::new)
            .collect::<Result<Vec<_>, _>>()?;
        let mut order: Vec<_> = (0..frames.len()).collect();
        order.sort_unstable_by_key(|&i| (frames[i], self.event_ranks[i]));
        let mut total = 0u64;
        let mut terms = self.recorder.as_ref().map(|_| vec![[0; 3]; frames.len()]);
        self.router.reset();
        for (rank, index) in order.into_iter().enumerate() {
            let frame = frames[index];
            self.router.press(index, frame)?;
            let score = self.note_score(index, frame, rank, &timeline, &mut states)?;
            if let Some(terms) = &mut terms {
                terms[index] = self.last_term;
            }
            total = total
                .checked_add(u64::from(score))
                .ok_or(ScoringError::TotalScoreOverflow)?;
        }
        self.record(frames, terms.as_deref());
        Ok(total)
    }

    pub fn prepare(&mut self, frames: &[u32]) -> Result<PreparedPlan, ReplayError> {
        self.calls += 1;
        let timeline = self.schedule(frames)?;
        let mut states = self
            .input
            .skills
            .iter()
            .map(SkillState::new)
            .collect::<Result<Vec<_>, _>>()?;
        let mut order: Vec<_> = (0..frames.len()).collect();
        order.sort_unstable_by_key(|&i| (frames[i], self.event_ranks[i]));
        let mut prefixes = vec![0u64];
        let mut checkpoints = vec![states.clone()];
        let mut terms = self.recorder.as_ref().map(|_| vec![[0; 3]; frames.len()]);
        self.router.reset();
        for (rank, &i) in order.iter().enumerate() {
            self.router.press(i, frames[i])?;
            let score = self.note_score(i, frames[i], rank, &timeline, &mut states)?;
            if let Some(terms) = &mut terms {
                terms[i] = self.last_term;
            }
            prefixes.push(
                prefixes
                    .last()
                    .unwrap()
                    .checked_add(u64::from(score))
                    .ok_or(ScoringError::TotalScoreOverflow)?,
            );
            checkpoints.push(states.clone());
        }
        let mut future_slots = vec![0; order.len() + 1];
        for rank in (0..order.len()).rev() {
            future_slots[rank] = future_slots[rank + 1];
            if let Some(slot) = timeline.slot(frames[order[rank]]) {
                future_slots[rank] |= 1 << slot;
            }
        }
        self.record(frames, terms.as_deref());
        Ok(PreparedPlan {
            frames: frames.to_vec(),
            order,
            total: *prefixes.last().unwrap(),
            prefix: prefixes,
            states: checkpoints,
            future_slots,
            timeline,
            terms,
        })
    }

    pub fn score_delta(&mut self, base: &PreparedPlan, frames: &[u32]) -> Result<u64, ReplayError> {
        // Only ordinary-note changes reuse checkpoints. Trigger changes build
        // their own exact schedule, including all delayed successor skills.
        if frames.len() != base.frames.len()
            || self
                .triggers
                .iter()
                .any(|&(i, _)| frames[i] != base.frames[i])
        {
            return self.score(frames);
        }
        self.calls += 1;
        self.delta_calls += 1;
        if frames.iter().any(|&f| f > MAX_SECONDS * REPLAY_FPS) {
            return Err(ReplayError::Input("invalid candidate frame range"));
        }
        for (index, (note, &frame)) in self.input.notes.iter().zip(frames).enumerate() {
            if matches!(note.window, NoteWindow::PinnedPerfect { frame: pinned } if frame != pinned)
            {
                return Err(ReplayError::Note {
                    index,
                    reason: "pinned node moved",
                });
            }
        }
        let mut order: Vec<_> = (0..frames.len()).collect();
        order.sort_unstable_by_key(|&i| (frames[i], self.event_ranks[i]));
        // Candidate ownership remains checked for the whole input sequence.
        // Score checkpoints do not authorize skipping input legality checks.
        if self.input.native_time_routing.is_some() {
            self.router.reset();
            for &i in &order {
                self.router.press(i, frames[i])?;
                // Unchanged hits were already valid in this exact schedule.
                // Check changed grades here to preserve first-error ordering.
                if frames[i] != base.frames[i] {
                    self.judgement(i, frames[i], &base.timeline, &base.states[0])?;
                }
            }
        }
        let same = |rank: usize| {
            order[rank] == base.order[rank] && frames[order[rank]] == base.frames[order[rank]]
        };
        let first = (0..order.len())
            .find(|&rank| !same(rank))
            .unwrap_or(order.len());
        if first == order.len() {
            return Ok(base.total);
        }
        let suffix = (first..order.len()).rfind(|&rank| !same(rank)).unwrap() + 1;
        let mut states = base.states[first].clone();
        let mut total = base.prefix[first];
        let mut terms = base.terms.clone();
        for rank in first..order.len() {
            if rank >= suffix
                && states
                    .iter()
                    .zip(&base.states[rank])
                    .enumerate()
                    .all(|(slot, (a, b))| {
                        let a = a.snapshot();
                        let b = b.snapshot();
                        base.future_slots[rank] & (1 << slot) == 0
                            || (a.worst_judgement == b.worst_judgement
                                && a.extra_percent.to_bits() == b.extra_percent.to_bits())
                    })
            {
                self.record(frames, terms.as_deref());
                return total
                    .checked_add(base.total - base.prefix[rank])
                    .ok_or(ScoringError::TotalScoreOverflow.into());
            }
            let i = order[rank];
            let score = self.note_score(i, frames[i], rank, &base.timeline, &mut states)?;
            if let Some(terms) = &mut terms {
                terms[i] = self.last_term;
            }
            total = total
                .checked_add(u64::from(score))
                .ok_or(ScoringError::TotalScoreOverflow)?;
        }
        self.record(frames, terms.as_deref());
        Ok(total)
    }

    fn record(&mut self, frames: &[u32], terms: Option<&[Term]>) {
        if let (Some(recorder), Some(terms)) = (&mut self.recorder, terms) {
            recorder.record(self.input, frames, terms);
        }
    }

    fn note_score(
        &mut self,
        index: usize,
        frame: u32,
        rank: usize,
        timeline: &Timeline,
        states: &mut [SkillState],
    ) -> Result<u32, ReplayError> {
        self.notes_scored += 1;
        let grade = self.judgement(index, frame, timeline, states)?;
        let multiplier = timeline
            .slot(frame)
            .map_or(1.0, |slot| states[slot].multiplier(grade));
        let combo = self
            .input
            .initial_combo
            .checked_add(rank as u32 + 1)
            .ok_or(ReplayError::Input("combo overflow"))?;
        let fever = if self
            .input
            .fever_frames
            .is_some_and(|[a, b]| a <= frame && frame < b)
        {
            2.0
        } else {
            1.0
        };
        let score =
            self.scorer
                .score(grade, self.input.combo_multiplier(combo), multiplier, fever)?;
        if self.recorder.is_some() {
            self.last_term = [
                grade.multiplier().to_bits(),
                self.input.combo_multiplier(combo).to_bits(),
                (fever * multiplier).to_bits(),
            ];
        }
        Ok(score.final_score)
    }

    fn judgement(
        &self,
        index: usize,
        frame: u32,
        timeline: &Timeline,
        states: &[SkillState],
    ) -> Result<Judgement, ReplayError> {
        let note = &self.input.notes[index];
        let raw = note.raw_judgement(frame)?;
        let grade = timeline
            .slot(frame)
            .map_or(raw, |slot| states[slot].correct(raw));
        if grade < Judgement::Great {
            return Err(ReplayError::Judgement {
                index,
                raw,
                corrected: grade,
            });
        }
        Ok(grade)
    }
}
