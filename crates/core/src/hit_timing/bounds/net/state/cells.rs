//! A complete same-frame subset closure is idempotent. Events observe grade,
//! weight and history, with no clock or retained timestamp in this relaxation.
use super::*;

pub(super) fn following(events: &[(Event, usize)], start: usize, end: usize)
    -> (usize, u32, u64) {
    let first = &events[start..end];
    let mut cursor = end;
    let mut last = first[0].0.time;
    let mut repetitions = 0;
    while let Some(&(event, _)) = events.get(cursor) {
        if last.checked_add(1) != Some(event.time) { break; }
        let next = cursor + events[cursor..].partition_point(|(e, _)| e.time == event.time);
        let batch = &events[cursor..next];
        if first.len() != batch.len() || first.iter().zip(batch).any(|((a, i), (b, j))|
            i != j || a.rank != b.rank || a.grade != b.grade || a.weight.to_bits() != b.weight.to_bits()) {
            break;
        }
        last = event.time;
        cursor = next;
        repetitions += 1;
    }
    (cursor, last, repetitions)
}

pub(super) fn work(frontier: &[Vec<Point>], batch: &[(Event, usize, usize)]) -> Option<u64> {
    let mut transitions = 0;
    for (mask, row) in frontier.iter().enumerate() {
        // NaN arithmetic need not have an idempotent frontier. The declared
        // chart model is finite; retain the original loop for other inputs.
        if row.iter().any(|p| !p.score.is_finite()) { return None; }
        let available = batch.iter().filter(|&&(_, _, flag)| mask & flag == 0).count();
        transitions += row.len() as u64 * available as u64;
    }
    Some(transitions)
}
