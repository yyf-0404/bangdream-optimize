use super::*;
use super::super::clock::ClockCosts;
use crate::hit_timing::independent::solve::blocks::packed::Layout;

#[test]
fn partial_temporal_frontiers_match_complete_paths_at_skill_expiry_and_float_boundaries() {
    let mut exercised=0;
    for (i,skill) in profiles().iter().enumerate() {
        let mut request=input(&[0.1,0.11,1.,1.],vec![skill.clone()],3);
        request.skills[0].duration=0.9;
        request.initial_combo=49;request.medley_combo=i%2==0;
        request.fever_frames=Some([118,122]);
        let model=JointTimeline::new(&request).unwrap();
        let layout=Layout::new(&model.graph.layers,&model.graph.touches,&model.graph.temporal).unwrap();
        let expected=brute(&request);
        for p in [request.power,0.125,37.5,f32::from_bits(request.power.to_bits()+1),1e20] {
            let power=ScoringPower::new(p).unwrap();
            let mut old=ClockCosts::new(JointCosts::new(&model,power).unwrap());
            let mut new=ClockCosts::new(JointCosts::new(&model,power).unwrap());
            let mut a=WindowStats::default();let mut b=WindowStats::default();
            let before=solve::blocks::packed::evaluate_partial_for_test(&model.graph,&layout,&mut old,&mut a,false);
            let after=solve::blocks::packed::evaluate_partial_for_test(&model.graph,&layout,&mut new,&mut b,true);
            match (before,after) {
                (Ok(a),Ok(result))=>{
                    assert_eq!(a.map(|v|v.map(|x|x.score)),result.map(|v|v.map(|x|x.score)));
                    if p==request.power {assert_eq!(result.unwrap().unwrap().score,expected);}
                    exercised+=b.temporal_partial_intervals;
                }
                (Err(a),Err(b))=>assert_eq!(a.to_string(),b.to_string()),
                _=>panic!("partial temporal skip changed native feasibility"),
            }
        }
    }
    assert!(exercised>0,"fixture must contain both stable and unstable clock prefixes");
}
