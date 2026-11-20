use super::*;
use crate::hit_timing::{
    independent::{machine::Costs, machine::Machine, solve},
    NativeTimeConnection, NativeTimeRouting, NativeTimeTarget, NoteConstraint, ReplayNote,
};

fn fixture() -> ReplayInput {
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 19,
        medley_combo: false,
        fever_frames: Some([37, 54]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
        notes: vec![
            ReplayNote {
                time: 0.25,
                trigger: Some(0),
                window: NoteWindow::Standard,
            },
            ReplayNote {
                time: 0.34,
                trigger: None,
                window: NoteWindow::Standard,
            },
            ReplayNote {
                time: 0.45,
                trigger: None,
                window: NoteWindow::Standard,
            },
            ReplayNote {
                time: 0.55,
                trigger: None,
                window: NoteWindow::PinnedPerfect { frame: 66 },
            },
        ],
        hit_frames: vec![30, 41, 54, 66],
        skills: vec![ReplaySkill {
            duration: 0.1,
            score: ScoreEffect::Continuous {
                high: 140.,
                low: 60.,
            },
            correction: Some(JudgementCorrection::Good),
        }],
    }
}

fn factor_key(factor: &Factor) -> (bool, bool, bool, u64, usize) {
    (
        factor.perfect,
        factor.inside,
        factor.fever,
        factor.reference.to_bits(),
        factor.rank,
    )
}

