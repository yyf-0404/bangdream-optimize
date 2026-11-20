use super::*;

#[test]
fn independent_functions_match_whole_native_dp_at_fresh_powers_and_skills() {
    let profiles = super::super::tests::profiles();
    for shift in 0..profiles.len() {
        let mut input = super::super::tests::input(
            &[0.2,2.,4.,6.,8.,10.,0.25,2.1,4.25,10.09],
            (0..6).map(|s| profiles[(s+shift)%profiles.len()].clone()).collect(),1);
        input.native_time_routing = None;
        input.fever_frames = Some([300,1000]);
        input.medley_combo = shift%2 == 0;
        let mut model = JointTimeline::new(&input).unwrap();
        model.function_plan = prepare(&model).unwrap();
        assert!(model.function_plan.as_ref().unwrap().segments.len() >= 2);
        for power in [0.125,37.5,300040.75,300041.,500001.5] {
            let power = ScoringPower::new(power).unwrap();
            let actual = score(&model,power).unwrap().unwrap().0;
            let expected = model.score_with(&mut JointCosts::new(&model,power).unwrap()).unwrap().0;
            assert_eq!(actual,expected,"profile {shift}, power {power:?}");
        }
        let stats = model.functions.lock().unwrap().stats();
        assert!(stats.hits > 0);
        assert!(stats.retained_bytes <= CACHE_BYTES);
        assert_eq!(model.maximum(ScoringPower::new(input.power).unwrap()).unwrap().result.total,
            score(&model,ScoringPower::new(input.power).unwrap()).unwrap().unwrap().0);
    }
}

#[test]
fn queued_and_crossing_geometry_never_get_unsafe_cuts() {
    let skills = vec![ReplaySkill {duration:2.,..super::super::tests::profiles()[0].clone()};3];
    let mut input = super::super::tests::input(&[0.2,0.3,0.4,0.5],skills,1);
    input.native_time_routing = None;
    let model = JointTimeline::new(&input).unwrap();
    assert!(model.function_plan.is_none());
    let powers = [0.125,300040.75];
    for power in powers {
        let power = ScoringPower::new(power).unwrap();
        assert_eq!(model.score(power).unwrap(),
            model.score_with(&mut JointCosts::new(&model,power).unwrap()).unwrap().0);
    }
}

#[test]
fn sufficient_operand_dominance_is_monotone_under_both_native_truncations() {
    let low = vec![[0.8_f32.to_bits(),1_f32.to_bits(),1_f32.to_bits()],
        [1.1_f32.to_bits(),1.05_f32.to_bits(),1.5_f32.to_bits()]];
    let high = vec![[0.8_f32.to_bits(),1.05_f32.to_bits(),1.2_f32.to_bits()],
        [1.1_f32.to_bits(),1.11_f32.to_bits(),2_f32.to_bits()]];
    assert!(dominates(&high,&low));
    for p in 0..1000 {
        let scorer = NoteScorer::new(ScoringPower::new(p as f32*0.137).unwrap(),27,10).unwrap();
        let sum = |terms:&Vec<Term>| terms.iter().map(|t| u64::from(scorer.score_with_judgement_multiplier(
            f32::from_bits(t[0]),f32::from_bits(t[1]),f32::from_bits(t[2]),1.).unwrap().final_score)).sum::<u64>();
        assert!(sum(&high) >= sum(&low));
    }
}
