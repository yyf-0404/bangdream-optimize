use super::*;
use crate::hit_timing::{
    search::frame_domains, JudgementCorrection, NativeTimeConnection, NativeTimeRouting,
    NativeTimeTarget, NoteConstraint, NoteWindow, ReplayNote, ScoreEffect,
};

fn skill(score: ScoreEffect) -> ReplaySkill {
    ReplaySkill {
        duration: 0.1,
        score,
        correction: None,
    }
}
fn fixture() -> ReplayInput {
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 19,
        medley_combo: false,
        fever_frames: Some([37, 54]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
        notes: vec![
            ReplayNote {
                time: 0.25,
                trigger: Some(0),
                window: NoteWindow::Standard,
            },
            ReplayNote {
                time: 0.34,
                trigger: None,
                window: NoteWindow::Standard,
            },
            ReplayNote {
                time: 0.45,
                trigger: None,
                window: NoteWindow::Standard,
            },
            ReplayNote {
                time: 0.55,
                trigger: None,
                window: NoteWindow::PinnedPerfect { frame: 66 },
            },
        ],
        hit_frames: vec![30, 41, 54, 66],
        skills: vec![skill(ScoreEffect::Plain { percent: 100. })],
    }
}

/// Independent oracle: Cartesian product of old raw frame domains, scored only
/// by native replay. No window intervals, frontier state, run sums or row bounds.
fn brute(input: &ReplayInput) -> (f64, u64, usize) {
    let domains = frame_domains(input).unwrap();
    let mut trial = input.clone();
    let mut best = (f64::NEG_INFINITY, 0, 0);
    fn visit(
        i: usize,
        domains: &[Vec<u32>],
        trial: &mut ReplayInput,
        best: &mut (f64, u64, usize),
    ) {
        if i < domains.len() {
            for &f in &domains[i] {
                trial.hit_frames[i] = f;
                visit(i + 1, domains, trial, best);
            }
            return;
        }
        crate::hit_timing::touch_order_tests::visit_orders(trial, |trial| {
            if let Ok(r) = replay(trial) {
                let scale = f64::from(1.0_f32 + (trial.play_level - 5) as f32 * 0.01_f32)
                    / trial.notes.len() as f64
                    * 3.;
                let coefficient: f64 = r
                    .trace
                    .iter()
                    .map(|n| {
                        scale
                            * f64::from(n.judgement.multiplier())
                            * f64::from(trial.combo_multiplier(n.combo))
                            * f64::from(n.fever_multiplier * n.skill_multiplier)
                    })
                    .sum();
                best.0 = best.0.max(coefficient);
                best.1 = best.1.max(r.total);
                best.2 += 1;
            }
        });
    }
    visit(0, &domains, &mut trial, &mut best);
    best
}
fn check(input: &ReplayInput) -> (WindowMaximum, WindowMaximum) {
    let oracle = brute(input);
    assert!(oracle.2 > 0);
    let mut table = IndependentWindows::new(input, WindowOptions::default()).unwrap();
    let power = ScoringPower::new(input.power).unwrap();
    let continuous = table
        .maximum(0, &input.skills[0], WindowObjective::Continuous, power)
        .unwrap();
    let native = table
        .maximum(0, &input.skills[0], WindowObjective::Native, power)
        .unwrap();
    for r in [&continuous, &native] {
        assert!(r.complete, "{r:?}");
    }
    assert!(
        (continuous.best.as_ref().unwrap().continuous_coefficient - oracle.0).abs() < 1e-10,
        "{:?} != {:?}",
        continuous.best,
        oracle
    );
    assert_eq!(native.best.as_ref().unwrap().native_score, oracle.1);
    assert!(native.stats.graph_hits + native.stats.skipped_requests > 0);
    (continuous, native)
}

#[test]
fn all_effects_match_native_exhaustion_across_window_fever_and_combo_boundaries() {
    for score in [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. },
        ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::PerfectOrHalf { percent: 140. },
        ScoreEffect::Continuous {
            high: 140.,
            low: 60.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 150,
        },
    ] {
        let mut input = fixture();
        input.skills[0] = skill(score);
        check(&input);
    }
}

#[test]
fn corrected_bad_and_trigger_cost_are_scored_in_the_same_plan() {
    let mut input = fixture();
    input.skills[0].correction = Some(JudgementCorrection::Bad);
    input.skills[0].score = ScoreEffect::Crescendo {
        percent: 100.,
        step: 0.5,
        cap: 150,
    };
    let (_, native) = check(&input);
    let r = native.best.unwrap();
    input.hit_frames = r.hit_frames;
    let trace = replay(&input).unwrap().trace;
    assert!(trace
        .iter()
        .find(|n| n.note_index == 0)
        .unwrap()
        .active_skill
        .is_none());
    assert_eq!(trace.len(), input.notes.len());
}

