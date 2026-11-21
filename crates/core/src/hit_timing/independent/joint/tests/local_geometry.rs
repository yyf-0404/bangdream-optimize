use super::*;

#[test]
fn cached_bad_layout_matches_frame_enumeration_with_fifo_corrections_and_special_windows() {
    let mut r = input(&[1., 1.04, 1.09, 1.5, 2., 4.],
        vec![skill(ScoreEffect::Plain { percent: 10. }); 3], 0);
    r.native_time_routing = None;
    r.notes[0].window = NoteWindow::WidenedEndpoint;
    r.notes[2].window = NoteWindow::NativeGreen {
        first_frames: [120, 124, 128, 130], last_frame: 144,
    };
    r.notes[3].window = NoteWindow::PinnedPerfect { frame: r.hit_frames[3] };
    r.notes[4].window = NoteWindow::SlideTime {
        role: crate::hit_timing::SlideTimeRole::End, last_frame:Some(272),
    };
    let bad = local::BadBounds::new(&r.notes).unwrap();
    for duration in [0.001, 0.1, 1., 4.] {
        for correction in [None, Some(JudgementCorrection::Great),
            Some(JudgementCorrection::Good), Some(JudgementCorrection::Bad)] {
            for s in &mut r.skills { s.duration = duration; s.correction = correction; }
            crate::hit_timing::validate(&r).unwrap();
            let new = local::geometry_with_bounds(&r, &bad, false).unwrap();
            let old_corrections = geometry::correction_ranges(&r).unwrap();
            let old_regions = local::regions(&r).unwrap();
            let old_protected: Vec<_> = r.notes.iter().map(|n| n.trigger.is_some()
                || n.frame_domain(Judgement::Bad).unwrap().iter()
                    .any(|&f| old_regions.iter().any(|&(a, b)| a <= f && f <= b))).collect();
            assert_eq!(new.corrections, old_corrections);
            assert_eq!(new.protected, old_protected);
            let mut key: Vec<_> = old_corrections.iter().map(|&(g, a, b)| (g as u8, a, b)).collect();
            key.push((u8::MAX, 0, 0));
            key.extend(old_protected.iter().enumerate().filter_map(|(i, &on)|
                on.then_some((u8::MAX, i as u32, 1))));
            assert_eq!(new.key, key);
        }
    }
}
use crate::hit_timing::{NoteConstraint, ScoreEffect};

#[test]
fn typed_slide_extended_timeout_survives_local_compaction_and_geometry_reuse() {
    let mut request=input(&[0.1,3.2],vec![skill(ScoreEffect::PerfectOnly {percent:120.})],1);
    request.native_time_routing=None;
    request.notes[1].window=NoteWindow::SlideTime {
        role:crate::hit_timing::SlideTimeRole::End,last_frame:Some(424),
    };
    compare(&request,brute(&request));
}

pub(super) fn compare(request: &ReplayInput, expected: u64) -> JointTimelineResult {
    let power = ScoringPower::new(request.power).unwrap();
    let local = JointTimeline::new_with_geometry(request, true).unwrap();
    let full = JointTimeline::new_with_geometry(request, false).unwrap();
    assert_eq!(full.score(power).unwrap(), expected);
    assert_eq!(local.score(power).unwrap(), expected);
    for compact in [false, true] {
        for prepared in [false, true] {
            let actual = super::prepared::answer(&local, power, prepared, compact).unwrap();
            assert_eq!(actual.0, expected);
            let mut replay_input = request.clone();
            replay_input.hit_frames = actual.1.frames;
            replay_input.hit_order = actual.1.order;
            assert_eq!(replay(&replay_input).unwrap().total, expected);
        }
    }
    let result = local.maximum(power).unwrap();
    assert_eq!(result.result.total, expected);
    result
}

#[test]
fn outside_transfers_match_exhaustive_for_skill_history_combo_and_fractional_power() {
    let profiles = profiles();
    for (i, skill) in profiles.iter().enumerate() {
        let mut request = input(&[0.1, 0.125, 0.23, 2., 2.025],
            vec![skill.clone(), profiles[(i + 4) % profiles.len()].clone()], 1);
        request.initial_combo = [19, 49, 297, 999][i % 4];
        request.medley_combo = i % 2 == 0;
        request.fever_frames = Some([200, 300]);
        for power in [2., 300040.75, f32::from_bits(300040.75_f32.to_bits() + 1)] {
            request.power = power;
            let result = compare(&request, brute(&request));
            assert_eq!(result.stats.collapsed_notes, 2);
            let full = JointTimeline::new_with_geometry(&request, false).unwrap();
            assert!(result.stats.graph_edges < full.stats.graph_edges);
        }
    }
}

