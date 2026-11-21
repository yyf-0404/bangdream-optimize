//! Select a playable witness after team, skill order and native score are fixed.
use super::*;

struct Section {
    notes: Vec<usize>,
    score: u64,
    low: u32,
    high: u32,
}

struct Presentation<'a> {
    input: &'a ReplayInput,
    prepared: Prepared,
    row: geometry::Row,
    references: Vec<u32>,
    seed: &'a ReplayResult,
    power: ScoringPower,
}

impl JointTimeline {
    /// For each independent section, minimize the largest absolute chart-frame
    /// offset. Within those caps maximize the number of zero-offset notes.
    pub fn simplest_maximum(
        input: &ReplayInput,
        limits: &[Option<[u32; 2]>],
        seed: &ReplayResult,
    ) -> Result<ReplayResult, SearchError> {
        let mut winning_input = input.clone();
        winning_input.hit_frames.clone_from(&seed.hit_frames);
        winning_input.hit_order.clone_from(&seed.hit_order);
        let input = &winning_input;
        if replay(input)? != *seed
            || seed
                .hit_frames
                .iter()
                .zip(limits)
                .any(|(&f, range)| range.is_some_and(|[a, b]| f < a || f > b))
        {
            return Err(ReplayError::Input("invalid winning presentation witness").into());
        }
        let references = input.chart_reference_frames()?;
        if limits.len() != references.len()
            || limits.iter().flatten().any(|&[a, b]| {
                a > b || b > crate::hit_timing::MAX_SECONDS * crate::hit_timing::REPLAY_FPS
            })
        {
            return Err(ReplayError::Input("presentation witness must match chart").into());
        }
        if references
            .iter()
            .zip(limits)
            .all(|(&f, range)| range.is_none_or(|[a, b]| a <= f && f <= b))
        {
            let mut request = input.clone();
            request.hit_frames.clone_from(&references);
            if let Ok(result) = replay(&request) {
                if result.total == seed.total {
                    return Ok(result);
                }
            }
        }
        let power = ScoringPower::new(input.power)?;
        let mut prepared = Prepared::new(input)?;
        let mut row =
            prepared
                .restrict_row(prepared.joint_row(), limits)?
                .ok_or(ReplayError::Input(
                    "winning presentation has empty frame domain",
                ))?;
        let mut normalized = None;
        if let Some(pinned) =
            prepared.pin_section_references(&row, &references, local::protected(input)?)
        {
            let mut request = input.clone();
            for (i, domains) in pinned.domains.iter().enumerate() {
                if domains.len() == 1 && domains[0].first == domains[0].last {
                    request.hit_frames[i] = domains[0].first;
                }
            }
            if request.hit_frames != input.hit_frames {
                normalized = replay(&request).ok().filter(|r| r.total == seed.total);
                if normalized.is_none() {
                    let model = Self::from_row(input, &prepared, pinned.clone(), true, false)?;
                    normalized = model.maximum_optional(power)?.map(|w| w.result);
                    if normalized.as_ref().is_none_or(|r| r.total != seed.total) {
                        return Err(
                            ReplayError::Input("reference pins lost the winning score").into()
                        );
                    }
                }
            }
            row = pinned;
        }
        let seed = normalized.as_ref().unwrap_or(seed);
        let mut scores = vec![0; references.len()];
        for hit in &seed.trace {
            scores[hit.note_index] = u64::from(hit.score);
        }
        let mut sections: Vec<_> = row
            .timing_sections(input)?
            .into_iter()
            .map(|notes| {
                let high = notes
                    .iter()
                    .map(|&i| references[i].abs_diff(seed.hit_frames[i]))
                    .max()
                    .unwrap();
                let low = notes
                    .iter()
                    .map(|&i| {
                        row.domains[i]
                            .iter()
                            .map(|d| references[i].abs_diff(references[i].clamp(d.first, d.last)))
                            .min()
                            .unwrap()
                    })
                    .max()
                    .unwrap();
                Section {
                    score: notes.iter().map(|&i| scores[i]).sum(),
                    notes,
                    low,
                    high,
                }
            })
            .collect();
        let plan = Presentation {
            input,
            prepared,
            row,
            references,
            seed,
            power,
        };
        loop {
            let open: Vec<_> = sections
                .iter()
                .enumerate()
                .filter_map(|(i, s)| (s.low < s.high).then_some(i))
                .collect();
            if open.is_empty() {
                break;
            }
            plan.probe(&mut sections, &open)?;
        }
        let mut limits = vec![None; input.notes.len()];
        for section in &sections {
            plan.cap(&mut limits, section, section.high);
        }
        // Score-only compression can pick an arbitrary Perfect frame. Keep all
        // remaining alternatives for the secondary, zero-offset objective.
        let model = plan
            .model(&limits, false)?
            .ok_or(ReplayError::Input("optimal section caps are infeasible"))?;
        let native = JointCosts::new_controlled(&model, plan.power, false)?;
        let mut costs = ReferenceHits {
            native,
            references: &plan.references,
        };
        let best = model
            .maximum_optional_with(plan.power, &mut costs)?
            .ok_or(ReplayError::Input(
                "optimal section caps have no playable witness",
            ))?
            .result;
        if best.total != seed.total {
            return Err(SearchError::Mismatch {
                cached: seed.total,
                replayed: best.total,
            });
        }
        Ok(best)
    }
}

