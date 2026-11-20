use super::*;
fn ready() -> Facts {
    Facts {
        active: true,
        middle_ready: true,
        inside: true,
        release_raw: Judgement::Perfect,
    }
}

#[test]
fn hidden_release_skips_even_unready_inactive_nodes_without_scoring_them() {
    let chain = Chain::new(vec![Node::Hidden, Node::Hidden, Node::PlainEnd(8)], 0).unwrap();
    let mut facts = [ready(); 3];
    facts[0] = Facts {
        active: false,
        middle_ready: false,
        inside: false,
        ..ready()
    };
    facts[1] = facts[0];
    let p = chain.dispatch(0, &facts, Callback::Release).unwrap();
    assert_eq!(p.next_cursor, 3);
    assert_eq!(
        p.settlements,
        vec![Settlement {
            note: 8,
            raw: Judgement::Perfect
        }]
    );
}

#[test]
fn move_hidden_is_a_real_transition_and_obeys_gate_but_not_hitbox() {
    for b in [-2, 0, 2] {
        let chain = Chain::new(vec![Node::Hidden, Node::Middle(1), Node::PlainEnd(2)], b).unwrap();
        let mut facts = [ready(); 3];
        facts[0].inside = false;
        facts[0].middle_ready = false;
        assert_eq!(
            chain
                .dispatch(
                    0,
                    &facts,
                    Callback::Move {
                        arrow_finish_passed: false
                    }
                )
                .unwrap(),
            Packet {
                next_cursor: 0,
                settlements: vec![],
                judged_hidden: vec![],
            }
        );
        facts[0].middle_ready = true;
        let p = chain
            .dispatch(
                0,
                &facts,
                Callback::Move {
                    arrow_finish_passed: false,
                },
            )
            .unwrap();
        assert_eq!(p.next_cursor, if b == 0 { 1 } else { 2 });
        assert_eq!(p.settlements.len(), usize::from(b != 0));
    }
}

#[test]
fn adjustment_changes_reentry_and_one_touch_cannot_choose_a_new_point_per_node() {
    for b in [-1, 0, 1] {
        let chain =
            Chain::new(vec![Node::Middle(1), Node::Middle(2), Node::PlainEnd(3)], b).unwrap();
        let mut facts = [ready(); 3];
        let p = chain
            .dispatch(
                0,
                &facts,
                Callback::Move {
                    arrow_finish_passed: false,
                },
            )
            .unwrap();
        assert_eq!(p.settlements.len(), if b == 0 { 1 } else { 2 });
        facts[1].inside = false;
        let p = chain
            .dispatch(
                0,
                &facts,
                Callback::Move {
                    arrow_finish_passed: false,
                },
            )
            .unwrap();
        assert_eq!(p.next_cursor, 1);
        assert_eq!(
            p.settlements,
            vec![Settlement {
                note: 1,
                raw: Judgement::Perfect
            }]
        );
    }
}

#[test]
fn b_zero_can_settle_middle_then_arrow_in_one_callback_and_skip_hidden_run() {
    let chain = Chain::new(vec![Node::Middle(4), Node::Hidden, Node::ArrowEnd(7)], 0).unwrap();
    let mut facts = [ready(); 3];
    facts[1].middle_ready = false;
    let p = chain
        .dispatch(
            0,
            &facts,
            Callback::Move {
                arrow_finish_passed: true,
            },
        )
        .unwrap();
    assert_eq!(
        p,
        Packet {
            next_cursor: 3,
            settlements: vec![
                Settlement {
                    note: 4,
                    raw: Judgement::Perfect
                },
                Settlement {
                    note: 7,
                    raw: Judgement::Perfect
                }
            ],
            judged_hidden: vec![],
        }
    );
    assert!(matches!(
        chain.dispatch(2, &facts, Callback::Release),
        Err(DispatchError::Miss(2))
    ));
}

#[test]
fn release_does_not_skip_visible_middle_or_rescue_miss_with_correction() {
    let chain = Chain::new(vec![Node::Hidden, Node::Middle(1), Node::PlainEnd(2)], 0).unwrap();
    let facts = [ready(); 3];
    assert_eq!(
        chain.dispatch(0, &facts, Callback::Release),
        Err(DispatchError::Miss(1))
    );
    let arrow = Chain::new(vec![Node::Middle(1), Node::Middle(2), Node::ArrowEnd(3)], 0).unwrap();
    assert_eq!(
        arrow.dispatch(
            0,
            &facts,
            Callback::Move {
                arrow_finish_passed: true
            }
        ),
        Err(DispatchError::Miss(1))
    );
}

