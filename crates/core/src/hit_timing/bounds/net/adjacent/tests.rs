use super::*;
use crate::hit_timing::{clock::SkillClock, JudgementCorrection, ReplayNote, SkillEvent};

fn clock_oracle(
    domains: &[Vec<(u32, Judgement)>; 6],
    skills: &[ReplaySkill; 6],
    value: fn(usize, u32) -> f64,
) -> f64 {
    fn visit(
        slot: usize,
        domains: &[Vec<(u32, Judgement)>; 6],
        skills: &[ReplaySkill; 6],
        chosen: &mut [(u32, Judgement); 6],
        value: fn(usize, u32) -> f64,
        best: &mut f64,
    ) {
        if slot < 6 {
            for &v in &domains[slot] {
                chosen[slot] = v;
                visit(slot + 1, domains, skills, chosen, value, best);
            }
            return;
        }
        if chosen.windows(2).any(|p| p[0].0 > p[1].0) {
            return;
        }
        let mut clock = SkillClock::new(120);
        let mut cursor = 0;
        for frame in 0..10000 {
            while cursor < 6 && chosen[cursor].0 == frame {
                let raw = chosen[cursor].1;
                if clock.active_mut().map_or(raw, |s| s.correct(raw)) < Judgement::Great {
                    return;
                }
                clock.request(cursor, frame);
                cursor += 1;
            }
            clock.after_hits(frame, skills).unwrap();
            if cursor == 6 && clock.finished() {
                break;
            }
        }
        assert_eq!(cursor, 6);
        let total = clock
            .events
            .iter()
            .filter_map(|e| {
                if let SkillEvent::Start {
                    index, after_frame, ..
                } = e
                {
                    Some(value(*index, *after_frame))
                } else {
                    None
                }
            })
            .sum();
        *best = f64::max(*best, total);
    }
    let mut best = 0.;
    visit(
        0,
        domains,
        skills,
        &mut [(0, Judgement::Perfect); 6],
        value,
        &mut best,
    );
    best
}
fn plain(duration: f32) -> ReplaySkill {
    ReplaySkill {
        duration,
        score: ScoreEffect::Plain { percent: 100. },
        correction: None,
    }
}
fn weight(s: usize, f: u32) -> f64 {
    ((s as u32 * 17 + f * 13) % 157) as f64
}

#[test]
fn clock_dp_matches_native_clock_with_chains_corrections_and_ties() {
    let mut skills = std::array::from_fn(|s| plain([0.5, 0.15, 0.25, 0.4, 0.1, 0.3][s]));
    skills[0].correction = Some(JudgementCorrection::Bad);
    skills[3].correction = Some(JudgementCorrection::Good);
    for times in [
        [30, 40, 50, 200, 207, 500],
        [30, 30, 30, 100, 100, 500],
        [30, 200, 400, 700, 1000, 1300],
    ] {
        let mut domains = times.map(|t| {
            vec![
                (t, Judgement::Perfect),
                (t + 1, Judgement::Good),
                (t + 2, Judgement::Bad),
            ]
        });
        for domain in &mut domains {
            domain.swap(0, 2);
        }
        let mut stats = TimingAdjacentStats::default();
        let result = schedule(
            &domains,
            [0, 1, 2, 3, 4, 5],
            skills.each_ref().map(|s| countdown(s.duration) + 1),
            skills.each_ref().map(|s| s.cache_key()[1]),
            WindowOptions::default(),
            &mut stats,
            |s, f, _| Ok(weight(s, f)),
        )
        .unwrap()
        .unwrap();
        let oracle = clock_oracle(&domains, &skills, weight);
        assert!(
            (result - oracle).abs() < 1e-9,
            "{times:?}: {result} != {oracle}"
        );
        assert_eq!(stats.schedule_budget_fallbacks, 0);
    }
}

