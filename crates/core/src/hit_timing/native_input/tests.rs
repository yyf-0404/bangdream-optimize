use super::*;
use crate::hit_timing::*;

fn candidate(id: usize, pos: i32) -> Candidate {
    Candidate {
        id,
        button: 0,
        kind: CandidateKind::Time { absolute_pos: pos },
        world_y: pos as f32,
        finger_id: -1,
        raw: Some(Judgement::Perfect),
    }
}

#[test]
fn nearest_is_native_position_distance_and_list_ties_are_stable() {
    let mut c = vec![candidate(0, 100), candidate(1, 110)];
    assert_eq!(
        dispatch(&c, 0, 106., 0.),
        Dispatch::Hit {
            id: 1,
            raw: Judgement::Perfect
        }
    );
    assert_eq!(
        dispatch(&c, 0, 105., 0.),
        Dispatch::Hit {
            id: 0,
            raw: Judgement::Perfect
        }
    );
    c.reverse();
    assert_eq!(
        dispatch(&c, 0, 105., 0.),
        Dispatch::Hit {
            id: 1,
            raw: Judgement::Perfect
        }
    );
    c[0].button = 1;
    assert_eq!(
        dispatch(&c, 0, 110., 0.),
        Dispatch::Hit {
            id: 0,
            raw: Judgement::Perfect
        }
    );
    // Integer conversion precedes subtraction; the two positions round together.
    assert!(!time_candidate_is_nearer(
        16_777_216,
        16_777_217,
        16_777_216.
    ));
}

#[test]
fn invalid_or_bound_winner_does_not_fall_back() {
    let mut c = vec![candidate(0, 100), candidate(1, 110)];
    c[1].raw = None;
    assert_eq!(dispatch(&c, 0, 109., 0.), Dispatch::NoJudgement { id: 1 });
    c[1].raw = Some(Judgement::Bad);
    c[1].finger_id = 0;
    assert_eq!(dispatch(&c, 0, 109., 0.), Dispatch::FingerBound { id: 1 });
    c[1].finger_id = -1;
    assert_eq!(
        dispatch(&c, 0, 109., 0.),
        Dispatch::Hit {
            id: 1,
            raw: Judgement::Bad
        }
    );
}

#[test]
fn mixed_candidates_use_world_y_and_time_candidate_wins_equal_distance() {
    let mut a = candidate(0, 100);
    a.world_y = -2.;
    let mut b = candidate(1, 1000);
    b.kind = CandidateKind::Position;
    b.world_y = 2.;
    assert_eq!(
        dispatch(&[b.clone(), a.clone()], 0, 900., 0.),
        Dispatch::Hit {
            id: 0,
            raw: Judgement::Perfect
        }
    );
    b.world_y = 1.;
    assert_eq!(
        dispatch(&[a, b], 0, 100., 0.),
        Dispatch::Hit {
            id: 1,
            raw: Judgement::Perfect
        }
    );
}

#[test]
fn position_binding_check_is_not_an_up_front_filter() {
    let mut a = candidate(0, 100);
    a.kind = CandidateKind::Position;
    let mut b = a.clone();
    b.id = 1;
    b.world_y = 0.;
    b.finger_id = 2;
    assert_eq!(
        dispatch(&[b.clone(), a.clone()], 0, 0., 0.),
        Dispatch::Hit {
            id: 0,
            raw: Judgement::Perfect
        }
    );
    assert_eq!(
        dispatch(&[a, b], 0, 0., 0.),
        Dispatch::FingerBound { id: 1 }
    );
}

#[test]
fn connection_recycle_branches_differ_at_and_after_next_node() {
    for current in [4., 5., 6., 10., 11.] {
        assert_eq!(
            connection_head_midpoint_expired(current, 0, 10),
            current > 5.
        );
        assert_eq!(
            connection_after_midpoint_expired(current, 0, 10),
            current > 5. && current < 10.
        );
    }
}

fn request() -> ReplayInput {
    ReplayInput {
        power: 300000.75,
        play_level: 27,
        fps: 120,
        notes: [1., 1.05]
            .into_iter()
            .map(|time| ReplayNote {
                time,
                trigger: None,
                window: NoteWindow::Standard,
            })
            .collect(),
        hit_frames: vec![120, 126],
        skills: vec![],
        initial_combo: 0,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: Some(NativeTimeRouting {
            distance_mode: Default::default(),
            connections: vec![],
            event_order: vec![],
            // Explicit test coordinates: MusicPos=10 * frame; not a chart converter.
            music_positions: (0..=150).map(|f| (f * 10) as f32).collect(),
            targets: [1200, 1260]
                .into_iter()
                .map(|absolute_pos| {
                    Some(NativeTimeTarget {
                        button: 0,
                        absolute_pos,
                        active_from_frame: 100,
                        active_until_frame: 150,
                    })
                })
                .collect(),
            candidate_order: vec![0, 1],
        }),
    }
}

