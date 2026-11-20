use super::*;
use crate::hit_timing::independent::geometry::{Action, Edge, Factor, Layer};
use crate::hit_timing::{NoteWindow, ReplayNote, ScoreEffect};

#[test]
fn interval_jump_preserves_reordered_geometry_frontiers_and_witnesses() {
    struct IntervalCosts;
    impl machine::ActionCosts for IntervalCosts {
        fn supports_temporal_intervals(&self) -> bool { true }
        fn stable_interval(&self, _: usize, _: u32, _: u32) -> bool { true }
        fn hit(&mut self, factor: Factor, state: usize) -> Result<(usize, Value), ReplayError> {
            Ok((state, Value { score: factor.reference as u64, coefficient: 0. }))
        }
        fn run(&mut self, _: Factor, _: usize, _: usize) -> Result<(usize, Value), ReplayError> {
            unreachable!()
        }
    }
    let action = |note, frame, score| Action {
        factor: Factor { perfect: true, inside: false, slot: 0, fever: false,
            reference: score, rank: note + 1 },
        hits: vec![(note, frame)].into(),
    };
    let graph = Graph {
        actions: vec![action(0, 10, 10.), action(0, 10, 20.),
            action(1, 12, 100.), action(1, 12, 0.)].into(),
        layers: vec![
            Layer { count: 2, edges: vec![
                Edge { from: 0, to: 0, action: Some(0) },
                Edge { from: 0, to: 1, action: Some(1) },
            ].into() },
            Layer { count: 2, edges: vec![
                Edge { from: 0, to: 1, action: None },
                Edge { from: 1, to: 0, action: None },
            ].into() },
            Layer { count: 1, edges: vec![
                Edge { from: 0, to: 0, action: Some(2) },
                Edge { from: 1, to: 0, action: Some(3) },
            ].into() },
        ],
        note_count: 2, edge_count: 6,
        temporal: vec![geometry::TemporalBlock {
            start: 0, after_first: 1, resume: 2, first_frame: 10, last_frame: 11,
            count: 2, permutation: vec![1, 0],
        }],
        touches: vec![],
    };
    for compact in [false, true] {
        let options = WindowOptions { compact_value_states: compact, ..WindowOptions::default() };
        let mut stats = WindowStats::default();
        let (value, plan) = evaluate_ordered(&graph, &mut IntervalCosts,
            WindowObjective::Native, options, &mut stats).unwrap().unwrap().unwrap();
        assert_eq!(value.score, 120);
        assert_eq!(plan.frames, vec![10, 12]);
        assert_eq!(plan.order, vec![0, 1]);
        assert_eq!(stats.transitions, if compact { 4 } else { 6 });
        assert_eq!(stats.temporal_skipped_layers, usize::from(compact));
    }
}

fn fixture(score: ScoreEffect) -> ReplayInput {
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 48,
        medley_combo: false,
        fever_frames: Some([33, 40]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
        notes: (0..8)
            .map(|note| ReplayNote {
                time: f64::from(30 + note) / 120.,
                trigger: (note == 0).then_some(0),
                window: NoteWindow::PinnedPerfect { frame: 30 + note },
            })
            .collect(),
        hit_frames: (30..38).collect(),
        skills: vec![ReplaySkill {
            duration: 0.1,
            score,
            correction: None,
        }],
    }
}

