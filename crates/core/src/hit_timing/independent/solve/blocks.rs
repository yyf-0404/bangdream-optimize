//! Native value sets share exact state IDs and relative integer scores. Geometry
//! vertices remain separate; only immutable value payloads and ordinary value
//! operators are shared within one fresh query. Witnesses keep the scalar DP.
use super::*;
mod exits;
pub(in crate::hit_timing::independent) mod packed;
use crate::scoring::ScoringError;
use std::rc::Rc;
use std::cell::Cell;

const SLOTS: usize = 1024;
const SHAPE_BYTES: usize = 256 * 1024;
const OPERATOR_BYTES: usize = 256 * 1024;
const REDUCTION_BYTES: usize = 256 * 1024;

struct Payload {
    id: usize,
    hash: u64,
    maximum: u64,
    entries: Vec<(usize, u64)>,
    uniform: Cell<Option<Option<machine::NativeUniformState>>>,
    irreducible: Cell<bool>,
}
impl Payload {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>()
            + self.entries.capacity() * std::mem::size_of::<(usize, u64)>()
    }
    fn uniform(&self, costs: &impl machine::ActionCosts) -> Option<machine::NativeUniformState> {
        if let Some(proof) = self.uniform.get() { return proof; }
        let proof = self.entries.iter().try_fold(None, |common, &(state, _)| {
            let mut current = costs.native_uniform_state(state)?;
            if let Some(old) = common {
                let old: machine::NativeUniformState = old;
                // A mixed class still certifies the frame interval for the
                // complete vector operator; only uniform scalar shifts stop.
                if old.class != current.class { current.class = (usize::MAX, usize::MAX); }
                current.first = current.first.max(old.first);
                current.last = current.last.min(old.last);
            }
            Some(Some(current))
        }).flatten();
        self.uniform.set(Some(proof));
        proof
    }
}

#[derive(Clone)]
struct Block {
    payload: Rc<Payload>,
    offset: u64,
}
impl Block {
    fn len(&self) -> usize { self.payload.entries.len() }
    fn score(&self, relative: u64) -> u64 { self.offset + relative }
    fn maximum(&self) -> u64 { self.score(self.payload.maximum) }
    fn shifted(&self, shift: u64) -> Result<Self, ReplayError> {
        let offset = self.offset.checked_add(shift).ok_or(ScoringError::TotalScoreOverflow)?;
        offset.checked_add(self.payload.maximum).ok_or(ScoringError::TotalScoreOverflow)?;
        Ok(Self { payload: self.payload.clone(), offset })
    }
}

#[derive(Clone)]
struct Cached {
    source: usize,
    operator: (u64, u8, usize),
    output: Option<Block>,
    shift: u64,
}

struct Reduced {
    source: usize,
    output: Option<Block>,
    shift: u64,
    removed: usize,
}

