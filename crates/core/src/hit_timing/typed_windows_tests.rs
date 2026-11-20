use super::*;
use independent::JointTimeline;
use SlideTimeRole::*;

fn note(time: f64, role: SlideTimeRole) -> ReplayNote {
    ReplayNote { time, trigger: None, window: NoteWindow::SlideTime { role, last_frame:None } }
}

// Expected constants come from the native Y-table gates, not frame_domain.
#[test]
fn typed_native_gates_and_distinct_expiry_branches() {
    for role in [Head, End] {
        let n = note(2., role);
        for (frame, grade) in [(223, Judgement::Miss), (224, Judgement::Bad),
            (225, Judgement::Bad), (226, Judgement::Good), (227, Judgement::Good),
            (228, Judgement::Great), (233, Judgement::Great), (234, Judgement::Perfect),
            (240, Judgement::Perfect), (266, Judgement::Perfect), (267, Judgement::Miss)] {
            assert_eq!(n.raw_judgement(frame).unwrap(), grade, "{role:?} frame={frame}");
        }
    }
    for (role, first, last) in [(Middle,238,266), (FlickEnd,240,254), (DirectionalEnd,240,266)] {
        let n = note(2., role);
        assert_eq!(n.raw_judgement(first-1).unwrap(), Judgement::Miss);
        assert_eq!(n.raw_judgement(last+1).unwrap(), Judgement::Miss);
        for frame in first..=last { assert_eq!(n.raw_judgement(frame).unwrap(), Judgement::Perfect); }
        for minimum in [Judgement::Bad, Judgement::Good, Judgement::Great, Judgement::Perfect] {
            assert_eq!(n.frame_domain(minimum).unwrap(), (first..=last).collect::<Vec<_>>());
        }
    }
}

#[test]
fn typed_fractional_arrival_does_not_round_a_tail_gate_early() {
    let n = note(2.001, FlickEnd);
    assert_eq!(n.raw_judgement(240).unwrap(), Judgement::Miss);
    assert_eq!(n.frame_domain(Judgement::Perfect).unwrap(), (241..=254).collect::<Vec<_>>());
    assert_eq!(note(2.001, Middle).frame_domain(Judgement::Perfect).unwrap(), (239..=266).collect::<Vec<_>>());
    assert_eq!(note(0., Head).frame_domain(Judgement::Perfect).unwrap(), (0..=26).collect::<Vec<_>>());
    assert_eq!(note(600., FlickEnd).frame_domain(Judgement::Perfect).unwrap(), [72_000]);
    for time in [-0.01, 600.01, f64::INFINITY, f64::NAN] {
        assert!(note(time, Middle).frame_domain(Judgement::Bad).is_err());
    }
}

// Independent per-update state walk: the Move branch updates the nominal
// position for the next input, while Stop only increments its own counter.
// No ceil/floor or closed-form expiry is used by this reference.
#[test]
fn tail_domain_matches_strict_move_then_fourteen_f32_stop_updates() {
    let execute_frame = (1.0_f32 / 120.0) * 60.0;
    assert_eq!(execute_frame.to_bits(), 0.5_f32.to_bits());
    let times = [0., 1. / 120., 0.1125, f64::from_bits(2.0_f64.to_bits() - 1),
        2., f64::from_bits(2.0_f64.to_bits() + 1), 2.001, 2.006, 359.99, 599.99, 600.];
    for time in times {
        let nominal_frame = time * 120.;
        let (mut stopped, mut expired, mut counter) = (false, false, 0.0_f32);
        let mut expected = Vec::new();
        for frame in 0..=72_000 {
            // Input precedes the single state handler in this round.
            if !expired && f64::from(frame) >= nominal_frame { expected.push(frame); }
            if expired { continue; }
            if stopped {
                counter += execute_frame;
                expired = counter >= 7.;
            } else if f64::from(frame + 1) > nominal_frame {
                stopped = true;
            }
        }
        assert_eq!(note(time, FlickEnd).frame_domain(Judgement::Perfect).unwrap(), expected,
            "time={time:?}");
    }
}

fn input(window: NoteWindow, correction: Option<JudgementCorrection>, effect: ScoreEffect) -> ReplayInput {
    ReplayInput {
        power: 300_001.25, play_level: 27, fps: REPLAY_FPS,
        notes: vec![ReplayNote {time:1.,trigger:Some(0),window:NoteWindow::Standard},
            ReplayNote {time:4.,trigger:None,window}],
        hit_frames: vec![120,480], hit_order: vec![],
        skills: vec![ReplaySkill {duration:3.,score:effect,correction}],
        initial_combo: 51, medley_combo:true, fever_frames:None,
        constraints:vec![], native_time_routing:None,
    }
}

#[test]
fn typed_dp_matches_exhaustive_replay_across_skill_end_and_correction() {
    let windows = [NoteWindow::WidenedEndpoint, NoteWindow::SlideTime { role:Head, last_frame:None },
        NoteWindow::SlideTime { role:Middle, last_frame:None }, NoteWindow::SlideTime { role:End, last_frame:None },
        NoteWindow::SlideTime { role:FlickEnd, last_frame:None }, NoteWindow::SlideTime { role:DirectionalEnd, last_frame:None }];
    for window in windows {
        for correction in [None, Some(JudgementCorrection::Great), Some(JudgementCorrection::Good), Some(JudgementCorrection::Bad)] {
            for effect in [ScoreEffect::PerfectOnly {percent:115.},
                ScoreEffect::Continuous {high:120.,low:80.},
                ScoreEffect::Crescendo {percent:80.,step:10.,cap:150}] {
                let mut request = input(window, correction, effect);
                // Fixed skill frame puts its end inside the new note windows.
                let limits = [Some([120,121]), Some([462,508])];
                let actual = JointTimeline::maximum_in_frame_ranges(&request, &limits).unwrap().unwrap();
                let mut expected = None;
                for first in 120..=121 { for second in 462..=508 {
                    request.hit_frames = vec![first,second];
                    if let Ok(r) = replay(&request) { expected = Some(expected.unwrap_or(0).max(r.total)); }
                }}
                assert_eq!(Some(actual.result.total),expected,"{window:?} {correction:?} {effect:?}");
                request.hit_frames = actual.result.hit_frames.clone();
                request.hit_order = actual.result.hit_order.clone();
                assert_eq!(replay(&request).unwrap(),actual.result);
            }
        }
    }
}

#[test]
fn typed_node_gate_cannot_be_rescued_by_bad_correction() {
    for (role, frame) in [(Middle,477), (FlickEnd,479), (DirectionalEnd,479)] {
        let mut request = input(NoteWindow::SlideTime { role, last_frame:None }, Some(JudgementCorrection::Bad),
            ScoreEffect::Plain {percent:100.});
        request.hit_frames[1] = frame;
        assert!(matches!(replay(&request),Err(ReplayError::Judgement {raw:Judgement::Miss,..})));
    }
}
