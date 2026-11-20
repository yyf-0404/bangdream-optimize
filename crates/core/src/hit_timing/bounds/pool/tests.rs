use super::*;
use crate::hit_timing::{layouts::physical_layouts, JudgementCorrection, ReplayNote};

#[test]
fn catalogue_and_merged_prefix_enclose_every_roster_and_order() {
    for spacing in [0.035, 0.27] {
        let notes: Vec<_> = (0..42)
            .map(|i| ReplayNote {
                time: 0.3 + i as f64 * spacing,
                trigger: (i % 7 == 0).then_some(i / 7),
                window: NoteWindow::Standard,
            })
            .collect();
        let skills: Vec<_> = (0..7)
            .map(|i| ReplaySkill {
                duration: 0.1 + 0.14 * i as f32,
                score: match i % 4 {
                    0 => ScoreEffect::Plain { percent: 110. },
                    1 => ScoreEffect::PerfectOnly { percent: 150. },
                    2 => ScoreEffect::Crescendo {
                        percent: 100.,
                        step: 0.5,
                        cap: 150,
                    },
                    _ => ScoreEffect::Continuous {
                        high: 140.,
                        low: 60.,
                    },
                },
                correction: if i == 6 {
                    Some(JudgementCorrection::Bad)
                } else {
                    None
                },
            })
            .collect();
        let input = ReplayInput {
            power: 300040.75,
            play_level: 27,
            fps: 120,
            hit_frames: notes
                .iter()
                .map(|n| (n.time * 120.).round() as u32)
                .collect(),
            notes,
            skills: vec![skills[0].clone(); 6],
            initial_combo: 699,
            medley_combo: true,
            fever_frames: Some([80, 140]),
            constraints: vec![],
            hit_order: vec![],
            native_time_routing: None,
        };
        let cap = ScoringPower::new(1_000_000.).unwrap();
        let pool = PoolTimingUpper::new(&input, &skills, cap).unwrap();
        let mut coarse = CoarseTimingUpperCache::new(&input, cap).unwrap();
        let power = ScoringPower::new(input.power).unwrap();
        let choices: Vec<_> = (0..7)
            .map(|i| pool.choice(i, f64::from(power) / 5.).unwrap())
            .collect();
        for omitted in 0..7 {
            let ids: Vec<_> = (0..7)
                .filter(|&i| i != omitted && i != (omitted + 1) % 7)
                .collect();
            let members = std::array::from_fn(|i| skills[ids[i]].clone());
            let exact_choices = std::array::from_fn(|i| choices[ids[i]]);
            let upper = pool.upper(&exact_choices).unwrap();
            assert_eq!(pool.separable_upper(&exact_choices).unwrap(), upper);
            let linear = pool.linearize(f64::from(power) * 1.01).unwrap();
            let weights = exact_choices.map(|c| linear.weights(c));
            let linear_upper = physical_layouts()
                .iter()
                .flat_map(|order| {
                    (0..5).map(move |captain| {
                        (0..5).fold(0_f64, |v, s| {
                            (v + weights[order[s]][s][usize::from(s == captain)]).next_up()
                        })
                    })
                })
                .fold(0., f64::max)
                + linear.error();
            assert!(linear_upper >= upper as f64);
            assert!(
                upper
                    >= coarse
                        .prepare_roster(&members)
                        .unwrap()
                        .upper(power)
                        .unwrap()
            );
            let union = choices
                .iter()
                .copied()
                .reduce(PoolTimingChoice::union)
                .unwrap();
            for order in physical_layouts() {
                let mut prefix = order.map(|i| exact_choices[i]);
                assert_eq!(pool.upper(&prefix).unwrap(), upper);
                for i in 0..5 {
                    prefix[i] = union;
                    let prefix_upper = pool.upper(&prefix).unwrap();
                    assert!(prefix_upper >= upper);
                    assert_eq!(pool.separable_upper(&prefix).unwrap(), prefix_upper);
                }
            }
        }
        assert!(pool.choice(7, 1.).is_err());
        assert!(pool.choice(0, f64::NAN).is_err());
        assert!(pool.upper(&[pool.choice(0, 300000.).unwrap(); 5]).is_err());
        assert!(pool
            .separable_upper(&[pool.choice(0, 300000.).unwrap(); 5])
            .is_err());
    }
}

