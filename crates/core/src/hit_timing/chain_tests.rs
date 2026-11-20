//! Keep scoring notes after every chained activation. A clock-only test, or a
//! chart ending before the queue drains, cannot detect a lost scoring suffix.
use super::*;
use bounds::{PoolTimingUpper, TimingNetUpper};
use compiled::{TimingCompileCache, TimingScoreScratch};
use independent::WindowOptions;
use search::SearchOptions;

pub(super) fn fixture() -> ReplayInput {
    let skills: Vec<_> = [0.5, 0.35, 0.25, 0.4, 0.3]
        .into_iter()
        .enumerate()
        .map(|(i, duration)| ReplaySkill {
            duration,
            score: match i {
                1 => ScoreEffect::Continuous {
                    high: 160.,
                    low: 60.,
                },
                2 => ScoreEffect::Crescendo {
                    percent: 100.,
                    step: 0.5,
                    cap: 150,
                },
                3 => ScoreEffect::PerfectOrZero { percent: 150. },
                _ => ScoreEffect::Plain { percent: 120. },
            },
            correction: (i == 0).then_some(JudgementCorrection::Bad),
        })
        .collect();
    let mut times: Vec<u32> = (0..6).map(|i| 60 + i * 15).collect();
    times.extend([70, 100, 115]);
    times.extend((160..1100).step_by(17));
    let mut input = ReplayInput {
        power: 300_040.75,
        play_level: 27,
        fps: 120,
        notes: times
            .iter()
            .enumerate()
            .map(|(i, &frame)| ReplayNote {
                time: f64::from(frame) / 120.,
                trigger: (i < 6).then_some(i),
                window: NoteWindow::PinnedPerfect { frame },
            })
            .collect(),
        hit_frames: times,
        skills: (0..6).map(|i| skills[i % 5].clone()).collect(),
        initial_combo: 699,
        medley_combo: true,
        fever_frames: Some([350, 765]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    };
    // This request can move while remaining queued: its hit must not be moved
    // to its actual activation. Two late boundary notes can also move.
    input.notes[2].window = NoteWindow::Standard;
    let starts = starts(&replay(&input).unwrap());
    for slot in [2, 5] {
        let index = (6..input.notes.len())
            .min_by_key(|&i| input.hit_frames[i].abs_diff(starts[slot]))
            .unwrap();
        input.notes[index].window = NoteWindow::Standard;
    }
    input
}

pub(super) fn starts(result: &ReplayResult) -> [u32; 6] {
    let mut starts = [u32::MAX; 6];
    for event in &result.skill_events {
        if let SkillEvent::Start {
            index, after_frame, ..
        } = *event
        {
            starts[index] = after_frame;
        }
    }
    assert!(!starts.contains(&u32::MAX));
    starts
}

#[test]
fn full_chain_changes_all_successors_but_queued_trigger_remains_at_its_hit() {
    let mut input = fixture();
    let original = replay(&input).unwrap();
    let original_starts = starts(&original);
    assert!((1..6).all(|s| original_starts[s] > input.hit_frames[s]));
    for slot in 0..6 {
        assert!(
            original
                .trace
                .iter()
                .any(|n| n.note_index >= 6 && n.active_skill == Some(slot)),
            "the test must score ordinary notes in slot {slot}"
        );
    }
    input.hit_frames[2] += 12;
    let moved = replay(&input).unwrap();
    assert_eq!(starts(&moved), original_starts);
    let trigger = moved.trace.iter().find(|n| n.note_index == 2).unwrap();
    assert_eq!(trigger.frame, input.hit_frames[2]);
    assert_eq!(trigger.raw, Judgement::Good);
    assert_eq!(trigger.judgement, Judgement::Perfect);
    assert_eq!(trigger.active_skill, Some(0));
    input.skills[0].duration = 0.75;
    input.skills[5] = input.skills[0].clone();
    let changed = replay(&input).unwrap();
    let changed_starts = starts(&changed);
    assert_eq!(changed_starts[0], original_starts[0]);
    assert!((1..6).all(|s| changed_starts[s] > original_starts[s]));
    assert!(changed.trace.iter().any(|n| n.active_skill == Some(5)));
}

#[test]
fn full_chain_compiled_cache_and_bounds_cover_late_hits_and_encore() {
    let input = fixture();
    let catalogue = input.skills[..5].to_vec();
    let cap = ScoringPower::new(900_000.).unwrap();
    let bounds =
        TimingNetUpper::new_adjacent_net(&input, &catalogue, cap, WindowOptions::default())
            .unwrap();
    let pool = PoolTimingUpper::new_adjacent_net(&input, &catalogue, cap, WindowOptions::default())
        .unwrap();
    let mut cache = TimingCompileCache::new(
        &input,
        SearchOptions {
            max_evaluations: 4_000,
            ..Default::default()
        },
    );
    let mut scratch = TimingScoreScratch::default();
    for first in [0, 2, 4, 0] {
        let mut changed = input.clone();
        changed.skills = (0..6).map(|s| catalogue[(s + first) % 5].clone()).collect();
        let candidates = cache.prepare(&changed.skills).unwrap();
        let sequence = changed.skills.clone().try_into().unwrap();
        for value in [300_040.75, 524_288.125] {
            let power = ScoringPower::new(value).unwrap();
            let upper = bounds.upper_sequence(&sequence, power).unwrap();
            let choices = std::array::from_fn(|k| pool.choice(k, f64::from(value) / 5.).unwrap());
            let team_upper = pool.upper(&choices).unwrap();
            let result = candidates.materialize(power).unwrap();
            assert_eq!(result.total, candidates.score(power, &mut scratch).unwrap());
            assert!(result.total <= upper && upper <= team_upper);
            changed.power = value;
            changed.hit_frames = result.hit_frames.clone();
            assert_eq!(replay(&changed).unwrap().total, result.total);
            assert!((1..6).all(|s| starts(&result)[s] > result.hit_frames[s]));
        }
    }
    assert!(cache.stats().group_hits > 0);
    for stats in [
        bounds.adjacent_stats().unwrap(),
        pool.adjacent_stats().unwrap(),
    ] {
        assert_eq!(stats.schedule_budget_fallbacks, 0);
        assert_eq!(stats.roster_budget_fallbacks, 0);
        assert_eq!(stats.windows.budget_requests, 0);
    }
}
