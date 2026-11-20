use super::*;
use super::super::prepared::MAX_SCORE_CELLS;

pub(super) fn answer(
    model: &JointTimeline,
    power: ScoringPower,
    prepared: bool,
    compact: bool,
) -> Result<(u64, solve::HitPlan), ReplayError> {
    let mut costs = JointCosts::new(model, power)?;
    costs.score_table = prepared
        .then(|| ScoreTable::new(model, costs.scorer, MAX_SCORE_CELLS))
        .flatten();
    let (score, plan) = solve::evaluate_ordered(
        &model.graph, &mut costs, WindowObjective::Native,
        WindowOptions { compact_value_states: compact, ..options() },
        &mut WindowStats::default(),
    )?.unwrap().unwrap();
    Ok((score.score, plan))
}

#[test]
fn prepared_edges_preserve_complete_witnesses_across_skills_combo_and_fever() {
    let profiles = profiles();
    for (i, skill) in profiles.iter().enumerate() {
        let mut request = input(&[0.1, 0.15, 0.22, 1.1],
            vec![skill.clone(), profiles[(i + 4) % profiles.len()].clone()], 3);
        request.initial_combo = [19, 49, 299, 999][i % 4];
        request.medley_combo = i % 2 == 0;
        request.fever_frames = Some([24, 30]);
        request.notes[3].time -= 0.05; // Include both raw Great and Perfect.
        let model = JointTimeline::new(&request).unwrap();
        for power in [2., 300040.75, f32::from_bits(300040.75_f32.to_bits() + 1)] {
            request.power = power;
            let power = ScoringPower::new(power).unwrap();
            let expected = brute(&request);
            for compact in [false, true] {
                let control = answer(&model, power, false, compact).unwrap();
                let actual = answer(&model, power, true, compact).unwrap();
                assert_eq!(actual, control, "profile={i}, compact={compact}");
                assert_eq!(actual.0, expected);
                let mut replay_input = request.clone();
                replay_input.hit_frames = actual.1.frames;
                replay_input.hit_order = actual.1.order;
                assert_eq!(replay(&replay_input).unwrap().total, expected);
            }
        }
    }
}

#[test]
fn prepared_edges_share_geometry_but_rebuild_skill_scores_and_defer_errors() {
    // This skill starts after the only note. Its score table overflows, but no
    // reachable edge consumes an overflowing cell, so the query must succeed.
    let mut request = input(&[0.1], vec![skill(crate::hit_timing::ScoreEffect::Plain {
        percent: 1.0e12,
    })], 0);
    request.notes[0].window = NoteWindow::PinnedPerfect { frame: 12 };
    request.native_time_routing = None;
    let model = JointTimeline::new(&request).unwrap();
    let power = ScoringPower::new(request.power).unwrap();
    let table = ScoreTable::new(&model, NoteScorer::new(power, 27, 1).unwrap(),
        MAX_SCORE_CELLS).unwrap();
    assert!(table.score(0, Some(0), 0, Judgement::Perfect).is_err());
    assert_eq!(answer(&model, power, true, true), answer(&model, power, false, true));
    assert_eq!(model.score(power).unwrap(), replay(&request).unwrap().total);
    let overflowing = ScoringPower::new(2.0e9).unwrap();
    let fast_error = answer(&model, overflowing, true, true).unwrap_err();
    let scalar_error = answer(&model, overflowing, false, true).unwrap_err();
    assert_eq!(fast_error, scalar_error);
    assert!(ScoreTable::new(&model, NoteScorer::new(power, 27, 1).unwrap(), 0).is_none());

    let request = input(&[0.1, 0.15, 0.22, 1.1], vec![profiles()[0].clone()], 3);
    let model = JointTimeline::new(&request).unwrap();
    for skill in profiles() {
        let mut changed = request.clone();
        changed.skills = vec![skill];
        let reused = model.with_skills(&changed.skills).unwrap();
        let fresh = JointTimeline::new(&changed).unwrap();
        assert_eq!(answer(&reused, power, true, true), answer(&fresh, power, false, true));
    }
}
