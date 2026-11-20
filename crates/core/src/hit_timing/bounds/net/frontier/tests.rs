use super::*;
use crate::hit_timing::{
    independent::WindowOptions, layouts::physical_layouts, search::frame_domains,
    JudgementCorrection, ReplayNote, SkillEvent,
};

fn fixture() -> ReplayInput {
    let mut notes: Vec<_> = (0..6)
        .map(|s| {
            let frame = 30 + s as u32 * 360;
            ReplayNote {
                time: f64::from(frame) / 120.,
                trigger: Some(s),
                window: NoteWindow::PinnedPerfect { frame },
            }
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
        skills: vec![
            ReplaySkill {
                duration: 0.15,
                score: ScoreEffect::Plain { percent: 100. },
                correction: None
            };
            6
        ],
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    }
}
fn catalogue() -> Vec<ReplaySkill> {
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
            cap: 150,
        },
    ]
    .into_iter()
    .map(|score| ReplaySkill {
        duration: 0.15,
        score,
        correction: None,
    })
    .collect()
}
fn cap() -> ScoringPower {
    ScoringPower::new(600000.).unwrap()
}

struct RawBounds {
    references: Vec<f64>,
    profiles: HashMap<[u32; 6], [f64; 6]>,
}
impl RawBounds {
    fn new(input: &ReplayInput, skills: &[ReplaySkill], options: WindowOptions) -> Self {
        let references = FrameBounds::new(input, skills).unwrap().note_baselines();
        let mut windows =
            FrontierNetWindows::new(input, skills, references.clone(), options).unwrap();
        let profiles = skills
            .iter()
            .map(|s| (s.cache_key(), windows.profile(s).unwrap().0))
            .collect();
        Self {
            references,
            profiles,
        }
    }
}

// This oracle uses actual six-skill replay, not local graph factors. Check the
// UNCLIPPED frontier profiles too: an old-table cap must not hide a graph bug.
fn check_plan(input: &ReplayInput, table: &TimingNetUpper, raw: &RawBounds) -> bool {
    let Ok(replayed) = replay(input) else {
        return false;
    };
    let rate = f64::from(1_f32 + (input.play_level - 5) as f32 * 0.01_f32);
    let mut contribution = [0.; 6];
    let mut total = 0.;
    let mut outside_loss = 0.;
    for n in &replayed.trace {
        let actual = rate / input.notes.len() as f64
            * 3.
            * f64::from(n.judgement.multiplier())
            * f64::from(input.combo_multiplier(n.combo))
            * f64::from(n.fever_multiplier * n.skill_multiplier);
        let delta = actual - raw.references[n.note_index];
        if let Some(s) = n.active_skill {
            contribution[s] += delta;
        } else {
            assert!(delta <= 1e-14);
            outside_loss += delta;
        }
        total += actual;
    }
    assert!(
        (raw.references.iter().sum::<f64>() + contribution.iter().sum::<f64>() + outside_loss
            - total)
            .abs()
            < 1e-12
    );
    for s in 0..6 {
        let upper = table.row(&input.skills[s]).unwrap()[s];
        assert!(
            contribution[s] <= upper,
            "slot {s}: {} > {upper}",
            contribution[s]
        );
        assert!(contribution[s] <= raw.profiles[&input.skills[s].cache_key()][s]);
    }
    let sequence = std::array::from_fn(|s| input.skills[s].clone());
    let upper = table
        .upper_sequence(&sequence, ScoringPower::new(input.power).unwrap())
        .unwrap();
    assert!(replayed.total <= upper, "{} > {upper}", replayed.total);
    true
}

#[test]
fn frontier_sum_bounds_exhaustive_effect_grade_fever_and_combo_plans() {
    let mut input = fixture();
    let mut skills = catalogue();
    let mut corrected = skills[5].clone();
    corrected.correction = Some(JudgementCorrection::Bad);
    skills.push(corrected);
    let table =
        TimingNetUpper::new_frontier_net(&input, &skills, cap(), WindowOptions::default()).unwrap();
    let raw = RawBounds::new(&input, &skills, WindowOptions::default());
    assert_eq!(table.stats().state_budget_rows, 0);
    let mut plans = 0;
    for skill in &skills {
        input.skills[0] = skill.clone();
        let domains = frame_domains(&input).unwrap();
        for &a in &domains[6] {
            for &b in &domains[7] {
                input.hit_frames[6] = a;
                input.hit_frames[7] = b;
                plans += usize::from(check_plan(&input, &table, &raw));
            }
        }
    }
    assert!(plans > 2000);
}

