use super::*;
use std::collections::BTreeMap;

mod compact;
mod dag;
mod incremental;
mod tree;
mod dominance;
mod trace_gc;
pub(super) mod blocks;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
struct Point {
    value: Value,
    trace: Option<usize>,
}
struct Trace {
    previous: Option<usize>,
    action: usize,
}

/// None=budget, Some(None)=infeasible. Neither is an exact zero or partial upper.
pub(super) fn evaluate_ordered(
    graph: &Graph,
    costs: &mut impl machine::ActionCosts,
    objective: WindowObjective,
    options: WindowOptions,
    stats: &mut WindowStats,
) -> Result<Option<Option<(Value, HitPlan)>>, ReplayError> {
    if options.compact_value_states {
        compact::evaluate::<true>(graph, costs, objective, options, stats)
    } else {
        tree::evaluate::<true>(graph, costs, objective, options, stats)
    }
}

/// A value-only query retains no path traces or whole-chart reconstruction.
pub(super) fn evaluate_value(
    graph: &Graph,
    costs: &mut impl machine::ActionCosts,
    objective: WindowObjective,
    options: WindowOptions,
    stats: &mut WindowStats,
) -> Result<Option<Option<Value>>, ReplayError> {
    evaluate_value_record::<true>(graph, costs, objective, options, stats)
}

pub(super) fn evaluate_value_record<const RECORD: bool>(
    graph: &Graph, costs: &mut impl machine::ActionCosts, objective: WindowObjective,
    options: WindowOptions, stats: &mut WindowStats,
) -> Result<Option<Option<Value>>, ReplayError> {
    if options.compact_value_states && objective == WindowObjective::Native
        && costs.supports_native_blocks() && !options.incremental_touch
        && options.max_transitions == u64::MAX && options.max_states == usize::MAX {
        return if RECORD { blocks::evaluate(graph, costs, options, stats) }
            else { blocks::evaluate_record::<false>(graph, costs, options, stats) };
    }
    let answer = if options.compact_value_states {
        compact::evaluate::<false>(graph, costs, objective, options, stats)?
    } else {
        tree::evaluate::<false>(graph, costs, objective, options, stats)?
    };
    Ok(answer.map(|answer| answer.map(|(value, _)| value)))
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct HitPlan {
    pub frames: Vec<u32>,
    pub order: Vec<usize>,
}

fn reconstruct(graph: &Graph, traces: &[Trace], mut trace: Option<usize>) -> HitPlan {
    let mut actions = vec![];
    while let Some(i) = trace {
        actions.push(traces[i].action);
        trace = traces[i].previous;
    }
    let mut plan = HitPlan {
        frames: vec![u32::MAX; graph.note_count],
        order: Vec::with_capacity(graph.note_count),
    };
    for a in actions.into_iter().rev() {
        for &(note, frame) in &graph.actions.get(a).hits {
            debug_assert_eq!(plan.frames[note], u32::MAX);
            plan.frames[note] = frame;
            plan.order.push(note);
        }
    }
    debug_assert!(plan.frames.iter().all(|&f| f != u32::MAX));
    plan
}

// Legacy graph-equivalence tests only compare frame witnesses. Runtime consumers
// must retain both fields through evaluate_ordered.
#[cfg(test)]
pub(super) fn evaluate(
    graph: &Graph,
    costs: &mut impl machine::ActionCosts,
    objective: WindowObjective,
    options: WindowOptions,
    stats: &mut WindowStats,
) -> Result<Option<Option<(Value, Vec<u32>)>>, ReplayError> {
    Ok(evaluate_ordered(graph, costs, objective, options, stats)?
        .map(|answer| answer.map(|(value, plan)| (value, plan.frames))))
}
