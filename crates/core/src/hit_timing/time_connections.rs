use super::*;

pub(super) fn request() -> ReplayInput {
    ReplayInput {
        power: 300000.75,
        play_level: 27,
        fps: 120,
        notes: [1., 1.1, 1.1]
            .into_iter()
            .enumerate()
            .map(|(i, time)| ReplayNote {
                time,
                trigger: None,
                window: if i == 1 {
                    NoteWindow::WidenedEndpoint
                } else {
                    NoteWindow::Standard
                },
            })
            .collect(),
        hit_frames: vec![120, 128, 132],
        skills: vec![],
        initial_combo: 0,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: Some(NativeTimeRouting {
            distance_mode: Default::default(),
            music_positions: (0..=200).map(|f| (f * 10) as f32).collect(),
            targets: [1200, 1320, 1320]
                .into_iter()
                .map(|absolute_pos| {
                    Some(NativeTimeTarget {
                        button: 0,
                        absolute_pos,
                        active_from_frame: 100,
                        active_until_frame: 200,
                    })
                })
                .collect(),
            candidate_order: vec![0, 2],
            connections: vec![NativeTimeConnection {
                head: 0,
                end: 1,
                finger_id: 0,
                release_inside: true,
            }],
            event_order: vec![],
        }),
    }
}

