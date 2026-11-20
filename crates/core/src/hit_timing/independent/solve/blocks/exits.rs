//! Stream a completed touch vertex directly into its epsilon successor layer.
//! No later touch can modify it: successful edges increase the vertex ID.
use super::*;

pub(super) fn following<'a>(index: usize, end: usize, layers: &'a [geometry::Layer],
    touches: &[geometry::TouchDag], temporal: &[geometry::TemporalBlock],
    interval_index: usize, skip: Option<&geometry::TemporalBlock>) -> Option<&'a geometry::Layer> {
    let next = index + 1;
    if next >= end || touches.first().is_some_and(|t| t.layer == next)
        || skip.is_some_and(|b| b.after_first == next)
        || temporal.get(interval_index).is_some_and(|b| b.start == next) { return None; }
    let layer = &layers[next];
    // Original retirement/extension layers and quotient layers are sorted by
    // source. Retain the normal evaluator for any other valid graph ordering.
    (layer.edges.iter().all(|e| e.action.is_none())
        && layer.edges.iter().zip(layer.edges.iter().skip(1)).all(|(a,b)| a.from <= b.from))
        .then_some(layer)
}

pub(super) fn emit<const RECORD: bool>(node: usize, value: Option<Block>,
    exits: &geometry::storage::Edges, cursor: &mut usize, target: &mut [Option<Block>],
    storage: &mut Storage<RECORD>, transitions: &mut u64) {
    let first = *cursor;
    while exits.get(*cursor).is_some_and(|e| e.from == node) { *cursor += 1; }
    let Some(value) = value else { return; };
    let count = *cursor - first;
    if RECORD {
        let count = (value.len() * count) as u64;
        *transitions += count;
        storage.stats.carried_entries += count;
    }
    if count > 0 {
        for index in first..*cursor - 1 {
            let edge = exits.get(index).unwrap();
            storage.merge(&mut target[edge.to], value.clone());
        }
        let last = exits.get(*cursor - 1).unwrap();
        storage.merge(&mut target[last.to], value);
    }
}
