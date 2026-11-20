//! Within-frame subset DP: a layer chooses the next touch, or pads with no hit.
//! At most events.len() touches can succeed because every note settles once.
//! This is a completeness bound, not an exploration cap. Geometry keys merge
//! equal consumed sets/timestamps/bindings; the value DP also retains skill state.
use super::*;
mod dag;
pub(super) mod template;

impl Prepared {
    pub(super) fn compile_touch_frame(
        &self,
        row: &Row,
        frame: u32,
        events: &[(usize, ActionInterval)],
        active: &[usize],
        settled: &[bool],
        expired: usize,
        mut keys: Vec<Key>,
        graph: &mut Graph,
        options: WindowOptions,
        stats: &mut WindowStats,
        templates: &mut template::Cache,
    ) -> Option<Vec<Key>> {
        if options.max_cached_component_edges == 0 && options.touch_dag {
            if templates.append(self, row, frame, events, active, expired, &keys, graph, options, stats) {
                return Some(keys);
            }
            let start = (graph.layers.len(), graph.actions.len());
            let keys = self.compile_touch_dag(
                row, frame, events, active, settled, expired, keys, graph, options, stats,
            )?;
            templates.capture(self, row, frame, events, active, expired, &keys, graph, start, options);
            return Some(keys);
        }
        let mut actions = HashMap::new();
        // The next layer replaces `keys`; keep the retired buffer and the
        // interning table's buckets for the following layer of this frame.
        let mut next = Vec::new();
        let mut map = HashMap::new();
        for _ in 0..events.len() {
            next.clear();
            map.clear();
            let mut edges = vec![];
            for (from, key) in keys.iter().enumerate() {
                let to = intern_borrowed(key, &mut next, &mut map, stats);
                edges.push(Edge {
                    from,
                    to,
                    action: None,
                });
                for &(note, d) in events {
                    let Some((hit, rank)) = self.touch_key(
                        row, frame, note, active, settled, expired, key,
                    ) else { continue; };
                    let to = intern(hit, &mut next, &mut map);
                    let reused = options
                        .reuse_graph_storage
                        .then(|| actions.get(&(note, rank)))
                        .flatten()
                        .copied();
                    let action = if let Some(a) = reused {
                        stats.graph_action_reuses += 1;
                        a
                    } else {
                        let a = graph.actions.len();
                        graph.actions.push(Action {
                            factor: Factor {
                                perfect: d.perfect,
                                inside: d.inside,
                                slot: d.slot,
                                fever: d.fever,
                                reference: row.references[note],
                                rank,
                            },
                            hits: ActionHits::event(note, frame, options.inline_event_hits),
                        });
                        stats.graph_actions += 1;
                        actions.insert((note, rank), a);
                        a
                    };
                    edges.push(Edge {
                        from,
                        to,
                        action: Some(action),
                    });
                }
            }
            if !push(graph, next.len(), edges, options, stats) {
                return None;
            }
            std::mem::swap(&mut keys, &mut next);
        }
        Some(keys)
    }

    /// A successful hit increases the consumed count by exactly one. Timestamp
    /// forgetting and binding updates never undo a consumed mark.
    fn touch_key(
        &self, row: &Row, frame: u32, note: usize, active: &[usize], settled: &[bool],
        expired: usize, key: &Key,
    ) -> Option<(Key, usize)> {
        let bit = active.iter().position(|&i| i == note).unwrap();
        if key.marks[bit] != 0 || self.blockers(note, frame).iter().any(|&i| {
            !settled[i] && active.iter().position(|&a| a == i)
                .is_none_or(|j| key.marks[j] == 0)
        }) { return None; }
        for &relation in &row.incident[note] {
            let r = &row.relations[relation];
            let other = if r.before == note { r.after } else { r.before };
            let stamp = active.iter().position(|&i| i == other)
                .map(|j| key.marks[j]).unwrap_or(0);
            if r.strict_event_order {
                let legal = if r.after == note { settled[other] || stamp != 0 }
                    else { !settled[other] && stamp == 0 };
                if !legal { return None; }
            }
            // One is a consumed predecessor whose fixed-gap obligations have
            // closed. Exact timestamps remain for pending time relations.
            if stamp > 1 {
                let (a, b) = if r.before == note { (frame, stamp - 1) }
                    else { (stamp - 1, frame) };
                if !r.allows_frames(a, b) { return None; }
            }
        }
        if let Some(c) = self.ends[note] {
            if key.bindings[self.connection_slots[c]] != c + 1 { return None; }
        }
        let mut hit = key.clone();
        if let Some(c) = self.ends[note] { hit.bindings[self.connection_slots[c]] = 0; }
        if let Some(c) = self.heads[note] { hit.bindings[self.connection_slots[c]] = c + 1; }
        let rank = expired + key.marks.iter().filter(|&&m| m != 0).count() + 1;
        hit.marks[bit] = if row.incident[note].is_empty() { 1 } else { frame + 1 };
        forget_closed_timestamps(row, active, settled, note, &mut hit);
        Some((hit, rank))
    }
}
