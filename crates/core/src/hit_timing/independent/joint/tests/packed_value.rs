use super::*;
use super::super::clock::ClockCosts;
use crate::hit_timing::independent::solve::blocks::packed::Layout;

#[test]
fn prepared_runtime_controls_match_dynamic_native_work_errors_and_witnesses() {
    for (i, skill) in profiles().iter().enumerate() {
        let mut request=input(&[0.2,0.32,0.205,0.28,0.5,0.7],
            vec![skill.clone(),profiles()[(i+3)%profiles().len()].clone()],2);
        request.fever_frames=Some([26,62]);
        request.initial_combo=699;
        let mut model=JointTimeline::new(&request).unwrap();
        let saved=Controls::capture(|switch|Controls::dynamic().enabled(switch));
        for p in [0.125,37.5,300040.75,f32::from_bits(300040.75_f32.to_bits()+1),600000.,1e20] {
            let power=ScoringPower::new(p).unwrap();
            model.controls=saved;
            let before=model.score_with_stats(power);
            model.controls=Controls::dynamic();
            let after=model.score_with_stats(power);
            match (before,after) {
                (Ok((a,sa)),Ok((b,sb)))=> {
                    assert_eq!(a,b);
                    assert_eq!(serde_json::to_value(sa).unwrap(),serde_json::to_value(sb).unwrap());
                }
                (Err(a),Err(b))=>assert_eq!(a.to_string(),b.to_string()),
                _=>panic!("prepared controls feasibility profile={i} power={p}"),
            }
        }
        let power=ScoringPower::new(300040.75).unwrap();
        model.controls=saved;
        let before=model.maximum(power).unwrap();
        model.controls=Controls::dynamic();
        let after=model.maximum(power).unwrap();
        assert_eq!(serde_json::to_value(before).unwrap(),serde_json::to_value(after).unwrap());
    }
}
    #[test]
    fn packed_value_graph_matches_native_results_full_statistics_and_bounds() {


        for (i, skill) in profiles().iter().enumerate() {
            let mut request = input(&[0.2, 0.32, 0.205, 0.28, 0.5, 0.7],
                vec![skill.clone(), profiles()[(i + 3) % profiles().len()].clone()], 2);
            request.fever_frames = Some([26, 62]);
            let model = JointTimeline::new(&request).unwrap();
            let packed = Layout::new(&model.graph.layers, &model.graph.touches, &model.graph.temporal).unwrap();
            for p in [0.125, 37.5, 300040.75, f32::from_bits(300040.75_f32.to_bits() + 1), 600000., 1e20] {
                let power = ScoringPower::new(p).unwrap();
                let mut a = WindowStats::default();
                let mut b = WindowStats::default();
                let mut old = ClockCosts::new(JointCosts::new(&model, power).unwrap());
                let mut new = ClockCosts::new(JointCosts::new(&model, power).unwrap());
                let before = solve::blocks::evaluate_unfused_for_test(&model.graph, &mut old, &mut a);
                let after = solve::blocks::packed::evaluate_original::<true>(&model.graph, &packed, &mut new, &mut b);
                match (before, after) {
                    (Ok(a), Ok(b)) => assert_eq!(a.map(|v| v.map(|x| x.score)), b.map(|v| v.map(|x| x.score))),
                    (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
                    _ => panic!("packed feasibility mismatch profile={i} power={p}"),
                }
                old.finish_stats(&mut a); new.finish_stats(&mut b);
                assert_eq!(serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
            }
            assert!(Layout::with_limit(&model.graph.layers, &model.graph.touches, &model.graph.temporal, 1).is_none());
            assert!(packed.bytes() <= 2 * 1024 * 1024);
        }
    }