struct Storage<const RECORD: bool> {
    shapes: Vec<Option<Rc<Payload>>>,
    operators: Vec<Option<Cached>>,
    reductions: Vec<Option<Reduced>>,
    shape_bytes: usize,
    operator_bytes: usize,
    reduction_bytes: usize,
    next_id: usize,
    scratch: Vec<(usize, u64)>,
    stats: NativeBlockStats,
    uniform: bool,
    static_operators: bool,
    inline_reductions: bool,
}
impl<const RECORD: bool> Storage<RECORD> {
    #[cfg(test)]
    fn new() -> Self {
        Self::with_controls(Controls::dynamic())
    }
    fn with_controls(controls: Controls) -> Self {
        Self { shapes: vec![None; SLOTS], operators: vec![None; SLOTS],
            reductions: if !controls.enabled(Switch::BlockReductions) { vec![] } else {
                    (0..SLOTS).map(|_| None).collect()
                },
            shape_bytes: 0, operator_bytes: 0, reduction_bytes: 0,
            next_id: 0, scratch: vec![],
            stats: NativeBlockStats::default(),
            uniform: controls.enabled(Switch::UniformBlocks),
            inline_reductions: controls.enabled(Switch::InlineReductions),
            static_operators: controls.enabled(Switch::StaticBlockOperators) }
    }
    fn retained(&mut self) {
        if RECORD {
        self.stats.retained_bytes = self.stats.retained_bytes.max(self.shape_bytes
            + self.operator_bytes
            + self.reduction_bytes
            + self.shapes.capacity() * std::mem::size_of::<Option<Rc<Payload>>>()
            + self.operators.capacity() * std::mem::size_of::<Option<Cached>>()
            + self.reductions.capacity() * std::mem::size_of::<Option<Reduced>>());
        }
    }
    fn recycle(&mut self, mut entries: Vec<(usize, u64)>) {
        entries.clear();
        if entries.capacity() > self.scratch.capacity() { self.scratch = entries; }
    }
    /// Input is sorted and unique by full skill state, with absolute scores.
    fn intern(&mut self, mut entries: Vec<(usize, u64)>) -> Block {
        debug_assert!(!entries.is_empty());
        debug_assert!(entries.windows(2).all(|w| w[0].0 < w[1].0));
        let offset = entries.iter().map(|e| e.1).min().unwrap();
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut maximum = 0;
        if RECORD { self.stats.normalized_entries += entries.len() as u64; }
        for (state, score) in &mut entries {
            *score -= offset;
            maximum = maximum.max(*score);
            hash = (hash ^ *state as u64).wrapping_mul(0x100_0000_01b3);
            hash = (hash ^ *score).wrapping_mul(0x100_0000_01b3);
        }
        hash ^= hash >> 32;
        let index = hash as usize & (SLOTS - 1);
        if let Some(payload) = &self.shapes[index] {
            if payload.hash == hash && payload.entries == entries {
                if RECORD { self.stats.shape_hits += 1; }
                let payload = payload.clone();
                self.recycle(entries);
                return Block { payload, offset };
            }
        }
        if RECORD { self.stats.shape_misses += 1; }
        let payload = Rc::new(Payload { id: self.next_id, hash, maximum, entries,
            uniform: Cell::new(None), irreducible: Cell::new(false) });
        self.next_id += 1; // IDs are never reused while any operator can refer to them.
        let bytes = payload.bytes();
        if bytes <= SHAPE_BYTES {
            if let Some(old) = self.shapes[index].take() { self.shape_bytes -= old.bytes(); }
            if self.shape_bytes + bytes > SHAPE_BYTES {
                self.shapes.fill(None);
                self.shape_bytes = 0;
            }
            self.shapes[index] = Some(payload.clone());
            self.shape_bytes += bytes;
            self.retained();
        }
        Block { payload, offset }
    }
    fn operator_index(source: usize, (frame, raw, band): (u64, u8, usize)) -> usize {
        (source.wrapping_mul(0x9e37_79b1) ^ ((frame ^ (frame >> 32)) as usize).wrapping_mul(0x85eb_ca6b)
            ^ (raw as usize).wrapping_mul(0xc2b2_ae35) ^ band.wrapping_mul(0x27d4_eb2d))
            & (SLOTS - 1)
    }
    fn apply(&mut self, source: &Block, index: usize, graph: &Graph,
        costs: &mut impl machine::ActionCosts) -> Result<Option<Block>, ReplayError> {
        let actual = costs.native_block_operator(index);
        let proof = actual.and_then(|(frame, _, _)| {
            if !(self.uniform || self.static_operators) { return None; }
            source.payload.uniform(costs).filter(|p| p.first <= frame && frame <= p.last)
        });
        // The immutable state vector itself supplies the interval. Different
        // frames in it have identical full native transitions for equal bands.
        let operator = actual.map(|(frame, raw, band)|
            (if self.static_operators && proof.is_some() { 1u64 << 32 } else { u64::from(frame) }, raw, band));
        let slot = operator.map(|key| Self::operator_index(source.payload.id, key));
        if let Some(slot) = slot {
            if let Some(cached) = &self.operators[slot] {
                if cached.source == source.payload.id && Some(cached.operator) == operator {
                    if RECORD { self.stats.operator_hits += 1; }
                    return cached.output.as_ref().map(|block|
                        block.shifted(source.offset.checked_add(cached.shift)
                            .ok_or(ScoringError::TotalScoreOverflow)?)).transpose();
                }
            }
            if RECORD { self.stats.operator_misses += 1; }
        }
        if self.uniform {
            if proof.is_some_and(|p| p.class != (usize::MAX, usize::MAX)) {
                    let state = source.payload.entries[0].0;
                    if RECORD { self.stats.evaluated_entries += 1; }
                    match costs.action_at(index, &graph.actions.get(index), state)? {
                        None => {
                            if RECORD { self.stats.uniform_operators += 1; }
                            if RECORD { self.stats.uniform_entries_saved += source.len().saturating_sub(1) as u64; }
                            if self.static_operators {
                                if let (Some(slot), Some(operator)) = (slot, operator) {
                                    self.store_operator(source, slot, operator, &None);
                                }
                            }
                            return Ok(None);
                        }
                        Some((next, add)) if next == state => {
                            debug_assert_eq!(add.coefficient, 0.);
                            if RECORD { self.stats.uniform_operators += 1; }
                            if RECORD { self.stats.uniform_entries_saved += source.len().saturating_sub(1) as u64; }
                            let output = Some(source.shifted(add.score)?);
                            if self.static_operators {
                                if let (Some(slot), Some(operator)) = (slot, operator) {
                                    self.store_operator(source, slot, operator, &output);
                                }
                            }
                            return Ok(output);
                        }
                        _ => {}
                    }
            }
        }
        let mut entries = std::mem::take(&mut self.scratch);
        for &(state, relative) in &source.payload.entries {
            if RECORD { self.stats.evaluated_entries += 1; }
            let Some((state, add)) = costs.action_at(index, &graph.actions.get(index), state)?
                else { continue; };
            debug_assert_eq!(add.coefficient, 0.);
            let score = source.score(relative).checked_add(add.score)
                .ok_or(ScoringError::TotalScoreOverflow)?;
            entries.push((state, score));
        }
        // Advancing the clock may reorder or collapse full state IDs.
        if !entries.windows(2).all(|w| w[0].0 < w[1].0) {
            entries.sort_unstable_by_key(|e| e.0);
            let mut write = 0;
            for read in 0..entries.len() {
                let entry = entries[read];
                if write > 0 && entries[write - 1].0 == entry.0 {
                    entries[write - 1].1 = entries[write - 1].1.max(entry.1);
                } else { entries[write] = entry; write += 1; }
            }
            entries.truncate(write);
        }
        let output = if entries.is_empty() { self.recycle(entries); None }
            else { Some(self.intern(entries)) };
        if let (Some(slot), Some(operator)) = (slot, operator) {
            self.store_operator(source, slot, operator, &output);
        }
        Ok(output)
    }

