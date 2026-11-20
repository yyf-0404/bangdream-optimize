use super::*;
use crate::hit_timing::{JudgementCorrection, NoteWindow, ReplayNote, ScoreEffect};
use std::collections::HashSet;

fn skill(score: ScoreEffect) -> ReplaySkill {
    ReplaySkill {
        duration: 0.5,
        score,
        correction: None,
    }
}

fn fixture() -> ReplayInput {
    let mut notes = vec![];
    let mut hit_frames = vec![];
    for slot in 0..6 {
        let start = 30 + slot as u32 * 120;
        for offset in [0, 15, 20, 25, 60] {
            let frame = start + offset;
            notes.push(ReplayNote {
                time: f64::from(frame) / 120.,
                trigger: (offset == 0).then_some(slot),
                window: if offset == 60 {
                    NoteWindow::Standard
                } else {
                    NoteWindow::PinnedPerfect { frame }
                },
            });
            hit_frames.push(frame);
        }
    }
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 48,
        medley_combo: false,
        fever_frames: Some([45, 295]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
        notes,
        hit_frames,
        skills: vec![skill(ScoreEffect::Plain { percent: 100. }); 6],
    }
}

fn catalogue() -> Vec<ReplaySkill> {
    let plain = skill(ScoreEffect::Plain { percent: 100. });
    let mut result = vec![
        plain.clone(),
        ReplaySkill {
            duration: 0.55,
            ..plain.clone()
        },
        ReplaySkill {
            correction: Some(JudgementCorrection::Good),
            ..plain.clone()
        },
        skill(ScoreEffect::Plain { percent: 100.5 }),
        skill(ScoreEffect::PerfectOnly { percent: 100. }),
        skill(ScoreEffect::PerfectOrZero { percent: 140. }),
        skill(ScoreEffect::PerfectOrHalf { percent: 140. }),
        skill(ScoreEffect::Continuous {
            high: 140.,
            low: 60.,
        }),
        skill(ScoreEffect::Continuous {
            high: 140.,
            low: 70.,
        }),
    ];
    for (percent, step, cap) in [
        (100., 0.5, 105),
        (101., 0.5, 105),
        (100., 1., 105),
        (100., 0.5, 106),
        (30., 0.5, 30),
    ] {
        result.push(skill(ScoreEffect::Crescendo { percent, step, cap }));
    }
    // A repeated descriptor must share the machine, independently of identity.
    result.push(plain);
    result
}

fn pair(
    input: &ReplayInput,
    skills: &[ReplaySkill],
    options: WindowOptions,
    reference: f64,
) -> (FrontierNetWindows, FrontierNetWindows) {
    let references: Vec<_> = (0..input.notes.len())
        .map(|i| reference + i as f64 * 0.001)
        .collect();
    let table = |fast_net_queries| {
        FrontierNetWindows::new(
            input,
            skills,
            references.clone(),
            WindowOptions {
                fast_net_queries,
                ..options
            },
        )
        .unwrap()
    };
    (table(true), table(false))
}

fn compare(
    fast: &mut FrontierNetWindows,
    legacy: &mut FrontierNetWindows,
    slot: usize,
    skill: &ReplaySkill,
    request: u32,
) -> (f64, WindowStats, WindowStats) {
    let (actual, actual_stats) = fast.at_start(slot, skill, request).unwrap();
    let (expected, expected_stats) = legacy.at_start(slot, skill, request).unwrap();
    assert_eq!(
        actual.to_bits(),
        expected.to_bits(),
        "slot={slot}, request={request}, skill={skill:?}"
    );
    assert_eq!(
        (
            actual_stats.infeasible_requests,
            actual_stats.budget_requests,
            actual_stats.graph_builds,
            actual_stats.graph_hits,
            actual_stats.graph_edges,
            actual_stats.graph_peak_states,
            actual_stats.collapsed_notes,
            actual_stats.transitions,
            actual_stats.peak_value_states,
        ),
        (
            expected_stats.infeasible_requests,
            expected_stats.budget_requests,
            expected_stats.graph_builds,
            expected_stats.graph_hits,
            expected_stats.graph_edges,
            expected_stats.graph_peak_states,
            expected_stats.collapsed_notes,
            expected_stats.transitions,
            expected_stats.peak_value_states,
        ),
        "solver counters differ at slot={slot}, request={request}, skill={skill:?}"
    );
    (actual, actual_stats, expected_stats)
}

