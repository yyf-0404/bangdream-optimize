use super::*;
use crate::scoring::{normal_combo_multiplier, Judgement};

fn plain(percent: f32) -> ReplaySkill {
    ReplaySkill {
        duration: 3.0,
        score: ScoreEffect::Plain { percent },
        correction: None,
    }
}
fn input(times: &[f64], frames: &[u32], skills: Vec<ReplaySkill>) -> ReplayInput {
    ReplayInput {
        power: times.len() as f32 * 1000.0 / 3.0,
        play_level: 5,
        fps: REPLAY_FPS,
        notes: times
            .iter()
            .map(|&time| ReplayNote {
                time,
                trigger: None,
                window: NoteWindow::Standard,
            })
            .collect(),
        hit_frames: frames.to_vec(),
        skills,
        initial_combo: 0,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    }
}
fn scores(result: &ReplayResult) -> Vec<u32> {
    result.trace.iter().map(|n| n.score).collect()
}

#[test]
fn compiled_witness_matches_replay_at_each_equipment_power() {
    let mut request = input(&[0.0, 0.5, 1.0, 1.5], &[0, 66, 120, 180], vec![plain(77.5)]);
    request.notes[0].trigger = Some(0);
    request.initial_combo = 98;
    request.fever_frames = Some([65, 125]);
    for medley_combo in [false, true] {
        request.medley_combo = medley_combo;
        let (_, timeline) = compile_replay(&request).unwrap();
        for power in [0.0, 123_456.75, 330_001.5] {
            request.power = power;
            assert_eq!(
                timeline.score(ScoringPower::new(power).unwrap()).unwrap(),
                replay(&request).unwrap().total,
            );
        }
    }
}

#[test]
fn same_frame_and_next_frame_have_explicit_skill_semantics() {
    let mut request = input(&[1.0, 1.0], &[120, 120], vec![plain(100.0)]);
    request.notes[0].trigger = Some(0);
    let same = replay(&request).unwrap();
    assert_eq!(scores(&same), vec![1100, 1100]);
    assert!(same.trace.iter().all(|n| n.active_skill.is_none()));
    request.hit_frames[1] += 1;
    let next = replay(&request).unwrap();
    assert_eq!(scores(&next), vec![1100, 2200]);
    assert_eq!(next.trace[1].active_skill, Some(0));
    assert_eq!(next.timing_revision, TIMING_REVISION);
    assert_eq!(next.fps, 120);
}

#[test]
fn native_f32_countdown_keeps_duration_specific_end_frames() {
    for (duration, end) in [(3.0, 361), (5.0, 601), (7.0, 842)] {
        let mut skill = plain(100.0);
        skill.duration = duration;
        let mut request = input(
            &[0.0, end as f64 / 120.0, (end + 1) as f64 / 120.0],
            &[0, end, end + 1],
            vec![skill],
        );
        request.notes[0].trigger = Some(0);
        let result = replay(&request).unwrap();
        assert_eq!(scores(&result), vec![1100, 2200, 1100]);
        assert_eq!(
            result.skill_events[2],
            SkillEvent::End {
                index: 0,
                after_frame: end,
                last_input_frame: end
            }
        );
    }
}

#[test]
fn queued_skills_wait_for_cooldown_and_reset_only_at_start() {
    let mut skill = plain(100.0);
    skill.score = ScoreEffect::Crescendo {
        percent: 100.0,
        step: 0.5,
        cap: 150,
    };
    let mut request = input(
        &[0.0, 1.0, 454.0 / 120.0, 455.0 / 120.0],
        &[0, 120, 454, 455],
        vec![skill.clone(), skill],
    );
    request.notes[0].trigger = Some(0);
    request.notes[1].trigger = Some(1);
    let result = replay(&request).unwrap();
    assert_eq!(scores(&result), vec![1100, 2205, 1100, 2205]);
    assert_eq!(
        result.trace[1].skill_state.as_ref().unwrap().extra_percent,
        0.5
    );
    assert_eq!(
        result.trace[3].skill_state.as_ref().unwrap().extra_percent,
        0.5
    );
    assert!(result.skill_events.contains(&SkillEvent::Start {
        index: 1,
        after_frame: 454,
        first_input_frame: 455
    }));
}

