mod area_item;
mod card;
mod skill;

use crate::{
    utils::{into_object_map, merge_object_map, normalize_object},
    DataError,
};
use bangdream_optimize_core::{AreaItemDefinition, CardDefinition, Server};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct BestdoriData {
    cards: BTreeMap<String, Value>,
    characters: BTreeMap<String, Value>,
    skills: BTreeMap<String, Value>,
    area_items: BTreeMap<String, Value>,
}

impl BestdoriData {
    pub fn from_values(
        mut cards: Value,
        characters: Value,
        mut skills: Value,
        mut area_items: Value,
    ) -> Result<Self, DataError> {
        normalize_object(&mut cards, "cards")?;
        normalize_object(&mut skills, "skills")?;
        normalize_object(&mut area_items, "areaItems")?;

        Ok(Self {
            cards: into_object_map(cards, "cards")?,
            characters: into_object_map(characters, "characters")?,
            skills: into_object_map(skills, "skills")?,
            area_items: into_object_map(area_items, "areaItems")?,
        })
    }

    pub fn apply_repairs(
        &mut self,
        cards_fix: Option<Value>,
        skills_fix: Option<Value>,
        area_items_fix: Option<Value>,
    ) -> Result<(), DataError> {
        if let Some(cards_fix) = cards_fix {
            merge_object_map(&mut self.cards, cards_fix, "cardsCNfix")?;
        }
        if let Some(skills_fix) = skills_fix {
            merge_object_map(&mut self.skills, skills_fix, "skillsCNfix")?;
        }
        if let Some(area_items_fix) = area_items_fix {
            merge_object_map(&mut self.area_items, area_items_fix, "areaItemFix")?;
        }
        Ok(())
    }

    pub fn card_definition(&self, card_id: u32) -> Result<CardDefinition, DataError> {
        let card = self.entity(&self.cards, "card", card_id)?;
        self.card_definition_from_card(card_id, card)
    }

    pub fn card_has_level(&self, card_id: u32, level: u8) -> Result<bool, DataError> {
        let card = self.entity(&self.cards, "card", card_id)?;
        Ok(card_has_level(card, level))
    }

    pub fn card_definition_with_detail(
        &self,
        card_id: u32,
        detail: &Value,
    ) -> Result<CardDefinition, DataError> {
        let mut card = self.entity(&self.cards, "card", card_id)?.clone();
        if let (Value::Object(card), Some(stat)) = (&mut card, detail.get("stat")) {
            card.insert("stat".to_owned(), stat.clone());
        }
        self.card_definition_from_card(card_id, &card)
    }

    fn card_definition_from_card(
        &self,
        card_id: u32,
        card: &Value,
    ) -> Result<CardDefinition, DataError> {
        let character_id = card::character_id(card)?;
        let character = self.entity(&self.characters, "character", character_id)?;
        let skill_id = card::skill_id(card)?;
        let skill = self.entity(&self.skills, "skill", skill_id)?;

        card::card_definition(card_id, card, character, skill)
    }

    pub fn area_item_definitions(
        &self,
        server: Server,
    ) -> Result<BTreeMap<u32, AreaItemDefinition>, DataError> {
        let server_index = server_index(server);
        self.area_items
            .iter()
            .map(|(id, value)| {
                let area_item_id = id.parse::<u32>().map_err(|_| DataError::InvalidField {
                    field: "areaItemId",
                    value: id.clone(),
                })?;
                Ok((
                    area_item_id,
                    area_item::area_item_definition(area_item_id, value, server_index)?,
                ))
            })
            .collect()
    }

    fn entity<'a>(
        &self,
        map: &'a BTreeMap<String, Value>,
        kind: &'static str,
        id: u32,
    ) -> Result<&'a Value, DataError> {
        map.get(&id.to_string()).ok_or(DataError::MissingEntity {
            kind,
            id: id.to_string(),
        })
    }
}

fn server_index(server: Server) -> usize {
    match server {
        Server::Jp => 0,
        Server::En => 1,
        Server::Tw => 2,
        Server::Cn => 3,
        Server::Kr => 4,
    }
}