#[test]
fn adjacent_peaks_cannot_be_taken_at_incompatible_starts() {
    let playing = [61; 6];
    let domains = [
        vec![(30, Judgement::Perfect), (31, Judgement::Perfect)],
        vec![(50, Judgement::Perfect)],
        vec![(1000, Judgement::Perfect)],
        vec![(2000, Judgement::Perfect)],
        vec![(3000, Judgement::Perfect)],
        vec![(4000, Judgement::Perfect)],
    ];
    let second_peak = 31 + playing[0] + countdown(0.75) + 2;
    let mut stats = TimingAdjacentStats::default();
    let value = schedule(
        &domains,
        [0, 1, 2, 3, 4, 5],
        playing,
        [0; 6],
        WindowOptions::default(),
        &mut stats,
        |s, f, _| {
            Ok(if s == 0 && f == 30 || s == 1 && f == second_peak {
                100.
            } else {
                0.
            })
        },
    )
    .unwrap()
    .unwrap();
    assert!((value - 100.).abs() < 1e-9); // Independent maxima would incorrectly add to 200.
}

#[test]
fn adjacent_cooldown_transition_frame_matches_native_clock() {
    let skills = std::array::from_fn(|_| plain(0.15));
    let playing = skills.each_ref().map(|s| countdown(s.duration) + 1);
    let edge = 30 + playing[0] + countdown(0.75) + 1;
    let domains = [
        vec![(30, Judgement::Perfect)],
        vec![
            (edge - 1, Judgement::Great),
            (edge, Judgement::Perfect),
            (edge + 1, Judgement::Perfect),
        ],
        vec![(edge, Judgement::Perfect), (edge + 1, Judgement::Perfect)],
        vec![(500, Judgement::Perfect)],
        vec![(1000, Judgement::Perfect)],
        vec![(1500, Judgement::Perfect)],
    ];
    let result = schedule(
        &domains,
        [0, 1, 2, 3, 4, 5],
        playing,
        [0; 6],
        WindowOptions::default(),
        &mut TimingAdjacentStats::default(),
        |s, f, _| Ok(weight(s, f)),
    )
    .unwrap()
    .unwrap();
    assert!((result - clock_oracle(&domains, &skills, weight)).abs() < 1e-9);
}

#[test]
fn idle_prefixes_merge_without_losing_the_best_start() {
    let domains = std::array::from_fn(|s| {
        (0..22)
            .rev()
            .map(|i| (30 + s as u32 * 1000 + i, Judgement::Perfect))
            .collect::<Vec<_>>()
    });
    let mut stats = TimingAdjacentStats::default();
    let result = schedule(
        &domains,
        [0, 1, 2, 3, 4, 5],
        [61; 6],
        [0; 6],
        WindowOptions::default(),
        &mut stats,
        |s, f, _| Ok(weight(s, f)),
    )
    .unwrap()
    .unwrap();
    let expected: f64 = domains
        .iter()
        .enumerate()
        .map(|(s, domain)| domain.iter().map(|&(f, _)| weight(s, f)).fold(0., f64::max))
        .sum();
    assert!((result - expected).abs() < 1e-9);
    assert_eq!(stats.peak_schedule_states, 1);
    assert_eq!(stats.schedule_transitions, 6 * 22);
}

#[test]
fn equal_frame_rank_restrictions_survive_clock_merging() {
    let domains = std::array::from_fn(|_| vec![(30, Judgement::Perfect)]);
    let result = schedule(
        &domains,
        [1, 0, 2, 3, 4, 5],
        [61; 6],
        [0; 6],
        WindowOptions::default(),
        &mut TimingAdjacentStats::default(),
        |_, _, _| Ok(100.),
    )
    .unwrap()
    .unwrap();
    assert_eq!(result, 0.);
}

fn fixture() -> ReplayInput {
    let mut notes: Vec<_> = [30, 40, 50, 300, 600, 900]
        .into_iter()
        .enumerate()
        .map(|(s, frame)| ReplayNote {
            time: f64::from(frame) / 120.,
            trigger: Some(s),
            window: NoteWindow::PinnedPerfect { frame },
        })
        .collect();
    notes.extend([0.36, 0.48].map(|time| ReplayNote {
        time,
        trigger: None,
        window: NoteWindow::Standard,
    }));
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 19,
        medley_combo: false,
        fever_frames: Some([40, 63]),
        hit_frames: notes
            .iter()
            .map(|n| (n.time * 120.).round() as u32)
            .collect(),
        notes,
        skills: vec![plain(0.5); 6],
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    }
}