    fn store_operator(&mut self, source: &Block, slot: usize,
        operator: (u64, u8, usize), output: &Option<Block>) {
            let shift = output.as_ref().map_or(0, |block| block.offset - source.offset);
            let cached_output = output.as_ref().map(|block|
                Block { payload: block.payload.clone(), offset: 0 });
            let bytes = cached_output.as_ref().map_or(0, |b| b.payload.bytes());
            if bytes <= OPERATOR_BYTES {
                if let Some(old) = self.operators[slot].take() {
                    self.operator_bytes -= old.output.as_ref().map_or(0, |b| b.payload.bytes());
                }
                if self.operator_bytes + bytes > OPERATOR_BYTES {
                    self.operators.fill(None);
                    self.operator_bytes = 0;
                }
                self.operators[slot] = Some(Cached { source: source.payload.id,
                    operator, output: cached_output, shift });
                self.operator_bytes += bytes;
                self.retained();
            }
    }
    /// Exact max union at a single geometry vertex. Return newly live states.
    fn merge(&mut self, target: &mut Option<Block>, source: Block) -> usize {
        let Some(old) = target else {
            let added = source.len();
            *target = Some(source);
            if RECORD { self.stats.shared_propagations += 1; }
            return added;
        };
        if Rc::ptr_eq(&old.payload, &source.payload) {
            old.offset = old.offset.max(source.offset);
            if RECORD { self.stats.equal_shape_merges += 1; }
            return 0;
        }
        let a = &old.payload.entries;
        let b = &source.payload.entries;
        if RECORD { self.stats.scanned_merge_entries += (a.len() + b.len()) as u64; }
        let mut entries = std::mem::take(&mut self.scratch);
        let (mut i, mut j, mut added, mut changed) = (0, 0, 0, false);
        while i < a.len() || j < b.len() {
            if j == b.len() || (i < a.len() && a[i].0 < b[j].0) {
                entries.push((a[i].0, old.score(a[i].1))); i += 1;
            } else if i == a.len() || b[j].0 < a[i].0 {
                entries.push((b[j].0, source.score(b[j].1))); j += 1;
                added += 1; changed = true;
            } else {
                let before = old.score(a[i].1);
                let after = before.max(source.score(b[j].1));
                changed |= after != before;
                entries.push((a[i].0, after)); i += 1; j += 1;
            }
        }
        if changed { *old = self.intern(entries); }
        else { self.recycle(entries); }
        added
    }
    fn reduce(&mut self, block: &mut Option<Block>, scratch: &mut dominance::Scratch,
        costs: &impl machine::ActionCosts, stats: &mut WindowStats) -> usize {
        let Some(value) = block else { return 0; };
        if value.len() <= 1 { return 0; }
        if self.inline_reductions && value.payload.irreducible.get() {
            if RECORD { self.stats.reduction_hits += 1; }
            return 0;
        }
        let source = value.payload.id;
        let offset = value.offset;
        let index = source & (SLOTS - 1);
        if let Some(cached) = self.reductions.get(index).and_then(Option::as_ref)
            .filter(|cached| cached.source == source) {
            if RECORD { self.stats.reduction_hits += 1; }
            let count = cached.removed;
            if RECORD { stats.dominated_value_states += count as u64; }
            if count != 0 {
                *block = cached.output.as_ref().map(|output| Block {
                    payload: output.payload.clone(), offset: offset + cached.shift,
                });
            }
            return count;
        }
        if RECORD { self.stats.reduction_builds += 1; }
        if RECORD { self.stats.reduction_entries += value.len() as u64; }
        let removed = scratch.dominated(value.payload.entries.iter()
            .map(|&(state, relative)| (state, Value { score: value.score(relative), coefficient: 0. })), costs);
        let count = removed.len();
        // Full state IDs and relative scores are immutable in this query.
        // A proof with no removed state therefore remains valid at every use
        // of this same payload, including after the bounded table evicts it.
        if self.inline_reductions && count == 0 { value.payload.irreducible.set(true); }
        if RECORD { stats.dominated_value_states += count as u64; }
        if count != 0 {
            let mut entries = std::mem::take(&mut self.scratch);
            let mut removed = removed.iter().peekable();
            for &(state, relative) in &value.payload.entries {
                if removed.peek().is_some_and(|&&id| id == state) { removed.next(); }
                else { entries.push((state, value.score(relative))); }
            }
            *block = if entries.is_empty() { self.recycle(entries); None }
                else { Some(self.intern(entries)) };
        }
        // Every rank observes immutable full state IDs. A common score shift
        // changes no prefix comparison, so identical relative payloads have
        // exactly the same removed set. This table lives in this fresh query.
        if !self.reductions.is_empty() {
            let output = block.as_ref().map(|output| Block {
                payload: output.payload.clone(), offset: 0,
            });
            let bytes = output.as_ref().map_or(0, |output| output.payload.bytes());
            if bytes <= REDUCTION_BYTES {
                if let Some(old) = self.reductions[index].take() {
                    self.reduction_bytes -= old.output.as_ref().map_or(0, |b| b.payload.bytes());
                }
                if self.reduction_bytes + bytes > REDUCTION_BYTES {
                    for cell in &mut self.reductions { *cell = None; }
                    self.reduction_bytes = 0;
                }
                self.reductions[index] = Some(Reduced { source, output,
                    shift: block.as_ref().map_or(0, |b| b.offset - offset), removed: count });
                self.reduction_bytes += bytes;
                self.retained();
            }
        }
        count
    }
}

pub(super) fn evaluate(graph: &Graph, costs: &mut impl machine::ActionCosts,
    options: WindowOptions, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    evaluate_range(graph, 0, graph.layers.len(), costs, options, stats)
}

pub(super) fn evaluate_record<const RECORD: bool>(graph: &Graph,
    costs: &mut impl machine::ActionCosts, options: WindowOptions, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    evaluate_range_record::<RECORD>(graph, 0, graph.layers.len(), costs, options, stats)
}

