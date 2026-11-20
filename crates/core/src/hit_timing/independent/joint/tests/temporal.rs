use super::*;
use crate::hit_timing::{CandidateDistanceMode, NoteConstraint};

fn ordinary(profile: ReplaySkill, radius: u32) -> ReplayInput {
    let mut request = input(&[0.1, 1., 1., 1.], vec![profile], radius);
    let routing = request.native_time_routing.as_mut().unwrap();
    routing.targets[0].as_mut().unwrap().active_from_frame = 12;
    routing.targets[0].as_mut().unwrap().active_until_frame = 12;
    request.skills[0].duration = 2.;
    request
}

fn compare(model: &JointTimeline, request: &ReplayInput, expected: u64) -> (WindowStats, WindowStats) {
    let power = ScoringPower::new(request.power).unwrap();
    let mut measured = vec![];
    for compact in [false, true] {
        for precompute in [false, true] {
            let mut costs = JointCosts::new(model, power).unwrap();
            if !precompute { costs.score_table = None; }
            let mut stats = WindowStats::default();
            let (value, plan) = solve::evaluate_ordered(
                &model.graph, &mut costs, WindowObjective::Native,
                WindowOptions { compact_value_states: compact, ..options() }, &mut stats,
            ).unwrap().unwrap().unwrap();
            assert_eq!(value.score, expected);
            let mut witness = request.clone();
            witness.hit_frames = plan.frames;
            witness.hit_order = plan.order;
            assert_eq!(replay(&witness).unwrap().total, expected);
            if precompute { measured.push(stats); }
        }
    }
    assert_eq!(model.score(power).unwrap(), expected);
    assert_eq!(model.maximum(power).unwrap().result.total, expected);
    (measured.remove(0), measured.remove(0))
}

#[test]
fn interval_closure_merges_translated_paths_and_preserves_native_progress_scores() {
    for (i, profile) in profiles().into_iter().enumerate() {
        let mut request = ordinary(profile, 3);
        request.initial_combo = [19, 49, 299, 999][i % 4];
        request.medley_combo = i % 2 != 0;
        request.power = f32::from_bits(request.power.to_bits() + (i % 2) as u32);
        let model = JointTimeline::new_with_geometry(&request, false).unwrap();
        let (old, new) = compare(&model, &request, brute(&request));
        assert!(new.temporal_skipped_frames > 0);
        assert!(new.transitions < old.transitions);
        assert!(new.value_graph_edges < model.stats.graph_edges);
        assert!(new.peak_value_states <= old.peak_value_states);
    }
}

#[test]
fn interval_closure_keeps_grade_fever_and_combo_order_decisions() {
    let mut request = ordinary(profiles()[4].clone(), 6);
    request.notes[2].time = 1.025;
    request.hit_frames[2] = 123;
    request.native_time_routing.as_mut().unwrap().targets[2] = Some(NativeTimeTarget {
        button: 2, absolute_pos: 1230, active_from_frame: 117, active_until_frame: 129,
    });
    request.native_time_routing.as_mut().unwrap().music_positions =
        (0..=135).map(|f| f as f32 * 10.).collect();
    request.initial_combo = 49;
    request.fever_frames = Some([120, 124]);
    let model = JointTimeline::new_with_geometry(&request, false).unwrap();
    let (_, new) = compare(&model, &request, brute(&request));
    assert!(new.temporal_skipped_frames > 0);
}

#[test]
fn interval_closure_preserves_nearest_root_switches_and_list_ties() {
    for reverse in [false, true] {
        let mut request = ordinary(profiles()[0].clone(), 3);
        let routing = request.native_time_routing.as_mut().unwrap();
        routing.targets[1].as_mut().unwrap().button = 1;
        routing.targets[2].as_mut().unwrap().button = 1;
        routing.targets[3].as_mut().unwrap().button = 1;
        if reverse {
            routing.candidate_order.reverse();
            request.hit_order = vec![0, 3, 2, 1];
        }
        let model = JointTimeline::new_with_geometry(&request, false).unwrap();
        let (_, new) = compare(&model, &request, brute(&request));
        assert!(new.temporal_skipped_frames > 0);
    }
}

#[test]
fn interval_closure_matches_full_bad_time_distance_domains_and_nearest_switches() {
    let mut profile = profiles()[4].clone();
    profile.duration = 2.;
    let mut request = input(&[0.1, 1., 1.025], vec![profile], 0);
    let routing = request.native_time_routing.as_mut().unwrap();
    routing.distance_mode = CandidateDistanceMode::ChartTime;
    routing.music_positions.clear();
    for (note, target) in request.notes.iter().zip(&mut routing.targets) {
        let target = target.as_mut().unwrap();
        let frames = note.frame_domain(Judgement::Bad).unwrap();
        target.absolute_pos = 0;
        target.active_from_frame = frames[0];
        target.active_until_frame = *frames.last().unwrap();
    }
    routing.targets[2].as_mut().unwrap().button = 1;
    request.fever_frames = Some([119, 125]);
    let model = JointTimeline::new_with_geometry(&request, false).unwrap();
    let (_, new) = compare(&model, &request, brute(&request));
    assert!(new.temporal_skipped_frames > 0);
}

#[test]
fn interval_clock_proof_is_rechecked_when_duration_reuses_geometry() {
    let request = ordinary(profiles()[0].clone(), 3);
    let model = JointTimeline::new_with_geometry(&request, true).unwrap();
    let mut changed = request.clone();
    changed.skills[0].duration = 0.9;
    let reused = model.with_skills(&changed.skills).unwrap();
    assert_eq!(model.geometry_identity(), reused.geometry_identity());
    let (_, new) = compare(&reused, &changed, brute(&changed));
    assert!(new.temporal_clock_fallbacks > 0);
    let fresh = JointTimeline::new_with_geometry(&changed, true).unwrap();
    assert_eq!(reused.score(ScoringPower::new(changed.power).unwrap()).unwrap(),
        fresh.score(ScoringPower::new(changed.power).unwrap()).unwrap());
}

#[test]
fn interval_closure_retains_exact_pending_gaps_and_green_bindings() {
    let mut request = ordinary(profiles()[0].clone(), 3);
    request.constraints.push(NoteConstraint {
        before: 1, after: 2, minimum_gap_frames: 2,
        require_dispatch_order: true, earliest_after: None,
    });
    request.hit_frames[1] = 118;
    request.hit_frames[2] = 120;
    let model = JointTimeline::new_with_geometry(&request, false).unwrap();
    compare(&model, &request, brute(&request));
    let request = super::super::super::super::time_connections::request();
    let model = JointTimeline::new_with_geometry(&request, false).unwrap();
    compare(&model, &request, brute(&request));
}

#[test]
fn interval_clock_proof_respects_input_before_start_inclusive_end_and_ready() {
    let request = ordinary(profiles()[0].clone(), 3);
    let model = JointTimeline::new_with_geometry(&request, false).unwrap();
    let mut costs = JointCosts::new(&model, ScoringPower::new(request.power).unwrap()).unwrap();
    for (start, end, ready, expected) in [
        (9, 20, 21, true), (10, 20, 21, false), (20, 30, 31, true),
        (0, 9, 10, true), (0, 10, 21, false), (0, 19, 21, false),
        (0, 20, 10, true), (0, 20, 11, false), (0, 20, 20, false),
    ] {
        let state = costs.intern_state(State {
            ready, windows: Rc::new(vec![Window { start, end, slot: 0 }]), progress: 0,
        }, 0);
        assert_eq!(costs.stable_interval(state, 10, 20), expected,
            "start={start}, end={end}, ready={ready}");
    }
}
