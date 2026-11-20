use super::*;

fn note(baseline: f64, events: &[(u32, usize, f64)], rank: usize) -> StateNote {
    StateNote {
        baseline,
        outside: true,
        events: events
            .iter()
            .map(|&(time, grade, weight)| Event {
                time,
                rank,
                grade,
                weight,
            })
            .collect(),
    }
}

fn brute(notes: &[StateNote], skill: &ReplaySkill) -> f64 {
    fn visit(
        notes: &[StateNote],
        skill: &ReplaySkill,
        i: usize,
        picked: &mut Vec<(Event, usize)>,
        best: &mut f64,
    ) {
        if i == notes.len() {
            let mut chosen = picked.clone();
            chosen.sort_unstable_by_key(|(e, _)| (e.time, e.rank));
            fn orders(
                i: usize,
                chosen: &mut [(Event, usize)],
                notes: &[StateNote],
                skill: &ReplaySkill,
                best: &mut f64,
            ) {
                if i < chosen.len() {
                    let frame = chosen[i].0.time;
                    for j in i..chosen.len() {
                        if chosen[j].0.time != frame {
                            break;
                        }
                        chosen.swap(i, j);
                        orders(i + 1, chosen, notes, skill, best);
                        chosen.swap(i, j);
                    }
                    return;
                }
                let mut state = SkillState::new(skill).unwrap();
                let score = chosen
                    .iter()
                    .map(|(event, index)| {
                        let grade = if event.grade == 0 {
                            Judgement::Great
                        } else {
                            Judgement::Perfect
                        };
                        event.weight * f64::from(state.multiplier(grade)) - notes[*index].baseline
                    })
                    .sum();
                *best = best.max(score);
            }
            orders(0, &mut chosen, notes, skill, best);
            return;
        }
        if notes[i].outside {
            visit(notes, skill, i + 1, picked, best);
        }
        for &event in &notes[i].events {
            picked.push((event, i));
            visit(notes, skill, i + 1, picked, best);
            picked.pop();
        }
    }
    let mut best = f64::NEG_INFINITY;
    visit(notes, skill, 0, &mut vec![], &mut best);
    best
}

fn check(notes: &[StateNote], score: ScoreEffect) -> f64 {
    let skill = ReplaySkill {
        duration: 1.,
        correction: None,
        score,
    };
    let machine = Machine::new(&skill, notes.len()).unwrap().unwrap();
    let mut stats = TimingNetStats::default();
    let value = maximum(notes, &machine, &mut stats).unwrap();
    let expected = brute(notes, &skill);
    assert!(value >= expected - 1e-12, "{value} < {expected}: {skill:?}");
    assert!(
        (value - expected).abs() < 1e-9,
        "{value} != {expected}: {skill:?}"
    );
    assert_eq!(stats.state_solved_rows, 1);
    assert_eq!(stats.state_budget_rows, 0);
    value
}

#[test]
fn state_maxima_match_exhaustive_choices_and_actual_event_order() {
    let mut random = 0xabcdef01_u32;
    for sample in 0..96 {
        let notes: Vec<_> = (0..5)
            .map(|i| {
                let events: Vec<_> = (0..3)
                    .map(|j| {
                        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                        (
                            10 + ((random >> 8) % 7),
                            (random as usize >> 24) % 2,
                            0.5 + ((random >> 16) % 30) as f64 * 0.1 + j as f64 * 0.001,
                        )
                    })
                    .collect();
                // All (time, rank) pairs of a note are unique, like frame domains.
                let mut unique = std::collections::BTreeMap::new();
                for event in events {
                    unique.insert(event.0, event);
                }
                let mut value = note(
                    1. + i as f64 * 0.2,
                    &unique.into_values().collect::<Vec<_>>(),
                    4 - i,
                );
                value.outside = (sample + i) % 3 != 0;
                value
            })
            .collect();
        check(
            &notes,
            ScoreEffect::Continuous {
                high: 140.,
                low: 40.,
            },
        );
        check(
            &notes,
            ScoreEffect::Crescendo {
                percent: 100.,
                step: 10.,
                cap: 140,
            },
        );
        if sample % 8 == 0 {
            check(
                &notes,
                ScoreEffect::Crescendo {
                    percent: 35.,
                    step: 0.5,
                    cap: 35,
                },
            );
        }
    }
}

