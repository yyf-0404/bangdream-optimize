use super::*;
use crate::hit_timing::{CandidateDistanceMode, JudgementCorrection, NativeTimeRouting, NativeTimeTarget, NoteWindow, ReplayNote, ReplaySkill, ScoreEffect};

mod exact_score_reuse;

#[test]
fn ranges_cache_reuses_preparation_and_matches_fresh_restricted_models() {
    let request=input(&[1.,1.025,1.05,2.],vec![skill(ScoreEffect::Plain{percent:100.})]);
    let limits=vec![Some([120,120]),None,Some([124,126]),None];
    let power=ScoringPower::new(request.power).unwrap();
    let mut cache=TimingCompileCache::new(&request,SearchOptions::default()).with_frame_ranges(limits.clone());
    let mut scratch=TimingScoreScratch::default();
    for percent in [60.,100.,140.,90.] {
        let skills=vec![skill(ScoreEffect::Plain{percent})];
        let bank=cache.prepare(&skills).unwrap();
        let mut fresh=request.clone();fresh.skills=skills;
        let expected=super::super::independent::JointTimeline::maximum_in_frame_ranges(&fresh,&limits).unwrap().unwrap();
        assert_eq!(bank.score_optional(power,&mut scratch).unwrap(),Some(expected.result.total));
        let result=bank.materialize(power).unwrap();
        assert_eq!(result.total,expected.result.total);
        assert_eq!(result.hit_frames[0],120);
    }
    assert_eq!(cache.preparation_stats().chart_preparations,1);
    assert!(cache.preparation_stats().geometry_reuses>0);
    let mut separate=TimingCompileCache::new(&request,SearchOptions::default())
        .with_frame_ranges(vec![Some([121,121]),None,None,None]);
    assert_eq!(separate.prepare(&request.skills).unwrap().materialize(power).unwrap().hit_frames[0],121);
}

#[test]
fn ranges_cache_new_owner_does_not_reuse_an_empty_previous_domain() {
    let request=input(&[1.,1.1],vec![]);
    let power=ScoringPower::new(request.power).unwrap();
    let mut cache=TimingCompileCache::new(&request,SearchOptions::default())
        .with_frame_ranges(vec![Some([0,0]),None]);
    assert_eq!(cache.prepare(&[]).unwrap().score_optional(power,&mut TimingScoreScratch::default()).unwrap(),None);
    let mut changed=cache.with_frame_ranges(vec![Some([120,120]),None]);
    assert_eq!(changed.prepare(&[]).unwrap().materialize(power).unwrap().hit_frames[0],120);
}

fn input(times: &[f64], skills: Vec<ReplaySkill>) -> ReplayInput {
    ReplayInput {
        power: 466061.125,
        play_level: 27,
        fps: 120,
        notes: times
            .iter()
            .enumerate()
            .map(|(i, &time)| ReplayNote {
                time,
                trigger: (i < skills.len()).then_some(i),
                window: NoteWindow::Standard,
            })
            .collect(),
        hit_frames: times.iter().map(|t| (t * 120.).round() as u32).collect(),
        skills,
        initial_combo: 0,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    }
}
fn skill(score: ScoreEffect) -> ReplaySkill {
    ReplaySkill {
        duration: 3.,
        score,
        correction: None,
    }
}

#[test]
fn chart_time_partition_retains_active_same_lane_competition() {
    let mut request = input(&[1., 1.05], vec![]);
    request.native_time_routing = Some(NativeTimeRouting {
        distance_mode: CandidateDistanceMode::ChartTime,
        music_positions: vec![],
        targets: vec![
            Some(NativeTimeTarget { button: 2, absolute_pos: 0, active_from_frame: 106, active_until_frame: 135 }),
            Some(NativeTimeTarget { button: 2, absolute_pos: 0, active_from_frame: 112, active_until_frame: 141 }),
        ],
        candidate_order: vec![0, 1],
        connections: vec![],
        event_order: vec![],
    });
    // A skill may restrict legal frames without removing the other candidate
    // from the button's raw Bad lifetime.
    assert_eq!(partitions(&request, &[vec![118], vec![128]]), vec![vec![0, 1]]);
    request.native_time_routing.as_mut().unwrap().targets[1]
        .as_mut().unwrap().button = 3;
    assert_eq!(partitions(&request, &[vec![118], vec![128]]), vec![vec![0], vec![1]]);
}

