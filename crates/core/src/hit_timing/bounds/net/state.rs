//! Exact state history on the declared independent-note window relaxation.
//! One note must be accepted once, unless its domain permits an external hit. Combo/routing
//! and connection feasibility remain relaxed; this is NOT a playable optimum.
use super::*;
mod cells;

const MAX_FRONTIER: usize = 12;
const MAX_TRANSITIONS: u64 = 2_000_000;

#[derive(Clone, Copy)]
pub(super) struct Event {
    pub time: u32,
    pub rank: usize,
    pub grade: usize,
    pub weight: f64,
}

#[derive(Clone)]
pub(super) struct StateNote {
    pub baseline: f64,
    pub outside: bool,
    pub events: Vec<Event>,
}

pub(super) struct Machine {
    values: Vec<[f32; 2]>,
    next: Vec<[usize; 2]>,
    initial: usize,
    ordered: bool,
}
impl Machine {
    pub fn new(skill: &ReplaySkill, count: usize) -> Result<Option<Self>, ScoringError> {
        let mut state = SkillState::new(skill)?;
        match skill.score {
            ScoreEffect::Continuous { .. } => {
                let high = state.clone().multiplier(Judgement::Perfect);
                let low = state.multiplier(Judgement::Great);
                Ok(Some(Self {
                    values: vec![[low, low], [low, high]],
                    next: vec![[0, 0], [0, 1]],
                    initial: 1,
                    ordered: true,
                }))
            }
            ScoreEffect::Crescendo { .. } => {
                let values: Vec<_> = (0..=count)
                    .map(|_| {
                        let great = state.clone().multiplier(Judgement::Great);
                        let perfect = state.multiplier(Judgement::Perfect);
                        [great, perfect]
                    })
                    .collect();
                // Equal base/cap may cause the first native increment to round
                // DOWN. In that case only merge exactly equal count states.
                let ordered = values
                    .windows(2)
                    .all(|v| (0..2).all(|g| v[1][g] >= v[0][g]));
                Ok(Some(Self {
                    values,
                    next: (0..=count).map(|n| [n, (n + 1).min(count)]).collect(),
                    initial: 0,
                    ordered,
                }))
            }
            _ => Ok(None),
        }
    }
}

#[derive(Clone, Copy)]
struct Point {
    state: usize,
    score: f64,
}

fn insert(frontier: &mut Vec<Point>, value: Point, ordered: bool) {
    if frontier.iter().any(|p| {
        (p.state == value.state || ordered && p.state >= value.state) && p.score >= value.score
    }) {
        return;
    }
    frontier.retain(|p| {
        !((p.state == value.state || ordered && p.state <= value.state) && p.score <= value.score)
    });
    frontier.push(value);
}

fn erase_bit(mask: usize, bit: usize) -> usize {
    (mask & ((1 << bit) - 1)) | ((mask >> (bit + 1)) << bit)
}

/// Only the maximum over request frames is required. The original envelope
/// bounds each refined row, so rows no larger than the current refined maximum
/// cannot change the result. This skips work without approximating the table.
pub(super) fn maximum_profile(
    envelopes: &[f64],
    machine: &Machine,
    stats: &mut TimingNetStats,
    mut build: impl FnMut(usize) -> Vec<StateNote>,
) -> f64 {
    let mut order: Vec<_> = (0..envelopes.len()).collect();
    order.sort_unstable_by(|&a, &b| envelopes[b].total_cmp(&envelopes[a]).then(a.cmp(&b)));
    // The pool's power linearization requires nonnegative gain bounds. This
    // clamp is a relaxation of the signed local maximum, NOT a playable skip.
    let mut best = 0_f64;
    for i in order {
        let envelope = envelopes[i];
        if envelope <= best {
            stats.state_skipped_rows += 1;
            continue;
        }
        let value = match maximum(&build(i), machine, stats) {
            Some(value) => {
                stats.state_tightened_rows += u64::from(value < envelope - 1e-12);
                envelope.min(value)
            }
            None => envelope,
        };
        best = best.max(value);
    }
    best
}

/// On a budget limit discard the partial DP, never its unfinished maximum.
/// The caller keeps the existing whole-domain upper for that request row.
pub(super) fn maximum(
    notes: &[StateNote],
    machine: &Machine,
    stats: &mut TimingNetStats,
) -> Option<f64> {
    let coalesce = std::env::var("BANDORI_TIMING_DISABLE_NET_FRAME_CELLS").as_deref() == Ok("0");
    maximum_controlled(notes, machine, stats, coalesce, MAX_TRANSITIONS)
}