#[test]
fn great_cannot_leave_continuous_clean_or_increment_crescendo() {
    let notes = vec![note(1., &[(1, 0, 1.)], 0), note(1., &[(2, 1, 2.)], 1)];
    let continuous = check(
        &notes,
        ScoreEffect::Continuous {
            high: 150.,
            low: 50.,
        },
    );
    // The relaxed optimum skips the early Great; charging it while keeping
    // high on the following Perfect would give the strictly larger 4.5.
    assert!((continuous - 4.).abs() < 1e-6);
    let crescendo = check(
        &notes,
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 10.,
            cap: 150,
        },
    );
    assert!((crescendo - 4.2).abs() < 1e-6);
}

#[test]
fn unavoidable_great_cannot_be_skipped_to_keep_a_clean_skill() {
    let mut notes = vec![note(1., &[(1, 0, 1.)], 0), note(1., &[(2, 1, 2.)], 1)];
    for n in &mut notes {
        n.outside = false;
    }
    let value = check(
        &notes,
        ScoreEffect::Continuous {
            high: 150.,
            low: 50.,
        },
    );
    assert!((value - 2.5).abs() < 1e-6);
    // Allowing the first note to occur outside is a different domain, worth 4.
    notes[0].outside = true;
    let outside = check(
        &notes,
        ScoreEffect::Continuous {
            high: 150.,
            low: 50.,
        },
    );
    assert!((outside - 4.).abs() < 1e-6);
}

#[test]
fn mandatory_notes_keep_negative_cost_in_the_signed_local_maximum() {
    let mut notes = vec![note(3., &[(1, 1, 1.)], 0)];
    notes[0].outside = false;
    let value = check(&notes, ScoreEffect::Continuous { high: 50., low: 0. });
    assert!((value + 1.5).abs() < 1e-12);
}

#[test]
fn mandatory_note_without_an_accepted_event_is_infeasible_not_a_free_skip() {
    let mut notes = vec![note(1., &[], 0)];
    notes[0].outside = false;
    let skill = ReplaySkill {
        duration: 1.,
        correction: None,
        score: ScoreEffect::Continuous { high: 50., low: 0. },
    };
    let machine = Machine::new(&skill, 1).unwrap().unwrap();
    let mut stats = TimingNetStats::default();
    assert_eq!(
        maximum(&notes, &machine, &mut stats),
        Some(f64::NEG_INFINITY)
    );
    assert_eq!(stats.state_infeasible_rows, 1);
    assert_eq!(stats.state_budget_rows, 0);
}

#[test]
fn same_frame_maximum_is_independent_of_baseline_rank() {
    let mut notes = vec![note(0., &[(5, 0, 10.)], 0), note(0., &[(5, 1, 10.)], 1)];
    let a = check(
        &notes,
        ScoreEffect::Continuous {
            high: 150.,
            low: 50.,
        },
    );
    notes[0].events[0].rank = 1;
    notes[1].events[0].rank = 0;
    let b = check(
        &notes,
        ScoreEffect::Continuous {
            high: 150.,
            low: 50.,
        },
    );
    assert!((a - 40.).abs() < 1e-10);
    assert!((b - 40.).abs() < 1e-10);
}

#[test]
fn frontier_budget_discards_partial_maximum() {
    let notes: Vec<_> = (0..13)
        .map(|i| note(0., &[(1, 1, 1.), (2, 1, 1.)], i))
        .collect();
    let skill = ReplaySkill {
        duration: 1.,
        correction: None,
        score: ScoreEffect::Continuous {
            high: 150.,
            low: 50.,
        },
    };
    let mut stats = TimingNetStats::default();
    assert!(maximum(
        &notes,
        &Machine::new(&skill, notes.len()).unwrap().unwrap(),
        &mut stats
    )
    .is_none());
    assert_eq!(stats.state_budget_rows, 1);
    assert_eq!(stats.state_solved_rows, 0);
}

