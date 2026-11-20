use super::*;
use crate::hit_timing::{
    independent::JointTimeline, ranges::{RelativeFrameRange, TimingSongRanges}, replay,
    CandidateDistanceMode, JudgementCorrection, NativeTimeRouting, NativeTimeTarget,
    NoteWindow, ReplayNote, ReplaySkill, ScoreEffect,
};

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    note: ReplayNote,
    reference: u32,
    target: Option<NativeTimeTarget>,
    grades: Vec<(i32, i32, Judgement)>,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!("../../../tests/fixtures/reference-offset-windows.json")).unwrap()
}

impl Case {
    fn raw(&self, frame: u32) -> Judgement {
        let offset = frame as i32 - self.reference as i32;
        self.grades.iter().find(|&&(a, b, _)| a <= offset && offset <= b)
            .map_or(Judgement::Miss, |&(_, _, g)| g)
    }

    fn input(&self) -> ReplayInput {
        ReplayInput {
            power: 300000.25, play_level: 27, fps: REPLAY_FPS,
            notes: vec![self.note.clone()], hit_frames: vec![self.reference],
            hit_order: vec![], skills: vec![], initial_combo: 0, medley_combo: true,
            fever_frames: None, constraints: vec![],
            native_time_routing: self.target.clone().map(|t| NativeTimeRouting {
                distance_mode: CandidateDistanceMode::ChartTime, music_positions: vec![],
                targets: vec![Some(t)], candidate_order: vec![0], connections: vec![], event_order: vec![],
            }),
        }
    }
}

// Hand-derived fixtures use the native f32 ordinary threshold rule and the
// declared Y[0,1,2,5,7,8] projection. They do not derive expectations from
// raw_judgement/frame_domain. Check every supported frame, including all Misses.
// In particular f32((7/120)/f32(1/60)) is 3.49999976, not 3.5, and
// f32((15/120)/f32(1/60)) is 7.49999952, not 7.5. Do not use ideal-real ties.
#[test]
fn every_offset_keeps_the_original_grade_instead_of_recentering_the_window() {
    for c in cases() {
        let input = c.input();
        assert_eq!(input.chart_reference_frame(0).unwrap(), c.reference, "{}", c.name);
        assert_eq!(c.raw(c.reference), Judgement::Perfect, "{}", c.name);
        let mut expected = [vec![], vec![], vec![], vec![]];
        for frame in 0..=72_000 {
            let raw = c.raw(frame);
            assert_eq!(c.note.raw_judgement(frame).unwrap(), raw, "{} offset={}",
                c.name, frame as i32 - c.reference as i32);
            for (domain, minimum) in expected.iter_mut().zip([
                Judgement::Bad, Judgement::Good, Judgement::Great, Judgement::Perfect,
            ]) {
                if raw >= minimum { domain.push(frame); }
            }
        }
        for (domain, minimum) in expected.into_iter().zip([
            Judgement::Bad, Judgement::Good, Judgement::Great, Judgement::Perfect,
        ]) {
            assert_eq!(c.note.frame_domain(minimum).unwrap(), domain, "{} {minimum:?}", c.name);
        }
    }
}

#[test]
fn relative_limits_preserve_all_grade_edges_and_clip_only_the_song_horizon() {
    for c in cases() {
        let input = c.input();
        let mut offsets = vec![-72_000, 0, 72_000];
        for &(a, b, _) in &c.grades { offsets.extend([a - 1, a, b, b + 1]); }
        for offset in offsets.into_iter().filter(|o| (-72_000..=72_000).contains(o)) {
            let first = offset.min(0);
            let last = offset.max(0);
            let request = TimingSongRanges {
                song_id: 1, difficulty: 3, reference: input.clone(),
                ranges: vec![RelativeFrameRange { note_index: 0, first_offset: first, last_offset: last }],
            };
            let limits = request.limits(&input).unwrap();
            assert_eq!(limits[0], Some([
                (i64::from(c.reference) + i64::from(first)).max(0) as u32,
                (i64::from(c.reference) + i64::from(last)).min(72_000) as u32,
            ]), "{} offset={offset}", c.name);
        }
    }
}

#[test]
fn per_offset_dp_and_replay_apply_correction_and_lifetime_after_translation() {
    for c in cases().into_iter().filter(|c| c.note.time > 1. && c.note.time < 10.
        && !matches!(c.note.window, NoteWindow::NativeGreen { .. } | NoteWindow::PinnedPerfect { .. })) {
        let first = (c.reference as i32 + c.grades.first().unwrap().0 - 1) as u32;
        let last = (c.reference as i32 + c.grades.last().unwrap().1 + 1) as u32;
        for correction in [None, Some(JudgementCorrection::Great), Some(JudgementCorrection::Good),
            Some(JudgementCorrection::Bad)] {
            let mut input = c.input();
            // A separate early trigger keeps the skill active over every probe.
            input.notes.insert(0, ReplayNote {time: 1., trigger: Some(0), window: NoteWindow::Standard});
            input.hit_frames.insert(0, 120);
            input.skills = vec![ReplaySkill {duration: 5., score: ScoreEffect::Plain {percent: 100.}, correction}];
            if let Some(r) = &mut input.native_time_routing {
                r.targets.insert(0, Some(NativeTimeTarget {button: 1, absolute_pos: 0,
                    active_from_frame: 105, active_until_frame: 135}));
                r.candidate_order = vec![0, 1];
            }
            let threshold = match correction {
                Some(JudgementCorrection::Great) => Judgement::Great,
                Some(JudgementCorrection::Good) => Judgement::Good,
                Some(JudgementCorrection::Bad) => Judgement::Bad,
                None => Judgement::Perfect,
            };
            for frame in first..=last {
                let raw = c.raw(frame);
                let corrected = if raw >= threshold { Judgement::Perfect } else { raw };
                let live = c.target.as_ref().is_none_or(|t| t.active_from_frame <= frame && frame <= t.active_until_frame);
                let feasible = live && corrected >= Judgement::Great;
                let winner = JointTimeline::maximum_in_frame_ranges(&input,
                    &[Some([120, 120]), Some([frame, frame])]).unwrap();
                assert_eq!(winner.is_some(), feasible, "{} offset={} {correction:?}", c.name,
                    frame as i32 - c.reference as i32);
                let mut trial = input.clone();
                trial.hit_frames[1] = frame;
                let result = replay(&trial);
                assert_eq!(result.is_ok(), feasible, "{} frame={frame} {correction:?}", c.name);
                if let Some(winner) = winner {
                    let result = result.unwrap();
                    assert_eq!(result, winner.result);
                    assert_eq!(result.trace[1].raw, raw);
                    assert_eq!(result.trace[1].judgement, corrected);
                }
            }
        }
    }
}