#[test]
fn cached_net_queries_match_legacy_across_slots_requests_and_full_skill_keys() {
    let input = fixture();
    let skills = catalogue();
    let (mut fast, mut legacy) = pair(&input, &skills, WindowOptions::default(), 0.1);
    let mut machines = HashSet::new();
    let mut stats = WindowStats::default();
    let mut legacy_stats = WindowStats::default();
    for pass in 0..2 {
        let order: Vec<_> = if pass == 0 {
            (0..skills.len()).collect()
        } else {
            (0..skills.len()).rev().collect()
        };
        for index in order {
            let skill = &skills[index];
            for slot in [0, 2, 5, 1, 4, 3] {
                for delay in [0, 1, 5] {
                    let first = machines.insert(skill.cache_key());
                    let (_, actual, expected) = compare(
                        &mut fast,
                        &mut legacy,
                        slot,
                        skill,
                        30 + slot as u32 * 120 + delay,
                    );
                    assert_eq!(actual.net_machine_builds, usize::from(first));
                    assert_eq!(actual.net_machine_hits, usize::from(!first));
                    assert_eq!(expected.net_machine_builds, 1);
                    assert_eq!(expected.net_machine_hits, 0);
                    stats.accumulate(&actual);
                    legacy_stats.accumulate(&expected);
                }
            }
        }
    }
    assert_eq!(fast.machines.len(), machines.len());
    assert_eq!(stats.net_machine_builds, machines.len());
    assert!(stats.graph_hits > 0);
    assert!(stats.prefix_tables > 0);
    assert!(stats.prefix_tables < legacy_stats.prefix_tables);
    assert_eq!(stats.budget_requests, 0);
}

#[test]
fn cached_net_queries_match_with_graph_cache_disabled_and_negative_gains() {
    let input = fixture();
    let skills = catalogue();
    let options = WindowOptions {
        max_cached_edges: 0,
        ..WindowOptions::default()
    };
    let (mut fast, mut legacy) = pair(&input, &skills, options, 20.);
    let mut saw_prefix_reuse = false;
    for skill in &skills {
        for pass in 0..2 {
            for slot in [0, 3, 5] {
                let (upper, stats, old_stats) =
                    compare(&mut fast, &mut legacy, slot, skill, 30 + slot as u32 * 120);
                assert_eq!(upper.to_bits(), 0_f64.to_bits());
                assert_eq!(stats.graph_hits, 0);
                assert_eq!(stats.graph_builds, 1);
                assert_eq!(stats.budget_requests, 0);
                saw_prefix_reuse |= pass > 0 && stats.prefix_tables < old_stats.prefix_tables;
            }
        }
    }
    assert!(fast.windows.graphs.is_empty());
    assert!(legacy.windows.graphs.is_empty());
    assert!(saw_prefix_reuse);
}

#[test]
fn cached_net_queries_preserve_graph_and_value_budget_fallbacks() {
    let input = fixture();
    let skills = [skill(ScoreEffect::Crescendo {
        percent: 100.,
        step: 0.5,
        cap: 150,
    })];
    for options in [
        WindowOptions {
            max_graph_edges: 1,
            ..WindowOptions::default()
        },
        WindowOptions {
            max_states: 1,
            ..WindowOptions::default()
        },
        WindowOptions {
            max_transitions: 1,
            ..WindowOptions::default()
        },
        WindowOptions {
            max_transitions: 1,
            max_cached_edges: 0,
            ..WindowOptions::default()
        },
    ] {
        let (mut fast, mut legacy) = pair(&input, &skills, options, 0.1);
        for pass in 0..2 {
            for slot in [0, 2, 5] {
                let (_, stats, _) = compare(
                    &mut fast,
                    &mut legacy,
                    slot,
                    &skills[0],
                    30 + slot as u32 * 120,
                );
                assert_eq!(stats.budget_requests, 1);
                if options.max_transitions == 1 {
                    assert_eq!(stats.transitions, 2);
                    if pass > 0 && options.max_cached_edges > 0 {
                        // Fallback must rebuild the envelope even on a graph hit.
                        assert_eq!(stats.graph_hits, 1);
                    }
                } else {
                    assert_eq!(stats.transitions, 0);
                }
            }
        }
        let (upper, stats, _) = compare(&mut fast, &mut legacy, 0, &skills[0], 29);
        assert_eq!(upper.to_bits(), 0_f64.to_bits());
        assert_eq!(stats.infeasible_requests, 1);
        assert_eq!(stats.budget_requests, 0);
    }
}

#[test]
fn cached_net_queries_keep_value_state_budget_on_reused_graphs() {
    let input = fixture();
    let skills = [skill(ScoreEffect::Crescendo {
        percent: 100.,
        step: 0.5,
        cap: 150,
    })];
    let (mut fast, mut legacy) = pair(&input, &skills, WindowOptions::default(), 0.1);
    let (_, warm, _) = compare(&mut fast, &mut legacy, 0, &skills[0], 30);
    assert_eq!(warm.budget_requests, 0);
    assert!(warm.peak_value_states > 1);
    // Retain a valid graph while lowering only the evaluation state allowance.
    // Both paths must return the same original envelope, never a partial value.
    fast.windows.options.max_states = 1;
    legacy.windows.options.max_states = 1;
    for _ in 0..2 {
        let (_, stats, _) = compare(&mut fast, &mut legacy, 0, &skills[0], 30);
        assert_eq!(stats.graph_hits, 1);
        assert_eq!(stats.budget_requests, 1);
        assert!(stats.peak_value_states > 1);
    }
}
