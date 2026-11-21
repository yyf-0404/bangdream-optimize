use super::*;

#[test]
fn each_skill_equality_pattern_preserves_first_seen_configuration_ids() {
    fn check(classes: &mut [usize; 5], next: usize, used: usize) -> usize {
        if next < classes.len() {
            return (0..=used).map(|class| {
                classes[next] = class;
                check(classes, next + 1, used.max(class + 1))
            }).sum();
        }
        let pattern = LayoutPattern::for_classes(*classes);
        let mut seen = BTreeMap::new();
        let mut refs = pattern.references.iter();
        for layout in &tables().layouts {
            for order in &layout.weighted_orders {
                let mut slots = [layout.cards[CAPTAIN_SLOT]; 6];
                slots[..5].copy_from_slice(&tables().orders[order.index]);
                let next_id = seen.len();
                let (id, first_slots) = *seen.entry(slots.map(|i| classes[i]))
                    .or_insert((next_id, slots));
                assert_eq!(refs.next().copied().map(usize::from), Some(id));
                assert_eq!(pattern.configurations[id], first_slots);
            }
        }
        assert_eq!(refs.next(), None);
        assert_eq!(pattern.configurations.len(), seen.len());
        1
    }
    assert_eq!(check(&mut [0; 5], 1, 1), 52);
}

#[test]
fn score_only_search_uses_reachable_peaks_and_full_search_recovers_lower_power_ties() {
    use crate::scoring::{Judgement, NoteScorer};

    // Five adjacent f32 powers straddle the native integer truncation at 100000.
    let layouts = TimingLayouts::build([0, 0, 1, 1, 2], |cards| {
        ScoringPower::new(f32::from_bits(100_000_f32.to_bits() - 2 + cards[0] as u32))
    }).unwrap();
    let actual: Vec<_> = layouts.requests().iter().map(|q| {
        let scorer = NoteScorer::new(q.power, 5, 3).unwrap();
        let skill = 1.0 + (q.configuration % 3) as f32;
        u64::from(scorer.score(Judgement::Perfect, 1., skill, 1.).unwrap().final_score)
    }).collect();
    let upper: Vec<_> = actual.iter().enumerate().map(|(i, &v)| v + (i % 5) as u64).collect();
    let mut visited = std::collections::BTreeSet::new();
    let best = layouts.maximum_monotone_score(&upper, |i, q| {
        assert!(visited.insert(q.configuration));
        assert!(layouts.configurations()[q.configuration].iter().all(|&j| layouts.requests()[j].power <= q.power));
        Ok(actual[i])
    }).unwrap();
    assert_eq!(best, *actual.iter().max().unwrap());
    assert_eq!(layouts.maximize(&upper, |i, _| Ok(actual[i])).unwrap().score, best);
    assert!(visited.len() < layouts.requests().len());

    // Every power ties. The first physical layout has lower power than the
    // representative, yet all 1024 native paths must still count for it.
    let tied = TimingLayouts::build([0; 5], |cards| ScoringPower::new(100. + cards[0] as f32)).unwrap();
    let mut calls = 0;
    let upper = vec![9; tied.requests().len()];
    let peak = tied.maximum_monotone_score(&upper, |_, q| {
        calls += 1;
        assert_eq!(q.power.value(), 104.);
        Ok(9)
    }).unwrap();
    assert_eq!(calls, 1);
    let winner = tied.maximize(&upper, |_, _| Ok(9)).unwrap();
    assert_eq!(winner.score, peak);
    assert_eq!(winner.recommended_layout, tables().layouts[0].cards);
    assert!(tied.requests()[winner.chosen_request].power.value() < 104.);
    assert_eq!(winner.max_score_path_count, 1024);
    assert_eq!(winner.optimal_order_count, 96);
}

