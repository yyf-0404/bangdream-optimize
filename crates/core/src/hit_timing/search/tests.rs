use super::*;
use crate::hit_timing::{NoteConstraint, ReplayNote, ReplaySkill, ScoreEffect};

fn plain() -> ReplaySkill {
    ReplaySkill {
        duration: 3.0,
        score: ScoreEffect::Plain { percent: 100.0 },
        correction: None,
    }
}

fn input(times: &[f64], skills: Vec<ReplaySkill>) -> ReplayInput {
    ReplayInput {
        power: times.len() as f32 * 1000.0 / 3.0,
        play_level: 5,
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
        hit_frames: times.iter().map(|t| (t * 120.0).round() as u32).collect(),
        skills,
        initial_combo: 0,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    }
}

fn profiles() -> Vec<ReplaySkill> {
    let mut profiles: Vec<_> = [
        ScoreEffect::Plain { percent: 100.0 },
        ScoreEffect::PerfectOnly { percent: 120.0 },
        ScoreEffect::PerfectOrZero { percent: 125.0 },
        ScoreEffect::PerfectOrHalf { percent: 125.0 },
        ScoreEffect::Continuous {
            high: 110.0,
            low: 90.0,
        },
        ScoreEffect::Crescendo {
            percent: 100.0,
            step: 0.5,
            cap: 150,
        },
    ]
    .into_iter()
    .map(|score| ReplaySkill { score, ..plain() })
    .collect();
    for correction in [
        JudgementCorrection::Great,
        JudgementCorrection::Good,
        JudgementCorrection::Bad,
    ] {
        profiles.push(ReplaySkill {
            correction: Some(correction),
            ..plain()
        });
    }
    profiles
}

#[test]
fn cached_timelines_match_full_replay_with_state_and_actual_hit_order() {
    let mut rng = 944u32;
    for skill in profiles() {
        let times: Vec<_> = [0.2, 1.0]
            .into_iter()
            .chain((0..24).map(|i| 0.23 + i as f64 * 0.18))
            .collect();
        let mut request = input(&times, vec![skill.clone(), skill]);
        request.power = 300040.75;
        request.initial_combo = 19;
        request.fever_frames = Some([180, 480]);
        let pinned = request.hit_frames[25];
        request.notes[25].window = NoteWindow::PinnedPerfect { frame: pinned };
        let mut scorer = Scorer::new(&request).unwrap();
        let base = scorer.prepare(&request.hit_frames).unwrap();
        for iteration in 0..48 {
            let mut candidate = request.clone();
            for i in 2..25 {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let radius = if iteration < 24 { 3 } else { 16 };
                let offset = (rng % (radius * 2 + 1)) as i32 - radius as i32;
                candidate.hit_frames[i] = (candidate.hit_frames[i] as i32 + offset) as u32;
            }
            let expected = replay(&candidate).map(|r| r.total);
            assert_eq!(scorer.score(&candidate.hit_frames), expected);
            assert_eq!(scorer.score(&candidate.hit_frames), expected);
            assert_eq!(scorer.score_delta(&base, &candidate.hit_frames), expected);
            assert_eq!(scorer.score_delta(&base, &candidate.hit_frames), expected);
        }
        assert_eq!(scorer.builds, 1);
        assert!(scorer.hits > 0);
    }
}