/// Every complete path settles all eight notes: an outside hit, three
/// branching inside hits, one inside Perfect run, and an outside tail.
fn branching_graph(reference: f64) -> Graph {
    let mut graph = Graph {
        layers: vec![],
        actions: vec![].into(),
        note_count: 8,
        edge_count: 0,
        temporal: vec![],
        touches: vec![],
    };
    let mut previous_count = 1;
    for (note, length, inside, count) in [
        (0, 1, false, 1),
        (1, 1, true, 2),
        (2, 1, true, 1),
        (3, 1, true, 2),
        (4, 3, true, 2),
        (7, 1, false, 1),
    ] {
        let mut edges = vec![];
        for from in 0..previous_count {
            let grades: &[bool] = if length == 1 && inside {
                &[true, false]
            } else {
                &[true]
            };
            for &perfect in grades {
                let action = graph.actions.len();
                graph.actions.push(Action {
                    factor: Factor {
                        perfect,
                        inside,
                        slot: u8::from(inside),
                        fever: note >= 3,
                        reference: (note..note + length)
                            .map(|i| reference + i as f64 * 0.05)
                            .sum(),
                        rank: note + 1,
                    },
                    hits: (note..note + length).map(|i| (i, 30 + i as u32)).collect(),
                });
                edges.push(Edge {
                    from,
                    to: if count == 1 {
                        0
                    } else if length > 1 {
                        from
                    } else {
                        usize::from(!perfect)
                    },
                    action: Some(action),
                });
            }
        }
        graph.edge_count += edges.len();
        graph.layers.push(Layer { count, edges: edges.into() });
        // Empty actions must preserve every live skill state and its value.
        let edges: Vec<_> = (0..count)
            .map(|i| Edge {
                from: i,
                to: i,
                action: None,
            })
            .collect();
        graph.edge_count += edges.len();
        graph.layers.push(Layer { count, edges: edges.into() });
        previous_count = count;
    }
    graph
}

fn compare(
    input: &ReplayInput,
    graph: &Graph,
    options: WindowOptions,
    objective: WindowObjective,
    net: bool,
) -> (Option<Option<Value>>, WindowStats) {
    let options = WindowOptions {
        compact_value_states: false,
        ..options
    };
    let machine = Machine::new(&input.skills[0], input.notes.len()).unwrap();
    let new_costs = || {
        if net {
            Costs::net(input, &machine, 200.)
        } else {
            Costs::new(input, ScoringPower::new(input.power).unwrap(), &machine)
        }
        .unwrap()
    };
    let mut expected_costs = new_costs();
    let mut actual_costs = new_costs();
    let mut expected_stats = WindowStats::default();
    let mut actual_stats = WindowStats::default();
    let expected = evaluate(
        graph,
        &mut expected_costs,
        objective,
        options,
        &mut expected_stats,
    )
    .unwrap();
    let expected_hits = expected
        .as_ref()
        .map(|r| r.as_ref().map(|(_, hits)| hits.clone()));
    let expected = expected.map(|result| result.map(|(value, _)| value));
    let actual = evaluate_value(
        graph,
        &mut actual_costs,
        objective,
        options,
        &mut actual_stats,
    )
    .unwrap();
    let bits = |answer: Option<Option<Value>>| {
        answer.map(|value| value.map(|v| (v.coefficient.to_bits(), v.score)))
    };
    assert_eq!(bits(actual), bits(expected));
    assert_eq!(actual_stats.transitions, expected_stats.transitions);
    assert_eq!(
        actual_stats.peak_value_states,
        expected_stats.peak_value_states
    );
    assert_eq!(actual_costs.prefix_count(), expected_costs.prefix_count());
    for trace in [false, true] {
        let mut compact_costs = new_costs();
        let mut compact_stats = WindowStats::default();
        let options = WindowOptions {
            compact_value_states: true,
            ..options
        };
        let compact = if trace {
            let result = evaluate(
                graph,
                &mut compact_costs,
                objective,
                options,
                &mut compact_stats,
            )
            .unwrap();
            assert_eq!(
                result
                    .as_ref()
                    .map(|r| r.as_ref().map(|(_, hits)| hits.clone())),
                expected_hits
            );
            result.map(|r| r.map(|(value, _)| value))
        } else {
            evaluate_value(
                graph,
                &mut compact_costs,
                objective,
                options,
                &mut compact_stats,
            )
            .unwrap()
        };
        assert_eq!(bits(compact), bits(expected));
        assert_eq!(compact_stats.transitions, expected_stats.transitions);
        assert_eq!(
            compact_stats.peak_value_states,
            expected_stats.peak_value_states
        );
        assert_eq!(compact_stats.compact_value_queries, 1);
        assert_eq!(compact_costs.prefix_count(), expected_costs.prefix_count());
    }
    (actual, actual_stats)
}

fn effects() -> [ScoreEffect; 7] {
    [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. },
        ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::PerfectOrHalf { percent: 140. },
        ScoreEffect::Continuous {
            high: 140.,
            low: 60.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 102,
        },
        ScoreEffect::Crescendo {
            percent: 30.,
            step: 0.5,
            cap: 30,
        },
    ]
}

