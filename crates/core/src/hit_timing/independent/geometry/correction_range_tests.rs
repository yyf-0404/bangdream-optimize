use super::*;
use crate::hit_timing::{replay, NoteWindow, ReplayNote, ReplaySkill, SkillEvent};
fn input() -> ReplayInput {
    ReplayInput {
        power: 300040.75,
        play_level: 27,
        fps: 120,
        notes: (0..4)
            .map(|i| ReplayNote {
                time: if i < 3 { 1. } else { 10. },
                trigger: (i < 3).then_some(i),
                window: if i < 3 {
                    NoteWindow::NativeGreen {
                        first_frames: [120; 4],
                        last_frame: 123,
                    }
                } else {
                    NoteWindow::Standard
                },
            })
            .collect(),
        hit_frames: vec![120, 121, 122, 1200],
        hit_order: vec![],
        skills: vec![
            ReplaySkill {
                duration: 0.025,
                score: ScoreEffect::Plain { percent: 100. },
                correction: Some(JudgementCorrection::Bad)
            };
            3
        ],
        initial_combo: 0,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        native_time_routing: None,
    }
}
#[test]
fn endpoints_cover_all_queued_clock_starts_and_ends() {
    let mut input = input();
    for durations in [[0.025, 0.05, 0.075], [0.1, 0.025, 0.2], [7., 7.5, 7.]] {
        for (s, d) in input.skills.iter_mut().zip(durations) {
            s.duration = d;
        }
        let ranges = correction_ranges(&input).unwrap();
        for a in 120..=123 {
            for b in a..=123 {
                for c in b..=123 {
                    input.hit_frames[..3].copy_from_slice(&[a, b, c]);
                    let result = replay(&input).unwrap();
                    for event in result.skill_events {
                        match event {
                            SkillEvent::Start {
                                index,
                                first_input_frame,
                                ..
                            } => assert!(
                                first_input_frame >= ranges[index].1
                                    && first_input_frame <= ranges[index].2
                            ),
                            SkillEvent::End {
                                index,
                                last_input_frame,
                                ..
                            } => assert!(
                                last_input_frame >= ranges[index].1
                                    && last_input_frame <= ranges[index].2
                            ),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn bad_domains_are_removed_only_where_no_correction_can_be_active() {
    let input = input();
    let prepared = Prepared::new(&input).unwrap();
    let row = prepared.joint_row();
    for d in &row.domains[3] {
        for f in d.first..=d.last {
            assert!(input.notes[3].raw_judgement(f).unwrap() >= Judgement::Great);
        }
    }
    assert!(prepared.intervals[3]
        .iter()
        .any(|i| i.raw == Judgement::Bad));
    // Extend the last correction across the ordinary note. Low grades must
    // reappear instead of becoming an unconditional per-note restriction.
    let mut extended = input.clone();
    extended.skills[2].duration = 20.;
    let row = Prepared::new(&extended).unwrap().joint_row();
    assert!(row.domains[3].iter().any(|d| (d.first..=d.last)
        .any(|f| extended.notes[3].raw_judgement(f).unwrap() == Judgement::Bad)));
}
