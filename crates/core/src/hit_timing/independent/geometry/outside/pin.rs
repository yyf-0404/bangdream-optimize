//! Narrow independent outside notes before overlap components are built. Moving
//! a hit within its original span can reorder other hits in that span. Require
//! one exact Combo multiplier for every possible rank there, uniform Fever and
//! universally satisfied time gaps and no binding dependency. Consumption is checked in
//! both directions against every remaining event, not only the chosen hit.
use super::*;

impl Prepared {
    pub(in crate::hit_timing::independent::geometry) fn pin_mixed_outside_notes(
        &self,
        row: &Row,
        protected: &[bool],
    ) -> Option<(usize, Row)> {
        let n = row.spans.len();
        let mut sorted: Vec<_> = (0..n).collect();
        sorted.sort_unstable_by_key(|&i| (row.spans[i].0, self.ranks[i]));
        let mut mixed_outside = vec![false; n];
        let mut begin = 0;
        while begin < n {
            let mut end = begin + 1;
            let mut last = row.spans[sorted[begin]].1;
            let mut mixed = protected[sorted[begin]];
            while end < n && row.spans[sorted[end]].0 <= last {
                last = last.max(row.spans[sorted[end]].1);
                mixed |= protected[sorted[end]];
                end += 1;
            }
            if mixed {
                for &i in &sorted[begin..end] {
                    mixed_outside[i] = !protected[i];
                }
            }
            begin = end;
        }
        if !mixed_outside.iter().any(|&yes| yes) {
            return None;
        }

        let mut starts: Vec<_> = row.spans.iter().map(|s| s.0).collect();
        let mut ends: Vec<_> = row.spans.iter().map(|s| s.1).collect();
        starts.sort_unstable();
        ends.sort_unstable();
        let mut pins = vec![None; n];
        for i in 0..n {
            let (first, last) = row.spans[i];
            if !mixed_outside[i]
                || first == last
                || self.input.notes[i].trigger.is_some()
                || self.heads[i].is_some()
                || self.ends[i].is_some()
            {
                continue;
            }
            let fever = row.domains[i][0].fever;
            if row.domains[i].iter().any(|d| d.fever != fever) {
                continue;
            }

            // At any frame in [first,last], all notes ending before first have
            // already hit, and only notes starting by last could have hit.
            // This also bounds ranks of every OTHER hit that can be reordered.
            let low = ends.partition_point(|&f| f < first) + 1;
            let high = starts.partition_point(|&f| f <= last);
            let Some(low) = self.input.initial_combo.checked_add(low as u32) else {
                continue;
            };
            let Some(high) = self.input.initial_combo.checked_add(high as u32) else {
                continue;
            };
            // Both native Combo tables are monotone; equal endpoint bits prove
            // the same f32 multiplier throughout this inclusive rank interval.
            if self.input.combo_multiplier(low).to_bits()
                != self.input.combo_multiplier(high).to_bits()
            {
                continue;
            }

            let mut perfect = row.domains[i].iter().filter(|d| d.perfect);
            let Some(domain) = perfect
                .clone()
                .find(|d| d.first <= self.input.hit_frames[i] && self.input.hit_frames[i] <= d.last)
                .or_else(|| perfect.next())
            else {
                continue;
            };
            let frame = if domain.first <= self.input.hit_frames[i]
                && self.input.hit_frames[i] <= domain.last
            {
                self.input.hit_frames[i]
            } else {
                domain.first
            };
            // A fixed endpoint may make an originally retained gap redundant.
            // Prove it against EVERY original frame of the other endpoint.
            // Arrival curves can be nonmonotone, so keep their complete search.
            if row.incident[i].iter().any(|&index| {
                let relation = &row.relations[index];
                if relation.earliest_after.is_some() {
                    return true;
                }
                let (before, after) = if relation.before == i {
                    (frame, row.spans[relation.after].0)
                } else {
                    (row.spans[relation.before].1, frame)
                };
                !relation.allows_frames(before, after)
                    || (relation.strict_event_order && before >= after)
            }) {
                continue;
            }
            // The fixed hit must be selectable for EVERY external schedule.
            // A strictly earlier domain end also proves same-frame order.
            if self
                .blockers(i, frame)
                .iter()
                .any(|&other| row.spans[other].1 >= frame)
            {
                continue;
            }
            pins[i] = Some(ActionInterval {
                first: frame,
                last: frame,
                ..*domain
            });
        }
        // Moving a root later must not remove a legal earlier hit of another
        // root that needed this one consumed. Keep the complete original row
        // when checking this, so individually proven pins compose safely.
        for (&(note, frame), blockers) in &self.blocker_cache {
            if !row.domains[note]
                .iter()
                .any(|d| d.first <= frame && frame <= d.last)
            {
                continue;
            }
            for &other in blockers {
                if pins[other].is_some_and(|d| d.first >= frame) {
                    pins[other] = None;
                }
            }
        }
        let count = pins.iter().filter(|p| p.is_some()).count();
        if count == 0 {
            return None;
        }
        let mut result = row.clone();
        for (i, &pin) in pins.iter().enumerate() {
            if let Some(pin) = pin {
                result.domains[i] = vec![pin];
                result.spans[i] = (pin.first, pin.last);
            }
        }
        // These edges were proven for all original choices above. Removing
        // them lets the normal overlap/Run compiler close the outside block.
        // Relations between two unpinned notes retain every original check.
        result
            .relations
            .retain(|r| pins[r.before].is_none() && pins[r.after].is_none());
        result.incident = vec![vec![]; n];
        for (index, relation) in result.relations.iter().enumerate() {
            result.incident[relation.before].push(index);
            result.incident[relation.after].push(index);
        }
        // Each replacement preserves every other hit's grade, Fever, Combo
        // multiplier, trigger time and binding. This outside hit becomes P.
        // Native f32 multiplication and integer truncations remain untouched.
        Some((count, result))
    }
}
