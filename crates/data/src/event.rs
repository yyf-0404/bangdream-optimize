use crate::{
    utils::{
        get_array, get_f64, get_str, get_u32, get_u8, optional_array, parse_attribute,
        stat_rate_from_percent_object,
    },
    DataError,
};
use bangdream_optimize_core::{
    EventAttributeBonus, EventBonus, EventCharacterBonus, EventMemberBonus,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn event_bonus(event_data: &Value) -> Result<EventBonus, DataError> {
    let attributes = get_array(event_data, "attributes")?
        .iter()
        .map(|value| {
            Ok(EventAttributeBonus {
                attribute: parse_attribute(get_str(value, "attribute")?)?,
                percent: get_f64(value, "percent")?,
            })
        })
        .collect::<Result<Vec<_>, DataError>>()?;

    let characters = get_array(event_data, "characters")?
        .iter()
        .map(|value| {
            Ok(EventCharacterBonus {
                character_id: get_u32(value, "characterId")?,
                percent: get_f64(value, "percent")?,
            })
        })
        .collect::<Result<Vec<_>, DataError>>()?;

    let members = optional_array(event_data, "members")
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .map(|value| {
            Ok(EventMemberBonus {
                card_id: get_u32(value, "situationId")?,
                percent: get_f64(value, "percent")?,
            })
        })
        .collect::<Result<Vec<_>, DataError>>()?;

    let event_character_parameter_bonus = event_data
        .get("eventCharacterParameterBonus")
        .and_then(|value| {
            if value.is_null() {
                None
            } else {
                Some(stat_rate_from_percent_object(value))
            }
        })
        .transpose()?;

    let event_attribute_and_character_parameter_percent = event_data
        .get("eventAttributeAndCharacterBonus")
        .and_then(|value| value.get("parameterPercent"))
        .and_then(Value::as_f64)
        .unwrap_or_default();

    let mut limit_breaks: BTreeMap<u8, BTreeMap<u8, f64>> = BTreeMap::new();
    for value in optional_array(event_data, "limitBreaks")
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let rarity = get_u8(value, "rarity")?;
        let rank = get_u8(value, "rank")?;
        let percent = get_f64(value, "percent")?;
        limit_breaks
            .entry(rarity)
            .or_default()
            .insert(rank, percent);
    }

    Ok(EventBonus {
        attributes,
        characters,
        members,
        event_character_parameter_bonus,
        event_attribute_and_character_parameter_percent,
        limit_breaks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bangdream_optimize_core::Attribute;
    use serde_json::json;

    #[test]
    fn maps_event_bonus() {
        let event = event_bonus(&json!({
            "attributes": [{"attribute": "happy", "percent": 20}],
            "characters": [{"characterId": 7, "percent": 10}],
            "members": [{"situationId": 101, "percent": 50}],
            "eventCharacterParameterBonus": {"performance": 1, "technique": 2, "visual": 3},
            "eventAttributeAndCharacterBonus": {"parameterPercent": 20},
            "limitBreaks": [{"rarity": 4, "rank": 2, "percent": 3}]
        }))
        .unwrap();

        assert_eq!(event.attributes[0].attribute, Attribute::Happy);
        assert_eq!(event.characters[0].character_id, 7);
        assert_eq!(event.members[0].card_id, 101);
        assert_eq!(event.limit_breaks[&4][&2], 3.0);
    }
}