/// A certified singleton frontier and expired preceding FIFO permit a fresh
/// default clock at `start`. Global action ranks and native chart count remain.
pub(in crate::hit_timing::independent) fn evaluate_range(graph: &Graph,
    start: usize, end: usize, costs: &mut impl machine::ActionCosts,
    options: WindowOptions, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    evaluate_range_record::<true>(graph, start, end, costs, options, stats)
}

fn evaluate_range_record<const RECORD: bool>(graph: &Graph,
    start: usize, end: usize, costs: &mut impl machine::ActionCosts,
    _options: WindowOptions, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    evaluate_view::<RECORD>(graph, &graph.layers, &graph.touches, &graph.temporal,
        start, end, costs, stats)
}

pub(in crate::hit_timing::independent) fn evaluate_layout_record<const RECORD: bool>(
    graph: &Graph, layout: &super::super::joint::quotient::Layout,
    costs: &mut impl machine::ActionCosts, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    evaluate_view::<RECORD>(graph, &layout.layers, &layout.touches, &layout.temporal,
        0, layout.layers.len(), costs, stats)
}

fn evaluate_view<const RECORD: bool>(graph: &Graph, layers: &[geometry::Layer],
    touches: &[geometry::TouchDag], temporal_blocks: &[geometry::TemporalBlock],
    start: usize, end: usize, costs: &mut impl machine::ActionCosts, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    // The full benchmark did not show a useful gain. Keep the experimental
    // fused traversal available for comparison, with the ordinary path default.
    let fused_exits = costs.value_controls().enabled(Switch::FusedExits);
    evaluate_view_controlled::<RECORD>(graph, layers, touches, temporal_blocks,
        start, end, costs, stats, fused_exits)
}

#[cfg(test)]
pub(in crate::hit_timing::independent) fn evaluate_unfused_for_test(graph: &Graph,
    costs: &mut impl machine::ActionCosts, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    evaluate_view_controlled::<true>(graph, &graph.layers, &graph.touches, &graph.temporal,
        0, graph.layers.len(), costs, stats, false)
}

fn evaluate_view_controlled<const RECORD: bool>(graph: &Graph, layers: &[geometry::Layer],
    touches: &[geometry::TouchDag], temporal_blocks: &[geometry::TemporalBlock],
    start: usize, end: usize, costs: &mut impl machine::ActionCosts, stats: &mut WindowStats,
    fused_exits: bool) -> Result<Option<Option<Value>>, ReplayError> {
    if fused_exits {
        evaluate_view_kernel::<RECORD, true>(graph, layers, touches, temporal_blocks,
            start, end, costs, stats)
    } else {
        evaluate_view_kernel::<RECORD, false>(graph, layers, touches, temporal_blocks,
            start, end, costs, stats)
    }
}