#[test]
fn repeated_frame_cells_preserve_bits_and_logical_budget_at_every_cutoff() {
    for sample in 0..48 {
        let notes: Vec<_> = (0..4).map(|i| {
            let events: Vec<_> = (1..=9).map(|time| (time,
                (sample + i + time as usize / 4) % 2,
                if time < 7 { 0.1 + i as f64 * 0.01 } else { 0.101 + i as f64 * 0.01 })).collect();
            let mut n = note(if i % 2 == 0 { 0.3 } else { 0.01 }, &events, 3 - i);
            n.outside = (sample + i) % 3 != 0;
            n
        }).collect();
        for score in [ScoreEffect::Continuous { high: 130., low: 30. },
            ScoreEffect::Crescendo { percent: 25., step: 0.5, cap: 25 },
            ScoreEffect::Crescendo { percent: 25., step: 2.5, cap: 120 }] {
            let machine = Machine::new(&ReplaySkill { duration: 1., correction: None, score }, notes.len())
                .unwrap().unwrap();
            for limit in [0, 1, 17, 48, 64, 100, 1000, MAX_TRANSITIONS] {
                let mut a = TimingNetStats::default();
                let mut b = TimingNetStats::default();
                let old = maximum_controlled(&notes, &machine, &mut a, false, limit);
                let new = maximum_controlled(&notes, &machine, &mut b, true, limit);
                assert_eq!(new.map(f64::to_bits), old.map(f64::to_bits), "sample={sample} limit={limit}");
                assert_eq!(b.state_transitions, a.state_transitions);
                assert_eq!(b.state_budget_rows, a.state_budget_rows);
                assert_eq!(b.state_solved_rows, a.state_solved_rows);
                assert_eq!(b.state_infeasible_rows, a.state_infeasible_rows);
                assert!(b.state_evaluated_transitions <= a.state_evaluated_transitions);
            }
        }
    }
}

#[test]
fn repeated_frame_cells_do_not_cross_event_changes_gaps_or_unfinite_frontiers() {
    let mut notes = vec![note(0.01, &[(1, 1, 0.137), (2, 1, 0.137),
        (3, 1, 0.137_f64.next_up()), (4, 0, 0.137), (6, 0, 0.137)], 0)];
    let machine = Machine::new(&ReplaySkill { duration: 1., correction: None,
        score: ScoreEffect::Continuous { high: 120., low: 60. } }, 1).unwrap().unwrap();
    for nonfinite in [false, true] {
        if nonfinite { notes[0].baseline = f64::INFINITY; }
        let mut a = TimingNetStats::default();
        let mut b = TimingNetStats::default();
        let old = maximum_controlled(&notes, &machine, &mut a, false, MAX_TRANSITIONS);
        let new = maximum_controlled(&notes, &machine, &mut b, true, MAX_TRANSITIONS);
        assert_eq!(new.map(f64::to_bits), old.map(f64::to_bits));
        assert_eq!(b.state_transitions, a.state_transitions);
    }
}

