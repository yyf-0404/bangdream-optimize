use super::*;
use crate::hit_timing::{
    replay, JudgementCorrection, ReplayInput, ReplayNote, ReplaySkill, ScoreEffect,
};
use crate::scoring::{Judgement, ScoringPower};
fn clock(pos: f32) -> NativeClock {
    NativeClock {
        adjusted_music_pos: pos,
        current_bpm: 240.,
        resolution: 1000,
    }
}
fn stop(pos: f32, next: Option<(i32, VisibleState)>, b: i32) -> LifetimeUpdate {
    LifetimeUpdate::AfterStop {
        clock: clock(pos),
        has_direct_after: next.is_some(),
        next_visible: next.map(|(absolute_pos, state)| VisibleAfter {
            absolute_pos,
            state,
        }),
        arrow_terminal: false,
        execute_frame: 0.5,
        adjust_b: b,
    }
}
#[test]
fn head_midpoint_is_strict_and_only_runs_in_wait() {
    let mut life = UnsettledLifetime::default();
    assert_eq!(life.update(1000, LifetimeUpdate::Move).unwrap(), None);
    for pos in [999., 1000., 1049., 1050.] {
        assert_eq!(
            life.update(
                1000,
                LifetimeUpdate::HeadWait {
                    clock: clock(pos),
                    next_visible_pos: 1100
                }
            )
            .unwrap(),
            None
        );
    }
    assert_eq!(
        life.update(
            1000,
            LifetimeUpdate::HeadWait {
                clock: clock(f32::from_bits(1050f32.to_bits() + 1)),
                next_visible_pos: 1100
            }
        )
        .unwrap(),
        Some(Expiry::HeadNextVisible)
    );
    assert_eq!(
        life.update(1000, LifetimeUpdate::Move).unwrap(),
        Some(Expiry::HeadNextVisible)
    );
}
#[test]
fn timeout_uses_current_bpm_and_native_f32_operation_order() {
    for bpm in [80., 137.25, 240., 333.] {
        let c = NativeClock {
            current_bpm: bpm,
            ..clock(1247.125)
        };
        assert_eq!(
            c.late_seconds(1000).unwrap().to_bits(),
            (((240f32 / bpm) * (1247.125f32 - 1000.)) / 1000.).to_bits()
        );
    }
    let c = NativeClock {
        adjusted_music_pos: MISS_SECOND_INTERVAL,
        current_bpm: 240.,
        resolution: 1,
    };
    let make = |c| LifetimeUpdate::HeadWait {
        clock: c,
        next_visible_pos: 10,
    };
    let mut life = UnsettledLifetime::default();
    assert_eq!(life.update(0, make(c)).unwrap(), None);
    assert_eq!(
        life.update(
            0,
            make(NativeClock {
                adjusted_music_pos: f32::from_bits(MISS_SECOND_INTERVAL.to_bits() + 1),
                ..c
            })
        )
        .unwrap(),
        Some(Expiry::Timeout)
    );
}
#[test]
fn after_midpoint_and_recall_do_not_use_head_predicate() {
    for (pos, expected) in [
        (1050., None),
        (1051., Some(Expiry::AfterMidpoint)),
        (1100., None),
        (1101., None),
    ] {
        let mut life = UnsettledLifetime::default();
        assert_eq!(
            life.update(1000, stop(pos, Some((1100, VisibleState::Move)), 0))
                .unwrap(),
            expected
        );
    }
    for state in [
        VisibleState::Stop,
        VisibleState::Deactive,
        VisibleState::WaitingDeactive,
    ] {
        let mut life = UnsettledLifetime::default();
        assert_eq!(
            life.update(1000, stop(1000., Some((1100, state)), 0))
                .unwrap(),
            Some(Expiry::NextVisibleStopped)
        );
    }
}
#[test]
fn negative_adjust_delays_recall_before_counter_increment_and_timeout() {
    for b in [-2, -1] {
        let mut life = UnsettledLifetime::default();
        for _ in 0..6 - b {
            assert_eq!(
                life.update(1000, stop(2000., Some((1100, VisibleState::Stop)), b))
                    .unwrap(),
                None
            );
        }
        assert_eq!(
            life.update(1000, stop(2000., Some((1100, VisibleState::Stop)), b))
                .unwrap(),
            Some(Expiry::NextVisibleStopped)
        );
        let mut terminal = UnsettledLifetime::default();
        assert_eq!(
            terminal.update(1000, stop(2000., None, b)).unwrap(),
            Some(Expiry::Timeout)
        );
    }
}
#[test]
fn terminal_arrow_uses_execute_frame_not_wall_timeout_or_render_frame_count() {
    let update = LifetimeUpdate::AfterStop {
        clock: clock(9999.),
        has_direct_after: false,
        next_visible: None,
        arrow_terminal: true,
        execute_frame: 0.5,
        adjust_b: 0,
    };
    let mut life = UnsettledLifetime::default();
    for _ in 0..13 {
        assert_eq!(life.update(1000, update).unwrap(), None);
    }
    assert_eq!(life.update(1000, update).unwrap(), Some(Expiry::ArrowWait));
}
#[test]
fn missing_visible_after_is_not_silently_ignored() {
    let u = LifetimeUpdate::AfterStop {
        clock: clock(1000.),
        has_direct_after: true,
        next_visible: None,
        arrow_terminal: false,
        execute_frame: 0.5,
        adjust_b: -1,
    };
    assert_eq!(
        UnsettledLifetime::default().update(1000, u).unwrap(),
        Some(Expiry::MissingVisibleAfter)
    );
}
fn table() -> [f32; 16] {
    std::array::from_fn(|i| (16 - i) as f32)
}
fn sampled(role: GreenRole, first: u32, ys: &[f32]) -> LiveWindow {
    let samples = ys.iter().enumerate().map(|(i, &local_y)| LifetimeSample {
        local_y,
        following_update: if i + 1 < ys.len() {
            LifetimeUpdate::Move
        } else if matches!(role, GreenRole::Head) {
            LifetimeUpdate::HeadWait {
                clock: clock(1100.),
                next_visible_pos: 1100,
            }
        } else {
            stop(1217., None, 0)
        },
    });
    compile_live_window(role, &table(), 1000, first, samples).unwrap()
}
#[test]
fn expiry_is_after_input_and_skills_cannot_rescue_expired_nodes() {
    let w = sampled(GreenRole::Head, 20, &[16., 15., 14., 11., 8.]);
    assert_eq!(w.expires_after_frame, 24);
    let n = ReplayNote {
        time: 0.,
        trigger: None,
        window: w.window,
    };
    assert_eq!(n.raw_judgement(20).unwrap(), Judgement::Bad);
    assert_eq!(n.raw_judgement(24).unwrap(), Judgement::Perfect);
    assert_eq!(n.raw_judgement(25).unwrap(), Judgement::Miss);
}
#[test]
fn incomplete_invalid_or_wrong_role_traces_are_rejected() {
    assert!(compile_live_window(GreenRole::Head, &table(), 1000, 0, []).is_err());
    assert!(compile_live_window(
        GreenRole::Head,
        &table(),
        1000,
        0,
        [LifetimeSample {
            local_y: 8.,
            following_update: LifetimeUpdate::Move
        }]
    )
    .is_err());
    assert!(compile_live_window(
        GreenRole::Head,
        &table(),
        1000,
        0,
        [LifetimeSample {
            local_y: 8.,
            following_update: stop(2000., None, 0)
        }]
    )
    .is_err());
    assert!(NativeClock {
        current_bpm: f32::NAN,
        ..clock(1.)
    }
    .late_seconds(0)
    .is_err());
    assert!(NativeClock {
        resolution: 0,
        ..clock(1.)
    }
    .late_seconds(0)
    .is_err());
}
#[test]
fn sampled_lifetimes_chain_search_and_replay_match_exhaustive_frames() {
    use super::super::compact::{compile_chain, GreenChainNode};
    use crate::hit_timing::independent::JointTimeline;
    // Synthetic position/clock trace, not a certified real-chart capture.
    let live = [
        sampled(GreenRole::Head, 24, &[16., 15., 14., 11., 8.]),
        sampled(GreenRole::HeldMiddle, 27, &[10., 9., 8., 7., 6.]),
        sampled(GreenRole::PlainEnd, 30, &[16., 15., 14., 11., 8.]),
    ];
    let chain = compile_chain(
        &live
            .iter()
            .enumerate()
            .map(|(i, l)| GreenChainNode {
                note_index: i,
                window: l.window,
                minimum_gap_from_previous: if i == 0 { 0 } else { 1 },
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for effect in [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. },
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
                notes: chain
                    .windows
                    .iter()
                    .map(|&(i, window)| ReplayNote {
                        time: [0.2, 0.25, 0.275][i],
                        trigger: (i == 0).then_some(0),
                        window,
                    })
                    .collect(),
                hit_frames: vec![27, 29, 33],
                skills: vec![ReplaySkill {
                    duration: 0.05,
                    score: effect,
                    correction,
                }],
                initial_combo: 19,
                medley_combo: false,
                fever_frames: None,
                constraints: chain.constraints.clone(),
                hit_order: vec![],
                native_time_routing: None,
            };
            let mut best = 0;
            // Deliberately extend past expiry. Replay must reject those frames.
            for a in 23..=29 {
                for b in 26..=32 {
                    for c in 29..=35 {
                        let mut trial = r.clone();
                        trial.hit_frames = vec![a, b, c];
                        if let Ok(result) = replay(&trial) {
                            best = best.max(result.total);
                        }
                    }
                }
            }
            let result = JointTimeline::new(&r)
                .unwrap()
                .maximum(ScoringPower::new(r.power).unwrap())
                .unwrap()
                .result;
            assert_eq!(result.total, best);
            assert!(result
                .hit_frames
                .iter()
                .zip(live)
                .all(|(&f, l)| f <= l.expires_after_frame));
            let mut witness = r;
            witness.hit_frames = result.hit_frames;
            witness.hit_order = result.hit_order;
            assert_eq!(replay(&witness).unwrap().total, best);
        }
    }
}

