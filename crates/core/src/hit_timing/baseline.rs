//! A success certificate for repeated validation of an immutable baseline.
//! It certifies input legality, not its score or any optimizing frame plan.
use super::{bounds::countdown, replay, skill::SkillState, ReplayError, ReplayInput, ReplaySkill};
use crate::scoring::{Judgement, NoteScorer, ScoringPower};
use std::collections::HashMap;

#[derive(Default, Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BaselineStats {
    pub full_replays: u64,
    pub certified_validations: u64,
    pub skill_envelopes: u64,
}

struct Certificate {
    slots: usize,
    count: u32,
    scorer: NoteScorer,
    combo: f32,
    fever: f32,
}

#[derive(Default)]
pub(crate) struct BaselineValidation {
    initialized: bool,
    certificate: Option<Certificate>,
    envelopes: HashMap<[u32; 6], (f32, u32)>,
    pub stats: BaselineStats,
}

impl BaselineValidation {
    /// The owner fixes every field except skills for this object's lifetime.
    /// If the certificate cannot prove success, run the original replay so
    /// repaired Good/Bad baselines, arithmetic failures and error order remain.
    pub fn validate(&mut self, input: &ReplayInput) -> Result<(), ReplayError> {
        if std::env::var("BANDORI_TIMING_DISABLE_BASELINE_CERTIFICATE").as_deref() == Ok("1") {
            self.stats.full_replays += 1;
            return replay(input).map(|_| ());
        }
        if !self.initialized {
            self.stats.full_replays += 1;
            let result = replay(input)?;
            self.initialized = true;
            if result.trace.iter().all(|t| t.raw >= Judgement::Great) {
                self.certificate = Some(Certificate {
                    slots: input.skills.len(),
                    count: input.notes.len() as u32,
                    scorer: NoteScorer::new(ScoringPower::new(input.power)?, input.play_level,
                        input.notes.len() as u32)?,
                    combo: result.trace.iter().map(|t| input.combo_multiplier(t.combo))
                        .fold(0.0, f32::max),
                    fever: result.trace.iter().map(|t| t.fever_multiplier).fold(0.0, f32::max),
                });
            }
            return Ok(());
        }
        if self.proves(input)? {
            self.stats.certified_validations += 1;
            return Ok(());
        }
        self.stats.full_replays += 1;
        replay(input).map(|_| ())
    }

    fn proves(&mut self, input: &ReplayInput) -> Result<bool, ReplayError> {
        let Some(c) = &self.certificate else { return Ok(false); };
        if input.skills.len() != c.slots { return Ok(false); }
        for (i, skill) in input.skills.iter().enumerate() { skill.validate(i)?; }
        let mut multiplier = 1.0_f32;
        let mut updates = 0_u32;
        for skill in &input.skills {
            let key = skill.cache_key();
            let (peak, playing) = if let Some(&v) = self.envelopes.get(&key) { v } else {
                let v = envelope(skill, c.count)?;
                self.stats.skill_envelopes += 1;
                // This is a descriptor envelope, never a complete score memo.
                // Bound storage regardless of the number of searched teams.
                if self.envelopes.len() < 256 { self.envelopes.insert(key, v); }
                v
            };
            multiplier = multiplier.max(peak);
            updates += playing + countdown(0.75) + 2;
        }
        let tail = (input.skills.iter().map(|s| f64::from(s.duration) + 1.0)
            .sum::<f64>() * f64::from(input.fps)).ceil() as u32 + 10;
        if updates > tail { return Ok(false); }
        // Positive f32 multiplication, u32 conversion and the two truncations
        // are monotone. Perfect/max Combo/max Fever/max reachable multiplier
        // therefore bound every actual baseline note, including corrections.
        let Ok(score) = c.scorer.score(Judgement::Perfect, c.combo, multiplier, c.fever)
            else { return Ok(false); };
        Ok(u64::from(score.final_score).checked_mul(u64::from(c.count)).is_some())
    }
}

fn envelope(skill: &ReplaySkill, count: u32) -> Result<(f32, u32), ReplayError> {
    let mut state = SkillState::new(skill)?;
    let mut peak = state.clone().multiplier(Judgement::Great)
        .max(state.clone().multiplier(Judgement::Perfect));
    // A Crescendo can see no more Perfects than the chart's note count. Include
    // untouched Great: the first increment at an equal cap may round downward.
    if matches!(skill.score, super::ScoreEffect::Crescendo { .. }) {
        for _ in 0..count { peak = peak.max(state.multiplier(Judgement::Perfect)); }
    }
    Ok((peak, countdown(skill.duration) + 1))
}

#[cfg(test)]
mod tests;