#[test]
fn compressed_and_uncompressed_runs_agree_at_fractional_powers_and_crescendo_cap() {
    let mut input = fixture();
    input.fever_frames = None;
    input.notes = (0..110)
        .map(|i| ReplayNote {
            time: 0.3 + i as f64 * 0.3,
            trigger: (i == 0).then_some(0),
            window: if i == 0 {
                NoteWindow::PinnedPerfect { frame: 36 }
            } else {
                NoteWindow::Standard
            },
        })
        .collect();
    input.hit_frames = input
        .notes
        .iter()
        .map(|n| (n.time * 120.).round() as u32)
        .collect();
    input.skills[0] = ReplaySkill {
        duration: 30.,
        score: ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 102,
        },
        correction: None,
    };
    let mut fast = IndependentWindows::new(&input, WindowOptions::default()).unwrap();
    let mut slow = IndependentWindows::new(
        &input,
        WindowOptions {
            collapse_perfect_runs: false,
            prune_request_frames: false,
            ..WindowOptions::default()
        },
    )
    .unwrap();
    for p in [0.01, 12345.678, 300040.75, 599999.94] {
        for objective in [WindowObjective::Continuous, WindowObjective::Native] {
            let p = ScoringPower::new(p).unwrap();
            let a = fast.maximum(0, &input.skills[0], objective, p).unwrap();
            let b = slow.maximum(0, &input.skills[0], objective, p).unwrap();
            assert!(a.complete && b.complete);
            let av = a.best.unwrap();
            let bv = b.best.unwrap();
            if objective == WindowObjective::Continuous {
                assert!(
                    (av.continuous_coefficient - bv.continuous_coefficient).abs() < 1e-10,
                    "power {p:?}: {av:?} != {bv:?}"
                );
            }
            if objective == WindowObjective::Native {
                assert_eq!(av.native_score, bv.native_score);
            }
            assert!(a.stats.transitions < b.stats.transitions);
        }
    }
}

#[test]
fn native_candidate_order_dispatch_and_constraints_match_exhaustion() {
    let mut input = fixture();
    input.notes[2].time = 0.365;
    input.hit_frames[2] = 44;
    input.constraints = vec![NoteConstraint {
        before: 1,
        after: 2,
        minimum_gap_frames: 0,
        require_dispatch_order: false,
        earliest_after: None,
    }];
    let targets: Vec<_> = input
        .notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            (i < 3).then_some(NativeTimeTarget {
                button: if i == 0 { 1 } else { 2 },
                absolute_pos: (n.time * 100000.).round() as i32,
                active_from_frame: 0,
                active_until_frame: 100,
            })
        })
        .collect();
    input.native_time_routing = Some(NativeTimeRouting {
        distance_mode: Default::default(),
        music_positions: (0..=100).map(|f| f as f32 / 120. * 100000.).collect(),
        targets,
        candidate_order: vec![2, 1, 0],
        connections: vec![],
        event_order: vec![0, 2, 1, 3],
    });
    input.skills[0].score = ScoreEffect::Continuous {
        high: 140.,
        low: 60.,
    };
    check(&input);
}

#[test]
fn connected_endpoint_keeps_binding_and_real_minimum_gap() {
    let mut input = fixture();
    input.notes[1].time = 0.34;
    input.hit_frames[1] = 41;
    input.notes[2].time = 0.45;
    input.notes[2].window = NoteWindow::WidenedEndpoint;
    input.constraints = vec![NoteConstraint {
        before: 1,
        after: 2,
        minimum_gap_frames: 3,
        require_dispatch_order: false,
        earliest_after: None,
    }];
    input.native_time_routing = Some(NativeTimeRouting {
        distance_mode: Default::default(),
        music_positions: (0..=100).map(|f| f as f32 / 120. * 100000.).collect(),
        targets: input
            .notes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                (i < 3).then_some(NativeTimeTarget {
                    button: if i == 0 { 1 } else { 2 },
                    absolute_pos: (n.time * 100000.).round() as i32,
                    active_from_frame: 0,
                    active_until_frame: 100,
                })
            })
            .collect(),
        candidate_order: vec![0, 1],
        connections: vec![NativeTimeConnection {
            head: 1,
            end: 2,
            finger_id: 0,
            release_inside: true,
        }],
        event_order: vec![],
    });
    check(&input);
}