fn card_has_level(card: &Value, level: u8) -> bool {
    card.get("stat")
        .and_then(Value::as_object)
        .is_some_and(|stat| stat.contains_key(&level.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_card_definition_with_skill_and_character_band() {
        let data = BestdoriData::from_values(
            json!({
                "101": {
                    "characterId": 5,
                    "rarity": 4,
                    "attribute": "cool",
                    "skillId": 9,
                    "stat": {
                        "1": {"performance": 1, "technique": 2, "visual": 3},
                        "60": {"performance": 1000, "technique": 2000, "visual": 3000},
                        "training": {"performance": 100, "technique": 100, "visual": 100},
                        "episodes": [
                            {"performance": 10, "technique": 20, "visual": 30},
                            {"performance": 40, "technique": 50, "visual": 60}
                        ]
                    }
                }
            }),
            json!({"5": {"bandId": 2}}),
            json!({
                "9": {
                    "duration": [5, 5.5, 6, 6.5, 7],
                    "activationEffect": {
                        "activateEffectTypes": {
                            "score": {"activateEffectValue": [60, 70, 80, 90, 100]}
                        }
                    }
                }
            }),
            json!({}),
        )
        .unwrap();

        let card = data.card_definition(101).unwrap();

        assert_eq!(card.band_id, 2);
        assert_eq!(card.level_stats[&60].performance, 1000);
        assert_eq!(card.training_stat.performance, 100);
        assert_eq!(card.episode_stats[0].performance, 10);
        assert_eq!(card.episode_stats[1].performance, 40);
        assert_eq!(card.skill.durations[4], 7.0);
        assert_eq!(card.skill.score_up.default, 1.0);
    }

    #[test]
    fn maps_card_definition_with_detail_stat_override() {
        let data = BestdoriData::from_values(
            json!({
                "101": {
                    "characterId": 5,
                    "rarity": 4,
                    "attribute": "cool",
                    "skillId": 9,
                    "stat": {
                        "1": {"performance": 1, "technique": 2, "visual": 3},
                        "training": {"performance": 100, "technique": 100, "visual": 100},
                        "episodes": []
                    }
                }
            }),
            json!({"5": {"bandId": 2}}),
            json!({
                "9": {
                    "duration": [5, 5, 5, 5, 5],
                    "activationEffect": {
                        "activateEffectTypes": {
                            "score": {"activateEffectValue": [100]}
                        }
                    }
                }
            }),
            json!({}),
        )
        .unwrap();

        assert!(!data.card_has_level(101, 60).unwrap());

        let card = data
            .card_definition_with_detail(
                101,
                &json!({
                    "stat": {
                        "1": {"performance": 1, "technique": 2, "visual": 3},
                        "60": {"performance": 1000, "technique": 2000, "visual": 3000},
                        "training": {"performance": 900, "technique": 900, "visual": 900}
                    }
                }),
            )
            .unwrap();

        assert_eq!(card.level_stats[&60].performance, 1000);
        assert_eq!(card.training_stat.performance, 900);
    }

    #[test]
    fn maps_area_items() {
        let data = BestdoriData::from_values(
            json!({}),
            json!({}),
            json!({}),
            json!({
                "80": {
                    "targetBandIds": [],
                    "targetAttributes": [],
                    "performance": {"1": ["30"]},
                    "technique": {"1": ["0"]},
                    "visual": {"1": ["0"]}
                },
                "10": {
                    "targetBandIds": [2],
                    "targetAttributes": [],
                    "performance": {"1": ["10"]},
                    "technique": {"1": ["10"]},
                    "visual": {"1": ["10"]}
                }
            }),
        )
        .unwrap();

        let defs = data.area_item_definitions(Server::Jp).unwrap();

        assert_eq!(defs[&80].percents[&1].performance, 0.3);
        assert_eq!(defs[&10].target_band_ids, vec![2]);
    }

    #[test]
    fn maps_area_items_by_server_and_falls_back_when_server_value_is_null() {
        let data = BestdoriData::from_values(
            json!({}),
            json!({}),
            json!({}),
            json!({
                "10": {
                    "targetBandIds": [2],
                    "targetAttributes": [],
                    "performance": {"1": [null, "20"]},
                    "technique": {"1": [null, "10"]},
                    "visual": {"1": [null, "5"]}
                },
                "11": {
                    "targetBandIds": [3],
                    "targetAttributes": [],
                    "performance": {"1": [null, null]},
                    "technique": {"1": [null, null]},
                    "visual": {"1": [null, null]}
                }
            }),
        )
        .unwrap();

        let jp_defs = data.area_item_definitions(Server::Jp).unwrap();
        let en_defs = data.area_item_definitions(Server::En).unwrap();

        assert_eq!(jp_defs[&10].percents[&1].performance, 0.2);
        assert_eq!(en_defs[&10].percents[&1].performance, 0.2);
        assert_eq!(en_defs[&10].percents[&1].technique, 0.1);
        assert_eq!(en_defs[&10].percents[&1].visual, 0.05);
        assert_eq!(jp_defs[&11].percents[&1].performance, 0.0);
    }
}
