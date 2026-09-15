use crate::DataError;
use bangdream_optimize_core::{
    Attribute, CardDefinition, CustomCardConfig, PlayerCardConfig, PlayerConfig, Stat,
};
use std::collections::{BTreeMap, BTreeSet};

pub const CUSTOM_CARD_ID_BASE: u32 = 1_000_000_000;
pub const CUSTOM_CARD_ID_MAX: u32 = 1_999_999_999;

fn invalid(id: &str, message: &str) -> DataError {
    DataError::InvalidField {
        field: "customCards",
        value: format!("{id}: {message}"),
    }
}

pub fn validate_custom_card(id: &str, entry: &CustomCardConfig) -> Result<(), DataError> {
    let parsed = id.parse::<u32>().map_err(|_| invalid(id, "invalid ID"))?;
    let d = &entry.definition;
    let g = &entry.growth;
    let valid_stat = |v: &Stat| {
        [v.performance, v.technique, v.visual]
            .iter()
            .all(|v| (0..=999_999).contains(v))
    };
    if parsed.to_string() != id
        || !(CUSTOM_CARD_ID_BASE + 1..=CUSTOM_CARD_ID_MAX).contains(&parsed)
        || d.card_id != parsed
    {
        return Err(invalid(
            id,
            "ID outside custom range or different from definition",
        ));
    }
    if entry.uid.is_empty()
        || entry.uid.len() > 128
        || d.character_id == 0
        || d.band_id == 0
        || !(1..=5).contains(&d.rarity)
        || d.attribute == Attribute::All
    {
        return Err(invalid(
            id,
            "invalid identity, character, rarity or attribute",
        ));
    }
    if d.level_stats.is_empty()
        || d.level_stats
            .iter()
            .any(|(level, v)| !(1..=100).contains(level) || !valid_stat(v))
        || !valid_stat(&d.training_stat)
        || !d.episode_stats.iter().all(valid_stat)
    {
        return Err(invalid(id, "invalid level, training or episode stats"));
    }
    if !d.level_stats.contains_key(&g.level)
        || g.limit_break_rank > 4
        || !(1..=5).contains(&g.skill_level)
    {
        return Err(invalid(id, "invalid growth configuration"));
    }
    let s = &d.skill;
    let score = |v: f64| v.is_finite() && (0.0..=10.0).contains(&v);
    if s.durations.len() != 5
        || s.durations
            .iter()
            .any(|v| !v.is_finite() || !(0.1..=60.0).contains(v))
        || !score(s.score_up.default)
        || s.score_up
            .unification_activate_effect_value
            .is_some_and(|v| !score(v))
        || s.score_up.unification_activate_condition_band_id == Some(0)
        || s.score_up.unification_activate_condition_type == Some(Attribute::All)
    {
        return Err(invalid(id, "invalid skill parameters"));
    }
    let durations: &[f64] = if s.rateup {
        &[5.0, 5.5, 6.0, 6.5, 7.0]
    } else {
        &[
            3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 5.6, 5.7, 6.0, 6.2, 6.4, 6.5, 6.8, 7.0, 7.2, 7.5, 8.0,
        ]
    };
    if s.durations.iter().any(|v| !durations.contains(v)) || (s.rateup && s.score_up.default != 1.0)
    {
        return Err(invalid(
            id,
            "skill values are outside existing engine support",
        ));
    }
    Ok(())
}

/// Both native and WASM preparations use this path; no custom ID is fetched
/// from Bestdori and disabled entries never enter the candidate pool.
pub fn enabled_custom_definitions(
    player: &PlayerConfig,
    official: &BTreeMap<u32, CardDefinition>,
) -> Result<Vec<CardDefinition>, DataError> {
    let mut uids = BTreeSet::new();
    let mut result = Vec::new();
    for (id, entry) in &player.custom_cards {
        validate_custom_card(id, entry)?;
        if !uids.insert(&entry.uid) {
            return Err(invalid(id, "duplicate unique identifier"));
        }
        if player.card_list.contains_key(id) || official.contains_key(&entry.definition.card_id) {
            return Err(invalid(id, "ID conflicts with official card"));
        }
        if entry.enabled {
            result.push(entry.definition.clone());
        }
    }
    Ok(result)
}

pub fn calculation_card_configs(player: &PlayerConfig) -> BTreeMap<String, PlayerCardConfig> {
    let mut result = player.card_list.clone();
    result.extend(
        player
            .custom_cards
            .iter()
            .filter(|(_, c)| c.enabled)
            .map(|(id, c)| (id.clone(), c.growth.clone())),
    );
    result
}