#[test]
fn plain_end_preserves_raw_bad_good_for_the_actual_active_correction() {
    let c = Chain::new(vec![Node::PlainEnd(0)], 0).unwrap();
    for raw in [
        Judgement::Bad,
        Judgement::Good,
        Judgement::Great,
        Judgement::Perfect,
    ] {
        let p = c
            .dispatch(
                0,
                &[Facts {
                    release_raw: raw,
                    ..ready()
                }],
                Callback::Release,
            )
            .unwrap();
        assert_eq!(p.settlements[0].raw, raw);
    }
    assert_eq!(
        c.dispatch(
            0,
            &[Facts {
                release_raw: Judgement::Miss,
                ..ready()
            }],
            Callback::Release
        ),
        Err(DispatchError::Miss(0))
    );
    assert_eq!(
        c.dispatch(
            0,
            &[Facts {
                inside: false,
                ..ready()
            }],
            Callback::Release
        ),
        Err(DispatchError::Miss(0))
    );
}

#[test]
fn invalid_layout_or_missing_lifecycle_data_is_not_silently_relaxed() {
    assert!(Chain::new(vec![], 0).is_err());
    assert!(Chain::new(vec![Node::Hidden], 0).is_err());
    assert!(Chain::new(vec![Node::PlainEnd(0), Node::PlainEnd(1)], 0).is_err());
    assert!(Chain::new(vec![Node::Middle(1), Node::PlainEnd(1)], 0).is_err());
    let c = Chain::new(vec![Node::PlainEnd(1)], 0).unwrap();
    assert!(c.dispatch(1, &[ready()], Callback::Release).is_err());
    assert!(c.dispatch(0, &[], Callback::Release).is_err());
    assert!(c
        .dispatch(
            0,
            &[Facts {
                active: false,
                ..ready()
            }],
            Callback::Release
        )
        .is_err());
    assert!(c
        .dispatch(
            0,
            &[ready()],
            Callback::Move {
                arrow_finish_passed: true
            }
        )
        .is_err());
}

#[test]
fn hidden_runs_have_no_arbitrary_length_or_recursion_limit() {
    let mut nodes = vec![Node::Hidden; 10_000];
    nodes.push(Node::PlainEnd(1));
    let facts = vec![ready(); nodes.len()];
    for b in [0, 1] {
        let c = Chain::new(nodes.clone(), b).unwrap();
        let p = c.dispatch(0, &facts, Callback::Release).unwrap();
        assert_eq!(p.next_cursor, 10_001);
        assert_eq!(p.settlements.len(), 1);
        let p = c
            .dispatch(
                0,
                &facts,
                Callback::Move {
                    arrow_finish_passed: false,
                },
            )
            .unwrap();
        assert_eq!(p.next_cursor, if b == 0 { 1 } else { 10_000 });
        assert!(p.settlements.is_empty());
    }
}

