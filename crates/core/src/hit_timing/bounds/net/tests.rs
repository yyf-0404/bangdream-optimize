use super::*;
use crate::hit_timing::{JudgementCorrection, ReplayNote};

#[test]
fn interval_index_preserves_every_native_bound_and_coefficient_bit() {
    let mut input = fixture();
    // Chart order differs from time order, and the early windows overlap.
    input.notes[1].time = input.notes[2].time + 0.02;
    input.hit_frames = input.notes.iter().map(|n| (n.time*120.).round() as u32).collect();
    for correction in [None,Some(JudgementCorrection::Great),Some(JudgementCorrection::Bad)] {
        let catalogue: Vec<_> = (0..6).map(|i| ReplaySkill {
            duration: [0.1,0.75,2.,7.5][i%4], correction,
            score: match i%3 {
                0 => ScoreEffect::Continuous {high:140.,low:60.},
                1 => ScoreEffect::Crescendo {percent:100.,step:0.5,cap:150},
                _ => ScoreEffect::PerfectOnly {percent:150.},
            },
        }).collect();
        for delayed in [false,true] { for stateful in [false,true] {
            let p = ScoringPower::new(600000.).unwrap();
            let off = TimingNetUpper::build_indexed(&input,&catalogue,p,delayed,stateful,false).unwrap();
            let on = TimingNetUpper::build_indexed(&input,&catalogue,p,delayed,stateful,true).unwrap();
            let exhaustive = TimingNetUpper::build_runs(&input,&catalogue,p,delayed,stateful,true,false).unwrap();
            assert_eq!(exhaustive.baseline.to_bits(),on.baseline.to_bits());
            assert_eq!(exhaustive.error.to_bits(),on.error.to_bits());
            assert_eq!(exhaustive.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>(),
                on.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>());
            if on.stats.stateless_profiles==0 {
                assert_eq!(exhaustive.stats.note_options,on.stats.note_options);
            }
            assert_eq!(exhaustive.stats.state_transitions,on.stats.state_transitions);
            assert_eq!(exhaustive.stats.state_solved_rows,on.stats.state_solved_rows);
            assert!(on.stats.option_evaluations <= exhaustive.stats.option_evaluations);
            assert_eq!(off.baseline.to_bits(),on.baseline.to_bits());
            assert_eq!(off.error.to_bits(),on.error.to_bits());
            assert_eq!(off.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>(),
                on.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>());
            assert_eq!(off.stats.note_options,on.stats.note_options);
            assert!(on.stats.candidate_note_visits < off.stats.exhaustive_note_visits);
        } }
    }
}

#[test]
fn scoring_net_upper_keeps_delays_and_encloses_state_history_and_reordered_hits() {
    let mut input = fixture();
    input.notes = (0..48)
        .map(|i| ReplayNote {
            time: 0.3 + (i / 2) as f64 * 0.3 + (i % 2) as f64 * 0.035,
            trigger: (i % 7 == 0 && i < 42).then_some(i / 7),
            window: NoteWindow::Standard,
        })
        .collect();
    input.hit_frames = input
        .notes
        .iter()
        .map(|n| (n.time * 120.).round() as u32)
        .collect();
    input.fever_frames = Some([80, 430]);
    let skills: Vec<_> = (0..6)
        .map(|i| ReplaySkill {
            duration: 0.5 + i as f32 * 0.2,
            correction: None,
            score: match i % 3 {
                0 => ScoreEffect::Crescendo {
                    percent: 100.,
                    step: 0.5,
                    cap: 150,
                },
                1 => ScoreEffect::Continuous {
                    high: 140.,
                    low: 60.,
                },
                _ => ScoreEffect::PerfectOnly { percent: 150. },
            },
        })
        .collect();
    input.skills = skills.clone();
    let p = ScoringPower::new(input.power).unwrap();
    let table =
        TimingNetUpper::new_stateful(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    assert!(table.stats().delayed_windows);
    let upper = table
        .upper_sequence(&std::array::from_fn(|s| skills[s].clone()), p)
        .unwrap();
    let old = TimingNetUpper::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    assert!(
        upper
            <= old
                .upper_sequence(&std::array::from_fn(|s| skills[s].clone()), p)
                .unwrap()
    );
    assert!(table.stats().state_solved_rows > 0);
    let mut random = 0x72a9d13_u32;
    let mut distinct = std::collections::BTreeSet::new();
    let mut reordered = false;
    for sample in 0..128 {
        input.hit_frames = input
            .notes
            .iter()
            .map(|n| {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                ((n.time * 120.).round() as i32 + [-6, 5, 0][(random >> 16) as usize % 3]) as u32
            })
            .collect();
        distinct.insert(input.hit_frames.clone());
        reordered |= input.hit_frames.windows(2).any(|f| f[0] > f[1]);
        let r = replay(&input).unwrap();
        assert!(r.total <= upper, "{sample}: {} > {upper}", r.total);
    }
    assert_eq!(distinct.len(), 128);
    assert!(reordered);
}

fn fixture() -> ReplayInput {
    let mut notes = vec![];
    for s in 0..6 {
        let frame = 120 + 360 * s as u32;
        notes.push(ReplayNote {
            time: f64::from(frame) / 120.,
            trigger: Some(s),
            window: NoteWindow::PinnedPerfect { frame },
        });
        if s == 0 {
            notes.push(ReplayNote {
                time: 1.18,
                trigger: None,
                window: NoteWindow::Standard,
            });
        }
    }
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
                correction: None,
                score: ScoreEffect::Plain { percent: 100. }
            };
            6
        ],
        initial_combo: 699,
        medley_combo: true,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    }
}

