use super::*;
use crate::hit_timing::{JudgementCorrection, NoteWindow, ReplayNote, ScoreEffect};

fn input() -> ReplayInput {
    ReplayInput { power: 466061.125, play_level: 27, fps: 120,
        notes: (0..24).map(|i| ReplayNote { time: 0.2 + i as f64 * 0.05,
            trigger: (i < 6).then_some(i), window: NoteWindow::Standard }).collect(),
        hit_frames: (0..24).map(|i| 24 + i * 6 + if i % 3 == 0 { 9 } else { 0 }).collect(),
        skills: vec![ReplaySkill { duration: 0.1, score: ScoreEffect::Plain { percent: 100. },
            correction: None }; 6], initial_combo: 699, medley_combo: true,
        fever_frames: Some([30, 100]), constraints: vec![], hit_order: vec![],
        native_time_routing: None }
}

#[test]
fn certifies_success_with_native_queue_grades_fever_and_all_effects() {
    let effects = [ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. }, ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::PerfectOrHalf { percent: 135. }, ScoreEffect::Continuous { high: 150., low: 80. },
        ScoreEffect::Crescendo { percent: 100., step: 5., cap: 150 },
        ScoreEffect::Crescendo { percent: 130., step: 0.5, cap: 130 }];
    // The input order itself must stay valid despite Great offsets.
    let mut r = input();
    r.hit_frames = (0..24).map(|i| 24 + i * 6).collect();
    r.hit_frames[8] += 9;
    let mut proof = BaselineValidation::default();
    proof.validate(&r).unwrap();
    for seed in 0..72 {
        for (i, s) in r.skills.iter_mut().enumerate() {
            s.score = effects[(seed + i) % effects.len()];
            s.duration = [0.008333334, 0.1, 0.75, 7.5, 60.][(seed + i) % 5];
            s.correction = [None, Some(JudgementCorrection::Great),
                Some(JudgementCorrection::Good), Some(JudgementCorrection::Bad)][(seed + i) % 4];
        }
        assert!(replay(&r).is_ok());
        proof.validate(&r).unwrap();
    }
    assert_eq!(proof.stats.full_replays, 1);
    assert_eq!(proof.stats.certified_validations, 72);
    r.skills[2].duration = f32::NAN;
    assert_eq!(proof.validate(&r).unwrap_err(), replay(&r).unwrap_err());
}

#[test]
fn repaired_baseline_and_unprovable_overflow_keep_original_replay_errors() {
    let mut r = input();
    r.hit_frames = (0..24).map(|i| 24 + i * 6).collect();
    r.skills.iter_mut().for_each(|s| { s.duration = 7.5;
        s.correction = Some(JudgementCorrection::Bad); });
    r.hit_frames[20] += 14;
    let mut proof = BaselineValidation::default();
    proof.validate(&r).unwrap();
    r.skills[0].correction = None;
    assert_eq!(proof.validate(&r).unwrap_err(), replay(&r).unwrap_err());
    assert_eq!(proof.stats.certified_validations, 0);
    let mut r = input();
    r.hit_frames = (0..24).map(|i| 24 + i * 6).collect();
    // An enormous unused skill invalidates a global arithmetic bound, but does
    // not invalidate this baseline. Falling back must still accept it.
    r.notes.iter_mut().for_each(|n| n.trigger = None);
    let mut proof = BaselineValidation::default();
    proof.validate(&r).unwrap();
    r.skills[5].score = ScoreEffect::Plain { percent: f32::MAX };
    assert!(replay(&r).is_ok());
    proof.validate(&r).unwrap();
    assert_eq!(proof.stats.full_replays, 2);
    r.skills.truncate(0);
    proof.validate(&r).unwrap();
    assert_eq!(proof.stats.full_replays, 3);
}

#[test]
fn reuses_only_successful_immutable_native_routing_baselines() {
    let mut r = crate::hit_timing::time_connections::request();
    let mut proof = BaselineValidation::default();
    proof.validate(&r).unwrap();
    for duration in [0.1, 0.75, 7.5, 60.] {
        for skill in &mut r.skills {
            skill.duration = duration;
            skill.score = ScoreEffect::Continuous { high: 145., low: 0. };
        }
        assert_eq!(proof.validate(&r), replay(&r).map(|_| ()));
    }
    assert_eq!(proof.stats.full_replays, 1);
    assert_eq!(proof.stats.certified_validations, 4);
}
