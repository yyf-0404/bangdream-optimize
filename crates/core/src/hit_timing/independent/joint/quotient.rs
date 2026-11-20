//! Minimize native value geometry by identical labeled continuations. Classes
//! remain in their original layer; clocks and scores never enter this proof.
//! The original action array and full witness graph are retained unchanged.
use super::*;
use geometry::{Edge, Layer, TemporalBlock, TouchDag};
mod signatures;
use signatures::Key;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const EPSILON: usize = usize::MAX;

pub(in crate::hit_timing::independent) struct Layout {
    pub layers: Vec<Layer>,
    pub touches: Vec<TouchDag>,
    pub temporal: Vec<TemporalBlock>,
}

impl Layout {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.layers.capacity() * std::mem::size_of::<Layer>()
            + self.layers.iter().map(|l| l.edges.bytes()).sum::<usize>()
            + self.touches.capacity() * std::mem::size_of::<TouchDag>()
            + self.touches.iter().map(|t| t.inputs.capacity() * std::mem::size_of::<usize>()).sum::<usize>()
            + self.temporal.capacity() * std::mem::size_of::<TemporalBlock>()
            + self.temporal.iter().map(|t| t.permutation.capacity() * std::mem::size_of::<usize>()).sum::<usize>()
    }
    pub(super) fn new(input: &ReplayInput, graph: &Graph, prepared: &PreparedActions) -> Option<Arc<Self>> {
        Self::new_controlled(input, graph, prepared,
            std::env::var("BANDORI_TIMING_DISABLE_SMALL_QUOTIENT_SIGNATURES").as_deref() != Ok("1"))
    }
    fn new_controlled(input: &ReplayInput, graph: &Graph, prepared: &PreparedActions,
        fast: bool) -> Option<Arc<Self>> {
        if fast { Self::build::<signatures::Inline, true>(input, graph, prepared) }
        else { Self::build::<Vec<signatures::Atom>, false>(input, graph, prepared) }
    }
    fn build<S: Key, const FAST: bool>(input: &ReplayInput, graph: &Graph,
        prepared: &PreparedActions) -> Option<Arc<Self>> {
        if std::env::var("BANDORI_TIMING_DISABLE_NATIVE_QUOTIENT").as_deref() == Ok("1")
            || graph.layers.is_empty() || graph.touches.iter().any(|t| t.incremental.is_some()) {
            return None;
        }
        // The mapped edge Vecs retain one allocation slot per original edge,
        // even after deduplication. Skip layouts that cannot be retained.
        if std::env::var("BANDORI_TIMING_DISABLE_QUOTIENT_ADMISSION").as_deref() != Ok("1") {
            let minimum = graph.layers.len().saturating_mul(std::mem::size_of::<Layer>())
                .saturating_add(graph.layers.iter().map(|l| l.edges.len()).sum::<usize>()
                    .saturating_mul(std::mem::size_of::<Edge>()));
            if minimum > MAX_BYTES { return None; }
        }
        // An ordinary native action observes only the exact frame, raw grade
        // and native Combo/Fever band. Triggers also observe their skill slot;
        // outside runs keep their original identity and aggregate operands.
        let mut labels = HashMap::new();
        let mut representatives = Vec::with_capacity(graph.actions.len());
        for (i, action) in graph.actions.iter().enumerate() {
            let key = match action.hits {
                geometry::ActionHits::Run(_) => [1, i, 0, 0, 0],
                geometry::ActionHits::Event(_) => {
                    let (note, frame) = action.hits[0];
                    let p = prepared.actions[i];
                    [0, frame as usize, p.raw as usize, p.band,
                        input.notes[note].trigger.map_or(0, |s| s + 1)]
                }
            };
            representatives.push(*labels.entry(key).or_insert(i));
        }
        let action_label = |a: Option<usize>| a.map_or(EPSILON, |i| representatives[i]);
        let mut touch_at = vec![None; graph.layers.len()];
        for (i, t) in graph.touches.iter().enumerate() { touch_at[t.layer] = Some(i); }
        let mut maps: Vec<Vec<usize>> = (0..graph.layers.len()).map(|_| vec![]).collect();
        let mut counts = vec![0; graph.layers.len()];
        // Every original final vertex is accepted by the value evaluator.
        let mut future = vec![S::default(); graph.layers.last()?.count];
        for signature in &mut future { signature.add((EPSILON, 2, 0)); }
        let mut classes = HashMap::<S, usize>::new();
        for index in (0..graph.layers.len()).rev() {
            let layer = &graph.layers[index];
            if FAST { classes.clear(); } else { classes = HashMap::new(); }
            let mut map = vec![0; layer.count];
            let mut end = layer.edges.len();
            // A layer with one original vertex has exactly one class, whatever
            // its continuation. No signature sorting or hash table is needed.
            if FAST && layer.count == 1 {
                if touch_at[index].is_some() { debug_assert!(layer.edges.is_empty()); }
            } else { for node in (0..layer.count).rev() {
                let mut signature = std::mem::take(&mut future[node]);
                if touch_at[index].is_some() {
                    // Touch edges stay inside this layer and go to larger IDs.
                    while end > 0 && layer.edges.get(end - 1).unwrap().from == node {
                        end -= 1;
                        let edge = layer.edges.get(end).unwrap();
                        signature.add((action_label(edge.action), 1, map[edge.to]));
                    }
                }
                signature.normalize();
                let next = classes.len();
                map[node] = *classes.entry(signature).or_insert(next);
            } }
            if touch_at[index].is_some() && !(FAST && layer.count == 1) { debug_assert_eq!(end, 0); }
            let count = if FAST && layer.count == 1 { 1 } else { classes.len() };
            // Reverse discovery order preserves from < to within touch DAGs.
            for id in &mut map { *id = count - 1 - *id; }
            counts[index] = count;
            let previous = if index == 0 { 1 } else { graph.layers[index - 1].count };
            if FAST {
                // Only the temporary continuation buffer is reused. The maps
                // retained for every original layer keep their own storage.
                future.clear();
                future.resize(previous, S::default());
            } else { future = vec![S::default(); previous]; }
            // Its sole predecessor vertex also needs no continuation key.
            if !(FAST && previous == 1) { if let Some(t) = touch_at[index] {
                for (from, &to) in graph.touches[t].inputs.iter().enumerate() {
                    future[from].add((EPSILON, 0, map[to]));
                }
            } else {
                for edge in &layer.edges {
                    future[edge.from].add((action_label(edge.action), 0, map[edge.to]));
                }
            } }
            maps[index] = map;
        }
        let mut result = Self { layers: Vec::with_capacity(graph.layers.len()), touches: vec![], temporal: vec![] };
        let mut saved = false;
        for (index, layer) in graph.layers.iter().enumerate() {
            let previous: &[usize] = if index == 0 { &[0] } else { &maps[index - 1] };
            let touch = touch_at[index];
            let mut edges: Vec<_> = layer.edges.iter().map(|e| Edge {
                from: if touch.is_some() { maps[index][e.from] } else { previous[e.from] },
                to: maps[index][e.to], action: e.action.map(|i| representatives[i]),
            }).collect();
            edges.sort_unstable_by_key(|e| (e.from, e.to, e.action));
            edges.dedup_by(|a, b| a.from == b.from && a.to == b.to && a.action == b.action);
            saved |= edges.len() < layer.edges.len() || counts[index] < layer.count;
            if let Some(t) = touch {
                debug_assert!(edges.iter().all(|e| e.from < e.to));
                let count = if index == 0 { 1 } else { counts[index - 1] };
                let mut inputs = vec![usize::MAX; count];
                for (from, &to) in graph.touches[t].inputs.iter().enumerate() {
                    let old = &mut inputs[previous[from]];
                    let next = maps[index][to];
                    if *old != usize::MAX && *old != next { return None; }
                    *old = next;
                }
                debug_assert!(inputs.iter().all(|&i| i != usize::MAX));
                result.touches.push(TouchDag { layer: index, inputs, incremental: None });
            }
            result.layers.push(Layer { count: counts[index], edges: edges.into() });
        }
        if !saved { return None; }
        for block in &graph.temporal {
            let a = &maps[block.after_first - 1];
            let b = &maps[block.resume - 1];
            let count = counts[block.after_first - 1];
            if counts[block.resume - 1] != count { continue; }
            let mut permutation = vec![usize::MAX; count];
            let mut compatible = true;
            for from in 0..block.count {
                let to = if block.permutation.is_empty() { from } else { block.permutation[from] };
                let (source, target) = (a[from], b[to]);
                if permutation[source] != usize::MAX && permutation[source] != target {
                    compatible = false; break;
                }
                permutation[source] = target;
            }
            let mut sorted = permutation.clone(); sorted.sort_unstable();
            if !compatible || !sorted.iter().enumerate().all(|(i, &j)| i == j) { continue; }
            if permutation.iter().enumerate().all(|(i, &j)| i == j) { permutation.clear(); }
            result.temporal.push(TemporalBlock { start: block.start, after_first: block.after_first,
                resume: block.resume, first_frame: block.first_frame, last_frame: block.last_frame,
                count, permutation });
        }
        (result.bytes() <= MAX_BYTES).then(|| Arc::new(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::{ReplayNote, NoteWindow, ScoreEffect};
    fn fixture(trigger: bool) -> (ReplayInput, Graph) {
        let input = ReplayInput { power: 300040.75, play_level: 27, fps: 120,
            notes: (0..2).map(|i| ReplayNote { time: 1., trigger: (trigger && i == 1).then_some(0),
                window: NoteWindow::Standard }).collect(), hit_frames: vec![120; 2],
            skills: vec![ReplaySkill { duration: 0.1, correction: None,
                score: ScoreEffect::Plain { percent: 110. } }], initial_combo: 0,
            medley_combo: false, fever_frames: None, constraints: vec![], hit_order: vec![], native_time_routing: None };
        let action = |note, rank| Action { factor: Factor { perfect: true, inside: false,
            slot: 0, fever: false, reference: 1., rank }, hits: geometry::ActionHits::Event([(note,120)]) };
        let graph = Graph { layers: vec![Layer { count: 4, edges: vec![
            Edge { from: 0, to: 1, action: Some(0) }, Edge { from: 0, to: 2, action: Some(1) },
            Edge { from: 1, to: 3, action: Some(2) }, Edge { from: 2, to: 3, action: Some(3) }].into() },
            Layer { count: 1, edges: vec![Edge { from: 3, to: 0, action: None }].into() }],
            actions: vec![action(0,1),action(1,1),action(1,2),action(0,2)].into(), note_count: 2,
            edge_count: 5, temporal: vec![], touches: vec![TouchDag { layer: 0, inputs: vec![0], incremental: None }] };
        (input, graph)
    }
    #[test]
    fn small_keys_and_single_vertex_layers_preserve_every_quotient_coordinate() {
        for effect in [ScoreEffect::Plain { percent: 110. },
            ScoreEffect::Continuous { high: 150., low: 80. },
            ScoreEffect::Crescendo { percent: 100., step: 0.5, cap: 150 }] {
            for duration in [0.05, 0.3, 1.1] {
                for correction in [None, Some(JudgementCorrection::Bad)] {
                    let input = ReplayInput { power: 300040.75, play_level: 27, fps: 120,
                        notes: (0..12).map(|i| ReplayNote {
                            time: 1. + (i / 2) as f64 * 0.04,
                            trigger: (i % 2 == 0).then_some(i / 2),
                            window: NoteWindow::Standard,
                        }).collect(), hit_frames: (0..12).map(|i| 120 + (i / 2) * 5).collect(),
                        skills: vec![ReplaySkill { duration, correction, score: effect.clone() }; 6],
                        initial_combo: 699, medley_combo: true, fever_frames: Some([125, 139]),
                        constraints: vec![], hit_order: vec![], native_time_routing: None };
                    let model = JointTimeline::new(&input).unwrap();
                    let a = Layout::new_controlled(&input, &model.graph, &model.prepared_actions, true);
                    let b = Layout::new_controlled(&input, &model.graph, &model.prepared_actions, false);
                    match (a, b) {
                        (None, None) => {},
                        (Some(a), Some(b)) => {
                            assert_eq!(a.bytes(), b.bytes());
                            assert_eq!(a.layers.len(), b.layers.len());
                            for (a, b) in a.layers.iter().zip(&b.layers) {
                                assert_eq!(a.count, b.count);
                                assert_eq!(a.edges.iter().map(|e| (e.from, e.to, e.action)).collect::<Vec<_>>(),
                                    b.edges.iter().map(|e| (e.from, e.to, e.action)).collect::<Vec<_>>());
                            }
                            assert_eq!(a.touches.iter().map(|t| (t.layer, &t.inputs)).collect::<Vec<_>>(),
                                b.touches.iter().map(|t| (t.layer, &t.inputs)).collect::<Vec<_>>());
                            assert_eq!(a.temporal.iter().map(|t| (t.start, t.after_first, t.resume,
                                t.first_frame, t.last_frame, t.count, &t.permutation)).collect::<Vec<_>>(),
                                b.temporal.iter().map(|t| (t.start, t.after_first, t.resume,
                                t.first_frame, t.last_frame, t.count, &t.permutation)).collect::<Vec<_>>());
                        },
                        _ => panic!("quotient admission differs"),
                    }
                }
            }
        }
    }
    #[test]
    fn symmetric_native_hits_merge_but_trigger_identity_keeps_paths_distinct() {
        let (input,graph) = fixture(false);
        let prepared = PreparedActions::new(&input,&graph).unwrap();
        let layout = Layout::new(&input,&graph,&prepared).unwrap();
        assert_eq!(layout.layers[0].count,3);
        assert_eq!(layout.layers[0].edges.len(),2);
        assert!(layout.bytes() <= MAX_BYTES);
        let (input,graph) = fixture(true);
        let prepared = PreparedActions::new(&input,&graph).unwrap();
        assert!(Layout::new(&input,&graph,&prepared).is_none());
    }
}