fn maximum_controlled(notes: &[StateNote], machine: &Machine, stats: &mut TimingNetStats,
    coalesce: bool, limit: u64) -> Option<f64> {
    stats.state_rows += 1;
    if notes.iter().any(|n| !n.outside && n.events.is_empty()) {
        stats.state_infeasible_rows += 1;
        return Some(f64::NEG_INFINITY);
    }
    let mut events: Vec<_> = notes
        .iter()
        .enumerate()
        .flat_map(|(i, note)| note.events.iter().map(move |&e| (e, i)))
        .collect();
    events.sort_unstable_by_key(|(e, _)| (e.time, e.rank));
    let mut first = vec![u32::MAX; notes.len()];
    let mut last = vec![0; notes.len()];
    for &(event, note) in &events {
        first[note] = first[note].min(event.time);
        last[note] = last[note].max(event.time);
    }
    let mut active = Vec::<usize>::new();
    let mut dp = vec![vec![Point {
        state: machine.initial,
        score: 0.,
    }]];
    let mut transitions = 0;
    let mut cursor = 0;
    while cursor < events.len() {
        let frame = events[cursor].0.time;
        let end = cursor + events[cursor..].partition_point(|(e, _)| e.time == frame);
        let batch = &events[cursor..end];
        for &(_, note) in batch {
            if first[note] == frame && !active.contains(&note) {
                if active.len() == MAX_FRONTIER {
                    stats.state_budget_rows += 1;
                    stats.state_transitions += transitions;
                    return None;
                }
                active.push(note);
                stats.state_max_frontier = stats.state_max_frontier.max(active.len());
                dp.resize_with(dp.len() * 2, Vec::new);
            }
        }
        // Active-note positions remain fixed throughout this frame closure.
        let indexed: Vec<_> = batch.iter().map(|&(event, note)|
            (event, note, 1usize << active.iter().position(|&i| i == note).unwrap())).collect();
        // Setting any absent bit increases the mask, so this is a DAG over
        // consumed subsets. Every same-frame order appears without n! paths.
        for mask in 0..dp.len() {
            for &(event, note, flag) in &indexed {
                if mask & flag != 0 {
                    continue;
                }
                let (lower, upper) = dp.split_at_mut(mask | flag);
                let source = &lower[mask];
                let destination = &mut upper[0];
                for point in source {
                    transitions += 1;
                    stats.state_evaluated_transitions += 1;
                    if transitions > limit {
                        stats.state_budget_rows += 1;
                        stats.state_transitions += transitions;
                        return None;
                    }
                    let gain = ((event.weight
                        * f64::from(machine.values[point.state][event.grade]))
                    .next_up()
                        - notes[note].baseline)
                        .next_up();
                    insert(
                        destination,
                        Point {
                            state: machine.next[point.state][event.grade],
                            score: (point.score + gain).next_up(),
                        },
                        machine.ordered,
                    );
                }
            }
        }
        let mut resume = end;
        let mut retire_frame = frame;
        if coalesce {
            let (next, last_frame, repetitions) = cells::following(&events, cursor, end);
            if repetitions != 0 {
                // A closed subset DAG is idempotent. Keep the old logical
                // transition budget, including the original overflow attempt.
                if let Some(per_frame) = cells::work(&dp, &indexed) {
                    if per_frame != 0 && per_frame > (limit - transitions) / repetitions {
                        stats.state_skipped_frames += (limit - transitions) / per_frame;
                        stats.state_budget_rows += 1;
                        stats.state_transitions += limit + 1;
                        return None;
                    }
                    transitions += per_frame * repetitions;
                    stats.state_skipped_frames += repetitions;
                    resume = next;
                    retire_frame = last_frame;
                }
            }
        }
        for bit in (0..active.len()).rev() {
            let note = active[bit];
            if last[note] != retire_frame {
                continue;
            }
            let flag = 1 << bit;
            let mut next = vec![Vec::new(); dp.len() / 2];
            for (mask, row) in dp.into_iter().enumerate() {
                if mask & flag == 0 && !notes[note].outside {
                    continue;
                }
                for point in row {
                    insert(&mut next[erase_bit(mask, bit)], point, machine.ordered);
                }
            }
            dp = next;
            active.remove(bit);
        }
        cursor = resume;
    }
    stats.state_transitions += transitions;
    stats.state_solved_rows += 1;
    debug_assert!(active.is_empty() && dp.len() == 1);
    let best = dp[0]
        .iter()
        .map(|p| p.score)
        .fold(f64::NEG_INFINITY, f64::max);
    if best == f64::NEG_INFINITY {
        stats.state_infeasible_rows += 1;
    }
    Some(best)
}

#[cfg(test)]
mod tests;
