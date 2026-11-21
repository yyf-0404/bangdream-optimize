use super::*;
use crate::hit_timing::ScoreEffect;

fn brute_ranges(input: &ReplayInput, limits: &[Option<[u32; 2]>]) -> Option<u64> {
    fn visit(i: usize, input: &mut ReplayInput, choices: &[Vec<u32>], best: &mut Option<u64>) {
        if i == choices.len() {
            if let Some(value) = crate::hit_timing::touch_order_tests::best_replay(input) {
                *best = Some(best.map_or(value.total, |b| b.max(value.total)));
            }
        } else {
            for &frame in &choices[i] {
                input.hit_frames[i] = frame;
                visit(i + 1, input, choices, best);
            }
        }
    }
    let choices = search::frame_domains(input).unwrap().into_iter().zip(limits)
        .map(|(frames, range)| frames.into_iter().filter(|&f|
            range.is_none_or(|[a, b]| a <= f && f <= b)).collect()).collect::<Vec<_>>();
    let mut best = None;
    visit(0, &mut input.clone(), &choices, &mut best);
    best
}

#[test]
fn restricted_frames_match_cartesian_replay_with_dynamic_skills_and_touch_order() {
    let request = input(&[0.5, 0.5, 0.525], vec![skill(ScoreEffect::Continuous { high: 140., low: 60. })], 2);
    for first in [58, 60, 62] {
        for last in [61, 63, 65] {
            let limits = [Some([first, first]), Some([59, 61]), Some([last, last])];
            let expected = brute_ranges(&request, &limits);
            let actual = JointTimeline::maximum_in_frame_ranges(&request, &limits).unwrap();
            assert_eq!(actual.map(|r| r.result.total), expected, "{limits:?}");
        }
    }
}

#[test]
fn presentation_matches_exhaustive_scores_offsets_and_touch_orders() {
    use std::cmp::Reverse;
    for effect in [
        ScoreEffect::Plain { percent: 110. },
        ScoreEffect::Continuous { high: 140., low: 60. },
        ScoreEffect::Crescendo { percent: 100., step: 10., cap: 130 },
    ] {
        let mut request = input(&[0.5, 0.5, 0.6], vec![skill(effect)], 2);
        let references = request.chart_reference_frames().unwrap();
        for limits in [[None; 3], [Some([58, 59]), None, Some([72, 74])]] {
            let winner = JointTimeline::maximum_in_frame_ranges(&request, &limits).unwrap().unwrap().result;
            let best = JointTimeline::simplest_maximum(&request, &limits, &winner).unwrap();
            let rank = |result: &ReplayResult| {
                let offsets = result.hit_frames.iter().zip(&references).map(|(&f, &r)| f.abs_diff(r));
                (result.total, Reverse(offsets.clone().max().unwrap()), Reverse(offsets.filter(|&d| d > 0).count()))
            };
            let mut expected = None;
            for a in 58..=62 { for b in 58..=62 { for c in 70..=74 {
                request.hit_frames = vec![a, b, c];
                if request.hit_frames.iter().zip(limits).any(|(&f,r)| r.is_some_and(|[lo,hi]| f < lo || f > hi)) {
                    continue;
                }
                if let Some(result) = crate::hit_timing::touch_order_tests::best_replay(&request) {
                    expected = expected.max(Some(rank(&result)));
                }
            }}}
            assert_eq!(Some(rank(&best)), expected);
            request.hit_frames.clone_from(&references);
        }
    }
}

#[test]
fn independent_skills_remain_separate_across_a_dense_ordinary_passage() {
    let times: Vec<_> = (60..=390).step_by(10).map(|f| f64::from(f) / 120.).collect();
    let effect = skill(ScoreEffect::Plain { percent: 100. });
    let mut request = input(&times, vec![effect.clone(), effect], 2);
    request.native_time_routing = None;
    request.notes[1].trigger = None;
    let last = request.notes.len() - 1;
    request.notes[last].trigger = Some(1);
    let mut prepared = Prepared::new(&request).unwrap();
    let row = prepared.joint_row();
    assert_eq!(row.timing_sections(&request).unwrap().len(), 1);
    let pinned = prepared.pin_section_references(&row, &request.chart_reference_frames().unwrap(),
        local::protected(&request).unwrap()).unwrap();
    assert!(!pinned.timing_sections(&request).unwrap().iter().any(|s| s.contains(&0) && s.contains(&last)));
}