fn check(
    input: &ReplayInput,
    request: u32,
    correction: u32,
    references: Option<&[f64]>,
    outside_correction: u32,
    trigger_before: bool,
) {
    let prepared = Prepared::new(input).unwrap();
    let rules = || RowRules {
        references,
        outside_correction,
        trigger_before,
    };
    let eager = prepared
        .row_with_rules(0, request, 13, correction, rules())
        .unwrap();
    let lazy = prepared
        .row_without_envelope(0, request, 13, correction, rules())
        .unwrap();
    assert!(lazy.envelope.is_empty());
    assert_eq!(eager.domains, lazy.domains);
    assert_eq!(eager.spans, lazy.spans);
    assert_eq!(eager.incident, lazy.incident);
    assert_eq!(eager.references, lazy.references);
    let relation_keys = |row: &Row| {
        row.relations
            .iter()
            .map(|r| (r.before, r.after, r.gap, r.strict_event_order))
            .collect::<Vec<_>>()
    };
    assert_eq!(relation_keys(&eager), relation_keys(&lazy));
    let envelope = prepared.envelope(&lazy);
    let factor_keys = |factors: &[Vec<Factor>]| {
        factors
            .iter()
            .map(|note| note.iter().map(factor_key).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    assert_eq!(factor_keys(&eager.envelope), factor_keys(&envelope));
    assert_eq!(envelope.len(), input.notes.len());
    for (note, factors) in envelope.iter().enumerate() {
        for (factor, domain) in factors.iter().zip(&lazy.domains[note]) {
            // Count directly to check the optimistic Combo rank independently
            // of the sorted-start lookup used by envelope construction.
            assert_eq!(
                factor.rank,
                lazy.spans.iter().filter(|s| s.0 <= domain.last).count()
            );
            assert_eq!(
                factor.reference.to_bits(),
                references.map_or(0., |r| r[note]).to_bits()
            );
        }
    }
    assert!(lazy.domains[0].iter().all(|d| d.last <= request));
    if trigger_before {
        assert!(lazy.domains[0].iter().any(|d| d.first < request));
    } else {
        assert!(lazy.domains[0].iter().all(|d| d.first == request));
    }

    let machine = Machine::new(&input.skills[0], input.notes.len()).unwrap();
    let reference_total = references.map_or(0., |r| r.iter().sum());
    let mut eager_costs = Costs::net(input, &machine, reference_total).unwrap();
    let mut lazy_costs = Costs::net(input, &machine, reference_total).unwrap();
    let eager_upper = eager_costs.upper(&eager.envelope).unwrap();
    let lazy_upper = lazy_costs.upper(&envelope).unwrap();
    let sparse_envelope = prepared.net_envelope_only(0, request, 13, correction, rules()).unwrap();
    let sparse_upper = lazy_costs.upper(&sparse_envelope).unwrap();
    assert_eq!(sparse_upper.coefficient.to_bits(), eager_upper.coefficient.to_bits());
    assert_eq!(sparse_upper.score, eager_upper.score);
    assert_eq!(
        eager_upper.coefficient.to_bits(),
        lazy_upper.coefficient.to_bits()
    );
    assert_eq!(eager_upper.score, lazy_upper.score);
    let options = WindowOptions::default();
    let mut eager_stats = WindowStats::default();
    let mut lazy_stats = WindowStats::default();
    let eager_graph = prepared
        .compile(&eager, true, options, &mut eager_stats)
        .unwrap();
    let lazy_graph = prepared
        .compile(&lazy, true, options, &mut lazy_stats)
        .unwrap();
    assert_eq!(eager_graph.edge_count, lazy_graph.edge_count);
    assert_eq!(eager_graph.note_count, lazy_graph.note_count);
    assert_eq!(eager_graph.layers.len(), lazy_graph.layers.len());
    for (a, b) in eager_graph.layers.iter().zip(&lazy_graph.layers) {
        assert_eq!(a.count, b.count);
        let edges = |layer: &Layer| {
            layer
                .edges
                .iter()
                .map(|e| (e.from, e.to, e.action))
                .collect::<Vec<_>>()
        };
        assert_eq!(edges(a), edges(b));
    }
    assert_eq!(eager_graph.actions.len(), lazy_graph.actions.len());
    for (a, b) in eager_graph.actions.iter().zip(&lazy_graph.actions) {
        assert_eq!(factor_key(&a.factor), factor_key(&b.factor));
        assert_eq!(a.hits, b.hits);
    }
    let eager_value = solve::evaluate(
        &eager_graph,
        &mut eager_costs,
        WindowObjective::Continuous,
        options,
        &mut eager_stats,
    )
    .unwrap()
    .unwrap()
    .unwrap();
    let lazy_value = solve::evaluate(
        &lazy_graph,
        &mut lazy_costs,
        WindowObjective::Continuous,
        options,
        &mut lazy_stats,
    )
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(
        eager_value.0.coefficient.to_bits(),
        lazy_value.0.coefficient.to_bits()
    );
    assert_eq!(eager_value.0.score, lazy_value.0.score);
    assert_eq!(eager_value.1, lazy_value.1);
    assert!(lazy_value.1.iter().all(|&f| f != u32::MAX));
}

#[test]
fn deferred_envelope_preserves_default_and_delayed_corrected_rows() {
    let input = fixture();
    check(&input, 30, 0, None, 0, false);
    check(&input, 42, 2, Some(&[0.013, 0.029, 0.047, 0.061]), 3, true);
}

#[test]
fn deferred_envelope_preserves_native_bindings_and_order_constraints() {
    let mut input = fixture();
    input.notes[2].window = NoteWindow::WidenedEndpoint;
    input.notes[2].trigger = Some(1);
    input.skills.push(input.skills[0].clone());
    input.constraints = vec![NoteConstraint {
        before: 1,
        after: 2,
        minimum_gap_frames: 3,
        require_dispatch_order: false,
        earliest_after: None,
    }];
    input.native_time_routing = Some(NativeTimeRouting {
        distance_mode: Default::default(),
        music_positions: (0..=100).map(|f| f as f32 / 120. * 100000.).collect(),
        targets: input
            .notes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                (i < 3).then_some(NativeTimeTarget {
                    button: if i == 0 { 1 } else { 2 },
                    absolute_pos: (n.time * 100000.).round() as i32,
                    active_from_frame: 0,
                    active_until_frame: 100,
                })
            })
            .collect(),
        candidate_order: vec![1, 0],
        connections: vec![NativeTimeConnection {
            head: 1,
            end: 2,
            finger_id: 0,
            release_inside: true,
        }],
        event_order: vec![1, 0, 2, 3],
    });
    check(&input, 36, 2, Some(&[0.013, 0.029, 0.047, 0.061]), 3, true);
}

#[test]
fn deferred_envelope_keeps_empty_trigger_domains_infeasible() {
    let mut input = fixture();
    input.notes[0].window = NoteWindow::PinnedPerfect { frame: 30 };
    let prepared = Prepared::new(&input).unwrap();
    assert!(prepared
        .row_with_rules(0, 29, 13, 3, RowRules::default())
        .is_none());
    assert!(prepared
        .row_without_envelope(0, 29, 13, 3, RowRules::default())
        .is_none());
}

