use super::*;
use crate::hit_timing::{
    search, JudgementCorrection, NoteConstraint, NoteWindow, ReplayNote, ReplayResult,
};

fn plain() -> ReplaySkill {
    ReplaySkill {
        duration: 0.1,
        score: ScoreEffect::Plain { percent: 100. },
        correction: None,
    }
}

fn profiles() -> Vec<ReplaySkill> {
    let mut skills: Vec<_> = [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 120. },
        ScoreEffect::PerfectOrZero { percent: 125. },
        ScoreEffect::PerfectOrHalf { percent: 125. },
        ScoreEffect::Continuous {
            high: 140.,
            low: 60.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 150,
        },
    ]
    .into_iter()
    .map(|score| ReplaySkill { score, ..plain() })
    .collect();
    for correction in [
        JudgementCorrection::Great,
        JudgementCorrection::Good,
        JudgementCorrection::Bad,
    ] {
        skills.push(ReplaySkill {
            correction: Some(correction),
            ..plain()
        });
    }
    skills
}

fn input(times: &[f64], skills: Vec<ReplaySkill>) -> ReplayInput {
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 18,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
        notes: times
            .iter()
            .enumerate()
            .map(|(i, &time)| ReplayNote {
                time,
                window: NoteWindow::Standard,
                trigger: (i < skills.len()).then_some(i),
            })
            .collect(),
        hit_frames: times.iter().map(|t| (t * 120.).round() as u32).collect(),
        skills,
    }
}

/// Deliberately independent of frontier geometry and effect-state compression.
/// Enumerate every allowed ordinary frame and use the full native replay.
fn exhaustive(input: &ReplayInput, frames: &[u32], movable: &[bool]) -> Option<ReplayResult> {
    fn visit(
        i: usize,
        trial: &mut ReplayInput,
        domains: &[Vec<u32>],
        best: &mut Option<ReplayResult>,
    ) {
        if i == domains.len() {
            if let Ok(result) = replay(trial) {
                if best.as_ref().is_none_or(|old| result.total > old.total) {
                    *best = Some(result);
                }
            }
            return;
        }
        for &frame in &domains[i] {
            trial.hit_frames[i] = frame;
            visit(i + 1, trial, domains, best);
        }
    }
    let mut domains = search::frame_domains(input).unwrap();
    for i in 0..domains.len() {
        if input.notes[i].trigger.is_some() || !movable[i] {
            domains[i] = vec![frames[i]];
        }
    }
    let mut best = None;
    visit(0, &mut input.clone(), &domains, &mut best);
    best
}

fn check(input: &ReplayInput, frames: &[u32], movable: &[bool]) -> Option<FixedTimelineResult> {
    let solver = FixedTimeline::new(input).unwrap();
    let graph = solver.prepare_selected(frames, movable, true).unwrap();
    let result = graph
        .as_ref()
        .and_then(|g| g.maximum(ScoringPower::new(input.power).unwrap()).unwrap());
    let expected = exhaustive(input, frames, movable);
    assert_eq!(
        result.as_ref().map(|r| r.score),
        expected.as_ref().map(|r| r.total),
        "skills {:?}, fixed {frames:?}",
        input.skills
    );
    result
}

#[test]
fn plain_and_crescendo_neutral_move_counterexamples_are_exact() {
    for (time, power, combo, score, expected) in [
        (
            2.9883333333333333,
            300040.75,
            18,
            ScoreEffect::Plain { percent: 100. },
            1476798,
        ),
        (
            2.98,
            1000.,
            48,
            ScoreEffect::Crescendo {
                percent: 100.,
                step: 0.5,
                cap: 150,
            },
            4976,
        ),
    ] {
        let mut request = input(
            &[0., time, 3.08],
            vec![ReplaySkill {
                duration: 3.,
                score,
                ..plain()
            }],
        );
        request.power = power;
        request.play_level = 5;
        request.initial_combo = combo;
        request.notes[0].window = NoteWindow::PinnedPerfect { frame: 0 };
        let result = check(&request, &request.hit_frames, &[false, true, true]).unwrap();
        assert_eq!(result.score, expected);
        assert!(result.hit_frames[2] < result.hit_frames[1]);
    }
}

#[test]
fn all_effects_match_exhaustive_across_queued_windows_combo_and_fever() {
    let skills = profiles();
    for (i, first) in skills.iter().enumerate() {
        for ordinary in [[0.095, 0.13], [0.87, 0.93], [0.98, 1.05], [0.12, 0.91]] {
            let mut request = input(
                &[0., 0.05, ordinary[0], ordinary[1]],
                vec![first.clone(), skills[(i + 5) % skills.len()].clone()],
            );
            request.fever_frames = Some([111, 121]);
            request.initial_combo = if i % 2 == 0 { 18 } else { 48 };
            request.medley_combo = i % 3 == 0;
            check(&request, &request.hit_frames, &[false, false, true, true]).unwrap();
        }
    }
}

#[test]
fn three_ordinary_notes_are_jointly_placed_without_pair_or_pass_limits() {
    for skill in [profiles()[4].clone(), profiles()[5].clone()] {
        let mut request = input(&[0., 0.085, 0.13, 0.155], vec![skill]);
        request.fever_frames = Some([12, 18]);
        request.constraints.push(NoteConstraint {
            before: 2,
            after: 3,
            minimum_gap_frames: 1,
            require_dispatch_order: false,
            earliest_after: None,
        });
        check(&request, &request.hit_frames, &[false, true, true, true]).unwrap();
    }
}

