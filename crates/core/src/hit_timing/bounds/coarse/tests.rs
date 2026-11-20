use super::*;
use crate::hit_timing::{lazy_orders, JudgementCorrection, ReplayNote};

fn chart(spacing: f64) -> ReplayInput {
    let notes: Vec<_> = (0..42)
        .map(|i| {
            let time = 0.2 + i as f64 * spacing;
            ReplayNote {
                time,
                trigger: (i % 7 == 0).then_some(i / 7),
                window: if i % 7 == 3 {
                    NoteWindow::WidenedEndpoint
                } else if i % 7 == 5 {
                    NoteWindow::PinnedPerfect {
                        frame: (time * 120.).round() as u32,
                    }
                } else {
                    NoteWindow::Standard
                },
            }
        })
        .collect();
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        hit_frames: notes
            .iter()
            .map(|n| (n.time * 120.).round() as u32)
            .collect(),
        notes,
        skills: vec![
            ReplaySkill {
                duration: 0.1,
                score: ScoreEffect::Plain { percent: 100. },
                correction: None
            };
            6
        ],
        initial_combo: 699,
        medley_combo: true,
        fever_frames: Some([60, 180]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    }
}

fn members() -> [ReplaySkill; 5] {
    [
        ScoreEffect::Plain { percent: 110. },
        ScoreEffect::PerfectOnly { percent: 150. },
        ScoreEffect::Continuous {
            high: 150.,
            low: 65.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 150,
        },
        ScoreEffect::PerfectOrHalf { percent: 125. },
    ]
    .map(|score| ReplaySkill {
        duration: 0.5,
        score,
        correction: None,
    })
}

#[test]
fn coarse_encloses_every_fine_order_through_queue_fever_and_correction() {
    for spacing in [0.055, 0.23] {
        let mut input = chart(spacing);
        let mut cache =
            CoarseTimingUpperCache::new(&input, ScoringPower::new(1_000_000.).unwrap()).unwrap();
        let mut fine = TimingUpperCache::new(&input);
        for correction in [
            None,
            Some(JudgementCorrection::Great),
            Some(JudgementCorrection::Good),
            Some(JudgementCorrection::Bad),
        ] {
            let mut members = members();
            members[1].correction = correction;
            for (i, s) in members.iter_mut().enumerate() {
                s.duration = 0.1 + i as f32 * 0.3;
            }
            let bound = cache.prepare_team(&members).unwrap();
            for order in lazy_orders::reachable_orders() {
                input.skills = order.slots.map(|i| members[i].clone()).to_vec();
                let fine = fine.prepare(&input.skills).unwrap();
                for value in [0., 0.125, 300040.75, 700000.5] {
                    let p = ScoringPower::new(value).unwrap();
                    assert!(
                        fine.score(p, &mut TimingUpperScratch::default()).unwrap()
                            <= bound.upper(p).unwrap(),
                        "spacing {spacing}, {correction:?}, {:?}, {value}",
                        order.slots
                    );
                }
            }
            assert!(bound.upper(ScoringPower::new(1_000_001.).unwrap()).is_err());
        }
        assert_eq!(cache.stats.geometries, 3);
        assert!(cache.stats.profile_hits > 0);
    }
}

#[test]
fn assignment_matches_all_and_only_native_orders_and_reuses_profiles() {
    let input = chart(0.15);
    let mut members = members();
    members[3].score = ScoreEffect::Crescendo {
        percent: 130.,
        step: 0.5,
        cap: 130,
    };
    members[4].score = ScoreEffect::PerfectOrZero { percent: 140. };
    let mut cache =
        CoarseTimingUpperCache::new(&input, ScoringPower::new(1_000_000.).unwrap()).unwrap();
    let bound = cache.prepare_team(&members).unwrap();
    let built = cache.stats.profile_builds;
    let second = cache.prepare_team(&members).unwrap();
    assert_eq!(built, cache.stats.profile_builds);
    for value in [0., 0.125, 300040.75, 700000.5] {
        let p = ScoringPower::new(value).unwrap();
        let full = lazy_orders::reachable_orders()
            .iter()
            .map(|order| {
                bound.baseline.bounds(p).unwrap().max
                    + bound.encore.upper(p).unwrap()
                    + (0..5)
                        .map(|slot| bound.first_five[slot][order.slots[slot]].upper(p).unwrap())
                        .sum::<u64>()
            })
            .max()
            .unwrap();
        assert_eq!(full, bound.upper(p).unwrap());
        assert_eq!(full, second.upper(p).unwrap());
    }
    members[0].duration = f32::NAN;
    assert!(cache.prepare_team(&members).is_err());
    let mut missing = input.clone();
    missing.notes[0].trigger = None;
    assert!(CoarseTimingUpperCache::new(&missing, ScoringPower::new(1.).unwrap()).is_err());
}

#[test]
fn roster_covers_every_physical_layout_and_captain() {
    for spacing in [0.055, 0.23] {
        let input = chart(spacing);
        let mut skills = members();
        skills[3].correction = Some(JudgementCorrection::Bad);
        for (i, s) in skills.iter_mut().enumerate() {
            s.duration = 0.1 + i as f32 * 0.3;
        }
        let power = ScoringPower::new(300040.75).unwrap();
        let mut cache = CoarseTimingUpperCache::new(&input, power).unwrap();
        let family = cache.prepare_roster(&skills).unwrap().upper(power).unwrap();
        for layout in &crate::skill_shuffle::tables().layouts {
            let members = layout.cards.map(|i| skills[i].clone());
            assert!(cache.prepare_team(&members).unwrap().upper(power).unwrap() <= family);
        }
        let mut fine = TimingUpperCache::new(&input);
        for captain in 0..5 {
            for order in &crate::skill_shuffle::tables().orders {
                let skills: Vec<_> = order
                    .iter()
                    .copied()
                    .chain([captain])
                    .map(|i| skills[i].clone())
                    .collect();
                assert!(
                    fine.prepare(&skills)
                        .unwrap()
                        .score(power, &mut TimingUpperScratch::default())
                        .unwrap()
                        <= family
                );
            }
        }
    }
}
