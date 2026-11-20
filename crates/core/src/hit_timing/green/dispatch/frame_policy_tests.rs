use super::replay::{replay_callbacks, Event};
use super::*;
use crate::hit_timing::*;

fn ready() -> Facts {
    Facts {
        active: true,
        middle_ready: true,
        inside: true,
        release_raw: Judgement::Perfect,
    }
}
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
fn touch_limit(e: ReplayError) {
    assert_eq!(
        e,
        ReplayError::Input("same touch cannot settle multiple scoring notes in one frame")
    );
}

#[test]
fn separate_callbacks_cannot_bypass_one_settlement_per_frame() {
    let q = input(4);
    let c = Chain::new(vec![Node::Middle(1), Node::ArrowEnd(2)], 0).unwrap();
    let facts = [ready(); 2];
    let run = |middle_frame, end_frame| {
        replay_callbacks(
            &q,
            &[(0, &c)],
            &[
                Event::Hit { note: 0, frame: 0 },
                Event::Green {
                    chain: 0,
                    frame: middle_frame,
                    facts: &facts,
                    callback: Callback::Move {
                        arrow_finish_passed: false,
                    },
                },
                // An unrelated touch may still score on the middle node's frame.
                Event::Hit {
                    note: 3,
                    frame: middle_frame,
                },
                Event::Green {
                    chain: 0,
                    frame: end_frame,
                    facts: &facts,
                    callback: Callback::Move {
                        arrow_finish_passed: true,
                    },
                },
            ],
        )
    };
    touch_limit(run(24, 24).unwrap_err());
    // The head press consumes this continuous touch's frame too.
    touch_limit(run(0, 1).unwrap_err());
    let ok = run(24, 25).unwrap();
    assert_eq!(ok.hit_frames, [0, 24, 25, 24]);
    assert_eq!(ok.hit_order, [0, 1, 3, 2]);
}

#[test]
fn no_score_hidden_advancement_does_not_consume_a_frame_slot() {
    let q = input(3);
    let c = Chain::new(vec![Node::Hidden, Node::Middle(1), Node::PlainEnd(2)], 0).unwrap();
    let facts = [ready(); 3];
    let result = replay_callbacks(
        &q,
        &[(0, &c)],
        &[
            Event::Hit { note: 0, frame: 0 },
            Event::Green {
                chain: 0,
                frame: 0,
                facts: &facts,
                callback: Callback::Move {
                    arrow_finish_passed: false,
                },
            },
            Event::Green {
                chain: 0,
                frame: 1,
                facts: &facts,
                callback: Callback::Move {
                    arrow_finish_passed: false,
                },
            },
            Event::Green {
                chain: 0,
                frame: 2,
                facts: &facts,
                callback: Callback::Release,
            },
        ],
    )
    .unwrap();
    assert_eq!(result.hit_frames, [0, 1, 2]);
}

// This validates a supplied callback schedule, not Unity's input sampling cadence.
#[test]
fn hidden_and_following_middle_can_share_frame_in_supplied_callback_schedule() {
    let mut q = input(3);
    for (note, first, last) in [(0, 6291, 6312), (1, 6318, 6325), (2, 6319, 6330)] {
        q.notes[note].window = NoteWindow::NativeGreen {
            first_frames: [first; 4],
            last_frame: last,
        };
    }
    let c = Chain::new(vec![Node::Hidden, Node::Middle(1), Node::PlainEnd(2)], 0).unwrap();
    let facts = [ready(); 3];
    let mut before_gate = facts;
    before_gate[0].middle_ready = false;
    before_gate[1].middle_ready = false;
    let result = replay_callbacks(
        &q,
        &[(0, &c)],
        &[
            Event::Hit {
                note: 0,
                frame: 6291,
            },
            Event::Green {
                chain: 0,
                frame: 6317,
                facts: &before_gate,
                callback: Callback::Move {
                    arrow_finish_passed: false,
                },
            },
            // B=0 advances the hidden node and returns, without scoring.
            Event::Green {
                chain: 0,
                frame: 6318,
                facts: &facts,
                callback: Callback::Move {
                    arrow_finish_passed: false,
                },
            },
            // A second supplied callback scores the middle node on the same frame.
            Event::Green {
                chain: 0,
                frame: 6318,
                facts: &facts,
                callback: Callback::Move {
                    arrow_finish_passed: false,
                },
            },
            Event::Green {
                chain: 0,
                frame: 6319,
                facts: &facts,
                callback: Callback::Release,
            },
        ],
    )
    .unwrap();
    assert_eq!(result.hit_frames, [6291, 6318, 6319]);
    assert_eq!(result.hit_order, [0, 1, 2]);
}

#[test]
fn independent_continuous_touches_can_settle_in_the_same_frame() {
    let q = input(4);
    let a = Chain::new(vec![Node::PlainEnd(2)], 0).unwrap();
    let b = Chain::new(vec![Node::PlainEnd(3)], 0).unwrap();
    let facts = [ready()];
    let result = replay_callbacks(
        &q,
        &[(0, &a), (1, &b)],
        &[
            Event::Hit { note: 0, frame: 0 },
            Event::Hit { note: 1, frame: 0 },
            Event::Green {
                chain: 1,
                frame: 1,
                facts: &facts,
                callback: Callback::Release,
            },
            Event::Green {
                chain: 0,
                frame: 1,
                facts: &facts,
                callback: Callback::Release,
            },
        ],
    )
    .unwrap();
    assert_eq!(result.hit_frames, [0, 0, 1, 1]);
    assert_eq!(result.hit_order, [0, 1, 3, 2]);
}