#[test]
fn coupled_net_and_pool_enclose_exhaustive_replay_and_reuse_rows() {
    let mut input = fixture();
    let skills = [
        plain(0.15),
        plain(0.5),
        ReplaySkill {
            score: ScoreEffect::Continuous {
                high: 140.,
                low: 60.,
            },
            ..plain(0.5)
        },
        ReplaySkill {
            score: ScoreEffect::Crescendo {
                percent: 100.,
                step: 0.5,
                cap: 150,
            },
            ..plain(0.25)
        },
        ReplaySkill {
            score: ScoreEffect::PerfectOrZero { percent: 150. },
            ..plain(0.4)
        },
    ];
    let cap = ScoringPower::new(600000.).unwrap();
    let table =
        TimingNetUpper::new_adjacent_net(&input, &skills, cap, WindowOptions::default()).unwrap();
    let old = TimingNetUpper::new_stateful(&input, &skills, cap).unwrap();
    let pool =
        PoolTimingUpper::new_adjacent_net(&input, &skills, cap, WindowOptions::default()).unwrap();
    let choices = std::array::from_fn(|k| pool.choice(k, f64::from(input.power) / 5.).unwrap());
    let pool_upper = pool.upper(&choices).unwrap();
    for first in 0..5 {
        input.skills = (0..6).map(|s| skills[(s + first) % 5].clone()).collect();
        let sequence = input.skills.clone().try_into().unwrap();
        let power = ScoringPower::new(input.power).unwrap();
        let upper = table.upper_sequence(&sequence, power).unwrap();
        assert!(upper <= old.upper_sequence(&sequence, power).unwrap());
        assert!(upper <= pool_upper);
        let domains = search::frame_domains(&input).unwrap();
        for &a in &domains[6] {
            for &b in &domains[7] {
                input.hit_frames[6] = a;
                input.hit_frames[7] = b;
                if let Ok(r) = replay(&input) {
                    assert!(r.total <= upper);
                }
            }
        }
    }
    let merged = choices.into_iter().reduce(PoolTimingChoice::union).unwrap();
    assert!(pool.upper(&[merged; 5]).unwrap() >= pool_upper);
    let minimum = choices[0].minimum_components(choices[1]);
    assert_eq!(pool.upper(&[minimum; 5]).unwrap(), 0);
    assert_eq!(table.adjacent_stats().unwrap().schedule_budget_fallbacks, 0);
    assert_eq!(table.adjacent_stats().unwrap().windows.budget_requests, 0);
    let p = ScoringPower::new(input.power).unwrap();
    let a = std::array::from_fn(|_| skills[1].clone());
    let mut b = a.clone();
    b[5] = skills[2].clone();
    table.upper_sequence(&a, p).unwrap();
    table.upper_sequence(&b, p).unwrap();
    assert!(table.adjacent_stats().unwrap().row_hits > 0);
}

#[test]
fn older_skill_can_correct_a_trigger_queued_behind_a_noncorrecting_skill() {
    let mut input = fixture();
    input.skills[0].correction = Some(JudgementCorrection::Bad);
    input.notes[2].time = 0.30;
    input.notes[2].window = NoteWindow::Standard;
    let r = replay(&input).unwrap();
    let n = r.trace.iter().find(|n| n.note_index == 2).unwrap();
    assert_eq!(n.raw, Judgement::Bad);
    assert_eq!(n.active_skill, Some(0));
    let skills = vec![input.skills[0].clone(), input.skills[1].clone()];
    let table = TimingNetUpper::new_adjacent_net(
        &input,
        &skills,
        ScoringPower::new(600000.).unwrap(),
        WindowOptions::default(),
    )
    .unwrap();
    assert!(
        r.total
            <= table
                .upper_sequence(
                    &input.skills.clone().try_into().unwrap(),
                    ScoringPower::new(input.power).unwrap()
                )
                .unwrap()
    );
}

