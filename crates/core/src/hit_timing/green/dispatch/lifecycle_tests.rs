use super::replay::{replay_callbacks, Event};
use super::*;
use crate::hit_timing::green::lifetime::{LifetimeUpdate, NativeClock, VisibleAfter, VisibleState};
use crate::hit_timing::*;

fn input(n: usize) -> ReplayInput {
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 0,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
        skills: vec![],
        notes: (0..n)
            .map(|_| ReplayNote {
                time: 0.,
                trigger: None,
                window: NoteWindow::NativeGreen {
                    first_frames: [0; 4],
                    last_frame: 30,
                },
            })
            .collect(),
        hit_frames: vec![0; n],
    }
}
fn ready() -> Facts {
    Facts {
        active: true,
        middle_ready: true,
        inside: true,
        release_raw: Judgement::Perfect,
    }
}
fn expired(node: usize, frame: u32) -> Event<'static> {
    Event::AfterUpdate {
        chain: 0,
        node,
        frame,
        absolute_pos: 0,
        update: LifetimeUpdate::AfterStop {
            clock: NativeClock {
                adjusted_music_pos: 300.,
                current_bpm: 120.,
                resolution: 480,
            },
            has_direct_after: true,
            next_visible: Some(VisibleAfter {
                absolute_pos: 480,
                state: VisibleState::Move,
            }),
            arrow_terminal: false,
            execute_frame: 0.5,
            adjust_b: 0,
        },
    }
}
fn moved(frame: u32, facts: &[Facts]) -> Event<'_> {
    Event::Green {
        chain: 0,
        frame,
        facts,
        callback: Callback::Move {
            arrow_finish_passed: false,
        },
    }
}
fn ended(frame: u32, facts: &[Facts]) -> Event<'_> {
    Event::Green {
        chain: 0,
        frame,
        facts,
        callback: Callback::Release,
    }
}

#[test]
fn hidden_expiry_advances_without_miss_combo_loss_or_score_slot() {
    let mut q = input(3);
    q.initial_combo = 19;
    let c = Chain::new(vec![Node::Hidden, Node::Middle(1), Node::PlainEnd(2)], 0).unwrap();
    let mut facts = [ready(); 3];
    facts[0].active = false;
    facts[0].middle_ready = false;
    let result = replay_callbacks(
        &q,
        &[(0, &c)],
        &[
            Event::Hit { note: 0, frame: 0 },
            expired(0, 0),
            moved(1, &facts),
            ended(2, &facts),
        ],
    )
    .unwrap();
    assert_eq!(result.hit_frames, [0, 1, 2]);
    assert_eq!(result.trace.len(), 3);
    assert_eq!(result.trace[2].combo, 22);
    assert!(result.trace.iter().all(|h| h.raw == Judgement::Perfect));
    q.hit_frames = vec![0, 1, 2];
    assert_eq!(result.total, replay(&q).unwrap().total);
}

#[test]
fn future_hidden_expiry_must_not_fabricate_a_skipped_middle_hit() {
    let q = input(3);
    let c = Chain::new(vec![Node::Middle(1), Node::Hidden, Node::PlainEnd(2)], 0).unwrap();
    let facts = [ready(); 3];
    let err = replay_callbacks(
        &q,
        &[(0, &c)],
        &[
            Event::Hit { note: 0, frame: 0 },
            expired(1, 0),
            ended(1, &facts),
        ],
    )
    .unwrap_err();
    assert_eq!(
        err,
        ReplayError::Input("callback schedule omits scoring notes")
    );
}

#[test]
fn hidden_expiry_does_not_rewind_when_successor_already_judged() {
    let q = input(3);
    let c = Chain::new(
        vec![
            Node::Hidden,
            Node::Hidden,
            Node::Middle(1),
            Node::PlainEnd(2),
        ],
        0,
    )
    .unwrap();
    let facts = [ready(); 4];
    let r = replay_callbacks(
        &q,
        &[(0, &c)],
        &[
            Event::Hit { note: 0, frame: 0 },
            expired(1, 0),
            moved(1, &facts),
            // Hidden 0 was never touched. Hidden 1 was already judged by expiry.
            expired(0, 1),
            ended(2, &facts),
        ],
    )
    .unwrap();
    assert_eq!(r.hit_frames, [0, 1, 2]);
}