#[test]
fn native_packet_counterexample_is_excluded_by_single_settlement_model() {
    use super::replay::{replay_callbacks, Event};
    use crate::hit_timing::*;
    let c = Chain::new(vec![Node::Middle(1), Node::ArrowEnd(2)], 0).unwrap();
    let facts = [ready(); 2];
    let q = ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 17,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
        notes: vec![
            ReplayNote {
                time: 0.,
                trigger: Some(0),
                window: NoteWindow::NativeGreen {
                    first_frames: [0; 4],
                    last_frame: 0,
                },
            },
            ReplayNote {
                time: 0.2,
                trigger: None,
                window: NoteWindow::NativeGreen {
                    first_frames: [24; 4],
                    last_frame: 24,
                },
            },
            ReplayNote {
                time: 0.21,
                trigger: None,
                window: NoteWindow::NativeGreen {
                    first_frames: [24; 4],
                    last_frame: 24,
                },
            },
            ReplayNote {
                time: 0.125,
                trigger: None,
                window: NoteWindow::Standard,
            },
        ],
        hit_frames: vec![0, 24, 24, 24],
        skills: vec![ReplaySkill {
            duration: 1.,
            score: ScoreEffect::Continuous {
                high: 100.,
                low: 99.9,
            },
            correction: None,
        }],
    };
    let run = |before: bool| {
        let callback = Event::Green {
            chain: 0,
            frame: 24,
            facts: &facts,
            callback: Callback::Move {
                arrow_finish_passed: true,
            },
        };
        let tap = Event::Hit { note: 3, frame: 24 };
        let mut events = vec![Event::Hit { note: 0, frame: 0 }];
        if before {
            events.extend([tap, callback]);
        } else {
            events.extend([callback, tap]);
        }
        assert_eq!(
            replay_callbacks(&q, &[(0, &c)], &events).unwrap_err(),
            ReplayError::Input("same touch cannot settle multiple scoring notes in one frame")
        );
        // Hypothetical domain-only packet, not an accepted model plan.
        let mut hypothetical = q.clone();
        hypothetical.hit_order = if before {
            vec![0, 3, 1, 2]
        } else {
            vec![0, 1, 2, 3]
        };
        crate::hit_timing::replay(&hypothetical).unwrap()
    };
    let a = run(false);
    let b = run(true);
    assert_eq!(a.hit_order, [0, 1, 2, 3]);
    assert_eq!(b.hit_order, [0, 3, 1, 2]);
    let mut split = q.clone();
    split.hit_order = vec![0, 1, 3, 2];
    let invalid_best = crate::hit_timing::replay(&split).unwrap();
    assert!(
        invalid_best.total > a.total.max(b.total),
        "need a scoring counterexample: split={}, packets={}/{}",
        invalid_best.total,
        a.total,
        b.total
    );
    // It is impossible to express the split as one Green callback: emitted
    // nodes cannot be injected separately as ordinary hits.
    assert!(replay_callbacks(
        &q,
        &[(0, &c)],
        &[
            Event::Hit { note: 0, frame: 0 },
            Event::Hit { note: 1, frame: 24 },
            Event::Hit { note: 3, frame: 24 },
            Event::Hit { note: 2, frame: 24 },
        ]
    )
    .is_err());
}

#[test]
fn callback_replay_rejects_missing_head_wrong_grade_and_unfinished_chain() {
    use super::replay::{replay_callbacks, Event};
    use crate::hit_timing::*;
    let c = Chain::new(vec![Node::PlainEnd(1)], 0).unwrap();
    let q = ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 0,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
        notes: vec![
            ReplayNote {
                time: 0.,
                trigger: Some(0),
                window: NoteWindow::NativeGreen {
                    first_frames: [0; 4],
                    last_frame: 0,
                },
            },
            ReplayNote {
                time: 0.2,
                trigger: None,
                window: NoteWindow::NativeGreen {
                    first_frames: [24, 25, 25, 25],
                    last_frame: 24,
                },
            },
        ],
        hit_frames: vec![0, 24],
        skills: vec![ReplaySkill {
            duration: 1.,
            score: ScoreEffect::Plain { percent: 0. },
            correction: Some(JudgementCorrection::Bad),
        }],
    };
    let facts = [Facts {
        release_raw: Judgement::Bad,
        ..ready()
    }];
    let head = Event::Hit { note: 0, frame: 0 };
    let end = Event::Green {
        chain: 0,
        frame: 24,
        facts: &facts,
        callback: Callback::Release,
    };
    let result = replay_callbacks(&q, &[(0, &c)], &[head, end]).unwrap();
    assert_eq!(result.trace[1].raw, Judgement::Bad);
    assert_eq!(result.trace[1].judgement, Judgement::Perfect);
    assert!(replay_callbacks(
        &q,
        &[(0, &c)],
        &[Event::Green {
            chain: 0,
            frame: 24,
            facts: &facts,
            callback: Callback::Release
        }]
    )
    .is_err());
    assert!(replay_callbacks(&q, &[(0, &c)], &[Event::Hit { note: 0, frame: 0 }]).is_err());
    assert!(replay_callbacks(
        &q,
        &[(0, &c)],
        &[
            Event::Hit { note: 0, frame: 0 },
            Event::Green {
                chain: 0,
                frame: 24,
                facts: &[ready()],
                callback: Callback::Release
            }
        ]
    )
    .is_err());
}