#[test]
fn all_layouts_keep_native_orders_weights_and_float_power_bits() {
    for distinct in [false, true] {
        let power = |l: [usize; 5]| {
            ScoringPower::new(300_000. + if distinct { l[0] as f32 * 0.03125 } else { 0. })
        };
        let catalog = TimingLayouts::new(power).unwrap();
        assert_eq!(catalog.references.len(), 120 * 96);
        if !distinct {
            assert_eq!(catalog.requests.len(), 600);
        }
        let mut offset = 0;
        for layout in &tables().layouts {
            let ids = &catalog.references[offset..offset + layout.weighted_orders.len()];
            offset += ids.len();
            assert_eq!(ids.len(), 96);
            assert_eq!(
                layout.weighted_orders.iter().map(|r| r.count as u64).sum::<u64>(),
                1024
            );
            for (&id, order) in ids.iter().zip(&layout.weighted_orders) {
                let r = &catalog.requests[usize::from(id)];
                assert_eq!(r.slots[5], layout.cards[2]);
                assert_eq!(r.power, power(layout.cards).unwrap());
                assert!(order.count > 0);
                assert_ne!(r.slots[4], layout.cards[4]);
            }
        }
        // Oracle walks all 120 * 96 cases directly, without using catalog refs.
        for seed in 0..15u32 {
            let value = |slots: [usize; 6], p: ScoringPower| -> u64 {
                if seed == 0 {
                    return 0;
                }
                let mut h = p.cache_key().wrapping_mul(seed);
                for i in slots {
                    h = h.wrapping_mul(31).wrapping_add(i as u32 + 1);
                }
                (h % 100) as u64 // intentionally many score ties
            };
            let actual: Vec<_> = catalog
                .requests
                .iter()
                .map(|r| value(r.slots, r.power))
                .collect();
            let upper: Vec<_> = actual
                .iter()
                .enumerate()
                .map(|(i, &v)| v + (i % 8) as u64)
                .collect();
            let result = catalog.maximize(&upper, |i, _| Ok(actual[i])).unwrap();
            let mut oracle = (0, 0u16, [usize::MAX; 5]);
            for layout in &tables().layouts {
                let mut scores = Vec::new();
                for o in &layout.weighted_orders {
                    let mut slots = [layout.cards[2]; 6];
                    slots[..5].copy_from_slice(&tables().orders[o.index]);
                    scores.push((value(slots, power(layout.cards).unwrap()), o.count));
                }
                let max = scores.iter().map(|s| s.0).max().unwrap();
                let paths = scores.iter().filter(|s| s.0 == max).map(|s| s.1).sum();
                if (max, paths) > (oracle.0, oracle.1) {
                    oracle = (max, paths, layout.cards);
                }
            }
            assert_eq!(
                (
                    result.score,
                    result.max_score_path_count,
                    result.recommended_layout
                ),
                oracle
            );
            assert_eq!(actual[result.chosen_request], result.score);
        }
        assert!(catalog.maximize(&[], |_, _| Ok(0)).is_err());
        assert!(matches!(
            catalog.maximize(&vec![0; catalog.requests.len()], |_, _| Ok(1)),
            Err(LayoutError::Underestimated { .. })
        ));
    }
}

#[test]
fn compact_references_cover_every_distinct_power_and_preserve_tie_witnesses() {
    // Different exact f32 bits in all 120 physical layouts exercise the largest
    // request family, not the much smaller equal-skill/equal-power case.
    let power = |l: [usize; 5]| ScoringPower::new(
        300_000. + crate::skill_shuffle::order_index(l) as f32 * 0.03125,
    );
    let catalog = TimingLayouts::new(power).unwrap();
    assert_eq!(catalog.requests.len(), 120 * 96);
    assert_eq!(catalog.references.last().copied(), Some(11519));
    assert_eq!(std::mem::size_of_val(&*catalog.references), 23_040);
    let mut next = 0;
    for layout in &tables().layouts {
        for order in &layout.weighted_orders {
            let mut slots = [layout.cards[CAPTAIN_SLOT]; 6];
            slots[..5].copy_from_slice(&tables().orders[order.index]);
            assert_eq!(usize::from(catalog.references[next]), next);
            assert_eq!(catalog.requests[next].slots, slots);
            assert_eq!(catalog.requests[next].power, power(layout.cards).unwrap());
            next += 1;
        }
    }
    let winner = catalog.maximize(&vec![0; next], |_, _| Ok(0)).unwrap();
    let layout = &tables().layouts[0];
    let chosen = layout.weighted_orders.iter().enumerate()
        .max_by_key(|&(i, o)| (o.count, std::cmp::Reverse(i))).unwrap();
    let mut slots = [layout.cards[CAPTAIN_SLOT]; 6];
    slots[..5].copy_from_slice(&tables().orders[chosen.1.index]);
    assert_eq!(winner.recommended_layout, layout.cards);
    assert_eq!(winner.chosen_request, chosen.0);
    assert_eq!(winner.chosen_slots, slots);
    assert_eq!(winner.max_score_path_count, 1024);
    assert_eq!(winner.optimal_order_count, 96);
    assert_eq!(winner.scored_requests, next);
}

