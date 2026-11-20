use super::*;
use crate::hit_timing::{NoteConstraint, ScoreEffect};

#[test]
fn frozen_geometry_preserves_native_values_ties_frames_and_dispatch_witnesses() {
    for local in [false, true] { for correction in [None, Some(JudgementCorrection::Bad)] {
        for effect in [ScoreEffect::Plain { percent: 100. },
            ScoreEffect::Continuous { high: 150., low: 80. },
            ScoreEffect::Crescendo { percent: 100., step: 0.5, cap: 130 }] {
            let mut request = input(&[0.2,0.8,0.8,0.81,1.8,2.8], vec![ReplaySkill {
                duration: 0.15, score: effect, correction }], 1);
            request.native_time_routing = None;
            request.initial_combo = 699;
            request.medley_combo = true;
            request.fever_frames = Some([96,120]);
            request.constraints.push(NoteConstraint { before: 1, after: 2,
                minimum_gap_frames: 0, require_dispatch_order: true, earliest_after: None });
            let model = JointTimeline::new_with_geometry(&request, local).unwrap();
            let prepared = Prepared::new(&request).unwrap();
            let mut stats = WindowStats::default();
            let original = if local {
                prepared.compile_local(&prepared.joint_row(), &local::protected(&request).unwrap(), options(), &mut stats)
            } else { prepared.compile(&prepared.joint_row(), false, options(), &mut stats) }.unwrap();
            assert!(JointTimeline::geometry_footprint(&model.graph, &model.prepared_actions)
                < JointTimeline::geometry_footprint(&original, &model.prepared_actions));
            assert_eq!(original.actions.len(), model.graph.actions.len());
            for (a,b) in original.actions.iter().zip(&model.graph.actions) {
                assert_eq!(&*a.hits, &*b.hits);
                assert_eq!((a.factor.perfect,a.factor.inside,a.factor.slot,a.factor.fever,
                    a.factor.rank,a.factor.reference.to_bits()),
                    (b.factor.perfect,b.factor.inside,b.factor.slot,b.factor.fever,
                    b.factor.rank,b.factor.reference.to_bits()));
                assert_eq!(matches!(a.hits,geometry::ActionHits::Run(_)),
                    matches!(b.hits,geometry::ActionHits::Run(_)));
            }
            for (a,b) in original.layers.iter().zip(&model.graph.layers) {
                assert_eq!(a.count,b.count);
                assert_eq!(a.edges.iter().map(|e| (e.from,e.to,e.action)).collect::<Vec<_>>(),
                    b.edges.iter().map(|e| (e.from,e.to,e.action)).collect::<Vec<_>>());
            }
            for power in [0.125,request.power,f32::from_bits(request.power.to_bits()+1)] {
                let power = ScoringPower::new(power).unwrap();
                for compact in [false,true] {
                    let evaluate = |graph: &Graph| {
                        let mut costs = JointCosts::new_controlled(&model,power,false).unwrap();
                        let mut stats = WindowStats::default();
                        let answer = solve::evaluate_ordered(graph,&mut costs,WindowObjective::Native,
                            WindowOptions {compact_value_states:compact,..options()},&mut stats)
                            .unwrap().unwrap().unwrap();
                        (answer.0.score,answer.0.coefficient.to_bits(),answer.1,
                            serde_json::to_value(stats).unwrap())
                    };
                    assert_eq!(evaluate(&original),evaluate(&model.graph));
                }
                assert_eq!(model.score(power).unwrap(),model.maximum(power).unwrap().result.total);
            }
        }
    } }
}