#[test]
fn compiled_and_cached_candidates_include_complete_replacement_witnesses() {
    let r = input(
        &[1., 1.06, 4.06, 4.08, 5.],
        vec![skill(ScoreEffect::Crescendo {
            percent: 95.,
            step: 0.5,
            cap: 150,
        })],
    );
    let witnesses = super::super::witness::perfect(&r, &search::frame_domains(&r).unwrap());
    assert!(witnesses.len() > 1);
    let options = SearchOptions {
        max_evaluations: 1,
        max_rounds: 1,
        max_refine_passes: 1,
        ..SearchOptions::default()
    };
    let direct = compile(&r, options).unwrap().candidates;
    let mut cache = TimingCompileCache::new(&r, options);
    for cached in [
        cache.prepare(&r.skills).unwrap(),
        cache.prepare(&r.skills).unwrap(),
    ] {
        for power in [200_031.25, 466_061.125] {
            let expected = witnesses
                .iter()
                .map(|w| {
                    let mut q = r.clone();
                    q.power = power;
                    q.hit_frames = w.hit_frames.clone();
                    replay(&q).unwrap().total
                })
                .max()
                .unwrap();
            for bank in [&direct, &cached] {
                assert!(
                    bank.materialize(ScoringPower::new(power).unwrap())
                        .unwrap()
                        .total
                        >= expected
                );
            }
        }
    }
}

#[test]
fn candidates_remain_exact_at_new_powers_and_small_exhaustive_cases() {
    for effect in [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 120. },
        ScoreEffect::PerfectOrZero { percent: 125. },
        ScoreEffect::PerfectOrHalf { percent: 125. },
        ScoreEffect::Continuous {
            high: 110.,
            low: 90.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 0.5,
            cap: 150,
        },
    ] {
        let r = input(&[1., 1.015, 4.08], vec![skill(effect)]);
        let compiled = compile(&r, SearchOptions::default()).unwrap();
        for power in [0., 0.125, 100., 300040.75, 466061.125, 999999.5] {
            let mut q = r.clone();
            q.power = power;
            let expected = search::optimize(&q, SearchOptions::default()).unwrap();
            let actual = compiled
                .candidates
                .materialize(ScoringPower::new(power).unwrap())
                .unwrap();
            assert!(
                actual.total >= expected.result.total,
                "{effect:?}, power {power}, {} < {}",
                actual.total,
                expected.result.total
            );
        }
    }
}

#[test]
fn queue_dependencies_and_stateful_windows_compose_legally() {
    let mut s = skill(ScoreEffect::Crescendo {
        percent: 100.,
        step: 0.5,
        cap: 150,
    });
    s.correction = Some(JudgementCorrection::Bad);
    let mut r = input(
        &[0., 1., 10., 0.025, 3.04, 3.79, 6.82, 10.025, 13.08, 20.],
        vec![s.clone(), s.clone(), s],
    );
    r.initial_combo = 699;
    r.medley_combo = true;
    r.fever_frames = Some([450, 900]);
    let model = compile(
        &r,
        SearchOptions {
            max_evaluations: 10000,
            ..SearchOptions::default()
        },
    )
    .unwrap();
    let mut scratch = TimingScoreScratch::default();
    for power in [0., 1., 300040.0, 300040.75, 500000.125, 900000.] {
        let p = ScoringPower::new(power).unwrap();
        let actual = model.candidates.materialize(p).unwrap();
        assert_eq!(
            actual.total,
            model.candidates.score(p, &mut scratch).unwrap()
        );
        for n in &actual.trace {
            assert_eq!(
                r.combo_multiplier(n.combo),
                crate::model::chart::get_combo_mod(n.combo as i32, true) as f32
            );
        }
    }
}