#[test]
fn ordinary_net_maximum_includes_great_cost_instead_of_adding_it_to_perfect_base() {
    let input = fixture();
    let skills = vec![input.skills[0].clone()];
    let table =
        TimingNetUpper::new_independent(&input, &skills, ScoringPower::new(600000.).unwrap())
            .unwrap();
    let frames = FrameBounds::new(&input, &skills).unwrap();
    let b = frames.notes[1]
        .iter()
        .filter(|f| f.raw >= Judgement::Great)
        .map(|f| f.weight[1] * f64::from(f.raw.multiplier()))
        .fold(0., f64::max);
    let playing = countdown(skills[0].duration) + 1;
    let inside: Vec<_> = frames.notes[1]
        .iter()
        .filter(|f| 120 < f.time && f.time <= 120 + playing && f.raw >= Judgement::Great)
        .collect();
    assert!(!inside.is_empty());
    assert!(inside.iter().all(|f| f.raw == Judgement::Great));
    let actual = inside
        .iter()
        .map(|f| f.weight[1] * f64::from(f.raw.multiplier()) * 2. - b)
        .fold(0., f64::max);
    assert!((actual - table.gains[0][0]).abs() < 1e-10);
    let old_gain = inside
        .iter()
        .map(|f| f.weight[1] * f64::from(f.raw.multiplier()))
        .fold(0., f64::max);
    assert!(table.gains[0][0] < old_gain * 0.95);
}

#[test]
fn net_upper_encloses_native_frames_correction_state_and_fractional_power() {
    let mut input = fixture();
    input.fever_frames = Some([128, 144]);
    let effects = [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. },
        ScoreEffect::PerfectOrHalf { percent: 140. },
        ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::Continuous {
            high: 140.,
            low: 60.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 150,
        },
    ];
    for score in effects {
        for correction in [
            None,
            Some(JudgementCorrection::Great),
            Some(JudgementCorrection::Bad),
        ] {
            let skill = ReplaySkill {
                duration: 0.1,
                score: score.clone(),
                correction,
            };
            input.skills = vec![skill.clone(); 6];
            let table = TimingNetUpper::new_independent_stateful(
                &input,
                &[skill.clone()],
                ScoringPower::new(600000.).unwrap(),
            )
            .unwrap();
            let domain = search::frame_domains(&input).unwrap()[1].clone();
            for power in [0.01, 300040.75, 599999.94] {
                let p = ScoringPower::new(power).unwrap();
                let upper = table
                    .upper_sequence(&std::array::from_fn(|_| skill.clone()), p)
                    .unwrap();
                for &f in &domain {
                    input.power = power;
                    input.hit_frames[1] = f;
                    let replay = match replay(&input) {
                        Ok(r) => r,
                        Err(ReplayError::Judgement { .. }) => continue,
                        Err(e) => panic!("unexpected replay failure: {e}"),
                    };
                    assert!(
                        replay.total <= upper,
                        "{} > {upper} at {f}: {skill:?}",
                        replay.total
                    );
                }
            }
            input.hit_frames[1] = 142;
        }
    }
}

#[test]
fn independent_net_profiles_share_duration_and_ignore_other_durations() {
    let input = fixture();
    let mut skills = vec![
        input.skills[0].clone(),
        ReplaySkill {
            duration: 0.1,
            correction: None,
            score: ScoreEffect::Plain { percent: 120. },
        },
    ];
    let cap = ScoringPower::new(600000.).unwrap();
    let a = TimingNetUpper::new_independent(&input, &skills, cap).unwrap();
    skills.push(ReplaySkill {
        duration: 30.,
        correction: None,
        score: ScoreEffect::Plain { percent: 100. },
    });
    let b = TimingNetUpper::new_independent(&input, &skills, cap).unwrap();
    assert_eq!(a.gains, &b.gains[..2]);
    assert_eq!(a.baseline, b.baseline);
    assert_eq!(a.stats.geometries, 1);
    assert_eq!(b.stats.geometries, 2);
    let members = std::array::from_fn(|i| skills[i % 2].clone());
    let p = ScoringPower::new(input.power).unwrap();
    let roster = a.upper_roster(&members, p).unwrap();
    for order in crate::hit_timing::layouts::physical_layouts() {
        for captain in 0..5 {
            let sequence =
                std::array::from_fn(|s| members[if s == 5 { captain } else { order[s] }].clone());
            assert!(a.upper_sequence(&sequence, p).unwrap() <= roster);
        }
    }
}

