//! A contiguous, checked u32 view of the same native value graph. The full
//! usize graph remains the ordered witness; no value state crosses queries.
use super::*;
use std::sync::Arc;
mod kernel;
mod arena;
mod idle;
pub(in crate::hit_timing::independent) fn evaluate<const RECORD: bool>(graph: &Graph,
    layout: &Layout, costs: &mut impl machine::ActionCosts, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    if !costs.value_controls().enabled(Switch::ValueArena) {
        return kernel::evaluate::<RECORD>(graph,layout,costs,stats);
    }
    evaluate_with_limit::<RECORD>(graph,layout,costs,stats,arena::LIMIT)
}
fn evaluate_with_limit<const RECORD: bool>(graph: &Graph, layout: &Layout,
    costs: &mut impl machine::ActionCosts, stats: &mut WindowStats, limit: usize)
    -> Result<Option<Option<Value>>, ReplayError> {
    let before=RECORD.then(||stats.clone());
    if let Some(answer)=arena::evaluate::<RECORD>(graph,layout,costs,stats,limit)? {
        return Ok(Some(answer));
    }
    // Handles are immutable/full-state IDs. After capacity exhaustion the
    // original traversal reruns at the same power; no path is discarded.
    if let Some(before)=before {*stats=before;}
    let answer=kernel::evaluate::<RECORD>(graph,layout,costs,stats);
    if RECORD {
        stats.native_blocks.get_or_insert_with(NativeBlockStats::default)
            .value_arena_budget_fallbacks += 1;
    }
    answer
}
#[cfg(test)]
pub(in crate::hit_timing::independent) fn evaluate_original<const RECORD: bool>(graph: &Graph,
    layout: &Layout, costs: &mut impl machine::ActionCosts, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    kernel::evaluate::<RECORD>(graph,layout,costs,stats)
}
#[cfg(test)]
pub(in crate::hit_timing::independent) fn evaluate_arena_for_test(graph: &Graph,
    layout: &Layout, costs: &mut impl machine::ActionCosts, stats: &mut WindowStats, limit: usize)
    -> Result<Option<Option<Value>>, ReplayError> {
    let before=stats.clone();
    if let Some(answer)=arena::evaluate_controlled::<true>(graph,layout,costs,stats,limit,false)? {
        return Ok(Some(answer));
    }
    *stats=before;
    let answer=kernel::evaluate::<true>(graph,layout,costs,stats);
    stats.native_blocks.get_or_insert_with(NativeBlockStats::default).value_arena_budget_fallbacks+=1;
    answer
}
#[cfg(test)]
pub(in crate::hit_timing::independent) fn evaluate_partial_for_test(graph:&Graph,layout:&Layout,
    costs:&mut impl machine::ActionCosts,stats:&mut WindowStats,enabled:bool)
    -> Result<Option<Option<Value>>,ReplayError> {
    arena::evaluate_controlled::<true>(graph,layout,costs,stats,arena::LIMIT,enabled)
}
#[cfg(test)]
pub(in crate::hit_timing::independent) fn evaluate_idle_for_test(graph:&Graph,layout:&Layout,
    costs:&mut impl machine::ActionCosts,stats:&mut WindowStats,enabled:bool)
    -> Result<Option<Option<Value>>,ReplayError> {
    arena::evaluate_with_idle::<true>(graph,layout,costs,stats,arena::LIMIT,true,enabled)
}
#[cfg(test)]
pub(in crate::hit_timing::independent) fn attempt_arena_for_test(graph: &Graph,
    layout: &Layout,costs:&mut impl machine::ActionCosts,stats:&mut WindowStats)
    -> Result<Option<Option<Value>>,ReplayError> {
    arena::evaluate::<true>(graph,layout,costs,stats,arena::initial_limit())
}
#[cfg(test)]
pub(in crate::hit_timing::independent) fn arena_initial_limit_for_test() -> usize {
    arena::initial_limit()
}
const MAX_BYTES: usize = 2 * 1024 * 1024;
const NONE: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct SmallEdge { from: u32, to: u32, action: u32 }
impl SmallEdge {
    #[inline(always)]
    fn action(self) -> Option<usize> { (self.action != NONE).then_some(self.action as usize) }
}
#[derive(Clone, Copy)]
struct Span { count: u32, first: u32, last: u32 }
struct LayerView<'a> { count: usize, edges: &'a [SmallEdge] }
pub(in crate::hit_timing::independent) struct Layout {
    spans: Vec<Span>, edges: Vec<SmallEdge>,
    touches: Vec<geometry::TouchDag>, temporal: Vec<geometry::TemporalBlock>,
    idle_nodes: Vec<Vec<usize>>,
}
impl Layout {
    pub(in crate::hit_timing::independent) fn new(layers: &[geometry::Layer],
        touches: &[geometry::TouchDag], temporal: &[geometry::TemporalBlock]) -> Option<Arc<Self>> {
        Self::with_limit(layers, touches, temporal, MAX_BYTES)
    }
    pub(in crate::hit_timing::independent) fn with_limit(layers: &[geometry::Layer], touches: &[geometry::TouchDag],
        temporal: &[geometry::TemporalBlock], limit: usize) -> Option<Arc<Self>> {
        if touches.iter().any(|t| t.incremental.is_some()) { return None; }
        let count: usize = layers.iter().map(|l| l.edges.len()).sum();
        u32::try_from(count).ok()?;
        let bytes = std::mem::size_of::<Self>() + layers.len() * std::mem::size_of::<Span>()
            + count * std::mem::size_of::<SmallEdge>()
            + touches.len() * std::mem::size_of::<geometry::TouchDag>()
            + touches.iter().map(|t| t.inputs.len() * std::mem::size_of::<usize>()).sum::<usize>()
            + temporal.len() * std::mem::size_of::<geometry::TemporalBlock>()
            + temporal.iter().map(|t| t.permutation.len() * std::mem::size_of::<usize>()).sum::<usize>();
        if bytes > limit { return None; }
        let mut spans = Vec::with_capacity(layers.len());
        let mut edges = Vec::with_capacity(count);
        for layer in layers {
            let first = edges.len() as u32;
            for edge in &layer.edges {
                let action = match edge.action {
                    Some(a) => { let a = u32::try_from(a).ok()?; if a == NONE { return None; } a },
                    None => NONE,
                };
                edges.push(SmallEdge { from: u32::try_from(edge.from).ok()?,
                    to: u32::try_from(edge.to).ok()?, action });
            }
            spans.push(Span { count: u32::try_from(layer.count).ok()?, first, last: edges.len() as u32 });
        }
        let idle_nodes=temporal.iter().map(|block|idle::nodes(layers,touches,block)).collect();
        let result = Arc::new(Self { spans, edges, idle_nodes,
            touches: touches.iter().map(|t| geometry::TouchDag {
                layer: t.layer, inputs: t.inputs.clone(), incremental: None }).collect(),
            temporal: temporal.iter().map(|t| geometry::TemporalBlock {
                start: t.start, after_first: t.after_first, resume: t.resume,
                first_frame: t.first_frame, last_frame: t.last_frame, count: t.count,
                permutation: t.permutation.clone() }).collect(),
        });
        (result.bytes() <= limit).then_some(result)
    }
    #[inline(always)]
    fn layer(&self, index: usize) -> LayerView<'_> {
        let span = self.spans[index];
        LayerView { count: span.count as usize, edges: &self.edges[span.first as usize..span.last as usize] }
    }
    pub(in crate::hit_timing::independent) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.spans.capacity() * std::mem::size_of::<Span>()
            + self.edges.capacity() * std::mem::size_of::<SmallEdge>()
            + self.touches.capacity() * std::mem::size_of::<geometry::TouchDag>()
            + self.touches.iter().map(|t| t.inputs.capacity() * std::mem::size_of::<usize>()).sum::<usize>()
            + self.temporal.capacity() * std::mem::size_of::<geometry::TemporalBlock>()
            + self.temporal.iter().map(|t| t.permutation.capacity() * std::mem::size_of::<usize>()).sum::<usize>()
            + self.idle_nodes.capacity()*std::mem::size_of::<Vec<usize>>()
            + self.idle_nodes.iter().map(|v|v.capacity()*std::mem::size_of::<usize>()).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_value_graph_rejects_unrepresentable_indices_and_action_sentinel() {
        assert_eq!(std::mem::size_of::<SmallEdge>(), 12);
        assert_eq!(std::mem::size_of::<Span>(), 12);
        for (count, from, to, action) in [
            (1, 0, 0, Some(u32::MAX as usize)),
            (u32::MAX as usize + 1, 0, 0, None),
            (1, u32::MAX as usize + 1, 0, None),
            (1, 0, u32::MAX as usize + 1, None),
        ] {
            let layers = [geometry::Layer { count, edges: vec![geometry::Edge { from, to, action }].into() }];
            assert!(Layout::new(&layers, &[], &[]).is_none());
        }
    }
}
