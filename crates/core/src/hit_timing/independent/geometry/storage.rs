//! Freeze cached geometry without changing IDs, edge order or action operands.
//! Construction keeps the original wide vectors and their budget accounting.
use super::{Edge, Graph};
use std::ops::Range;

mod actions;
pub(super) use actions::Actions;

pub(in crate::hit_timing::independent) enum Edges {
    Wide(Vec<Edge>),
    Compact(Box<[SmallEdge]>),
}
pub(in crate::hit_timing::independent) struct SmallEdge {
    from: u32,
    to: u32,
    // MAX is epsilon. A real action with that ID keeps the wide representation.
    action: u32,
}
impl From<Vec<Edge>> for Edges {
    fn from(edges: Vec<Edge>) -> Self { Self::Wide(edges) }
}
impl Edges {
    pub fn len(&self) -> usize {
        match self { Self::Wide(edges) => edges.len(), Self::Compact(edges) => edges.len() }
    }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub fn capacity(&self) -> usize {
        match self { Self::Wide(edges) => edges.capacity(), Self::Compact(edges) => edges.len() }
    }
    pub fn bytes(&self) -> usize {
        self.capacity() * match self {
            Self::Wide(_) => std::mem::size_of::<Edge>(),
            Self::Compact(_) => std::mem::size_of::<SmallEdge>(),
        }
    }
    pub fn get(&self, index: usize) -> Option<Edge> {
        match self {
            Self::Wide(edges) => edges.get(index).copied(),
            Self::Compact(edges) => edges.get(index).map(|e| Edge {
                from: e.from as usize, to: e.to as usize,
                action: (e.action != u32::MAX).then_some(e.action as usize),
            }),
        }
    }
    pub fn first(&self) -> Option<Edge> { self.get(0) }
    pub fn iter(&self) -> EdgeIter<'_> { EdgeIter { edges: self, range: 0..self.len() } }
    fn freeze(&mut self) {
        let Self::Wide(edges) = self else { return; };
        if edges.iter().any(|e| u32::try_from(e.from).is_err() || u32::try_from(e.to).is_err()
            || e.action.is_some_and(|a| a >= u32::MAX as usize)) { return; }
        let compact = edges.iter().map(|e| SmallEdge {
            from: e.from as u32, to: e.to as u32,
            action: e.action.map_or(u32::MAX, |a| a as u32),
        }).collect::<Vec<_>>().into_boxed_slice();
        *self = Self::Compact(compact);
    }
    #[cfg(test)]
    pub fn wide_mut(&mut self) -> &mut Vec<Edge> {
        if matches!(self, Self::Compact(_)) { *self = Self::Wide(self.iter().collect()); }
        match self { Self::Wide(edges) => edges, _ => unreachable!() }
    }
}
pub(in crate::hit_timing::independent) struct EdgeIter<'a> { edges: &'a Edges, range: Range<usize> }
impl Iterator for EdgeIter<'_> {
    type Item = Edge;
    fn next(&mut self) -> Option<Edge> { self.range.next().and_then(|i| self.edges.get(i)) }
    fn size_hint(&self) -> (usize, Option<usize>) { self.range.size_hint() }
}
impl DoubleEndedIterator for EdgeIter<'_> {
    fn next_back(&mut self) -> Option<Edge> { self.range.next_back().and_then(|i| self.edges.get(i)) }
}
impl ExactSizeIterator for EdgeIter<'_> {}
impl<'a> IntoIterator for &'a Edges {
    type Item = Edge;
    type IntoIter = EdgeIter<'a>;
    fn into_iter(self) -> Self::IntoIter { self.iter() }
}

impl Graph {
    pub(in crate::hit_timing::independent) fn freeze(&mut self) {
        if std::env::var("BANDORI_TIMING_DISABLE_COMPACT_GEOMETRY").as_deref() == Ok("1") { return; }
        self.actions.freeze();
        for layer in &mut self.layers { layer.edges.freeze(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edge_freeze_preserves_order_epsilon_and_checked_overflow_fallback() {
        let original = vec![Edge { from: 3, to: 1, action: Some(7) },
            Edge { from: 0, to: 2, action: None },
            Edge { from: u32::MAX as usize, to: 0, action: Some(u32::MAX as usize - 1) }];
        let signature = |e: Edge| (e.from, e.to, e.action);
        let mut edges: Edges = original.clone().into();
        let before = edges.bytes();
        edges.freeze();
        assert!(matches!(edges, Edges::Compact(_)));
        assert_eq!(edges.iter().map(signature).collect::<Vec<_>>(), original.iter().copied().map(signature).collect::<Vec<_>>());
        assert!(edges.bytes() < before);
        assert_eq!(edges.iter().rev().map(signature).collect::<Vec<_>>(), original.into_iter().rev().map(signature).collect::<Vec<_>>());
        assert!(edges.get(edges.len()).is_none());
        let mut overflow = vec![Edge { from: 0, to: 0, action: Some(u32::MAX as usize) }];
        if usize::BITS > 32 { overflow.push(Edge { from: usize::MAX, to: 0, action: None }); }
        for edge in overflow {
            let mut edges: Edges = vec![edge].into();
            edges.freeze();
            assert!(matches!(edges, Edges::Wide(_)));
            assert_eq!(signature(edges.first().unwrap()), signature(edge));
        }
    }
}
