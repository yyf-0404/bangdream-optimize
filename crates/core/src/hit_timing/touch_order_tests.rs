use super::*;
use independent::{
    FixedTimeline, IndependentWindows, JointTimeline, WindowObjective, WindowOptions,
};

/// Test oracle: enumerate actual equal-frame permutations, with no geometry,
/// subset merging, machine compression or bound calls from the production DP.
pub(super) fn visit_orders(input: &ReplayInput, mut visit: impl FnMut(&ReplayInput)) {
    fn permute(i: usize, input: &mut ReplayInput, visit: &mut impl FnMut(&ReplayInput)) {
        if i == input.hit_order.len() {
            visit(input);
            return;
        }
        let frame = input.hit_frames[input.hit_order[i]];
        for j in i..input.hit_order.len() {
            if input.hit_frames[input.hit_order[j]] != frame {
                break;
            }
            input.hit_order.swap(i, j);
            permute(i + 1, input, visit);
            input.hit_order.swap(i, j);
        }
    }
    let mut trial = input.clone();
    trial.hit_order = (0..trial.notes.len()).collect();
    trial
        .hit_order
        .sort_unstable_by_key(|&i| trial.hit_frames[i]);
    permute(0, &mut trial, &mut visit);
}

pub(super) fn best_replay(input: &ReplayInput) -> Option<ReplayResult> {
    let mut best: Option<ReplayResult> = None;
    visit_orders(input, |trial| {
        if let Ok(result) = replay(trial) {
            if best.as_ref().is_none_or(|old| result.total > old.total) {
                best = Some(result);
            }
        }
    });
    best
}

fn fixture(score: ScoreEffect) -> ReplayInput {
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 18,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        notes: [0., 0.125, 0.2]
            .into_iter()
            .enumerate()
            .map(|(i, time)| ReplayNote {
                time,
                window: NoteWindow::Standard,
                trigger: (i == 0).then_some(0),
            })
            .collect(),
        hit_frames: vec![0, 24, 24],
        skills: vec![ReplaySkill {
            duration: 1.,
            score,
            correction: None,
        }],
        native_time_routing: Some(NativeTimeRouting {
            distance_mode: Default::default(),
            music_positions: (0..=24).map(|f| f as f32 * 100.).collect(),
            targets: [0, 24, 24]
                .into_iter()
                .enumerate()
                .map(|(i, frame)| {
                    Some(NativeTimeTarget {
                        button: i as u8,
                        absolute_pos: (frame * 100) as i32,
                        active_from_frame: frame,
                        active_until_frame: frame,
                    })
                })
                .collect(),
            candidate_order: vec![0, 1, 2],
            connections: vec![],
            event_order: vec![],
        }),
    }
}

fn check_all_paths(input: &ReplayInput) -> ReplayResult {
    let expected = best_replay(input).unwrap();
    let power = ScoringPower::new(input.power).unwrap();
    let model = JointTimeline::new(input).unwrap();
    let actual = model.maximum(power).unwrap().result;
    assert_eq!(actual.total, expected.total);
    assert_eq!(model.score(power).unwrap(), expected.total);
    let fixed = FixedTimeline::new(input).unwrap();
    let fixed = fixed
        .prepare(&input.hit_frames)
        .unwrap()
        .unwrap()
        .maximum(power)
        .unwrap()
        .unwrap();
    assert_eq!(fixed.score, expected.total);
    let mut window = IndependentWindows::new(input, WindowOptions::default()).unwrap();
    let one = window
        .maximum(0, &input.skills[0], WindowObjective::Native, power)
        .unwrap();
    assert!(one.complete);
    assert_eq!(one.best.unwrap().native_score, expected.total);
    let mut witness = input.clone();
    witness.hit_frames = actual.hit_frames.clone();
    witness.hit_order = actual.hit_order.clone();
    assert_eq!(replay(&witness).unwrap().total, expected.total);
    let json = serde_json::to_string(&witness).unwrap();
    assert_eq!(
        replay(&serde_json::from_str(&json).unwrap()).unwrap().total,
        expected.total
    );
    actual
}

