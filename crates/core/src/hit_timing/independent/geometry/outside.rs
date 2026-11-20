//! Exact all-Perfect transfers wholly outside every possible skill window.
use super::*;

mod cache;
mod pin;
pub(in crate::hit_timing::independent) use cache::OutsideCache;

#[derive(Clone)]
struct OutsideTemplate {
    action: Action,
    required_settled: Vec<usize>,
    empty_slots: Vec<usize>,
}
impl OutsideTemplate {
    fn accepts(&self, settled: &[bool], keys: &[Key]) -> bool {
        self.required_settled.iter().all(|&i| settled[i])
            && self.empty_slots.iter().all(|&slot| keys.iter().all(|key| key.bindings[slot] == 0))
    }
    fn bytes(&self) -> usize {
        let hits = match &self.action.hits {
            ActionHits::Run(hits) => hits.capacity() * std::mem::size_of::<(usize, u32)>(),
            ActionHits::Event(_) => 0,
        };
        hits
            + (self.required_settled.capacity() + self.empty_slots.capacity())
                * std::mem::size_of::<usize>()
    }
}

impl Prepared {
    pub(super) fn outside_components(
        &self,
        row: &Row,
        components: Vec<Vec<usize>>,
        protected: &[bool],
    ) -> Vec<Vec<usize>> {
        let mut ids = vec![0; self.input.notes.len()];
        let mut prefix = vec![0usize];
        for (id, notes) in components.iter().enumerate() {
            for &note in notes { ids[note] = id; }
            prefix.push(prefix.last().unwrap() + usize::from(notes.iter().any(|&i| protected[i])));
        }
        let mut ends: Vec<_> = (0..components.len()).collect();
        let mut join = |a: usize, b: usize| {
            let (first, last) = (ids[a].min(ids[b]), ids[a].max(ids[b]));
            // Keep the existing boundary state for dependencies crossing a
            // protected region. Close only fully outside dependency blocks.
            if prefix[last + 1] == prefix[first] { ends[first] = ends[first].max(last); }
        };
        for r in &row.relations { join(r.before, r.after); }
        if let Some(routing) = &self.input.native_time_routing {
            for c in &routing.connections { join(c.head, c.end); }
        }
        for (&(note, _), blockers) in &self.blocker_cache {
            for &other in blockers { join(note, other); }
        }
        let mut result: Vec<Vec<usize>> = vec![];
        let mut last = 0;
        for (id, notes) in components.into_iter().enumerate() {
            if result.is_empty() || id > last { result.push(vec![]); }
            last = last.max(ends[id]);
            result.last_mut().unwrap().extend(notes);
        }
        result
    }

    pub(super) fn outside_run(
        &self,
        row: &Row,
        notes: &[usize],
        settled: &[bool],
        keys: &[Key],
    ) -> Option<Action> {
        let template = self.outside_template(row, notes)?;
        template.accepts(settled, keys).then_some(template.action)
    }

    fn outside_template(&self, row: &Row, notes: &[usize]) -> Option<OutsideTemplate> {
        let first = row.domains[notes[0]][0];
        let mut hits = Vec::with_capacity(notes.len());
        let mut empty_slots = vec![];
        for &i in notes {
            if self.input.notes[i].trigger.is_some() || row.domains[i].iter()
                .any(|d| d.fever != first.fever) { return None; }
            let mut perfect = row.domains[i].iter().filter(|d| d.perfect);
            let frame = perfect.clone()
                .find(|d| d.first <= self.input.hit_frames[i] && self.input.hit_frames[i] <= d.last)
                .map(|_| self.input.hit_frames[i])
                .or_else(|| perfect.next().map(|d| d.first))?;
            hits.push((i, frame));
        }
        hits.sort_unstable_by_key(|&(i, frame)| (frame, self.ranks[i]));
        let chosen: HashMap<_, _> = hits.iter().copied().collect();
        for &i in notes {
            for &index in &row.incident[i] {
                let r = &row.relations[index];
                let (&before, &after) = (chosen.get(&r.before)?, chosen.get(&r.after)?);
                if !r.allows_frames(before, after) || (r.strict_event_order
                    && (before, self.ranks[r.before]) >= (after, self.ranks[r.after])) {
                    return None;
                }
            }
            for &c in self.heads[i].iter().chain(self.ends[i].iter()) {
                let connection = &self.input.native_time_routing.as_ref()?.connections[c];
                if !chosen.contains_key(&connection.head) || !chosen.contains_key(&connection.end) {
                    return None;
                }
                empty_slots.push(self.connection_slots[c]);
            }
        }
        let mut consumed = vec![];
        let mut required_settled = vec![];
        let mut bindings = vec![0; self.binding_slots];
        for &(note, frame) in &hits {
            for &other in self.blockers(note, frame) {
                if !consumed.contains(&other) {
                    if chosen.contains_key(&other) { return None; }
                    required_settled.push(other);
                }
            }
            if let Some(c) = self.ends[note] {
                let slot = self.connection_slots[c];
                if bindings[slot] != c + 1 { return None; }
                bindings[slot] = 0;
            }
            if let Some(c) = self.heads[note] { bindings[self.connection_slots[c]] = c + 1; }
            consumed.push(note);
        }
        if bindings.iter().any(|&binding| binding != 0) { return None; }
        // Every accepted hit has <= Perfect's score. Fever is uniform and the
        // fixed number of hits covers the same Combo ranks in every schedule.
        // All dependencies close here, and touched bindings start/end empty.
        // This legal all-P schedule attains an upper bound for every entry state.
        required_settled.sort_unstable();
        required_settled.dedup();
        empty_slots.sort_unstable();
        empty_slots.dedup();
        Some(OutsideTemplate {
            action: Action {
                factor: Factor { perfect: true, inside: false, slot: 0, fever: first.fever,
                    reference: 0., rank: 0 },
                hits: hits.into(),
            },
            required_settled,
            empty_slots,
        })
    }
}
