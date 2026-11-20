//! Reclaim only histories unreachable from the completed current frontier.
//! The trigger is an allocation policy, never a search/state/candidate limit.
use super::{Point, Trace};
pub(super) const MIN_INTERVAL: usize = 65_536;
pub(super) fn collect<'a>(
    traces: &mut Vec<Trace>,
    points: impl Iterator<Item = &'a mut Point>,
) -> usize {
    let mut points: Vec<_> = points.collect();
    let mut remap = vec![usize::MAX; traces.len()];
    for point in &points {
        let mut trace = point.trace;
        while let Some(i) = trace {
            if remap[i] != usize::MAX {
                break;
            }
            remap[i] = 0;
            trace = traces[i].previous;
        }
    }
    let mut retained = 0;
    for i in 0..traces.len() {
        if remap[i] == usize::MAX {
            continue;
        }
        // A history always points backwards. Its surviving parent has already
        // received its new index, so compaction can happen in place.
        let previous = traces[i].previous.map(|p| {
            debug_assert!(p < i && remap[p] != usize::MAX);
            remap[p]
        });
        let action = traces[i].action;
        remap[i] = retained;
        traces[retained] = Trace { previous, action };
        retained += 1;
    }
    traces.truncate(retained);
    for point in &mut points {
        point.trace = point.trace.map(|i| remap[i]);
    }
    retained.saturating_mul(2).max(MIN_INTERVAL)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::independent::Value;
    fn path(traces: &[Trace], mut root: Option<usize>) -> Vec<usize> {
        let mut actions = vec![];
        while let Some(i) = root {
            actions.push(traces[i].action);
            root = traces[i].previous;
        }
        actions.reverse();
        actions
    }
    #[test]
    fn collection_preserves_every_live_history_and_reclaims_only_unreachable_nodes() {
        let mut traces = vec![];
        for i in 0..4096 {
            traces.push(Trace {
                previous: if i == 0 { None } else { Some((i - 1) / 2) },
                action: i,
            });
        }
        let mut points: Vec<_> = [None, Some(108), Some(2901), Some(4095), Some(108)]
            .into_iter()
            .map(|trace| Point {
                value: Value::default(),
                trace,
            })
            .collect();
        let before: Vec<_> = points.iter().map(|p| path(&traces, p.trace)).collect();
        collect(&mut traces, points.iter_mut());
        assert!(traces.len() < 60);
        assert_eq!(
            before,
            points
                .iter()
                .map(|p| path(&traces, p.trace))
                .collect::<Vec<_>>()
        );
        // Further transitions and another collection use the remapped parents.
        let last = traces.len();
        traces.push(Trace {
            previous: points[2].trace,
            action: 9000,
        });
        points[2].trace = Some(last);
        let mut expected = before;
        expected[2].push(9000);
        collect(&mut traces, points.iter_mut());
        assert_eq!(
            expected,
            points
                .iter()
                .map(|p| path(&traces, p.trace))
                .collect::<Vec<_>>()
        );
        collect(&mut traces, std::iter::empty());
        assert!(traces.is_empty());
    }
}
