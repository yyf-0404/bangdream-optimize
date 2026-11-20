use super::*;
use crate::hit_timing::{search, NativeTimeRouting, NativeTimeTarget, ReplayNote};

mod composition;
mod progress_dominance;
mod prepared;
mod local_geometry;
mod outside_pins;
mod temporal;
mod touch_dag;
mod incremental;
mod buckets;
mod clock_aliases;
mod frame_templates;
mod compact_geometry;
mod packed_value;
mod value_arena;
mod queued_uniform;
mod clock_cells;
mod partial_temporal;
mod window_pareto;
mod idle_temporal;
mod frame_ranges;

fn skill(score: super::super::super::ScoreEffect) -> ReplaySkill {
    ReplaySkill {
        duration: 0.1,
        score,
        correction: None,
    }
}
pub(super) fn input(times: &[f64], skills: Vec<ReplaySkill>, radius: u32) -> ReplayInput {
    let frames: Vec<_> = times.iter().map(|t| (t * 120.).round() as u32).collect();
    let last = frames.iter().max().unwrap() + radius + 1;
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        initial_combo: 18,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        notes: times
            .iter()
            .enumerate()
            .map(|(i, &time)| ReplayNote {
                time,
                trigger: (i < skills.len()).then_some(i),
                window: NoteWindow::Standard,
            })
            .collect(),
        hit_order: vec![],
        native_time_routing: Some(NativeTimeRouting {
            distance_mode: Default::default(),
            music_positions: (0..=last).map(|f| f as f32 * 10.).collect(),
            targets: frames
                .iter()
                .enumerate()
                .map(|(i, &f)| {
                    Some(NativeTimeTarget {
                        button: i as u8,
                        absolute_pos: (times[i] * 1200.) as i32,
                        active_from_frame: f.saturating_sub(radius),
                        active_until_frame: f + radius,
                    })
                })
                .collect(),
            candidate_order: (0..times.len()).collect(),
            connections: vec![],
            event_order: vec![],
        }),
        hit_frames: frames,
        skills,
    }
}

fn brute(request: &ReplayInput) -> u64 {
    fn visit(i: usize, input: &mut ReplayInput, domains: &[Vec<u32>], best: &mut u64) {
        if i == domains.len() {
            if let Some(r) = crate::hit_timing::touch_order_tests::best_replay(input) {
                *best = (*best).max(r.total);
            }
        } else {
            for &f in &domains[i] {
                input.hit_frames[i] = f;
                visit(i + 1, input, domains, best);
            }
        }
    }
    let mut best = 0;
    visit(
        0,
        &mut request.clone(),
        &search::frame_domains(request).unwrap(),
        &mut best,
    );
    best
}

pub(super) fn profiles() -> Vec<ReplaySkill> {
    use crate::hit_timing::ScoreEffect::*;
    let mut skills: Vec<_> = [
        Plain { percent: 100. },
        PerfectOnly { percent: 120. },
        PerfectOrZero { percent: 125. },
        PerfectOrHalf { percent: 125. },
        Continuous {
            high: 140.,
            low: 60.,
        },
        Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 150,
        },
    ]
    .into_iter()
    .map(skill)
    .collect();
    for correction in [
        JudgementCorrection::Great,
        JudgementCorrection::Good,
        JudgementCorrection::Bad,
    ] {
        skills.push(ReplaySkill {
            correction: Some(correction),
            ..skills[0].clone()
        });
    }
    skills
}

#[test]
fn jointly_moving_two_triggers_and_ordinary_notes_matches_exhaustive() {
    let profiles = profiles();
    for (i, a) in profiles.iter().enumerate() {
        let mut r = input(
            &[0.1, 0.125, 0.215, 0.99],
            vec![a.clone(), profiles[(i + 4) % profiles.len()].clone()],
            3,
        );
        r.fever_frames = Some([25, 120]);
        r.medley_combo = i % 2 == 0;
        let model = JointTimeline::new(&r).unwrap();
        for p in [2., 300040.75] {
            r.power = p;
            let expected = brute(&r);
            let power = ScoringPower::new(p).unwrap();
            assert_eq!(
                model.maximum(power).unwrap().result.total,
                expected,
                "{i}, {p}"
            );
            assert_eq!(model.score(power).unwrap(), expected);
        }
    }
}