#[test]
fn continuous_skill_moves_perfect_before_great_and_replays_witness() {
    let r = fixture(ScoreEffect::Continuous {
        high: 150.,
        low: 50.,
    });
    let baseline = replay(&r).unwrap();
    let result = check_all_paths(&r);
    assert!(result.total > baseline.total);
    assert_eq!(result.hit_order, [0, 2, 1]);
    assert_eq!(result.trace[1].skill_multiplier, 2.5);
    assert_eq!(result.trace[2].skill_multiplier, 1.5);
}

#[test]
fn combo_boundary_can_require_great_before_perfect() {
    let mut r = fixture(ScoreEffect::Plain { percent: 0. });
    r.hit_order = vec![0, 2, 1];
    let baseline = replay(&r).unwrap();
    let result = check_all_paths(&r);
    assert!(result.total > baseline.total);
    assert_eq!(result.hit_order, [0, 1, 2]);
}

#[test]
fn all_effects_corrections_fever_and_float_powers_match_permutations() {
    for score in [
        ScoreEffect::Plain { percent: 110. },
        ScoreEffect::PerfectOnly { percent: 120. },
        ScoreEffect::PerfectOrZero { percent: 125. },
        ScoreEffect::PerfectOrHalf { percent: 125. },
        ScoreEffect::Continuous {
            high: 150.,
            low: 50.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 2.5,
            cap: 150,
        },
    ] {
        for correction in [
            None,
            Some(JudgementCorrection::Great),
            Some(JudgementCorrection::Bad),
        ] {
            for power in [2., 300040.75] {
                let mut r = fixture(score);
                r.power = power;
                r.skills[0].correction = correction;
                r.fever_frames = Some([24, 25]);
                check_all_paths(&r);
            }
        }
    }
}

#[test]
fn native_nearest_candidate_still_controls_which_note_each_touch_hits() {
    let mut r = fixture(ScoreEffect::Plain { percent: 0. });
    let route = r.native_time_routing.as_mut().unwrap();
    route.targets[1].as_mut().unwrap().button = 2;
    route.targets[1].as_mut().unwrap().absolute_pos = 1500;
    r.hit_order = vec![0, 2, 1]; // Perfect is nearest and must be consumed first.
    let actual = check_all_paths(&r);
    assert_eq!(actual.hit_order, [0, 2, 1]);
    r.hit_order = vec![0, 1, 2];
    assert!(matches!(replay(&r), Err(ReplayError::InputTarget { .. })));
}

#[test]
fn trigger_cannot_activate_its_skill_for_another_touch_in_the_same_frame() {
    let mut r = fixture(ScoreEffect::Plain { percent: 900. });
    r.notes[0].time = 0.2;
    r.hit_frames[0] = 24;
    let route = r.native_time_routing.as_mut().unwrap();
    let target = route.targets[0].as_mut().unwrap();
    target.active_from_frame = 24;
    target.active_until_frame = 24;
    let result = check_all_paths(&r);
    assert!(result
        .trace
        .iter()
        .all(|t| t.active_skill.is_none() && t.skill_multiplier == 1.));
}

#[test]
fn explicit_order_is_validated_and_overrides_legacy_routing_metadata() {
    let mut r = fixture(ScoreEffect::Plain { percent: 0. });
    r.native_time_routing.as_mut().unwrap().event_order = vec![2, 1, 0];
    r.hit_order = vec![0, 1, 2];
    assert_eq!(replay(&r).unwrap().hit_order, [0, 1, 2]);
    for invalid in [vec![0, 1], vec![0, 0, 2], vec![0, 1, 3]] {
        r.hit_order = invalid;
        assert!(matches!(replay(&r), Err(ReplayError::Input(_))));
    }
}

