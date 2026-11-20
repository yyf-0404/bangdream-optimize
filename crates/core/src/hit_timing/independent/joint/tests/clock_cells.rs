use super::*;
use super::super::clock::ClockCosts;

#[test]
fn complete_clock_cell_operators_keep_expiry_reset_pending_skills_and_native_scores_exact() {
    for (i,skill) in profiles().iter().enumerate() {
        let mut request=input(&[0.2,0.21,0.205,0.28,0.5,0.7,1.4],
            vec![skill.clone(),profiles()[(i+3)%profiles().len()].clone()],2);
        request.initial_combo=699;request.medley_combo=true;request.fever_frames=Some([26,62]);
        let model=JointTimeline::new(&request).unwrap();
        for p in [0.125,37.5,300040.75,f32::from_bits(300040.75_f32.to_bits()+1),1e20] {
            for bucket in [false,true] {
                let power=ScoringPower::new(p).unwrap();
                let costs=|enabled| {
                    let mut base=JointCosts::new_controlled(&model,power,bucket).unwrap();
                    base.operators=operators::Operators::with_controlled(true,enabled);
                    ClockCosts::new(base)
                };
                let mut old=costs(false);let mut new=costs(true);
                match (model.score_with(&mut old),model.score_with(&mut new)) {
                    (Ok((a,x)),Ok((b,y)))=>{
                        assert_eq!(a,b);
                        for (a,b) in [(x.transitions,y.transitions),
                            (x.touch_entry_transitions,y.touch_entry_transitions),
                            (x.touch_hit_transitions,y.touch_hit_transitions),
                            (x.value_graph_edges as u64,y.value_graph_edges as u64),
                            (x.peak_value_states as u64,y.peak_value_states as u64)] {assert_eq!(a,b);}
                    }
                    (Err(a),Err(b))=>assert_eq!(a.to_string(),b.to_string()),
                    _=>panic!("clock cell changed native feasibility"),
                }
            }
        }
    }
}
