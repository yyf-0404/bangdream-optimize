use super::*;

#[test]
fn clock_quotient_matches_original_fifo_dp_for_all_effects_and_queued_boundaries() {
    let mut profiles=profiles();
    profiles.push(ReplaySkill {duration:0.1,correction:None,
        score:crate::hit_timing::ScoreEffect::Crescendo {percent:18.,step:0.5,cap:18}});
    for duration in [0.008333334,0.1,1.25,4.9] {
        for shift in 0..profiles.len() {
            let mut skills:Vec<_>=(0..6).map(|s| profiles[(s+shift)%profiles.len()].clone()).collect();
            for skill in &mut skills { skill.duration=duration; }
            let mut request=input(&[0.2,1.3,2.5,3.7,4.9,6.1,0.3,1.31,2.63,6.21],skills,1);
            request.native_time_routing=None;
            request.fever_frames=Some([45,400]);
            request.medley_combo=shift%2==0;
            let model=JointTimeline::new(&request).unwrap();
            for power in [0.125,37.5,300040.75,300041.,500001.5] {
                let power=ScoringPower::new(power).unwrap();
                let old=model.score_with(&mut JointCosts::new_controlled(&model,power,false).unwrap()).unwrap();
                let new=model.score_with(&mut JointCosts::new_controlled(&model,power,true).unwrap()).unwrap();
                let interned=model.score_with(&mut clock::ClockCosts::new(
                    JointCosts::new_controlled(&model,power,true).unwrap())).unwrap();
                assert_eq!(old.0,new.0,"duration {duration}, shift {shift}, power {power:?}");
                assert_eq!(old.0,interned.0,"interned duration {duration}, shift {shift}, power {power:?}");
            }
        }
    }
}

#[test]
fn endpoint_bucketing_retains_every_observable_queue_ready_frame() {
    let skills=vec![ReplaySkill {duration:0.1,..profiles()[4].clone()};3];
    let mut request=input(&[1.,1.05,1.10,1.15,1.3,2.1],skills,1);
    request.native_time_routing=None;
    let model=JointTimeline::new(&request).unwrap();
    let power=ScoringPower::new(request.power).unwrap();
    let mut old=JointCosts::new_controlled(&model,power,false).unwrap();
    let mut new=JointCosts::new_controlled(&model,power,true).unwrap();
    let a=model.score_with(&mut old).unwrap().0;
    let b=model.score_with(&mut new).unwrap().0;
    let interned = model.score_with(&mut clock::ClockCosts::new(
        JointCosts::new_controlled(&model,power,true).unwrap())).unwrap().0;
    assert_eq!(a,b);
    assert_eq!(a,interned);
    assert_eq!(a,model.maximum(power).unwrap().result.total);
}
