use super::skill::skill_definition;
use crate::{
    utils::{get_str, get_u32, get_u8, parse_attribute, stat_from_object},
    DataError,
};
use bangdream_optimize_core::{CardDefinition, Stat};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn character_id(card: &Value) -> Result<u32, DataError> {
    get_u32(card, "characterId")
}

pub(super) fn skill_id(card: &Value) -> Result<u32, DataError> {
    get_u32(card, "skillId")
}

pub(super) fn card_definition(
    card_id: u32,
    card: &Value,
    character: &Value,
    skill: &Value,
) -> Result<CardDefinition, DataError> {
    Ok(CardDefinition {
        card_id,
        character_id: character_id(card)?,
        band_id: get_u32(character, "bandId")?,
        rarity: get_u8(card, "rarity")?,
        attribute: parse_attribute(get_str(card, "attribute")?)?,
        level_stats: level_stats(card)?,
        training_stat: training_stat(card)?,
        episode_stats: episode_stats(card)?,
        skill: skill_definition(skill_id(card)?, skill)?,
    })
}

fn level_stats(card: &Value) -> Result<BTreeMap<u8, Stat>, DataError> {
    let stat = card
        .get("stat")
        .and_then(Value::as_object)
        .ok_or(DataError::MissingField { field: "stat" })?;

    let mut levels = BTreeMap::new();
    for (key, value) in stat {
        if let Ok(level) = key.parse::<u8>() {
            levels.insert(level, stat_from_object(value)?);
        }
    }

    if levels.is_empty() {
        return Err(DataError::MissingField {
            field: "stat[level]",
        });
    }

    Ok(levels)
}

fn training_stat(card: &Value) -> Result<Stat, DataError> {
    let Some(training) = card
        .get("stat")
        .and_then(Value::as_object)
        .and_then(|stat| stat.get("training"))
    else {
        return Ok(zero_stat());
    };
    stat_from_object(training)
}

fn episode_stats(card: &Value) -> Result<[Stat; 2], DataError> {
    let mut result = [zero_stat(), zero_stat()];
    if let Some(episodes) = card
        .get("stat")
        .and_then(Value::as_object)
        .and_then(|stat| stat.get("episodes"))
        .and_then(Value::as_array)
    {
        for (index, episode) in episodes.iter().take(2).enumerate() {
            result[index] = stat_from_object(episode)?;
        }
    }
    Ok(result)
}

fn zero_stat() -> Stat {
    Stat {
        performance: 0,
        technique: 0,
        visual: 0,
    }
}
