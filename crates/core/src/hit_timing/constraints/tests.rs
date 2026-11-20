use super::*;
use crate::hit_timing::{
    compiled, independent::FixedTimeline, replay, search, JudgementCorrection, NoteWindow,
    ReplayInput, ReplayNote, ReplaySkill, ScoreEffect,
};
use crate::scoring::ScoringPower;

#[test]
fn compression_is_lossless_including_holes_nonmonotone_values_and_edges() {
    // Exhaust ALL small tables, not just tables produced by hidden projection.
    for n in 1..=6 {
        for mut code in 0..4usize.pow(n) {
            let samples: Vec<_> = (0..n)
                .map(|_| {
                    let digit = code % 4;
                    code /= 4;
                    (digit != 0).then_some(digit as u32 + 20)
                })
                .collect();
            let curve = ArrivalCurve::from_samples(10, &samples).unwrap();
            assert_eq!(curve.earliest(9), None);
            assert_eq!(curve.earliest(10 + n), None);
            for (i, &sample) in samples.iter().enumerate() {
                assert_eq!(curve.earliest(10 + i as u32), sample);
            }
            assert_eq!(
                serde_json::from_str::<ArrivalCurve>(&serde_json::to_string(&curve).unwrap())
                    .unwrap(),
                curve
            );
        }
    }
    let curve = ArrivalCurve::from_samples(0, &(0..=72000).map(Some).collect::<Vec<_>>()).unwrap();
    assert_eq!(curve.runs.len(), 1);
    assert_eq!(curve.earliest(72000), Some(72000));
}

#[test]
fn malformed_curves_rejected_and_legacy_json_preserved() {
    let legacy: NoteConstraint =
        serde_json::from_str(r#"{"before":0,"after":1,"minimumGapFrames":1}"#).unwrap();
    assert!(legacy.earliest_after.is_none());
    assert!(legacy.allows_frames(10, 11));
    assert!(!legacy.allows_frames(u32::MAX, 0));
    assert!(!serde_json::to_string(&legacy)
        .unwrap()
        .contains("earliestAfter"));
    let base = ArrivalCurve::from_samples(10, &[Some(20), Some(21)]).unwrap();
    let mut bad = base.clone();
    bad.runs.clear();
    assert!(bad.validate().is_err());
    let mut bad = base.clone();
    bad.runs[0].through_frame = 9;
    assert!(bad.validate().is_err());
    let mut bad = base.clone();
    bad.runs[0].first_after_frame = Some(72000);
    assert!(bad.validate().is_err());
    let mut bad = base.clone();
    bad.runs[0].first_after_frame = None;
    assert!(bad.validate().is_err());
    let mut bad = base.clone();
    bad.runs.push(bad.runs[0].clone());
    assert!(bad.validate().is_err());
    assert!(ArrivalCurve::from_samples(72000, &[Some(0), Some(0)]).is_err());
}

fn input() -> ReplayInput {
    let note = |time, first, last, trigger| ReplayNote {
        time: time as f64 / 120.,
        trigger,
        window: NoteWindow::NativeGreen {
            first_frames: [first; 4],
            last_frame: last,
        },
    };
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        notes: vec![
            note(10, 10, 12, Some(0)),
            note(12, 11, 15, None),
            note(14, 12, 16, None),
        ],
        hit_frames: vec![10, 14, 15],
        hit_order: vec![],
        skills: vec![ReplaySkill {
            duration: 0.025,
            score: ScoreEffect::Plain { percent: 100. },
            correction: None,
        }],
        initial_combo: 48,
        medley_combo: false,
        fever_frames: Some([11, 14]),
        constraints: vec![NoteConstraint {
            before: 0,
            after: 1,
            minimum_gap_frames: 1,
            require_dispatch_order: true,
            // Earliest predecessor needs an extra wait; the middle value is a
            // hole. Latest-only redundancy checks would incorrectly discard it.
            earliest_after: Some(
                ArrivalCurve::from_samples(10, &[Some(14), None, Some(13)]).unwrap(),
            ),
        }],
        native_time_routing: None,
    }
}

fn brute(input: &ReplayInput, fixed_trigger: Option<u32>) -> u64 {
    let mut best = 0;
    for a in 10..=12 {
        if fixed_trigger.is_some_and(|f| f != a) {
            continue;
        }
        for b in 11..=15 {
            for c in 12..=16 {
                for order in [
                    vec![0, 1, 2],
                    vec![0, 2, 1],
                    vec![1, 0, 2],
                    vec![1, 2, 0],
                    vec![2, 0, 1],
                    vec![2, 1, 0],
                ] {
                    let mut q = input.clone();
                    q.hit_frames = vec![a, b, c];
                    q.hit_order = order;
                    if let Ok(r) = replay(&q) {
                        best = best.max(r.total);
                    }
                }
            }
        }
    }
    best
}

#[test]
fn projected_constraints_reach_joint_fixed_compiled_and_cached_search() {
    let effects = [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 120. },
        ScoreEffect::PerfectOrZero { percent: 125. },
        ScoreEffect::PerfectOrHalf { percent: 130. },
        ScoreEffect::Continuous {
            high: 140.,
            low: 80.,
        },
        ScoreEffect::Crescendo {
            percent: 95.,
            step: 3.,
            cap: 150,
        },
    ];
    for effect in effects {
        let mut q = input();
        q.skills[0].score = effect;
        // One graded node exercises actual correction before score-state updates.
        q.notes[2].window = NoteWindow::NativeGreen {
            first_frames: [12, 13, 14, 15],
            last_frame: 16,
        };
        q.skills[0].correction = Some(JudgementCorrection::Bad);
        let expected = brute(&q, None);
        let got = search::optimize(&q, search::SearchOptions::default()).unwrap();
        assert!(got.complete);
        assert_eq!(got.result.total, expected, "{effect:?}");
        assert!(q.constraints[0].allows_frames(got.result.hit_frames[0], got.result.hit_frames[1]));
        let fixed = FixedTimeline::new(&q).unwrap();
        for a in [10, 12] {
            let mut frames = q.hit_frames.clone();
            frames[0] = a;
            let got = fixed
                .prepare(&frames)
                .unwrap()
                .unwrap()
                .maximum(ScoringPower::new(q.power).unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(got.score, brute(&q, Some(a)));
        }
        let compiled = compiled::compile(&q, search::SearchOptions::default()).unwrap();
        let mut cache = compiled::TimingCompileCache::new(&q, search::SearchOptions::default());
        for power in [0.125, 300040.75] {
            q.power = power;
            let expected = brute(&q, None);
            let cached = cache.prepare(&q.skills).unwrap();
            for bank in [&compiled.candidates, &cached] {
                let got = bank.materialize(ScoringPower::new(power).unwrap()).unwrap();
                assert_eq!(got.total, expected);
            }
        }
    }
}

#[test]
fn replay_rejects_missing_arrivals_and_out_of_domain_predecessors() {
    let mut q = input();
    for (a, b) in [(10, 13), (11, 14)] {
        q.hit_frames[0] = a;
        q.hit_frames[1] = b;
        assert!(matches!(
            replay(&q),
            Err(ReplayError::Constraint { index: 0 })
        ));
    }
    let c = &q.constraints[0];
    assert!(!c.allows_frames(9, 72000));
    assert!(!c.allows_frames(13, 72000));
}
