use super::*;
use crate::hit_timing::{CandidateDistanceMode, NoteConstraint, ScoreEffect};

fn model(request: &ReplayInput, dag: bool) -> JointTimeline {
    let mut model = JointTimeline::new_with_geometry(request, false).unwrap();
    let prepared = Prepared::new(request).unwrap();
    let mut stats = WindowStats::default();
    let graph = prepared.compile(&prepared.joint_row(), false,
        WindowOptions { touch_dag: dag, ..options() }, &mut stats).unwrap();
    let actions = PreparedActions::new(request, &graph).unwrap();
    model.geometry_bytes = JointTimeline::geometry_footprint(&graph, &actions);
    model.graph = Arc::new(graph);
    model.prepared_actions = Arc::new(actions);
    model.stats = stats;
    model
}

fn check(request: &ReplayInput) -> (WindowStats, WindowStats) {
    let expected = brute(request);
    let power = ScoringPower::new(request.power).unwrap();
    let mut measured = vec![];
    for dag in [false, true] {
        let model = model(request, dag);
        assert_eq!(model.graph.touches.is_empty(), !dag);
        for compact in [false, true] {
            for precompute in [false, true] {
                let mut costs = JointCosts::new(&model, power).unwrap();
                if !precompute { costs.score_table = None; }
                let mut stats = WindowStats::default();
                let (value, plan) = solve::evaluate_ordered(
                    &model.graph, &mut costs, WindowObjective::Native,
                    WindowOptions { compact_value_states: compact, ..options() }, &mut stats,
                ).unwrap().unwrap().unwrap();
                assert_eq!(value.score, expected, "dag={dag}, compact={compact}");
                let mut replay_input = request.clone();
                replay_input.hit_frames = plan.frames;
                replay_input.hit_order = plan.order;
                assert_eq!(replay(&replay_input).unwrap().total, expected);
                if compact && precompute { measured.push(stats); }
                let mut costs = JointCosts::new(&model, power).unwrap();
                let value = solve::evaluate_value(&model.graph, &mut costs, WindowObjective::Native,
                    WindowOptions { compact_value_states: compact, ..options() },
                    &mut WindowStats::default()).unwrap().unwrap().unwrap();
                assert_eq!(value.score, expected);
            }
        }
        assert_eq!(model.maximum(power).unwrap().result.total, expected);
    }
    (measured.remove(0), measured.remove(0))
}

#[test]
fn touch_dag_removes_padding_and_repeated_edges_from_one_frame() {
    let mut request = input(&[0.1, 1., 1., 1.], vec![ReplaySkill {
        duration: 2., score: ScoreEffect::Plain { percent: 100. }, correction: None,
    }], 0);
    for (target, &frame) in request.native_time_routing.as_mut().unwrap().targets.iter_mut()
        .zip(&request.hit_frames) {
        let target = target.as_mut().unwrap();
        target.active_from_frame = frame;
        target.active_until_frame = frame;
    }
    let (old, new) = check(&request);
    assert_eq!(old.transitions, 40);
    assert_eq!(new.transitions, 17);
    assert_eq!(new.touch_hit_transitions, 13);
    assert_eq!(new.touch_entry_transitions, 2);
    assert_eq!(new.touch_dag_layers, 2);
    assert!(new.peak_value_states <= old.peak_value_states);
    let old = model(&request, false);
    let new = model(&request, true);
    assert!(new.footprint() < old.footprint());
}

#[test]
fn touch_dag_retains_native_progress_and_combo_order_for_all_profiles() {
    for (i, mut profile) in profiles().into_iter().enumerate() {
        profile.duration = 2.;
        let mut request = input(&[0.1, 1., 1., 1.], vec![profile], 1);
        request.initial_combo = [19, 49, 299, 999][i % 4];
        request.medley_combo = i % 2 != 0;
        request.power = f32::from_bits(request.power.to_bits() + (i % 3) as u32);
        request.fever_frames = Some([120, 122]);
        let (old, new) = check(&request);
        assert!(new.transitions < old.transitions);
    }
}

#[test]
fn touch_dag_orders_queued_triggers_and_keeps_all_entry_consumed_counts() {
    let request = input(&[0.1, 0.125, 0.215, 0.99],
        vec![profiles()[5].clone(), profiles()[4].clone(), profiles()[0].clone()], 2);
    let (old, new) = check(&request);
    assert!(new.transitions < old.transitions);
}

#[test]
fn touch_dag_keeps_pending_timestamps_and_green_finger_bindings() {
    let mut request = input(&[0.1, 1., 1.025], vec![profiles()[0].clone()], 3);
    request.constraints.push(NoteConstraint {
        before: 1, after: 2, minimum_gap_frames: 2,
        require_dispatch_order: true, earliest_after: None,
    });
    request.hit_frames[1] = 118;
    request.hit_frames[2] = 123;
    check(&request);
    let request = super::super::super::super::time_connections::request();
    check(&request);
}

#[test]
fn touch_dag_preserves_complete_bad_domains_and_same_lane_tie_order() {
    for reverse in [false, true] {
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
        if reverse { routing.candidate_order.reverse(); }
        request.fever_frames = Some([119, 125]);
        check(&request);
    }
}

#[test]
fn touch_dag_reports_incomplete_value_and_geometry_budgets() {
    let request = input(&[0.1, 0.21, 0.215], vec![profiles()[0].clone()], 1);
    let model = model(&request, true);
    let power = ScoringPower::new(request.power).unwrap();
    for compact in [false, true] {
        let mut costs = JointCosts::new(&model, power).unwrap();
        let mut stats = WindowStats::default();
        let result = solve::evaluate_ordered(&model.graph, &mut costs, WindowObjective::Native,
            WindowOptions { compact_value_states: compact, max_transitions: 1, ..options() },
            &mut stats).unwrap();
        assert!(result.is_none());
        assert_eq!(stats.transitions, 2);
        let mut costs = JointCosts::new(&model, power).unwrap();
        let result = solve::evaluate_value(&model.graph, &mut costs, WindowObjective::Native,
            WindowOptions { compact_value_states: compact, max_states: 1, ..options() },
            &mut WindowStats::default()).unwrap();
        assert!(result.is_none());
    }
    let prepared = Prepared::new(&request).unwrap();
    assert!(prepared.compile(&prepared.joint_row(), false,
        WindowOptions { max_graph_edges: 1, ..options() }, &mut WindowStats::default()).is_none());
}
