use super::*;
use crate::hit_timing::green::{compile_window, position_grade};
use crate::scoring::Judgement;

fn note(window: NoteWindow) -> ReplayNote {
    ReplayNote {
        time: 0.,
        trigger: None,
        window,
    }
}
fn window(first: [u32; 4], last: u32) -> NoteWindow {
    NoteWindow::NativeGreen {
        first_frames: first,
        last_frame: last,
    }
}
fn same_window(a: NoteWindow, b: NoteWindow) {
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap()
    );
}

#[test]
fn shared_profile_matches_position_oracle_at_every_boundary_and_ulp() {
    for scale in [0.125, 1., 7.25] {
        let table = std::array::from_fn(|i| (16 - i) as f32 * scale);
        let mut ys = vec![17. * scale, -100. * scale];
        for y in table {
            for d in [-1i32, 0, 1] {
                ys.push(f32::from_bits(
                    (i64::from(y.to_bits()) + i64::from(d)) as u32,
                ));
            }
            ys.push(y); // Equal-position plateaus are legal.
        }
        ys.sort_by(|a, b| b.total_cmp(a));
        let compact = GreenFrameProfile::compile(&table, 120, ys.iter().copied()).unwrap();
        for role in [
            GreenRole::Head,
            GreenRole::PlainEnd,
            GreenRole::HeldMiddle,
            GreenRole::HeldArrowEnd,
        ] {
            let projected = compact.window(role).unwrap();
            same_window(projected, compile_window(role, &table, 120, &ys).unwrap());
            let n = note(projected);
            for (i, &y) in ys.iter().enumerate() {
                assert_eq!(
                    n.raw_judgement(120 + i as u32).unwrap(),
                    position_grade(role, &table, y).unwrap()
                );
            }
            assert_eq!(n.raw_judgement(119).unwrap(), Judgement::Miss);
            assert_eq!(
                n.raw_judgement(120 + ys.len() as u32).unwrap(),
                Judgement::Miss
            );
        }
    }
}

#[test]
fn equal_position_plateau_cannot_be_replaced_with_reference_progress_cut() {
    // Captured synthetic f32 arithmetic counterexample, not a live game trace.
    let candidate_progress = 0.9939384460449219f32;
    let reference_progress = 0.9939385652542114f32;
    let geometry = crate::hit_timing::green::motion::GreenGeometry {
        specific_speed: 1.,
        start_y: 1000.,
        button_world_y: 900.,
        button_local_y: 900.,
    };
    let candidate_y = geometry.position_y(candidate_progress).unwrap();
    let reference_y = geometry.position_y(reference_progress).unwrap();
    assert_eq!(candidate_y, reference_y);
    assert!(candidate_progress < reference_progress);
    assert!(candidate_y <= reference_y);
    let table = std::array::from_fn(|i| reference_y + (5 - i as i32) as f32);
    let compact = GreenFrameProfile::compile(&table, 7, [candidate_y]).unwrap();
    let n = note(compact.window(GreenRole::Head).unwrap());
    assert_eq!(n.raw_judgement(7).unwrap(), Judgement::Perfect);
}

#[test]
fn invalid_profiles_reject_and_within_bucket_motion_need_not_be_strict() {
    let table = std::array::from_fn(|i| (16 - i) as f32);
    assert!(GreenFrameProfile::compile(&table, 0, []).is_err());
    assert!(GreenFrameProfile::compile(&table, 0, [f32::NAN]).is_err());
    assert!(GreenFrameProfile::compile(&table, 72000, [8., 8.]).is_err());
    assert!(GreenFrameProfile::compile(&table, u32::MAX, [8.]).is_err());
    assert!(GreenFrameProfile::compile(&table, 0, [11., 14.]).is_err());
    assert!(GreenFrameProfile::compile(&[1.; 16], 0, [0.]).is_err());
    let p = GreenFrameProfile::compile(&table, 0, [13., 13.5, 12., 12.]).unwrap();
    same_window(p.window(GreenRole::Head).unwrap(), window([0, 0, 0, 4], 3));
    assert!(p.window(GreenRole::HeldMiddle).is_err());
    let p = GreenFrameProfile::compile(&table, 0, [17.]).unwrap();
    assert!(p.window(GreenRole::Head).is_err());
}

