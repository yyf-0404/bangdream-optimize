use super::green::{compile_window, position_grade, GreenRole};
use super::*;
use crate::scoring::ScoringPower;

fn input() -> ReplayInput {
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        notes: vec![
            ReplayNote {
                time: 0.25,
                trigger: Some(0),
                window: NoteWindow::NativeGreen {
                    first_frames: [24, 26, 28, 30],
                    last_frame: 32,
                },
            },
            ReplayNote {
                time: 0.3,
                trigger: None,
                window: NoteWindow::NativeGreen {
                    first_frames: [35; 4],
                    last_frame: 39,
                },
            },
            ReplayNote {
                time: 0.4,
                trigger: None,
                window: NoteWindow::NativeGreen {
                    first_frames: [40, 41, 42, 43],
                    last_frame: 50,
                },
            },
        ],
        hit_frames: vec![30, 36, 48],
        skills: vec![ReplaySkill {
            duration: 0.1,
            score: ScoreEffect::Plain { percent: 100. },
            correction: None,
        }],
        initial_combo: 19,
        medley_combo: false,
        fever_frames: Some([36, 47]),
        constraints: vec![
            NoteConstraint {
                before: 0,
                after: 1,
                minimum_gap_frames: 0,
                require_dispatch_order: true,
                earliest_after: None,
            },
            NoteConstraint {
                before: 1,
                after: 2,
                minimum_gap_frames: 0,
                require_dispatch_order: true,
                earliest_after: None,
            },
        ],
        hit_order: vec![],
        native_time_routing: None,
    }
}

#[test]
fn green_position_grade_preserves_clamp_and_distinct_node_gates() {
    let table = std::array::from_fn(|i| (16 - i) as f32);
    for role in [GreenRole::Head, GreenRole::PlainEnd] {
        for (y, grade) in [
            (16.1, Judgement::Miss),
            (16., Judgement::Bad),
            (15., Judgement::Good),
            (14., Judgement::Great),
            (11., Judgement::Perfect),
            (8., Judgement::Perfect),
            (-100., Judgement::Perfect),
        ] {
            assert_eq!(position_grade(role, &table, y).unwrap(), grade);
        }
    }
    assert_eq!(
        position_grade(GreenRole::HeldMiddle, &table, 10.).unwrap(),
        Judgement::Miss
    );
    assert_eq!(
        position_grade(GreenRole::HeldMiddle, &table, 9.).unwrap(),
        Judgement::Perfect
    );
    assert_eq!(
        position_grade(GreenRole::HeldArrowEnd, &table, 9.).unwrap(),
        Judgement::Miss
    );
    assert_eq!(
        position_grade(GreenRole::HeldArrowEnd, &table, 8.).unwrap(),
        Judgement::Perfect
    );
}

#[test]
fn native_green_lifetime_is_not_clipped_to_ordinary_150ms_scan() {
    let table = std::array::from_fn(|i| (16 - i) as f32);
    let ys = [17., 16., 15., 14., 11., 9., 8., 7., 6.];
    let window = compile_window(GreenRole::PlainEnd, &table, 120, &ys).unwrap();
    let note = ReplayNote {
        time: 0.5,
        trigger: None,
        window,
    };
    assert_eq!(
        note.frame_domain(Judgement::Great).unwrap(),
        (123..=128).collect::<Vec<_>>()
    );
    assert_eq!(note.raw_judgement(129).unwrap(), Judgement::Miss);
    assert!(compile_window(GreenRole::Head, &table, 0, &[11., 14.]).is_err());
    assert!(compile_window(GreenRole::Head, &table, 0, &[]).is_err());
    assert!(compile_window(GreenRole::Head, &table, 72000, &[8., 8.]).is_err());
}

#[test]
fn green_joint_search_matches_all_frame_combinations_for_every_skill_family() {
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
            let mut r = input();
            r.skills[0].score = score;
            r.skills[0].correction = correction;
            let upper = bounds::TimingScoreUpper::new(&r)
                .unwrap()
                .score(
                    ScoringPower::new(r.power).unwrap(),
                    &mut bounds::TimingUpperScratch::default(),
                )
                .unwrap();
            let mut best = 0;
            let mut trial = r.clone();
            // Explicit raw ranges, independent of frame-domain construction.
            for a in 23..=33 {
                for b in 34..=40 {
                    for c in 39..=51 {
                        trial.hit_frames = vec![a, b, c];
                        if let Ok(replayed) = replay(&trial) {
                            best = best.max(replayed.total);
                            assert!(replayed.total <= upper);
                        }
                    }
                }
            }
            let result = search::optimize(&r, search::SearchOptions::default()).unwrap();
            assert!(result.complete);
            assert_eq!(result.result.total, best, "{score:?}, {correction:?}");
            assert!(result.result.total > result.baseline.total);
            assert_ne!(result.result.hit_frames, r.hit_frames);
            assert_eq!(result.result.pinned_note_count, 0);
        }
    }
}

#[test]
fn green_correction_cannot_rescue_before_gate_after_expiry_or_self_trigger() {
    let mut r = input();
    r.skills[0].correction = Some(JudgementCorrection::Bad);
    for frames in [[30, 34, 48], [30, 40, 48], [30, 36, 51], [24, 36, 48]] {
        r.hit_frames = frames.to_vec();
        assert!(replay(&r).is_err(), "{frames:?}");
    }
    r.hit_frames = vec![30, 36, 40];
    let result = replay(&r).unwrap();
    let tail = result.trace.last().unwrap();
    assert_eq!(tail.raw, Judgement::Bad);
    assert_eq!(tail.judgement, Judgement::Perfect);
}

