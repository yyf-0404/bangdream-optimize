use crate::timing_chart::time_distance::{TimeDistanceChart, TimeDistanceScoring};
use bangdream_optimize_core::hit_timing::{independent::JointTimeline, replay, ReplayInput};
use serde_json::json;

fn input() -> ReplayInput {
    TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":120},
        {"type":"Single","beat":1,"lane":1},
        {"type":"Single","beat":2,"lane":2},
        {"type":"Single","beat":3,"lane":3}
    ]))
    .unwrap()
    .input(TimeDistanceScoring {
        power: 300040.75,
        play_level: 27,
        skills: vec![],
        initial_combo: 0,
        medley_combo: true,
        fever: false,
    })
    .unwrap()
}

#[test]
fn restores_all_free_notes_and_preserves_an_excluded_reference() {
    let mut input = input();
    input.hit_frames = vec![58, 118, 178];
    let original = replay(&input).unwrap();
    let restored =
        JointTimeline::simplest_maximum(&input, &[None, Some([118, 118]), None], &original)
            .unwrap();
    assert_eq!(restored.total, original.total);
    assert_eq!(restored.hit_frames, [60, 118, 180]);
    input.hit_frames = restored.hit_frames.clone();
    input.hit_order = restored.hit_order.clone();
    assert_eq!(replay(&input).unwrap(), restored);
}

#[test]
fn group_restoration_handles_a_connection_that_blocks_the_first_single_move() {
    let mut input = input();
    input
        .constraints
        .push(bangdream_optimize_core::hit_timing::NoteConstraint {
            before: 0,
            after: 1,
            minimum_gap_frames: 60,
            require_dispatch_order: true,
            earliest_after: None,
        });
    input.hit_frames = vec![58, 118, 178];
    let original = replay(&input).unwrap();
    let mut one = input.clone();
    one.hit_frames[0] = 60;
    assert!(replay(&one).is_err());
    let restored = JointTimeline::simplest_maximum(&input, &[None; 3], &original).unwrap();
    assert_eq!(restored.total, original.total);
    assert_eq!(restored.hit_frames, [60, 120, 180]);
}

#[test]
fn a_forced_great_is_preserved_at_the_highest_allowed_score() {
    let mut input = input();
    input.hit_frames = vec![69, 118, 178]; // First note Great, others Perfect.
    let original = replay(&input).unwrap();
    let restored =
        JointTimeline::simplest_maximum(&input, &[Some([69, 70]), None, None], &original).unwrap();
    assert_eq!(restored.total, original.total);
    assert_eq!(restored.hit_frames, [69, 120, 180]);
    assert_eq!(restored.trace[0].score, original.trace[0].score);
}

#[test]
fn restores_fractional_flick_to_a_legal_zero_offset_without_losing_score() {
    let mut input = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":137},
        {"type":"Slide","connections":[{"beat":2,"lane":0},{"beat":4,"lane":1,"flick":true}]}
    ]))
    .unwrap()
    .input(TimeDistanceScoring {
        power: 300000.,
        play_level: 27,
        skills: vec![],
        initial_combo: 0,
        medley_combo: true,
        fever: false,
    })
    .unwrap();
    let references = input.chart_reference_frames().unwrap();
    assert_eq!(references[1], 211);
    input.hit_frames[1] = 214;
    let before = replay(&input).unwrap();
    let restored = JointTimeline::simplest_maximum(&input, &[None; 2], &before).unwrap();
    assert_eq!(restored.total, before.total);
    assert_eq!(restored.hit_frames, references);
    input.hit_frames = restored.hit_frames.clone();
    input.hit_order = restored.hit_order.clone();
    assert_eq!(replay(&input).unwrap(), restored);
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
    ]))
    .unwrap()
    .input(TimeDistanceScoring {
        power: 300040.75,
        play_level: 27,
        skills: vec![ReplaySkill {
            duration: 5.0,
            score: ScoreEffect::Plain { percent: 100.0 },
            correction: Some(JudgementCorrection::Bad),
        }],
        initial_combo: 0,
        medley_combo: true,
        fever: false,
    })
    .unwrap();
    input.hit_frames = vec![0, 546, 556, 566, 576, 596];
    input
}