fn chart_time_request(times: [f64; 2]) -> ReplayInput {
    let mut input = request();
    for (note, time) in input.notes.iter_mut().zip(times) {
        note.time = time;
    }
    let route = input.native_time_routing.as_mut().unwrap();
    route.distance_mode = CandidateDistanceMode::ChartTime;
    route.music_positions.clear();
    for (note, target) in input.notes.iter().zip(&mut route.targets) {
        let frames = note.frame_domain(Judgement::Bad).unwrap();
        let target = target.as_mut().unwrap();
        target.absolute_pos = 0;
        target.active_from_frame = frames[0];
        target.active_until_frame = *frames.last().unwrap();
    }
    input
}

#[test]
fn chart_time_routing_selects_nearest_unconsumed_same_lane_note() {
    let mut input = chart_time_request([1., 1.05]);
    input.hit_frames = vec![124, 126];
    assert!(matches!(
        replay(&input),
        Err(ReplayError::InputTarget { index: 0, selected: Some(1), .. })
    ));
    input.hit_frames = vec![120, 120];
    let result = replay(&input).unwrap();
    assert!(!result.routing_coverage.native_coordinates_supplied);
    assert_eq!(result.routing_coverage.time_candidate_notes, 2);
    input.native_time_routing.as_mut().unwrap().targets[0]
        .as_mut().unwrap().active_until_frame -= 1;
    assert!(replay(&input).is_ok());
}

#[test]
fn chart_time_shortened_head_lifetime_rejects_late_hit() {
    let mut input = chart_time_request([1., 1.5]);
    input.hit_frames = vec![120, 180];
    input.native_time_routing.as_mut().unwrap().targets[0]
        .as_mut().unwrap().active_until_frame = 120;
    assert!(replay(&input).is_ok());
    input.hit_frames[0] = 121;
    assert!(matches!(
        replay(&input),
        Err(ReplayError::InputTarget { index: 0, selected: None, .. })
    ));
}

#[test]
fn scored_green_head_does_not_block_a_later_press_while_still_in_bad() {
    let mut input = chart_time_request([1., 1.02]);
    input.hit_frames = vec![120, 121];
    let route = input.native_time_routing.as_mut().unwrap();
    // At frame 121 the already scored head is still in its candidate window
    // and nearer than the later note. Settlement alone removes it.
    route.targets[0].as_mut().unwrap().active_until_frame = 121;
    assert!(replay(&input).is_ok());
}

#[test]
fn shortened_green_head_lifetime_does_not_imply_a_one_frame_chain_gap() {
    let mut input = chart_time_request([1., 1.02]);
    input.hit_frames = vec![121, 121];
    let route = input.native_time_routing.as_mut().unwrap();
    route.targets[1].as_mut().unwrap().button = 1;
    route.targets[0].as_mut().unwrap().active_until_frame = 121;
    assert!(replay(&input).is_ok());
    input.constraints.push(NoteConstraint {
        before: 0,
        after: 1,
        minimum_gap_frames: 1,
        require_dispatch_order: true,
        earliest_after: None,
    });
    assert!(replay(&input).is_err());
}

#[test]
fn chart_time_exact_tie_preserves_candidate_order() {
    let mut input = chart_time_request([0.96875, 1.03125]);
    input.hit_frames = vec![120, 120];
    input.native_time_routing.as_mut().unwrap().candidate_order = vec![1, 0];
    assert!(matches!(
        replay(&input),
        Err(ReplayError::InputTarget { index: 0, selected: Some(1), .. })
    ));
    input.hit_order = vec![1, 0];
    assert!(replay(&input).is_ok());
}

#[test]
fn order_alone_does_not_make_a_late_first_press_reachable() {
    let mut r = request();
    r.hit_frames = vec![124, 126];
    assert!(matches!(
        replay(&r),
        Err(ReplayError::InputTarget {
            index: 0,
            selected: Some(1),
            ..
        })
    ));
    r.native_time_routing = None;
    assert!(replay(&r).is_ok()); // Regression: former replay accepted this plan.
}

#[test]
fn consumed_candidates_stop_interfering_and_two_inputs_may_share_a_frame() {
    let mut r = request();
    r.hit_frames = vec![120, 120];
    let result = replay(&r).unwrap();
    assert_eq!(result.trace.len(), 2);
    assert_eq!(result.routing_coverage.time_candidate_notes, 2);
    // One event per note is explicit; this is not one touch hitting two notes.
    r.hit_frames = vec![123, 126];
    assert!(replay(&r).is_ok());
    r.native_time_routing
        .as_mut()
        .unwrap()
        .candidate_order
        .reverse();
    assert!(matches!(
        replay(&r),
        Err(ReplayError::InputTarget { index: 0, .. })
    ));
}

