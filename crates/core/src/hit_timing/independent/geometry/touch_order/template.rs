//! A fully closed ordinary DAG has the same ordered keys at entry and exit.
//! Retain one bounded, build-local shape; scoring still uses each actual frame.
use super::*;

const MAX_BYTES: usize = 256 * 1024;
pub(in crate::hit_timing::independent::geometry) struct Cache {
    enabled: bool,
    template: Option<Template>,
}
struct Template {
    frame: u32,
    expired: usize,
    active: Vec<usize>,
    events: Vec<(usize, ActionInterval)>,
    blockers: Vec<Vec<usize>>,
    references: Vec<u64>,
    keys: Vec<Key>,
    edges: Vec<Edge>,
    edge_capacity: usize,
    actions: Vec<(Factor, usize)>,
}
impl Cache {
    pub(in crate::hit_timing::independent::geometry) fn new(enabled: bool) -> Self {
        Self { enabled, template: None }
    }
    pub(super) fn append(&mut self, prepared: &Prepared, row: &Row, frame: u32,
        events: &[(usize, ActionInterval)], active: &[usize], expired: usize, keys: &[Key],
        graph: &mut Graph, options: WindowOptions, stats: &mut WindowStats) -> bool {
        if !self.enabled || options.incremental_touch { return false; }
        let Some(t) = self.template.as_mut() else { return false; };
        if t.frame.checked_add(1) != Some(frame) || t.expired != expired
            || t.active != active || t.events != events || t.keys != keys
            || events.iter().enumerate().any(|(i, &(note, _))|
                row.references[note].to_bits() != t.references[i]
                || prepared.blockers(note, frame) != t.blockers[i]) {
            return false;
        }
        // Keep original budget failure diagnostics: an insufficient budget
        // takes the normal constructor and its original partial traversal.
        if keys.len() > options.max_states
            || graph.edge_count.saturating_add(t.edges.len()).saturating_add(keys.len()) > options.max_graph_edges {
            return false;
        }
        let base = graph.actions.len();
        // Match the original Vec growth, action IDs and operand encounter order.
        for &(factor, note) in &t.actions {
            graph.actions.push(Action { factor,
                hits: ActionHits::event(note, frame, options.inline_event_hits) });
        }
        let mut edges = Vec::with_capacity(t.edge_capacity);
        edges.extend(t.edges.iter().map(|e| Edge { from: e.from, to: e.to,
            action: e.action.map(|a| base + a) }));
        let layer = graph.layers.len();
        let count = edges.len();
        let pushed = push(graph, keys.len(), edges, options, stats);
        debug_assert!(pushed);
        graph.edge_count += keys.len();
        stats.graph_edges += keys.len();
        stats.graph_actions += t.actions.len();
        stats.graph_action_reuses += count - t.actions.len();
        stats.graph_touch_dag_frames += 1;
        stats.graph_touch_dag_edges += count;
        graph.touches.push(TouchDag { layer, inputs: (0..keys.len()).collect(), incremental: None });
        t.frame = frame;
        true
    }
    pub(super) fn capture(&mut self, prepared: &Prepared, row: &Row, frame: u32,
        events: &[(usize, ActionInterval)], active: &[usize], expired: usize, keys: &[Key],
        graph: &Graph, start: (usize, usize), options: WindowOptions) {
        if !self.enabled { return; }
        self.template = None;
        let Some(touch) = graph.touches.last().filter(|t| t.layer == start.0) else { return; };
        // The first closure can discover and reorder vertices. Capture only a
        // later closure that added no vertex and kept every input ID unchanged.
        if options.incremental_touch || touch.inputs.len() != keys.len()
            || !touch.inputs.iter().enumerate().all(|(i, &j)| i == j)
            || events.iter().any(|&(note, _)| prepared.input.notes[note].trigger.is_some()
                || !row.incident[note].is_empty() || prepared.heads[note].is_some()
                || prepared.ends[note].is_some()) { return; }
        let layer = &graph.layers[start.0];
        let actions = &graph.actions.wide()[start.1..];
        let bytes = std::mem::size_of::<Template>()
            + keys.len() * std::mem::size_of::<Key>()
            + keys.iter().map(|k| k.marks.len() * 4 + k.bindings.len() * std::mem::size_of::<usize>()).sum::<usize>()
            + layer.edges.len() * std::mem::size_of::<Edge>()
            + actions.len() * std::mem::size_of::<(Factor, usize)>()
            + active.len() * std::mem::size_of::<usize>()
            + events.len() * (std::mem::size_of::<(usize, ActionInterval)>()
                + std::mem::size_of::<Vec<usize>>() + 8)
            + events.iter().map(|&(note, _)| prepared.blockers(note, frame).len()
                * std::mem::size_of::<usize>()).sum::<usize>();
        if bytes > MAX_BYTES { return; }
        self.template = Some(Template { frame, expired, active: active.to_vec(),
            events: events.to_vec(),
            blockers: events.iter().map(|&(note, _)| prepared.blockers(note, frame).to_vec()).collect(),
            references: events.iter().map(|&(note, _)| row.references[note].to_bits()).collect(),
            keys: keys.to_vec(), edge_capacity: layer.edges.capacity(),
            edges: layer.edges.iter().map(|e| Edge { from: e.from, to: e.to,
                action: e.action.map(|a| a - start.1) }).collect(),
            actions: actions.iter().map(|a| (a.factor, a.hits[0].0)).collect(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::{ReplayNote, ReplaySkill};

    fn graph() -> Graph {
        Graph { layers: vec![], actions: vec![].into(), note_count: 4, edge_count: 0,
            temporal: vec![], touches: vec![] }
    }

    #[test]
    fn closed_frame_template_really_reuses_the_shape_and_preserves_actual_frames() {
        let times = [0.1, 0.5, 0.5, 0.5];
        let input = ReplayInput {
            power: 300040.75, play_level: 27, fps: 120, initial_combo: 18,
            medley_combo: false, fever_frames: None, constraints: vec![],
            notes: times.into_iter().enumerate().map(|(i, time)| ReplayNote {
                time, trigger: (i == 0).then_some(0), window: NoteWindow::Standard,
            }).collect(),
            hit_order: vec![], native_time_routing: None, hit_frames: vec![12, 60, 60, 60],
            skills: vec![ReplaySkill { duration: 0.1, correction: None,
                score: ScoreEffect::Plain { percent: 25. } }],
        };
        let prepared = Prepared::new(&input).unwrap();
        let row = prepared.joint_row();
        let active = [1, 2, 3];
        let settled = [true, false, false, false];
        let events: Vec<_> = active.iter().map(|&note| (note, ActionInterval {
            first: 59, last: 63, perfect: true, inside: false, slot: 0, fever: false,
        })).collect();
        let options = WindowOptions { max_cached_component_edges: 0, touch_dag: true,
            reuse_graph_storage: true, inline_event_hits: true, ..WindowOptions::default() };
        let seed = vec![Key { marks: vec![0; 3], bindings: vec![0; prepared.binding_slots] }];
        let mut old = graph();
        let mut new = graph();
        let mut old_stats = WindowStats::default();
        let mut new_stats = WindowStats::default();
        let mut cache = Cache::new(true);
        let mut keys = seed.clone();
        let mut old_keys = seed;
        for frame in [60, 61] {
            old_keys = prepared.compile_touch_dag(&row, frame, &events, &active, &settled,
                1, old_keys, &mut old, options, &mut old_stats).unwrap();
            keys = prepared.compile_touch_frame(&row, frame, &events, &active, &settled,
                1, keys, &mut new, options, &mut new_stats, &mut cache).unwrap();
        }
        assert_eq!(keys.len(), 8);
        assert!(cache.template.is_some());
        // Call the reuse operation itself: a silent fallback cannot pass this test.
        assert!(cache.append(&prepared, &row, 62, &events, &active, 1, &keys,
            &mut new, options, &mut new_stats));
        old_keys = prepared.compile_touch_dag(&row, 62, &events, &active, &settled,
            1, old_keys, &mut old, options, &mut old_stats).unwrap();
        assert!(keys == old_keys);
        assert_eq!(old.edge_count, new.edge_count);
        assert_eq!(old.actions.capacity(), new.actions.capacity());
        assert_eq!(old.layers.len(), new.layers.len());
        for (a, b) in old.layers.iter().zip(&new.layers) {
            assert_eq!(a.count, b.count);
            assert_eq!(a.edges.capacity(), b.edges.capacity());
            assert_eq!(a.edges.iter().map(|e| (e.from, e.to, e.action)).collect::<Vec<_>>(),
                b.edges.iter().map(|e| (e.from, e.to, e.action)).collect::<Vec<_>>());
        }
        for (a, b) in old.actions.iter().zip(&new.actions) {
            assert_eq!(&*a.hits, &*b.hits);
            assert_eq!((a.factor.rank, a.factor.reference.to_bits()),
                (b.factor.rank, b.factor.reference.to_bits()));
        }
        assert_eq!(old.actions.len(), new.actions.len());
        assert_eq!(serde_json::to_value(old_stats).unwrap(), serde_json::to_value(new_stats).unwrap());
    }
}