fn evaluate_view_kernel<const RECORD: bool, const FUSED_EXITS: bool>(graph: &Graph,
    layers: &[geometry::Layer], touches: &[geometry::TouchDag],
    temporal_blocks: &[geometry::TemporalBlock], start: usize, end: usize,
    costs: &mut impl machine::ActionCosts, stats: &mut WindowStats)
    -> Result<Option<Option<Value>>, ReplayError> {
    if RECORD { stats.compact_value_queries += 1; }
    let mut storage = Storage::<RECORD>::with_controls(costs.value_controls());
    let mut values = vec![Some(storage.intern(vec![(0, 0)]))];
    let mut next: Vec<Option<Block>> = vec![];
    let mut dominance = dominance::Scratch::default();
    let reduce = costs.supports_dominance();
    let temporal = costs.supports_temporal_intervals();
    let (mut interval_index, mut touch_index, mut layer_index, mut live_nodes) = (
        temporal_blocks.partition_point(|b| b.start < start),
        touches.partition_point(|b| b.layer < start), start, 1);
    let mut stable: Vec<(usize, bool)> = vec![];
    let mut skip = None;
    let mut transitions = 0;
    let frontier_lengths=costs.value_controls().enabled(Switch::FrontierLengths);
    while layer_index < end {
        if temporal {
            if let Some(block) = temporal_blocks.get(interval_index)
                .filter(|block| block.start == layer_index && block.resume < end) {
                let epoch = interval_index + 1;
                let all_stable = values[..live_nodes].iter().flatten()
                    .flat_map(|value| value.payload.entries.iter()).all(|&(state, _)| {
                        if stable.len() <= state { stable.resize(state + 1, (0, false)); }
                        if stable[state].0 != epoch {
                            if RECORD { stats.temporal_stability_probes += 1; }
                            stable[state] = (epoch, costs.stable_interval(state,
                                block.first_frame, block.last_frame));
                        }
                        stable[state].1
                    });
                if all_stable { skip = Some(block); }
                else if RECORD { stats.temporal_clock_fallbacks += 1; }
                interval_index += 1;
            }
        }
        let layer = &layers[layer_index];
        if RECORD { stats.value_graph_layers += 1; }
        if RECORD { stats.value_graph_edges += layer.edges.len(); }
        if frontier_lengths || next.len() < layer.count { next.resize_with(layer.count, || None); }
        next.fill(None); // Release retired vertices too when the frontier shrinks.
        while touches.get(touch_index).is_some_and(|t| t.layer < layer_index) {
            touch_index += 1;
        }
        let mut fused_count = None;
        let live = if let Some(touch) = touches.get(touch_index)
            .filter(|touch| touch.layer == layer_index) {
            touch_index += 1;
            if RECORD { stats.touch_dag_layers += 1; }
            if RECORD { stats.value_graph_edges += touch.inputs.len(); }
            let mut live = 0;
            for (from, &to) in touch.inputs.iter().enumerate() {
                let entry = values[from].take();
                let count = entry.as_ref().map_or(0, Block::len);
                if RECORD { transitions += count as u64; }
                if RECORD { stats.touch_entry_transitions += count as u64; }
                if RECORD { storage.stats.carried_entries += count as u64; }
                if let Some(entry) = entry {
                    let added = storage.merge(&mut next[to], entry);
                    if RECORD { live += added; }
                }
            }
            let exit = FUSED_EXITS.then(|| exits::following(layer_index, end, layers,
                &touches[touch_index..], temporal_blocks, interval_index, skip)).flatten();
            if let Some(exit) = exit {
                // Entry blocks have all been taken. The retired input frontier
                // now holds the exit outputs while `next` holds the DAG work.
                if frontier_lengths || values.len() < exit.count {
                    values.resize_with(exit.count, || None);
                }
                values.fill(None);
                fused_count = Some(exit.count);
                if RECORD {
                    stats.value_graph_layers += 1;
                    stats.value_graph_edges += exit.edges.len();
                    storage.stats.fused_exit_layers += 1;
                }
            }
            if RECORD { stats.peak_value_states = stats.peak_value_states.max(live); }
            let mut edge_index = 0;
            let mut exit_index = 0;
            for node in 0..layer.count {
                if reduce { let removed = storage.reduce(&mut next[node], &mut dominance, costs, stats); if RECORD { live -= removed; } }
                while let Some(edge) = layer.edges.get(edge_index).filter(|e| e.from == node) {
                    debug_assert!(edge.from < edge.to);
                    if let Some(source) = &next[node] {
                        let count = source.len() as u64;
                        if RECORD { transitions += count; }
                        if RECORD { stats.touch_hit_transitions += count; }
                        let action = edge.action.expect("touch DAG edges are successful hits");
                        if let Some(output) = storage.apply(source, action, graph, costs)? {
                            let added = storage.merge(&mut next[edge.to], output); if RECORD { live += added; }
                        }
                    }
                    if RECORD { stats.peak_value_states = stats.peak_value_states.max(live); }
                    edge_index += 1;
                }
                if let Some(exit) = exit {
                    let value = next[node].take();
                    if RECORD { live -= value.as_ref().map_or(0, Block::len); }
                    exits::emit(node, value, &exit.edges, &mut exit_index,
                        &mut values[..exit.count], &mut storage, &mut transitions);
                }
            }
            debug_assert_eq!(edge_index, layer.edges.len());
            if let Some(exit) = exit {
                debug_assert_eq!(exit_index, exit.edges.len());
                if reduce {
                    for value in &mut values[..exit.count] {
                        storage.reduce(value, &mut dominance, costs, stats);
                    }
                }
                if RECORD { values[..exit.count].iter().flatten().map(Block::len).sum() }
                else { 0 }
            } else { live }
        } else {
            for edge in &layer.edges {
                debug_assert!(edge.from < live_nodes);
                if let Some(source) = &values[edge.from] {
                    if RECORD { transitions += source.len() as u64; }
                    let output = if let Some(action) = edge.action {
                        storage.apply(source, action, graph, costs)?
                    } else {
                        if RECORD { storage.stats.carried_entries += source.len() as u64; }
                        Some(source.clone())
                    };
                    if let Some(output) = output { storage.merge(&mut next[edge.to], output); }
                }
            }
            if reduce {
                for value in &mut next[..layer.count] { storage.reduce(value, &mut dominance, costs, stats); }
            }
            if RECORD { next[..layer.count].iter().flatten().map(Block::len).sum() } else { 0 }
        };
        if RECORD { stats.peak_value_states = stats.peak_value_states.max(live); }
        if let Some(count) = fused_count {
            live_nodes = count;
            layer_index += 2;
        } else {
            live_nodes = layer.count;
            std::mem::swap(&mut values, &mut next);
            layer_index += 1;
        }
        if let Some(block) = skip.filter(|block| block.after_first == layer_index) {
            debug_assert_eq!(live_nodes, block.count);
            if RECORD { stats.temporal_skipped_intervals += 1; }
            if RECORD { stats.temporal_skipped_frames += (block.last_frame - block.first_frame) as usize; }
            if RECORD { stats.temporal_skipped_layers += block.resume - block.after_first; }
            if RECORD { stats.temporal_carried_value_states += live as u64; }
            if !block.permutation.is_empty() {
                if frontier_lengths || next.len() < block.count { next.resize_with(block.count, || None); }
                next.fill(None);
                for (from, &to) in block.permutation.iter().enumerate() {
                    next[to] = values[from].take();
                }
                std::mem::swap(&mut values, &mut next);
            }
            layer_index = block.resume;
            skip = None;
        }
    }
    if RECORD { stats.transitions += transitions; }
    if RECORD { stats.native_blocks = Some(storage.stats); }
    Ok(Some(values[..live_nodes].iter().flatten().map(Block::maximum).max()
        .map(|score| Value { score, coefficient: 0. })))
}

#[cfg(test)]
mod tests {
    use super::*;
    type Storage = super::Storage<true>;
    use geometry::{Action, ActionHits, Factor};