#[test]
fn native_input_keeps_whole_plan_and_explicit_constraints_survive() {
    let r = crate::hit_timing::time_connections::request();
    let model = compile(&r, SearchOptions::default()).unwrap();
    assert_eq!(model.candidates.stats().independent_groups, 1);
    for power in [1., 300040.75, 500000.25] {
        model
            .candidates
            .materialize(ScoringPower::new(power).unwrap())
            .unwrap();
    }
    let mut r = input(
        &[1., 1.02, 4.08],
        vec![skill(ScoreEffect::Plain { percent: 100. })],
    );
    r.constraints.push(crate::hit_timing::NoteConstraint {
        before: 0,
        after: 1,
        minimum_gap_frames: 1,
        require_dispatch_order: false,
        earliest_after: None,
    });
    let model = compile(&r, SearchOptions::default()).unwrap();
    let result = model
        .candidates
        .materialize(ScoringPower::new(17.25).unwrap())
        .unwrap();
    assert!(result.hit_frames[0] < result.hit_frames[1]);
}

#[test]
fn dominance_is_monotone_across_rounding_and_operand_changes() {
    let mut seed = 944u32;
    for _ in 0..1000 {
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seed
        };
        let b = [
            0.8_f32.to_bits(),
            (1.0 + (next() % 10) as f32 * 0.01).to_bits(),
            (1.0 + (next() % 100) as f32 * 0.01).to_bits(),
        ];
        let a = [
            1.1_f32.to_bits(),
            (f32::from_bits(b[1]) + 0.01).to_bits(),
            (f32::from_bits(b[2]) + 0.5).to_bits(),
        ];
        assert!(dominates(&[a], &[b]));
        for value in [0., 0.00001, 100., 300040.75, 524288., 999999.5] {
            let s = NoteScorer::new(ScoringPower::new(value).unwrap(), 27, 760).unwrap();
            let value = |t: Term| {
                s.score_with_judgement_multiplier(
                    f32::from_bits(t[0]),
                    f32::from_bits(t[1]),
                    f32::from_bits(t[2]),
                    1.,
                )
                .unwrap()
                .final_score
            };
            assert!(value(a) >= value(b));
        }
    }
}

#[test]
fn component_cache_reuses_unrelated_skills_and_invalidates_changed_domains() {
    let r = input(
        &[1., 10., 1.025, 4.08, 10.025, 13.08, 20.],
        vec![skill(ScoreEffect::Plain { percent: 100. }); 2],
    );
    let mut cache = TimingCompileCache::new(&r, SearchOptions::default());
    cache.prepare(&r.skills).unwrap();
    let first_builds = cache.stats().group_builds;
    cache.prepare(&r.skills).unwrap();
    assert_eq!(cache.stats().group_builds, first_builds);
    assert!(cache.stats().group_hits > 0);
    let mut changed = r.skills.clone();
    changed[1].score = ScoreEffect::PerfectOnly { percent: 150. };
    let model = cache.prepare(&changed).unwrap();
    assert_eq!(cache.stats().group_builds, first_builds + 1);
    for power in [1., 300040.75, 700000.5] {
        let mut q = r.clone();
        q.skills = changed.clone();
        q.power = power;
        assert!(
            model
                .materialize(ScoringPower::new(power).unwrap())
                .unwrap()
                .total
                >= search::optimize(&q, SearchOptions::default())
                    .unwrap()
                    .result
                    .total
        );
    }
    changed[1].correction = Some(JudgementCorrection::Bad);
    cache
        .prepare(&changed)
        .unwrap()
        .materialize(ScoringPower::new(300040.75).unwrap())
        .unwrap();
    assert!(cache.stats().group_builds > first_builds + 1);
}

