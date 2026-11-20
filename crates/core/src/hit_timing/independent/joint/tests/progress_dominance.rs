use super::*;
use crate::hit_timing::ScoreEffect;

#[test]
fn immutable_clock_dominance_metadata_matches_all_coordinates_and_native_work() {
    for (i,skill) in profiles().iter().enumerate() {
        let mut request=input(&[0.2,0.32,0.205,0.28,0.5,0.7],
            vec![skill.clone(),profiles()[(i+3)%profiles().len()].clone()],2);
        request.fever_frames=Some([26,62]);
        request.initial_combo=699;
        let model=JointTimeline::new(&request).unwrap();
        for p in [0.125,37.5,300040.75,f32::from_bits(300040.75_f32.to_bits()+1),600000.,1e20] {
            let power=ScoringPower::new(p).unwrap();
            let mut cached=clock::ClockCosts::new(JointCosts::new(&model,power).unwrap());
            let mut dynamic=clock::ClockCosts::new(JointCosts::new(&model,power).unwrap());
            cached.set_dominance_proofs(true);
            dynamic.set_dominance_proofs(false);
            let before=model.score_with(&mut cached);
            let after=model.score_with(&mut dynamic);
            match (before,after) {
                (Ok((a,sa)),Ok((b,sb)))=> {
                    assert_eq!(a,b);
                    assert_eq!(serde_json::to_value(sa).unwrap(),serde_json::to_value(sb).unwrap());
                }
                (Err(a),Err(b))=>assert_eq!(a.to_string(),b.to_string()),
                _=>panic!("clock dominance metadata feasibility profile={i} power={p}"),
            }
            cached.check_dominance_proofs();
        }
    }
}

fn evaluate(
    model: &JointTimeline,
    power: ScoringPower,
    enabled: bool,
    compact: bool,
) -> (u64, WindowStats) {
    let mut costs = JointCosts::new_controlled(model, power, false).unwrap();
    costs.progress_dominance &= enabled;
    let mut stats = WindowStats::default();
    let (value, plan) = solve::evaluate_ordered(
        &model.graph, &mut costs, WindowObjective::Native,
        WindowOptions { compact_value_states: compact, ..options() }, &mut stats,
    ).unwrap().unwrap().unwrap();
    let mut request = model.input.clone();
    request.power = power.value();
    request.hit_frames = plan.frames;
    request.hit_order = plan.order;
    assert_eq!(replay(&request).unwrap().total, value.score);
    (value.score, stats)
}

#[test]
fn progress_dominance_matches_unpruned_and_exhaustive_with_queue_and_fever() {
    let mut removed = 0;
    let mut fewer = false;
    for queued in [false, true] {
        let mut skills = vec![ReplaySkill {
            duration: 0.3,
            score: ScoreEffect::Crescendo { percent: 100., step: 0.5, cap: 150 },
            correction: None,
        }];
        if queued {
            skills.push(ReplaySkill { correction: Some(JudgementCorrection::Good),
                ..skills[0].clone() });
        }
        let mut request = input(&[0.2, 0.27, 0.34, 0.41], skills, 8);
        request.fever_frames = Some([35, 47]);
        request.initial_combo = 19;
        for power in [2., 300040.75] {
            request.power = power;
            let expected = brute(&request);
            let model = JointTimeline::new(&request).unwrap();
            for compact in [false, true] {
                let power = ScoringPower::new(power).unwrap();
                let (original, before) = evaluate(&model, power, false, compact);
                let (optimized, after) = evaluate(&model, power, true, compact);
                assert_eq!(original, expected);
                assert_eq!(optimized, expected);
                assert_eq!(before.dominated_value_states, 0);
                removed += after.dominated_value_states;
                fewer |= after.transitions < before.transitions;
            }
        }
    }
    assert!(removed > 0 && fewer);
}

#[test]
fn nonmonotone_native_crescendo_disables_progress_dominance() {
    // Native f32's equal-cap clamp lowers the multiplier here by one ULP.
    let request = input(&[0.2, 0.27, 0.34, 0.41], vec![ReplaySkill {
        duration: 0.3,
        score: ScoreEffect::Crescendo { percent: 18., step: 0.5, cap: 18 },
        correction: None,
    }], 3);
    let model = JointTimeline::new(&request).unwrap();
    assert!(!model.machines[0].monotone);
    let power = ScoringPower::new(request.power).unwrap();
    assert!(!JointCosts::new_controlled(&model, power, false).unwrap().supports_dominance());
    let (score, stats) = evaluate(&model, power, true, true);
    assert_eq!(score, brute(&request));
    assert_eq!(stats.dominated_value_states, 0);
}

