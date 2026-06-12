use crate::model::chart::TeamCardSkill;
use crate::model::schema::{
    AreaItemConfig, Attribute, CharacterBonusConfig, PlayerCardConfig, Stat,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const ALL_BAND_KEY: &str = "1000";
pub const ALL_ATTRIBUTE_KEY: &str = "~all";
pub const PERFORMANCE_KEY: &str = "performance";
pub const TECHNIQUE_KEY: &str = "technique";
pub const VISUAL_KEY: &str = "visual";

#[derive(Debug, Error)]
pub enum PreparationError {
    #[error("player card config for card {card_id} is missing")]
    MissingPlayerCardConfig { card_id: u32 },

    #[error("character bonus for character {character_id} is missing")]
    MissingCharacterBonus { character_id: u32 },

    #[error("skill level {skill_level} is invalid for card {card_id}")]
    InvalidSkillLevel { card_id: u32, skill_level: u8 },

    #[error("level {level} is invalid for card {card_id}")]
    InvalidCardLevel { card_id: u32, level: u8 },

    #[error("area item definition {area_item_id} is missing")]
    MissingAreaItemDefinition { area_item_id: u32 },

    #[error("area item {area_item_id} does not map to a supported target key")]
    UnsupportedAreaItemTarget { area_item_id: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatValue {
    pub performance: f64,
    pub technique: f64,
    pub visual: f64,
}

impl StatValue {
    pub fn zero() -> Self {
        Self {
            performance: 0.0,
            technique: 0.0,
            visual: 0.0,
        }
    }

    pub fn sum(self) -> f64 {
        self.performance + self.technique + self.visual
    }

    pub fn add(&mut self, other: Self) {
        self.performance += other.performance;
        self.technique += other.technique;
        self.visual += other.visual;
    }

    pub fn sub(&mut self, other: Self) {
        self.performance -= other.performance;
        self.technique -= other.technique;
        self.visual -= other.visual;
    }

    pub fn add_each(&mut self, value: f64) {
        self.performance += value;
        self.technique += value;
        self.visual += value;
    }

    pub fn mul_rate(self, rate: StatRate) -> Self {
        Self {
            performance: self.performance * rate.performance,
            technique: self.technique * rate.technique,
            visual: self.visual * rate.visual,
        }
    }

    pub fn floor_components(self) -> Self {
        Self {
            performance: self.performance.floor(),
            technique: self.technique.floor(),
            visual: self.visual.floor(),
        }
    }
}

impl From<Stat> for StatValue {
    fn from(value: Stat) -> Self {
        Self {
            performance: value.performance as f64,
            technique: value.technique as f64,
            visual: value.visual as f64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatRate {
    pub performance: f64,
    pub technique: f64,
    pub visual: f64,
}

impl StatRate {
    pub fn zero() -> Self {
        Self {
            performance: 0.0,
            technique: 0.0,
            visual: 0.0,
        }
    }

    pub fn all(value: f64) -> Self {
        Self {
            performance: value,
            technique: value,
            visual: value,
        }
    }

    pub fn add(&mut self, other: Self) {
        self.performance += other.performance;
        self.technique += other.technique;
        self.visual += other.visual;
    }

    pub fn div_scalar(self, divisor: f64) -> Self {
        Self {
            performance: self.performance / divisor,
            technique: self.technique / divisor,
            visual: self.visual / divisor,
        }
    }
}

impl From<crate::model::schema::StatRate> for StatRate {
    fn from(value: crate::model::schema::StatRate) -> Self {
        Self {
            performance: value.performance,
            technique: value.technique,
            visual: value.visual,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardDefinition {
    pub card_id: u32,
    pub character_id: u32,
    pub band_id: u32,
    pub rarity: u8,
    pub attribute: Attribute,
    pub level_stats: BTreeMap<u8, Stat>,
    pub training_stat: Stat,
    pub episode_stats: [Stat; 2],
    pub skill: SkillDefinition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDefinition {
    pub durations: Vec<f64>,
    pub score_up: ScoreUp,
    #[serde(default)]
    pub rateup: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreUp {
    pub default: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unification_activate_effect_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unification_activate_condition_band_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unification_activate_condition_type: Option<Attribute>,
}

impl ScoreUp {
    pub fn resolve(self, team_band_id: Option<u32>, team_attribute: Option<Attribute>) -> f64 {
        let Some(unified_value) = self.unification_activate_effect_value else {
            return self.default;
        };

        if let Some(condition_band_id) = self.unification_activate_condition_band_id {
            if team_band_id != Some(condition_band_id) {
                return self.default;
            }
        }

        if let Some(condition_attribute) = self.unification_activate_condition_type {
            if team_attribute != Some(condition_attribute) {
                return self.default;
            }
        }

        unified_value
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventBonus {
    #[serde(default)]
    pub attributes: Vec<EventAttributeBonus>,
    #[serde(default)]
    pub characters: Vec<EventCharacterBonus>,
    #[serde(default)]
    pub members: Vec<EventMemberBonus>,
    #[serde(default)]
    pub event_character_parameter_bonus: Option<StatRate>,
    #[serde(default)]
    pub event_attribute_and_character_parameter_percent: f64,
    #[serde(default)]
    pub limit_breaks: BTreeMap<u8, BTreeMap<u8, f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventAttributeBonus {
    pub attribute: Attribute,
    pub percent: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventCharacterBonus {
    pub character_id: u32,
    pub percent: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventMemberBonus {
    pub card_id: u32,
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedCard {
    pub card_id: u32,
    pub character_id: u32,
    pub band_id: u32,
    pub rarity: u8,
    pub attribute: Attribute,
    pub level: u8,
    pub training: bool,
    pub illust_training_status: bool,
    pub episodes: [bool; 2],
    pub limit_break_rank: u8,
    pub skill_level: u8,
    pub stat: StatValue,
    pub event_add_stat: StatValue,
    pub skill: TeamCardSkill,
    pub score_up: ScoreUp,
}

impl PreparedCard {
    pub fn add_up_stat(
        &self,
        area_item_percent: &AreaItemPercent,
        band_id: &str,
        attribute: &str,
        magazine: &str,
    ) -> f64 {
        let mut stat = StatValue::zero();

        if band_key_matches(band_id, self.band_id) {
            if let Some(rate) = area_item_percent.band.get(band_id) {
                stat.add(self.stat.mul_rate(*rate));
            }
        }

        if attribute_key_matches(attribute, self.attribute) {
            if let Some(rate) = area_item_percent.attribute.get(attribute) {
                stat.add(self.stat.mul_rate(*rate));
            }
        }

        if let Some(rate) = area_item_percent.magazine.get(magazine) {
            stat.add(self.stat.mul_rate(*rate));
        }

        stat.add(self.stat);
        stat.add(self.event_add_stat);
        stat.sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AreaItemType {
    Band,
    Attribute,
    Magazine,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaItemDefinition {
    pub area_item_id: u32,
    #[serde(default)]
    pub target_band_ids: Vec<u32>,
    #[serde(default)]
    pub target_attributes: Vec<Attribute>,
    pub percents: BTreeMap<u8, StatRate>,
}

impl AreaItemDefinition {
    pub fn item_type(&self) -> AreaItemType {
        if self.target_band_ids.len() == 1 {
            return AreaItemType::Band;
        }
        if self.target_attributes.len() == 1 {
            return AreaItemType::Attribute;
        }
        if self.area_item_id >= 80 {
            return AreaItemType::Magazine;
        }
        if self.area_item_id >= 73 {
            return AreaItemType::Band;
        }
        AreaItemType::Attribute
    }

    pub fn target_key(&self) -> Result<String, PreparationError> {
        match self.item_type() {
            AreaItemType::Band => Ok(band_target_key(&self.target_band_ids)),
            AreaItemType::Attribute => Ok(attribute_target_key(&self.target_attributes)),
            AreaItemType::Magazine => match self.area_item_id {
                80 => Ok(PERFORMANCE_KEY.to_owned()),
                81 => Ok(TECHNIQUE_KEY.to_owned()),
                82 => Ok(VISUAL_KEY.to_owned()),
                _ => Err(PreparationError::UnsupportedAreaItemTarget {
                    area_item_id: self.area_item_id,
                }),
            },
        }
    }

    pub fn percent(&self, level: u8) -> StatRate {
        if level == 0 {
            return StatRate::zero();
        }
        self.percents
            .get(&level)
            .copied()
            .unwrap_or_else(StatRate::zero)
    }
}

fn band_target_key(target_band_ids: &[u32]) -> String {
    if target_band_ids.is_empty() {
        return ALL_BAND_KEY.to_owned();
    }
    let mut band_ids = target_band_ids.to_vec();
    band_ids.sort_unstable();
    band_ids.dedup();
    band_ids
        .into_iter()
        .map(|band_id| band_id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn attribute_target_key(target_attributes: &[Attribute]) -> String {
    if target_attributes.is_empty() {
        return ALL_ATTRIBUTE_KEY.to_owned();
    }
    let mut attributes = target_attributes.to_vec();
    attributes.sort_unstable_by_key(|attribute| attribute.as_str());
    attributes.dedup();
    attributes
        .into_iter()
        .map(|attribute| attribute.as_str().to_owned())
        .collect::<Vec<_>>()
        .join(",")
}

fn band_key_matches(key: &str, band_id: u32) -> bool {
    key == ALL_BAND_KEY
        || key
            .split(',')
            .filter_map(|part| part.parse::<u32>().ok())
            .any(|target_band_id| target_band_id == band_id)
}

fn attribute_key_matches(key: &str, attribute: Attribute) -> bool {
    key == ALL_ATTRIBUTE_KEY || key.split(',').any(|part| part == attribute.as_str())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaItemPercent {
    pub band: BTreeMap<String, StatRate>,
    pub attribute: BTreeMap<String, StatRate>,
    pub magazine: BTreeMap<String, StatRate>,
}

impl AreaItemPercent {
    pub fn empty() -> Self {
        Self {
            band: BTreeMap::new(),
            attribute: BTreeMap::new(),
            magazine: BTreeMap::new(),
        }
    }

    fn entry_mut(&mut self, item_type: AreaItemType, key: String) -> &mut StatRate {
        match item_type {
            AreaItemType::Band => self.band.entry(key).or_insert_with(StatRate::zero),
            AreaItemType::Attribute => self.attribute.entry(key).or_insert_with(StatRate::zero),
            AreaItemType::Magazine => self.magazine.entry(key).or_insert_with(StatRate::zero),
        }
    }
}

pub fn prepare_card(
    card: &CardDefinition,
    player_card: &PlayerCardConfig,
    character_bonus: &CharacterBonusConfig,
    event: &EventBonus,
) -> Result<PreparedCard, PreparationError> {
    let level = card_level(card, player_card)?;
    let base_stat =
        card.level_stats
            .get(&level)
            .copied()
            .ok_or(PreparationError::InvalidCardLevel {
                card_id: card.card_id,
                level,
            })?;
    let mut stat = StatValue::from(base_stat);
    if player_card.training {
        stat.add(card.training_stat.into());
    }
    for (enabled, episode_stat) in player_card
        .episodes
        .iter()
        .copied()
        .zip(card.episode_stats.iter().copied())
    {
        if enabled {
            stat.add(episode_stat.into());
        }
    }
    stat.add_each(card.rarity as f64 * player_card.limit_break_rank as f64 * 50.0);

    let potential = stat
        .mul_rate(character_bonus.potential.into())
        .floor_components();
    let character_task = stat
        .mul_rate(character_bonus.character_task.into())
        .floor_components();
    stat.add(potential);
    stat.add(character_task);

    let event_add_stat = stat.mul_rate(event_percent(card, player_card.limit_break_rank, event));
    let duration = card
        .skill
        .durations
        .get(player_card.skill_level.saturating_sub(1) as usize)
        .copied()
        .ok_or(PreparationError::InvalidSkillLevel {
            card_id: card.card_id,
            skill_level: player_card.skill_level,
        })?;

    Ok(PreparedCard {
        card_id: card.card_id,
        character_id: card.character_id,
        band_id: card.band_id,
        rarity: card.rarity,
        attribute: card.attribute,
        level,
        training: player_card.training,
        illust_training_status: player_card.illust_training_status,
        episodes: player_card.episodes,
        limit_break_rank: player_card.limit_break_rank,
        skill_level: player_card.skill_level,
        stat,
        event_add_stat,
        skill: TeamCardSkill {
            card_id: card.card_id,
            duration,
            score_up: card.skill.score_up.default,
            rateup: card.skill.rateup,
        },
        score_up: card.skill.score_up,
    })
}

fn card_level(
    card: &CardDefinition,
    player_card: &PlayerCardConfig,
) -> Result<u8, PreparationError> {
    if !player_card.level_is_auto_max() {
        return Ok(player_card.level);
    }
    card.level_stats
        .keys()
        .next_back()
        .copied()
        .ok_or(PreparationError::InvalidCardLevel {
            card_id: card.card_id,
            level: player_card.level,
        })
}

pub fn prepare_cards(
    cards: &[CardDefinition],
    player_cards: &BTreeMap<String, PlayerCardConfig>,
    character_bonuses: &BTreeMap<String, CharacterBonusConfig>,
    event: &EventBonus,
) -> Result<Vec<PreparedCard>, PreparationError> {
    cards
        .iter()
        .map(|card| {
            let player_card = player_cards.get(&card.card_id.to_string()).ok_or(
                PreparationError::MissingPlayerCardConfig {
                    card_id: card.card_id,
                },
            )?;
            let character_bonus = character_bonuses
                .get(&card.character_id.to_string())
                .ok_or(PreparationError::MissingCharacterBonus {
                    character_id: card.character_id,
                })?;
            prepare_card(card, player_card, character_bonus, event)
        })
        .collect()
}

pub fn calculate_area_item_percent(
    player_area_items: &BTreeMap<String, AreaItemConfig>,
    area_item_definitions: &BTreeMap<u32, AreaItemDefinition>,
) -> Result<AreaItemPercent, PreparationError> {
    let mut result = AreaItemPercent::empty();

    for (area_item_id, config) in player_area_items {
        let area_item_id = area_item_id
            .parse::<u32>()
            .map_err(|_| PreparationError::MissingAreaItemDefinition { area_item_id: 0 })?;
        let definition = area_item_definitions
            .get(&area_item_id)
            .ok_or(PreparationError::MissingAreaItemDefinition { area_item_id })?;

        let item_type = definition.item_type();
        let key = definition.target_key()?;
        result
            .entry_mut(item_type, key)
            .add(definition.percent(config.level));
    }

    apply_shell_and_coffee_adjustment(&mut result, player_area_items, area_item_definitions)?;

    Ok(result)
}

fn apply_shell_and_coffee_adjustment(
    result: &mut AreaItemPercent,
    player_area_items: &BTreeMap<String, AreaItemConfig>,
    area_item_definitions: &BTreeMap<u32, AreaItemDefinition>,
) -> Result<(), PreparationError> {
    let Some(shell) = player_area_items.get("59") else {
        return Ok(());
    };
    let Some(coffee) = player_area_items.get("72") else {
        return Ok(());
    };

    let min_area_item_id = if shell.level < coffee.level { 59 } else { 72 };
    let Some(definition) = area_item_definitions.get(&min_area_item_id) else {
        return Err(PreparationError::MissingAreaItemDefinition {
            area_item_id: min_area_item_id,
        });
    };
    let adjustment = definition.percent(player_area_items[&min_area_item_id.to_string()].level);
    let key = definition.target_key()?;
    let all_attribute = result.attribute.entry(key).or_insert_with(StatRate::zero);

    all_attribute.performance -= adjustment.performance;
    all_attribute.technique -= adjustment.technique;
    all_attribute.visual -= adjustment.visual;

    Ok(())
}

fn event_percent(card: &CardDefinition, limit_break_rank: u8, event: &EventBonus) -> StatRate {
    let mut rate = StatRate::zero();
    let mut matched_attribute = false;
    let mut matched_character = false;

    for bonus in &event.attributes {
        if bonus.attribute == card.attribute {
            matched_attribute = true;
            rate.add(StatRate::all(bonus.percent));
        }
    }

    for bonus in &event.characters {
        if bonus.character_id == card.character_id {
            matched_character = true;
            rate.add(StatRate::all(bonus.percent));
        }
    }

    for bonus in &event.members {
        if bonus.card_id == card.card_id {
            rate.add(StatRate::all(bonus.percent));
        }
    }

    if matched_attribute && matched_character {
        if let Some(parameter_bonus) = event.event_character_parameter_bonus {
            rate.add(parameter_bonus);
        }
        rate.add(StatRate::all(
            event.event_attribute_and_character_parameter_percent,
        ));
    }

    if let Some(percent) = event
        .limit_breaks
        .get(&card.rarity)
        .and_then(|by_rank| by_rank.get(&limit_break_rank))
    {
        rate.add(StatRate::all(*percent));
    }

    rate.div_scalar(100.0)
}

impl Attribute {
    pub fn as_str(self) -> &'static str {
        match self {
            Attribute::Cool => "cool",
            Attribute::Happy => "happy",
            Attribute::Pure => "pure",
            Attribute::Powerful => "powerful",
            Attribute::All => ALL_ATTRIBUTE_KEY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::schema::{
        AreaItemConfig, CharacterBonusConfig, PlayerCardConfig, StatRate as SchemaStatRate,
    };

    fn card_definition() -> CardDefinition {
        CardDefinition {
            card_id: 101,
            character_id: 5,
            band_id: 2,
            rarity: 4,
            attribute: Attribute::Cool,
            level_stats: BTreeMap::from([(
                60,
                Stat {
                    performance: 900,
                    technique: 1900,
                    visual: 2900,
                },
            )]),
            training_stat: Stat {
                performance: 50,
                technique: 50,
                visual: 50,
            },
            episode_stats: [
                Stat {
                    performance: 10,
                    technique: 20,
                    visual: 30,
                },
                Stat {
                    performance: 40,
                    technique: 30,
                    visual: 20,
                },
            ],
            skill: SkillDefinition {
                durations: vec![5.0, 5.5, 6.0, 6.5, 7.0],
                score_up: ScoreUp {
                    default: 1.15,
                    unification_activate_effect_value: None,
                    unification_activate_condition_band_id: None,
                    unification_activate_condition_type: None,
                },
                rateup: false,
            },
        }
    }

    fn player_card() -> PlayerCardConfig {
        PlayerCardConfig {
            level: 60,
            training: true,
            illust_training_status: true,
            episodes: [true, true],
            limit_break_rank: 2,
            skill_level: 5,
        }
    }

    #[test]
    fn prepares_card_stat_and_event_bonus_like_ts_card_info() {
        let mut limit_breaks = BTreeMap::new();
        limit_breaks.insert(4, BTreeMap::from([(2, 2.0)]));
        let event = EventBonus {
            attributes: vec![EventAttributeBonus {
                attribute: Attribute::Cool,
                percent: 20.0,
            }],
            characters: vec![EventCharacterBonus {
                character_id: 5,
                percent: 10.0,
            }],
            members: vec![EventMemberBonus {
                card_id: 101,
                percent: 50.0,
            }],
            event_character_parameter_bonus: Some(StatRate {
                performance: 1.0,
                technique: 2.0,
                visual: 3.0,
            }),
            event_attribute_and_character_parameter_percent: 20.0,
            limit_breaks,
        };
        let bonus = CharacterBonusConfig {
            potential: SchemaStatRate {
                performance: 0.01,
                technique: 0.02,
                visual: 0.03,
            },
            character_task: SchemaStatRate {
                performance: 0.02,
                technique: 0.01,
                visual: 0.0,
            },
        };

        let prepared = prepare_card(&card_definition(), &player_card(), &bonus, &event).unwrap();

        assert_eq!(
            prepared.stat,
            StatValue {
                performance: 1442.0,
                technique: 2472.0,
                visual: 3502.0,
            }
        );
        assert_stat_close(
            prepared.event_add_stat,
            StatValue {
                performance: 1485.26,
                technique: 2570.88,
                visual: 3677.1,
            },
        );
        assert_eq!(prepared.skill.duration, 7.0);
        assert_eq!(prepared.skill.score_up, 1.15);
    }

    #[test]
    fn calculates_add_up_stat_for_selected_area_items() {
        let prepared = PreparedCard {
            card_id: 1,
            character_id: 1,
            band_id: 2,
            rarity: 4,
            attribute: Attribute::Cool,
            level: 60,
            training: true,
            illust_training_status: true,
            episodes: [true, true],
            limit_break_rank: 0,
            skill_level: 1,
            stat: StatValue {
                performance: 100.0,
                technique: 200.0,
                visual: 300.0,
            },
            event_add_stat: StatValue {
                performance: 10.0,
                technique: 20.0,
                visual: 30.0,
            },
            skill: TeamCardSkill {
                card_id: 1,
                duration: 7.0,
                score_up: 1.0,
                rateup: false,
            },
            score_up: ScoreUp {
                default: 1.0,
                unification_activate_effect_value: None,
                unification_activate_condition_band_id: None,
                unification_activate_condition_type: None,
            },
        };
        let area = AreaItemPercent {
            band: BTreeMap::from([("2".to_owned(), StatRate::all(0.1))]),
            attribute: BTreeMap::from([("cool".to_owned(), StatRate::all(0.2))]),
            magazine: BTreeMap::from([(
                PERFORMANCE_KEY.to_owned(),
                StatRate {
                    performance: 0.3,
                    technique: 0.0,
                    visual: 0.0,
                },
            )]),
        };

        let stat = prepared.add_up_stat(&area, "2", "cool", PERFORMANCE_KEY);

        assert_eq!(stat, 870.0);
    }

    #[test]
    fn area_item_percent_groups_items_by_target() {
        let player_items = BTreeMap::from([
            ("10".to_owned(), AreaItemConfig { level: 1 }),
            ("20".to_owned(), AreaItemConfig { level: 2 }),
            ("80".to_owned(), AreaItemConfig { level: 1 }),
        ]);
        let defs = BTreeMap::from([
            (
                10,
                AreaItemDefinition {
                    area_item_id: 10,
                    target_band_ids: vec![2],
                    target_attributes: vec![],
                    percents: BTreeMap::from([(1, StatRate::all(0.1))]),
                },
            ),
            (
                20,
                AreaItemDefinition {
                    area_item_id: 20,
                    target_band_ids: vec![],
                    target_attributes: vec![Attribute::Cool],
                    percents: BTreeMap::from([(2, StatRate::all(0.2))]),
                },
            ),
            (
                80,
                AreaItemDefinition {
                    area_item_id: 80,
                    target_band_ids: vec![],
                    target_attributes: vec![],
                    percents: BTreeMap::from([(
                        1,
                        StatRate {
                            performance: 0.3,
                            technique: 0.0,
                            visual: 0.0,
                        },
                    )]),
                },
            ),
        ]);

        let area = calculate_area_item_percent(&player_items, &defs).unwrap();

        assert_eq!(area.band["2"], StatRate::all(0.1));
        assert_eq!(area.attribute["cool"], StatRate::all(0.2));
        assert_eq!(area.magazine[PERFORMANCE_KEY].performance, 0.3);
    }

    #[test]
    fn area_item_shell_coffee_adjustment_keeps_larger_bonus() {
        let player_items = BTreeMap::from([
            ("59".to_owned(), AreaItemConfig { level: 1 }),
            ("72".to_owned(), AreaItemConfig { level: 2 }),
        ]);
        let defs = BTreeMap::from([
            (
                59,
                AreaItemDefinition {
                    area_item_id: 59,
                    target_band_ids: vec![],
                    target_attributes: vec![
                        Attribute::Powerful,
                        Attribute::Pure,
                        Attribute::Cool,
                        Attribute::Happy,
                    ],
                    percents: BTreeMap::from([(1, StatRate::all(0.1))]),
                },
            ),
            (
                72,
                AreaItemDefinition {
                    area_item_id: 72,
                    target_band_ids: vec![],
                    target_attributes: vec![
                        Attribute::Powerful,
                        Attribute::Pure,
                        Attribute::Cool,
                        Attribute::Happy,
                    ],
                    percents: BTreeMap::from([(2, StatRate::all(0.2))]),
                },
            ),
        ]);

        let area = calculate_area_item_percent(&player_items, &defs).unwrap();

        assert_rate_close(
            area.attribute["cool,happy,powerful,pure"],
            StatRate::all(0.2),
        );
    }

    #[test]
    fn multi_target_band_area_item_uses_actual_target_set_key() {
        let definition = AreaItemDefinition {
            area_item_id: 73,
            target_band_ids: vec![1, 2, 3, 4, 5, 18, 21, 45],
            target_attributes: vec![
                Attribute::Powerful,
                Attribute::Pure,
                Attribute::Cool,
                Attribute::Happy,
            ],
            percents: BTreeMap::new(),
        };

        assert_eq!(definition.item_type(), AreaItemType::Band);
        assert_eq!(definition.target_key().unwrap(), "1,2,3,4,5,18,21,45");
    }

    #[test]
    fn multi_target_attribute_area_item_uses_actual_target_set_key() {
        let definition = AreaItemDefinition {
            area_item_id: 56,
            target_band_ids: vec![],
            target_attributes: vec![Attribute::Powerful, Attribute::Pure],
            percents: BTreeMap::new(),
        };

        assert_eq!(definition.item_type(), AreaItemType::Attribute);
        assert_eq!(definition.target_key().unwrap(), "powerful,pure");
    }

    #[test]
    fn add_up_stat_matches_multi_target_area_item_keys() {
        let prepared = PreparedCard {
            card_id: 1,
            character_id: 1,
            band_id: 4,
            rarity: 4,
            attribute: Attribute::Pure,
            level: 60,
            training: true,
            illust_training_status: true,
            episodes: [true, true],
            limit_break_rank: 0,
            skill_level: 1,
            stat: StatValue {
                performance: 100.0,
                technique: 100.0,
                visual: 100.0,
            },
            event_add_stat: StatValue::zero(),
            skill: TeamCardSkill {
                card_id: 1,
                duration: 7.0,
                score_up: 1.0,
                rateup: false,
            },
            score_up: ScoreUp {
                default: 1.0,
                unification_activate_effect_value: None,
                unification_activate_condition_band_id: None,
                unification_activate_condition_type: None,
            },
        };
        let area = AreaItemPercent {
            band: BTreeMap::from([("1,2,3,4,5,18,21,45".to_owned(), StatRate::all(0.1))]),
            attribute: BTreeMap::from([(
                "cool,happy,pure,powerful".to_owned(),
                StatRate::all(0.2),
            )]),
            magazine: BTreeMap::new(),
        };

        assert_eq!(
            prepared.add_up_stat(
                &area,
                "1,2,3,4,5,18,21,45",
                "cool,happy,pure,powerful",
                PERFORMANCE_KEY,
            ),
            390.0
        );
    }

    fn assert_rate_close(actual: StatRate, expected: StatRate) {
        assert!((actual.performance - expected.performance).abs() < 1e-9);
        assert!((actual.technique - expected.technique).abs() < 1e-9);
        assert!((actual.visual - expected.visual).abs() < 1e-9);
    }

    fn assert_stat_close(actual: StatValue, expected: StatValue) {
        assert!((actual.performance - expected.performance).abs() < 1e-9);
        assert!((actual.technique - expected.technique).abs() < 1e-9);
        assert!((actual.visual - expected.visual).abs() < 1e-9);
    }
}
