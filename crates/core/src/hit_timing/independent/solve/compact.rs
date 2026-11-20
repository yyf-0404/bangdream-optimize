//! Same ordered dynamic program, without a tree allocation for each live node
//! in each layer. Both layer buffers keep their per-node capacities. Entries
//! remain sorted by skill state, including after a layer shrinks then grows.
use super::*;

pub(super) fn evaluate<const TRACE: bool>(
    graph: &Graph,
    costs: &mut impl machine::ActionCosts,
    objective: WindowObjective,
    options: WindowOptions,
    stats: &mut WindowStats,
) -> Result<Option<Option<(Value, HitPlan)>>, ReplayError> {
    stats.compact_value_queries += 1;
    let mut values = vec![vec![(
        0,
        Point {
            value: Value::default(),
            trace: None,
        },
    )]];
    let mut next: Vec<Vec<(usize, Point)>> = vec![];
    let mut traces: Vec<Trace> = vec![];
    let mut next_trace_gc = trace_gc::MIN_INTERVAL;
    let mut transitions = 0;
    let mut dominance = dominance::Scratch::default();
    let mut incremental = incremental::Scratch::default();
    let mut last_touch = None;
    let reduce = objective == WindowObjective::Native && costs.supports_dominance();
    let temporal = objective == WindowObjective::Native && costs.supports_temporal_intervals();
    let mut interval_index = 0;
    let mut touch_index = 0;
    let mut skip = None;
    // Cache each actual clock proof once per interval, across geometry nodes.
    let mut stable: Vec<(usize, bool)> = vec![];
    let mut live_nodes = 1;
    let mut layer_index = 0;
    while layer_index < graph.layers.len() {
        if temporal {
            if let Some(block) = graph.temporal.get(interval_index)
                .filter(|block| block.start == layer_index) {
                let epoch = interval_index + 1;
                let all_stable = values[..live_nodes].iter().flat_map(|v| v.iter())
                    .all(|&(state, _)| {
                        if stable.len() <= state { stable.resize(state + 1, (0, false)); }
                        if stable[state].0 != epoch {
                            stats.temporal_stability_probes += 1;
                            stable[state] = (epoch, costs.stable_interval(
                                state, block.first_frame, block.last_frame));
                        }
                        stable[state].1
                    });
                if all_stable { skip = Some(block); }
                else { stats.temporal_clock_fallbacks += 1; }
                interval_index += 1;
            }
        }
        let layer = &graph.layers[layer_index];
        stats.value_graph_layers += 1;
        stats.value_graph_edges += layer.edges.len();
        if next.len() < layer.count {
            next.resize_with(layer.count, Vec::new);
        }
        for entries in &mut next[..layer.count] {
            entries.clear();
        }
        while graph.touches.get(touch_index).is_some_and(|touch| touch.layer < layer_index) {
            touch_index += 1;
        }
        let live = if let Some(touch) = graph.touches.get(touch_index)
            .filter(|touch| touch.layer == layer_index) {
            touch_index += 1;
            let Some(live) = dag::compact::<TRACE>(
                touch, layer, graph, costs, &values[..live_nodes], &mut next[..layer.count],
                objective, options, stats, &mut transitions, &mut dominance, reduce,
                &mut traces, &mut next_trace_gc,
                &mut incremental, last_touch,
            )? else { stats.transitions += transitions; return Ok(None); };
            last_touch = Some(layer_index);
            live
        } else {
            if layer.edges.iter().any(|edge| edge.action.is_some()) { last_touch = None; }
            for edge in &layer.edges {
                debug_assert!(edge.from < live_nodes);
                for &(state, point) in &values[edge.from] {
                    transitions += 1;
                    if transitions > options.max_transitions {
                        stats.transitions += transitions;
                        return Ok(None);
                    }
                    let (state, value) = if let Some(i) = edge.action {
                        let action = &graph.actions.get(i);
                        let Some((state, add)) = costs.action_at(i, action, state)? else {
                            continue;
                        };
                        (state, point.value.add(add)?)
                    } else {
                        (state, point.value)
                    };
                    let entries = &mut next[edge.to];
                    let position = entries.binary_search_by_key(&state, |&(key, _)| key);
                    if let Ok(i) = position {
                        if costs.compare(entries[i].1.value, value, objective).is_ge() {
                            continue;
                        }
                    }
                    // Preserve first-arrival wins within a state and exactly the
                    // original trace insertion order. A value-only query allocates
                    // no trace entries and performs no whole-chart reconstruction.
                    let trace = if TRACE {
                        edge.action.map_or(point.trace, |action| {
                            let index = traces.len();
                            traces.push(Trace {
                                previous: point.trace,
                                action,
                            });
                            Some(index)
                        })
                    } else {
                        None
                    };
                    let point = Point { value, trace };
                    match position {
                        Ok(i) => entries[i].1 = point,
                        Err(i) => entries.insert(i, (state, point)),
                    }
                }
            }
            if reduce {
                for entries in &mut next[..layer.count] {
                    let removed = dominance.dominated(
                        entries.iter().map(|&(state, point)| (state, point.value)), costs);
                    stats.dominated_value_states += removed.len() as u64;
                    let mut removed = removed.iter().peekable();
                    entries.retain(|(state, _)| {
                        if removed.peek().is_some_and(|&&id| id == *state) {
                            removed.next();
                            false
                        } else {
                            true
                        }
                    });
                }
            }
            next[..layer.count].iter().map(Vec::len).sum()
        };
        stats.peak_value_states = stats.peak_value_states.max(live);
        if live > options.max_states {
            stats.transitions += transitions;
            return Ok(None);
        }
        if TRACE && traces.len() >= next_trace_gc {
            next_trace_gc = trace_gc::collect(
                &mut traces,
                next[..layer.count]
                    .iter_mut()
                    .flat_map(|v| v.iter_mut().map(|(_, p)| p)),
            );
        }
        live_nodes = layer.count;
        std::mem::swap(&mut values, &mut next);
        layer_index += 1;
        if let Some(block) = skip.filter(|block| block.after_first == layer_index) {
            debug_assert_eq!(live_nodes, block.count);
            stats.temporal_skipped_intervals += 1;
            stats.temporal_skipped_frames += (block.last_frame - block.first_frame) as usize;
            stats.temporal_skipped_layers += block.resume - block.after_first;
            stats.temporal_carried_value_states += live as u64;
            if !block.permutation.is_empty() {
                if next.len() < block.count { next.resize_with(block.count, Vec::new); }
                for entries in &mut next[..block.count] { entries.clear(); }
                for (from, &to) in block.permutation.iter().enumerate() {
                    std::mem::swap(&mut values[from], &mut next[to]);
                }
                std::mem::swap(&mut values, &mut next);
            }
            layer_index = block.resume;
            last_touch = None;
            skip = None;
        }
    }
    stats.transitions += transitions;
    // Restrict to the final layer: retained capacity may contain older entries.
    // max_by keeps the last equal winner, matching ordered BTreeMap iteration.
    let winner = values[..live_nodes]
        .iter()
        .flat_map(|entries| entries.iter().map(|&(_, point)| point))
        .max_by(|a, b| costs.compare(a.value, b.value, objective));
    Ok(Some(winner.map(|winner| {
        let plan = if TRACE {
            reconstruct(graph, &traces, winner.trace)
        } else {
            HitPlan::default()
        };
        (winner.value, plan)
    })))
}