#[test]
fn skipping_request_rows_matches_exhaustive_state_maxima() {
    let skill = ReplaySkill {
        duration: 1.,
        correction: None,
        score: ScoreEffect::Continuous {
            high: 150.,
            low: 50.,
        },
    };
    let rows: Vec<_> = (0..48)
        .map(|i| {
            vec![
                note(1., &[(1, 0, 1.), (2, 1, 2.)], 0),
                note(1., &[(1, 1, 0.5 + (i % 9) as f64 * 0.2), (3, 0, 1.)], 1),
            ]
        })
        .collect();
    let machine = Machine::new(&skill, 2).unwrap().unwrap();
    let actual: Vec<_> = rows
        .iter()
        .map(|r| maximum(r, &machine, &mut TimingNetStats::default()).unwrap())
        .collect();
    let envelopes: Vec<_> = actual
        .iter()
        .enumerate()
        .map(|(i, &v)| (v + (i % 3) as f64 * 0.3).next_up())
        .collect();
    let mut stats = TimingNetStats::default();
    let mut built = 0;
    let best = maximum_profile(&envelopes, &machine, &mut stats, |i| {
        built += 1;
        rows[i].clone()
    });
    assert_eq!(built, stats.state_rows);
    assert_eq!(best, actual.into_iter().fold(0., f64::max));
    assert!(stats.state_skipped_rows > 0);
    assert_eq!(
        stats.state_rows + stats.state_skipped_rows,
        rows.len() as u64
    );
}

fn native_sequence(score: ScoreEffect, grades: &[Judgement], initial_combo: u32) -> u64 {
    let skill = ReplaySkill {
        duration: 7.,
        correction: None,
        score,
    };
    let mut state = SkillState::new(&skill).unwrap();
    let scorer = crate::scoring::NoteScorer::new(ScoringPower::new(1000.).unwrap(), 5, 3).unwrap();
    grades
        .iter()
        .enumerate()
        .map(|(i, &grade)| {
            scorer
                .score(
                    grade,
                    crate::scoring::normal_combo_multiplier(initial_combo + i as u32 + 1),
                    state.multiplier(grade),
                    1.,
                )
                .unwrap()
                .final_score as u64
        })
        .sum()
}

#[test]
fn judgement_dominance_is_valid_inside_a_constant_scoring_region() {
    use Judgement::{Great as G, Perfect as P};
    for effect in [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 100. },
        ScoreEffect::PerfectOrZero { percent: 100. },
        ScoreEffect::PerfectOrHalf { percent: 100. },
        ScoreEffect::Continuous {
            high: 140.,
            low: 60.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 150,
        },
    ] {
        // The action's position and all other grades remain fixed. P both
        // scores at least as much and preserves/improves future skill state.
        assert!(
            native_sequence(effect, &[P, P, G, P], 0) >= native_sequence(effect, &[G, P, G, P], 0)
        );
        // This only proves the arithmetic exchange when both schedules are
        // feasible and all hits keep their skill/Fever/Combo region.
        assert!(native_sequence(effect, &[P, G, P], 0) >= native_sequence(effect, &[G, P, P], 0));
    }
}

#[test]
fn moving_great_after_perfect_can_lose_at_a_combo_boundary_even_for_crescendo() {
    use Judgement::{Great as G, Perfect as P};
    let effect = ScoreEffect::Crescendo {
        percent: 100.,
        step: 0.5,
        cap: 150,
    };
    // Notes 20 and 21 use native Combo rates 1.0 and 1.01. These are genuine
    // two-stage integer scores, not just real-valued multiplier products.
    let pg = native_sequence(effect, &[P, G], 19);
    let gp = native_sequence(effect, &[G, P], 19);
    assert_eq!((pg, gp), (3825, 3827));
    assert!(gp > pg);
}

#[test]
fn perfect_preference_cannot_cross_skill_or_fever_boundaries_unconditionally() {
    let scorer = crate::scoring::NoteScorer::new(ScoringPower::new(1000.).unwrap(), 5, 3).unwrap();
    let perfect_outside = scorer
        .score(Judgement::Perfect, 1., 1., 1.)
        .unwrap()
        .final_score;
    let great_in_skill = scorer
        .score(Judgement::Great, 1., 2., 1.)
        .unwrap()
        .final_score;
    let great_in_fever = scorer
        .score(Judgement::Great, 1., 1., 2.)
        .unwrap()
        .final_score;
    assert_eq!(
        (perfect_outside, great_in_skill, great_in_fever),
        (1100, 1600, 1600)
    );
}