    fn absolute(block: &Block) -> Vec<(usize, u64)> {
        block.payload.entries.iter().map(|&(s, v)| (s, block.score(v))).collect()
    }
    struct Costs { calls: usize, reject: bool }
    impl machine::ActionCosts for Costs {
        fn native_block_operator(&self, _: usize) -> Option<(u32, u8, usize)> { Some((120, 4, 0)) }
        fn hit(&mut self, _: Factor, state: usize) -> Result<(usize, Value), ReplayError> {
            self.calls += 1;
            Ok((state / 2, Value { score: 3 + state as u64, coefficient: 0. }))
        }
        fn action_at(&mut self, _: usize, action: &Action, state: usize)
            -> Result<Option<(usize, Value)>, ReplayError> {
            if self.reject && state == 1 { self.calls += 1; Ok(None) }
            else { self.hit(action.factor, state).map(Some) }
        }
        fn run(&mut self, _: Factor, _: usize, _: usize) -> Result<(usize, Value), ReplayError> { unreachable!() }
    }
    fn graph() -> Graph {
        Graph { layers: vec![], touches: vec![], temporal: vec![], edge_count: 0,
            note_count: 1, actions: vec![Action {
                factor: Factor { perfect: true, inside: false, slot: 0, fever: false, reference: 1., rank: 1 },
                hits: ActionHits::Event([(0, 120)]) }].into() }
    }
    fn exit_graph() -> Graph {
        use geometry::{Edge, Layer, TouchDag};
        let mut result = graph();
        result.layers = vec![Layer { count: 4, edges: vec![
            Edge { from: 0, to: 1, action: Some(0) },
            Edge { from: 0, to: 2, action: Some(0) },
            Edge { from: 1, to: 3, action: Some(0) },
            Edge { from: 2, to: 3, action: Some(0) },
        ].into() }, Layer { count: 2, edges: vec![
            Edge { from: 1, to: 0, action: None },
            Edge { from: 2, to: 0, action: None },
            Edge { from: 3, to: 0, action: None },
            Edge { from: 3, to: 1, action: None },
        ].into() }];
        result.touches = vec![TouchDag { layer: 0, inputs: vec![0], incremental: None }];
        result
    }
    fn exit_value(graph: &Graph, enabled: bool, reject: bool) -> (Option<Option<Value>>, WindowStats) {
        let mut costs = Costs { calls: 0, reject };
        let mut stats = WindowStats::default();
        let value = evaluate_view_controlled::<true>(graph, &graph.layers, &graph.touches,
            &graph.temporal, 0, graph.layers.len(), &mut costs, &mut stats, enabled).unwrap();
        (value, stats)
    }
    #[test]
    fn fused_exits_preserve_missing_multiple_and_merged_exits() {
        let mut graph = exit_graph();
        for reject in [false, true] {
            let (old, old_stats) = exit_value(&graph, false, reject);
            let (new, new_stats) = exit_value(&graph, true, reject);
            assert_eq!(new.map(|v| v.map(|v| v.score)), old.map(|v| v.map(|v| v.score)));
            assert_eq!(new_stats.transitions, old_stats.transitions);
            assert_eq!(new_stats.value_graph_edges, old_stats.value_graph_edges);
            assert_eq!(new_stats.value_graph_layers, old_stats.value_graph_layers);
            assert_eq!(new_stats.native_blocks.unwrap().fused_exit_layers, 1);
        }
        graph.layers[1].edges.wide_mut().clear();
        graph.layers[1].count = 0;
        assert!(exit_value(&graph, true, false).0.unwrap().is_none());
        assert!(exit_value(&graph, false, false).0.unwrap().is_none());
    }
    #[test]
    fn fused_exits_fall_back_for_actions_unsorted_edges_and_clock_boundaries() {
        let mut graph = exit_graph();
        graph.layers[1].edges.wide_mut().swap(0, 3);
        assert_eq!(exit_value(&graph, true, false).1.native_blocks.unwrap().fused_exit_layers, 0);
        graph.layers[1].edges.wide_mut().swap(0, 3);
        graph.layers[1].edges.wide_mut()[0].action = Some(0);
        assert_eq!(exit_value(&graph, true, false).1.native_blocks.unwrap().fused_exit_layers, 0);
        graph.layers[1].edges.wide_mut()[0].action = None;
        let boundary = geometry::TemporalBlock { start: 1, after_first: 2, resume: 3,
            first_frame: 120, last_frame: 121, count: 2, permutation: vec![] };
        assert!(exits::following(0, 2, &graph.layers, &[], &[boundary], 0, None).is_none());
        let boundary = geometry::TemporalBlock { start: 0, after_first: 1, resume: 3,
            first_frame: 120, last_frame: 121, count: 2, permutation: vec![] };
        assert!(exits::following(0, 2, &graph.layers, &[], &[], 0, Some(&boundary)).is_none());
        assert!(exits::following(0, 1, &graph.layers, &[], &[], 0, None).is_none());
    }
    #[test]
    fn fused_exits_match_stateful_rejections_and_checked_overflow() {
        struct Stateful { reject: bool, overflow: bool }
        impl machine::ActionCosts for Stateful {
            fn action_at(&mut self, index: usize, _: &Action, state: usize)
                -> Result<Option<(usize, Value)>, ReplayError> {
                if self.reject && state == 2 && index == 0 { return Ok(None); }
                Ok(Some(((state + index + 1) % 4, Value {
                    score: if self.overflow { u64::MAX - 1 } else { (2 + state * 3 + index) as u64 },
                    coefficient: 0.,
                })))
            }
            fn hit(&mut self, _: Factor, _: usize) -> Result<(usize, Value), ReplayError> { unreachable!() }
            fn run(&mut self, _: Factor, _: usize, _: usize) -> Result<(usize, Value), ReplayError> { unreachable!() }
        }
        use geometry::{Edge, Layer, TouchDag};
        for seed in 0..64 {
            let mut graph = graph();
            graph.actions.push(graph.actions.get(0).clone());
            let count = 3 + seed % 7;
            let mut hits = vec![];
            let mut exits = vec![];
            for from in 0..count {
                for to in from + 1..count {
                    if (from * 11 + to * 7 + seed) % 3 != 0 {
                        hits.push(Edge { from, to, action: Some((from + to + seed) % 2) });
                    }
                }
                for to in 0..seed % 4 {
                    if (from * 5 + to + seed) % 3 != 0 {
                        exits.push(Edge { from, to, action: None });
                    }
                }
            }
            graph.layers = vec![Layer { count, edges: hits.into() },
                Layer { count: seed % 4, edges: exits.into() }];
            graph.touches = vec![TouchDag { layer: 0, inputs: vec![0], incremental: None }];
            for reject in [false, true] {
                for overflow in [false, true] {
                    let query = |enabled| {
                        let mut costs = Stateful { reject, overflow };
                        evaluate_view_controlled::<true>(&graph, &graph.layers, &graph.touches,
                            &graph.temporal, 0, 2, &mut costs, &mut WindowStats::default(), enabled)
                            .map(|v| v.map(|v| v.map(|v| v.score))).map_err(|e| format!("{e:?}"))
                    };
                    assert_eq!(query(true), query(false), "seed={seed} reject={reject} overflow={overflow}");
                }
            }
        }
    }
    #[test]
    fn canonical_offsets_and_exact_partial_max_union() {
        let mut storage = Storage::new();
        let first = storage.intern(vec![(0, 10), (2, 20)]);
        let shifted = storage.intern(vec![(0, 110), (2, 120)]);
        assert!(Rc::ptr_eq(&first.payload, &shifted.payload));
        let mut target = Some(first);
        assert_eq!(storage.merge(&mut target, shifted), 0);
        let other = storage.intern(vec![(1, 7), (2, 130)]);
        assert_eq!(storage.merge(&mut target, other), 1);
        assert_eq!(absolute(target.as_ref().unwrap()), vec![(0, 110), (1, 7), (2, 130)]);
    }
    #[test]
    fn operators_preserve_rejection_collapsed_ids_offsets_and_overflow() {
        let mut storage = Storage::new();
        let input = storage.intern(vec![(0, 10), (1, 30), (2, 20), (3, 40)]);
        let mut costs = Costs { calls: 0, reject: true };
        let graph = graph();
        let output = storage.apply(&input, 0, &graph, &mut costs).unwrap().unwrap();
        assert_eq!(absolute(&output), vec![(0, 13), (1, 46)]);
        let output = storage.apply(&input.shifted(100).unwrap(), 0, &graph, &mut costs).unwrap().unwrap();
        assert_eq!(absolute(&output), vec![(0, 113), (1, 146)]);
        assert_eq!(costs.calls, 4);
        assert_eq!(storage.stats.operator_hits, 1);
        let near_limit = input.shifted(u64::MAX - 40).unwrap();
        assert!(storage.apply(&near_limit, 0, &graph, &mut costs).is_err());
        assert_eq!(costs.calls, 4);
        let rejected = storage.intern(vec![(1, 30)]);
        assert!(storage.apply(&rejected, 0, &graph, &mut costs).unwrap().is_none());
        assert!(storage.apply(&rejected.shifted(1).unwrap(), 0, &graph, &mut costs).unwrap().is_none());
        assert_eq!(costs.calls, 5);
    }
    #[test]
    fn query_local_tables_obey_retention_caps() {
        let mut storage = Storage::new();
        let mut costs = Costs { calls: 0, reject: false };
        let graph = graph();
        for id in 0..4096 {
            let block = storage.intern((0..64).map(|s| (id * 64 + s, s as u64)).collect());
            storage.apply(&block, 0, &graph, &mut costs).unwrap();
            assert!(storage.shape_bytes <= SHAPE_BYTES);
            assert!(storage.operator_bytes <= OPERATOR_BYTES);
        }
        assert!(storage.stats.retained_bytes <= SHAPE_BYTES + OPERATOR_BYTES + REDUCTION_BYTES
            + SLOTS * (std::mem::size_of::<Option<Rc<Payload>>>()
                + std::mem::size_of::<Option<Cached>>() + std::mem::size_of::<Option<Reduced>>()));
    }

