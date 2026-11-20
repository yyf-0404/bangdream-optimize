//! Each geometry vertex expands once, ordered by the number of consumed notes.
//! A path may stop at any vertex; padding a shorter sequence adds no decision.
use super::*;

impl Prepared {
    pub(super) fn compile_touch_dag(
        &self, row: &Row, frame: u32, events: &[(usize, ActionInterval)],
        active: &[usize], settled: &[bool], expired: usize, mut keys: Vec<Key>,
        graph: &mut Graph, options: WindowOptions, stats: &mut WindowStats,
    ) -> Option<Vec<Key>> {
        let inputs = keys.len();
        let mut map: HashMap<_, _> = keys.iter().cloned().enumerate()
            .map(|(i, key)| (key, i)).collect();
        let mut actions = HashMap::new();
        let mut edges = vec![];
        let mut from = 0;
        while from < keys.len() {
            for &(note, d) in events {
                let Some((hit, rank)) = self.touch_key(
                    row, frame, note, active, settled, expired, &keys[from],
                ) else { continue; };
                let to = intern(hit, &mut keys, &mut map);
                let reused = options.reuse_graph_storage
                    .then(|| actions.get(&(note, rank))).flatten().copied();
                let action = if let Some(a) = reused {
                    stats.graph_action_reuses += 1;
                    a
                } else {
                    let a = graph.actions.len();
                    graph.actions.push(Action {
                        factor: Factor {
                            perfect: d.perfect, inside: d.inside, slot: d.slot,
                            fever: d.fever, reference: row.references[note], rank,
                        },
                        hits: ActionHits::event(note, frame, options.inline_event_hits),
                    });
                    stats.graph_actions += 1;
                    actions.insert((note, rank), a);
                    a
                };
                edges.push(Edge { from, to, action: Some(action) });
                if keys.len() > options.max_states
                    || graph.edge_count + edges.len() + inputs > options.max_graph_edges {
                    return None;
                }
            }
            from += 1;
        }
        let mut order: Vec<_> = (0..keys.len()).collect();
        order.sort_by_key(|&i| keys[i].marks.iter().filter(|&&m| m != 0).count());
        let mut ids = vec![0; keys.len()];
        for (new, &old) in order.iter().enumerate() { ids[old] = new; }
        let inputs = ids[..inputs].to_vec();
        for edge in &mut edges {
            edge.from = ids[edge.from];
            edge.to = ids[edge.to];
            debug_assert!(edge.from < edge.to);
        }
        // Stable within a source vertex: retain candidate dispatch order.
        edges.sort_by_key(|e| e.from);
        let keys: Vec<_> = order.into_iter().map(|i| keys[i].clone()).collect();
        let layer = graph.layers.len();
        let hit_edges = edges.len();
        if graph.edge_count + hit_edges + inputs.len() > options.max_graph_edges
            || !push(graph, keys.len(), edges, options, stats) { return None; }
        // Entry carries are logical edges stored compactly as destination IDs.
        graph.edge_count += inputs.len();
        stats.graph_edges += inputs.len();
        stats.graph_touch_dag_frames += 1;
        stats.graph_touch_dag_edges += hit_edges;
        graph.touches.push(TouchDag { layer, inputs, incremental: None });
        Some(keys)
    }
}