#[test]
fn state_coefficients_do_not_depend_on_power_cap_or_other_skill_durations() {
    let input = fixture();
    let skills = vec![ReplaySkill {
        duration: 0.1,
        correction: None,
        score: ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 150,
        },
    }];
    let a = TimingNetUpper::new_independent_stateful(
        &input,
        &skills,
        ScoringPower::new(400000.).unwrap(),
    )
    .unwrap();
    let b = TimingNetUpper::new_independent_stateful(
        &input,
        &skills,
        ScoringPower::new(800000.).unwrap(),
    )
    .unwrap();
    assert_eq!(a.gains, b.gains);
    let mut more = skills.clone();
    more.push(ReplaySkill {
        duration: 30.,
        ..skills[0].clone()
    });
    let c = TimingNetUpper::new_independent_stateful(
        &input,
        &more,
        ScoringPower::new(800000.).unwrap(),
    )
    .unwrap();
    assert_eq!(b.gains[0], c.gains[0]);
}

#[test]
fn unavoidable_window_notes_match_exhaustive_native_state_replay_at_constant_combo() {
    let mut input = fixture();
    // Both movable notes are necessarily inside slot 0. The first can obtain
    // Fever only with Great. Skipping it used to keep its best external base
    // while preserving a clean Continuous state for the second note.
    input.notes[1].time = 1.3;
    input.hit_frames[1] = 156;
    input.notes.push(ReplayNote {
        time: 1.6,
        trigger: None,
        window: NoteWindow::Standard,
    });
    input.hit_frames.push(192);
    input.initial_combo = 1000;
    input.medley_combo = false;
    input.fever_frames = Some([145, 150]);
    let last = input.notes.len() - 1;
    let effects = [
        // Native P = 1.1 and G = 0.8: 1.4 * P < 2 * G, so preserving
        // the skill at this Fever boundary genuinely incurs a local cost.
        ScoreEffect::Continuous { high: 40., low: 0. },
        ScoreEffect::PerfectOrZero { percent: 40. },
        ScoreEffect::PerfectOrHalf { percent: 40. },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 10.,
            cap: 150,
        },
    ];
    let scale = f64::from(1.0_f32 + (input.play_level - 5) as f32 * 0.01_f32)
        / input.notes.len() as f64
        * 3.;
    for effect in effects {
        let skill = ReplaySkill {
            duration: 1.,
            correction: None,
            score: effect,
        };
        input.skills = vec![skill.clone(); 6];
        let cap = ScoringPower::new(600000.).unwrap();
        let table =
            TimingNetUpper::new_independent_stateful(&input, &[skill.clone()], cap).unwrap();
        let legacy = TimingNetUpper::new_independent(&input, &[skill.clone()], cap).unwrap();
        let domains = search::frame_domains(&input).unwrap();
        let end = 120 + countdown(skill.duration) + 1;
        assert!([1, last]
            .iter()
            .all(|&i| domains[i].iter().all(|&f| 120 < f && f <= end)));
        let mut best = f64::NEG_INFINITY;
        let upper = table
            .upper_sequence(
                &std::array::from_fn(|_| skill.clone()),
                ScoringPower::new(input.power).unwrap(),
            )
            .unwrap();
        for &a in &domains[1] {
            for &b in &domains[last] {
                input.hit_frames[1] = a;
                input.hit_frames[last] = b;
                let result = replay(&input).unwrap();
                let coefficient: f64 = result
                    .trace
                    .iter()
                    .map(|t| {
                        scale
                            * f64::from(t.judgement.multiplier())
                            * f64::from(input.combo_multiplier(t.combo))
                            * f64::from(t.skill_multiplier)
                            * f64::from(t.fever_multiplier)
                    })
                    .sum();
                best = best.max(coefficient);
                assert!(result.total <= upper);
            }
        }
        assert!(table.stats.mandatory_note_rows > 0);
        let found = table.baseline + table.gains[0][0];
        assert!(
            (found - best).abs() < 1e-10,
            "{effect:?}: {found} vs {best}"
        );
        if !matches!(effect, ScoreEffect::Crescendo { .. }) {
            assert!(
                table.gains[0][0] < legacy.gains[0][0] - 0.01,
                "{effect:?}: state {} vs legacy {}",
                table.gains[0][0],
                legacy.gains[0][0]
            );
        }
        input.hit_frames[1] = 156;
        input.hit_frames[last] = 192;
    }
}