#[test]
fn conservative_regions_cover_queued_skills_and_preserve_late_ordinary_hits() {
    let mut skills = vec![profiles()[5].clone(); 3];
    for skill in &mut skills { skill.duration = 0.5; }
    skills[1] = ReplaySkill { correction: Some(JudgementCorrection::Bad), ..skills[1].clone() };
    let request = input(&[0.1, 0.125, 0.15, 1.45, 2.72, 4.], skills, 1);
    let protected = super::super::local::protected(&request).unwrap();
    assert!(protected[3] && protected[4] && !protected[5]);
    let ranges = super::super::local::regions(&request).unwrap();
    let result = compare(&request, brute(&request));
    assert_eq!(result.stats.collapsed_notes, 1);
    for hit in &result.result.trace {
        if hit.active_skill.is_some() {
            assert!(ranges.iter().any(|&(a, b)| a <= hit.frame && hit.frame <= b));
        }
    }
}

fn long_connection() -> ReplayInput {
    let mut request = crate::hit_timing::time_connections::request();
    request.notes[0].time = 0.5;
    request.notes[1].time = 2.;
    request.notes[2].time = 1.25;
    request.hit_frames = vec![60, 240, 150];
    let routing = request.native_time_routing.as_mut().unwrap();
    routing.music_positions = (0..=300).map(|f| f as f32 * 10.).collect();
    for (target, pos) in routing.targets.iter_mut().zip([600, 2400, 1500]) {
        let target = target.as_mut().unwrap();
        target.absolute_pos = pos;
        target.active_from_frame = 0;
        target.active_until_frame = 300;
    }
    request
}

#[test]
fn closed_outside_green_connection_collapses_across_disjoint_note_components() {
    let request = long_connection();
    let result = compare(&request, brute(&request));
    assert_eq!(result.stats.collapsed_notes, 3);
    assert_eq!(result.stats.graph_edges, 1);
    assert_eq!(result.result.routing_coverage.time_connection_count, 1);
}

#[test]
fn green_binding_crossing_a_skill_region_keeps_its_boundary_state() {
    let mut request = long_connection();
    request.notes[2].trigger = Some(0);
    request.skills = vec![ReplaySkill { duration: 0.2, ..profiles()[4].clone() }];
    let result = compare(&request, brute(&request));
    assert_eq!(result.stats.collapsed_notes, 0);
    assert_eq!(result.result.routing_coverage.time_connection_count, 1);
}

#[test]
fn outside_fever_boundary_keeps_great_that_beats_unboosted_perfect() {
    let mut request = input(&[0.1], vec![], 13);
    request.fever_frames = Some([20, 28]);
    let result = compare(&request, brute(&request));
    assert_eq!(result.stats.collapsed_notes, 0);
    assert_eq!(result.result.trace[0].judgement, Judgement::Great);
    assert_eq!(result.result.trace[0].fever_multiplier, 2.);
}

#[test]
fn outside_constraint_without_an_all_perfect_completion_keeps_complete_search() {
    let mut request = input(&[0.1, 0.12], vec![], 8);
    // Perfect domains are [7, 17] and [10, 19] here. A 13-frame gap
    // requires a Great; the baseline is a legal Perfect/Great completion.
    request.hit_frames = vec![7, 20];
    request.constraints.push(NoteConstraint { before: 0, after: 1,
        minimum_gap_frames: 13, require_dispatch_order: false, earliest_after: None });
    let result = compare(&request, brute(&request));
    assert_eq!(result.stats.collapsed_notes, 0);
    assert!(result.result.trace.iter().any(|hit| hit.judgement == Judgement::Great));
}

#[test]
fn geometry_reuse_includes_protected_notes_and_shares_equal_masks() {
    let request = input(&[0.1, 0.3, 1.], vec![profiles()[0].clone()], 1);
    let local = JointTimeline::new_with_geometry(&request, true).unwrap();
    let mut skills = request.skills.clone();
    skills[0].duration = 0.11;
    skills[0].score = ScoreEffect::Continuous { high: 140., low: 60. };
    let shared = local.with_skills(&skills).unwrap();
    assert_eq!(shared.geometry_identity(), local.geometry_identity());
    skills[0].duration = 1.;
    let changed = local.with_skills(&skills).unwrap();
    assert_ne!(changed.geometry_identity(), local.geometry_identity());
    let mut request = request.clone();
    request.skills = skills;
    let expected = brute(&request);
    let power = ScoringPower::new(request.power).unwrap();
    assert_eq!(changed.maximum(power).unwrap().result.total, expected);
}

#[test]
fn outside_integer_aggregates_preserve_reachable_overflow_errors() {
    let request = input(&[0.1, 0.125], vec![], 1);
    let local = JointTimeline::new_with_geometry(&request, true).unwrap();
    let full = JointTimeline::new_with_geometry(&request, false).unwrap();
    assert_eq!(local.stats.collapsed_notes, 2);
    let power = ScoringPower::new(4.0e9).unwrap();
    assert_eq!(local.score(power).unwrap_err().to_string(), full.score(power).unwrap_err().to_string());
}