#[test]
fn separable_prefilter_preserves_values_without_querying_adjacent_windows() {
    let mut notes: Vec<_> = [30, 40, 50, 300, 600, 900]
        .into_iter()
        .enumerate()
        .map(|(slot, frame)| ReplayNote {
            time: f64::from(frame) / 120.,
            trigger: Some(slot),
            window: NoteWindow::PinnedPerfect { frame },
        })
        .collect();
    notes.extend([0.36, 0.48].map(|time| ReplayNote {
        time,
        trigger: None,
        window: NoteWindow::Standard,
    }));
    let skills = [
        ReplaySkill {
            duration: 0.15,
            score: ScoreEffect::Plain { percent: 100. },
            correction: None,
        },
        ReplaySkill {
            duration: 0.5,
            score: ScoreEffect::Continuous {
                high: 140.,
                low: 60.,
            },
            correction: Some(JudgementCorrection::Bad),
        },
    ];
    let input = ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        hit_frames: notes
            .iter()
            .map(|n| (n.time * 120.).round() as u32)
            .collect(),
        notes,
        skills: vec![skills[0].clone(); 6],
        initial_combo: 19,
        medley_combo: false,
        fever_frames: Some([40, 63]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    };
    let cap = ScoringPower::new(600000.).unwrap();
    let pool = PoolTimingUpper::new_adjacent_net(&input, &skills, cap, Default::default()).unwrap();
    let old = PoolTimingUpper::new_stateful_net(&input, &skills, cap).unwrap();
    for split in [0, 2, 5] {
        let choices = std::array::from_fn(|i| {
            pool.choice(usize::from(i < split), f64::from(input.power) / 5.)
                .unwrap()
        });
        let old_choices = std::array::from_fn(|i| {
            old.choice(usize::from(i < split), f64::from(input.power) / 5.)
                .unwrap()
        });
        let before = pool.adjacent_stats().unwrap();
        assert!(pool.has_adjacent_roster(&choices));
        assert!(!old.has_adjacent_roster(&old_choices));
        let cheap = pool.separable_upper(&choices).unwrap();
        let after = pool.adjacent_stats().unwrap();
        assert_eq!(after.sequence_queries, before.sequence_queries);
        assert_eq!(after.roster_queries, before.roster_queries);
        assert_eq!(after.row_builds, before.row_builds);
        assert_eq!(after.row_hits, before.row_hits);
        assert_eq!(after.schedule_transitions, before.schedule_transitions);
        assert_eq!(cheap, old.upper(&old_choices).unwrap());
        assert_eq!(cheap, old.separable_upper(&old_choices).unwrap());
        assert!(cheap >= pool.upper(&choices).unwrap());
    }
    // Both lower and higher caps must match a fresh model exactly. The chart
    // contains overlapping trigger windows and correction, exercising queues.
    for limit in [400000., 700000.25] {
        let cap = ScoringPower::new(limit).unwrap();
        let shared = pool.with_power_cap(cap).unwrap();
        let fresh =
            PoolTimingUpper::new_adjacent_net(&input, &skills, cap, Default::default()).unwrap();
        assert_eq!(shared.error, fresh.error);
        assert_eq!(shared.baseline, fresh.baseline);
        assert_eq!(shared.gains, fresh.gains);
        for split in [0, 2, 5] {
            for member_power in [41000.125, 68000.25] {
                let choices = std::array::from_fn(|i| {
                    shared.choice(usize::from(i < split), member_power).unwrap()
                });
                assert_eq!(
                    shared.upper(&choices).unwrap(),
                    fresh.upper(&choices).unwrap()
                );
                assert_eq!(
                    shared.separable_upper(&choices).unwrap(),
                    fresh.separable_upper(&choices).unwrap()
                );
            }
        }
        assert!(shared
            .upper(&[shared.choice(0, limit as f64).unwrap(); 5])
            .is_err());
    }
    assert!(PoolTimingUpper::new(&input, &skills, cap)
        .unwrap()
        .with_power_cap(cap)
        .is_err());
    let a = pool.choice(0, 50000.).unwrap();
    let b = pool.choice(1, 60000.).unwrap();
    let same_skill_union = a.union(pool.choice(0, 60000.).unwrap());
    assert!(pool.has_adjacent_roster(&[same_skill_union; 5]));
    let mut merged = [a; 5];
    merged[2] = a.union(b);
    assert!(!pool.has_adjacent_roster(&merged));
    let minimum = pool
        .choice(0, 50000.)
        .unwrap()
        .minimum_components(pool.choice(1, 60000.).unwrap());
    let before = pool.adjacent_stats().unwrap();
    assert!(!pool.has_adjacent_roster(&[minimum; 5]));
    assert!(!pool.has_adjacent_roster(&[minimum.union(a); 5]));
    assert!(pool.separable_upper(&[minimum; 5]).unwrap() > 0);
    assert_eq!(pool.upper(&[minimum; 5]).unwrap(), 0);
    let after = pool.adjacent_stats().unwrap();
    assert_eq!(after.roster_queries, before.roster_queries);
    assert_eq!(after.sequence_queries, before.sequence_queries);
    let old_minimum = old
        .choice(0, 50000.)
        .unwrap()
        .minimum_components(old.choice(1, 60000.).unwrap());
    assert_eq!(
        old.upper(&[old_minimum; 5]).unwrap(),
        old.separable_upper(&[old_minimum; 5]).unwrap()
    );
}