#[test]
fn hidden_expiry_current_pointer_is_not_an_extra_native_guard() {
    let c = Chain::new(
        vec![
            Node::Hidden,
            Node::Middle(1),
            Node::Hidden,
            Node::PlainEnd(2),
        ],
        0,
    )
    .unwrap();
    for cursor in 0..c.len() {
        for target in [0, 2] {
            assert_eq!(c.expire_hidden(cursor, target, false).unwrap(), target + 1);
            assert_eq!(c.expire_hidden(cursor, target, true).unwrap(), cursor);
        }
    }
    assert!(c.expire_hidden(0, 1, false).is_err());
    assert!(c.expire_hidden(c.len(), 0, false).is_err());
    assert!(c.expire_hidden(0, usize::MAX, false).is_err());
}

#[test]
fn visible_expiry_is_unrepairable_even_under_bad_correction() {
    let mut q = input(3);
    q.notes[0].trigger = Some(0);
    q.skills = vec![ReplaySkill {
        duration: 7.,
        score: ScoreEffect::Plain { percent: 100. },
        correction: Some(JudgementCorrection::Bad),
    }];
    let c = Chain::new(vec![Node::Middle(1), Node::PlainEnd(2)], 0).unwrap();
    let err = replay_callbacks(
        &q,
        &[(0, &c)],
        &[Event::Hit { note: 0, frame: 0 }, expired(0, 0)],
    )
    .unwrap_err();
    assert_eq!(
        err,
        ReplayError::Input("visible green node expired before scoring")
    );
}

#[test]
fn judged_middle_does_not_expire_again_in_later_updates() {
    let q = input(3);
    let c = Chain::new(vec![Node::Middle(1), Node::PlainEnd(2)], 0).unwrap();
    let facts = [ready(); 2];
    let r = replay_callbacks(
        &q,
        &[(0, &c)],
        &[
            Event::Hit { note: 0, frame: 0 },
            moved(1, &facts),
            expired(0, 1),
            expired(0, 2),
            ended(3, &facts),
        ],
    )
    .unwrap();
    assert_eq!(r.hit_frames, [0, 1, 3]);
}

#[test]
fn following_update_cannot_unlock_an_earlier_input_in_the_same_frame() {
    let q = input(3);
    let c = Chain::new(vec![Node::Hidden, Node::Middle(1), Node::PlainEnd(2)], 0).unwrap();
    let facts = [ready(); 3];
    let err = replay_callbacks(
        &q,
        &[(0, &c)],
        &[
            Event::Hit { note: 0, frame: 0 },
            expired(0, 1),
            moved(1, &facts),
            ended(2, &facts),
        ],
    )
    .unwrap_err();
    assert_eq!(
        err,
        ReplayError::Input("input cannot follow the following update in the same frame")
    );
}

#[test]
fn ended_skips_hidden_without_gates_or_hidden_judgements() {
    for b in [-1, 0, 1] {
        let c = Chain::new(vec![Node::Hidden, Node::Hidden, Node::PlainEnd(1)], b).unwrap();
        let mut facts = [ready(); 3];
        for f in &mut facts[..2] {
            f.active = false;
            f.middle_ready = false;
        }
        let p = c.dispatch(0, &facts, Callback::Release).unwrap();
        assert!(p.judged_hidden.is_empty());
        assert_eq!(p.settlements.len(), 1);
        assert_eq!(p.next_cursor, 3);
    }
}

#[test]
fn movement_records_only_hidden_nodes_it_actually_judged() {
    for b in [-1, 0, 1] {
        let c = Chain::new(vec![Node::Hidden, Node::Hidden, Node::PlainEnd(1)], b).unwrap();
        let facts = [ready(); 3];
        let p = c
            .dispatch(
                0,
                &facts,
                Callback::Move {
                    arrow_finish_passed: false,
                },
            )
            .unwrap();
        assert_eq!(p.judged_hidden, if b == 0 { vec![0] } else { vec![0, 1] });
        assert!(p.settlements.is_empty());
    }
}

#[test]
fn lifecycle_facts_cannot_silently_switch_node_identity_or_precede_head() {
    let q = input(3);
    let c = Chain::new(vec![Node::Hidden, Node::Middle(1), Node::PlainEnd(2)], 0).unwrap();
    assert_eq!(
        replay_callbacks(&q, &[(0, &c)], &[expired(0, 0)]).unwrap_err(),
        ReplayError::Input("green update precedes its head")
    );
    let a = Event::AfterUpdate {
        chain: 0,
        node: 0,
        frame: 0,
        absolute_pos: 1,
        update: LifetimeUpdate::Move,
    };
    assert_eq!(
        replay_callbacks(
            &q,
            &[(0, &c)],
            &[Event::Hit { note: 0, frame: 0 }, a, expired(0, 1),]
        )
        .unwrap_err(),
        ReplayError::Input("green node absolute position changed")
    );
}
