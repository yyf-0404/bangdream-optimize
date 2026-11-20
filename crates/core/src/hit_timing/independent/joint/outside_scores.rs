//! Integer multiplicities of equal native outside operands. Every query calls
//! the original f32 scorer at its new power; only exact u64 sums are grouped.
use super::*;
use crate::scoring::ScoringError;

pub(super) struct Run {
    pub index: usize,
    fever: f32,
    terms: Vec<(f32,u64)>,
}
impl Run {
    pub(super) fn new(input: &ReplayInput, action: &Action, index: usize) -> Self {
        let mut terms=Vec::<(f32,u64)>::new();
        for offset in 0..action.hits.len() {
            let combo=input.combo_multiplier(input.initial_combo+(action.factor.rank+offset) as u32);
            if let Some(last)=terms.last_mut().filter(|t| t.0.to_bits()==combo.to_bits()) {
                last.1+=1;
            } else { terms.push((combo,1)); }
        }
        Self { index,fever:if action.factor.fever {2.} else {1.},terms }
    }
    pub(super) fn score(&self, scorer: NoteScorer) -> Result<u64,ScoringError> {
        self.terms.iter().try_fold(0_u64,|total,&(combo,count)| {
            let score=u64::from(scorer.score(Judgement::Perfect,combo,1.,self.fever)?.final_score);
            let add=score.checked_mul(count).ok_or(ScoringError::TotalScoreOverflow)?;
            total.checked_add(add).ok_or(ScoringError::TotalScoreOverflow)
        })
    }
    pub(super) fn bytes(&self) -> usize { self.terms.capacity()*std::mem::size_of::<(f32,u64)>() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::{ReplayNote,NoteWindow};
    #[test]
    fn fresh_grouped_outside_scores_keep_native_truncation_combo_boundaries_and_errors() {
        let mut input=ReplayInput {power:300040.75,play_level:27,fps:120,
            notes:(0..1300).map(|i| ReplayNote {time:i as f64/120.,trigger:None,
                window:NoteWindow::PinnedPerfect {frame:i as u32}}).collect(),
            hit_frames:(0..1300).collect(),skills:vec![],initial_combo:0,medley_combo:false,
            fever_frames:None,constraints:vec![],hit_order:vec![],native_time_routing:None};
        let mut action=Action {factor:Factor {perfect:true,inside:false,slot:0,fever:false,reference:0.,rank:1},
            hits:geometry::ActionHits::Run((0..1300).map(|i|(i,i as u32)).collect())};
        for medley in [false,true] { for combo in [0,699,999] { for fever in [false,true] {
            input.medley_combo=medley;input.initial_combo=combo;action.factor.fever=fever;
            let program=Run::new(&input,&action,13); assert_eq!(program.index,13);
            assert!(program.terms.len()<25);
            for power in [0.,0.125,37.5,300040.75,f32::from_bits(300040.75_f32.to_bits()+1),
                600000.,1e12,1e20] {
                let scorer=NoteScorer::new(ScoringPower::new(power).unwrap(),27,1300).unwrap();
                let expected=(0..action.hits.len()).try_fold(0_u64,|total,i| {
                    let score=scorer.score(Judgement::Perfect,input.combo_multiplier(combo+(i+1) as u32),
                        1.,if fever {2.} else {1.})?.final_score;
                    total.checked_add(u64::from(score)).ok_or(ScoringError::TotalScoreOverflow)
                });
                assert_eq!(program.score(scorer),expected,"medley {medley}, combo {combo}, fever {fever}, power {power}");
            }
        } } }
    }
}
