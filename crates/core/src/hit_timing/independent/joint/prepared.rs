//! Compile only local edge operands. Every power still runs a fresh complete
//! dynamic program; no maximum, prefix value or skill state crosses queries.
use super::*;
use crate::scoring::ScoringError;
use std::cell::Cell;

// Bound temporary score-table memory even for unusually long Crescendos.
// Above this limit the exact scalar action implementation remains available.
pub(super) const MAX_SCORE_CELLS: usize = 65_536;

#[derive(Clone, Copy)]
pub(super) struct PreparedAction {
    pub raw: Judgement,
    pub band: usize,
}

pub(super) struct PreparedActions {
    pub actions: Vec<PreparedAction>,
    bands: Vec<(f32, f32)>,
    pub(super) buckets: buckets::Buckets,
    pub(super) outside: Vec<outside_scores::Run>,
}

impl PreparedActions {
    pub fn new(input: &ReplayInput, graph: &Graph) -> Result<Self, ReplayError> {
        let mut ids = HashMap::new();
        let mut bands = Vec::new();
        let actions = graph.actions.iter().map(|action| {
            // Outside runs have their own exact per-query aggregate. Retain
            // only their first band's metadata; scalar hits use this table.
            let (note, frame) = action.hits[0];
            let combo = input.combo_multiplier(input.initial_combo + action.factor.rank as u32);
            let band = *ids.entry((combo.to_bits(), action.factor.fever)).or_insert_with(|| {
                let index = bands.len();
                bands.push((combo, if action.factor.fever { 2. } else { 1. }));
                index
            });
            Ok(PreparedAction { raw: input.notes[note].raw_judgement(frame)?, band })
        }).collect::<Result<_, ReplayError>>()?;
        let outside=graph.actions.iter().enumerate()
            .filter(|(_,a)| matches!(a.hits,geometry::ActionHits::Run(_)))
            .map(|(i,a)| outside_scores::Run::new(input,&a,i)).collect();
        Ok(Self { actions, bands, outside, buckets: buckets::Buckets::new(input,graph) })
    }

    pub fn footprint(&self) -> usize {
        self.buckets.bytes() + self.actions.capacity() * std::mem::size_of::<PreparedAction>()
            + self.bands.capacity() * std::mem::size_of::<(f32, f32)>()
            + self.outside.capacity()*std::mem::size_of::<outside_scores::Run>()
            + self.outside.iter().map(outside_scores::Run::bytes).sum::<usize>()
    }
}

pub(super) struct ScoreTable {
    offsets: Vec<usize>,
    scores: Scores,
}
enum Scores {
    Eager(Vec<Vec<[Result<u32, ScoringError>; 2]>>),
    Lazy {
        scorer: NoteScorer,
        values: Vec<[f32; 2]>,
        bands: Vec<(f32, f32)>,
        // Successful u32 values only. Errors are evaluated by the original
        // scorer when the cell is actually reached and never cached.
        cells: Vec<Cell<Option<u32>>>,
    },
}

impl ScoreTable {
    pub fn new(model: &JointTimeline, scorer: NoteScorer, limit: usize) -> Option<Self> {
        Self::new_controlled(model, scorer, limit,
            model.controls.enabled(Switch::LazyJointScores))
    }

    fn new_controlled(model: &JointTimeline, scorer: NoteScorer, limit: usize, lazy: bool) -> Option<Self> {
        let states = model.machines.iter()
            .try_fold(1usize, |total, machine| total.checked_add(machine.values.len()))?;
        if states.checked_mul(model.prepared_actions.bands.len())? > limit {
            return None;
        }
        let mut offset = 1;
        let offsets = model.machines.iter().map(|machine| {
            let start = offset;
            offset += machine.values.len();
            start
        }).collect();
        if lazy {
            return Some(Self { offsets, scores: Scores::Lazy {
                scorer,
                values: std::iter::once([1., 1.])
                    .chain(model.machines.iter().flat_map(|m| m.values.iter().copied())).collect(),
                bands: model.prepared_actions.bands.clone(),
                cells: (0..states * model.prepared_actions.bands.len() * 2)
                    .map(|_| Cell::new(None)).collect(),
            } });
        }
        let bands = model.prepared_actions.bands.iter().map(|&(combo, fever)| {
            std::iter::once(&[1., 1.])
                .chain(model.machines.iter().flat_map(|machine| &machine.values))
                .map(|multipliers| std::array::from_fn(|grade| {
                    let judgement = if grade == 0 { Judgement::Great } else { Judgement::Perfect };
                    // Call the authoritative f32 arithmetic, in its original
                    // order including both integer truncations. An unreachable
                    // overflowing cell must not fail an otherwise valid query.
                    scorer.score(judgement, combo, multipliers[grade], fever)
                        .map(|score| score.final_score)
                })).collect()
        }).collect();
        Some(Self { offsets, scores: Scores::Eager(bands) })
    }

