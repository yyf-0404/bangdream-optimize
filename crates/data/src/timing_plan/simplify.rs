//! Restore chart positions only when a complete native replay retains the exact
//! maximum score. This is a witness preference, not a second scoring model or a
//! claim that the globally smallest changed-note set has been found.
use bangdream_optimize_core::hit_timing::{replay, ReplayError, ReplayInput, ReplayResult};

pub(crate) fn restore_chart_positions(
    input: &ReplayInput,
    limits: &[Option<[u32; 2]>],
    mut best: ReplayResult,
) -> ReplayResult {
    let Ok(references) = input.chart_reference_frames() else { return best; };
    let candidates: Vec<usize> = references.iter().enumerate().filter_map(|(i, &frame)| {
        (frame != best.hit_frames[i] && limits[i].is_none_or(|[a, b]| a <= frame && frame <= b)).then_some(i)
    }).collect();
    let mut request = input.clone();
    best = restore_by_splitting(&mut request, &references, &candidates, best);

    // Splitting by note index can separate a connected pair or same-lane
    // competitors, especially when an essential boundary shift sits nearby.
    // Follow concrete replay conflicts to reunite those notes. Every merge
    // reduces the number of groups, so this is not a search over subsets.
    let remaining: Vec<usize> = candidates.into_iter()
        .filter(|&i| best.hit_frames[i] != references[i]).collect();
    if remaining.len() < 2 { return best; }
    let mut group_of_note = vec![usize::MAX; references.len()];
    for (group, &note) in remaining.iter().enumerate() { group_of_note[note] = group; }
    let mut groups = RestoreGroups::new(&remaining);
    for seed in 0..remaining.len() {
        let mut group = groups.root(seed);
        if groups.attempted[group] { continue; }
        loop {
            groups.attempted[group] = true;
            match restore_group(&mut request, &references, &groups.members[group], &best) {
                Ok(result) => {
                    if result.total == best.total { best = result; }
                    break;
                }
                Err(error) => {
                    let involved = match error {
                        ReplayError::Constraint { index } => {
                            let edge = &input.constraints[index];
                            [edge.before, edge.after]
                        }
                        ReplayError::InputTarget { index, selected: Some(selected), .. } => [index, selected],
                        _ => break,
                    };
                    let mut expanded = false;
                    for note in involved {
                        // A pinned/excluded reference or an already restored
                        // note cannot supply another move to this trial.
                        let other = group_of_note[note];
                        if other == usize::MAX || best.hit_frames[note] == references[note] { continue; }
                        let (merged, changed) = groups.join(group, other);
                        group = merged;
                        expanded |= changed;
                    }
                    if !expanded { break; }
                }
            }
        }
    }
    best
}

fn restore_by_splitting(
    request: &mut ReplayInput,
    references: &[u32],
    candidates: &[usize],
    mut best: ReplayResult,
) -> ReplayResult {
    // A group can be legal even when moving either endpoint alone is illegal
    // (e.g. two connected notes). Try the whole group before splitting it.
    let mut pending = vec![(0, candidates.len())];
    while let Some((first, last)) = pending.pop() {
        if first == last { continue; }
        if let Ok(result) = restore_group(request, references, &candidates[first..last], &best) {
            if result.total == best.total {
                best = result;
                continue;
            }
        }
        if last - first > 1 {
            let middle = first + (last - first) / 2;
            pending.push((middle, last));
            pending.push((first, middle));
        }
    }
    best
}

fn restore_group(
    request: &mut ReplayInput,
    references: &[u32],
    group: &[usize],
    best: &ReplayResult,
) -> Result<ReplayResult, ReplayError> {
    request.hit_frames.clone_from(&best.hit_frames);
    request.hit_order.clone_from(&best.hit_order);
    for &i in group { request.hit_frames[i] = references[i]; }
    replay(request)
}

struct RestoreGroups {
    parents: Vec<usize>,
    members: Vec<Vec<usize>>,
    attempted: Vec<bool>,
}

impl RestoreGroups {
    fn new(notes: &[usize]) -> Self {
        Self {
            parents: (0..notes.len()).collect(),
            members: notes.iter().map(|&i| vec![i]).collect(),
            attempted: vec![false; notes.len()],
        }
    }

    fn root(&mut self, mut group: usize) -> usize {
        while self.parents[group] != group {
            self.parents[group] = self.parents[self.parents[group]];
            group = self.parents[group];
        }
        group
    }