#[test]
fn same_frame_queue_countdown_and_state_reset_match_full_replay() {
    for profile in profiles() {
        let request = input(
            &[0.0, 1.0, 3.0, 3.025, 454.0 / 120.0, 455.0 / 120.0],
            vec![profile.clone(), profile],
        );
        let mut scorer = Scorer::new(&request).unwrap();
        for first in 0..=3 {
            for second in [first, 120] {
                for end in 359..=364 {
                    let mut candidate = request.clone();
                    candidate.notes[1].time = f64::from(second) / 120.0;
                    candidate.hit_frames[0] = first;
                    candidate.hit_frames[1] = second;
                    candidate.hit_frames[2] = end;
                    // The same-frame variant needs matching note metadata in
                    // both evaluators; its cache remains local to that input.
                    let expected = replay(&candidate).map(|r| r.total);
                    if second == 120 {
                        assert_eq!(scorer.score(&candidate.hit_frames), expected);
                    } else {
                        assert_eq!(
                            Scorer::new(&candidate)
                                .unwrap()
                                .score(&candidate.hit_frames),
                            expected
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn joint_search_matches_small_exhaustive_oracles_for_all_effects() {
    for skill in profiles() {
        let request = input(&[1.0, 1.015, 4.08], vec![skill]);
        let result = optimize(&request, SearchOptions::default()).unwrap();
        let domain = Search::new(&request, SearchOptions::default())
            .unwrap()
            .choices;
        let mut best = 0;
        let mut candidate = request.clone();
        for &a in &domain[0] {
            for &b in &domain[1] {
                for &c in &domain[2] {
                    candidate.hit_frames = vec![a, b, c];
                    if let Ok(result) = replay(&candidate) {
                        best = best.max(result.total);
                    }
                }
            }
        }
        assert_eq!(result.result.total, best, "{:?}", request.skills[0]);
        assert!(result.result.total >= result.coordinate_result.total);
        assert_eq!(result.stop, SearchStop::Complete);
    }
}

#[test]
fn pinned_nodes_and_minimum_gaps_survive_joint_proposals_and_limits() {
    let mut request = input(&[1.0, 1.02, 1.03, 4.08], vec![plain()]);
    request.notes[2].window = NoteWindow::PinnedPerfect { frame: 124 };
    request.hit_frames[2] = 124;
    request.constraints = vec![
        NoteConstraint {
            before: 0,
            after: 1,
            minimum_gap_frames: 1,
            require_dispatch_order: false,
            earliest_after: None,
        },
        NoteConstraint {
            before: 1,
            after: 2,
            minimum_gap_frames: 0,
            require_dispatch_order: false,
            earliest_after: None,
        },
    ];
    for budget in [1, 2, 30, 300, 20_000] {
        let options = SearchOptions {
            max_evaluations: budget,
            ..SearchOptions::default()
        };
        let result = optimize(&request, options).unwrap();
        assert!(result.evaluations <= budget);
        assert_eq!(result.result.hit_frames[2], 124);
        assert!(result.result.hit_frames[0] < result.result.hit_frames[1]);
        assert!(result.result.hit_frames[1] <= result.result.hit_frames[2]);
        assert!(result.result.total >= result.baseline.total);
        if budget <= 2 {
            assert_eq!(result.stop, SearchStop::EvaluationLimit);
        }
        let mut replay_input = request.clone();
        replay_input.hit_frames = result.result.hit_frames;
        assert_eq!(replay(&replay_input).unwrap().total, result.result.total);
    }
}

#[test]
fn accepted_group_history_is_legal_and_adds_up() {
    let request = input(&[1.0, 1.015, 4.08, 4.081], vec![plain()]);
    let result = optimize(&request, SearchOptions::default()).unwrap();
    let mut candidate = request.clone();
    candidate.hit_frames = result.coordinate_result.hit_frames.clone();
    let mut previous = result.coordinate_result.total;
    for group in &result.accepted {
        for change in &group.changes {
            assert_eq!(candidate.hit_frames[change.note_index], change.from_frame);
            candidate.hit_frames[change.note_index] = change.to_frame;
        }
        let verified = replay(&candidate).unwrap().total;
        assert_eq!(verified - previous, group.gain);
        assert_eq!(verified, group.score);
        previous = verified;
    }
    assert_eq!(previous, result.result.total);
    assert_eq!(candidate.hit_frames, result.result.hit_frames);
}

#[test]
fn shortlist_sees_queued_successor_boundaries() {
    let request = input(&[0.0, 1.0, 3.79, 5.5], vec![plain(), plain()]);
    let mut search = Search::new(&request, SearchOptions::default()).unwrap();
    let shortlist = search.shortlist(&request.hit_frames).unwrap();
    assert!(shortlist.contains(&2));
    assert!(!shortlist.contains(&3));
}

#[test]
fn invalid_input_fails_without_search() {
    let request = input(&[1.0, 1.015, 4.08], vec![plain()]);
    let mut invalid = request.clone();
    invalid.fps = 60;
    assert!(optimize(&invalid, SearchOptions::default()).is_err());
    invalid = request.clone();
    invalid.hit_frames[0] = 100;
    invalid.skills[0].correction = Some(JudgementCorrection::Bad);
    assert!(optimize(&invalid, SearchOptions::default()).is_err());
}

#[test]
fn native_ownership_is_rechecked_even_when_the_skill_timeline_is_cached() {
    use crate::hit_timing::{NativeTimeRouting, NativeTimeTarget};
    let mut request = input(&[0., 1., 1.05], vec![plain()]);
    request.skills[0].correction = Some(JudgementCorrection::Bad);
    request.native_time_routing = Some(NativeTimeRouting {
        distance_mode: Default::default(),
        connections: vec![],
        event_order: vec![],
        music_positions: (0..=150).map(|f| (f * 10) as f32).collect(),
        targets: [0, 1200, 1260]
            .into_iter()
            .enumerate()
            .map(|(i, absolute_pos)| {
                Some(NativeTimeTarget {
                    button: if i == 0 { 1 } else { 0 },
                    absolute_pos,
                    active_from_frame: 0,
                    active_until_frame: 150,
                })
            })
            .collect(),
        candidate_order: vec![0, 1, 2],
    });
    let mut scorer = Scorer::new(&request).unwrap();
    let base = scorer.prepare(&request.hit_frames).unwrap();
    for a in 108..=138 {
        for b in 108..=138 {
            let frames = vec![0, a, b];
            let fast = scorer.score(&frames);
            let mut trial = request.clone();
            trial.hit_frames = frames;
            let full = replay(&trial).map(|r| r.total);
            assert_eq!(fast, full, "frames {a}, {b}");
            assert_eq!(scorer.score_delta(&base, &trial.hit_frames), full);
        }
    }
    assert_eq!(scorer.builds, 1);
    assert_eq!(scorer.hits, 961);
}

#[test]
fn connection_binding_and_event_order_are_replayed_on_cached_skill_timelines() {
    for tap_button in [0, 1] {
        let mut r = crate::hit_timing::time_connections::request();
        r.skills = vec![plain()];
        r.notes[0].trigger = Some(0);
        let routing = r.native_time_routing.as_mut().unwrap();
        routing.event_order = vec![0, 2, 1];
        routing.targets[2].as_mut().unwrap().button = tap_button;
        let mut scorer = Scorer::new(&r).unwrap();
        let base = scorer.prepare(&r.hit_frames).unwrap();
        for end in 119..=139 {
            for tap in 119..=139 {
                let mut plan = r.clone();
                plan.hit_frames = vec![120, end, tap];
                assert_eq!(
                    scorer.score(&plan.hit_frames),
                    replay(&plan).map(|r| r.total)
                );
                assert_eq!(
                    scorer.score_delta(&base, &plan.hit_frames),
                    replay(&plan).map(|r| r.total)
                );
            }
        }
        assert_eq!(scorer.builds, 1);
        assert_eq!(scorer.hits, 441);
        let found = optimize(&r, SearchOptions::default()).unwrap();
        r.hit_frames = found.result.hit_frames;
        assert_eq!(replay(&r).unwrap().total, found.result.total);
    }
}

#[test]
fn skill_cache_keys_follow_explicit_within_frame_dispatch_order() {
    let mut r = input(&[1., 1.], vec![plain(), plain()]);
    r.notes[0].trigger = Some(1);
    r.notes[1].trigger = Some(0);
    r.native_time_routing = Some(crate::hit_timing::NativeTimeRouting {
        distance_mode: Default::default(),
        music_positions: vec![1200.; 130],
        targets: (0..2)
            .map(|button| {
                Some(crate::hit_timing::NativeTimeTarget {
                    button,
                    absolute_pos: 1200,
                    active_from_frame: 110,
                    active_until_frame: 129,
                })
            })
            .collect(),
        candidate_order: vec![0, 1],
        connections: vec![],
        event_order: vec![1, 0],
    });
    let expected = replay(&r).unwrap().total;
    let mut scorer = Scorer::new(&r).unwrap();
    assert_eq!(scorer.score(&r.hit_frames).unwrap(), expected);
    assert_eq!(scorer.score(&r.hit_frames).unwrap(), expected);
    assert_eq!(scorer.hits, 1);
}

#[test]
fn delta_replays_state_until_convergence_and_skips_unaffected_suffix() {
    let times = [0., 0.2, 0.5, 0.75, 1.0, 1.25, 4., 5.];
    for skill in profiles() {
        let mut r = input(&times, vec![skill]);
        r.initial_combo = 18;
        r.power = 466061.125;
        let mut scorer = Scorer::new(&r).unwrap();
        let base = scorer.prepare(&r.hit_frames).unwrap();
        for (index, offset) in [(2, 1), (2, 8), (6, 8)] {
            let mut candidate = r.clone();
            candidate.hit_frames[index] += offset;
            let before = scorer.notes_scored;
            assert_eq!(
                scorer.score_delta(&base, &candidate.hit_frames),
                replay(&candidate).map(|r| r.total),
                "{:?}, index {index}, offset {offset}",
                r.skills[0]
            );
            let scored = scorer.notes_scored - before;
            // One unchanged-grade move has no downstream state change.
            if offset == 1 || index == 6 {
                assert_eq!(scored, 1);
            } else if matches!(
                r.skills[0].score,
                ScoreEffect::Continuous { .. } | ScoreEffect::Crescendo { .. }
            ) {
                // Great changes the active skill, but never the far-away tail.
                assert_eq!(scored, 4);
            }
        }
        // Moving a trigger invalidates the fixed schedule and uses full scoring.
        let deltas = scorer.delta_calls;
        let mut candidate = r.clone();
        candidate.hit_frames[0] = 2;
        assert_eq!(
            scorer.score_delta(&base, &candidate.hit_frames),
            replay(&candidate).map(|r| r.total)
        );
        assert_eq!(scorer.delta_calls, deltas);
    }
}

#[test]
fn delta_handles_combo_rank_reordering_fever_and_same_frame_dispatch() {
    for skill in profiles() {
        let mut r = input(&[0., 1., 1.025, 1.05, 1.075, 3.], vec![skill]);
        r.power = 300040.75;
        r.initial_combo = 18;
        r.fever_frames = Some([122, 130]);
        let mut scorer = Scorer::new(&r).unwrap();
        let base = scorer.prepare(&r.hit_frames).unwrap();
        for first in 115..=133 {
            for second in 115..=136 {
                let mut candidate = r.clone();
                candidate.hit_frames[1] = first;
                candidate.hit_frames[2] = second;
                assert_eq!(
                    scorer.score_delta(&base, &candidate.hit_frames),
                    replay(&candidate).map(|r| r.total)
                );
            }
        }
    }
}

#[test]
fn window_shortlist_keeps_fever_and_dependencies_but_not_stable_interior() {
    let mut r = input(&[0., 0.5, 1., 2., 3.04, 5., 10.], vec![plain()]);
    r.fever_frames = Some([120, 240]);
    r.constraints.push(NoteConstraint {
        before: 4,
        after: 6,
        minimum_gap_frames: 0,
        require_dispatch_order: false,
        earliest_after: None,
    });
    let mut search = Search::new(&r, SearchOptions::default()).unwrap();
    let indices = search.shortlist(&r.hit_frames).unwrap();
    assert_eq!(indices, vec![2, 3, 4, 6]);
}

#[test]
fn default_search_has_no_iteration_or_evaluation_cutoff() {
    let options = SearchOptions::default();
    assert!(options.complete_ordinary);
    assert_eq!(
        (
            options.max_rounds,
            options.max_refine_passes,
            options.max_evaluations
        ),
        (0, 0, 0)
    );
    let request = input(&[1., 1.015, 4.08], vec![plain()]);
    let found = optimize(&request, options).unwrap();
    assert!(found.ordinary_dp.graph_builds > 0);
    assert!(found.ordinary_dp.transitions > 0);
    assert_eq!(found.stop, SearchStop::Complete);
    assert_eq!(found.inner_pass_limit_count, 0);
    let mut settled = request;
    settled.hit_frames = found.result.hit_frames;
    let again = optimize(&settled, options).unwrap();
    assert_eq!(again.result.total, found.result.total);
    assert!(again.accepted.is_empty());
}

#[test]
#[ignore = "bounded fixture discovery; freeze any counterexample before changing the search"]
fn discover_fixed_window_counterexample() {
    let mut rng = 944u32;
    for skill in profiles() {
        for case in 0..80 {
            let mut next = || {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                rng
            };
            let a = 3. + (next() % 31) as f64 / 120. - 0.12;
            let b = 3. + (next() % 31) as f64 / 120. - 0.12;
            let mut r = input(&[0., a, b], vec![skill.clone()]);
            r.notes[0].window = NoteWindow::PinnedPerfect { frame: 0 };
            r.initial_combo = [0, 18, 19, 48][case % 4];
            r.power = [1000., 300040.75, 466061.125][case % 3];
            let found = optimize(&r, SearchOptions::default()).unwrap();
            let domains = frame_domains(&r).unwrap();
            let mut q = r.clone();
            let mut best = found.result.total;
            let mut best_frames = found.result.hit_frames.clone();
            for &x in &domains[1] {
                for &y in &domains[2] {
                    q.hit_frames = vec![0, x, y];
                    if let Ok(v) = replay(&q) {
                        if v.total > best {
                            best = v.total;
                            best_frames = q.hit_frames.clone();
                        }
                    }
                }
            }
            assert_eq!(
                found.result.total,
                best,
                "request={} found={:?} best={:?}",
                serde_json::to_string(&r).unwrap(),
                found.result.hit_frames,
                best_frames
            );
        }
    }
}

#[test]
fn fixed_plain_window_escapes_neutral_move_at_combo_boundary() {
    let mut r = input(&[0., 2.9883333333333333, 3.08], vec![plain()]);
    r.notes[0].window = NoteWindow::PinnedPerfect { frame: 0 };
    r.initial_combo = 18;
    r.power = 300040.75;
    let mut old = r.clone();
    old.hit_frames = vec![0, 359, 361];
    assert_eq!(replay(&old).unwrap().total, 1_474_996);
    let found = optimize(&r, SearchOptions::default()).unwrap();
    let domains = frame_domains(&r).unwrap();
    let mut best = 0;
    for &a in &domains[1] {
        for &b in &domains[2] {
            old.hit_frames = vec![0, a, b];
            if let Ok(v) = replay(&old) {
                best = best.max(v.total);
            }
        }
    }
    assert_eq!(best, 1_476_798);
    assert_eq!(found.result.total, best);
    assert_eq!(found.stop, SearchStop::Complete);
}

#[test]
fn fixed_crescendo_window_can_reorder_hits_with_different_frame_domains() {
    let mut s = plain();
    s.score = ScoreEffect::Crescendo {
        percent: 100.,
        step: 0.5,
        cap: 150,
    };
    let mut r = input(&[0., 2.98, 3.08], vec![s]);
    r.notes[0].window = NoteWindow::PinnedPerfect { frame: 0 };
    r.initial_combo = 48;
    r.power = 1000.;
    let found = optimize(&r, SearchOptions::default()).unwrap();
    assert_eq!(found.result.total, 4976);
    assert_eq!(found.stop, SearchStop::Complete);
    let mut q = r.clone();
    q.hit_frames = vec![0, 360, 359];
    assert_eq!(replay(&q).unwrap().total, found.result.total);
}