    pub fn score(
        &self,
        band: usize,
        slot: Option<usize>,
        progress: usize,
        grade: Judgement,
    ) -> Result<u32, ScoringError> {
        let state = slot.map_or(0, |slot| self.offsets[slot] + progress);
        let grade_index = usize::from(grade == Judgement::Perfect);
        match &self.scores {
            Scores::Eager(bands) => bands[band][state][grade_index],
            Scores::Lazy { scorer, values, bands, cells } => {
                let cell = &cells[(band * values.len() + state) * 2 + grade_index];
                if let Some(score) = cell.get() { return Ok(score); }
                let (combo, fever) = bands[band];
                let judgement = if grade_index == 0 { Judgement::Great } else { Judgement::Perfect };
                let score = scorer.score(judgement, combo, values[state][grade_index], fever)?.final_score;
                cell.set(Some(score));
                Ok(score)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::{NoteWindow, ReplayNote, ScoreEffect};

    #[test]
    fn lazy_cells_match_every_eager_cell_native_float_boundary_error_and_budget() {
        for effect in [
            ScoreEffect::Plain { percent: 100. },
            ScoreEffect::Plain { percent: 1e10 },
            ScoreEffect::PerfectOnly { percent: 150. },
            ScoreEffect::PerfectOrZero { percent: 140. },
            ScoreEffect::PerfectOrHalf { percent: 135. },
            ScoreEffect::Continuous { high: 150., low: 80. },
            ScoreEffect::Crescendo { percent: 100., step: 0.5, cap: 150 },
            ScoreEffect::Crescendo { percent: 130., step: 0.5, cap: 130 },
        ] {
            let input = ReplayInput {
                power: 0.125, play_level: 27, fps: 120,
                notes: [0.2, 0.3, 0.6].into_iter().enumerate().map(|(i,time)| ReplayNote {
                    time, trigger: (i == 0).then_some(0), window: NoteWindow::Standard,
                }).collect(),
                hit_frames: vec![24,36,72], skills: vec![ReplaySkill {
                    duration: 0.1, score: effect, correction: Some(JudgementCorrection::Bad),
                }],
                initial_combo: 699, medley_combo: true, fever_frames: Some([30,55]),
                constraints: vec![], hit_order: vec![], native_time_routing: None,
            };
            let model = JointTimeline::new(&input).unwrap();
            for p in [0.125,37.5,300040.75,f32::from_bits(300040.75_f32.to_bits()+1),1e20] {
                let scorer = NoteScorer::new(ScoringPower::new(p).unwrap(), input.play_level,3).unwrap();
                let eager = ScoreTable::new_controlled(&model,scorer,MAX_SCORE_CELLS,false).unwrap();
                let lazy = ScoreTable::new_controlled(&model,scorer,MAX_SCORE_CELLS,true).unwrap();
                if let Scores::Lazy { cells, .. } = &lazy.scores {
                    assert!(cells.iter().all(|cell| cell.get().is_none()));
                } else { unreachable!(); }
                for band in 0..model.prepared_actions.bands.len() {
                    for (slot,count) in [(None,1), (Some(0),model.machines[0].values.len())] {
                        for progress in 0..count { for grade in [Judgement::Great,Judgement::Perfect] {
                            let expected = eager.score(band,slot,progress,grade);
                            assert_eq!(lazy.score(band,slot,progress,grade),expected);
                            assert_eq!(lazy.score(band,slot,progress,grade),expected);
                        } }
                    }
                }
                assert!(ScoreTable::new_controlled(&model,scorer,0,true).is_none());
                assert!(ScoreTable::new_controlled(&model,scorer,0,false).is_none());
                if p == 1e20 {
                    if let Scores::Lazy { cells, .. } = &lazy.scores {
                        assert!(cells.iter().all(|cell| cell.get().is_none()));
                    }
                }
            }
        }
    }
}
