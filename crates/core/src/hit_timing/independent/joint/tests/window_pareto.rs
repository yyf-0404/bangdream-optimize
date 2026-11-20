use super::*;
use crate::hit_timing::ScoreEffect;

fn evaluate(model:&JointTimeline,power:ScoringPower,enabled:bool,clocks:bool)
    ->Result<(u64,WindowStats),ReplayError> {
    let mut base=JointCosts::new_controlled(model,power,true)?;
    base.window_pareto=enabled;
    let mut stats=WindowStats::default();
    let value=if clocks {
        let mut costs=clock::ClockCosts::new(base);
        solve::evaluate_value(&model.graph,&mut costs,WindowObjective::Native,
            options(),&mut stats)?
    } else {
        solve::evaluate_value(&model.graph,&mut base,WindowObjective::Native,
            options(),&mut stats)?
    }.unwrap().unwrap();
    Ok((value.score,stats))
}

#[test]
fn later_end_and_higher_progress_match_complete_paths_and_unpruned_native_scores() {
    let mut removed=0;
    for queued in [false,true] {
        for correction in [None,Some(JudgementCorrection::Good)] {
            let mut skills=vec![ReplaySkill {duration:0.3,
                score:ScoreEffect::Crescendo {percent:100.,step:0.5,cap:150},correction}];
            if queued {skills.push(ReplaySkill {duration:0.1,..skills[0].clone()});}
            let mut request=input(&[0.2,0.27,0.34,0.41],skills,8);
            request.initial_combo=699;
            request.fever_frames=Some([35,47]);
            for power in [0.125,2.,300040.75] {
                request.power=power;
                let expected=brute(&request);
                let model=JointTimeline::new(&request).unwrap();
                for clocks in [false,true] {
                    let power=ScoringPower::new(power).unwrap();
                    let (before,b)=evaluate(&model,power,false,clocks).unwrap();
                    let (after,a)=evaluate(&model,power,true,clocks).unwrap();
                    assert_eq!(before,expected);
                    assert_eq!(after,expected);
                    assert!(a.transitions<=b.transitions);
                    removed+=b.transitions-a.transitions;
                }
            }
        }
    }
    assert!(removed>0,"fixture must exercise cross-progress/end dominance");
}

#[test]
fn native_nonmonotone_cap_and_observable_cooling_disable_the_plane() {
    use machine::ActionCosts;
    for (skill,ready,active) in [
        (ScoreEffect::Crescendo {percent:18.,step:0.5,cap:18},0,true),
        (ScoreEffect::Crescendo {percent:100.,step:0.5,cap:150},10,true),
        (ScoreEffect::Crescendo {percent:100.,step:0.5,cap:150},0,false),
        (ScoreEffect::Plain {percent:100.},0,true),
    ] {
        let request=input(&[0.2,0.27,0.34,0.41],vec![ReplaySkill {duration:0.3,score:skill,correction:None}],8);
        let model=JointTimeline::new(&request).unwrap();
        let mut costs=JointCosts::new_controlled(&model,ScoringPower::new(request.power).unwrap(),true).unwrap();
        costs.window_pareto=true;
        costs.states.push(State {ready,progress:0,
            windows:Rc::new(vec![Window {start:if active {0}else {20},end:100,slot:0}])});
        assert!(costs.dominance_plane(1).is_none());
    }
}

fn continuous_plane(model:&JointTimeline,power:ScoringPower,enabled:bool,clocks:bool)
    ->Result<(u64,WindowStats),SearchError> {
    let mut base=JointCosts::new_controlled(model,power,true)?;
    base.continuous_dominance=true;
    base.continuous_window_dominance=enabled;
    base.progress_dominance=true;
    base.window_pareto=false;
    if clocks {model.score_with(&mut clock::ClockCosts::new(base))}
    else {
        let mut stats=WindowStats::default();
        let result=solve::evaluate_value(&model.graph,&mut base,WindowObjective::Native,
            options(),&mut stats)?.unwrap().unwrap();
        Ok((result.score,stats))
    }
}

#[test]
fn continuous_clean_end_contours_match_one_axis_and_complete_native_paths() {
    let mut saved=0;
    for queued in [false,true] {
        for (high,low) in [(140.,60.),(25.,25.)] {
            let mut skills=vec![ReplaySkill {duration:0.3,
                score:ScoreEffect::Continuous {high,low},correction:None}];
            if queued {skills.push(ReplaySkill {duration:0.1,..skills[0].clone()});}
            let mut request=input(&[0.2,0.27,0.34],skills,8);
            request.initial_combo=699;request.medley_combo=queued;
            request.fever_frames=Some([31,42]);
            let model=JointTimeline::new(&request).unwrap();
            let expected=brute(&request);
            assert_eq!(model.maximum(ScoringPower::new(request.power).unwrap()).unwrap().result.total,expected);
            for p in [request.power,0.125,37.5,f32::from_bits(request.power.to_bits()-1),
                f32::from_bits(request.power.to_bits()+1),1e20] {
                for clocks in [false,true] {
                    let power=ScoringPower::new(p).unwrap();
                    match (continuous_plane(&model,power,false,clocks),continuous_plane(&model,power,true,clocks)) {
                        (Ok((a,before)),Ok((b,after)))=>{
                            assert_eq!(a,b);
                            if p==request.power {assert_eq!(b,expected);}
                            assert!(after.transitions<=before.transitions);
                            saved+=before.transitions-after.transitions;
                        }
                        (Err(a),Err(b))=>assert_eq!(a.to_string(),b.to_string()),
                        _=>panic!("continuous end contours changed native feasibility"),
                    }
                }
            }
        }
    }
    assert!(saved>0,"fixture must exercise combined clean/end pruning");
}
