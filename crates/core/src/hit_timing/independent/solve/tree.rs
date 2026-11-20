//! Ordered tree evaluator retained as an independent value-container audit.
use super::*;

pub(super) fn evaluate<const TRACE: bool>(
    graph: &Graph, costs: &mut impl machine::ActionCosts, objective: WindowObjective,
    options: WindowOptions, stats: &mut WindowStats,
) -> Result<Option<Option<(Value, HitPlan)>>, ReplayError> {
    let mut values = vec![BTreeMap::from([(
        0, Point { value: Value::default(), trace: None },
    )])];
    let mut traces = vec![];
    let mut next_trace_gc = trace_gc::MIN_INTERVAL;
    let mut transitions = 0;
    let mut dominance = dominance::Scratch::default();
    let reduce = objective == WindowObjective::Native && costs.supports_dominance();
    let mut touch_index = 0;
    for (layer_index, layer) in graph.layers.iter().enumerate() {
        let mut next = if let Some(touch) = graph.touches.get(touch_index)
            .filter(|touch| touch.layer == layer_index) {
            touch_index += 1;
            let Some(next) = dag::tree::<TRACE>(
                touch, layer, graph, costs, &values, objective, options, stats,
                &mut transitions, &mut dominance, reduce, &mut traces, &mut next_trace_gc,
            )? else { stats.transitions += transitions; return Ok(None); };
            next
        } else {
            let mut next = vec![BTreeMap::<usize, Point>::new(); layer.count];
            for edge in &layer.edges {
                for (&state, &point) in &values[edge.from] {
                    transitions += 1;
                    if transitions > options.max_transitions {
                        stats.transitions += transitions; return Ok(None);
                    }
                    let (state, value) = if let Some(i) = edge.action {
                        let Some((state, add)) = costs.action_at(i, &graph.actions.get(i), state)?
                            else { continue; };
                        (state, point.value.add(add)?)
                    } else { (state, point.value) };
                    if next[edge.to].get(&state)
                        .is_some_and(|old| costs.compare(old.value, value, objective).is_ge()) {
                        continue;
                    }
                    let trace = if TRACE {
                        edge.action.map_or(point.trace, |action| {
                            let index = traces.len();
                            traces.push(Trace { previous: point.trace, action });
                            Some(index)
                        })
                    } else { None };
                    next[edge.to].insert(state, Point { value, trace });
                }
            }
            next
        };
        if reduce {
            for entries in &mut next {
                let removed = dominance.dominated(
                    entries.iter().map(|(&state, point)| (state, point.value)), costs);
                stats.dominated_value_states += removed.len() as u64;
                for state in removed { entries.remove(state); }
            }
        }
        let live: usize = next.iter().map(BTreeMap::len).sum();
        stats.peak_value_states = stats.peak_value_states.max(live);
        if live > options.max_states { stats.transitions += transitions; return Ok(None); }
        if TRACE && traces.len() >= next_trace_gc {
            next_trace_gc = trace_gc::collect(&mut traces, next.iter_mut().flat_map(BTreeMap::values_mut));
        }
        values = next;
    }
    stats.transitions += transitions;
    let winner = values.iter().flat_map(BTreeMap::values)
        .max_by(|a, b| costs.compare(a.value, b.value, objective)).copied();
    Ok(Some(winner.map(|winner| {
        let plan = if TRACE { reconstruct(graph, &traces, winner.trace) }
            else { HitPlan::default() };
        (winner.value, plan)
    })))
}