fn continuous_value(model:&JointTimeline,power:ScoringPower,enabled:bool,clocks:bool)
    ->Result<(u64,WindowStats),SearchError> {
    let mut base=JointCosts::new_controlled(model,power,true)?;
    base.continuous_dominance=enabled;
    base.window_dominance=false;
    base.window_pareto=false;
    if clocks {
        model.score_with(&mut clock::ClockCosts::new(base))
    } else {
        let mut stats=WindowStats::default();
        let result=solve::evaluate_value(&model.graph,&mut base,WindowObjective::Native,
            options(),&mut stats)?.unwrap().unwrap();
        Ok((result.score,stats))
    }
}

#[test]
fn continuous_clean_dominance_matches_unpruned_native_and_exhaustive_paths() {
    let mut saved=0;
    for queued in [false,true] {
        for correction in [None,Some(JudgementCorrection::Good),Some(JudgementCorrection::Bad)] {
            for (high,low) in [(140.,60.),(25.,25.)] {
                let mut skills=vec![ReplaySkill {duration:0.3,
                    score:ScoreEffect::Continuous {high,low},correction}];
                if queued {skills.push(ReplaySkill {duration:0.1,..skills[0].clone()});}
                let mut request=input(&[0.2,0.27,0.34],skills,8);
                request.initial_combo=699;
                request.medley_combo=queued;
                request.fever_frames=Some([31,42]);
                let model=JointTimeline::new(&request).unwrap();
                let expected=brute(&request);
                let witness=model.maximum(ScoringPower::new(request.power).unwrap()).unwrap();
                assert_eq!(witness.result.total,expected);
                for p in [request.power,f32::from_bits(request.power.to_bits()-1),
                    f32::from_bits(request.power.to_bits()+1),0.125,37.5,1e20] {
                    for clocks in [false,true] {
                        let power=ScoringPower::new(p).unwrap();
                        let before=continuous_value(&model,power,false,clocks);
                        let after=continuous_value(&model,power,true,clocks);
                        match (before,after) {
                            (Ok((a,old)),Ok((b,new)))=>{
                                assert_eq!(a,b);
                                if p==request.power {assert_eq!(b,expected);}
                                assert!(new.transitions<=old.transitions);
                                saved+=old.transitions-new.transitions;
                            }
                            (Err(a),Err(b))=>assert_eq!(a.to_string(),b.to_string()),
                            _=>panic!("continuous dominance changed native feasibility"),
                        }
                    }
                }
            }
        }
    }
    assert!(saved>0,"fixture must exercise clean/dirty state pruning");
}

#[test]
fn continuous_dominance_keeps_witness_order_and_checks_native_tables() {
    let request=input(&[0.2,0.27,0.34],vec![ReplaySkill {duration:0.3,
        score:ScoreEffect::Continuous {high:140.,low:60.},correction:None}],8);
    let model=JointTimeline::new(&request).unwrap();
    let power=ScoringPower::new(request.power).unwrap();
    let original=JointCosts::new_controlled(&model,power,false).unwrap();
    assert!(!original.continuous_dominance);
    assert!(!original.progress_dominance);
    let machine=&model.machines[0];
    let effect=&request.skills[0].score;
    assert_eq!(machine.progress_strength(effect,0,true),Some(1));
    assert_eq!(machine.progress_strength(effect,1,true),Some(0));
    assert_eq!(machine.progress_strength(effect,2,true),None);
    assert_eq!(machine.progress_strength(effect,0,false),None);
    // Even an unexpected native table must not authorize the simulation.
    let mut inverted=Machine::new(&request.skills[0],3).unwrap();
    inverted.clean_dominance=false;
    assert_eq!(inverted.progress_strength(effect,0,true),None);
}

#[test]
fn continuous_dominance_keeps_corrected_good_bad_hits_and_fifo_expiry() {
    for correction in [JudgementCorrection::Good,JudgementCorrection::Bad] {
        let mut request=input(&[0.2,0.41],vec![ReplaySkill {duration:0.18,
            score:ScoreEffect::Continuous {high:140.,low:60.},correction:Some(correction)},
            ReplaySkill {duration:0.1,score:ScoreEffect::Plain {percent:100.},correction:None}],24);
        request.initial_combo=699;
        request.fever_frames=Some([31,48]);
        let model=JointTimeline::new(&request).unwrap();
        let expected=brute(&request);
        for clocks in [false,true] {
            let power=ScoringPower::new(request.power).unwrap();
            let (a,_)=continuous_value(&model,power,false,clocks).unwrap();
            let (b,_)=continuous_value(&model,power,true,clocks).unwrap();
            assert_eq!(a,expected);assert_eq!(b,expected);
        }
    }
}