#[test]
fn same_frame_green_domains_participate_in_permutation_search() {
    let mut r = fixture(ScoreEffect::Continuous {
        high: 150.,
        low: 50.,
    });
    r.native_time_routing = None;
    r.notes[0].window = NoteWindow::NativeGreen {
        first_frames: [0; 4],
        last_frame: 0,
    };
    r.notes[1].window = NoteWindow::NativeGreen {
        first_frames: [24, 24, 24, 25],
        last_frame: 24,
    };
    r.notes[2].window = NoteWindow::NativeGreen {
        first_frames: [24; 4],
        last_frame: 24,
    };
    let result = check_all_paths(&r);
    assert_eq!(result.hit_order, [0, 2, 1]);
    // Position/connection adaptation remains a separate declared coverage gap.
    assert_eq!(result.routing_coverage.unmapped_green_notes, [0, 1, 2]);
}

#[test]
fn multiple_same_frame_notes_and_cached_batches_keep_complete_order_witnesses() {
    use super::compiled::{TimingCompileCache, TimingScoreScratch};
    for score in [
        ScoreEffect::Continuous {
            high: 150.,
            low: 40.,
        },
        ScoreEffect::Crescendo {
            percent: 30.,
            step: 0.5,
            cap: 30,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 2.5,
            cap: 150,
        },
    ] {
        let mut r = fixture(score);
        for (i, grade) in [false, true, false].into_iter().enumerate() {
            r.notes.push(ReplayNote {
                time: if grade { 0.2 } else { 0.125 },
                trigger: None,
                window: NoteWindow::Standard,
            });
            r.hit_frames.push(24);
            let route = r.native_time_routing.as_mut().unwrap();
            route.candidate_order.push(3 + i);
            route.targets.push(Some(NativeTimeTarget {
                button: (3 + i) as u8,
                absolute_pos: 2400,
                active_from_frame: 24,
                active_until_frame: 24,
            }));
        }
        let mut cache = TimingCompileCache::new(&r, search::SearchOptions::default());
        let bank = cache.prepare(&r.skills).unwrap();
        for p in [2., 300040.75] {
            r.power = p;
            let expected = best_replay(&r).unwrap();
            let power = ScoringPower::new(p).unwrap();
            assert_eq!(
                bank.score(power, &mut TimingScoreScratch::default())
                    .unwrap(),
                expected.total
            );
            let actual = bank.materialize(power).unwrap();
            r.hit_frames = actual.hit_frames;
            r.hit_order = actual.hit_order;
            assert_eq!(replay(&r).unwrap().total, expected.total);
        }
    }
}