#[test]
fn delayed_and_chained_starts_accept_trigger_corrected_by_previous_skill() {
    let mut input = fixture();
    // All requests queue behind slot 0. In particular slot 1 is raw Bad, hit
    // while slot 0 is active, long before its own skill can start.
    for s in 0..6 {
        input.notes[s].time = 0.25 + s as f64 * 0.06;
        let frame = (input.notes[s].time * 120.).round() as u32;
        input.notes[s].window = NoteWindow::PinnedPerfect { frame };
        input.hit_frames[s] = frame;
    }
    input.notes[1].time = 0.18;
    input.notes[1].window = NoteWindow::Standard;
    input.hit_frames[1] = 36;
    let skill = ReplaySkill {
        duration: 0.5,
        score: ScoreEffect::Continuous {
            high: 140.,
            low: 60.,
        },
        correction: Some(JudgementCorrection::Bad),
    };
    input.skills = vec![skill.clone(); 6];
    let r = replay(&input).unwrap();
    let trigger = r.trace.iter().find(|n| n.note_index == 1).unwrap();
    assert_eq!(trigger.raw, Judgement::Bad);
    assert_eq!(trigger.judgement, Judgement::Perfect);
    assert_eq!(trigger.active_skill, Some(0));
    assert!(r.skill_events.iter().any(
        |e| matches!(e, SkillEvent::Start { index: 1, after_frame, .. } if *after_frame > 100)
    ));
    let table =
        TimingNetUpper::new_frontier_net(&input, &[skill.clone()], cap(), WindowOptions::default())
            .unwrap();
    let raw = RawBounds::new(&input, &[skill], WindowOptions::default());
    assert!(check_plan(&input, &table, &raw));
    assert_eq!(table.stats().state_budget_rows, 0);
}

#[test]
fn frontier_pool_and_linear_prefix_bound_enclose_rosters_and_unions() {
    let input = fixture();
    let mut skills = catalogue();
    skills.push(skills[0].clone()); // Catalogue indices must survive deduplication.
    let table =
        TimingNetUpper::new_frontier_net(&input, &skills, cap(), WindowOptions::default()).unwrap();
    assert_eq!(table.gains[0], table.gains[6]);
    assert_eq!(table.stats().profiles, 6);
    let pool = PoolTimingUpper::new_frontier_net(&input, &skills, cap(), WindowOptions::default())
        .unwrap();
    let power = ScoringPower::new(input.power).unwrap();
    let choices = std::array::from_fn(|i| pool.choice(i, f64::from(power) / 5.).unwrap());
    let upper = pool.upper(&choices).unwrap();
    let linear = pool.linearize(f64::from(power) * 1.01).unwrap();
    let weights = choices.map(|c| linear.weights(c));
    let mut best_linear = 0_f64;
    let mut trial = input.clone();
    for order in physical_layouts() {
        for captain in 0..5 {
            trial.skills = order.iter().map(|&i| skills[i].clone()).collect();
            trial.skills.push(trial.skills[captain].clone());
            assert!(replay(&trial).unwrap().total <= upper);
            let score = (0..5).fold(0_f64, |v, s| {
                (v + weights[order[s]][s][usize::from(s == captain)]).next_up()
            }) + linear.error();
            best_linear = best_linear.max(score);
        }
    }
    assert!(best_linear >= upper as f64);
    let union = choices.into_iter().reduce(PoolTimingChoice::union).unwrap();
    assert!(pool.upper(&[union; 5]).unwrap() >= upper);
    assert!(pool.upper(&[pool.choice(0, 200000.).unwrap(); 5]).is_err());
}

#[test]
fn frontier_budget_falls_back_to_upper_instead_of_partial_maximum() {
    let input = fixture();
    let skills = catalogue();
    let full =
        TimingNetUpper::new_frontier_net(&input, &skills, cap(), WindowOptions::default()).unwrap();
    let limited = TimingNetUpper::new_frontier_net(
        &input,
        &skills,
        cap(),
        WindowOptions {
            max_states: 1,
            max_graph_edges: 1,
            max_transitions: 1,
            ..WindowOptions::default()
        },
    )
    .unwrap();
    assert!(limited.stats().state_budget_rows > 0);
    let s = std::array::from_fn(|i| skills[i].clone());
    let p = ScoringPower::new(input.power).unwrap();
    assert!(limited.upper_sequence(&s, p).unwrap() >= full.upper_sequence(&s, p).unwrap());
    let raw = RawBounds::new(
        &input,
        &skills,
        WindowOptions {
            max_states: 1,
            max_graph_edges: 1,
            max_transitions: 1,
            ..WindowOptions::default()
        },
    );
    assert!(check_plan(&input, &limited, &raw));
}