#[test]
fn value_only_is_bitwise_equal_for_all_effects_and_signed_net_values() {
    for score in effects() {
        let input = fixture(score);
        for reference in [0.5, 20.] {
            let graph = branching_graph(reference);
            for net in [false, true] {
                for objective in [WindowObjective::Continuous, WindowObjective::Native] {
                    let (answer, _) =
                        compare(&input, &graph, WindowOptions::default(), objective, net);
                    let value = answer.unwrap().unwrap();
                    if net && reference == 20. {
                        assert!(value.coefficient < 0.);
                    }
                }
            }
        }
    }
}

#[test]
fn value_only_keeps_exact_budget_boundaries_and_infeasibility() {
    for score in effects() {
        let input = fixture(score);
        let mut graph = branching_graph(0.5);
        let (_, stats) = compare(
            &input,
            &graph,
            WindowOptions::default(),
            WindowObjective::Continuous,
            true,
        );
        // Test every transition cutoff, including the first complete budget.
        for max_transitions in 1..=stats.transitions + 1 {
            compare(
                &input,
                &graph,
                WindowOptions {
                    max_transitions,
                    ..WindowOptions::default()
                },
                WindowObjective::Continuous,
                true,
            );
        }
        for max_states in 1..=stats.peak_value_states + 1 {
            compare(
                &input,
                &graph,
                WindowOptions {
                    max_states,
                    ..WindowOptions::default()
                },
                WindowObjective::Continuous,
                true,
            );
        }
        graph.layers.push(Layer {
            count: 0,
            edges: vec![].into(),
        });
        let (answer, _) = compare(
            &input,
            &graph,
            WindowOptions::default(),
            WindowObjective::Continuous,
            true,
        );
        assert!(matches!(answer, Some(None)));
    }
}

#[test]
fn compact_states_preserve_ties_and_discard_retired_nodes() {
    let input = fixture(ScoreEffect::Plain { percent: 0. });
    let mut graph = branching_graph(0.5);
    // Equal native scores, distinct frame witnesses, descending destination
    // states, and a final reduction from two live nodes to one. Stale capacity
    // must never become a final winner or affect its tie-breaking order.
    for layer in &mut graph.layers {
        layer.edges.wide_mut().reverse();
    }
    for (i, action) in graph.actions.wide_mut().iter_mut().enumerate() {
        for (_, frame) in &mut action.hits {
            *frame += i as u32;
        }
    }
    compare(
        &input,
        &graph,
        WindowOptions::default(),
        WindowObjective::Native,
        true,
    );
    graph.layers.clear();
    graph.actions.wide_mut().clear();
    graph.note_count = 0;
    compare(
        &input,
        &graph,
        WindowOptions::default(),
        WindowObjective::Continuous,
        true,
    );
}

#[test]
fn compact_states_keep_sparse_crescendo_ids_and_absorbing_cap() {
    let mut input = fixture(ScoreEffect::Crescendo {
        percent: 100.,
        step: 0.5,
        cap: 112,
    });
    input.notes.resize(40, input.notes[0].clone());
    let mut graph = Graph {
        layers: vec![],
        actions: vec![].into(),
        note_count: 40,
        edge_count: 0,
        temporal: vec![],
        touches: vec![],
    };
    for note in 0..40 {
        let mut edges = vec![];
        // Visit Great first so skill-state insertions need not be monotone.
        for perfect in [false, true] {
            let action = graph.actions.len();
            graph.actions.push(Action {
                factor: Factor {
                    perfect,
                    inside: true,
                    slot: 1,
                    fever: note >= 20,
                    reference: 0.2,
                    rank: note + 1,
                },
                hits: vec![(note, 30 + note as u32 + u32::from(!perfect))].into(),
            });
            edges.push(Edge {
                from: 0,
                to: 0,
                action: Some(action),
            });
        }
        graph.edge_count += edges.len();
        graph.layers.push(Layer { count: 1, edges: edges.into() });
    }
    for objective in [WindowObjective::Continuous, WindowObjective::Native] {
        for net in [false, true] {
            let (_, stats) = compare(&input, &graph, WindowOptions::default(), objective, net);
            assert!(stats.peak_value_states > 20);
        }
    }
}
