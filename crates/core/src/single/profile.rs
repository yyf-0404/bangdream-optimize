use crate::{Chart, DpChartModel, DpModelError, TeamCardSkill};

const NORMAL_SKILL_POSITIONS: usize = 5;

#[derive(Debug, Clone, Copy)]
pub(crate) struct SkillMetaProfile {
    pub(crate) normal: [f64; NORMAL_SKILL_POSITIONS],
    pub(crate) captain: f64,
}

impl SkillMetaProfile {
    pub(crate) fn best_normal(self) -> f64 {
        self.normal.into_iter().fold(0.0_f64, f64::max)
    }
}

pub(crate) fn skill_meta_profile(
    chart: &Chart,
    model: &DpChartModel,
    skill: TeamCardSkill,
) -> Result<SkillMetaProfile, DpModelError> {
    skill_meta_profile_for_pool(chart, model, skill, false)
}

pub(crate) fn skill_meta_profile_for_pool(
    chart: &Chart,
    model: &DpChartModel,
    skill: TeamCardSkill,
    queue_supported: bool,
) -> Result<SkillMetaProfile, DpModelError> {
    if !chart.warning.is_empty() {
        if super::queue_optimization_enabled() && queue_supported {
            let values = chart.skill_meta_upper_values(skill)?;
            return Ok(SkillMetaProfile {
                normal: std::array::from_fn(|p| values[p]),
                captain: values[5],
            });
        }
        let upper = chart
            .optimistic_skill_meta_any_window(skill)
            .map_err(DpModelError::from)?;
        return Ok(SkillMetaProfile {
            normal: [upper; NORMAL_SKILL_POSITIONS],
            captain: upper,
        });
    }

    let mut normal = [0.0; NORMAL_SKILL_POSITIONS];
    for (position, value) in normal.iter_mut().enumerate() {
        *value = model.skill_term(chart, position, skill)?.sb;
    }
    Ok(SkillMetaProfile {
        normal,
        captain: model.skill_term(chart, NORMAL_SKILL_POSITIONS, skill)?.sb,
    })
}