fn storage_fixture(native: bool) -> ReplayInput {
    let mut input = fixture();
    input.initial_combo = 48;
    input.fever_frames = Some([35, 39]);
    input.notes = [20, 32, 36, 36, 37, 80, 81]
        .into_iter()
        .enumerate()
        .map(|(note, frame)| ReplayNote {
            time: f64::from(frame) / 120.,
            trigger: (note == 0).then_some(0),
            window: if (2..=4).contains(&note) {
                NoteWindow::Standard
            } else {
                NoteWindow::PinnedPerfect { frame }
            },
        })
        .collect();
    input.hit_frames = vec![20, 32, 36, 36, 37, 80, 81];
    input.skills[0] = ReplaySkill {
        duration: 0.175,
        score: ScoreEffect::Plain { percent: 100. },
        correction: None,
    };
    if native {
        input.notes[2].trigger = Some(1);
        input.notes[3].trigger = Some(2);
        input.notes[3].time = 37. / 120.;
        input.notes[3].window = NoteWindow::WidenedEndpoint;
        input.notes[4].time = 38. / 120.;
        input.hit_frames[3] = 37;
        input.hit_frames[4] = 38;
        input.skills = vec![input.skills[0].clone(); 3];
        input.constraints = vec![
            NoteConstraint {
                before: 2,
                after: 3,
                minimum_gap_frames: 0,
                require_dispatch_order: false,
                earliest_after: None,
            },
            NoteConstraint {
                before: 2,
                after: 4,
                minimum_gap_frames: 0,
                require_dispatch_order: false,
                earliest_after: None,
            },
        ];
        input.native_time_routing = Some(NativeTimeRouting {
            distance_mode: Default::default(),
            music_positions: (0..=100).map(|f| f as f32 / 120. * 100000.).collect(),
            targets: input
                .notes
                .iter()
                .enumerate()
                .map(|(note, n)| {
                    (2..=4).contains(&note).then_some(NativeTimeTarget {
                        button: 2,
                        absolute_pos: (n.time * 100000.).round() as i32,
                        active_from_frame: 20,
                        active_until_frame: 70,
                    })
                })
                .collect(),
            candidate_order: vec![4, 2],
            connections: vec![NativeTimeConnection {
                head: 2,
                end: 3,
                finger_id: 0,
                release_inside: true,
            }],
            // Trigger 1 must precede trigger 2, so this dispatch order rules
            // out their same-frame tie even though the explicit gap is zero.
            event_order: vec![0, 1, 3, 2, 4, 5, 6],
        });
    }
    input
}

fn assert_graph_semantics(original: &Graph, reused: &Graph) {
    assert_eq!(original.note_count, reused.note_count);
    assert_eq!(original.edge_count, reused.edge_count);
    assert_eq!(original.layers.len(), reused.layers.len());
    for (layer_index, (a, b)) in original.layers.iter().zip(&reused.layers).enumerate() {
        assert_eq!(a.count, b.count, "layer {layer_index}");
        assert_eq!(a.edges.len(), b.edges.len(), "layer {layer_index}");
        for (edge_index, (a, b)) in a.edges.iter().zip(&b.edges).enumerate() {
            assert_eq!(
                (a.from, a.to),
                (b.from, b.to),
                "layer {layer_index}, edge {edge_index}"
            );
            let meaning = |graph: &Graph, action: Option<usize>| {
                action.map(|index| {
                    let action = &graph.actions.get(index);
                    (factor_key(&action.factor), action.hits.clone())
                })
            };
            assert_eq!(
                meaning(original, a.action),
                meaning(reused, b.action),
                "layer {layer_index}, edge {edge_index}"
            );
        }
    }
}

fn compile_storage_pair(
    prepared: &Prepared,
    row: &Row,
    collapse: bool,
    options: WindowOptions,
) -> (Option<Graph>, Option<Graph>, WindowStats, WindowStats) {
    let mut original_stats = WindowStats::default();
    let mut reused_stats = WindowStats::default();
    let original = prepared.compile(
        row,
        collapse,
        WindowOptions {
            reuse_graph_storage: false,
            max_cached_component_edges: 0,
            ..options
        },
        &mut original_stats,
    );
    let reused = prepared.compile(
        row,
        collapse,
        WindowOptions {
            reuse_graph_storage: true,
            max_cached_component_edges: 0,
            ..options
        },
        &mut reused_stats,
    );
    assert_eq!(original.is_some(), reused.is_some());
    assert_eq!(original_stats.graph_edges, reused_stats.graph_edges);
    assert_eq!(
        original_stats.graph_peak_states,
        reused_stats.graph_peak_states
    );
    assert_eq!(original_stats.collapsed_notes, reused_stats.collapsed_notes);
    assert_eq!(original_stats.graph_reuse_builds, 0);
    assert_eq!(reused_stats.graph_reuse_builds, 1);
    if let (Some(original), Some(reused)) = (&original, &reused) {
        assert_graph_semantics(original, reused);
        assert_eq!(original_stats.graph_actions, original.actions.len());
        assert_eq!(reused_stats.graph_actions, reused.actions.len());
        assert_eq!(
            original_stats.graph_actions,
            reused_stats.graph_actions + reused_stats.graph_action_reuses
        );
    }
    (original, reused, original_stats, reused_stats)
}

