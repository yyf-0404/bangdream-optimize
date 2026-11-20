use super::*;
use crate::hit_timing::{replay, search, ReplayNote};
use std::collections::BTreeSet;

fn gate(ready: u32, expiry: u32) -> HiddenGate {
    HiddenGate {
        first_ready_frame: Some(ready),
        expires_after_frame: Some(expiry),
    }
}
fn node(i: usize, first: u32, last: u32) -> GreenChainNode {
    GreenChainNode {
        note_index: i,
        window: NoteWindow::NativeGreen {
            first_frames: [first; 4],
            last_frame: last,
        },
        minimum_gap_from_previous: if i == 0 { 0 } else { 1 },
    }
}
fn compile(
    nodes: &[GreenChainNode],
    bridges: &[HiddenBridge],
) -> Result<CompiledGreenChain, ReplayError> {
    compile_projected_chain(&[GreenChainVariant {
        nodes,
        bridges,
        adjust_b: 0,
    }])
}

// Independent frame-by-frame oracle: retain EVERY wait/Move option, then run
// hidden expiry callbacks with explicit judged flags and successor checks.
// No prefix-max reduction or greedy earliest implementation is used here.
fn brute(bridge: &HiddenBridge, a: u32, limit: u32) -> Option<u32> {
    let n = bridge.hidden.len();
    let mut states = BTreeSet::from([(0usize, vec![false; n])]);
    for f in a..=limit {
        let mut next = BTreeSet::new();
        for (cursor, judged) in states {
            if f > a {
                if cursor == n
                    || matches!(
                        bridge.successor,
                        GreenRole::PlainEnd | GreenRole::HeldArrowEnd
                    )
                {
                    return Some(f);
                }
                if bridge.hidden[cursor]
                    .first_ready_frame
                    .is_some_and(|r| r <= f)
                {
                    let mut moved = judged.clone();
                    moved[cursor] = true;
                    next.insert((cursor + 1, moved));
                }
            }
            next.insert((cursor, judged));
        }
        states = next
            .into_iter()
            .map(|(mut cursor, mut judged)| {
                for (i, h) in bridge.hidden.iter().enumerate() {
                    if h.expires_after_frame == Some(f) && !judged[i] {
                        if i + 1 == n || !judged[i + 1] {
                            cursor = i + 1;
                        }
                        judged[i] = true;
                    }
                }
                (cursor, judged)
            })
            .collect();
    }
    None
}

#[test]
fn projection_matches_all_wait_move_paths_including_timeout_prefix_jumps() {
    let mut cases = 0;
    // Includes missing readiness, absent expiry, multiple same-frame expiries.
    for n in 0..=3u32 {
        for mut code in 0..16usize.pow(n) {
            let mut hidden = vec![];
            for _ in 0..n {
                let d = code % 16;
                code /= 16;
                hidden.push(HiddenGate {
                    first_ready_frame: [None, Some(1), Some(3), Some(5)][d % 4],
                    expires_after_frame: [None, Some(2), Some(4), Some(6)][d / 4],
                });
            }
            let b = HiddenBridge {
                hidden,
                successor: GreenRole::HeldMiddle,
            };
            for a in 0..=2 {
                if b.validate(a).is_err() {
                    continue;
                }
                let expected = brute(&b, a, 12);
                assert_eq!(b.earliest(a).filter(|&f| f <= 12), expected, "{b:?} a={a}");
                cases += 1;
            }
        }
    }
    assert!(cases > 3000);
}

#[test]
fn middle_requires_next_input_frame_but_end_skips_hidden_gate() {
    let nodes = [node(0, 6316, 6317), node(1, 6318, 6321)];
    let mut bridges = [HiddenBridge {
        hidden: vec![gate(6318, 6320)],
        successor: GreenRole::HeldMiddle,
    }];
    let c = compile(&nodes, &bridges).unwrap();
    assert!(!c.constraints[0].allows_frames(6317, 6318));
    assert!(c.constraints[0].allows_frames(6317, 6319));
    for role in [GreenRole::PlainEnd, GreenRole::HeldArrowEnd] {
        bridges[0].successor = role;
        let c = compile(&nodes, &bridges).unwrap();
        assert!(c.constraints[0].earliest_after.is_none());
        assert!(c.constraints[0].allows_frames(6317, 6318));
        assert_eq!(brute(&bridges[0], 6317, 6321), Some(6318));
    }
}