#[test]
fn cache_does_not_reuse_independent_groups_when_duration_creates_queue() {
    let r = input(
        &[0., 4., 0.025, 3.08, 4.025, 7.08, 20.],
        vec![
            skill(ScoreEffect::Crescendo {
                percent: 100.,
                step: 0.5,
                cap: 150
            });
            2
        ],
    );
    let mut cache = TimingCompileCache::new(&r, SearchOptions::default());
    cache.prepare(&r.skills).unwrap();
    let old = cache.stats().group_builds;
    let mut changed = r.skills.clone();
    changed[0].duration = 5.;
    let model = cache.prepare(&changed).unwrap();
    assert!(cache.stats().group_builds > old);
    for power in [300040.75, 600000.125] {
        model
            .materialize(ScoringPower::new(power).unwrap())
            .unwrap();
    }
}

#[test]
fn a_new_power_can_select_a_different_plan_after_both_integer_casts() {
    let mut r = input(
        &[0., 0.19],
        vec![ReplaySkill {
            duration: 0.1,
            score: ScoreEffect::Plain { percent: 50. },
            correction: None,
        }],
    );
    r.play_level = 5;
    r.notes[0].window = NoteWindow::PinnedPerfect { frame: 0 };
    let mut recorder = Recorder::new(&r, &[vec![0], vec![13, 23]]);
    for frame in [13, 23] {
        r.hit_frames = vec![0, frame];
        let replay = replay(&r).unwrap();
        let mut terms = vec![[0; 3]; 2];
        for t in replay.trace {
            terms[t.note_index] = [
                t.judgement.multiplier().to_bits(),
                r.combo_multiplier(t.combo).to_bits(),
                (t.fever_multiplier * t.skill_multiplier).to_bits(),
            ];
        }
        recorder.record(&r, &r.hit_frames, &terms);
    }
    let model = recorder.finish(&r).unwrap();
    assert_eq!(model.stats().variants, 2);
    let low = model.materialize(ScoringPower::new(0.75).unwrap()).unwrap();
    let high = model.materialize(ScoringPower::new(100.).unwrap()).unwrap();
    assert_eq!(low.hit_frames[1], 23);
    assert_eq!(high.hit_frames[1], 13);
}

#[test]
fn orders_share_arithmetic_without_changing_any_score_or_witness() {
    let r = input(
        &[1., 10., 1.025, 4.08, 10.025, 13.08, 20.],
        vec![
            skill(ScoreEffect::Plain { percent: 100. }),
            skill(ScoreEffect::Continuous {
                high: 120.,
                low: 90.,
            }),
        ],
    );
    // Arithmetic interning belongs to the retained finite-family audit path.
    let mut cache = TimingCompileCache::new(
        &r,
        SearchOptions {
            complete_search: false,
            ..SearchOptions::default()
        },
    );
    let mut reverse = r.skills.clone();
    reverse.reverse();
    let banks = vec![
        cache.prepare(&r.skills).unwrap(),
        cache.prepare(&reverse).unwrap(),
        cache.prepare(&r.skills).unwrap(),
    ];
    let powers = [0., 0.125, 1., 300040.75, 466061.125, 999999.5];
    let expected: Vec<Vec<_>> = powers
        .iter()
        .map(|&p| {
            banks
                .iter()
                .map(|b| {
                    b.score(
                        ScoringPower::new(p).unwrap(),
                        &mut TimingScoreScratch::default(),
                    )
                    .unwrap()
                })
                .collect()
        })
        .collect();
    let model = CompiledTimingOrders::new(banks).unwrap();
    assert!(model.stats().unique_score_terms < model.stats().separate_score_terms);
    let mut scratch = TimingOrderScratch::default();
    for (p, expected) in powers.into_iter().zip(expected) {
        let p = ScoringPower::new(p).unwrap();
        assert_eq!(model.score(p, &mut scratch).unwrap(), expected);
        for (i, &score) in expected.iter().enumerate() {
            assert_eq!(model.materialize(i, p).unwrap().total, score);
        }
    }
    assert!(CompiledTimingOrders::new(vec![]).is_err());
    assert!(model
        .materialize(3, ScoringPower::new(1.).unwrap())
        .is_err());
}