#[test]
fn frontier_compression_handles_unequal_references_and_native_crescendo() {
    let mut input = fixture();
    // Consecutive P-only components cross a Combo boundary. Their fixed
    // references differ, but subtracting their sum still permits run merging.
    input.initial_combo = 48;
    input.notes[1].time = 0.75;
    input.notes[1].window = NoteWindow::PinnedPerfect { frame: 90 };
    input.hit_frames[1] = 90;
    input.notes[6].time = 0.49;
    input.notes[7].time = 0.51;
    for i in [6, 7] {
        let frame = 60 + (i - 6) as u32;
        input.notes[i].window = NoteWindow::PinnedPerfect { frame };
        input.hit_frames[i] = frame;
    }
    let skills = vec![ReplaySkill {
        duration: 0.5,
        score: ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 102,
        },
        correction: None,
    }];
    input.skills = vec![skills[0].clone(); 6];
    let fast =
        TimingNetUpper::new_frontier_net(&input, &skills, cap(), WindowOptions::default()).unwrap();
    let slow = TimingNetUpper::new_frontier_net(
        &input,
        &skills,
        cap(),
        WindowOptions {
            collapse_perfect_runs: false,
            prune_request_frames: false,
            ..WindowOptions::default()
        },
    )
    .unwrap();
    assert!(
        fast.stats()
            .frontier
            .as_ref()
            .unwrap()
            .windows
            .collapsed_notes
            > 0
    );
    let raw = RawBounds::new(&input, &skills, WindowOptions::default());
    let uncompressed = RawBounds::new(
        &input,
        &skills,
        WindowOptions {
            collapse_perfect_runs: false,
            prune_request_frames: false,
            ..WindowOptions::default()
        },
    );
    assert_ne!(raw.references[6], raw.references[7]);
    for s in 0..6 {
        let a = fast.gains[0][s];
        let b = slow.gains[0][s];
        assert!((a - b).abs() < 1e-10, "{s}: {a} != {b}");
        assert!(
            (raw.profiles[&skills[0].cache_key()][s]
                - uncompressed.profiles[&skills[0].cache_key()][s])
                .abs()
                < 1e-10
        );
    }
    assert!(check_plan(&input, &fast, &raw));
}

#[test]
fn mandatory_great_keeps_negative_net_contribution() {
    use crate::hit_timing::NoteConstraint;
    let mut input = fixture();
    input.fever_frames = None;
    input.notes[1].time = 0.75;
    input.notes[1].window = NoteWindow::PinnedPerfect { frame: 90 };
    input.hit_frames[1] = 90;
    input.notes[6].time = 0.56; // Forced Great during slot 0.
    input.hit_frames[6] = 60;
    input.notes[7].time = 0.57;
    input.notes[7].window = NoteWindow::PinnedPerfect { frame: 60 };
    input.hit_frames[7] = 60;
    input.constraints = vec![NoteConstraint {
        before: 6,
        after: 7,
        minimum_gap_frames: 0,
        require_dispatch_order: false,
        earliest_after: None,
    }];
    let zero = ReplaySkill {
        duration: 0.5,
        correction: None,
        score: ScoreEffect::PerfectOrZero { percent: 140. },
    };
    let normal = ReplaySkill {
        score: ScoreEffect::PerfectOnly { percent: 140. },
        ..zero.clone()
    };
    input.skills = vec![zero.clone(); 6];
    let r = replay(&input).unwrap();
    let n = r.trace.iter().find(|n| n.note_index == 6).unwrap();
    assert_eq!(n.judgement, Judgement::Great);
    assert_eq!(n.skill_multiplier, 0.);
    let base = f64::from(1_f32 + 22_f32 * 0.01_f32) / 8.
        * 3.
        * f64::from(n.judgement.multiplier())
        * f64::from(input.combo_multiplier(n.combo));
    let skills = [zero, normal];
    let table =
        TimingNetUpper::new_frontier_net(&input, &skills, cap(), WindowOptions::default()).unwrap();
    // Common per-note reference cancels: the zero-score skill must lose this
    // compulsory note's full base, rather than relaxing its negative term to 0.
    assert!((table.gains[1][0] - table.gains[0][0] - base).abs() < 1e-9);
    let raw = RawBounds::new(&input, &skills, WindowOptions::default());
    assert!(
        (raw.profiles[&skills[1].cache_key()][0] - raw.profiles[&skills[0].cache_key()][0] - base)
            .abs()
            < 1e-9
    );
    assert!(check_plan(&input, &table, &raw));
}

