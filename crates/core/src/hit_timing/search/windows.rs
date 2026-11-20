use super::{super::clock::Timeline, scorer::PreparedPlan, *};

fn optional<T>(result: Result<T, ReplayError>) -> Result<Option<T>, SearchError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(
            ReplayError::Judgement { .. }
            | ReplayError::SkillOrder
            | ReplayError::InputTarget { .. }
            | ReplayError::InputBinding { .. },
        ) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// The legal frame domains never change within a search. Compile their overlap
/// and dependency components once, rather than flood-filling for every window.
pub(super) fn dependency_groups(
    input: &ReplayInput,
    choices: &[Vec<u32>],
    incident: &[Vec<usize>],
) -> Vec<usize> {
    let spans: Vec<_> = choices
        .iter()
        .map(|v| (*v.iter().min().unwrap(), *v.iter().max().unwrap()))
        .collect();
    let mut groups = vec![usize::MAX; choices.len()];
    for root in 0..choices.len() {
        if groups[root] != usize::MAX {
            continue;
        }
        groups[root] = root;
        let mut pending = vec![root];
        while let Some(i) = pending.pop() {
            let mut neighbours = Vec::new();
            for (j, &(a, b)) in spans.iter().enumerate() {
                if spans[i].0 <= b && a <= spans[i].1 {
                    neighbours.push(j);
                }
            }
            if let Some(r) = &input.native_time_routing {
                if r.distance_mode == super::super::CandidateDistanceMode::ChartTime {
                    let a = r.targets[i].as_ref().unwrap();
                    for (j, target) in r.targets.iter().enumerate() {
                        let b = target.as_ref().unwrap();
                        if a.button == b.button
                            && a.active_from_frame <= b.active_until_frame
                            && b.active_from_frame <= a.active_until_frame
                        {
                            neighbours.push(j);
                        }
                    }
                }
            }
            for &c in &incident[i] {
                let c = &input.constraints[c];
                neighbours.extend([c.before, c.after]);
            }
            if let Some(r) = &input.native_time_routing {
                for c in &r.connections {
                    if c.head == i || c.end == i {
                        neighbours.extend([c.head, c.end]);
                    }
                }
            }
            for j in neighbours {
                if groups[j] == usize::MAX {
                    groups[j] = root;
                    pending.push(j);
                }
            }
        }
    }
    groups
}