#[test]
fn exact_cache_reuses_matching_correction_geometry_and_counts_each_graph_once() {
    let r = input(
        &[1., 1.05, 4.1, 10.],
        vec![skill(ScoreEffect::Plain { percent: 100. })],
    );
    let mut cache = TimingCompileCache::new(&r, SearchOptions::default());
    for k in 0..12 {
        let mut skills = r.skills.clone();
        skills[0].score = ScoreEffect::Plain {
            percent: 100. + k as f32,
        };
        skills[0].correction = if k % 2 == 0 {
            None
        } else {
            Some(JudgementCorrection::Bad)
        };
        let bank = cache.prepare(&skills).unwrap();
        let mut direct = r.clone();
        direct.skills = skills;
        let expected = JointTimeline::new(&direct)
            .unwrap()
            .score(ScoringPower::new(r.power).unwrap())
            .unwrap();
        assert_eq!(
            bank.score(
                ScoringPower::new(r.power).unwrap(),
                &mut TimingScoreScratch::default()
            )
            .unwrap(),
            expected
        );
    }
    let ids: std::collections::HashSet<_> = cache
        .exact
        .values()
        .map(|m| m.geometry_identity())
        .collect();
    assert_eq!(ids.len(), 2);
    let bytes = ids
        .iter()
        .map(|&id| {
            cache
                .exact
                .values()
                .find(|m| m.geometry_identity() == id)
                .unwrap()
                .footprint()
        })
        .sum::<usize>();
    assert_eq!(cache.footprint().payload_bytes, bytes);
    let mut skills = r.skills.clone();
    skills[0].correction = Some(JudgementCorrection::Bad);
    skills[0].duration = 4.;
    cache.prepare(&skills).unwrap();
    assert_eq!(
        cache
            .exact
            .values()
            .map(|m| m.geometry_identity())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
    assert!(cache.prepare(&[]).is_err());
}

#[test]
fn geometry_byte_budget_evicts_lru_models_without_invalidating_returned_banks() {
    let r = input(&[1., 1.05, 4.1, 10.], vec![skill(ScoreEffect::Plain { percent: 100. })]);
    let mut cache = TimingCompileCache::new(&r, SearchOptions::default());
    let first = cache.prepare(&r.skills).unwrap();
    cache.max_geometry_bytes = first.exact.as_ref().unwrap().footprint();
    let mut short = r.skills.clone();
    short[0].duration = 1.;
    let second = cache.prepare(&short).unwrap();
    assert!(cache.exact_geometry_bytes() <= cache.max_geometry_bytes);
    assert!(!cache.exact.contains_key(&r.skills.iter().map(ReplaySkill::cache_key).collect::<Vec<_>>()));
    for (bank, skills) in [(&first, &r.skills), (&second, &short)] {
        let mut request = r.clone();
        request.skills = skills.clone();
        let power = ScoringPower::new(r.power).unwrap();
        assert_eq!(bank.score(power, &mut TimingScoreScratch::default()).unwrap(),
            JointTimeline::new(&request).unwrap().score(power).unwrap());
    }
    // An oversized graph is returned complete and left uncached. Even zero
    // cache bytes cannot turn a score into a truncated or approximate result.
    cache.max_geometry_bytes = 0;
    let third = cache.prepare(&r.skills).unwrap();
    assert!(cache.exact.is_empty() && cache.exact_stamps.is_empty());
    let power = ScoringPower::new(r.power).unwrap();
    assert_eq!(third.score(power, &mut TimingScoreScratch::default()).unwrap(),
        first.score(power, &mut TimingScoreScratch::default()).unwrap());
}

#[test]
fn outside_proofs_survive_skill_boundary_changes_and_model_eviction() {
    let r = input(&[1., 1.05, 4.1, 10.], vec![skill(ScoreEffect::Plain { percent: 100. })]);
    let mut cache = TimingCompileCache::new(&r, SearchOptions::default());
    // Keep no complete graph: the chart data and independently owned outside
    // proofs must still survive and must never retain an old score or FIFO DP.
    cache.max_geometry_bytes = 0;
    let mut held = vec![];
    for duration in [3., 1., 4., 1.] {
        let mut skills = r.skills.clone();
        skills[0].duration = duration;
        skills[0].correction = if duration == 4. { Some(JudgementCorrection::Bad) } else { None };
        let bank = cache.prepare(&skills).unwrap();
        let mut direct = r.clone();
        direct.skills = skills;
        for p in [2., 300040.75, f32::from_bits(300040.75_f32.to_bits() + 1)] {
            let power = ScoringPower::new(p).unwrap();
            let expected = JointTimeline::new(&direct).unwrap().score(power).unwrap();
            assert_eq!(bank.score(power, &mut TimingScoreScratch::default()).unwrap(), expected);
            assert_eq!(bank.materialize(power).unwrap().total, expected);
        }
        held.push(bank);
        assert!(cache.exact.is_empty());
    }
    let stats = cache.preparation_stats();
    assert_eq!(stats.chart_preparations, 1);
    assert_eq!(stats.bad_bounds_preparations, 1);
    assert_eq!(stats.protected_scans, 4);
    assert_eq!(stats.protected_note_visits, 4 * r.notes.len() as u64);
    assert_eq!(stats.geometry_builds, 4);
    assert!(stats.outside_cache_hits > 0 && stats.outside_proofs > 0);
    assert!(stats.outside_cache_payload_bytes <= stats.outside_cache_budget_bytes);
    // Clearing every cached graph has not invalidated the caller's first bank.
    assert_eq!(held[0].materialize(ScoringPower::new(r.power).unwrap()).unwrap().total,
        JointTimeline::new(&r).unwrap().score(ScoringPower::new(r.power).unwrap()).unwrap());
}

#[test]
fn geometry_owners_survive_resolved_binding_eviction_and_keep_both_byte_caps() {
    let r = input(&[1., 1.05, 4.1, 10.], vec![skill(ScoreEffect::Plain { percent: 100. })]);
    let mut cache = TimingCompileCache::new(&r, SearchOptions::default());
    cache.max_owner_bytes = 32 * 1024 * 1024;
    let first = cache.prepare(&r.skills).unwrap();
    let bytes = first.exact.as_ref().unwrap().footprint();
    cache.max_owner_bytes = bytes * 2;
    cache.exact.clear();
    cache.exact_stamps.clear();
    let builds = cache.preparation_stats().geometry_builds;
    let mut skills = r.skills.clone();
    skills[0].score = ScoreEffect::PerfectOnly { percent: 155. };
    let second = cache.prepare(&skills).unwrap();
    assert_eq!(cache.preparation_stats().geometry_builds, builds);
    assert_eq!(first.exact.as_ref().unwrap().geometry_identity(),
        second.exact.as_ref().unwrap().geometry_identity());
    for duration in [0.1, 1., 2., 3., 4., 7.] {
        skills[0].duration = duration;
        skills[0].correction = (duration == 4.).then_some(JudgementCorrection::Bad);
        let bank = cache.prepare(&skills).unwrap();
        let mut direct = r.clone();
        direct.skills = skills.clone();
        for p in [0.125, r.power, f32::from_bits(r.power.to_bits() + 1)] {
            let power = ScoringPower::new(p).unwrap();
            let expected = JointTimeline::new(&direct).unwrap().maximum(power).unwrap().result.total;
            assert_eq!(bank.score(power, &mut TimingScoreScratch::default()).unwrap(), expected);
            assert_eq!(bank.materialize(power).unwrap().total, expected);
        }
        assert!(cache.geometry_owners.len() <= 16);
        assert!(cache.owner_bytes() <= cache.max_owner_bytes);
        assert!(cache.exact_geometry_bytes() <= cache.max_geometry_bytes);
    }
    let power = ScoringPower::new(r.power).unwrap();
    assert_eq!(first.materialize(power).unwrap().total,
        JointTimeline::new(&r).unwrap().maximum(power).unwrap().result.total);
    assert_eq!(cache.footprint().geometry_owner_payload_bytes, cache.owner_bytes());
}
