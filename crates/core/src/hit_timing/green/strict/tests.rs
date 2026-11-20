use super::*;
use crate::hit_timing::{replay, JudgementCorrection, ReplayInput, ReplaySkill, ScoreEffect};
use crate::scoring::{Judgement, ScoringPower};
fn window(first_frames: [u32; 4], last_frame: u32) -> NoteWindow {
    NoteWindow::NativeGreen {
        first_frames,
        last_frame,
    }
}
fn raw(w: NoteWindow, f: u32) -> Judgement {
    ReplayNote {
        time: 0.,
        trigger: None,
        window: w,
    }
    .raw_judgement(f)
    .unwrap()
}
fn equal(a: NoteWindow, b: NoteWindow) {
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap()
    );
}

#[test]
fn strict_domain_equals_worst_raw_grade_for_every_small_window_pair() {
    let mut windows = Vec::new();
    for last in 0..=3 {
        for a in 0..=last {
            for b in a..=last + 1 {
                for c in b..=last + 1 {
                    for d in c..=last + 1 {
                        windows.push(window([a, b, c, d], last));
                    }
                }
            }
        }
    }
    for &a in &windows {
        for &b in &windows {
            let strict = intersect_windows([Ok(a), Ok(b)]);
            let exists = (0..=5).any(|f| raw(a, f).min(raw(b, f)) > Judgement::Miss);
            assert_eq!(strict.is_ok(), exists);
            if let Ok(strict) = strict {
                for f in 0..=5 {
                    assert_eq!(raw(strict, f), raw(a, f).min(raw(b, f)));
                }
                equal(strict, intersect_windows([Ok(b), Ok(a)]).unwrap());
                equal(
                    strict,
                    intersect_windows([Ok(strict), Ok(a), Ok(b)]).unwrap(),
                );
            }
        }
    }
}
#[test]
fn strict_earliest_latest_and_absent_grade_are_merged_separately() {
    let a = window([10, 12, 14, 18], 20);
    let b = window([11, 13, 15, 16], 17);
    let merged = intersect_windows([Ok(a), Ok(b)]).unwrap();
    equal(merged, window([11, 13, 15, 18], 17)); // Perfect absent; Great remains.
    assert_eq!(raw(merged, 17), Judgement::Great);
    assert_eq!(raw(merged, 18), Judgement::Miss);
}
#[test]
fn bad_remains_correctable_but_empty_or_invalid_inputs_are_never_relaxed() {
    equal(
        intersect_windows([Ok(window([2, 3, 3, 3], 2)), Ok(window([1, 3, 4, 5], 4))]).unwrap(),
        window([2, 3, 3, 3], 2),
    );
    assert!(intersect_windows([]).is_err());
    assert!(intersect_windows([Ok(window([1; 4], 2)), Ok(window([3; 4], 4))]).is_err());
    assert!(intersect_windows([Ok(NoteWindow::Standard)]).is_err());
    assert!(intersect_windows([Ok(window([2, 1, 3, 4], 4))]).is_err());
    assert!(intersect_windows([
        Ok(window([0; 4], 2)),
        Err(ReplayError::Input("missing native lifetime"))
    ])
    .is_err());
}
fn chain(w: [NoteWindow; 3], gaps: [u32; 2]) -> Vec<GreenChainNode> {
    w.into_iter()
        .enumerate()
        .map(|(i, window)| GreenChainNode {
            note_index: i,
            window,
            minimum_gap_from_previous: if i == 0 { 0 } else { gaps[i - 1] },
        })
        .collect()
}
fn accepts(nodes: &[GreenChainNode], frames: [u32; 3]) -> bool {
    nodes
        .iter()
        .enumerate()
        .all(|(i, n)| raw(n.window, frames[i]) > Judgement::Miss)
        && (1..3).all(|i| frames[i - 1] + nodes[i].minimum_gap_from_previous.max(1) <= frames[i])
}
#[test]
fn strict_chain_is_exact_intersection_of_variant_frame_assignments() {
    for shift in 0..=3 {
        for ga in 0..=2 {
            for gb in 0..=2 {
                let a = chain(
                    [
                        window([0, 1, 2, 3], 5),
                        window([1; 4], 6),
                        window([2, 3, 4, 5], 7),
                    ],
                    [ga, 1],
                );
                let b = chain(
                    [
                        window([shift; 4], 4),
                        window([shift + 1; 4], 6),
                        window([shift + 2; 4], 7),
                    ],
                    [1, gb],
                );
                let common = compile_strict_chain([&a, &b]);
                let mut count = 0;
                for x in 0..=8 {
                    for y in 0..=8 {
                        for z in 0..=8 {
                            let f = [x, y, z];
                            let expected = accepts(&a, f) && accepts(&b, f);
                            count += usize::from(expected);
                            let actual = common.as_ref().is_ok_and(|c| {
                                c.windows
                                    .iter()
                                    .all(|&(i, w)| raw(w, f[i]) > Judgement::Miss)
                                    && c.constraints
                                        .iter()
                                        .all(|e| f[e.before] + e.minimum_gap_frames <= f[e.after])
                            });
                            assert_eq!(actual, expected);
                            if actual {
                                for &(i, w) in &common.as_ref().unwrap().windows {
                                    assert_eq!(
                                        raw(w, f[i]),
                                        raw(a[i].window, f[i]).min(raw(b[i].window, f[i]))
                                    );
                                }
                            }
                        }
                    }
                }
                assert_eq!(common.is_ok(), count > 0);
            }
        }
    }
}
#[test]
fn strict_chain_rejects_changed_identity_and_joint_infeasibility() {
    let a = chain(
        [window([0; 4], 3), window([1; 4], 4), window([5; 4], 8)],
        [1, 1],
    );
    let b = chain(
        [window([3; 4], 4), window([4; 4], 6), window([7; 4], 8)],
        [2, 1],
    );
    assert!(compile_chain(&a).is_ok() && compile_chain(&b).is_ok());
    assert!(compile_strict_chain([&a, &b]).is_err());
    let mut wrong = a.clone();
    wrong[1].note_index = 99;
    assert!(compile_strict_chain([&a, &wrong]).is_err());
    let empty: Vec<Vec<GreenChainNode>> = vec![];
    assert!(compile_strict_chain(empty).is_err());
}
#[test]
fn strict_chain_search_replays_the_declared_worst_grade_model() {
    use crate::hit_timing::independent::JointTimeline;
    let a = chain(
        [
            window([24, 25, 26, 27], 30),
            window([28; 4], 33),
            window([31, 32, 33, 34], 36),
        ],
        [1, 1],
    );
    let b = chain(
        [
            window([25, 26, 27, 28], 29),
            window([29; 4], 32),
            window([30, 31, 32, 33], 35),
        ],
        [1, 1],
    );
    let merged = compile_strict_chain([&a, &b]).unwrap();
    for score in [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. },
        ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::PerfectOrHalf { percent: 135. },
        ScoreEffect::Continuous {
            high: 150.,
            low: 80.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 5.,
            cap: 150,
        },
    ] {
        for correction in [None, Some(JudgementCorrection::Bad)] {
            let r = ReplayInput {
                power: 300040.75,
                play_level: 27,
                fps: 120,
                notes: merged
                    .windows
                    .iter()
                    .map(|&(i, window)| ReplayNote {
                        time: [0.225, 0.25, 0.275][i],
                        trigger: (i == 0).then_some(0),
                        window,
                    })
                    .collect(),
                hit_frames: vec![28, 30, 34],
                skills: vec![ReplaySkill {
                    duration: 0.05,
                    score,
                    correction,
                }],
                initial_combo: 19,
                medley_combo: false,
                fever_frames: None,
                constraints: merged.constraints.clone(),
                hit_order: vec![],
                native_time_routing: None,
            };
            let mut best = 0;
            for x in 23..=31 {
                for y in 27..=34 {
                    for z in 29..=37 {
                        let f = [x, y, z];
                        if !accepts(&a, f) || !accepts(&b, f) {
                            continue;
                        }
                        let mut trial = r.clone();
                        trial.hit_frames = f.to_vec();
                        if let Ok(result) = replay(&trial) {
                            best = best.max(result.total);
                        }
                    }
                }
            }
            let solved = JointTimeline::new(&r)
                .unwrap()
                .maximum(ScoringPower::new(r.power).unwrap())
                .unwrap()
                .result;
            assert_eq!(solved.total, best);
            let mut witness = r;
            witness.hit_frames = solved.hit_frames;
            witness.hit_order = solved.hit_order;
            assert_eq!(replay(&witness).unwrap().total, best);
        }
    }
}