impl Search<'_> {
    fn available(&mut self) -> bool {
        if self.options.max_evaluations != 0 && self.scorer.calls >= self.options.max_evaluations {
            self.exhausted = true;
        }
        !self.exhausted
    }

    fn raw(&self, i: usize, frame: u32) -> Result<Judgement, ReplayError> {
        let note = &self.input.notes[i];
        note.raw_judgement(frame)
    }

    fn fever(&self, frame: u32) -> bool {
        self.input
            .fever_frames
            .is_some_and(|[a, b]| a <= frame && frame < b)
    }

    /// Seed from actual skill/Fever boundaries, then close over possible order
    /// changes and explicit dependencies, including a far-away connection end.
    pub(super) fn shortlist(&mut self, frames: &[u32]) -> Result<Vec<usize>, ReplayError> {
        let timeline = self.scorer.schedule(frames)?;
        let mut selected = vec![false; frames.len()];
        for &i in &self.anchors {
            selected[self.dependency_groups[i]] = true;
        }
        for &i in &self.ordinary {
            if self.raw(i, frames[i])? != Judgement::Perfect
                || self.choices[i].iter().any(|&f| {
                    timeline.slot(f) != timeline.slot(frames[i])
                        || self.fever(f) != self.fever(frames[i])
                })
            {
                selected[self.dependency_groups[i]] = true;
            }
        }
        Ok(self
            .ordinary
            .iter()
            .copied()
            .filter(|&i| selected[self.dependency_groups[i]])
            .collect())
    }

    /// With other inputs fixed, same raw grade/skill/Fever/rank gives identical
    /// score and skill state. Routing/explicit dependencies keep all frames:
    /// equal scoring signatures alone do not prove equal input feasibility.
    fn representatives(
        &mut self,
        i: usize,
        frames: &[u32],
        timeline: &Timeline,
        base: &PreparedPlan,
    ) -> Result<Vec<u32>, ReplayError> {
        self.candidate_frames += self.choices[i].len() as u64;
        if self.input.native_time_routing.is_some() || !self.incident[i].is_empty() {
            let choices: Vec<_> = self.choices[i]
                .iter()
                .copied()
                .filter(|&f| f != frames[i])
                .collect();
            self.representative_frames += choices.len() as u64;
            return Ok(choices);
        }
        let signature = |frame| -> Result<_, ReplayError> {
            let rank = base.rank_after_move(i, frame, &self.event_ranks);
            Ok((
                self.raw(i, frame)?,
                timeline.slot(frame),
                self.fever(frame),
                rank,
            ))
        };
        let mut seen = vec![signature(frames[i])?];
        let mut out = Vec::new();
        for &f in &self.choices[i] {
            let key = signature(f)?;
            if !seen.contains(&key) {
                seen.push(key);
                out.push(f);
            }
        }
        self.representative_frames += out.len() as u64;
        Ok(out)
    }

    fn delta(&mut self, base: &PreparedPlan, frames: &[u32]) -> Result<Option<u64>, SearchError> {
        if !self.available() {
            return Ok(None);
        }
        optional(self.scorer.score_delta(base, frames))
    }

    /// Move a boundary group into one actual window together. Perfect-only and
    /// Great-allowed variants compete by exact total, including stateful skills.
    fn pack(
        &self,
        frames: &[u32],
        indices: &[usize],
        slot: usize,
        great: bool,
        timeline: &Timeline,
    ) -> Result<Vec<u32>, ReplayError> {
        let state = super::super::skill::SkillState::new(&self.input.skills[slot])?;
        let mut trial = frames.to_vec();
        let minimum = if great {
            Judgement::Great
        } else {
            Judgement::Perfect
        };
        for &i in indices {
            let current_grade = state.correct(self.raw(i, frames[i])?);
            // Do not move an already suitable hit merely to choose another
            // equally graded frame. Whole-window proposals should stay sparse.
            let mut best = (timeline.slot(frames[i]) == Some(slot) && current_grade >= minimum)
                .then_some((current_grade, frames[i]));
            for &f in &self.choices[i] {
                if timeline.slot(f) != Some(slot) {
                    continue;
                }
                let grade = state.correct(self.raw(i, f)?);
                if grade < minimum {
                    continue;
                }
                if best.is_none_or(|(old, _)| grade > old) {
                    best = Some((grade, f));
                }
            }
            if let Some((_, f)) = best {
                trial[i] = f;
            }
        }
        Ok(trial)
    }

    /// Refine two neighbouring ordinary hits jointly after single-hit moves
    /// settle. Equal raw grades, windows, Fever flags and final event ranks
    /// produce identical native scoring terms; record a signature only after a
    /// legal replay so an invalid representative cannot hide a feasible one.
    fn refine_neighbors(
        &mut self,
        frames: &[u32],
        base: &PreparedPlan,
    ) -> Result<Option<(Vec<u32>, u64)>, SearchError> {
        let mut indices = self.shortlist(frames)?;
        indices.sort_unstable_by_key(|&i| (frames[i], self.event_ranks[i]));
        let timeline = self.scorer.schedule(frames)?;
        for pair in indices.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            // This repair targets exchanges in overlapping domains. Disjoint
            // domains and larger joint moves belong to the separate full solver.
            if self.choices[a].iter().max() < self.choices[b].iter().min() {
                continue;
            }
            let domain = |i: usize, other: usize| -> Result<Vec<_>, ReplayError> {
                self.choices[i]
                    .iter()
                    .map(|&f| {
                        let key = (f, self.event_ranks[i]);
                        let rank = base.rank_after_move(i, f, &self.event_ranks)
                            - usize::from((frames[other], self.event_ranks[other]) < key);
                        Ok((
                            f,
                            self.raw(i, f)? as u8,
                            timeline.slot(f),
                            self.fever(f),
                            rank,
                        ))
                    })
                    .collect()
            };
            let da = domain(a, b)?;
            let db = domain(b, a)?;
            let mut seen = std::collections::HashSet::new();
            let mut trial = frames.to_vec();
            for &(fa, ga, sa, va, ra) in &da {
                for &(fb, gb, sb, vb, rb) in &db {
                    let before = (fa, self.event_ranks[a]) < (fb, self.event_ranks[b]);
                    let signature = [
                        (ga, sa, va, ra + usize::from(!before)),
                        (gb, sb, vb, rb + usize::from(before)),
                    ];
                    if seen.contains(&signature) {
                        continue;
                    }
                    trial[a] = fa;
                    trial[b] = fb;
                    if self.constraints_hold(&trial) {
                        if let Some(score) = self.delta(base, &trial)? {
                            seen.insert(signature);
                            if score > base.total {
                                return Ok(Some((trial, score)));
                            }
                        }
                    }
                    if self.exhausted {
                        return Ok(None);
                    }
                }
            }
        }
        Ok(None)
    }

    pub(super) fn refine(
        &mut self,
        mut frames: Vec<u32>,
    ) -> Result<Option<(Vec<u32>, u64)>, SearchError> {
        if !self.available() {
            return Ok(None);
        }
        if self.options.complete_ordinary {
            return self.refine_complete(&frames);
        }
        let Some(mut base) = optional(self.scorer.prepare(&frames))? else {
            return Ok(None);
        };
        let mut current = base.total;
        let timeline = self.scorer.schedule(&frames)?;
        let mut passes = 0u64;
        loop {
            let mut changed = false;
            let indices = self.shortlist(&frames)?;
            self.boundary_note_visits += indices.len() as u64;
            // A whole-window proposal can cross a neutral/loss-making step
            // that single-note strict improvement would never accept.
            for slot in 0..self.input.skills.len() {
                for great in [false, true] {
                    let trial = self.pack(&frames, &indices, slot, great, &timeline)?;
                    if trial == frames || !self.constraints_hold(&trial) {
                        continue;
                    }
                    if let Some(score) = self.delta(&base, &trial)? {
                        if score > current {
                            frames = trial;
                            current = score;
                            changed = true;
                            if !self.available() {
                                return Ok(Some((frames, current)));
                            }
                            base = self.scorer.prepare(&frames)?;
                        }
                    }
                    if self.exhausted {
                        return Ok(Some((frames, current)));
                    }
                }
            }
            for i in indices {
                let original = frames[i];
                let (mut best_score, mut best_frame) = (current, original);
                for frame in self.representatives(i, &frames, &timeline, &base)? {
                    frames[i] = frame;
                    if self.incident[i]
                        .iter()
                        .all(|&c| self.constraint_holds(c, &frames))
                    {
                        if let Some(score) = self.delta(&base, &frames)? {
                            if score > best_score {
                                best_score = score;
                                best_frame = frame;
                            }
                        }
                    }
                    if self.exhausted {
                        break;
                    }
                }
                frames[i] = best_frame;
                if best_frame != original {
                    current = best_score;
                    changed = true;
                    if !self.available() {
                        return Ok(Some((frames, current)));
                    }
                    base = self.scorer.prepare(&frames)?;
                }
                if self.exhausted {
                    return Ok(Some((frames, current)));
                }
            }
            if !changed {
                if let Some((trial, score)) = self.refine_neighbors(&frames, &base)? {
                    frames = trial;
                    current = score;
                    if !self.available() {
                        return Ok(Some((frames, current)));
                    }
                    base = self.scorer.prepare(&frames)?;
                } else {
                    break;
                }
            }
            passes += 1;
            if self.options.max_refine_passes != 0 && passes >= self.options.max_refine_passes {
                self.inner_limits += 1;
                break;
            }
        }
        Ok(Some((frames, current)))
    }

    fn refine_complete(&mut self, frames: &[u32]) -> Result<Option<(Vec<u32>, u64)>, SearchError> {
        if self.fixed_timeline.is_none() {
            self.fixed_timeline = Some(super::super::independent::FixedTimeline::new(self.input)?);
        }
        let mut movable = vec![false; frames.len()];
        for &i in &self.ordinary {
            movable[i] = true;
        }
        let Some(Some(graph)) = optional(
            self.fixed_timeline
                .as_ref()
                .unwrap()
                .prepare_selected(frames, &movable, true),
        )?
        else {
            return Ok(None);
        };
        let Some(result) = graph.maximum(crate::scoring::ScoringPower::new(self.input.power)?)?
        else {
            return Ok(None);
        };
        self.ordinary_dp.accumulate(&result.stats);
        // Preserve the candidate recorder and audit the winner with the shared
        // scorer. Its power-independent terms remain valid at other powers,
        // but this winner alone is not an exhaustive other-power frontier.
        let score = self.scorer.score(&result.hit_frames)?;
        if score != result.score {
            return Err(SearchError::Mismatch {
                cached: result.score,
                replayed: score,
            });
        }
        Ok(Some((result.hit_frames, score)))
    }
}