#[test]
fn selected_target_can_precede_an_unconsumed_earlier_note() {
    let mut r = request();
    r.hit_frames = vec![126, 124];
    let result = replay(&r).unwrap();
    assert_eq!(
        result
            .trace
            .iter()
            .map(|row| row.note_index)
            .collect::<Vec<_>>(),
        [1, 0]
    );
    assert_eq!(result.trace[0].raw, Judgement::Perfect);
    assert_eq!(result.trace[1].raw, Judgement::Great);
}

#[test]
fn ordinary_router_matches_native_dispatch_after_each_press() {
    // Compare the replay adapter with the native selector at each actual press,
    // retaining/removing candidates according to successful settlement. Include
    // reverse plans, exact ties, per-button lists and late list activation.
    for reverse_list in [false, true] {
        for second_button in [0, 1] {
            for second_from in [100, 125] {
                let mut r = request();
                let routing = r.native_time_routing.as_mut().unwrap();
                routing.targets[1].as_mut().unwrap().button = second_button;
                routing.targets[1].as_mut().unwrap().active_from_frame = second_from;
                if reverse_list {
                    routing.candidate_order.reverse();
                }
                let mut router = crate::hit_timing::routing::Router::new(&r);
                for a in 115..=135 {
                    for b in 115..=135 {
                        router.reset();
                        let mut consumed = [false; 2];
                        let mut events = [(a, 0), (b, 1)];
                        events.sort_unstable();
                        let routing = r.native_time_routing.as_ref().unwrap();
                        for (frame, note) in events {
                            let candidates = routing
                                .candidate_order
                                .iter()
                                .filter_map(|&i| {
                                    let t = routing.targets[i].as_ref().unwrap();
                                    (!consumed[i]
                                        && t.active_from_frame <= frame
                                        && frame <= t.active_until_frame)
                                        .then(|| Candidate {
                                            button: t.button,
                                            ..candidate(i, t.absolute_pos)
                                        })
                                })
                                .collect::<Vec<_>>();
                            let target = routing.targets[note].as_ref().unwrap();
                            let native = dispatch(
                                &candidates,
                                target.button,
                                routing.music_positions[frame as usize],
                                0.,
                            );
                            let selected = match native {
                                Dispatch::Hit { id, .. } => Some(id),
                                Dispatch::Empty => None,
                                _ => panic!("unexpected dispatch: {native:?}"),
                            };
                            let expected = if selected == Some(note) {
                                consumed[note] = true;
                                Ok(())
                            } else {
                                Err(ReplayError::InputTarget {
                                    index: note,
                                    frame,
                                    selected,
                                })
                            };
                            assert_eq!(router.press(note, frame), expected);
                            if expected.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn routing_is_independent_of_correction_and_respects_active_membership() {
    let mut r = request();
    r.skills.push(ReplaySkill {
        duration: 3.,
        score: ScoreEffect::Plain { percent: 100. },
        correction: Some(JudgementCorrection::Bad),
    });
    r.hit_frames = vec![124, 126];
    assert!(matches!(replay(&r), Err(ReplayError::InputTarget { .. })));
    r.native_time_routing.as_mut().unwrap().targets[1]
        .as_mut()
        .unwrap()
        .active_from_frame = 125;
    assert!(replay(&r).is_ok());
}

#[test]
fn invalid_metadata_is_rejected_instead_of_assuming_seconds_or_lane_order() {
    let mut r = request();
    r.native_time_routing.as_mut().unwrap().candidate_order = vec![0, 0];
    assert!(matches!(replay(&r), Err(ReplayError::Input(_))));
    r = request();
    r.native_time_routing.as_mut().unwrap().targets[1] = None;
    assert!(matches!(replay(&r), Err(ReplayError::Input(_))));
    r = request();
    r.native_time_routing.as_mut().unwrap().music_positions[3] = f32::NAN;
    assert!(matches!(replay(&r), Err(ReplayError::Input(_))));
}

#[test]
fn search_keeps_native_routing_on_every_proposal() {
    let mut r = request();
    r.skills = vec![ReplaySkill {
        duration: 3.,
        score: ScoreEffect::Plain { percent: 100. },
        correction: None,
    }];
    r.notes[0].trigger = Some(0);
    let result = search::optimize(&r, search::SearchOptions::default()).unwrap();
    r.hit_frames = result.result.hit_frames;
    assert_eq!(replay(&r).unwrap().total, result.result.total);
    // Exhaust every reachable two-note plan, including ties and order reversal.
    let mut maximum = 0;
    for a in 109..=131 {
        for b in 115..=137 {
            r.hit_frames = vec![a, b];
            if let Ok(value) = replay(&r) {
                maximum = maximum.max(value.total);
            }
        }
    }
    assert_eq!(result.result.total, maximum);
}