#[test]
fn chain_clipping_preserves_every_feasible_assignment_and_raw_grade() {
    // Exhaust small intervals and requested gaps of 0, 1 and 2 frames.
    // Oracle: strictly increasing frames AND original requested constraints.
    let intervals: Vec<_> = (0..=3)
        .flat_map(|start| (start..=4).map(move |end| (start, end)))
        .collect();
    for &(a, az) in &intervals {
        for &(b, bz) in &intervals {
            for &(c, cz) in &intervals {
                for g1 in 0..=2 {
                    for g2 in 0..=2 {
                        let nodes: Vec<_> = [(a, az, 0), (b, bz, g1), (c, cz, g2)]
                            .into_iter()
                            .enumerate()
                            .map(|(i, (s, e, g))| GreenChainNode {
                                note_index: i,
                                window: window(
                                    [
                                        s,
                                        (s + 1).min(e + 1),
                                        (s + 2).min(e + 1),
                                        (s + 3).min(e + 1),
                                    ],
                                    e,
                                ),
                                minimum_gap_from_previous: g,
                            })
                            .collect();
                        let feasible: Vec<_> = (a..=az)
                            .flat_map(|x| {
                                (b..=bz).flat_map(move |y| (c..=cz).map(move |z| [x, y, z]))
                            })
                            .filter(|f| {
                                f[0] < f[1] && f[1] < f[2] && f[0] + g1 <= f[1] && f[1] + g2 <= f[2]
                            })
                            .collect();
                        let compiled = compile_chain(&nodes);
                        if feasible.is_empty() {
                            assert!(compiled.is_err());
                            continue;
                        }
                        let compiled = compiled.unwrap();
                        assert_eq!(compiled.constraints.len(), 2);
                        assert!(compiled
                            .constraints
                            .iter()
                            .all(|r| r.require_dispatch_order && r.minimum_gap_frames >= 1));
                        let old_size = u64::from(az - a + 1 + bz - b + 1 + cz - c + 1);
                        let new_size: usize = compiled
                            .windows
                            .iter()
                            .map(|&(_, w)| note(w).frame_domain(Judgement::Bad).unwrap().len())
                            .sum();
                        assert_eq!(compiled.removed_frames, old_size - new_size as u64);
                        for frames in &feasible {
                            for i in 0..3 {
                                assert_eq!(
                                    note(compiled.windows[i].1)
                                        .raw_judgement(frames[i])
                                        .unwrap(),
                                    note(nodes[i].window).raw_judgement(frames[i]).unwrap()
                                );
                            }
                        }
                        // Conversely, no new frame tuple satisfying retained edges appears.
                        for x in 0..=4 {
                            for y in 0..=4 {
                                for z in 0..=4 {
                                    let f = [x, y, z];
                                    let legal =
                                        compiled.constraints.iter().all(|c| {
                                            f[c.before] + c.minimum_gap_frames <= f[c.after]
                                        }) && (0..3).all(|i| {
                                            note(compiled.windows[i].1).raw_judgement(f[i]).unwrap()
                                                >= Judgement::Bad
                                        });
                                    assert_eq!(legal, feasible.contains(&f));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn chain_keeps_correctable_bad_and_never_invents_a_perfect_frame() {
    let nodes = [
        GreenChainNode {
            note_index: 3,
            window: window([2, 4, 6, 9], 10),
            minimum_gap_from_previous: 0,
        },
        GreenChainNode {
            note_index: 7,
            window: window([1, 2, 3, 4], 5),
            minimum_gap_from_previous: 1,
        },
    ];
    let c = compile_chain(&nodes).unwrap();
    same_window(c.windows[0].1, window([2, 4, 5, 5], 4));
    same_window(c.windows[1].1, window([3, 3, 3, 4], 5));
    assert_eq!(c.removed_frames, 8);
    assert_eq!(
        note(c.windows[0].1).raw_judgement(2).unwrap(),
        Judgement::Bad
    );
    assert!(note(c.windows[0].1)
        .frame_domain(Judgement::Perfect)
        .unwrap()
        .is_empty());
}

#[test]
fn invalid_or_infeasible_chains_fail_without_relaxation() {
    let n = GreenChainNode {
        note_index: 0,
        window: window([0; 4], 1),
        minimum_gap_from_previous: 0,
    };
    assert!(compile_chain(&[]).is_err());
    assert!(compile_chain(&[GreenChainNode {
        minimum_gap_from_previous: 1,
        ..n
    }])
    .is_err());
    assert!(compile_chain(&[n, n]).is_err());
    assert!(compile_chain(&[GreenChainNode {
        window: NoteWindow::Standard,
        ..n
    }])
    .is_err());
    assert!(compile_chain(&[
        n,
        GreenChainNode {
            note_index: 1,
            minimum_gap_from_previous: 2,
            ..n
        }
    ])
    .is_err());
    assert!(compile_chain(&[
        n,
        GreenChainNode {
            note_index: 1,
            minimum_gap_from_previous: u32::MAX,
            ..n
        }
    ])
    .is_err());
}
