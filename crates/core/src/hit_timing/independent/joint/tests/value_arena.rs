use super::*;
use super::super::clock::ClockCosts;
use crate::hit_timing::independent::solve::blocks::packed::Layout;

#[test]
fn bounded_value_arena_and_exhaustion_preserve_native_results() {
    for (i,skill) in profiles().iter().enumerate() {
        let mut request=input(&[0.2,0.32,0.205,0.28,0.5,0.7],
            vec![skill.clone(),profiles()[(i+3)%profiles().len()].clone()],2);
        request.fever_frames=Some([26,62]);
        let model=JointTimeline::new(&request).unwrap();
        let packed=Layout::new(&model.graph.layers,&model.graph.touches,&model.graph.temporal).unwrap();
        for p in [0.125,37.5,300040.75,f32::from_bits(300040.75_f32.to_bits()+1),600000.,1e20] {
            for limit in [1,solve::blocks::packed::arena_initial_limit_for_test(),128*1024,2*1024*1024] {
                let power=ScoringPower::new(p).unwrap();
                let mut old=ClockCosts::new(JointCosts::new(&model,power).unwrap());
                let mut new=ClockCosts::new(JointCosts::new(&model,power).unwrap());
                let mut a=WindowStats::default(); let mut b=WindowStats::default();
                let before=solve::blocks::packed::evaluate_original::<true>(&model.graph,&packed,&mut old,&mut a);
                let after=solve::blocks::packed::evaluate_arena_for_test(&model.graph,&packed,&mut new,&mut b,limit);
                let succeeded=before.is_ok();
                match (before,after) {
                    (Ok(a),Ok(b))=>assert_eq!(a.map(|v|v.map(|x|x.score)),b.map(|v|v.map(|x|x.score))),
                    (Err(a),Err(b))=>assert_eq!(a.to_string(),b.to_string()),
                    _=>panic!("arena feasibility mismatch profile={i} power={p} limit={limit}"),
                }
                for (a,b) in [(a.transitions,b.transitions),(a.touch_entry_transitions,b.touch_entry_transitions),
                    (a.touch_hit_transitions,b.touch_hit_transitions),(a.peak_value_states as u64,b.peak_value_states as u64),
                    (a.value_graph_edges as u64,b.value_graph_edges as u64),
                    (a.value_graph_layers as u64,b.value_graph_layers as u64)] {assert_eq!(a,b);}
                if succeeded {
                    let blocks=b.native_blocks.as_ref().unwrap();
                    assert_eq!(blocks.value_arena_queries+blocks.value_arena_budget_fallbacks,1);
                    if limit==1 { assert_eq!(blocks.value_arena_budget_fallbacks,1); }
                    if blocks.value_arena_queries==1 { assert!(blocks.retained_bytes<=limit); }
                }
            }
        }
    }
}

#[test]
fn value_arena_partial_run_exhaustion_restarts_a_complete_native_query() {
    let request=input(&[0.2,0.32,0.205,0.28,0.5,0.7],
        vec![profiles()[1].clone(),profiles()[3].clone()],2);
    let model=JointTimeline::new(&request).unwrap();
    let layout=Layout::new(&model.graph.layers,&model.graph.touches,&model.graph.temporal).unwrap();
    let power=ScoringPower::new(300040.75).unwrap();
    let mut attempt=ClockCosts::new(JointCosts::new(&model,power).unwrap());
    let mut partial=WindowStats::default();
    assert!(solve::blocks::packed::attempt_arena_for_test(&model.graph,&layout,&mut attempt,&mut partial)
        .unwrap().is_none());
    assert!(partial.value_graph_layers>0);
    let mut expected=ClockCosts::new(JointCosts::new(&model,power).unwrap());
    let mut old=WindowStats::default();let mut actual=WindowStats::default();
    let a=solve::blocks::packed::evaluate_original::<true>(&model.graph,&layout,&mut expected,&mut old).unwrap();
    let b=solve::blocks::packed::evaluate_arena_for_test(&model.graph,&layout,&mut attempt,&mut actual,
        solve::blocks::packed::arena_initial_limit_for_test()).unwrap();
    assert_eq!(a.unwrap().unwrap().score,b.unwrap().unwrap().score);
    assert_eq!(old.transitions,actual.transitions);
    assert_eq!(actual.native_blocks.as_ref().unwrap().value_arena_budget_fallbacks,1);
}