#[test]
fn pending_skill_cannot_correct_its_own_bad_and_expiry_removes_correction() {
    let mut skill = plain(40.0);
    skill.correction = Some(JudgementCorrection::Bad);
    let mut request = input(&[1.0], &[106], vec![skill.clone()]);
    request.notes[0].trigger = Some(0);
    assert!(matches!(
        replay(&request),
        Err(ReplayError::Judgement {
            raw: Judgement::Bad,
            ..
        })
    ));
    let mut request = input(&[0.0, 376.0 / 120.0], &[0, 362], vec![skill]);
    request.notes[0].trigger = Some(0);
    assert!(matches!(
        replay(&request),
        Err(ReplayError::Judgement {
            raw: Judgement::Bad,
            ..
        })
    ));
}

#[test]
fn active_correction_can_rescue_next_skill_note_but_never_miss() {
    let mut skill = plain(40.0);
    skill.correction = Some(JudgementCorrection::Bad);
    let mut request = input(&[0.0, 1.0], &[0, 106], vec![skill, plain(100.0)]);
    request.notes[0].trigger = Some(0);
    request.notes[1].trigger = Some(1);
    let result = replay(&request).unwrap();
    assert_eq!(result.trace[1].raw, Judgement::Bad);
    assert_eq!(result.trace[1].judgement, Judgement::Perfect);
    assert_eq!(result.trace[1].score, 1540);
    request.hit_frames[1] = 100;
    assert!(matches!(
        replay(&request),
        Err(ReplayError::Judgement {
            raw: Judgement::Miss,
            ..
        })
    ));
    request.hit_frames[1] = 106;
    for correction in [JudgementCorrection::Good, JudgementCorrection::Great] {
        request.skills[0].correction = Some(correction);
        assert!(matches!(
            replay(&request),
            Err(ReplayError::Judgement {
                raw: Judgement::Bad,
                ..
            })
        ));
    }
}

#[test]
fn great_modes_and_continuous_downgrade_keep_their_distinct_effects() {
    for (effect, great_score, following) in [
        (ScoreEffect::PerfectOnly { percent: 125.0 }, 800, 2475),
        (ScoreEffect::PerfectOrZero { percent: 125.0 }, 0, 2475),
        (ScoreEffect::PerfectOrHalf { percent: 125.0 }, 400, 2475),
        (
            ScoreEffect::Continuous {
                high: 110.0,
                low: 90.0,
            },
            1520,
            2090,
        ),
        (
            ScoreEffect::Crescendo {
                percent: 100.0,
                step: 0.5,
                cap: 150,
            },
            1604,
            2211,
        ),
    ] {
        let mut skill = plain(100.0);
        skill.score = effect;
        let mut request = input(&[0.0, 0.5, 1.0, 1.5], &[0, 60, 128, 180], vec![skill]);
        request.notes[0].trigger = Some(0);
        let result = replay(&request).unwrap();
        assert_eq!(result.trace[2].judgement, Judgement::Great);
        assert_eq!(
            (result.trace[2].score, result.trace[3].score),
            (great_score, following)
        );
        if matches!(effect, ScoreEffect::Continuous { .. }) {
            assert_eq!(
                result.trace[3]
                    .skill_state
                    .as_ref()
                    .unwrap()
                    .worst_judgement,
                Judgement::Great
            );
        }
        request.skills[0].correction = Some(JudgementCorrection::Great);
        let corrected = replay(&request).unwrap();
        assert_eq!(corrected.trace[2].judgement, Judgement::Perfect);
        assert_eq!(
            corrected.trace[3]
                .skill_state
                .as_ref()
                .unwrap()
                .worst_judgement,
            Judgement::Perfect
        );
    }
}

#[test]
fn pinned_nodes_and_explicit_constraints_cannot_be_repaired_by_correction() {
    let mut request = input(&[0.0, 1.0], &[0, 120], vec![]);
    request.notes[1].window = NoteWindow::PinnedPerfect { frame: 120 };
    let result = replay(&request).unwrap();
    assert_eq!(result.pinned_note_count, 1);
    request.hit_frames[1] += 1;
    assert!(matches!(
        replay(&request),
        Err(ReplayError::Note { index: 1, .. })
    ));
    request.notes[1].window = NoteWindow::Standard;
    request.notes[0].time = 1.0;
    request.hit_frames = vec![121, 120];
    request.constraints.push(NoteConstraint {
        before: 0,
        after: 1,
        minimum_gap_frames: 1,
        require_dispatch_order: false,
        earliest_after: None,
    });
    assert_eq!(
        replay(&request).unwrap_err(),
        ReplayError::Constraint { index: 0 }
    );
    request.hit_frames = vec![120, 121];
    assert!(replay(&request).is_ok());
}