#[test]
fn complete_skill_equivalence_preserves_layout_probability_and_actual_card_witness() {
    use crate::hit_timing::{JudgementCorrection, ScoreEffect};
    let base = ReplaySkill {
        duration: 7.,
        score: ScoreEffect::Plain { percent: 100. },
        correction: None,
    };
    let mut skills: [ReplaySkill; 5] = std::array::from_fn(|_| base.clone());
    for mode in 0..5 {
        if mode == 1 {
            skills[4].correction = Some(JudgementCorrection::Bad);
        }
        if mode == 2 {
            skills[3].duration = 7.5;
        }
        if mode == 3 {
            skills[2].score = ScoreEffect::Continuous {
                high: 100.,
                low: 60.,
            };
        }
        if mode == 4 {
            skills[3].score = ScoreEffect::PerfectOnly { percent: 100. };
        }
        let p = |l: [usize; 5]| {
            ScoringPower::new(300000. + if mode == 0 { 0. } else { l[0] as f32 * 0.03125 })
        };
        let shared = TimingLayouts::for_skills(&skills, p).unwrap();
        let separate = TimingLayouts::new(p).unwrap();
        assert!(shared.requests.len() < separate.requests.len());
        if mode == 0 {
            assert_eq!(shared.requests.len(), 1);
        }
        let score = |r: &LayoutScoreRequest| {
            let mut h = r.power.cache_key();
            for i in r.slots {
                for b in skills[i].cache_key() {
                    h = h.wrapping_mul(31).wrapping_add(b);
                }
            }
            (h % 100) as u64
        };
        let run = |c: &TimingLayouts| {
            let upper: Vec<_> = c.requests.iter().map(|r| score(r) + 5).collect();
            c.maximize(&upper, |_, r| Ok(score(r))).unwrap()
        };
        let a = run(&shared);
        let b = run(&separate);
        assert_eq!(
            (
                a.score,
                a.recommended_layout,
                a.max_score_path_count,
                a.optimal_order_count
            ),
            (
                b.score,
                b.recommended_layout,
                b.max_score_path_count,
                b.optimal_order_count
            )
        );
        assert_eq!(a.chosen_slots[5], a.recommended_layout[2]);
        assert_ne!(a.chosen_slots[4], a.recommended_layout[4]);
        assert_eq!(
            score(&LayoutScoreRequest {
                slots: a.chosen_slots,
                configuration: 0,
                power: p(a.recommended_layout).unwrap()
            }),
            a.score
        );
        if mode == 0 {
            assert_eq!(a.max_score_path_count, 1024);
        }
        let mut seen = std::collections::BTreeSet::new();
        for (id, group) in shared.configurations().iter().enumerate() {
            let first = &shared.requests[group[0]];
            for &i in group {
                assert!(seen.insert(i));
                let r = &shared.requests[i];
                assert_eq!(r.configuration, id);
                assert_eq!(
                    r.slots.map(|i| skills[i].cache_key()),
                    first.slots.map(|i| skills[i].cache_key())
                );
            }
        }
        assert_eq!(seen.len(), shared.requests.len());
    }
    skills[0].duration = f32::NAN;
    assert!(TimingLayouts::for_skills(&skills, |_| ScoringPower::new(1.)).is_err());
}