#[test]
fn equality_is_input_before_update_and_expiry_can_rescue_unready_hidden() {
    let b = HiddenBridge {
        hidden: vec![gate(99, 10), gate(99, 10)],
        successor: GreenRole::HeldMiddle,
    };
    assert!(b.validate(10).is_ok());
    assert_eq!(b.earliest(9), Some(11));
    assert_eq!(b.earliest(10), Some(11));
    assert_eq!(brute(&b, 9, 12), Some(11));
    let b = HiddenBridge {
        hidden: vec![gate(10, 10)],
        successor: GreenRole::HeldMiddle,
    };
    assert_eq!(b.earliest(9), Some(11)); // either Move or update: next input
}

#[test]
fn unsupported_lifecycle_or_setting_is_rejected_not_silently_relaxed() {
    let nodes = [node(0, 1, 5), node(1, 6, 9)];
    let bridge = |hidden| {
        [HiddenBridge {
            hidden,
            successor: GreenRole::HeldMiddle,
        }]
    };
    assert!(compile(&nodes, &bridge(vec![gate(2, 4)])).is_err());
    assert!(compile(&nodes, &bridge(vec![gate(6, 8), gate(7, 7)])).is_err());
    assert!(compile(
        &nodes,
        &bridge(vec![HiddenGate {
            first_ready_frame: None,
            expires_after_frame: None
        }])
    )
    .is_err());
    assert!(compile_projected_chain(&[GreenChainVariant {
        nodes: &nodes,
        bridges: &bridge(vec![]),
        adjust_b: 1
    }])
    .is_err());
}

#[test]
fn redundant_hidden_runs_disappear_and_strict_variants_keep_strongest_relation() {
    let nodes = [node(0, 10, 12), node(1, 14, 18)];
    let early = [HiddenBridge {
        hidden: vec![gate(10, 17)],
        successor: GreenRole::HeldMiddle,
    }];
    assert!(compile(&nodes, &early).unwrap().constraints[0]
        .earliest_after
        .is_none());
    let late = [HiddenBridge {
        hidden: vec![gate(15, 17)],
        successor: GreenRole::HeldMiddle,
    }];
    let c = compile_projected_chain(&[
        GreenChainVariant {
            nodes: &nodes,
            bridges: &early,
            adjust_b: 0,
        },
        GreenChainVariant {
            nodes: &nodes,
            bridges: &late,
            adjust_b: 0,
        },
    ])
    .unwrap();
    for a in 10..=12 {
        for b in 14..=18 {
            assert_eq!(c.constraints[0].allows_frames(a, b), b >= 16);
        }
    }
}

#[test]
fn projection_propagates_through_visible_chain_and_installs_atomically() {
    let nodes = [node(0, 10, 13), node(1, 11, 14), node(2, 12, 15)];
    let bridges = [
        HiddenBridge {
            hidden: vec![gate(12, 14)],
            successor: GreenRole::HeldMiddle,
        },
        HiddenBridge {
            hidden: vec![gate(14, 16)],
            successor: GreenRole::PlainEnd,
        },
    ];
    let c = compile(&nodes, &bridges).unwrap();
    assert_eq!(domain(c.windows[1].1).0, 13);
    assert_eq!(domain(c.windows[2].1).0, 14);
    let mut input = ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        notes: nodes
            .iter()
            .map(|n| ReplayNote {
                time: (10 + n.note_index * 2) as f64 / 120.,
                trigger: None,
                window: n.window,
            })
            .collect(),
        hit_frames: vec![11, 13, 14],
        hit_order: vec![],
        skills: vec![],
        initial_combo: 49,
        medley_combo: false,
        fever_frames: Some([12, 15]),
        constraints: vec![],
        native_time_routing: None,
    };
    c.apply_to(&mut input).unwrap();
    assert_eq!(input.notes.len(), 3);
    assert_eq!(input.constraints.len(), 2);
    let result = search::optimize(&input, search::SearchOptions::default()).unwrap();
    assert!(result.complete);
    for edge in &input.constraints {
        assert!(edge.allows_frames(
            result.result.hit_frames[edge.before],
            result.result.hit_frames[edge.after]
        ));
    }
    assert!(replay(&input).is_ok());
    let mut bad = input.clone();
    bad.hit_frames = vec![11, 12, 14];
    let before = serde_json::to_value(&bad).unwrap();
    assert!(c.apply_to(&mut bad).is_err());
    assert_eq!(serde_json::to_value(&bad).unwrap(), before);
}