#[test]
fn skill_slot_order_is_checked_after_actual_hit_sorting() {
    let mut request = input(&[1.0, 1.02], &[124, 120], vec![plain(0.0), plain(0.0)]);
    request.notes[0].trigger = Some(0);
    request.notes[1].trigger = Some(1);
    assert_eq!(replay(&request).unwrap_err(), ReplayError::SkillOrder);
    request.hit_frames = vec![120, 120];
    assert!(replay(&request).is_ok());
    request.notes[1].trigger = Some(0);
    assert!(matches!(
        replay(&request),
        Err(ReplayError::Note { index: 1, .. })
    ));
}

#[test]
fn float_power_combo_and_fever_use_the_shared_scorer_in_hit_order() {
    let mut request = input(&[1.02, 1.0, 1.1], &[122, 120, 132], vec![]);
    request.power = 300_040.75;
    request.play_level = 27;
    request.initial_combo = 19;
    request.fever_frames = Some([120, 121]);
    let result = replay(&request).unwrap();
    let direct = NoteScorer::new(ScoringPower::new(request.power).unwrap(), 27, 3).unwrap();
    assert_eq!(
        result
            .trace
            .iter()
            .map(|n| n.note_index)
            .collect::<Vec<_>>(),
        vec![1, 0, 2]
    );
    for row in &result.trace {
        let expected = direct
            .score(
                row.judgement,
                normal_combo_multiplier(row.combo),
                1.0,
                row.fever_multiplier,
            )
            .unwrap();
        assert_eq!(
            (row.before_skill, row.score),
            (expected.before_skill, expected.final_score)
        );
    }
    assert_eq!(result.trace[0].fever_multiplier, 2.0);
    assert_eq!(result.trace[1].fever_multiplier, 1.0);
    assert_eq!(result.scoring_power.to_bits(), request.power.to_bits());
}

#[test]
fn ordinary_and_endpoint_windows_keep_fixed_judgment_units() {
    for sign in [-1.0, 1.0] {
        for (offset, expected) in [
            (0.04, Judgement::Perfect),
            (0.075, Judgement::Great),
            (0.10, Judgement::Good),
            (0.12, Judgement::Bad),
            (0.14, Judgement::Miss),
        ] {
            assert_eq!(time_judgement(sign * offset, false).unwrap(), expected);
        }
        assert_eq!(
            time_judgement(sign * 0.05, true).unwrap(),
            Judgement::Perfect
        );
        assert_eq!(time_judgement(sign * 0.10, true).unwrap(), Judgement::Great);
    }
    // Exactly 2.5 and 6.5 native units round to even rather than away from zero.
    for (units, expected) in [(2.5, Judgement::Perfect), (6.5, Judgement::Good)] {
        let delta = f64::from((1.0_f32 / 60.0) * units);
        assert_eq!(time_judgement(delta, false).unwrap(), expected);
    }
}

#[test]
fn malformed_replays_fail_before_calculation_and_ignore_no_unknown_fields() {
    let mut request = input(&[0.0], &[0], vec![plain(100.0)]);
    request.notes[0].trigger = Some(0);
    for duration in [f32::NAN, f32::INFINITY, 0.0, -1.0, 61.0] {
        request.skills[0].duration = duration;
        assert!(replay(&request).is_err());
    }
    request.skills[0].duration = 3.0;
    request.initial_combo = u32::MAX;
    assert!(replay(&request).is_err());
    request.initial_combo = 0;
    for fps in [0, 60, 90, 240] {
        request.fps = fps;
        assert_eq!(
            replay(&request).unwrap_err(),
            ReplayError::Input("hit-timing replay requires 120 FPS")
        );
    }
    let mut value = serde_json::to_value(input(&[0.0], &[0], vec![])).unwrap();
    value["deviceAccurate"] = true.into();
    assert!(serde_json::from_value::<ReplayInput>(value).is_err());
}

#[test]
fn medley_combo_stays_inside_the_verified_native_table() {
    let mut request = input(&[0.0, 0.0], &[0, 0], vec![]);
    request.medley_combo = true;
    request.initial_combo = 9_997;
    let last = replay(&request).unwrap();
    assert_eq!(last.trace.last().unwrap().combo, 9_999);
    request.initial_combo = 9_998;
    assert_eq!(
        replay(&request).unwrap_err(),
        ReplayError::Input("Medley Combo exceeds the verified master-table range [0, 9999]"),
    );
    // Standard play uses a separate capped native formula and keeps its domain.
    request.medley_combo = false;
    assert_eq!(replay(&request).unwrap().trace.last().unwrap().combo, 10_000);
}
