use super::*;
use crate::{
    hit_timing::{layouts::physical_layouts, JudgementCorrection, ReplayNote},
    scoring::MemberPower,
};

#[test]
fn minimizing_interval_corners_match_all_products_including_nonfinite_and_unordered_inputs() {
    let values = [f64::NEG_INFINITY,-f64::MAX,-1e100,-1.,-f64::MIN_POSITIVE,
        -f64::from_bits(1),-0.,0.,f64::from_bits(1),f64::MIN_POSITIVE,0.5,1.,1e100,
        f64::MAX,f64::INFINITY,f64::NAN];
    let exhaustive = |a: [f64;2], b: [f64;2]| a.into_iter()
        .flat_map(|x| b.into_iter().map(move |y| (x*y).next_down()))
        .fold(f64::INFINITY,f64::min);
    for &a0 in &values { for &a1 in &values { for &b0 in &values { for &b1 in &values {
        let (a,b)=([a0,a1],[b0,b1]);
        assert_eq!(product_lower(a,b).to_bits(),exhaustive(a,b).to_bits(),"{a:?} {b:?}");
    } } } }
    let mut state=0x619ba57u64;
    for _ in 0..20000 {
        let mut next=|| { state=state.wrapping_mul(6364136223846793005).wrapping_add(1); f64::from_bits(state) };
        let mut a=[next(),next()];
        let mut b=[next(),next()];
        a.sort_by(f64::total_cmp); b.sort_by(f64::total_cmp);
        assert_eq!(product_lower(a,b).to_bits(),exhaustive(a,b).to_bits());
    }
}

// Each request starts independently; overlap ownership is the latest active
#[test]
fn equipment_frame_bank_keeps_coefficient_bits_and_recomputes_native_errors() {
    let skills=catalogue();
    for combo in [0,699] {
        let mut input=fixture(0.045); input.initial_combo=combo;
        let owner=TimingReplacementCache::new(&input,&skills);
        owner.frames.lock().unwrap().enabled=true;
        for power in [300040.75,600000.,800000.,300040.75] {
            let cap=ScoringPower::new(power).unwrap();
            let mut fresh=TimingReplacementBounds::new_adjacent(&input,&skills,cap).unwrap();
            fresh.tighten_frames(&input,&skills).unwrap();
            let mut reused=owner.prepare_tightened(cap).unwrap();
            assert_eq!((fresh.score_error.to_bits(),fresh.power_error.to_bits()),
                (reused.score_error.to_bits(),reused.power_error.to_bits()));
            assert_eq!(fresh.coefficients.iter().map(|r| r.map(|v| v.map(f64::to_bits))).collect::<Vec<_>>(),
                reused.coefficients.iter().map(|r| r.map(|v| v.map(f64::to_bits))).collect::<Vec<_>>());
            for new in 0..skills.len() { for old in 0..skills.len() {
                if fresh.profiles[new].correction!=fresh.profiles[old].correction { continue; }
                let a=fresh.frames.as_mut().unwrap().difference(new,old,&fresh.profiles,&mut fresh.stats);
                let b=reused.frames.as_mut().unwrap().difference(new,old,&reused.profiles,&mut reused.stats);
                assert_eq!(a.map(f64::to_bits),b.map(f64::to_bits));
            } }
        }
        assert!(owner.frame_cache_stats().hits>0);
        assert!(owner.frame_cache_stats().payload_bytes<=frame_cache::MAX_BYTES);
        let other=TimingReplacementCache::new(&input,&skills.iter().rev().cloned().collect::<Vec<_>>());
        assert_eq!(other.frame_cache_stats().hits,0);
        assert_eq!(other.frame_cache_stats().stores,0);
    }
}

// Each request starts independently; overlap ownership is the latest active
// request. No request is delayed, and this does not call the queued SkillClock.
fn independent_score(input: &ReplayInput) -> Option<(u64, Vec<Option<usize>>)> {
    let mut requests = [0; 6];
    for (i, n) in input.notes.iter().enumerate() {
        if let Some(s) = n.trigger {
            requests[s] = input.hit_frames[i];
        }
    }
    if requests.windows(2).any(|v| v[0] > v[1]) {
        return None;
    }
    let mut states: Vec<_> = input
        .skills
        .iter()
        .map(|s| SkillState::new(s).unwrap())
        .collect();
    let ends: Vec<_> = input
        .skills
        .iter()
        .enumerate()
        .map(|(s, skill)| requests[s] + countdown(skill.duration) + 1)
        .collect();
    let scorer = NoteScorer::new(
        ScoringPower::new(input.power).unwrap(),
        input.play_level,
        input.notes.len() as u32,
    )
    .unwrap();
    let mut order: Vec<_> = (0..input.notes.len()).collect();
    order.sort_by_key(|&i| (input.hit_frames[i], i));
    let mut total = 0;
    let mut active = Vec::new();
    for (rank, i) in order.into_iter().enumerate() {
        let f = input.hit_frames[i];
        let slot = (0..6).rev().find(|&s| requests[s] < f && f <= ends[s]);
        let raw = match input.notes[i].window {
            NoteWindow::PinnedPerfect { .. } => Judgement::Perfect,
            window => time_judgement(
                f64::from(f) / 120. - input.notes[i].time,
                matches!(window, NoteWindow::WidenedEndpoint),
            )
            .unwrap(),
        };
        let grade = slot.map_or(raw, |s| states[s].correct(raw));
        if grade < Judgement::Great {
            return None;
        }
        let mult = slot.map_or(1., |s| states[s].multiplier(grade));
        let fever = if input.fever_frames.is_some_and(|[a, b]| a <= f && f < b) {
            2.
        } else {
            1.
        };
        total += u64::from(
            scorer
                .score(
                    grade,
                    input.combo_multiplier(input.initial_combo + rank as u32 + 1),
                    mult,
                    fever,
                )
                .unwrap()
                .final_score,
        );
        active.push(slot);
    }
    Some((total, active))
}