#[test]
fn shares_a_boundary_shift_while_respecting_connections_and_same_lane_routing() {
    let mut input = interleaved_connections();
    let original = replay(&input).unwrap();
    let restored = JointTimeline::simplest_maximum(&input, &[None; 6], &original).unwrap();
    assert_eq!(restored.total, original.total);
    assert_eq!(restored.hit_frames, [2, 560, 570, 580, 590, 603]);
    input.hit_frames = restored.hit_frames.clone();
    input.hit_order = restored.hit_order.clone();
    assert_eq!(replay(&input).unwrap(), restored);
    assert_eq!(
        JointTimeline::simplest_maximum(&input, &[None; 6], &restored).unwrap(),
        restored
    );
}

#[test]
fn a_forced_offset_leaves_room_to_restore_the_last_note() {
    let input = interleaved_connections();
    let original = replay(&input).unwrap();
    let limits = [None, None, Some([556, 556]), None, None, None];
    let restored = JointTimeline::simplest_maximum(&input, &limits, &original).unwrap();
    assert_eq!(restored.total, original.total);
    // The pinned long endpoint forces its head to shift too. Only one
    // further move is needed to keep the last note inside the skill.
    let reference = input.chart_reference_frames().unwrap();
    assert_eq!(
        restored
            .hit_frames
            .iter()
            .zip(reference)
            .filter(|(f, r)| **f != *r)
            .count(),
        3
    );
    assert!(restored
        .hit_frames
        .iter()
        .zip(limits)
        .all(|(&f, limit)| limit.is_none_or(|[a, b]| a <= f && f <= b)));
}

fn check_skill_sections(second: u32, expected_moves: [u32; 2], expected_radii: [u32; 2]) {
    use bangdream_optimize_core::hit_timing::{NoteConstraint, ReplaySkill, ScoreEffect};
    let frames = [60, 96, 132, second, second + 36, second + 72];
    let mut notes = vec![json!({"type":"BPM","beat":0,"bpm":120})];
    for (i, frame) in frames.iter().enumerate() {
        notes.push(json!({"type":"Single","beat":f64::from(*frame)/60.,"lane":i,"skill":i%3==0}));
    }
    let skill = ReplaySkill {
        duration: 1.,
        score: ScoreEffect::Plain { percent: 0. },
        correction: None,
    };
    let mut input = TimeDistanceChart::from_bestdori(&json!(notes))
        .unwrap()
        .input(TimeDistanceScoring {
            power: 300040.75,
            play_level: 27,
            skills: vec![skill.clone(), skill],
            initial_combo: 0,
            medley_combo: true,
            fever: false,
        })
        .unwrap();
    for (before, gap) in [(1, 40), (4, 44)] {
        input.constraints.push(NoteConstraint {
            before,
            after: before + 1,
            minimum_gap_frames: gap,
            require_dispatch_order: true,
            earliest_after: None,
        });
    }
    input.hit_frames = frames.to_vec();
    input.hit_frames[1] -= 4;
    input.hit_frames[4] -= 4;
    input.hit_frames[5] += 4;
    let seed = replay(&input).unwrap();
    let result = JointTimeline::simplest_maximum(&input, &[None; 6], &seed).unwrap();
    assert_eq!(result.total, seed.total);
    for i in 0..2 {
        let offsets: Vec<_> = (3 * i..3 * i + 3)
            .map(|n| result.hit_frames[n].abs_diff(frames[n]))
            .collect();
        assert_eq!(
            offsets.iter().filter(|&&d| d > 0).count() as u32,
            expected_moves[i]
        );
        assert_eq!(*offsets.iter().max().unwrap(), expected_radii[i]);
    }
}

#[test]
fn independent_skills_each_minimize_their_own_peak_before_counting_moved_keys() {
    check_skill_sections(600, [2, 2], [2, 4]);
}

#[test]
fn skills_linked_by_cooldown_share_the_peak_and_then_minimize_the_total_keys() {
    check_skill_sections(240, [1, 2], [4, 4]);
}