#[test]
fn graph_storage_reuse_preserves_actions_values_and_exact_witnesses() {
    let effects = [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::Continuous {
            high: 140.,
            low: 60.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 102,
        },
    ];
    let mut original_actions = 0;
    let mut reused_actions = 0;
    let mut action_reuses = 0;
    let mut key_reuses = 0;
    for native in [false, true] {
        let input = storage_fixture(native);
        replay(&input).unwrap();
        let prepared = Prepared::new(&input).unwrap();
        for reference in [0.01, 20.] {
            let references: Vec<_> = (0..input.notes.len())
                .map(|note| reference * (note + 1) as f64)
                .collect();
            let row = prepared
                .row_with_rules(
                    0,
                    20,
                    21,
                    0,
                    RowRules {
                        references: Some(&references),
                        outside_correction: 3,
                        trigger_before: true,
                    },
                )
                .unwrap();
            if native {
                assert!(row.relations.iter().any(|relation| {
                    relation.before == 2 && relation.after == 3 && relation.strict_event_order
                }));
            }
            for collapse in [false, true] {
                let options = WindowOptions::default();
                let (original, reused, old_stats, new_stats) =
                    compile_storage_pair(&prepared, &row, collapse, options);
                let (original, reused) = (original.unwrap(), reused.unwrap());
                assert_eq!(new_stats.collapsed_notes > 0, collapse);
                original_actions += old_stats.graph_actions;
                reused_actions += new_stats.graph_actions;
                action_reuses += new_stats.graph_action_reuses;
                key_reuses += new_stats.graph_key_reuses;
                for score in effects {
                    let skill = ReplaySkill {
                        score,
                        ..input.skills[0].clone()
                    };
                    let machine = Machine::new(&skill, input.notes.len()).unwrap();
                    for net in [false, true] {
                        for objective in [WindowObjective::Continuous, WindowObjective::Native] {
                            let new_costs = || {
                                if net {
                                    Costs::net(&input, &machine, references.iter().sum())
                                } else {
                                    Costs::new(
                                        &input,
                                        ScoringPower::new(input.power).unwrap(),
                                        &machine,
                                    )
                                }
                                .unwrap()
                            };
                            let mut old_costs = new_costs();
                            let mut new_costs = new_costs();
                            let mut old_stats = WindowStats::default();
                            let mut new_stats = WindowStats::default();
                            let old_value = solve::evaluate_ordered(
                                &original,
                                &mut old_costs,
                                objective,
                                options,
                                &mut old_stats,
                            )
                            .unwrap()
                            .unwrap()
                            .unwrap();
                            let new_value = solve::evaluate_ordered(
                                &reused,
                                &mut new_costs,
                                objective,
                                options,
                                &mut new_stats,
                            )
                            .unwrap()
                            .unwrap()
                            .unwrap();
                            assert_eq!(
                                old_value.0.coefficient.to_bits(),
                                new_value.0.coefficient.to_bits()
                            );
                            assert_eq!(old_value.0.score, new_value.0.score);
                            assert_eq!(old_value.1, new_value.1);
                            assert_eq!(old_stats.transitions, new_stats.transitions);
                            assert_eq!(old_stats.peak_value_states, new_stats.peak_value_states);
                            if net && objective == WindowObjective::Continuous {
                                assert_eq!(new_value.0.coefficient < 0., reference == 20.);
                            }
                            if native {
                                // Baseline event ranks are no longer binding.
                                // Equal frames are legal iff the witness puts
                                // the earlier trigger first.
                                let plan = &new_value.1;
                                let a = plan.order.iter().position(|&i| i == 2).unwrap();
                                let b = plan.order.iter().position(|&i| i == 3).unwrap();
                                assert!((plan.frames[2], a) < (plan.frames[3], b));
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(reused_actions < original_actions);
    assert!(action_reuses > 0);
    assert!(key_reuses > 0);
}

#[test]
fn graph_storage_reuse_keeps_each_layer_budget_boundary() {
    let input = storage_fixture(false);
    let prepared = Prepared::new(&input).unwrap();
    let references: Vec<_> = (0..input.notes.len()).map(|i| 0.013 * i as f64).collect();
    let row = prepared
        .row_with_rules(
            0,
            20,
            21,
            0,
            RowRules {
                references: Some(&references),
                outside_correction: 0,
                trigger_before: true,
            },
        )
        .unwrap();
    for collapse in [false, true] {
        let options = WindowOptions::default();
        let (original, _, _, _) = compile_storage_pair(&prepared, &row, collapse, options);
        let original = original.unwrap();
        let mut edge_budgets = std::collections::BTreeSet::from([1]);
        let mut state_budgets = std::collections::BTreeSet::from([1]);
        let mut edges = 0;
        for layer in &original.layers {
            edges += layer.edges.len();
            edge_budgets.extend([edges.saturating_sub(1).max(1), edges.max(1), edges + 1]);
            state_budgets.extend([
                layer.count.saturating_sub(1).max(1),
                layer.count.max(1),
                layer.count + 1,
            ]);
        }
        for max_graph_edges in edge_budgets {
            let (graph, _, _, _) = compile_storage_pair(
                &prepared,
                &row,
                collapse,
                WindowOptions {
                    max_graph_edges,
                    ..options
                },
            );
            assert_eq!(graph.is_some(), max_graph_edges >= original.edge_count);
        }
        let peak_states = original
            .layers
            .iter()
            .map(|layer| layer.count)
            .max()
            .unwrap();
        for max_states in state_budgets {
            let (graph, _, _, _) = compile_storage_pair(
                &prepared,
                &row,
                collapse,
                WindowOptions {
                    max_states,
                    ..options
                },
            );
            assert_eq!(graph.is_some(), max_states >= peak_states);
        }
    }
}

#[test]
fn graph_storage_reuse_preserves_forced_same_frame_dispatch_and_binding_failures() {
    let mut input = fixture();
    input.notes = (0..3)
        .map(|note| ReplayNote {
            time: 0.3,
            trigger: match note {
                0 => Some(0),
                2 => Some(1),
                _ => None,
            },
            window: if note == 2 {
                NoteWindow::WidenedEndpoint
            } else {
                NoteWindow::Standard
            },
        })
        .collect();
    input.hit_frames = vec![35, 35, 36];
    input.skills = vec![input.skills[0].clone(); 2];
    input.constraints = vec![NoteConstraint {
        before: 0,
        after: 1,
        minimum_gap_frames: 0,
        require_dispatch_order: false,
        earliest_after: None,
    }];
    input.native_time_routing = Some(NativeTimeRouting {
        distance_mode: Default::default(),
        music_positions: (0..=100).map(|f| f as f32 / 120. * 100000.).collect(),
        targets: (0..3)
            .map(|note| {
                Some(NativeTimeTarget {
                    button: if note == 0 { 1 } else { 2 },
                    absolute_pos: 30000,
                    active_from_frame: if note == 2 { 36 } else { 35 },
                    active_until_frame: 36,
                })
            })
            .collect(),
        candidate_order: vec![1, 0],
        connections: vec![NativeTimeConnection {
            head: 1,
            end: 2,
            finger_id: 0,
            release_inside: true,
        }],
        event_order: vec![],
    });
    for (order, feasible) in [
        (vec![0, 1, 2], true),
        (vec![1, 2, 0], false), // The later trigger dispatches first.
        (vec![0, 2, 1], false), // The endpoint dispatches before its bound head.
    ] {
        input.native_time_routing.as_mut().unwrap().event_order = order;
        replay(&input).unwrap();
        let prepared = Prepared::new(&input).unwrap();
        let row = prepared.row(0, 36, 13, 0).unwrap();
        for collapse in [false, true] {
            for optimize_touch_order in [false, true] {
                let options = WindowOptions {
                    optimize_touch_order,
                    ..WindowOptions::default()
                };
                let (original, reused, _, _) =
                    compile_storage_pair(&prepared, &row, collapse, options);
                let (original, reused) = (original.unwrap(), reused.unwrap());
                let machine = Machine::new(&input.skills[0], input.notes.len()).unwrap();
                let evaluate = |graph: &Graph| {
                    solve::evaluate_ordered(
                        graph,
                        &mut Costs::net(&input, &machine, 0.).unwrap(),
                        WindowObjective::Continuous,
                        options,
                        &mut WindowStats::default(),
                    )
                    .unwrap()
                    .unwrap()
                    .map(|(value, hits)| (value.coefficient.to_bits(), value.score, hits))
                };
                let old = evaluate(&original);
                let new = evaluate(&reused);
                assert_eq!(old, new);
                assert_eq!(new.is_some(), feasible || optimize_touch_order);
                if let Some((_, _, hits)) = new {
                    assert_eq!(hits.frames, vec![36; 3]);
                    // Either independent press may be first; both must precede
                    // the bound endpoint / second trigger.
                    assert_eq!(hits.order.last(), Some(&2));
                    let mut witness = input.clone();
                    witness.hit_frames = hits.frames;
                    witness.hit_order = hits.order;
                    replay(&witness).unwrap();
                }
            }
        }
    }
}

fn component_fixture(native: bool) -> ReplayInput {
    let mut input = fixture();
    if native {
        input.native_time_routing = Some(NativeTimeRouting {
            distance_mode: Default::default(),
            music_positions: (0..=120).map(|f| f as f32 / 120. * 100000.).collect(),
            targets: input
                .notes
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    (i < 3).then_some(NativeTimeTarget {
                        button: (i % 2) as u8,
                        absolute_pos: (n.time * 100000.).round() as i32,
                        active_from_frame: 0,
                        active_until_frame: 120,
                    })
                })
                .collect(),
            candidate_order: vec![2, 1, 0],
            event_order: vec![2, 1, 0, 3],
            connections: vec![],
        });
    }
    input
}

#[test]
fn component_templates_keep_graphs_values_and_witnesses_across_window_changes() {
    let mut hits = 0;
    let mut saved_edges = 0;
    for native in [false, true] {
        let input = component_fixture(native);
        replay(&input).unwrap();
        let prepared = Prepared::new(&input).unwrap();
        let baseline = WindowOptions {
            max_cached_component_edges: 0,
            inline_event_hits: false,
            ..Default::default()
        };
        let enabled = WindowOptions {
            max_cached_component_edges: 1_000_000,
            inline_event_hits: true,
            ..baseline
        };
        for (request, playing, correction, outside, reference) in [
            (30, 13, 0, 0, 0.013),
            (31, 13, 0, 0, 0.013),
            (30, 13, 0, 0, 0.019),
            (30, 14, 3, 0, 0.013),
            (31, 14, 2, 3, 0.013),
        ] {
            let refs: Vec<_> = (0..input.notes.len())
                .map(|i| reference * (i + 1) as f64)
                .collect();
            let row = prepared
                .row_with_rules(
                    0,
                    request,
                    playing,
                    correction,
                    RowRules {
                        references: Some(&refs),
                        outside_correction: outside,
                        trigger_before: true,
                    },
                )
                .unwrap();
            for collapse in [false, true] {
                let mut original_stats = WindowStats::default();
                let original = prepared
                    .compile(&row, collapse, baseline, &mut original_stats)
                    .unwrap();
                // Warm the templates, then compare the genuinely reused graph.
                prepared
                    .compile(&row, collapse, enabled, &mut WindowStats::default())
                    .unwrap();
                let mut stats = WindowStats::default();
                let cached = prepared
                    .compile(&row, collapse, enabled, &mut stats)
                    .unwrap();
                assert_graph_semantics(&original, &cached);
                assert_eq!(original.actions.len(), cached.actions.len());
                assert_eq!(original_stats.graph_edges, stats.graph_edges);
                assert_eq!(original_stats.graph_peak_states, stats.graph_peak_states);
                assert_eq!(original_stats.collapsed_notes, stats.collapsed_notes);
                hits += stats.component_cache_hits;
                saved_edges += stats.component_replayed_edges;
                for score in [
                    ScoreEffect::Plain { percent: 100. },
                    ScoreEffect::Continuous {
                        high: 140.,
                        low: 60.,
                    },
                    ScoreEffect::Crescendo {
                        percent: 100.,
                        step: 0.5,
                        cap: 102,
                    },
                ] {
                    let machine = Machine::new(
                        &ReplaySkill {
                            score,
                            ..input.skills[0].clone()
                        },
                        input.notes.len(),
                    )
                    .unwrap();
                    for objective in [WindowObjective::Continuous, WindowObjective::Native] {
                        let evaluate = |graph: &Graph| {
                            let mut costs = Costs::new(
                                &input,
                                ScoringPower::new(input.power).unwrap(),
                                &machine,
                            )
                            .unwrap();
                            solve::evaluate(
                                graph,
                                &mut costs,
                                objective,
                                enabled,
                                &mut WindowStats::default(),
                            )
                            .unwrap()
                        };
                        let a = evaluate(&original);
                        let b = evaluate(&cached);
                        match (a, b) {
                            (Some(Some((a, ah))), Some(Some((b, bh)))) => {
                                assert_eq!(a.coefficient.to_bits(), b.coefficient.to_bits());
                                assert_eq!(a.score, b.score);
                                assert_eq!(ah, bh);
                            }
                            (Some(None), Some(None)) => {}
                            _ => {
                                panic!("component reuse changed feasibility or exhausted a budget")
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(hits > 0);
    assert!(saved_edges > 100);
}

#[test]
fn warmed_component_templates_preserve_every_layer_budget_boundary() {
    let input = fixture();
    let prepared = Prepared::new(&input).unwrap();
    let row = prepared.row(0, 30, 13, 0).unwrap();
    let base = WindowOptions {
        max_cached_component_edges: 0,
        inline_event_hits: false,
        ..Default::default()
    };
    let warm = WindowOptions {
        max_cached_component_edges: 1_000_000,
        inline_event_hits: true,
        ..base
    };
    for collapse in [false, true] {
        let graph = prepared
            .compile(&row, collapse, warm, &mut WindowStats::default())
            .unwrap();
        let mut edges = 0;
        let mut edge_budgets = std::collections::BTreeSet::new();
        let mut state_budgets = std::collections::BTreeSet::new();
        for layer in &graph.layers {
            edges += layer.edges.len();
            edge_budgets.extend([edges.saturating_sub(1).max(1), edges, edges + 1]);
            state_budgets.extend([
                layer.count.saturating_sub(1).max(1),
                layer.count.max(1),
                layer.count + 1,
            ]);
        }
        let limits = edge_budgets
            .into_iter()
            .map(|max_graph_edges| WindowOptions {
                max_graph_edges,
                ..base
            })
            .chain(
                state_budgets
                    .into_iter()
                    .map(|max_states| WindowOptions { max_states, ..base }),
            );
        for options in limits {
            let mut a = WindowStats::default();
            let mut b = WindowStats::default();
            let original = prepared.compile(&row, collapse, options, &mut a);
            let cached = prepared.compile(
                &row,
                collapse,
                WindowOptions {
                    max_cached_component_edges: 1_000_000,
                    inline_event_hits: true,
                    ..options
                },
                &mut b,
            );
            assert_eq!(original.is_some(), cached.is_some());
            assert_eq!(a.graph_edges, b.graph_edges);
            assert_eq!(a.graph_peak_states, b.graph_peak_states);
            assert_eq!(a.graph_actions, b.graph_actions);
            assert_eq!(a.collapsed_notes, b.collapsed_notes);
            if let (Some(a), Some(b)) = (original, cached) {
                assert_graph_semantics(&a, &b);
            }
        }
    }
}

#[test]
fn component_templates_bypass_bound_connections_and_respect_capacity() {
    for native in [false, true] {
        let input = storage_fixture(native);
        let prepared = Prepared::new(&input).unwrap();
        let row = prepared.row(0, 20, 21, 0).unwrap();
        let baseline = WindowOptions {
            max_cached_component_edges: 0,
            ..Default::default()
        };
        let original = prepared
            .compile(&row, false, baseline, &mut WindowStats::default())
            .unwrap();
        for capacity in [1, 1_000_000] {
            let options = WindowOptions {
                max_cached_component_edges: capacity,
                ..baseline
            };
            prepared
                .compile(&row, false, options, &mut WindowStats::default())
                .unwrap();
            let mut stats = WindowStats::default();
            let graph = prepared.compile(&row, false, options, &mut stats).unwrap();
            assert_graph_semantics(&original, &graph);
            if capacity == 1 {
                assert_eq!(stats.component_cache_hits, 0);
                assert!(stats.component_cache_skips > 0);
            }
        }
    }
}

#[test]
fn inline_events_keep_fresh_and_cached_graphs_and_cannot_leak_across_modes() {
    for native in [false, true] {
        for collapse in [false, true] {
            let input = component_fixture(native);
            let prepared = Prepared::new(&input).unwrap();
            let references = vec![20.; input.notes.len()];
            let row = prepared
                .row_with_rules(
                    0,
                    30,
                    13,
                    0,
                    RowRules {
                        references: Some(&references),
                        outside_correction: 3,
                        trigger_before: true,
                    },
                )
                .unwrap();
            let old_options = WindowOptions {
                inline_event_hits: false,
                max_cached_component_edges: 1_000_000,
                ..Default::default()
            };
            let new_options = WindowOptions {
                inline_event_hits: true,
                ..old_options
            };
            let original = prepared
                .compile(&row, collapse, old_options, &mut WindowStats::default())
                .unwrap();
            assert!(original
                .actions
                .iter()
                .all(|a| matches!(a.hits, ActionHits::Run(_))));
            let mut first_stats = WindowStats::default();
            let first = prepared
                .compile(&row, collapse, new_options, &mut first_stats)
                .unwrap();
            assert_eq!(
                first_stats.component_cache_hits, 0,
                "old-mode templates must not mask new allocations"
            );
            let mut warm_stats = WindowStats::default();
            let warm = prepared
                .compile(&row, collapse, new_options, &mut warm_stats)
                .unwrap();
            assert!(warm_stats.component_cache_hits > 0);
            assert_eq!(warm_stats.inline_event_builds, 1);
            for graph in [&first, &warm] {
                assert_graph_semantics(&original, graph);
                assert!(graph
                    .actions
                    .iter()
                    .any(|a| matches!(a.hits, ActionHits::Event(_))));
            }
            for score in [
                ScoreEffect::Plain { percent: 100. },
                ScoreEffect::Continuous {
                    high: 140.,
                    low: 60.,
                },
                ScoreEffect::Crescendo {
                    percent: 100.,
                    step: 0.5,
                    cap: 102,
                },
            ] {
                let machine = Machine::new(
                    &ReplaySkill {
                        score,
                        ..input.skills[0].clone()
                    },
                    input.notes.len(),
                )
                .unwrap();
                for net in [false, true] {
                    for objective in [WindowObjective::Continuous, WindowObjective::Native] {
                        let evaluate = |graph: &Graph| {
                            let mut costs = if net {
                                Costs::net(&input, &machine, references.iter().sum())
                            } else {
                                Costs::new(
                                    &input,
                                    ScoringPower::new(input.power).unwrap(),
                                    &machine,
                                )
                            }
                            .unwrap();
                            let mut stats = WindowStats::default();
                            let result = solve::evaluate(
                                graph,
                                &mut costs,
                                objective,
                                new_options,
                                &mut stats,
                            )
                            .unwrap();
                            (
                                result
                                    .map(|r| r.map(|(v, h)| (v.coefficient.to_bits(), v.score, h))),
                                stats.transitions,
                                stats.peak_value_states,
                            )
                        };
                        let expected = evaluate(&original);
                        assert_eq!(expected, evaluate(&first));
                        assert_eq!(expected, evaluate(&warm));
                    }
                }
            }
        }
    }
}

#[test]
fn component_keys_distinguish_projected_arrival_relations() {
    use crate::hit_timing::ArrivalCurve;
    let prepared = Prepared::new(&fixture()).unwrap();
    let mut row = prepared
        .row_without_envelope(0, 30, 13, 0, RowRules::default())
        .unwrap();
    let entry = Key {
        marks: vec![],
        bindings: vec![],
    };
    let options = WindowOptions::default();
    let settled = vec![false; prepared.input.notes.len()];
    let indices: Vec<_> = (0..prepared.input.notes.len()).collect();
    row.relations = vec![Relation {
        before: 1,
        after: 2,
        gap: 1,
        strict_event_order: true,
        earliest_after: Some(std::sync::Arc::new(
            ArrivalCurve::from_samples(35, &[Some(40), Some(41)]).unwrap(),
        )),
    }];
    let a = ComponentKey::new(&prepared, &row, &indices, &entry, &settled, 0, options);
    row.relations[0].earliest_after = Some(std::sync::Arc::new(
        ArrivalCurve::from_samples(35, &[Some(41), Some(41)]).unwrap(),
    ));
    let b = ComponentKey::new(&prepared, &row, &indices, &entry, &settled, 0, options);
    assert!(a != b);
}