#[test]
fn unrestricted_query_stays_identical_and_a_displaced_baseline_is_allowed() {
    let request = input(&[0.5, 0.6, 0.7], vec![skill(ScoreEffect::Plain { percent: 100. })], 2);
    let expected = JointTimeline::new(&request).unwrap().maximum(ScoringPower::new(request.power).unwrap()).unwrap();
    let actual = JointTimeline::maximum_in_frame_ranges(&request, &[None; 3]).unwrap().unwrap();
    assert_eq!(actual.result, expected.result);
    let limits = [Some([58, 58]), None, None];
    let displaced = JointTimeline::maximum_in_frame_ranges(&request, &limits).unwrap().unwrap();
    assert_eq!(displaced.result.hit_frames[0], 58);
    assert_eq!(displaced.result.total, brute_ranges(&request, &limits).unwrap());
}

#[test]
fn restrictions_never_shorten_the_candidate_lifetime_of_unhit_notes() {
    let mut request = input(&[0.5, 0.525], vec![], 4);
    let bad: Vec<_> = request.notes.iter().map(|n| n.frame_domain(crate::scoring::Judgement::Bad).unwrap()).collect();
    let routing = request.native_time_routing.as_mut().unwrap();
    routing.distance_mode = crate::hit_timing::CandidateDistanceMode::ChartTime;
    routing.music_positions.clear();
    for (i, target) in routing.targets.iter_mut().enumerate() {
        let target = target.as_mut().unwrap();
        target.button = 0; target.absolute_pos = 0;
        target.active_from_frame = bad[i][0];
        target.active_until_frame = *bad[i].last().unwrap();
    }
    request.hit_frames = vec![60, 63];
    let limits = [Some([64, 64]), Some([60, 60])];
    assert_eq!(brute_ranges(&request, &limits), None);
    assert!(JointTimeline::maximum_in_frame_ranges(&request, &limits).unwrap().is_none());
}

#[test]
fn empty_and_contradictory_green_domains_return_infeasible() {
    let mut request = input(&[0.5, 0.525], vec![], 2);
    request.constraints.push(crate::hit_timing::NoteConstraint {
        before: 0, after: 1, minimum_gap_frames: 1,
        require_dispatch_order: true, earliest_after: None,
    });
    for limits in [[Some([0, 0]), None], [Some([62, 62]), Some([61, 61])]] {
        assert!(JointTimeline::maximum_in_frame_ranges(&request, &limits).unwrap().is_none());
    }
    assert!(JointTimeline::maximum_in_frame_ranges(&request, &[Some([62, 60]), None]).is_err());
    assert!(JointTimeline::maximum_in_frame_ranges(&request, &[]).is_err());
}

#[test]
fn outside_skill_restrictions_keep_forced_great_and_match_full_geometry() {
    let request = input(&[0.5, 0.6, 0.7], vec![], 12);
    let limits = [Some([69, 69]), Some([71, 73]), Some([83, 85])];
    let restricted = JointTimeline::maximum_in_frame_ranges(&request, &limits).unwrap().unwrap();
    assert_eq!(restricted.result.hit_frames[0], 69);
    assert_eq!(restricted.result.trace.iter().find(|h| h.note_index == 0).unwrap().raw,
        crate::scoring::Judgement::Great);
    let prepared = geometry::Prepared::new(&request).unwrap();
    let row = prepared.restrict_row(prepared.joint_row(), &limits).unwrap().unwrap();
    let full = JointTimeline::from_row(&request, &prepared, row, false, false).unwrap()
        .maximum(ScoringPower::new(request.power).unwrap()).unwrap();
    assert_eq!(restricted.result.total, full.result.total);
    assert_eq!(Some(restricted.result.total), brute_ranges(&request, &limits));
}
