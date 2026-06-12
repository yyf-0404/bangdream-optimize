use crate::model::{
    chart::{Chart, ChartError, TeamCardSkill},
    preparation::PreparedCard,
    schema::Attribute,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DpModelError {
    #[error("song mode does not allow card {card_id}")]
    CardNotAllowed { card_id: u32 },

    #[error("chart is missing skill meta for activation {activation}")]
    MissingSkillMeta { activation: usize },

    #[error("duration {duration} is not supported by chart meta")]
    UnsupportedSkillDuration { duration: f64 },

    #[error("chart error: {0}")]
    Chart(#[from] ChartError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SongMode {
    Mixed,
    UnifiedBand(u32),
    UnifiedAttribute(Attribute),
    UnifiedBandAttribute(u32, Attribute),
}

impl SongMode {
    pub fn allows(self, card: &PreparedCard) -> bool {
        match self {
            Self::Mixed => true,
            Self::UnifiedBand(band_id) => card.band_id == band_id,
            Self::UnifiedAttribute(attribute) => card.attribute == attribute,
            Self::UnifiedBandAttribute(band_id, attribute) => {
                card.band_id == band_id && card.attribute == attribute
            }
        }
    }

    pub fn resolve_skill(self, card: &PreparedCard) -> Result<TeamCardSkill, DpModelError> {
        if !self.allows(card) {
            return Err(DpModelError::CardNotAllowed {
                card_id: card.card_id,
            });
        }

        let (band, attribute) = match self {
            Self::Mixed => (None, None),
            Self::UnifiedBand(band_id) => (Some(band_id), None),
            Self::UnifiedAttribute(attribute) => (None, Some(attribute)),
            Self::UnifiedBandAttribute(band_id, attribute) => (Some(band_id), Some(attribute)),
        };

        Ok(TeamCardSkill {
            card_id: card.card_id,
            duration: card.skill.duration,
            score_up: card.score_up.resolve(band, attribute),
            rateup: card.skill.rateup,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelTerm {
    pub sb: f64,
    pub c: f64,
}

impl ModelTerm {
    pub const ZERO: Self = Self { sb: 0.0, c: 0.0 };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DpChartModel {
    pub d: f64,
    pub c: f64,
    skill_anchor_stat: Option<i32>,
}

impl DpChartModel {
    pub fn from_chart(chart: &Chart) -> Self {
        Self {
            d: chart.meta.no_skill,
            c: 0.0,
            skill_anchor_stat: None,
        }
    }

    pub fn from_chart_with_anchor(
        chart: &Chart,
        skill_anchor_stat: i32,
    ) -> Result<Self, DpModelError> {
        let anchor = skill_anchor_stat.max(1);
        let fit = fit_stat_model(anchor, |stat| chart.no_skill_score_at_stat(stat))?;
        Ok(Self {
            d: fit.slope,
            c: round_model_c(fit.intercept),
            skill_anchor_stat: Some(anchor),
        })
    }

    pub fn skill_term(
        &self,
        chart: &Chart,
        activation: usize,
        skill: TeamCardSkill,
    ) -> Result<ModelTerm, DpModelError> {
        if let Some(anchor) = self.skill_anchor_stat {
            let fit = fit_stat_model(anchor, |stat| {
                chart.skill_delta_at_stat(activation, skill, stat)
            })?;
            return Ok(ModelTerm {
                sb: fit.slope,
                c: round_model_c(fit.intercept),
            });
        }

        let key = duration_key(skill.duration);
        let value = if skill.rateup {
            chart
                .meta
                .rateup
                .get(activation)
                .ok_or(DpModelError::MissingSkillMeta { activation })?
                .get(&key)
                .copied()
                .ok_or(DpModelError::UnsupportedSkillDuration {
                    duration: skill.duration,
                })?
        } else {
            chart
                .meta
                .skill
                .get(activation)
                .ok_or(DpModelError::MissingSkillMeta { activation })?
                .get(&key)
                .copied()
                .ok_or(DpModelError::UnsupportedSkillDuration {
                    duration: skill.duration,
                })?
                * skill.score_up
        };

        Ok(ModelTerm { sb: value, c: 0.0 })
    }

    pub fn score_raw(self, sa: i32, sb: f64, c: f64) -> f64 {
        sa as f64 * (self.d + sb) + self.c + c
    }

    pub fn score(self, sa: i32, sb: f64, c: f64) -> i32 {
        floor_score(self.score_raw(sa, sb, c))
    }
}

pub fn floor_score(score: f64) -> i32 {
    score.floor().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

fn duration_key(duration: f64) -> i32 {
    (duration * 1000.0).round() as i32
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LinearFit {
    slope: f64,
    intercept: f64,
}

fn fit_stat_model(
    anchor: i32,
    mut value_at: impl FnMut(i32) -> Result<f64, ChartError>,
) -> Result<LinearFit, DpModelError> {
    let samples = fit_stat_samples(anchor);
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xx = 0.0;
    let mut sum_xy = 0.0;

    for stat in samples.iter().copied() {
        let x = stat as f64;
        let y = value_at(stat)?;
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }

    let n = samples.len() as f64;
    let denominator = n * sum_xx - sum_x * sum_x;
    if denominator.abs() <= f64::EPSILON {
        let stat = anchor.max(1);
        return Ok(LinearFit {
            slope: value_at(stat)? / stat as f64,
            intercept: 0.0,
        });
    }

    let slope = (n * sum_xy - sum_x * sum_y) / denominator;
    let intercept = (sum_y - slope * sum_x) / n;
    Ok(LinearFit { slope, intercept })
}

fn fit_stat_samples(anchor: i32) -> Vec<i32> {
    const SAMPLE_FACTORS: [f64; 9] = [0.90, 0.94, 0.97, 0.99, 1.0, 1.01, 1.03, 1.06, 1.10];

    let anchor = anchor.max(1);
    let mut samples = SAMPLE_FACTORS
        .iter()
        .map(|factor| ((*factor * anchor as f64).round()).clamp(1.0, i32::MAX as f64) as i32)
        .collect::<Vec<_>>();
    samples.push(anchor);
    samples.sort_unstable();
    samples.dedup();

    let mut next = anchor.saturating_add(1).max(1);
    while samples.len() < 3 && next > 0 {
        samples.push(next);
        next = next.saturating_add(1);
        samples.sort_unstable();
        samples.dedup();
    }

    samples
}

fn round_model_c(c: f64) -> f64 {
    c.round()
}
