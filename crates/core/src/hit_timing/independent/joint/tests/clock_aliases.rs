use super::*;
use crate::hit_timing::{JudgementCorrection, ScoreEffect};

#[test]
fn native_clock_aliases_compare_full_tables_corrections_and_progress() {
    let base = skill(ScoreEffect::Plain { percent: 25. });
    let skills = vec![base.clone(), ReplaySkill { duration: 1.25, ..base.clone() },
        skill(ScoreEffect::PerfectOrHalf { percent: 25. }),
        skill(ScoreEffect::Crescendo { percent: 25., step: 0.5, cap: 25 }),
        ReplaySkill { correction: Some(JudgementCorrection::Good), ..base.clone() },
        skill(ScoreEffect::Continuous { high: 25., low: 25. })];
    let model = JointTimeline::new(&input(&[0.2, 1.3, 2.5, 3.7, 4.9, 6.1], skills, 1)).unwrap();
    let costs = clock::ClockCosts::new_controlled(JointCosts::new_controlled(&model,
        ScoringPower::new(model.input.power).unwrap(), true).unwrap(), true);
    assert_eq!(costs.alias_classes(), &[0, 0, 2, 0, 4, 5]);
    let costs = clock::ClockCosts::new_controlled(JointCosts::new_controlled(&model,
        ScoringPower::new(model.input.power).unwrap(), true).unwrap(), false);
    assert_eq!(costs.alias_classes(), &[0, 1, 2, 3, 4, 5]);
}

#[test]
fn native_clock_aliases_preserve_actual_durations_fifo_expiry_grades_and_f32_powers() {
    let mut profiles = profiles();
    profiles.push(skill(ScoreEffect::Crescendo { percent: 20., step: 0.5, cap: 20 }));
    for (profile_index, profile) in profiles.into_iter().enumerate() {
        for queued in [false, true] {
            let times = if queued { vec![0.2, 0.23, 0.30, 0.31, 0.33, 0.40,
                0.28, 0.53, 1.01, 3.3, 8.] }
                else { vec![0.2, 1.3, 2.5, 3.7, 4.9, 6.1, 0.3, 1.31, 2.63, 6.21] };
            let skills = [0.008333334, 0.1, 1.25, 0.05, 0.2, 0.1].into_iter()
                .map(|duration| ReplaySkill { duration, ..profile.clone() }).collect();
            let mut request = input(&times, skills, 1);
            request.native_time_routing = None;
            request.fever_frames = Some([45, 400]);
            request.medley_combo = profile_index % 2 == 0;
            let model = JointTimeline::new(&request).unwrap();
            for power in [0.125, 37.5, 300040.75, 500001.5] {
                let power = ScoringPower::new(power).unwrap();
                let old = model.score_with(&mut JointCosts::new_controlled(&model, power, false).unwrap()).unwrap();
                let off = model.score_with(&mut clock::ClockCosts::new_controlled(
                    JointCosts::new_controlled(&model, power, true).unwrap(), false)).unwrap();
                let on = model.score_with(&mut clock::ClockCosts::new_controlled(
                    JointCosts::new_controlled(&model, power, true).unwrap(), true)).unwrap();
                assert_eq!(old.0, off.0, "profile {profile_index}, queued {queued}, power {power:?}");
                assert_eq!(old.0, on.0, "profile {profile_index}, queued {queued}, power {power:?}");
                assert_eq!(on.1.native_blocks.as_ref().unwrap().clock_aliases, 5);
            }
        }
    }
}