    #[test]
    fn relative_payload_reduction_reuses_only_one_query_and_preserves_shifts() {
        struct Ranked;
        impl machine::ActionCosts for Ranked {
            fn dominance_rank_extra(&self, state: usize) -> Option<(usize, u32, usize)> {
                Some((0, 0, state))
            }
            fn hit(&mut self, _: Factor, _: usize) -> Result<(usize, Value), ReplayError> {
                unreachable!()
            }
            fn run(&mut self, _: Factor, _: usize, _: usize) -> Result<(usize, Value), ReplayError> {
                unreachable!()
            }
        }
        let mut storage = Storage::new();
        let source = storage.intern(vec![(0, 10), (1, 20), (2, 20), (3, 19)]);
        let mut first = Some(source.clone());
        let mut scratch = dominance::Scratch::default();
        let mut stats = WindowStats::default();
        assert_eq!(storage.reduce(&mut first, &mut scratch, &Ranked, &mut stats), 2);
        assert_eq!(absolute(first.as_ref().unwrap()), vec![(2, 20), (3, 19)]);
        let mut shifted = Some(source.shifted(100).unwrap());
        assert_eq!(storage.reduce(&mut shifted, &mut scratch, &Ranked, &mut stats), 2);
        assert_eq!(absolute(shifted.as_ref().unwrap()), vec![(2, 120), (3, 119)]);
        assert_eq!(storage.stats.reduction_hits, 1);
        assert_eq!(storage.stats.reduction_builds, 1);
        assert!(storage.reduction_bytes <= REDUCTION_BYTES);
        let mut other_query = Storage::new();
        let mut independent = Some(other_query.intern(vec![(0, 10), (1, 20), (2, 20), (3, 19)]));
        other_query.reduce(&mut independent, &mut scratch, &Ranked, &mut stats);
        assert_eq!(other_query.stats.reduction_hits, 0);
        assert_eq!(absolute(independent.as_ref().unwrap()), vec![(2, 20), (3, 19)]);
    }

