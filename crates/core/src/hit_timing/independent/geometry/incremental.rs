//! A repeated hit is certified against the previous complete frame closure.
//! Full keys and native operands must agree; only ordinary hits without time
//! or binding writes qualify. Retirement may project a source only injectively.
use super::*;
use std::collections::HashSet;

pub(in crate::hit_timing::independent) struct Proof {
    pub previous_layer: usize,
    pub first_frame: u32,
    pub last_frame: u32,
    pub repeated: Vec<bool>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Signature {
    note: usize,
    raw: u8,
    perfect: bool,
    inside: bool,
    slot: u8,
    fever: bool,
    reference: u64,
    rank: usize,
}

#[derive(Clone)]
struct Hit {
    from: usize,
    to: usize,
    signature: Signature,
}

struct Frame {
    layer: usize,
    frame: u32,
    active: Vec<usize>,
    keys: Vec<Key>,
    hits: Vec<Hit>,
}

#[derive(Default)]
pub(super) struct Tracker {
    previous: Option<Frame>,
}

impl Tracker {
    pub(super) fn touch(&mut self, prepared: &Prepared, row: &Row, frame: u32,
        active: &[usize], keys: &[Key], graph: &mut Graph, stats: &mut WindowStats) {
        let dag = graph.touches.last().expect("incremental proofs require a touch DAG");
        let layer = &graph.layers[dag.layer];
        let signatures: Vec<_> = layer.edges.iter().map(|edge| {
            let action = &graph.actions.get(edge.action.unwrap());
            let note = action.hits[0].0;
            if prepared.input.notes[note].trigger.is_some() || !row.incident[note].is_empty()
                || prepared.heads[note].is_some() || prepared.ends[note].is_some() {
                return None;
            }
            let intervals = &prepared.intervals[note];
            let raw = intervals[intervals.partition_point(|d| d.last < frame)].raw;
            Some(Signature { note, raw: raw as u8, perfect: action.factor.perfect,
                inside: action.factor.inside, slot: action.factor.slot, fever: action.factor.fever,
                reference: action.factor.reference.to_bits(), rank: action.factor.rank })
        }).collect();
        let proof = self.previous.as_ref().and_then(|old| {
            if !active.starts_with(&old.active) || old.keys.len() != dag.inputs.len() {
                return None;
            }
            // Newly entering notes are appended as unconsumed. Check complete
            // keys rather than assuming projection or timestamp saturation.
            for (i, old_key) in old.keys.iter().enumerate() {
                let new_key = &keys[dag.inputs[i]];
                if new_key.bindings != old_key.bindings
                    || !new_key.marks.starts_with(&old_key.marks)
                    || new_key.marks[old_key.marks.len()..].iter().any(|&m| m != 0) {
                    return None;
                }
            }
            let old_hits: HashSet<_> = old.hits.iter()
                .map(|hit| (hit.from, hit.to, &hit.signature)).collect();
            let mut inputs = vec![usize::MAX; keys.len()];
            for (input, &node) in dag.inputs.iter().enumerate() { inputs[node] = input; }
            let repeated: Vec<_> = layer.edges.iter().zip(&signatures).map(|(edge, signature)| {
                signature.as_ref().is_some_and(|s| {
                    old_hits.contains(&(inputs[edge.from], inputs[edge.to], s))
                })
            }).collect();
            repeated.iter().any(|&r| r).then(|| Box::new(Proof {
                previous_layer: old.layer, first_frame: old.frame, last_frame: frame, repeated,
            }))
        });
        if let Some(proof) = &proof {
            stats.graph_incremental_frames += 1;
            stats.graph_incremental_edges += proof.repeated.iter().filter(|&&r| r).count();
        }
        let hits = layer.edges.iter().zip(signatures).filter_map(|(edge, signature)| {
            signature.map(|signature| Hit { from: edge.from, to: edge.to, signature })
        }).collect();
        self.previous = Some(Frame { layer: dag.layer, frame, active: active.to_vec(),
            keys: keys.to_vec(), hits });
        graph.touches.last_mut().unwrap().incremental = proof;
    }

    pub(super) fn retire(&mut self, edges: &[Edge], keys: &[Key]) {
        let Some(old) = &mut self.previous else { return; };
        let mut map = vec![usize::MAX; old.keys.len()];
        let mut predecessors = vec![0; keys.len()];
        for edge in edges {
            debug_assert!(edge.action.is_none());
            map[edge.from] = edge.to;
            predecessors[edge.to] += 1;
        }
        old.hits.retain_mut(|hit| {
            let (from, to) = (map[hit.from], map[hit.to]);
            if from == usize::MAX || to == usize::MAX || predecessors[from] != 1 { return false; }
            hit.from = from;
            hit.to = to;
            true
        });
        old.keys = keys.to_vec();
    }

    pub(super) fn after_frame(&mut self, active: &[usize]) {
        if let Some(old) = &mut self.previous { old.active = active.to_vec(); }
    }
}