#[test]
fn boundary_great_deducts_loss_against_the_same_perfect_reference() {
    let mut input = fixture();
    input.initial_combo = 1000;
    input.fever_frames = None;
    let skill = ReplaySkill {
        duration: 0.25,
        score: ScoreEffect::Plain { percent: 100. },
        correction: None,
    };
    input.skills = vec![skill.clone(); 6];
    input.notes[1].time = 0.75;
    input.notes[1].window = NoteWindow::PinnedPerfect { frame: 90 };
    input.hit_frames[1] = 90;
    let end = 30 + countdown(skill.duration) + 1;
    input.notes[6].time = f64::from(end) / 120. + 0.06;
    input.hit_frames[6] = (input.notes[6].time * 120.).round() as u32;
    input.notes[7].time = 800. / 120.;
    input.notes[7].window = NoteWindow::PinnedPerfect { frame: 800 };
    input.hit_frames[7] = 800;
    let skills = [skill];
    let table =
        TimingNetUpper::new_frontier_net(&input, &skills, cap(), WindowOptions::default()).unwrap();
    let raw = RawBounds::new(&input, &skills, WindowOptions::default());
    let scale =
        f64::from(1_f32 + 22_f32 * 0.01_f32) / 8. * 3. * f64::from(input.combo_multiplier(1001));
    let mut best = 0_f64;
    let mut best_grade = None;
    let mut outside_perfect = false;
    for &frame in &frame_domains(&input).unwrap()[6] {
        input.hit_frames[6] = frame;
        let r = replay(&input).unwrap();
        let note = r.trace.iter().find(|n| n.note_index == 6).unwrap();
        outside_perfect |= note.judgement == Judgement::Perfect && note.active_skill.is_none();
        if note.active_skill == Some(0) {
            let net =
                scale * f64::from(note.judgement.multiplier()) * f64::from(note.skill_multiplier)
                    - raw.references[6];
            if net > best {
                best = net;
                best_grade = Some(note.judgement);
            }
        }
        assert!(check_plan(&input, &table, &raw));
    }
    assert!(outside_perfect);
    assert_eq!(best_grade, Some(Judgement::Great));
    let upper = raw.profiles[&skills[0].cache_key()][0];
    assert!((upper - best).abs() < 1e-9);
    // Incorrect owner split would keep the full Perfect base and only add
    // Great's skill increment. The fixed-reference solver pays the P->G loss.
    let loose_increment = scale * f64::from(Judgement::Great.multiplier());
    let great_loss = raw.references[6] - scale * f64::from(Judgement::Great.multiplier());
    assert!((loose_increment - upper - great_loss).abs() < 1e-9);
    assert!(great_loss > 0.1);
}

#[test]
fn published_bounds_keep_old_baseline_and_never_loosen_it() {
    let input = fixture();
    let skills = catalogue();
    let old = TimingNetUpper::new_stateful(&input, &skills, cap()).unwrap();
    let new =
        TimingNetUpper::new_frontier_net(&input, &skills, cap(), WindowOptions::default()).unwrap();
    assert_eq!(old.baseline.to_bits(), new.baseline.to_bits());
    assert_eq!(old.error.to_bits(), new.error.to_bits());
    for (k, skill) in skills.iter().enumerate() {
        for s in 0..6 {
            assert!(new.gains[k][s] <= old.gains[k][s]);
        }
        let sequence = std::array::from_fn(|_| skill.clone());
        for power in [0., 1., input.power, 600000.] {
            let power = ScoringPower::new(power).unwrap();
            assert!(
                new.upper_sequence(&sequence, power).unwrap()
                    <= old.upper_sequence(&sequence, power).unwrap()
            );
        }
    }
}