#[test]
fn schedule_budget_returns_complete_old_bound_not_partial_value() {
    let input = fixture();
    let skills = [plain(0.5)];
    let cap = ScoringPower::new(600000.).unwrap();
    let table = TimingNetUpper::new_adjacent_net(
        &input,
        &skills,
        cap,
        WindowOptions {
            max_transitions: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let old = TimingNetUpper::new_stateful(&input, &skills, cap).unwrap();
    let seq = std::array::from_fn(|_| skills[0].clone());
    let p = ScoringPower::new(input.power).unwrap();
    assert_eq!(
        table.upper_sequence(&seq, p).unwrap(),
        old.upper_sequence(&seq, p).unwrap()
    );
    assert!(table.adjacent_stats().unwrap().schedule_budget_fallbacks > 0);
}

#[test]
fn joint_roster_matches_enumerated_real_window_rows_and_reuses_canonical_members() {
    let mut input = fixture();
    input.skills[0].correction = Some(JudgementCorrection::Bad);
    input.notes[2].time = 0.30;
    input.notes[2].window = NoteWindow::Standard;
    let replayed = replay(&input).unwrap();
    let queued = replayed.trace.iter().find(|n| n.note_index == 2).unwrap();
    assert_eq!(queued.raw, Judgement::Bad);
    assert_eq!(queued.active_skill, Some(0));
    assert_eq!(queued.judgement, Judgement::Perfect);
    let skills = [
        ReplaySkill {
            correction: Some(JudgementCorrection::Bad),
            ..plain(0.5)
        },
        ReplaySkill {
            score: ScoreEffect::Crescendo {
                percent: 100.,
                step: 0.5,
                cap: 150,
            },
            ..plain(0.25)
        },
        ReplaySkill {
            score: ScoreEffect::PerfectOrZero { percent: 150. },
            ..plain(0.4)
        },
    ];
    let table = TimingNetUpper::new_adjacent_net(
        &input,
        &skills,
        ScoringPower::new(600000.).unwrap(),
        WindowOptions::default(),
    )
    .unwrap();
    let mut engine = table.adjacent.as_ref().unwrap().lock().unwrap();
    for members in [[0, 0, 1, 2, 2], [0, 1, 1, 1, 2]] {
        let actual = engine.roster(members).unwrap();
        let expected = engine.roster_enumerated(members).unwrap();
        assert_eq!(actual.to_bits(), expected.to_bits());
        let before = engine.stats();
        let mut reversed = members;
        reversed.reverse();
        assert_eq!(engine.roster(reversed).unwrap().to_bits(), actual.to_bits());
        assert_eq!(engine.stats().roster_hits, before.roster_hits + 1);
        assert_eq!(engine.stats().row_builds, before.row_builds);
    }
    assert_eq!(engine.stats().roster_budget_fallbacks, 0);
    assert_eq!(engine.stats().schedule_budget_fallbacks, 0);
    assert_eq!(engine.stats().windows.budget_requests, 0);
}

#[test]
fn joint_roster_budget_publishes_full_independent_cap_and_statistics() {
    let input = fixture();
    let skills = [plain(0.15), plain(0.5)];
    let table = TimingNetUpper::new_adjacent_net(
        &input,
        &skills,
        ScoringPower::new(600000.).unwrap(),
        WindowOptions {
            max_transitions: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let mut engine = table.adjacent.as_ref().unwrap().lock().unwrap();
    let members = [0, 0, 1, 1, 1];
    let expected = joint::independent_cap(members, &engine.caps);
    assert_eq!(
        engine.roster(members).unwrap().to_bits(),
        expected.to_bits()
    );
    // The best-cap seed itself exhausts this tiny budget and publishes the
    // complete cap. The joint search then has nothing that can exceed it.
    assert_eq!(engine.stats().roster_seed_queries, 1);
    assert_eq!(engine.stats().roster_budget_fallbacks, 0);
    assert_eq!(engine.stats().schedule_budget_fallbacks, 1);
}
