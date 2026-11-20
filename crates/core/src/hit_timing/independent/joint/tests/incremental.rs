use super::*;
use crate::hit_timing::{CandidateDistanceMode, NoteConstraint, ScoreEffect};

fn model(request: &ReplayInput) -> JointTimeline {
    let mut model = JointTimeline::new_with_geometry(request, false).unwrap();
    let prepared = Prepared::new(request).unwrap();
    let mut stats = WindowStats::default();
    let graph = prepared.compile(&prepared.joint_row(), false,
        WindowOptions { touch_dag: true, incremental_touch: true, ..options() }, &mut stats).unwrap();
    let actions = PreparedActions::new(request, &graph).unwrap();
    model.geometry_bytes = JointTimeline::geometry_footprint(&graph, &actions);
    model.graph = Arc::new(graph);
    model.prepared_actions = Arc::new(actions);
    model.stats = stats;
    model
}

fn run(model: &JointTimeline, request: &ReplayInput, costs: &mut impl ActionCosts,
    incremental: bool, expected: u64) -> WindowStats {
    let options = WindowOptions { incremental_touch: incremental, ..options() };
    let mut stats = WindowStats::default();
    let (value, plan) = solve::evaluate_ordered(&model.graph, costs, WindowObjective::Native,
        options, &mut stats).unwrap().unwrap().unwrap();
    assert_eq!(value.score, expected);
    let mut witness = request.clone();
    witness.hit_frames = plan.frames;
    witness.hit_order = plan.order;
    assert_eq!(replay(&witness).unwrap().total, expected);
    stats
}

fn check(request: &ReplayInput) -> (WindowStats, WindowStats) {
    let expected = brute(request);
    let power = ScoringPower::new(request.power).unwrap();
    let model = model(request);
    let mut measured = vec![];
    for clock in [false, true] {
        for incremental in [false, true] {
            for prepared in [false, true] {
                let mut costs = JointCosts::new(&model, power).unwrap();
                if !prepared { costs.score_table = None; }
                let stats = if clock {
                    run(&model, request, &mut clock::ClockCosts::new(costs), incremental, expected)
                } else { run(&model, request, &mut costs, incremental, expected) };
                if !clock && prepared { measured.push(stats); }
            }
            let costs = JointCosts::new(&model, power).unwrap();
            let value = if clock {
                solve::evaluate_value(&model.graph, &mut clock::ClockCosts::new(costs),
                    WindowObjective::Native,
                    WindowOptions { incremental_touch: incremental, ..options() },
                    &mut WindowStats::default())
            } else {
                solve::evaluate_value(&model.graph, &mut { costs }, WindowObjective::Native,
                    WindowOptions { incremental_touch: incremental, ..options() },
                    &mut WindowStats::default())
            }.unwrap().unwrap().unwrap();
            assert_eq!(value.score, expected);
        }
    }
    (measured.remove(0), measured.remove(0))
}

fn ordinary(profile: ReplaySkill) -> ReplayInput {
    let mut request = input(&[0.1, 1., 1.025], vec![profile], 3);
    request.skills[0].duration = 2.;
    let target = request.native_time_routing.as_mut().unwrap().targets[0].as_mut().unwrap();
    target.active_from_frame = 12;
    target.active_until_frame = 12;
    request
}

#[test]
fn incremental_closure_retains_native_progress_combo_fever_and_all_grades() {
    for (i, profile) in profiles().into_iter().enumerate() {
        let mut request = ordinary(profile);
        request.initial_combo = [19, 49, 299, 999][i % 4];
        request.medley_combo = i % 2 != 0;
        request.power = f32::from_bits(request.power.to_bits() + (i % 3) as u32);
        request.fever_frames = Some([120, 123]);
        let (old, new) = check(&request);
        assert!(new.incremental_skipped_transitions > 0);
        assert!(new.transitions < old.transitions);
        assert!(new.peak_value_states <= old.peak_value_states);
        assert_eq!(new.incremental_candidate_transitions, new.incremental_skipped_transitions
            + new.incremental_changed_transitions + new.incremental_clock_fallback_transitions);
    }
}

