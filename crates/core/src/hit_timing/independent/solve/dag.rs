//! A frame is an acyclic hit graph. All incoming paths to a geometry vertex are
//! merged before its outgoing hits; every intermediate vertex is a legal exit.
use super::*;
use geometry::{Layer, TouchDag};

pub(super) fn compact<const TRACE: bool>(
    dag: &TouchDag, layer: &Layer, graph: &Graph, costs: &mut impl machine::ActionCosts,
    values: &[Vec<(usize, Point)>], next: &mut [Vec<(usize, Point)>],
    objective: WindowObjective, options: WindowOptions, stats: &mut WindowStats,
    transitions: &mut u64, dominance: &mut dominance::Scratch, reduce: bool,
    traces: &mut Vec<Trace>, next_trace_gc: &mut usize,
    incremental: &mut incremental::Scratch, last_touch: Option<usize>,
) -> Result<Option<usize>, ReplayError> {
    stats.touch_dag_layers += 1;
    stats.value_graph_edges += dag.inputs.len();
    let mut live = 0;
    for (from, &to) in dag.inputs.iter().enumerate() {
        debug_assert!(next[to].is_empty());
        for &entry in &values[from] {
            *transitions += 1;
            stats.touch_entry_transitions += 1;
            if *transitions > options.max_transitions { return Ok(None); }
            next[to].push(entry);
            live += 1;
        }
    }
    stats.peak_value_states = stats.peak_value_states.max(live);
    if live > options.max_states { return Ok(None); }
    let mut edge_index = 0;
    let proof = incremental.begin(dag, layer.count, last_touch, costs, objective, options);
    for node in 0..layer.count {
        if reduce {
            let entries = &mut next[node];
            let removed = dominance.dominated(
                entries.iter().map(|&(state, point)| (state, point.value)), costs);
            live -= removed.len();
            stats.dominated_value_states += removed.len() as u64;
            let mut removed = removed.iter().peekable();
            entries.retain(|(state, _)| {
                if removed.peek().is_some_and(|&&id| id == *state) {
                    removed.next(); false
                } else { true }
            });
        }
        if let Some(proof) = proof.filter(|p| {
            layer.edges.iter().skip(edge_index).enumerate().take_while(|(_, e)| e.from == node)
                .any(|(offset, _)| p.repeated[edge_index + offset])
        }) {
            incremental.node(node, &next[node], values, proof, costs, objective, stats);
        }
        while let Some(edge) = layer.edges.get(edge_index).filter(|e| e.from == node) {
            debug_assert!(edge.from < edge.to);
            let (sources, targets) = next.split_at_mut(edge.to);
            let entries = &mut targets[0];
            for (entry, &(state, point)) in sources[node].iter().enumerate() {
                if let Some(proof) = proof {
                    if proof.repeated[edge_index] {
                        if incremental.skip(entry, stats) { continue; }
                    } else { stats.incremental_new_transitions += 1; }
                }
                *transitions += 1;
                stats.touch_hit_transitions += 1;
                if *transitions > options.max_transitions { return Ok(None); }
                let action = edge.action.expect("touch DAG edges are successful hits");
                let Some((state, add)) = costs.action_at(action, &graph.actions.get(action), state)?
                    else { continue; };
                let value = point.value.add(add)?;
                let position = entries.binary_search_by_key(&state, |&(key, _)| key);
                if let Ok(i) = position {
                    if costs.compare(entries[i].1.value, value, objective).is_ge() { continue; }
                }
                let trace = if TRACE {
                    let index = traces.len();
                    traces.push(Trace { previous: point.trace, action });
                    Some(index)
                } else { None };
                let point = Point { value, trace };
                match position {
                    Ok(i) => entries[i].1 = point,
                    Err(i) => { entries.insert(i, (state, point)); live += 1; }
                }
            }
            stats.peak_value_states = stats.peak_value_states.max(live);
            if live > options.max_states { return Ok(None); }
            edge_index += 1;
        }
        if TRACE && traces.len() >= *next_trace_gc {
            *next_trace_gc = trace_gc::collect(
                traces, next.iter_mut().flat_map(|v| v.iter_mut().map(|(_, p)| p)),
            );
        }
    }
    debug_assert_eq!(edge_index, layer.edges.len());
    Ok(Some(live))
}

pub(super) fn tree<const TRACE: bool>(
    dag: &TouchDag, layer: &Layer, graph: &Graph, costs: &mut impl machine::ActionCosts,
    values: &[BTreeMap<usize, Point>], objective: WindowObjective, options: WindowOptions,
    stats: &mut WindowStats, transitions: &mut u64, dominance: &mut dominance::Scratch,
    reduce: bool, traces: &mut Vec<Trace>, next_trace_gc: &mut usize,
) -> Result<Option<Vec<BTreeMap<usize, Point>>>, ReplayError> {
    let mut next = vec![BTreeMap::new(); layer.count];
    let mut live = 0;
    stats.touch_dag_layers += 1;
    for (from, &to) in dag.inputs.iter().enumerate() {
        for (&state, &point) in &values[from] {
            *transitions += 1;
            stats.touch_entry_transitions += 1;
            if *transitions > options.max_transitions { return Ok(None); }
            next[to].insert(state, point);
            live += 1;
        }
    }
    stats.peak_value_states = stats.peak_value_states.max(live);
    if live > options.max_states { return Ok(None); }
    let mut edge_index = 0;
    for node in 0..layer.count {
        if reduce {
            let removed = dominance.dominated(
                next[node].iter().map(|(&state, point)| (state, point.value)), costs);
            live -= removed.len();
            stats.dominated_value_states += removed.len() as u64;
            for state in removed { next[node].remove(state); }
        }
        while let Some(edge) = layer.edges.get(edge_index).filter(|e| e.from == node) {
            let (sources, targets) = next.split_at_mut(edge.to);
            let entries = &mut targets[0];
            for (&state, &point) in &sources[node] {
                *transitions += 1;
                stats.touch_hit_transitions += 1;
                if *transitions > options.max_transitions { return Ok(None); }
                let action = edge.action.unwrap();
                let Some((state, add)) = costs.action_at(action, &graph.actions.get(action), state)?
                    else { continue; };
                let value = point.value.add(add)?;
                if entries.get(&state)
                    .is_some_and(|old| costs.compare(old.value, value, objective).is_ge()) {
                    continue;
                }
                let trace = if TRACE {
                    let index = traces.len();
                    traces.push(Trace { previous: point.trace, action });
                    Some(index)
                } else { None };
                if entries.insert(state, Point { value, trace }).is_none() { live += 1; }
            }
            stats.peak_value_states = stats.peak_value_states.max(live);
            if live > options.max_states { return Ok(None); }
            edge_index += 1;
        }
        if TRACE && traces.len() >= *next_trace_gc {
            *next_trace_gc = trace_gc::collect(traces, next.iter_mut().flat_map(BTreeMap::values_mut));
        }
    }
    Ok(Some(next))
}