#[test]
fn same_geometry_reuses_graphs_but_never_reuses_the_continuous_winner_at_a_new_power() {
    let mut input = fixture();
    input.skills[0].score = ScoreEffect::Crescendo {
        percent: 100.,
        step: 0.5,
        cap: 150,
    };
    let mut table = IndependentWindows::new(&input, WindowOptions::default()).unwrap();
    for p in [13.37, 300040.75] {
        input.power = p;
        let oracle = brute(&input);
        let result = table
            .maximum(
                0,
                &input.skills[0],
                WindowObjective::Native,
                ScoringPower::new(p).unwrap(),
            )
            .unwrap();
        assert_eq!(result.best.unwrap().native_score, oracle.1);
    }
    let mut other = input.skills[0].clone();
    other.score = ScoreEffect::Plain { percent: 80. };
    let result = table
        .maximum(
            0,
            &other,
            WindowObjective::Native,
            ScoringPower::new(input.power).unwrap(),
        )
        .unwrap();
    input.skills[0] = other;
    assert_eq!(result.best.unwrap().native_score, brute(&input).1);
    assert!(result.stats.graph_hits > 0);
}

#[test]
fn resource_limits_report_unresolved_upper_instead_of_claiming_a_maximum() {
    let input = fixture();
    let mut table = IndependentWindows::new(
        &input,
        WindowOptions {
            max_graph_edges: 1,
            ..WindowOptions::default()
        },
    )
    .unwrap();
    let result = table
        .maximum(
            0,
            &input.skills[0],
            WindowObjective::Native,
            ScoringPower::new(input.power).unwrap(),
        )
        .unwrap();
    assert!(!result.complete);
    assert!(result.best.is_some());
    assert_eq!(
        result.best.as_ref().unwrap().native_score,
        replay(&input).unwrap().total
    );
    assert!(result.stats.budget_requests > 0);
    assert!(result.upper_bound.unwrap() >= brute(&input).1 as f64);
}

#[test]
fn equal_base_cap_rounding_disables_only_the_unproven_greedy_run() {
    let mut input = fixture();
    input.skills[0].score = ScoreEffect::Crescendo {
        percent: 30.,
        step: 0.5,
        cap: 30,
    };
    check(&input);
}

#[test]
fn integer_maximum_can_choose_a_different_plan_than_the_continuous_champion() {
    let mut input = fixture();
    input.notes.truncate(2);
    input.hit_frames.truncate(2);
    input.notes[0].window = NoteWindow::PinnedPerfect { frame: 30 };
    input.notes[1].time = 0.43;
    input.hit_frames[1] = 52;
    input.initial_combo = 0;
    input.fever_frames = None;
    input.power = 2.;
    input.skills[0].score = ScoreEffect::Plain { percent: 40. };
    let (continuous, native) = check(&input);
    let (a, b) = (continuous.best.unwrap(), native.best.unwrap());
    assert!(a.continuous_coefficient > b.continuous_coefficient);
    assert!(a.native_score < b.native_score, "{a:?} {b:?}");
}

#[test]
fn unavoidable_great_keeps_negative_whole_window_gain() {
    let mut input = fixture();
    input.notes.truncate(2);
    input.hit_frames = vec![30, 36];
    input.fever_frames = None;
    input.notes[0].window = NoteWindow::PinnedPerfect { frame: 30 };
    input.notes[1].time = 0.35;
    input.skills[0].score = ScoreEffect::PerfectOrZero { percent: 150. };
    input.native_time_routing = Some(NativeTimeRouting {
        distance_mode: Default::default(),
        music_positions: vec![35000.; 101],
        targets: vec![
            None,
            Some(NativeTimeTarget {
                button: 0,
                absolute_pos: 35000,
                active_from_frame: 36,
                active_until_frame: 36,
            }),
        ],
        candidate_order: vec![1],
        connections: vec![],
        event_order: vec![],
    });
    let (a, b) = check(&input);
    assert!(a.witness_net_coefficient.unwrap() < 0.);
    assert!(b.witness_net_coefficient.unwrap() < 0.);
}

#[test]
fn other_skills_do_not_change_the_independent_window_or_original_trigger_order() {
    let mut input = fixture();
    input.notes[2].trigger = Some(1);
    input
        .skills
        .push(skill(ScoreEffect::Plain { percent: 40. }));
    let mut cache = IndependentWindows::new(&input, WindowOptions::default()).unwrap();
    let power = ScoringPower::new(input.power).unwrap();
    let a = cache
        .maximum(0, &input.skills[0], WindowObjective::Native, power)
        .unwrap();
    input.skills[1].duration = 60.;
    input.skills[1].correction = Some(JudgementCorrection::Bad);
    let mut other = IndependentWindows::new(&input, WindowOptions::default()).unwrap();
    let b = other
        .maximum(0, &input.skills[0], WindowObjective::Native, power)
        .unwrap();
    assert!(a.complete && b.complete);
    assert_eq!(
        a.best.as_ref().unwrap().native_score,
        b.best.as_ref().unwrap().native_score
    );
    let hits = &a.best.as_ref().unwrap().hit_frames;
    assert!((hits[0], 0) < (hits[2], 2));
    assert_eq!(a.reference_coefficient, b.reference_coefficient);
}
