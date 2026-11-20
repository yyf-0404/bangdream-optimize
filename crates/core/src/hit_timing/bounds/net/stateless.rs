//! Constant native multipliers observe no accepted-note history. Preserve the
//! original per-note maximum and outward sum without allocating option rows.
use super::*;

pub(super) fn profile(frames:&FrameBounds, runs:&[runs::NoteRuns], references:&[f64],
    playing:u32, correction:u32, values:[f32;2], indexed:bool, stats:&mut TimingNetStats) -> [f64;6] {
    std::array::from_fn(|slot|frames.requests[slot].iter().map(|&request| {
        stats.stateless_rows+=1;
        let candidates=if indexed {frames.interval_index.query(request+1,request+playing)}
            else {(0..frames.notes.len()).collect()};
        if indexed {stats.candidate_note_visits+=candidates.len() as u64;}
        else {stats.exhaustive_note_visits+=candidates.len() as u64;}
        // The interval index preserves original note order; zero contributions
        // that existed in the original row still receive the same next_up.
        candidates.into_iter().filter(|&i|!frames.triggers[i].is_some_and(|s|s<=slot))
            .filter_map(|i| {
                let gain=runs[i].constant_gain(request,request+playing,correction,
                    values,references[i],&mut stats.stateless_evaluations);
                if gain.is_some() && !runs[i].outside(request,request+playing) {
                    stats.mandatory_note_rows+=1;
                }
                gain
            })
            .fold(0_f64,|sum,gain|(sum+gain).next_up())
    }).fold(0.,f64::max))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_fixed_multiplier_profiles_keep_every_coefficient_and_integer_upper_bit() {
        use crate::hit_timing::{JudgementCorrection,ReplayNote,NoteWindow};
        let effects=[ScoreEffect::Plain {percent:100.},ScoreEffect::PerfectOnly {percent:140.},
            ScoreEffect::PerfectOrZero {percent:130.},ScoreEffect::PerfectOrHalf {percent:150.}];
        for combo in [0,699] {for correction in [None,Some(JudgementCorrection::Great),
            Some(JudgementCorrection::Good),Some(JudgementCorrection::Bad)] {
            let notes:Vec<_>=(0..28).map(|i|ReplayNote {time:1.+(i/2) as f64*0.1,
                trigger:(i%4==0 && i<24).then_some(i/4),window:NoteWindow::Standard}).collect();
            let mut input=ReplayInput {power:300040.75,play_level:27,fps:120,
                hit_frames:notes.iter().map(|n|(n.time*120.).round() as u32).collect(),notes,
                skills:vec![ReplaySkill {duration:0.1,correction:None,
                    score:ScoreEffect::Plain {percent:100.}};6],initial_combo:0,medley_combo:true,
                fever_frames:None,constraints:vec![],hit_order:vec![],native_time_routing:None};
            input.initial_combo=combo;input.medley_combo=true;input.fever_frames=Some([6,24]);
            let skills:Vec<_>=[0.01,0.12,0.4].into_iter().flat_map(|duration|effects.iter()
                .map(move |score|ReplaySkill {duration,correction,score:score.clone()})).collect();
            for indexed in [false,true] {
                let build=|direct|TimingNetUpper::build_cached_controlled(&input,&skills,
                    ScoringPower::new(700000.).unwrap(),true,true,indexed,true,true,direct,None,None).unwrap();
                let old=build(false);let direct=build(true);
                assert_eq!(old.baseline.to_bits(),direct.baseline.to_bits());
                assert_eq!(old.error.to_bits(),direct.error.to_bits());
                assert_eq!(old.gains.iter().map(|g|g.map(f64::to_bits)).collect::<Vec<_>>(),
                    direct.gains.iter().map(|g|g.map(f64::to_bits)).collect::<Vec<_>>());
                for first in 0..skills.len() {
                    let roster=std::array::from_fn(|s|skills[(first+s)%skills.len()].clone());
                    let members=std::array::from_fn(|s|skills[(first+s)%skills.len()].clone());
                    for power in [0.125,300040.75,600000.] {
                        let power=ScoringPower::new(power).unwrap();
                        assert_eq!(old.upper_sequence(&roster,power).unwrap(),direct.upper_sequence(&roster,power).unwrap());
                        assert_eq!(old.upper_roster(&members,power).unwrap(),direct.upper_roster(&members,power).unwrap());
                    }
                }
                assert!(direct.stats.stateless_profiles>0);
            }
        }}
    }
}
