use crate::{
    utils::{get_array, parse_attribute},
    DataError,
};
use bangdream_optimize_core::{ScoreUp, SkillDefinition};
use serde_json::{Map, Value};

pub(super) fn skill_definition(skill_id: u32, skill: &Value) -> Result<SkillDefinition, DataError> {
    let durations = get_array(skill, "duration")?
        .iter()
        .map(|value| {
            value.as_f64().ok_or(DataError::InvalidField {
                field: "duration",
                value: value.to_string(),
            })
        })
        .collect::<Result<Vec<_>, DataError>>()?;
    let score_up = score_up(skill)?;

    Ok(SkillDefinition {
        durations,
        score_up,
        rateup: skill_id == 61,
    })
}

fn score_up(skill: &Value) -> Result<ScoreUp, DataError> {
    let Some(activation_effect) = skill.get("activationEffect") else {
        return Ok(ScoreUp {
            default: 0.0,
            unification_activate_effect_value: None,
            unification_activate_condition_band_id: None,
            unification_activate_condition_type: None,
        });
    };

    let activate_effect_types = activation_effect
        .get("activateEffectTypes")
        .and_then(Value::as_object);

    let default = if activation_effect
        .get("unificationActivateEffectValue")
        .is_some()
    {
        max_score_values(activate_effect_types, false) / 100.0
    } else {
        max_score_values(activate_effect_types, true) / 100.0
    };

    let unification_activate_effect_value = activation_effect
        .get("unificationActivateEffectValue")
        .and_then(Value::as_f64)
        .map(|value| value / 100.0);
    let unification_activate_condition_band_id = activation_effect
        .get("unificationActivateConditionBandId")
        .and_then(Value::as_u64)
        .map(|value| value as u32);
    let unification_activate_condition_type = activation_effect
        .get("unificationActivateConditionType")
        .and_then(Value::as_str)
        .map(parse_attribute)
        .transpose()?;

    Ok(ScoreUp {
        default,
        unification_activate_effect_value,
        unification_activate_condition_band_id,
        unification_activate_condition_type,
    })
}

fn max_score_values(activate_effect_types: Option<&Map<String, Value>>, include_all: bool) -> f64 {
    let mut max_value = 0.0;
    let Some(types) = activate_effect_types else {
        return max_value;
    };

    for (key, effect) in types {
        if !include_all && !key.starts_with("score") {
            continue;
        }
        if let Some(values) = effect.get("activateEffectValue").and_then(Value::as_array) {
            for value in values {
                if let Some(value) = value.as_f64() {
                    if value > max_value {
                        max_value = value;
                    }
                }
            }
        }
    }

    max_value
}

#[cfg(test)]
mod tests {
    use super::*;
    use bangdream_optimize_core::Attribute;
    use serde_json::json;

    #[test]
    fn maps_unification_skill_score_up() {
        let score = score_up(&json!({
            "activationEffect": {
                "unificationActivateEffectValue": 150,
                "unificationActivateConditionBandId": 1,
                "unificationActivateConditionType": "cool",
                "activateEffectTypes": {
                    "score": {"activateEffectValue": [90, 100, 110, 120, 130]}
                }
            }
        }))
        .unwrap();

        assert_eq!(score.default, 1.3);
        assert_eq!(score.unification_activate_effect_value, Some(1.5));
        assert_eq!(score.unification_activate_condition_band_id, Some(1));
        assert_eq!(
            score.unification_activate_condition_type,
            Some(Attribute::Cool)
        );
    }
}