#[test]
fn duration_prefixes_ignore_other_skills_even_when_their_windows_overlap() {
    let input = fixture(0.045);
    let skills = catalogue();
    let bound =
        TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    let mut extended = skills;
    extended.push(skill(ScoreEffect::Plain { percent: 110. }, 30.));
    let extended =
        TimingReplacementBounds::new(&input, &extended, ScoringPower::new(600000.).unwrap())
            .unwrap();
    for (&duration, weights) in &bound.duration_weights {
        assert_eq!(weights, &extended.duration_weights[&duration]);
        assert_eq!(
            bound.guaranteed_weights[&duration],
            extended.guaranteed_weights[&duration]
        );
    }
    for (pair, weights) in &bound.tails {
        assert_eq!(weights.possible, extended.tails[pair].possible);
        assert_eq!(weights.guaranteed, extended.tails[pair].guaranteed);
    }
    for id in 0..bound.profiles.len() {
        assert_eq!(
            bound.scenario_coefficients(id),
            extended.scenario_coefficients(id)
        );
    }
    assert_eq!(bound.stats.model, "independent-skill-windows-v1");
}

#[test]
fn frame_bounds_ignore_other_durations_even_when_windows_overlap() {
    let input = fixture(0.045);
    let skills = catalogue();
    let mut extended_skills = skills.clone();
    extended_skills.push(skill(ScoreEffect::Plain { percent: 110. }, 30.));
    let mut extended_input = input.clone();
    extended_input.skills = vec![extended_skills.last().unwrap().clone(); 6];
    let cap = ScoringPower::new(600000.).unwrap();
    let mut before = TimingReplacementBounds::new(&input, &skills, cap).unwrap();
    let mut after = TimingReplacementBounds::new(&extended_input, &extended_skills, cap).unwrap();
    before.tighten_frames(&input, &skills).unwrap();
    after
        .tighten_frames(&extended_input, &extended_skills)
        .unwrap();
    assert_eq!(before.base_lower, after.base_lower);
    assert_eq!(before.base_upper, after.base_upper);
    for new in 0..skills.len() {
        assert_eq!(
            before.scenario_coefficients(new),
            after.scenario_coefficients(new)
        );
        for old in 0..skills.len() {
            let a = &before.profiles[new];
            let b = &before.profiles[old];
            if a.correction != b.correction || (a.correction != 0 && a.playing != b.playing) {
                continue;
            }
            assert_eq!(
                before.frames.as_mut().unwrap().difference(
                    new,
                    old,
                    &before.profiles,
                    &mut before.stats
                ),
                after.frames.as_mut().unwrap().difference(
                    new,
                    old,
                    &after.profiles,
                    &mut after.stats
                ),
                "frame difference changed for {new} - {old}"
            );
        }
    }
}

