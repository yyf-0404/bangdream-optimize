use super::*;

impl Prepared {
    /// A dense run of ordinary notes can overlap two otherwise separate skills.
    /// A certified outside pin at its chart reference is optimal for both
    /// presentation objectives and can expose a cut through that run.
    pub(in crate::hit_timing::independent) fn pin_section_references(
        &mut self,
        row: &Row,
        references: &[u32],
        mut protected: Vec<bool>,
    ) -> Option<Row> {
        for (i, domains) in row.domains.iter().enumerate() {
            protected[i] |= !domains
                .iter()
                .any(|d| d.perfect && d.first <= references[i] && references[i] <= d.last);
        }
        self.input.hit_frames.clone_from_slice(references);
        self.pin_mixed_outside_notes(row, &protected)
            .map(|(_, row)| row)
    }
}

impl Row {
    /// Only cut where hit order, Combo rank, causal constraints and the skill
    /// clock are independent. Each section covers a consecutive time interval.
    pub(in crate::hit_timing::independent) fn timing_sections(
        &self,
        input: &ReplayInput,
    ) -> Result<Vec<Vec<usize>>, ReplayError> {
        let mut spans = self.spans.clone();
        let windows = skill_ranges(input, |note| Ok(self.spans[note]))?;
        let cooling = crate::hit_timing::bounds::countdown(0.75) + 1;
        let mut triggers: Vec<_> = input
            .notes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| Some((n.trigger?, i)))
            .collect();
        triggers.sort_unstable();
        let mut previous = None;
        for ((_, note), (_, start, end)) in triggers.into_iter().zip(windows) {
            let (first, _) = self.spans[note];
            spans[note].1 = spans[note].1.max(end);
            // A queued request depends on the previous start even if no note
            // can be hit during the cooldown between the two playing windows.
            if let Some((previous_start, ready)) = previous {
                if first < ready {
                    spans[note].0 = first.min(previous_start);
                }
            }
            previous = Some((start - 1, end + cooling + 1));
        }
        for edge in &self.relations {
            let (first, last) = self.spans[edge.before];
            let earliest = self.spans[edge.after].0;
            if last < earliest
                && edge.earliest_after.is_some()
                && self.domains[edge.before]
                    .iter()
                    .all(|d| (d.first..=d.last).all(|f| edge.allows_frames(f, earliest)))
            {
                continue;
            }
            spans[edge.after].0 = spans[edge.after].0.min(first);
            spans[edge.after].1 = spans[edge.after].1.max(last);
        }
        if let Some(routing) = &input.native_time_routing {
            for edge in &routing.connections {
                spans[edge.head].1 = spans[edge.head].1.max(self.spans[edge.end].1);
            }
        }
        let mut notes: Vec<_> = (0..spans.len()).collect();
        notes.sort_unstable_by_key(|&i| spans[i].0);
        let mut sections: Vec<Vec<usize>> = vec![];
        let mut end = 0;
        for i in notes {
            let (first, last) = spans[i];
            if sections.is_empty() || first > end {
                sections.push(vec![]);
            }
            end = end.max(last);
            sections.last_mut().unwrap().push(i);
        }
        Ok(sections)
    }
}