#[test]
fn causal_zero_gap_is_not_a_free_same_frame_permutation() {
    use super::compiled::{TimingCompileCache, TimingScoreScratch};
    let mut r = fixture(ScoreEffect::Continuous {
        high: 150.,
        low: 50.,
    });
    let unrestricted = check_all_paths(&r);
    r.constraints.push(NoteConstraint {
        before: 1,
        after: 2,
        minimum_gap_frames: 0,
        require_dispatch_order: true,
        earliest_after: None,
    });
    let expected = replay(&r).unwrap();
    assert!(unrestricted.total > expected.total);
    let actual = check_all_paths(&r);
    assert_eq!(actual.hit_order, [0, 1, 2]);
    assert_eq!(actual.total, expected.total);
    let mut cache = TimingCompileCache::new(&r, search::SearchOptions::default());
    let bank = cache.prepare(&r.skills).unwrap();
    let power = ScoringPower::new(r.power).unwrap();
    for _ in 0..2 {
        assert_eq!(
            bank.score(power, &mut TimingScoreScratch::default())
                .unwrap(),
            expected.total
        );
        assert_eq!(bank.materialize(power).unwrap().hit_order, [0, 1, 2]);
    }
    r.hit_order = vec![0, 2, 1];
    assert!(replay(&r).is_err());
    r.constraints[0].require_dispatch_order = false;
    assert_eq!(replay(&r).unwrap().total, unrestricted.total);
    let legacy: NoteConstraint =
        serde_json::from_str(r#"{"before":1,"after":2,"minimumGapFrames":0}"#).unwrap();
    assert!(!legacy.require_dispatch_order);
}

#[test]
fn compact_green_chain_enforces_separate_frames_in_all_search_paths() {
    use super::green::compact::{compile_chain, GreenChainNode};
    let mut r = fixture(ScoreEffect::Continuous {
        high: 150.,
        low: 50.,
    });
    r.native_time_routing = None;
    r.notes[0].window = NoteWindow::NativeGreen {
        first_frames: [0; 4],
        last_frame: 0,
    };
    r.notes[1].window = NoteWindow::NativeGreen {
        first_frames: [23, 23, 23, 25],
        last_frame: 24,
    };
    r.notes[2].window = NoteWindow::NativeGreen {
        first_frames: [24; 4],
        last_frame: 25,
    };
    r.hit_frames = vec![0, 23, 24];
    let c = compile_chain(&[0, 1, 2].map(|i| GreenChainNode {
        note_index: i,
        window: r.notes[i].window,
        minimum_gap_from_previous: 0,
    }))
    .unwrap();
    for &(i, w) in &c.windows {
        r.notes[i].window = w;
    }
    assert!(c.constraints.iter().all(|c| c.minimum_gap_frames == 1));
    r.constraints = c.constraints;
    let actual = check_all_paths(&r);
    assert_eq!(actual.hit_order, [0, 1, 2]);
    assert!(actual.hit_frames.windows(2).all(|w| w[0] < w[1]));
    r.hit_frames = vec![0, 24, 24];
    assert!(matches!(replay(&r), Err(ReplayError::Constraint { .. })));
}

#[test]
fn diagnostic_search_and_variant_bank_do_not_reverse_causal_dispatch() {
    use super::compiled::{compile, TimingScoreScratch};
    let mut r = fixture(ScoreEffect::Continuous {
        high: 150.,
        low: 50.,
    });
    r.native_time_routing = None;
    r.notes[0].window = NoteWindow::NativeGreen {
        first_frames: [0; 4],
        last_frame: 0,
    };
    r.notes[1].window = NoteWindow::NativeGreen {
        first_frames: [23, 23, 23, 25],
        last_frame: 24,
    };
    r.notes[2].window = NoteWindow::NativeGreen {
        first_frames: [24; 4],
        last_frame: 25,
    };
    r.hit_frames = vec![0, 23, 24];
    r.hit_order = vec![0, 2, 1]; // Legal across frames, illegal if both move to frame 24.
    r.constraints.push(NoteConstraint {
        before: 1,
        after: 2,
        minimum_gap_frames: 0,
        require_dispatch_order: true,
        earliest_after: None,
    });
    let power = ScoringPower::new(r.power).unwrap();
    let expected = JointTimeline::new(&r)
        .unwrap()
        .maximum(power)
        .unwrap()
        .result
        .total;
    for complete_ordinary in [false, true] {
        let options = search::SearchOptions {
            complete_search: false,
            complete_ordinary,
            max_rounds: 2,
            max_refine_passes: 2,
            max_evaluations: 100,
        };
        let result = search::optimize(&r, options).unwrap();
        assert!(result.result.total <= expected);
        let bank = compile(&r, options).unwrap().candidates;
        let replayed = bank.materialize(power).unwrap();
        assert!(replayed.total <= expected);
        assert_eq!(
            bank.score(power, &mut TimingScoreScratch::default())
                .unwrap(),
            replayed.total
        );
        let mut witness = r.clone();
        witness.hit_frames = replayed.hit_frames;
        witness.hit_order = replayed.hit_order;
        assert_eq!(replay(&witness).unwrap().total, replayed.total);
    }
}