#[test]
fn release_uses_bound_head_instead_of_nearest_tap() {
    let r = request();
    // At frame 128, the tap is nearer than the held root. Release still scores
    // the bound root's endpoint, never selects the nearby tap.
    let result = replay(&r).unwrap();
    assert_eq!(
        result
            .trace
            .iter()
            .map(|n| n.note_index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(result.routing_coverage.time_connection_count, 1);
    assert!(result.trace.iter().all(|n| n.raw == Judgement::Perfect));
}

#[test]
fn scored_head_leaves_press_candidates_but_keeps_endpoint_binding() {
    let mut r = request();
    r.hit_frames[2] = 122;
    // Deliberately outside the supported chart assumption: prove the optimizer
    // no longer models a held head blocking another press on the same button.
    // The endpoint must still resolve its original binding afterwards.
    let result = replay(&r).unwrap();
    assert_eq!(
        result
            .trace
            .iter()
            .map(|n| n.note_index)
            .collect::<Vec<_>>(),
        [0, 2, 1]
    );
    assert_eq!(result.trace[1].raw, Judgement::Great);
    assert_eq!(result.trace[2].raw, Judgement::Perfect);
}

#[test]
fn held_connections_do_not_restrict_other_lanes() {
    for head_button in 0..7 {
        for tap_button in 0..7 {
            if head_button == tap_button {
                continue;
            }
            let mut r = request();
            r.hit_frames[2] = 122;
            let targets = &mut r.native_time_routing.as_mut().unwrap().targets;
            targets[0].as_mut().unwrap().button = head_button;
            targets[1].as_mut().unwrap().button = head_button;
            targets[2].as_mut().unwrap().button = tap_button;
            let result = replay(&r).unwrap();
            assert_eq!(result.trace[1].note_index, 2);
            assert_eq!(result.trace[2].note_index, 1);
        }
    }
}

#[test]
fn same_frame_press_then_release_has_no_invented_minimum_hold_frame() {
    let mut r = request();
    r.notes[1].time = 1.02;
    r.notes[2].time = 1.04;
    let routing = r.native_time_routing.as_mut().unwrap();
    routing.targets[1].as_mut().unwrap().absolute_pos = 1224;
    routing.targets[2].as_mut().unwrap().absolute_pos = 1248;
    r.hit_frames = vec![120, 120, 120];
    assert!(replay(&r).is_ok());
    r.native_time_routing.as_mut().unwrap().event_order = vec![1, 0, 2];
    assert!(matches!(
        replay(&r),
        Err(ReplayError::InputBinding { index: 1, .. })
    ));
    r.native_time_routing.as_mut().unwrap().event_order = vec![0, 2, 1];
    assert_eq!(
        replay(&r)
            .unwrap()
            .trace
            .iter()
            .map(|n| n.note_index)
            .collect::<Vec<_>>(),
        [0, 2, 1]
    );
}

#[test]
fn release_outside_button_or_after_root_retirement_is_not_repaired_by_skill() {
    let mut r = request();
    r.skills.push(ReplaySkill {
        duration: 3.,
        score: ScoreEffect::Plain { percent: 100. },
        correction: Some(JudgementCorrection::Bad),
    });
    r.notes[0].trigger = Some(0);
    r.native_time_routing.as_mut().unwrap().connections[0].release_inside = false;
    assert!(matches!(
        replay(&r),
        Err(ReplayError::InputBinding { index: 1, .. })
    ));
    let routing = r.native_time_routing.as_mut().unwrap();
    routing.connections[0].release_inside = true;
    routing.targets[0].as_mut().unwrap().active_until_frame = 127;
    assert!(matches!(
        replay(&r),
        Err(ReplayError::InputBinding { index: 1, .. })
    ));
}

#[test]
fn native_binding_is_per_button_and_overwrite_is_observed_on_release() {
    let mut r = request();
    r.notes = [1., 1.4, 1.1, 1.5]
        .into_iter()
        .enumerate()
        .map(|(i, time)| ReplayNote {
            time,
            trigger: None,
            window: if i % 2 == 1 {
                NoteWindow::WidenedEndpoint
            } else {
                NoteWindow::Standard
            },
        })
        .collect();
    r.hit_frames = vec![120, 168, 132, 180];
    let routing = r.native_time_routing.as_mut().unwrap();
    routing.targets = [1200, 1680, 1320, 1800]
        .into_iter()
        .map(|absolute_pos| {
            Some(NativeTimeTarget {
                button: 0,
                absolute_pos,
                active_from_frame: 100,
                active_until_frame: 200,
            })
        })
        .collect();
    routing.connections.push(NativeTimeConnection {
        head: 2,
        end: 3,
        finger_id: 0,
        release_inside: true,
    });
    // The second press can overwrite the button's binding; do not add a global
    // "finger already busy" rejection absent from GamePlayButton.
    assert!(matches!(
        replay(&r),
        Err(ReplayError::InputBinding {
            index: 1,
            frame: 168,
            ..
        })
    ));
    let routing = r.native_time_routing.as_mut().unwrap();
    routing.targets[2].as_mut().unwrap().button = 1;
    routing.targets[3].as_mut().unwrap().button = 1;
    assert!(replay(&r).is_ok());
}

#[test]
fn endpoint_is_not_an_independent_press_candidate_and_metadata_must_match() {
    let mut r = request();
    r.native_time_routing
        .as_mut()
        .unwrap()
        .candidate_order
        .push(1);
    assert!(matches!(replay(&r), Err(ReplayError::Input(_))));
    r = request();
    r.notes[1].window = NoteWindow::Standard;
    assert!(matches!(replay(&r), Err(ReplayError::Input(_))));
    r = request();
    r.native_time_routing.as_mut().unwrap().event_order = vec![0, 0, 2];
    assert!(matches!(replay(&r), Err(ReplayError::Input(_))));
}

#[test]
fn explicit_same_frame_event_order_is_used_by_scoring() {
    let mut r = request();
    r.hit_frames = vec![120, 132, 132];
    r.native_time_routing.as_mut().unwrap().event_order = vec![0, 2, 1];
    let result = replay(&r).unwrap();
    assert_eq!(
        result
            .trace
            .iter()
            .map(|n| n.note_index)
            .collect::<Vec<_>>(),
        [0, 2, 1]
    );
    assert_eq!(
        result.trace.iter().map(|n| n.combo).collect::<Vec<_>>(),
        [1, 2, 3]
    );
}