    #[test]
    fn irreducible_payload_survives_table_eviction_but_not_a_fresh_query() {
        struct Ranked;
        impl machine::ActionCosts for Ranked {
            fn dominance_rank_extra(&self, state: usize) -> Option<(usize,u32,usize)> {
                Some((0,0,state))
            }
            fn hit(&mut self, _: Factor, _: usize) -> Result<(usize, Value), ReplayError> { unreachable!() }
            fn run(&mut self, _: Factor, _: usize, _: usize) -> Result<(usize, Value), ReplayError> { unreachable!() }
        }
        let mut storage = Storage::new();
        storage.inline_reductions = true;
        storage.reductions.clear();
        let source = storage.intern(vec![(0,20),(1,10)]);
        let mut scratch = dominance::Scratch::default();
        let mut stats = WindowStats::default();
        let mut first = Some(source.clone());
        assert_eq!(storage.reduce(&mut first,&mut scratch,&Ranked,&mut stats),0);
        let mut shifted = Some(source.shifted(100).unwrap());
        assert_eq!(storage.reduce(&mut shifted,&mut scratch,&Ranked,&mut stats),0);
        assert_eq!(absolute(shifted.as_ref().unwrap()),vec![(0,120),(1,110)]);
        assert_eq!(storage.stats.reduction_builds,1);
        assert_eq!(storage.stats.reduction_hits,1);
        let mut fresh = Storage::new();
        let mut same = Some(fresh.intern(vec![(0,20),(1,10)]));
        assert_eq!(fresh.reduce(&mut same,&mut scratch,&Ranked,&mut stats),0);
        assert_eq!(fresh.stats.reduction_builds,1);
    }

    #[test]
    fn uniform_blocks_preserve_ids_scores_rejection_expiry_and_overflow() {
        struct UniformCosts { frame: u32, calls: usize, reject: bool }
        impl machine::ActionCosts for UniformCosts {
            fn native_block_operator(&self, _: usize) -> Option<(u32, u8, usize)> {
                Some((self.frame, 4, 0))
            }
            fn native_uniform_state(&self, state: usize) -> Option<machine::NativeUniformState> {
                Some(machine::NativeUniformState { class: (state / 2, 0), first: 120, last: 121 })
            }
            fn action_at(&mut self, _: usize, _: &Action, state: usize)
                -> Result<Option<(usize, Value)>, ReplayError> {
                self.calls += 1;
                if self.reject { return Ok(None); }
                let (next, score) = if self.frame <= 121 { (state, 5) } else { (state + 10, 2) };
                Ok(Some((next, Value { score, coefficient: 0. })))
            }
            fn hit(&mut self, _: Factor, _: usize) -> Result<(usize, Value), ReplayError> { unreachable!() }
            fn run(&mut self, _: Factor, _: usize, _: usize) -> Result<(usize, Value), ReplayError> { unreachable!() }
        }
        let graph = graph();
        let mut storage = Storage::new();
        storage.uniform = true;
        storage.static_operators = true;
        let input = storage.intern(vec![(0, 10), (1, 30)]);
        let mut costs = UniformCosts { frame: 120, calls: 0, reject: false };
        let output = storage.apply(&input, 0, &graph, &mut costs).unwrap().unwrap();
        assert_eq!(absolute(&output), vec![(0, 15), (1, 35)]);
        assert!(Rc::ptr_eq(&input.payload, &output.payload));
        assert_eq!(costs.calls, 1);
        costs.frame = 121;
        let output = storage.apply(&input.shifted(100).unwrap(), 0, &graph, &mut costs).unwrap().unwrap();
        assert_eq!(absolute(&output), vec![(0, 115), (1, 135)]);
        assert_eq!(costs.calls, 1);
        let near_limit = input.shifted(u64::MAX - 30).unwrap();
        assert!(storage.apply(&near_limit, 0, &graph, &mut costs).is_err());
        costs.frame = 122;
        let output = storage.apply(&input, 0, &graph, &mut costs).unwrap().unwrap();
        assert_eq!(absolute(&output), vec![(10, 12), (11, 32)]);
        assert_eq!(storage.stats.uniform_operators, 1);
        let mut independent = Storage::new();
        independent.uniform = true;
        independent.static_operators = true;
        let mixed = independent.intern(vec![(0, 10), (2, 30)]);
        costs.frame = 120;
        let first = independent.apply(&mixed, 0, &graph, &mut costs).unwrap().unwrap();
        assert_eq!(independent.stats.uniform_operators, 0);
        let calls = costs.calls;
        costs.frame = 121;
        let second = independent.apply(&mixed.shifted(100).unwrap(), 0, &graph, &mut costs).unwrap().unwrap();
        assert_eq!(costs.calls, calls);
        assert_eq!(absolute(&second), absolute(&first).into_iter().map(|(s,v)| (s,v+100)).collect::<Vec<_>>());
        assert_eq!(independent.stats.operator_hits, 1);
        costs.reject = true;
        let all = independent.intern(vec![(0, 0), (1, 0)]);
        assert!(independent.apply(&all, 0, &graph, &mut costs).unwrap().is_none());
        assert_eq!(independent.stats.uniform_operators, 1);
    }
}