#[test]
fn incremental_closure_propagates_improved_prefixes_after_candidate_changes() {
    let mut request = ordinary(skill(ScoreEffect::Crescendo { percent: 100., step: 0.5, cap: 150 }));
    request.initial_combo = 49;
    request.fever_frames = Some([119, 123]);
    let routing = request.native_time_routing.as_mut().unwrap();
    routing.targets[1].as_mut().unwrap().active_from_frame = 113;
    routing.targets[1].as_mut().unwrap().active_until_frame = 127;
    routing.targets[2].as_mut().unwrap().active_from_frame = 114;
    routing.targets[2].as_mut().unwrap().active_until_frame = 132;
    routing.music_positions = (0..=135).map(|f| f as f32 * 10.).collect();
    let (_, new) = check(&request);
    assert!(new.incremental_changed_transitions > 0);
    assert!(new.incremental_new_transitions > 0);
    assert!(new.incremental_skipped_transitions > 0);
}

#[test]
fn incremental_closure_rechecks_skill_start_end_and_cooling_boundaries() {
    let mut request = input(&[0.1, 0.125, 0.15, 0.175],
        vec![profiles()[4].clone(), profiles()[5].clone()], 3);
    request.skills[0].duration = 0.03;
    request.skills[1].duration = 0.03;
    let (_, new) = check(&request);
    assert!(new.incremental_clock_fallback_transitions > 0);
}

#[test]
fn incremental_closure_keeps_pending_gaps_retirement_merges_and_green_bindings() {
    let mut request = ordinary(profiles()[0].clone());
    request.constraints.push(NoteConstraint { before: 1, after: 2, minimum_gap_frames: 1,
        require_dispatch_order: true, earliest_after: None });
    request.hit_frames[1] = 118;
    request.hit_frames[2] = 123;
    check(&request);
    let request = super::super::super::super::time_connections::request();
    check(&request);
}

#[test]
fn incremental_closure_preserves_complete_bad_windows_and_same_lane_switches() {
    for reverse in [false, true] {
        let mut request = ordinary(profiles()[4].clone());
        let routing = request.native_time_routing.as_mut().unwrap();
        routing.distance_mode = CandidateDistanceMode::ChartTime;
        routing.music_positions.clear();
        for (note, target) in request.notes.iter().zip(&mut routing.targets) {
            let target = target.as_mut().unwrap();
            let frames = note.frame_domain(Judgement::Bad).unwrap();
            target.active_from_frame = frames[0];
            target.active_until_frame = *frames.last().unwrap();
            target.absolute_pos = 0;
        }
        routing.targets[2].as_mut().unwrap().button = 1;
        if reverse { routing.candidate_order.reverse(); }
        request.fever_frames = Some([119, 125]);
        check(&request);
    }
}

#[test]
fn factored_clocks_reuse_geometry_but_recompute_every_duration_and_power() {
    let request = ordinary(profiles()[5].clone());
    let base = model(&request);
    for duration in [0.03, 0.9, 2.] {
        let mut changed = request.clone();
        changed.skills[0].duration = duration;
        changed.power = f32::from_bits(changed.power.to_bits() + 1);
        let reused = base.with_skills(&changed.skills).unwrap();
        assert_eq!(reused.geometry_identity(), base.geometry_identity());
        let power = ScoringPower::new(changed.power).unwrap();
        let expected = brute(&changed);
        for incremental in [false, true] {
            let costs = JointCosts::new(&reused, power).unwrap();
            run(&reused, &changed, &mut clock::ClockCosts::new(costs), incremental, expected);
        }
    }
}

#[test]
fn incremental_and_factored_queries_preserve_explicit_budget_failure() {
    let request = ordinary(profiles()[0].clone());
    let model = model(&request);
    let power = ScoringPower::new(request.power).unwrap();
    for limit in [WindowOptions { max_transitions: 1, ..options() },
        WindowOptions { max_states: 1, ..options() }] {
        let mut costs = clock::ClockCosts::new(JointCosts::new(&model, power).unwrap());
        assert!(solve::evaluate_value(&model.graph, &mut costs, WindowObjective::Native,
            limit, &mut WindowStats::default()).unwrap().is_none());
    }
}