#[test]
fn three_queued_triggers_keep_all_future_windows_and_effect_histories() {
    let mut r = input(
        &[0.1, 0.15, 0.2, 0.995, 1.9],
        vec![profiles()[5].clone(); 3],
        2,
    );
    r.skills[0].correction = Some(JudgementCorrection::Bad);
    r.skills[1].score = crate::hit_timing::ScoreEffect::Continuous {
        high: 150.,
        low: 50.,
    };
    r.initial_combo = 48;
    r.fever_frames = Some([118, 230]);
    let model = JointTimeline::new(&r).unwrap();
    assert_eq!(
        model
            .maximum(ScoringPower::new(r.power).unwrap())
            .unwrap()
            .result
            .total,
        brute(&r)
    );
}

#[test]
fn countdown_and_cooling_boundaries_match_native_replay() {
    for duration in [0.1, 3., 5., 7.5] {
        let first = 12;
        let ready = first + countdown(duration) + 1 + countdown(0.75) + 2;
        let mut r = input(
            &[0.1, ready as f64 / 120., (ready + 2) as f64 / 120.],
            vec![profiles()[0].clone(); 2],
            2,
        );
        r.skills[0].duration = duration;
        let model = JointTimeline::new(&r).unwrap();
        assert_eq!(
            model
                .maximum(ScoringPower::new(r.power).unwrap())
                .unwrap()
                .result
                .total,
            brute(&r)
        );
    }
}

#[test]
fn queued_bad_trigger_requires_an_already_active_correcting_skill() {
    let mut r = input(
        &[0., 0.025, 0.2],
        vec![profiles()[8].clone(), profiles()[0].clone()],
        2,
    );
    r.skills[0].duration = 0.3;
    r.hit_frames[1] = 15;
    let route = r.native_time_routing.as_mut().unwrap();
    route.targets[1].as_mut().unwrap().active_from_frame = 13;
    route.targets[1].as_mut().unwrap().active_until_frame = 17;
    replay(&r).unwrap();
    let model = JointTimeline::new(&r).unwrap();
    assert_eq!(
        model
            .maximum(ScoringPower::new(r.power).unwrap())
            .unwrap()
            .result
            .total,
        brute(&r)
    );
}

#[test]
fn native_bindings_and_same_frame_dispatch_survive_joint_state_merging() {
    let mut r = crate::hit_timing::time_connections::request();
    r.notes[0].trigger = Some(0);
    r.skills = vec![profiles()[4].clone()];
    r.native_time_routing.as_mut().unwrap().event_order = vec![0, 2, 1];
    let model = JointTimeline::new(&r).unwrap();
    assert_eq!(
        model
            .maximum(ScoringPower::new(r.power).unwrap())
            .unwrap()
            .result
            .total,
        brute(&r)
    );
}

#[test]
fn complete_cached_scoring_and_order_batches_reoptimize_at_actual_power() {
    use crate::hit_timing::compiled::*;
    let r = input(
        &[0.1, 0.125, 0.22, 0.99],
        vec![profiles()[5].clone(), profiles()[4].clone()],
        2,
    );
    let mut cache = TimingCompileCache::new(&r, search::SearchOptions::default());
    let a = cache.prepare(&r.skills).unwrap();
    let b = cache.prepare(&r.skills).unwrap();
    assert!(a.stats().complete);
    assert_eq!(cache.stats().group_builds, 1);
    assert_eq!(cache.stats().group_hits, 1);
    let orders = CompiledTimingOrders::new(vec![a, b]).unwrap();
    assert!(orders.stats().complete);
    for p in [2., 300040.75] {
        let mut query = r.clone();
        query.power = p;
        let expected = brute(&query);
        let mut scratch = TimingOrderScratch::default();
        assert_eq!(
            orders
                .score(ScoringPower::new(p).unwrap(), &mut scratch)
                .unwrap(),
            [expected, expected]
        );
        assert_eq!(
            orders
                .materialize(1, ScoringPower::new(p).unwrap())
                .unwrap()
                .total,
            expected
        );
    }
}

