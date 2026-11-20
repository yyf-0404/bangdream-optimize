//! Exact templates for closed, single-entry/single-exit ambiguous components.
//! Only the expensive frontier construction is reused; every original layer
//! and its budgets are replayed in order, and score evaluation is unchanged.
use super::*;
use std::sync::Arc;

#[derive(Hash, PartialEq, Eq)]
pub(super) struct ComponentKey {
    notes: Vec<(usize, Vec<ActionInterval>, u64)>,
    expired: usize,
    relations: Vec<Relation>,
    bindings: Vec<usize>,
    settled: Vec<u64>,
    reuse_storage: bool,
    inline_hits: bool,
    optimize_touch_order: bool,
}
impl ComponentKey {
    pub(super) fn new(
        prepared: &Prepared,
        row: &Row,
        notes: &[usize],
        entry: &Key,
        settled: &[bool],
        expired: usize,
        options: WindowOptions,
    ) -> Self {
        debug_assert!(entry.marks.is_empty());
        let mut settled_bits = vec![];
        // With native routing, previously consumed outside roots can affect
        // nearest-target legality. Keep the full exact history, not a digest.
        if prepared.input.native_time_routing.is_some() {
            settled_bits.resize(settled.len().div_ceil(64), 0);
            for (i, &done) in settled.iter().enumerate() {
                if done {
                    settled_bits[i / 64] |= 1_u64 << (i % 64);
                }
            }
        }
        Self {
            notes: notes
                .iter()
                .map(|&i| (i, row.domains[i].clone(), row.references[i].to_bits()))
                .collect(),
            expired,
            relations: row
                .relations
                .iter()
                .filter(|r| notes.contains(&r.before) || notes.contains(&r.after))
                .cloned()
                .collect(),
            bindings: entry.bindings.clone(),
            settled: settled_bits,
            reuse_storage: options.reuse_graph_storage,
            inline_hits: options.inline_event_hits,
            optimize_touch_order: options.optimize_touch_order,
        }
    }
}

struct CachedLayer {
    count: usize,
    edges: Vec<Edge>,
    action_end: usize,
}
pub(super) struct CachedComponent {
    layers: Vec<CachedLayer>,
    actions: Vec<Action>,
}
impl CachedComponent {
    fn from_graph(graph: &Graph, first_layer: usize, first_action: usize) -> Self {
        let mut action_end = 0;
        let layers = graph.layers[first_layer..]
            .iter()
            .map(|layer| {
                let edges = layer
                    .edges
                    .iter()
                    .map(|e| {
                        let action = e.action.map(|a| {
                            let a = a - first_action;
                            action_end = action_end.max(a + 1);
                            a
                        });
                        Edge { action, ..e }
                    })
                    .collect();
                CachedLayer {
                    count: layer.count,
                    edges,
                    action_end,
                }
            })
            .collect();
        Self {
            layers,
            actions: graph.actions.wide()[first_action..].to_vec(),
        }
    }
    pub(super) fn append(
        &self,
        graph: &mut Graph,
        options: WindowOptions,
        stats: &mut WindowStats,
    ) -> bool {
        let base = graph.actions.len();
        let mut copied = 0;
        for layer in &self.layers {
            // Materialize only actions reached before the original budget check.
            graph
                .actions
                .extend_from_slice(&self.actions[copied..layer.action_end]);
            stats.graph_actions += layer.action_end - copied;
            copied = layer.action_end;
            let edges: Vec<_> = layer
                .edges
                .iter()
                .map(|e| Edge {
                    action: e.action.map(|a| a + base),
                    ..*e
                })
                .collect();
            let edge_count = edges.len();
            if !push(graph, layer.count, edges, options, stats) {
                return false;
            }
            stats.component_replayed_edges += edge_count;
        }
        debug_assert_eq!(copied, self.actions.len());
        true
    }
}

#[derive(Default)]
pub(super) struct ComponentCache {
    entries: HashMap<ComponentKey, Arc<CachedComponent>>,
    edges: usize,
}
impl ComponentCache {
    pub(super) fn get(&self, key: &ComponentKey) -> Option<Arc<CachedComponent>> {
        self.entries.get(key).cloned()
    }
    pub(super) fn insert(
        &mut self,
        key: ComponentKey,
        graph: &Graph,
        start: (usize, usize, usize),
        limit: usize,
        stats: &mut WindowStats,
    ) {
        let edges = graph.edge_count - start.2;
        if edges == 0 || self.entries.contains_key(&key) {
            return;
        }
        if edges > limit.saturating_sub(self.edges) || self.entries.len() >= 4096 {
            stats.component_cache_skips += 1;
            return;
        }
        self.entries.insert(
            key,
            Arc::new(CachedComponent::from_graph(graph, start.0, start.1)),
        );
        self.edges += edges;
        stats.component_cache_stores += 1;
        stats.component_cached_edges = stats.component_cached_edges.max(self.edges);
    }
}