fn variant(
    first_frame: u32,
    positions: &[f32],
    expire: bool,
) -> LifetimeVariant<Vec<LifetimeSample>> {
    LifetimeVariant {
        table: table(),
        first_frame,
        samples: positions
            .iter()
            .enumerate()
            .map(|(i, &local_y)| LifetimeSample {
                local_y,
                following_update: if expire && i + 1 == positions.len() {
                    LifetimeUpdate::HeadWait {
                        clock: clock(1100.),
                        next_visible_pos: 1100,
                    }
                } else {
                    LifetimeUpdate::Move
                },
            })
            .collect(),
    }
}
#[test]
fn strict_live_compilation_intersects_actual_expiry_and_grade() {
    let a = variant(20, &[16., 15., 14., 11., 8.], true);
    let b = variant(21, &[16., 15., 14., 11., 8., 7.], true);
    let merged = compile_strict_live_window(GreenRole::Head, 1000, [a, b]).unwrap();
    let note = ReplayNote {
        time: 0.,
        trigger: None,
        window: merged,
    };
    assert_eq!(note.raw_judgement(20).unwrap(), Judgement::Miss);
    assert_eq!(note.raw_judgement(21).unwrap(), Judgement::Bad);
    assert_eq!(note.raw_judgement(22).unwrap(), Judgement::Good);
    assert_eq!(note.raw_judgement(23).unwrap(), Judgement::Great);
    assert_eq!(note.raw_judgement(24).unwrap(), Judgement::Perfect);
    assert_eq!(note.raw_judgement(25).unwrap(), Judgement::Miss);
}
#[test]
fn strict_live_compilation_never_skips_an_unverified_or_disjoint_variant() {
    assert!(compile_strict_live_window(
        GreenRole::Head,
        1000,
        [
            variant(20, &[11., 8.], true),
            variant(20, &[11., 8.], false)
        ]
    )
    .is_err());
    assert!(compile_strict_live_window(
        GreenRole::Head,
        1000,
        [variant(20, &[11., 8.], true), variant(22, &[11., 8.], true)]
    )
    .is_err());
    let none: Vec<LifetimeVariant<Vec<LifetimeSample>>> = vec![];
    assert!(compile_strict_live_window(GreenRole::Head, 1000, none).is_err());
}