#[test]
fn all_perfect_runs_do_not_merge_skill_slots_and_keep_crescendo_progression() {
    for score in [
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 104,
        },
        // Native equal-cap rounding must disable unproved run dominance.
        ScoreEffect::Crescendo {
            percent: 30.,
            step: 0.5,
            cap: 30,
        },
    ] {
        let skills = vec![
            ReplaySkill {
                duration: 1.5,
                score,
                ..plain()
            },
            plain(),
        ];
        let request = input(&[0., 0.1, 0.3, 0.6, 0.9, 1.2, 2.27, 2.32, 2.5], skills);
        let solver = FixedTimeline::new(&request).unwrap();
        let mask = vec![true; request.notes.len()];
        let compressed = solver
            .prepare_selected(&request.hit_frames, &mask, true)
            .unwrap()
            .unwrap();
        let complete = solver
            .prepare_selected(&request.hit_frames, &mask, false)
            .unwrap()
            .unwrap();
        for power in [0., 1000., 300040.75, 500000.125] {
            let power = ScoringPower::new(power).unwrap();
            assert_eq!(
                compressed.maximum(power).unwrap().unwrap().score,
                complete.maximum(power).unwrap().unwrap().score
            );
        }
    }
}

#[test]
fn graph_reuse_reoptimizes_at_each_native_power() {
    let request = input(&[0., 0.09, 0.155], vec![profiles()[5].clone()]);
    let solver = FixedTimeline::new(&request).unwrap();
    let graph = solver.prepare(&request.hit_frames).unwrap().unwrap();
    for power in [0., 1., 998.125, 1000., 300040., 300040.75, 700001.25] {
        let mut actual = request.clone();
        actual.power = power;
        let expected = exhaustive(&actual, &actual.hit_frames, &[false, true, true]).unwrap();
        assert_eq!(
            graph
                .maximum(ScoringPower::new(power).unwrap())
                .unwrap()
                .unwrap()
                .score,
            expected.total
        );
    }
}

#[test]
fn constraints_can_make_a_changed_fixed_schedule_infeasible() {
    let mut request = input(&[1., 1.], vec![plain()]);
    request.notes[1].window = NoteWindow::PinnedPerfect { frame: 120 };
    request.constraints.push(NoteConstraint {
        before: 0,
        after: 1,
        minimum_gap_frames: 0,
        require_dispatch_order: false,
        earliest_after: None,
    });
    assert!(check(&request, &[121, 120], &[false, true]).is_none());
}

#[test]
fn native_candidate_order_connections_and_bindings_match_exhaustive() {
    for button in [0, 1] {
        let mut request = crate::hit_timing::time_connections::request();
        request.skills = vec![plain()];
        request.notes[0].trigger = Some(0);
        let routing = request.native_time_routing.as_mut().unwrap();
        routing.event_order = vec![0, 2, 1];
        routing.targets[2].as_mut().unwrap().button = button;
        check(&request, &request.hit_frames, &[false, true, true]).unwrap();
    }
}

#[test]
fn a_winner_at_one_power_is_not_reused_as_the_other_power_optimum() {
    let mut request = input(
        &[0.25, 0.43],
        vec![ReplaySkill {
            score: ScoreEffect::Plain { percent: 40. },
            ..plain()
        }],
    );
    request.initial_combo = 0;
    let solver = FixedTimeline::new(&request).unwrap();
    let graph = solver.prepare(&request.hit_frames).unwrap().unwrap();
    let high = graph
        .maximum(ScoringPower::new(300040.75).unwrap())
        .unwrap()
        .unwrap();
    let low = graph
        .maximum(ScoringPower::new(2.).unwrap())
        .unwrap()
        .unwrap();
    request.power = 2.;
    request.hit_frames = high.hit_frames;
    assert!(low.score > replay(&request).unwrap().total);
}

#[test]
fn changed_trigger_can_repair_an_invalid_old_ordinary_position() {
    let mut request = input(
        &[1., 1.],
        vec![ReplaySkill {
            correction: Some(JudgementCorrection::Bad),
            ..plain()
        }],
    );
    request.hit_frames[1] = 132;
    replay(&request).unwrap();
    let mut changed = request.clone();
    changed.hit_frames[0] = 110;
    assert!(replay(&changed).is_err());
    let result = check(&request, &changed.hit_frames, &[false, true]).unwrap();
    assert_ne!(result.hit_frames[1], 132);
}

#[test]
fn queued_trigger_uses_previous_skills_correction_not_its_own() {
    let mut request = input(
        &[0., 0.025, 0.2],
        vec![
            ReplaySkill {
                duration: 0.3,
                correction: Some(JudgementCorrection::Bad),
                ..plain()
            },
            plain(),
        ],
    );
    let moved = [0, 15, 24];
    check(&request, &moved, &[false, false, true]).unwrap();
    request.skills[0].correction = None;
    request.skills[1].correction = Some(JudgementCorrection::Bad);
    assert!(check(&request, &moved, &[false, false, true]).is_none());
}

#[test]
fn slot_labels_do_not_grow_existing_geometry_records() {
    // The six-slot tag fits in the padding beside the existing flags. These
    // records also occur in the much larger contribution/pruning graph caches.
    assert!(std::mem::size_of::<geometry::ActionInterval>() <= 12);
    assert!(std::mem::size_of::<geometry::Factor>() <= 24);
}