impl Presentation<'_> {
    fn cap(&self, limits: &mut [Option<[u32; 2]>], section: &Section, radius: u32) {
        let last = crate::hit_timing::MAX_SECONDS * crate::hit_timing::REPLAY_FPS;
        for &i in &section.notes {
            let f = self.references[i];
            limits[i] = Some([f.saturating_sub(radius), f.saturating_add(radius).min(last)]);
        }
    }

    fn model(
        &self,
        caps: &[Option<[u32; 2]>],
        local: bool,
    ) -> Result<Option<JointTimeline>, ReplayError> {
        let Some(row) = self.prepared.restrict_row(self.row.clone(), caps)? else {
            return Ok(None);
        };
        JointTimeline::from_row(self.input, &self.prepared, row, local, false).map(Some)
    }

    // Probe independent sections together. Each has a fixed set of Combo ranks
    // and its own attainable score in the seed. If a cap prevents any legal
    // replay at all, isolate it before raising a lower bound.
    fn probe(&self, sections: &mut [Section], open: &[usize]) -> Result<(), SearchError> {
        let mut caps: Vec<_> = self.seed.hit_frames.iter().map(|&f| Some([f, f])).collect();
        for &i in open {
            let s = &sections[i];
            self.cap(&mut caps, s, s.low + (s.high - s.low) / 2);
        }
        let trial = match self.model(&caps, local::enabled())? {
            Some(model) => model.maximum_optional(self.power)?,
            None => None,
        };
        if trial.is_none() && open.len() > 1 {
            let (a, b) = open.split_at(open.len() / 2);
            self.probe(sections, a)?;
            return self.probe(sections, b);
        }
        let mut scores = vec![0; caps.len()];
        if let Some(winner) = &trial {
            for hit in &winner.result.trace {
                scores[hit.note_index] = u64::from(hit.score);
            }
        }
        for &i in open {
            let s = &mut sections[i];
            let mid = s.low + (s.high - s.low) / 2;
            if trial.is_some() && s.notes.iter().map(|&n| scores[n]).sum::<u64>() == s.score {
                s.high = mid;
            } else {
                s.low = mid + 1;
            }
        }
        Ok(())
    }
}

struct ReferenceHits<'a, C> {
    native: C,
    references: &'a [u32],
}

impl<C: ActionCosts> ActionCosts for ReferenceHits<'_, C> {
    fn action_at(
        &mut self,
        index: usize,
        action: &Action,
        state: usize,
    ) -> Result<Option<(usize, Value)>, ReplayError> {
        Ok(self
            .native
            .action_at(index, action, state)?
            .map(|(next, mut value)| {
                value.coefficient = action
                    .hits
                    .iter()
                    .filter(|&&(i, f)| self.references[i] == f)
                    .count() as f64;
                (next, value)
            }))
    }

    fn compare(&self, a: Value, b: Value, _: WindowObjective) -> std::cmp::Ordering {
        a.score
            .cmp(&b.score)
            .then_with(|| a.coefficient.total_cmp(&b.coefficient))
    }

    fn hit(&mut self, factor: Factor, state: usize) -> Result<(usize, Value), ReplayError> {
        self.native.hit(factor, state)
    }

    fn run(
        &mut self,
        factor: Factor,
        state: usize,
        count: usize,
    ) -> Result<(usize, Value), ReplayError> {
        self.native.run(factor, state, count)
    }
}