#[test]
fn affine_interval_margin_is_below_exhaustive_continuous_differences() {
    for pa in [30., 40., 51.] {
        for pb in [30., 40., 51.] {
            for delta in [-2., 0., 1.] {
                let margin = affine_margin(
                    pa,
                    pb,
                    [100., 180.],
                    [3., 9.],
                    [1., 14.],
                    [-1., 6.],
                    [-2., 5.],
                    delta,
                );
                for q in [100., 121., 180.] {
                    for x in [3., 6., 9.] {
                        for a in [-1., 0., 2., 6.] {
                            for b in [-2., 0., 3., 5.] {
                                if a - b < delta {
                                    continue;
                                }
                                let actual = (q + pa) * (x + a) - (q + pb) * (x + b);
                                assert!(margin <= actual, "{margin} > {actual}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn affine_scenarios_preserve_native_scores_for_all_slots_and_captains() {
    check_native_affine(false);
}

#[test]
fn frame_affine_preserves_native_scores_for_all_slots_and_captains() {
    check_native_affine(true);
}

fn check_native_affine(tight_frames: bool) {
    let mut input = fixture(0.5);
    let mut skills = catalogue();
    for (i, s) in skills.iter_mut().enumerate() {
        s.duration = if i % 2 == 0 { 1.4 } else { 2.4 };
    }
    input.skills = vec![skills[0].clone(); 6];
    let mut bounds =
        TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    if tight_frames {
        bounds.tighten_frames(&input, &skills).unwrap();
    }
    let old_member = member([20000; 3], 0.13);
    let others = [member([22001, 23003, 20009], 0.16); 4];
    let other_power = others.iter().map(|p| p.linear_total()).sum::<f64>();
    let rows = bounds.scenario_coefficients(0);
    // Four identical teammates; normal rows, plus one encore if target is not captain.
    let extra = bounds.coefficients[0][5][0] - bounds.coefficients[0][0][0];
    let extra_hi = bounds.coefficients[0][5][1] - bounds.coefficients[0][0][1];
    let context = TimingAffineContext {
        other_power: [other_power - 1., other_power + 1.],
        required_score: 0.,
        other_coefficient: std::array::from_fn(|s| {
            let mut result = [0_f64; 2];
            for slot in 0..5 {
                if slot == s % 5 {
                    continue;
                }
                result[0] = (result[0] + rows[slot][0]).next_down();
                result[1] = (result[1] + rows[slot][1]).next_up();
            }
            if s < 5 {
                result[0] = (result[0] + extra - 1e-10).next_down();
                result[1] = (result[1] + extra_hi + 1e-10).next_up();
            }
            result
        }),
    };
    let mut checked = 0;
    let mut compensated = 0;
    for new_member in [
        member([19990; 3], 0.13),
        member([22500; 3], 0.13),
        member([28000, 30000, 19000], 0.13),
    ] {
        for a in 0..9 {
            for b in 0..9 {
                if !bounds
                    .can_replace_affine_context(a, b, new_member, old_member, &context, true)
                    .unwrap()
                {
                    continue;
                }
                compensated += usize::from(new_member.linear_total() < old_member.linear_total());
                for scenario in 0..10 {
                    for variant in 0..4 {
                        let mut old = input.clone();
                        old.skills[scenario % 5] = skills[b].clone();
                        if scenario >= 5 {
                            old.skills[5] = skills[b].clone();
                        }
                        for (i, n) in old.notes.iter().enumerate() {
                            if !matches!(n.window, NoteWindow::PinnedPerfect { .. }) {
                                old.hit_frames[i] = (old.hit_frames[i] as i32
                                    + [-5, 6, 0, 0][(i + variant) % 4])
                                    as u32;
                            }
                        }
                        let mut new = old.clone();
                        new.skills[scenario % 5] = skills[a].clone();
                        if scenario >= 5 {
                            new.skills[5] = skills[a].clone();
                        }
                        for layout in physical_layouts().iter().step_by(31) {
                            let x = [old_member, others[0], others[1], others[2], others[3]];
                            let y = [new_member, others[0], others[1], others[2], others[3]];
                            old.power = ScoringPower::from_member_parameters(
                                &layout.map(|i| x[i].final_parameters()),
                            )
                            .unwrap()
                            .value();
                            new.power = ScoringPower::from_member_parameters(
                                &layout.map(|i| y[i].final_parameters()),
                            )
                            .unwrap()
                            .value();
                            let Some(before) = independent_score(&old) else {
                                continue;
                            };
                            let after = independent_score(&new).unwrap();
                            assert!(
                                after.0 >= before.0,
                                "{a} replaces {b}, scenario {scenario}: {} < {}",
                                after.0,
                                before.0
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(
        checked > 1000 && compensated > 0,
        "checked {checked}, lower-power replacements {compensated}"
    );
}

#[test]
fn one_request_cannot_claim_both_ends_of_the_possible_union() {
    let mut input = fixture(0.5);
    input.notes = vec![
        ReplayNote {
            time: 117. / 120.,
            trigger: None,
            window: NoteWindow::PinnedPerfect { frame: 117 },
        },
        ReplayNote {
            time: 1.,
            trigger: Some(0),
            window: NoteWindow::Standard,
        },
        ReplayNote {
            time: 133. / 120.,
            trigger: None,
            window: NoteWindow::PinnedPerfect { frame: 133 },
        },
    ];
    for s in 1..6 {
        let frame = 120 + s as u32 * 240;
        input.notes.push(ReplayNote {
            time: f64::from(frame) / 120.,
            trigger: Some(s),
            window: NoteWindow::PinnedPerfect { frame },
        });
    }
    input.hit_frames = input
        .notes
        .iter()
        .map(|n| (n.time * 120.).round() as u32)
        .collect();
    input.fever_frames = None;
    let skills = vec![skill(ScoreEffect::Plain { percent: 100. }, 0.06)];
    input.skills = vec![skills[0].clone(); 6];
    let mut bound =
        TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    let before = bound.scenario_coefficients(0)[0];
    bound.tighten_frames(&input, &skills).unwrap();
    let after = bound.scenario_coefficients(0)[0];
    assert!(after[1] < before[1] * 0.8, "{before:?} -> {after:?}");
    // Exhaust every allowed request frame. Other notes are pinned, so the
    // continuous contribution can be calculated directly without the table.
    let domain = search::frame_domains(&input).unwrap();
    let factor = f64::from(1.0_f32 + (input.play_level - 5) as f32 * 0.01_f32)
        / input.notes.len() as f64
        * 3.;
    for &request in &domain[1] {
        let mut frames = input.hit_frames.clone();
        frames[1] = request;
        let mut order: Vec<_> = (0..frames.len()).collect();
        order.sort_by_key(|&i| (frames[i], i));
        let mut contribution = 0.;
        for (rank, i) in order.into_iter().enumerate() {
            if input.notes[i].trigger.is_none()
                && request < frames[i]
                && frames[i] <= request + countdown(skills[0].duration) + 1
            {
                contribution += factor
                    * f64::from(Judgement::Perfect.multiplier())
                    * f64::from(input.combo_multiplier(input.initial_combo + rank as u32 + 1));
            }
        }
        assert!(
            after[0] <= contribution && contribution <= after[1],
            "{request}: {contribution} outside {after:?}"
        );
    }
}

#[test]
fn frame_coefficients_cover_judgment_history_combo_fever_and_correction() {
    let mut input = fixture(0.5);
    let mut skills = catalogue();
    for s in &mut skills {
        s.duration = 2.4;
    }
    input.skills = vec![skills[0].clone(); 6];
    input.fever_frames = Some([65, 1210]);
    let mut bound =
        TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    bound.tighten_frames(&input, &skills).unwrap();
    let factor = f64::from(1.0_f32 + (input.play_level - 5) as f32 * 0.01_f32)
        / input.notes.len() as f64
        * 3.;
    for (id, skill) in skills.iter().enumerate() {
        for variant in 0..16 {
            let mut frames = input.hit_frames.clone();
            for (i, n) in input.notes.iter().enumerate() {
                if !matches!(n.window, NoteWindow::PinnedPerfect { .. }) {
                    frames[i] = (frames[i] as i32 + [-5, 6, 0, 0][(i + variant) % 4]) as u32;
                }
            }
            let mut requests = [0; 6];
            for (i, n) in input.notes.iter().enumerate() {
                if let Some(s) = n.trigger {
                    requests[s] = frames[i];
                }
            }
            let mut states: Vec<_> = (0..6).map(|_| SkillState::new(skill).unwrap()).collect();
            let mut order: Vec<_> = (0..frames.len()).collect();
            order.sort_by_key(|&i| (frames[i], i));
            let mut base = 0.;
            let mut gains = [0.; 6];
            for (rank, i) in order.into_iter().enumerate() {
                let raw = match input.notes[i].window {
                    NoteWindow::PinnedPerfect { .. } => Judgement::Perfect,
                    w => time_judgement(
                        f64::from(frames[i]) / 120. - input.notes[i].time,
                        matches!(w, NoteWindow::WidenedEndpoint),
                    )
                    .unwrap(),
                };
                let slot = (0..6).find(|&s| {
                    requests[s] < frames[i]
                        && frames[i] <= requests[s] + countdown(skill.duration) + 1
                });
                let grade = slot.map_or(raw, |s| states[s].correct(raw));
                let fever = if input
                    .fever_frames
                    .is_some_and(|[a, b]| a <= frames[i] && frames[i] < b)
                {
                    2.
                } else {
                    1.
                };
                let w = factor
                    * f64::from(grade.multiplier())
                    * f64::from(input.combo_multiplier(input.initial_combo + rank as u32 + 1))
                    * fever;
                base += w;
                if let Some(s) = slot {
                    gains[s] += w * (f64::from(states[s].multiplier(grade)) - 1.);
                }
            }
            assert!(bound.base_lower <= base && base <= bound.base_upper);
            for (s, row) in bound.scenario_coefficients(id).iter().enumerate() {
                let value = gains[s % 5] + if s >= 5 { gains[5] } else { 0. };
                assert!(
                    row[0] <= value && value <= row[1],
                    "skill {id} scenario {s}: {value} outside {row:?}"
                );
            }
        }
    }
}

pub(super) fn fixture(spacing: f64) -> ReplayInput {
    let notes: Vec<_> = (0..42)
        .map(|i| ReplayNote {
            time: 0.3 + i as f64 * spacing,
            trigger: (i % 7 == 0).then_some(i / 7),
            window: if i % 11 == 3 {
                NoteWindow::PinnedPerfect {
                    frame: ((0.3 + i as f64 * spacing) * 120.).round() as u32,
                }
            } else if i % 7 == 2 {
                NoteWindow::WidenedEndpoint
            } else {
                NoteWindow::Standard
            },
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
        skills: vec![skill(ScoreEffect::Plain { percent: 110. }, 0.4); 6],
        initial_combo: 699,
        medley_combo: true,
        fever_frames: Some([70, 145]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    }
}
fn skill(score: ScoreEffect, duration: f32) -> ReplaySkill {
    ReplaySkill {
        duration,
        score,
        correction: None,
    }
}
fn member(axes: [u32; 3], rate: f32) -> MemberPower {
    MemberPower::from_bonuses(axes, [0.; 3], [rate; 3], 0., [0.; 3]).unwrap()
}
pub(super) fn catalogue() -> Vec<ReplaySkill> {
    vec![
        skill(ScoreEffect::Plain { percent: 110. }, 0.4),
        skill(ScoreEffect::Plain { percent: 105. }, 0.4),
        skill(ScoreEffect::PerfectOnly { percent: 110. }, 0.4),
        skill(ScoreEffect::PerfectOrZero { percent: 110. }, 0.4),
        skill(ScoreEffect::PerfectOrHalf { percent: 110. }, 0.4),
        skill(
            ScoreEffect::Continuous {
                high: 110.,
                low: 50.,
            },
            0.4,
        ),
        skill(
            ScoreEffect::Crescendo {
                percent: 100.,
                step: 0.5,
                cap: 150,
            },
            0.4,
        ),
        skill(
            ScoreEffect::Crescendo {
                percent: 95.,
                step: 0.25,
                cap: 150,
            },
            0.4,
        ),
        skill(ScoreEffect::Plain { percent: 300. }, 0.5),
        ReplaySkill {
            correction: Some(JudgementCorrection::Bad),
            ..skill(ScoreEffect::Plain { percent: 110. }, 0.4)
        },
    ]
}

#[test]
fn adjacent_coefficients_enclose_native_delays_and_chains() {
    for spacing in [0.045, 0.19, 0.5] {
        let mut input = fixture(spacing);
        let skills = catalogue();
        let mut model = TimingReplacementBounds::new_adjacent(
            &input,
            &skills,
            ScoringPower::new(600000.).unwrap(),
        )
        .unwrap();
        model.tighten_frames(&input, &skills).unwrap();
        let original = input.hit_frames.clone();
        let scale = f64::from(1_f32 + (input.play_level - 5) as f32 * 0.01_f32)
            / input.notes.len() as f64
            * 3.;
        for (id, skill) in skills.iter().enumerate() {
            input.skills = vec![skill.clone(); 6];
            for variant in 0..8 {
                for (i, n) in input.notes.iter().enumerate() {
                    input.hit_frames[i] = if matches!(n.window, NoteWindow::PinnedPerfect { .. }) {
                        original[i]
                    } else {
                        (original[i] as i32 + [-5, 6, 0, 0][(i + variant) % 4]) as u32
                    };
                }
                let Ok(r) = replay(&input) else { continue };
                let mut base = 0.;
                let mut gains = [0.; 6];
                for n in r.trace {
                    let w = scale
                        * f64::from(n.judgement.multiplier())
                        * f64::from(input.combo_multiplier(n.combo))
                        * f64::from(n.fever_multiplier);
                    base += w;
                    if let Some(s) = n.active_skill {
                        gains[s] += w * (f64::from(n.skill_multiplier) - 1.);
                    }
                }
                assert!(model.base_lower <= base && base <= model.base_upper);
                for (s, interval) in model.scenario_coefficients(id).iter().enumerate() {
                    let v = gains[s % 5] + if s >= 5 { gains[5] } else { 0. };
                    assert!(
                        interval[0] <= v && v <= interval[1],
                        "{spacing} {id} {s}: {v} not in {interval:?}"
                    );
                }
            }
        }
        assert_eq!(model.stats().model, "adjacent-skill-windows-v1");
    }
}

#[test]
fn duration_replacement_requires_unchanged_successor_clock() {
    let skills = catalogue();
    let close = fixture(0.045);
    let far = fixture(0.5);
    let cap = ScoringPower::new(600000.).unwrap();
    let mut queued = TimingReplacementBounds::new_adjacent(&close, &skills, cap).unwrap();
    assert!(queued.extended_coefficient(8, 0).unwrap().is_none());
    assert_eq!(queued.stats().successor_changing_pairs, 1);
    // Equal durations preserve all starts, even with a long queue.
    assert!(queued.signed_slot_coefficients(0, 1).unwrap().is_some());
    let mut separated = TimingReplacementBounds::new_adjacent(&far, &skills, cap).unwrap();
    assert!(separated.extended_coefficient(8, 0).unwrap().is_some());
}

#[test]
fn correction_replacement_preserves_frames_queue_and_native_scores() {
    let corrections = [
        None,
        Some(JudgementCorrection::Great),
        Some(JudgementCorrection::Good),
        Some(JudgementCorrection::Bad),
    ];
    let effects = [
        ScoreEffect::Plain { percent: 10. },
        ScoreEffect::Plain { percent: 140. },
        ScoreEffect::Continuous {
            high: 160.,
            low: 50.,
        },
        ScoreEffect::Crescendo {
            percent: 60.,
            step: 0.5,
            cap: 150,
        },
        ScoreEffect::PerfectOnly { percent: 150. },
    ];
    let skills: Vec<_> = corrections
        .iter()
        .flat_map(|&correction| {
            effects.iter().map(move |score| ReplaySkill {
                duration: 2.4,
                correction,
                score: score.clone(),
            })
        })
        .collect();
    let old_power = member([20000; 3], 0.13);
    let others = [member([23001, 25103, 20199], 0.15); 4];
    let q: f64 = others.iter().map(|p| p.linear_total()).sum();
    let mut certificates = 0;
    let mut checked = 0;
    let mut changed_grade = 0;
    let mut raw_below_great = 0;
    let mut queued = 0;
    let mut reduced_power = 0;
    for spacing in [0.19, 0.5] {
        let input = fixture(spacing);
        let mut model = TimingReplacementBounds::new_adjacent(
            &input,
            &skills,
            ScoringPower::new(600000.).unwrap(),
        )
        .unwrap();
        model.tighten_frames(&input, &skills).unwrap();
        let upper = model.coefficient_upper(&(0..skills.len()).collect::<Vec<_>>());
        for new_power in [member([22000; 3], 0.13), member([19990; 3], 0.13)] {
            for a in 0..skills.len() {
                for b in 0..skills.len() {
                    if !model
                        .can_replace_correction_context(
                            a,
                            b,
                            new_power,
                            old_power,
                            [q - 1., q + 1.],
                            upper,
                            0.,
                        )
                        .unwrap()
                    {
                        continue;
                    }
                    certificates += 1;
                    reduced_power +=
                        usize::from(new_power.linear_total() < old_power.linear_total());
                    for role in 0..10 {
                        for variant in 0..4 {
                            let mut old = input.clone();
                            old.skills = vec![skills[15].clone(); 6];
                            old.skills[role % 5] = skills[b].clone();
                            if role >= 5 {
                                old.skills[5] = skills[b].clone();
                            }
                            let nominal = replay(&old).unwrap();
                            let mut caps = vec![6; old.notes.len()];
                            for n in nominal.trace {
                                caps[n.note_index] =
                                    n.active_skill
                                        .map_or(6, |s| match old.skills[s].correction {
                                            Some(JudgementCorrection::Bad) => 13,
                                            Some(JudgementCorrection::Good) => 9,
                                            _ => 6,
                                        });
                            }
                            for (i, n) in old.notes.iter().enumerate() {
                                if !matches!(n.window, NoteWindow::PinnedPerfect { .. }) {
                                    let offset = if n.trigger.is_some() {
                                        0
                                    } else {
                                        match variant {
                                            0 => 0,
                                            1 => 6,
                                            2 => caps[i],
                                            _ => {
                                                if i % 2 == 0 {
                                                    caps[i]
                                                } else {
                                                    0
                                                }
                                            }
                                        }
                                    };
                                    old.hit_frames[i] += offset;
                                }
                            }
                            let mut new = old.clone();
                            new.skills[role % 5] = skills[a].clone();
                            if role >= 5 {
                                new.skills[5] = skills[a].clone();
                            }
                            for layout in physical_layouts().iter().step_by(47) {
                                let before =
                                    [old_power, others[0], others[1], others[2], others[3]];
                                let after = [new_power, others[0], others[1], others[2], others[3]];
                                old.power = ScoringPower::from_member_parameters(
                                    &layout.map(|i| before[i].final_parameters()),
                                )
                                .unwrap()
                                .value();
                                new.power = ScoringPower::from_member_parameters(
                                    &layout.map(|i| after[i].final_parameters()),
                                )
                                .unwrap()
                                .value();
                                let Ok(x) = replay(&old) else { continue };
                                let y =
                                    replay(&new).expect("certified same frames must stay legal");
                                assert!(
                                    y.total >= x.total,
                                    "{a} replaces {b} role {role}: {} < {}",
                                    y.total,
                                    x.total
                                );
                                assert_eq!(x.skill_events, y.skill_events);
                                for (x, y) in x.trace.iter().zip(&y.trace) {
                                    assert_eq!(x.active_skill, y.active_skill);
                                    changed_grade += usize::from(x.judgement != y.judgement);
                                    raw_below_great += usize::from(x.raw < Judgement::Great);
                                }
                                queued += usize::from(x.skill_events.iter().any(|event| {
                                    let super::super::super::SkillEvent::Start {
                                        index,
                                        after_frame,
                                        ..
                                    } = event
                                    else {
                                        return false;
                                    };
                                    x.skill_events.iter().any(|r| {
                                        matches!(r,
                                        super::super::super::SkillEvent::Request { index: i, frame }
                                        if i == index && after_frame > frame)
                                    })
                                }));
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        // Removing Good/Bad acceptance cannot be certified by a same-frame proof.
        for &(a, b) in &[(1, 10), (6, 15), (11, 15)] {
            assert!(!model
                .can_replace_correction_context(
                    a,
                    b,
                    member([50000; 3], 0.),
                    old_power,
                    [q - 1., q + 1.],
                    upper,
                    0.
                )
                .unwrap());
        }
    }
    assert!(certificates > 10 && checked > 100 && changed_grade > 0
        && raw_below_great > 0 && queued > 0 && reduced_power > 0,
        "{certificates} certificates, {checked} replays, {changed_grade} changed, {raw_below_great} raw Good/Bad, {queued} queued, {reduced_power} reduced power");
}

#[test]
fn independent_window_replacements_cover_great_history_captain_and_native_layouts() {
    let cards = [
        member([20101, 22009, 17890], 0.123),
        member([20303, 19797, 18911], 0.076),
        member([19743, 22543, 19091], 0.18),
        member([20743, 19543, 19491], 0.33),
    ];
    let powers = [
        (member([20000; 3], 0.13), member([20000; 3], 0.13)),
        (member([20000; 3], 0.13), member([26000; 3], 0.13)),
        // Scalar improvement without componentwise dominance.
        (
            member([27000, 18000, 15000], 0.13),
            member([26000, 29000, 30000], 0.13),
        ),
    ];
    let mut certified = 0;
    let mut compensated = 0;
    let mut replays = 0;
    for spacing in [0.045, 0.19] {
        let input = fixture(spacing);
        let skills = catalogue();
        let mut bound =
            TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap())
                .unwrap();
        for &(old_power, new_power) in &powers {
            for a in 0..8 {
                for b in 0..8 {
                    if !bound.can_replace(a, b, new_power, old_power).unwrap() {
                        continue;
                    }
                    certified += 1;
                    compensated += usize::from(bound.loss_coefficient(a, b).unwrap().unwrap() > 0.);
                    for variant in 0..8 {
                        let mut old = input.clone();
                        old.skills = vec![skills[(variant + 2) % 8].clone(); 6];
                        let slot = variant % 5;
                        old.skills[slot] = skills[b].clone();
                        if variant % 2 == 0 {
                            old.skills[5] = skills[b].clone();
                        }
                        for (i, n) in old.notes.iter().enumerate() {
                            if !matches!(n.window, NoteWindow::PinnedPerfect { .. }) {
                                let shift = match (i * 17 + variant * 3) % 5 {
                                    0 => -5,
                                    1 => 6,
                                    _ => 0,
                                };
                                old.hit_frames[i] = (old.hit_frames[i] as i32 + shift) as u32;
                            }
                        }
                        let mut new = old.clone();
                        new.skills[slot] = skills[a].clone();
                        if variant % 2 == 0 {
                            new.skills[5] = skills[a].clone();
                        }
                        for layout in physical_layouts().iter().step_by(17) {
                            let old_rows = [old_power, cards[0], cards[1], cards[2], cards[3]];
                            let new_rows = [new_power, cards[0], cards[1], cards[2], cards[3]];
                            old.power = ScoringPower::from_member_parameters(
                                &layout.map(|i| old_rows[i].final_parameters()),
                            )
                            .unwrap()
                            .value();
                            new.power = ScoringPower::from_member_parameters(
                                &layout.map(|i| new_rows[i].final_parameters()),
                            )
                            .unwrap()
                            .value();
                            let Some(x) = independent_score(&old) else {
                                continue;
                            };
                            let y = independent_score(&new)
                                .expect("same-frame replacement must stay legal");
                            assert_eq!(x.1, y.1);
                            assert!(y.0 >= x.0, "{a} replaces {b}: {} < {}", y.0, x.0);
                            replays += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(
        certified > 50 && compensated > 10 && replays > 1000,
        "{certified} / {compensated} / {replays}"
    );
}

#[test]
fn rejects_different_durations_corrections_and_unsafe_scalar_comparisons() {
    let input = fixture(0.19);
    let skills = catalogue();
    let mut bound =
        TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    let same = member([20000; 3], 0.);
    assert_eq!(bound.loss_coefficient(8, 0).unwrap(), None);
    assert_eq!(bound.loss_coefficient(9, 0).unwrap(), None);
    assert!(!bound.can_replace(1, 0, same, same).unwrap());
    assert!(!bound.can_replace(3, 0, same, same).unwrap());
    assert!(bound.loss_coefficient(999, 0).is_err());
    assert!(bound.can_replace(0, 2, same, same).unwrap());
    assert_eq!(bound.stats.duration_profiles, 2);
    assert_eq!(bound.stats.duration_queries, 12);
    assert!(bound.stats.prefix_entries <= 6 * input.notes.len());
    for weights in bound
        .duration_weights
        .values()
        .collect::<Vec<_>>()
        .windows(2)
    {
        assert!((0..6).all(|s| weights[0][s] <= weights[1][s]));
    }
}

#[test]
fn state_pair_maxima_cover_all_short_judgment_histories() {
    let input = fixture(0.19);
    let skills = catalogue();
    let bound =
        TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    for a in 0..8 {
        for b in 0..8 {
            let upper = bound.profiles[a]
                .states
                .iter()
                .zip(&bound.profiles[b].states)
                .flat_map(|(a, b)| (0..4).map(move |k| f64::from(b[k]) - f64::from(a[k])))
                .fold(0., f64::max);
            for mask in 0..256 {
                let mut x = SkillState::new(&skills[a]).unwrap();
                let mut y = SkillState::new(&skills[b]).unwrap();
                for i in 0..8 {
                    let g = if mask & (1 << i) == 0 {
                        Judgement::Perfect
                    } else {
                        Judgement::Great
                    };
                    assert!(f64::from(y.multiplier(g)) - f64::from(x.multiplier(g)) <= upper);
                }
            }
        }
    }
}

#[test]
fn signed_context_can_trade_lower_power_for_stronger_skill() {
    // Six independent, separated windows. The new experiment does not inspect
    // neighboring skills to decide whether it is enabled.
    let mut input = fixture(0.5);
    let mut skills = catalogue();
    for skill in &mut skills {
        skill.duration = 2.4;
    }
    input.skills = vec![skills[0].clone(); 6];
    let mut bounds =
        TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    let old_member = member([20000; 3], 0.13);
    let new_member = member([19990; 3], 0.13);
    let others = [member([23001, 25103, 20199], 0.15); 4];
    let other_total: f64 = others.iter().map(|p| p.linear_total()).sum();
    let coefficient = bounds.coefficient_upper(&(0..skills.len()).collect::<Vec<_>>());
    assert!(!bounds.can_replace(0, 1, new_member, old_member).unwrap());
    assert!(bounds
        .can_replace_in_context(
            0,
            1,
            new_member,
            old_member,
            [other_total - 1., other_total + 1.],
            coefficient,
            0.
        )
        .unwrap());
    let mut count = 0;
    for a in 0..8 {
        for b in 0..8 {
            if !bounds
                .can_replace_in_context(
                    a,
                    b,
                    new_member,
                    old_member,
                    [other_total - 1., other_total + 1.],
                    coefficient,
                    0.,
                )
                .unwrap()
            {
                continue;
            }
            for variant in 0..8 {
                for slot in 0..5 {
                    let mut old = input.clone();
                    old.skills = vec![skills[variant].clone(); 6];
                    old.skills[slot] = skills[b].clone();
                    if variant % 2 == 0 {
                        old.skills[5] = skills[b].clone();
                    }
                    for (i, n) in old.notes.iter().enumerate() {
                        if !matches!(n.window, NoteWindow::PinnedPerfect { .. }) {
                            let delta = [-5, 6, 0, 0, 0][(i + variant) % 5];
                            old.hit_frames[i] = (old.hit_frames[i] as i32 + delta) as u32;
                        }
                    }
                    let mut new = old.clone();
                    new.skills[slot] = skills[a].clone();
                    if variant % 2 == 0 {
                        new.skills[5] = skills[a].clone();
                    }
                    for layout in physical_layouts().iter().step_by(13) {
                        let old_rows = [old_member, others[0], others[1], others[2], others[3]];
                        let new_rows = [new_member, others[0], others[1], others[2], others[3]];
                        old.power = ScoringPower::from_member_parameters(
                            &layout.map(|i| old_rows[i].final_parameters()),
                        )
                        .unwrap()
                        .value();
                        new.power = ScoringPower::from_member_parameters(
                            &layout.map(|i| new_rows[i].final_parameters()),
                        )
                        .unwrap()
                        .value();
                        let Some(x) = independent_score(&old) else {
                            continue;
                        };
                        let y = independent_score(&new).unwrap();
                        assert!(y.0 >= x.0, "{a} replaces {b}: {} < {}", y.0, x.0);
                        count += 1;
                    }
                }
            }
        }
    }
    assert!(count > 1000, "only {count} certified replays");
}

#[test]
fn different_duration_common_and_tail_bounds_match_independent_replays() {
    let mut input = fixture(0.5);
    let mut skills = vec![];
    for original in catalogue().into_iter().take(9) {
        for duration in [1.4, 2.4] {
            skills.push(ReplaySkill {
                duration,
                ..original.clone()
            });
        }
    }
    input.skills = vec![skills[0].clone(); 6];
    let mut bounds =
        TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    assert_eq!(bounds.stats.duration_pair_profiles, 1);
    assert_eq!(bounds.stats.tail_queries, 12);
    let others = [member([22001, 23003, 20009], 0.16); 4];
    let other_sum = others.iter().map(|p| p.linear_total()).sum::<f64>();
    let old_member = member([20000; 3], 0.13);
    let coefficient = bounds.coefficient_upper(&(0..skills.len()).collect::<Vec<_>>());
    assert!(!bounds
        .can_replace_in_context(
            1,
            0,
            old_member,
            old_member,
            [other_sum, other_sum],
            coefficient,
            0.
        )
        .unwrap());
    assert!(bounds
        .can_replace_extended_context(
            1,
            0,
            old_member,
            old_member,
            [other_sum, other_sum],
            coefficient,
            0.
        )
        .unwrap());
    let mut checked = [0_usize; 2];
    for new_member in [
        old_member,
        member([19990; 3], 0.13),
        member([25000; 3], 0.13),
    ] {
        for a in 0..skills.len() {
            for b in 0..skills.len() {
                if a % 2 == b % 2 {
                    continue;
                }
                if !bounds
                    .can_replace_extended_context(
                        a,
                        b,
                        new_member,
                        old_member,
                        [other_sum - 1., other_sum + 1.],
                        coefficient,
                        0.,
                    )
                    .unwrap()
                {
                    continue;
                }
                for variant in 0..6 {
                    let slot = variant % 5;
                    let mut old = input.clone();
                    old.skills = vec![skills[(variant * 3) % 16].clone(); 6];
                    old.skills[slot] = skills[b].clone();
                    if variant % 2 == 0 {
                        old.skills[5] = skills[b].clone();
                    }
                    for (i, note) in old.notes.iter().enumerate() {
                        if !matches!(note.window, NoteWindow::PinnedPerfect { .. }) {
                            old.hit_frames[i] = (old.hit_frames[i] as i32
                                + [-5, 6, 0, 0][(i + variant) % 4])
                                as u32;
                        }
                    }
                    let mut new = old.clone();
                    new.skills[slot] = skills[a].clone();
                    if variant % 2 == 0 {
                        new.skills[5] = skills[a].clone();
                    }
                    for layout in physical_layouts().iter().step_by(31) {
                        let x = [old_member, others[0], others[1], others[2], others[3]];
                        let y = [new_member, others[0], others[1], others[2], others[3]];
                        old.power = ScoringPower::from_member_parameters(
                            &layout.map(|i| x[i].final_parameters()),
                        )
                        .unwrap()
                        .value();
                        new.power = ScoringPower::from_member_parameters(
                            &layout.map(|i| y[i].final_parameters()),
                        )
                        .unwrap()
                        .value();
                        let Some(before) = independent_score(&old) else {
                            continue;
                        };
                        let after = independent_score(&new).unwrap();
                        assert!(
                            after.0 >= before.0,
                            "{a} replaces {b}: {} < {}",
                            after.0,
                            before.0
                        );
                        checked[usize::from(a % 2 == 1)] += 1;
                    }
                }
            }
        }
    }
    assert!(
        checked.iter().all(|n| *n > 0) && checked.iter().sum::<usize>() > 1000,
        "shorter/longer {checked:?}"
    );
}

#[test]
fn duration_changes_do_not_silently_remove_judgement_correction() {
    let input = fixture(0.5);
    let mut skills = catalogue();
    skills.push(ReplaySkill {
        duration: 1.4,
        ..skills[9].clone()
    });
    let mut bounds =
        TimingReplacementBounds::new(&input, &skills, ScoringPower::new(600000.).unwrap()).unwrap();
    let same = member([20000; 3], 0.13);
    let coefficient = bounds.coefficient_upper(&(0..skills.len()).collect::<Vec<_>>());
    assert!(!bounds
        .can_replace_extended_context(9, 10, same, same, [200000., 300000.], coefficient, 0.)
        .unwrap());
    assert!(!bounds
        .can_replace_extended_context(10, 9, same, same, [200000., 300000.], coefficient, 0.)
        .unwrap());
}