#[test]
fn existing_adjacent_pruning_bounds_enclose_complete_six_trigger_optima() {
    use crate::hit_timing::bounds::{TimingNetUpper, TimingScoreUpper, TimingUpperScratch};
    for correcting in [false, true] {
        let mut skills = vec![profiles()[5].clone(); 6];
        skills[2] = profiles()[4].clone();
        if correcting {
            skills[0].correction = Some(JudgementCorrection::Bad);
        }
        let mut r = input(&[0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 4.5], skills, 1);
        r.fever_frames = Some([15, 541]);
        let max_power = ScoringPower::new(700000.5).unwrap();
        let net =
            TimingNetUpper::new_adjacent_net(&r, &r.skills, max_power, WindowOptions::default())
                .unwrap();
        let upper = TimingScoreUpper::new(&r).unwrap();
        let exact = JointTimeline::new(&r).unwrap();
        for p in [2., 300040.75] {
            r.power = p;
            let power = ScoringPower::new(p).unwrap();
            let score = exact.maximum(power).unwrap().result.total;
            assert_eq!(score, brute(&r));
            assert!(
                net.upper_sequence(&r.skills.clone().try_into().unwrap(), power)
                    .unwrap()
                    >= score
            );
            assert!(
                upper
                    .score(power, &mut TimingUpperScratch::default())
                    .unwrap()
                    >= score
            );
        }
    }
}

#[test]
fn cached_geometry_expands_and_contracts_correction_domains_without_reusing_winners() {
    use crate::hit_timing::compiled::{TimingCompileCache, TimingScoreScratch};
    let mut r = input(&[0.1, 0.125, 0.42], vec![profiles()[0].clone(); 2], 1);
    r.skills[0].duration = 0.3;
    let target = r.native_time_routing.as_mut().unwrap().targets[2]
        .as_mut()
        .unwrap();
    target.active_from_frame = 26;
    target.active_until_frame = 52;
    let mut cache = TimingCompileCache::new(&r, search::SearchOptions::default());
    for (i, correction) in [
        None,
        Some(JudgementCorrection::Bad),
        Some(JudgementCorrection::Good),
        None,
        Some(JudgementCorrection::Great),
    ]
    .into_iter()
    .enumerate()
    {
        r.skills[0].correction = correction;
        r.skills[1].duration += 0.01;
        r.skills[1].score = profiles()[i].score.clone();
        let bank = cache.prepare(&r.skills).unwrap();
        for p in [2., 300040.75] {
            r.power = p;
            let power = ScoringPower::new(p).unwrap();
            let expected = brute(&r);
            assert_eq!(
                bank.score(power, &mut TimingScoreScratch::default())
                    .unwrap(),
                expected
            );
            assert_eq!(bank.materialize(power).unwrap().total, expected);
        }
    }
}

#[test]
fn cached_model_eviction_never_invalidates_prepared_banks_or_limits_search() {
    use crate::hit_timing::compiled::{TimingCompileCache, TimingScoreScratch};
    use crate::hit_timing::ScoreEffect;
    let r = input(&[0.1, 0.21], vec![profiles()[0].clone()], 1);
    let power = ScoringPower::new(r.power).unwrap();
    let mut cache = TimingCompileCache::new(&r, search::SearchOptions::default());
    let retained = cache.prepare(&r.skills).unwrap();
    for percent in 101..=165 {
        let mut q = r.clone();
        q.skills[0].score = ScoreEffect::Plain {
            percent: percent as f32,
        };
        let bank = cache.prepare(&q.skills).unwrap();
        assert_eq!(
            bank.score(power, &mut TimingScoreScratch::default())
                .unwrap(),
            brute(&q)
        );
    }
    assert_eq!(cache.footprint().groups, 64);
    assert_eq!(retained.materialize(power).unwrap().total, brute(&r));
}

#[test]
fn changing_baseline_order_cannot_remove_same_frame_trigger_schedules() {
    let mut r = input(
        &[0.1, 0.125, 0.215, 0.99],
        vec![profiles()[5].clone(), profiles()[4].clone()],
        3,
    );
    let power = ScoringPower::new(r.power).unwrap();
    let expected = brute(&r);
    let first = JointTimeline::new(&r).unwrap().score(power).unwrap();
    r.native_time_routing.as_mut().unwrap().event_order = vec![1, 0, 3, 2];
    let second = JointTimeline::new(&r)
        .unwrap()
        .maximum(power)
        .unwrap()
        .result;
    assert_eq!(first, expected);
    assert_eq!(second.total, expected);
    assert_eq!(second.total, brute(&r));
    r.hit_frames = second.hit_frames;
    r.hit_order = second.hit_order;
    assert_eq!(replay(&r).unwrap().total, expected);
}