#[test]
fn real_chart_490_same_lane_skill_can_precede_earlier_tap() {
    let mut r = input();
    r.notes = vec![
        ReplayNote {
            time: 90.,
            trigger: None,
            window: NoteWindow::Standard,
        },
        ReplayNote {
            time: 90.12,
            trigger: Some(0),
            window: NoteWindow::Standard,
        },
    ];
    r.hit_frames = vec![10800, 10815];
    r.constraints.clear();
    r.fever_frames = None;
    r.initial_combo = 492;
    r.play_level = 25;
    r.skills[0].duration = 7.;
    r.native_time_routing = Some(NativeTimeRouting {
        distance_mode: Default::default(),
        // Constant-BPM pair: milliseconds are an affine coordinate fixture.
        music_positions: (0..=10840)
            .map(|f| (f as f64 / 120. * 1000.) as f32)
            .collect(),
        targets: [90000, 90120]
            .into_iter()
            .map(|absolute_pos| {
                Some(NativeTimeTarget {
                    button: 6,
                    absolute_pos,
                    active_from_frame: 10770,
                    active_until_frame: 10840,
                })
            })
            .collect(),
        candidate_order: vec![0, 1],
        connections: vec![],
        event_order: vec![0, 1],
    });
    let mut best = 0;
    let mut chronological = 0;
    let mut trial = r.clone();
    for a in 10789..=10812 {
        for b in 10803..=10826 {
            trial.hit_frames = vec![a, b];
            if let Ok(result) = replay(&trial) {
                best = best.max(result.total);
                if a <= b {
                    chronological = chronological.max(result.total);
                }
            }
        }
    }
    assert!(best > chronological);
    let result = search::optimize(&r, search::SearchOptions::default()).unwrap();
    assert!(result.complete);
    assert_eq!(result.result.total, best);
    assert!(result.result.hit_frames[1] < result.result.hit_frames[0]);
    trial.hit_frames = vec![10808, 10807];
    assert!(matches!(
        replay(&trial),
        Err(ReplayError::InputTarget {
            index: 1,
            selected: Some(0),
            ..
        })
    ));
    trial.hit_frames = vec![10809, 10808];
    let result = replay(&trial).unwrap();
    assert_eq!(result.trace[0].note_index, 1);
    assert_eq!(result.trace[1].note_index, 0);
    assert_eq!(result.trace[1].active_skill, Some(0));
}

#[test]
fn native_green_generated_motion_flows_through_search_and_upper_bound() {
    use super::green::compact::{compile_chain, GreenChainNode, GreenFrameProfile};
    use super::green::motion::{judge_table, positions_120, GreenGeometry};
    let geometry = GreenGeometry {
        specific_speed: 10.0,
        start_y: 10.0,
        button_world_y: -5.0,
        button_local_y: -5.0,
    };
    for score in [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. },
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
            let mut r = input();
            r.skills[0].score = score;
            r.skills[0].correction = correction;
            for (i, role, first, elapsed, count) in [
                (0, GreenRole::Head, 24, 0.95, 9),
                (1, GreenRole::HeldMiddle, 35, 1.0, 5),
                (2, GreenRole::PlainEnd, 40, 0.90, 11),
            ] {
                // Identity-parent fixture only: generated world Y equals local Y.
                let table = judge_table(geometry, 0).unwrap();
                let ys = positions_120(geometry, 0., elapsed, count).unwrap();
                let compact =
                    GreenFrameProfile::compile(&table.positions, first, ys.iter().copied())
                        .unwrap()
                        .window(role)
                        .unwrap();
                assert_eq!(
                    serde_json::to_value(compact).unwrap(),
                    serde_json::to_value(
                        compile_window(role, &table.positions, first, &ys).unwrap()
                    )
                    .unwrap()
                );
                r.notes[i].window = compact;
            }
            let chain = compile_chain(
                &(0..r.notes.len())
                    .map(|i| GreenChainNode {
                        note_index: i,
                        window: r.notes[i].window,
                        minimum_gap_from_previous: 0,
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            for (i, w) in chain.windows {
                r.notes[i].window = w;
            }
            r.constraints = chain.constraints;
            let upper = bounds::TimingScoreUpper::new(&r)
                .unwrap()
                .score(
                    ScoringPower::new(r.power).unwrap(),
                    &mut bounds::TimingUpperScratch::default(),
                )
                .unwrap();
            let mut best = 0;
            let mut trial = r.clone();
            // Raw ranges include out-of-lifetime frames; this oracle does not
            // ask the window compiler which frames the search should consider.
            for a in 23..=33 {
                for b in 34..=40 {
                    for c in 39..=51 {
                        trial.hit_frames = vec![a, b, c];
                        if let Ok(value) = replay(&trial) {
                            best = best.max(value.total);
                            assert!(value.total <= upper);
                        }
                    }
                }
            }
            let result = search::optimize(&r, search::SearchOptions::default()).unwrap();
            assert!(result.complete);
            assert_eq!(result.result.total, best, "{score:?}/{correction:?}");
            assert_eq!(result.result.pinned_note_count, 0);
        }
    }
}
