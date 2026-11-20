//! Same checked geometry traversal using query-local numeric value handles.
use super::*;
pub(in crate::hit_timing::independent) fn evaluate<const RECORD: bool>(graph: &Graph,
    layout: &Layout,
    costs: &mut impl machine::ActionCosts, stats: &mut WindowStats, limit: usize)
    -> Result<Option<Option<Value>>, ReplayError> {
    evaluate_controlled::<RECORD>(graph,layout,costs,stats,limit,
        costs.value_controls().enabled(Switch::PartialTemporal))
}
pub(in crate::hit_timing::independent::solve::blocks::packed) fn evaluate_controlled<const RECORD:bool>(graph:&Graph,layout:&Layout,
    costs:&mut impl machine::ActionCosts,stats:&mut WindowStats,limit:usize,partial:bool)
    -> Result<Option<Option<Value>>,ReplayError> {
    evaluate_with_idle::<RECORD>(graph,layout,costs,stats,limit,partial,
        costs.value_controls().enabled(Switch::IdleTemporal))
}
pub(in crate::hit_timing::independent::solve::blocks::packed) fn evaluate_with_idle<const RECORD:bool>(graph:&Graph,layout:&Layout,
    costs:&mut impl machine::ActionCosts,stats:&mut WindowStats,limit:usize,partial:bool,idle:bool)
    -> Result<Option<Option<Value>>,ReplayError> {
    let start = 0;
    let end = layout.spans.len();
    let touches = &layout.touches;
    let temporal_blocks = &layout.temporal;
    if RECORD { stats.compact_value_queries += 1; }
    let mut storage = Storage::<RECORD>::with_controls(limit, costs.value_controls());
    let mut values = vec![storage.intern(vec![(0, 0)])];
    if storage.exhausted { return Ok(None); }
    let mut next: Vec<Option<Block>> = vec![];
    let mut dominance = dominance::Scratch::default();
    let reduce = costs.supports_dominance();
    let temporal = costs.supports_temporal_intervals();
    let (mut interval_index, mut touch_index, mut layer_index, mut live_nodes) = (
        temporal_blocks.partition_point(|b| b.start < start),
        touches.partition_point(|b| b.layer < start), start, 1);
    let mut stable: Vec<(usize, bool)> = vec![];
    let mut skip = None;
    let mut partial_candidate=None;
    let mut parked:Option<partial::Parked>=None;
    let mut transitions = 0;
    let frontier_lengths=costs.value_controls().enabled(Switch::FrontierLengths);
    while layer_index < end {
        if parked.as_ref().is_some_and(|p|p.resume==layer_index) {
            parked.take().unwrap().reunite(&mut storage,&mut values[..live_nodes]);
            if storage.exhausted {return Ok(None);}
        }
        if temporal {
            if let Some(block) = temporal_blocks.get(interval_index)
                .filter(|block| block.start == layer_index && block.resume < end) {
                let epoch = interval_index + 1;
                let all_stable = values[..live_nodes].iter().flatten()
                    .flat_map(|value| storage.values(value).iter()).all(|&(state, _)| {
                        if stable.len() <= state { stable.resize(state + 1, (0, false)); }
                        if stable[state].0 != epoch {
                            if RECORD { stats.temporal_stability_probes += 1; }
                            stable[state] = (epoch, costs.stable_interval(state,
                                block.first_frame, block.last_frame));
                        }
                        stable[state].1
                    });
                if all_stable { skip = Some(block); }
                else {
                    if RECORD { stats.temporal_clock_fallbacks += 1; }
                    if partial {partial_candidate=Some((block,
                        if idle {layout.idle_nodes[interval_index].as_slice()} else {&[]}));}
                }
                interval_index += 1;
            }
        }
        let layer = layout.layer(layer_index);
        if RECORD { stats.value_graph_layers += 1; }
        if RECORD { stats.value_graph_edges += layer.edges.len(); }
        if frontier_lengths || next.len() < layer.count { next.resize_with(layer.count, || None); }
        next.fill(None); // Release retired vertices too when the frontier shrinks.
        while touches.get(touch_index).is_some_and(|t| t.layer < layer_index) {
            touch_index += 1;
        }
        let live = if let Some(touch) = touches.get(touch_index)
            .filter(|touch| touch.layer == layer_index) {
            touch_index += 1;
            if RECORD { stats.touch_dag_layers += 1; }
            if RECORD { stats.value_graph_edges += touch.inputs.len(); }
            let mut live = 0;
            for (from, &to) in touch.inputs.iter().enumerate() {
                let entry = values[from].take();
                let count = entry.as_ref().map_or(0, |b| storage.len(b));
                if RECORD { transitions += count as u64; }
                if RECORD { stats.touch_entry_transitions += count as u64; }
                if RECORD { storage.stats.carried_entries += count as u64; }
                if let Some(entry) = entry {
                    let added = storage.merge(&mut next[to], entry);
                    if storage.exhausted { return Ok(None); }
                    if RECORD { live += added; }
                }
            }
            if RECORD { stats.peak_value_states = stats.peak_value_states.max(live+parked.as_ref().map_or(0,|p|p.count)); }
            let mut edge_index = 0;
            for node in 0..layer.count {
                if reduce { let removed = storage.reduce(&mut next[node], &mut dominance, costs, stats); if storage.exhausted { return Ok(None); } if RECORD { live -= removed; } }
                while let Some(edge) = layer.edges.get(edge_index).filter(|e| e.from as usize == node) {
                    debug_assert!(edge.from < edge.to);
                    if let Some(source) = &next[node] {
                        let count = storage.len(source) as u64;
                        if RECORD { transitions += count; }
                        if RECORD { stats.touch_hit_transitions += count; }
                        let action = edge.action().expect("touch DAG edges are successful hits");
                        if let Some(output) = { let output=storage.apply(source, action, graph, costs)?; if storage.exhausted { return Ok(None); } output } {
                            let added = storage.merge(&mut next[edge.to as usize], output); if storage.exhausted { return Ok(None); }
                            if RECORD { live += added; }
                        }
                    }
                    if RECORD { stats.peak_value_states = stats.peak_value_states.max(live+parked.as_ref().map_or(0,|p|p.count)); }
                    edge_index += 1;
                }
            }
            debug_assert_eq!(edge_index, layer.edges.len());
            live
        } else {
            for edge in layer.edges {
                debug_assert!((edge.from as usize) < live_nodes);
                if let Some(source) = &values[edge.from as usize] {
                    if RECORD { transitions += storage.len(source) as u64; }
                    let output = if let Some(action) = edge.action() {
                        { let output=storage.apply(source, action, graph, costs)?; if storage.exhausted { return Ok(None); } output }
                    } else {
                        if RECORD { storage.stats.carried_entries += storage.len(source) as u64; }
                        Some(*source)
                    };
                    if let Some(output) = output { storage.merge(&mut next[edge.to as usize], output); if storage.exhausted { return Ok(None); } }
                }
            }
            if reduce {
                for value in &mut next[..layer.count] { storage.reduce(value, &mut dominance, costs, stats); if storage.exhausted { return Ok(None); } }
            }
            if RECORD { next[..layer.count].iter().flatten().map(|b| storage.len(b)).sum() } else { 0 }
        };
        if RECORD { stats.peak_value_states = stats.peak_value_states.max(live+parked.as_ref().map_or(0,|p|p.count)); }
        live_nodes = layer.count;
        std::mem::swap(&mut values, &mut next);
        layer_index += 1;
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
        if let Some((block,idle_nodes))=partial_candidate.filter(|(block,_)|block.after_first==layer_index) {
            debug_assert!(parked.is_none());
            debug_assert_eq!(live_nodes,block.count);
            parked=partial::Parked::new(&mut storage,&mut values[..live_nodes],block,idle_nodes,costs,stats);
            if storage.exhausted {return Ok(None);}
            if parked.is_some()&&values[..live_nodes].iter().all(Option::is_none) {
                values=parked.take().unwrap().values;
                layer_index=block.resume;
            }
            partial_candidate=None;
        }
    }
    if RECORD { stats.transitions += transitions; }
    let maximum=values[..live_nodes].iter().flatten().map(|b| storage.maximum(b)).max();
    if RECORD {
        storage.stats.value_arena_queries += 1;
        stats.native_blocks = Some(storage.stats);
    }
    Ok(Some(maximum.map(|score| Value { score, coefficient: 0. })))
}