    fn join(&mut self, first: usize, second: usize) -> (usize, bool) {
        let (mut a, mut b) = (self.root(first), self.root(second));
        if a == b { return (a, false); }
        if self.members[a].len() < self.members[b].len() { std::mem::swap(&mut a, &mut b); }
        self.parents[b] = a;
        let mut members = std::mem::take(&mut self.members[b]);
        self.members[a].append(&mut members);
        (a, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing_chart::time_distance::{TimeDistanceChart, TimeDistanceScoring};
    use serde_json::json;

    fn input() -> ReplayInput {
        TimeDistanceChart::from_bestdori(&json!([
            {"type":"BPM","beat":0,"bpm":120},
            {"type":"Single","beat":1,"lane":1},
            {"type":"Single","beat":2,"lane":2},
            {"type":"Single","beat":3,"lane":3}
        ])).unwrap().input(TimeDistanceScoring {power:300040.75,play_level:27,
            skills:vec![],initial_combo:0,medley_combo:true,fever:false}).unwrap()
    }

    #[test]
    fn restores_all_free_notes_and_preserves_an_excluded_reference() {
        let mut input = input();
        input.hit_frames = vec![58, 118, 178];
        let original = replay(&input).unwrap();
        let restored = restore_chart_positions(&input, &[None, Some([118,118]), None], original.clone());
        assert_eq!(restored.total, original.total);
        assert_eq!(restored.hit_frames, [60,118,180]);
        input.hit_frames = restored.hit_frames.clone();
        input.hit_order = restored.hit_order.clone();
        assert_eq!(replay(&input).unwrap(), restored);
    }

    #[test]
    fn group_restoration_handles_a_connection_that_blocks_the_first_single_move() {
        let mut input = input();
        input.constraints.push(bangdream_optimize_core::hit_timing::NoteConstraint {
            before:0, after:1, minimum_gap_frames:60, require_dispatch_order:true, earliest_after:None,
        });
        input.hit_frames = vec![58,118,178];
        let original = replay(&input).unwrap();
        let mut one = input.clone(); one.hit_frames[0] = 60;
        assert!(replay(&one).is_err());
        let restored = restore_chart_positions(&input, &[None;3], original.clone());
        assert_eq!(restored.total, original.total);
        assert_eq!(restored.hit_frames, [60,120,180]);
    }

    #[test]
    fn a_score_change_is_never_accepted_even_if_all_references_are_legal() {
        let mut input = input();
        input.hit_frames = vec![69,118,178]; // First note Great, others Perfect.
        let original = replay(&input).unwrap();
        let restored = restore_chart_positions(&input, &[None;3], original.clone());
        assert_eq!(restored.total, original.total);
        assert_eq!(restored.hit_frames, [69,120,180]);
        assert_eq!(restored.trace[0].score, original.trace[0].score);
    }

    #[test]
    fn restores_fractional_flick_to_a_legal_zero_offset_without_losing_score() {
        let mut input=TimeDistanceChart::from_bestdori(&json!([
            {"type":"BPM","beat":0,"bpm":137},
            {"type":"Slide","connections":[{"beat":2,"lane":0},{"beat":4,"lane":1,"flick":true}]}
        ])).unwrap().input(TimeDistanceScoring {power:300000.,play_level:27,
            skills:vec![],initial_combo:0,medley_combo:true,fever:false}).unwrap();
        let references=input.chart_reference_frames().unwrap();
        assert_eq!(references[1],211);
        input.hit_frames[1]=214;
        let before=replay(&input).unwrap();
        let restored=restore_chart_positions(&input,&[None;2],before.clone());
        assert_eq!(restored.total,before.total);
        assert_eq!(restored.hit_frames,references);
        input.hit_frames=restored.hit_frames.clone(); input.hit_order=restored.hit_order.clone();
        assert_eq!(replay(&input).unwrap(),restored);
    }

    fn interleaved_connections() -> ReplayInput {
        use bangdream_optimize_core::hit_timing::{JudgementCorrection, ReplaySkill, ScoreEffect};
        let mut input = TimeDistanceChart::from_bestdori(&json!([
            {"type":"BPM","beat":0,"bpm":120},
            {"type":"Single","beat":0,"lane":0,"skill":true},
            {"type":"Long","connections":[
                {"beat":28.0/3.0,"lane":2},{"beat":9.5,"lane":2}]},
            {"type":"Long","connections":[
                {"beat":29.0/3.0,"lane":2},{"beat":59.0/6.0,"lane":2}]},
            {"type":"Single","beat":121.0/12.0,"lane":1}
        ])).unwrap().input(TimeDistanceScoring {power:300040.75,play_level:27,
            skills:vec![ReplaySkill {duration:5.0, score:ScoreEffect::Plain {percent:100.0},
                correction:Some(JudgementCorrection::Bad)}],
            initial_combo:0,medley_combo:true,fever:false}).unwrap();
        input.hit_frames = vec![0,546,556,566,576,596];
        input
    }

    #[test]
    fn follows_connection_then_same_lane_conflicts_across_a_failed_split() {
        let mut input = interleaved_connections();
        let original = replay(&input).unwrap();
        let references: Vec<_> = input.notes.iter().map(|n| (n.time*120.).round() as u32).collect();
        // The old binary pass leaves the first three redundant shifts behind.
        let split = restore_by_splitting(&mut input.clone(), &references, &[1,2,3,4,5], original.clone());
        assert_eq!(split.hit_frames, [0,546,556,566,590,596]);
        let mut trial = input.clone();
        trial.hit_frames = split.hit_frames.clone();
        trial.hit_frames[1] = 560;
        assert!(matches!(replay(&trial), Err(ReplayError::Constraint { .. })));
        trial.hit_frames[2] = 570;
        assert!(matches!(replay(&trial), Err(ReplayError::InputTarget {index:3,selected:Some(2),..})));

        let restored = restore_chart_positions(&input, &[None;6], original.clone());
        assert_eq!(restored.total, original.total);
        assert_eq!(restored.hit_frames, [0,560,570,580,590,596]);
        // The last note still needs its skill-boundary shift for the maximum.
        input.hit_frames = restored.hit_frames.clone();
        input.hit_order = restored.hit_order.clone();
        assert_eq!(replay(&input).unwrap(), restored);
        assert_eq!(restore_chart_positions(&input, &[None;6], restored.clone()), restored);
    }

    #[test]
    fn conflict_groups_never_expand_into_a_reference_excluded_by_limits() {
        let input = interleaved_connections();
        let original = replay(&input).unwrap();
        let limits = [None,None,Some([556,556]),None,None,None];
        let restored = restore_chart_positions(&input, &limits, original.clone());
        assert_eq!(restored.total, original.total);
        assert_eq!(restored.hit_frames, [0,546,556,580,590,596]);
        assert!(restored.hit_frames.iter().zip(limits).all(|(&f, limit)|
            limit.is_none_or(|[a,b]| a <= f && f <= b)));
    }
}
