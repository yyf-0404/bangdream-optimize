mod area_item;
mod card;
mod skill;
pub mod timing_skill;

use crate::{
    utils::{into_object_map, merge_object_map, normalize_object},
    DataError,
};
use bangdream_optimize_core::{AreaItemDefinition, CardDefinition, Server};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct BestdoriData {
    // Calculation contexts share immutable catalogs. Repairing a cloned
    // context detaches only the affected catalog, preserving clone isolation.
    cards: Arc<BTreeMap<String, Value>>,
    characters: Arc<BTreeMap<String, Value>>,
    skills: Arc<BTreeMap<String, Value>>,
    area_items: Arc<BTreeMap<String, Value>>,
    custom_timing_cards: BTreeMap<u32, CardDefinition>,
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
            cards: Arc::new(into_object_map(cards, "cards")?),
            characters: Arc::new(into_object_map(characters, "characters")?),
            skills: Arc::new(into_object_map(skills, "skills")?),
            area_items: Arc::new(into_object_map(area_items, "areaItems")?),
            custom_timing_cards: BTreeMap::new(),
        })
    }

    pub(crate) fn with_custom_timing_definitions(&self, definitions: &[CardDefinition]) -> Self {
        let mut data = self.clone();
        data.custom_timing_cards = definitions.iter().map(|d| (d.card_id, d.clone())).collect();
        data
    }

    pub fn apply_repairs(
        &mut self,
        cards_fix: Option<Value>,
        skills_fix: Option<Value>,
        area_items_fix: Option<Value>,
    ) -> Result<(), DataError> {
        if let Some(cards_fix) = cards_fix {
            merge_object_map(Arc::make_mut(&mut self.cards), cards_fix, "cardsCNfix")?;
        }
        if let Some(skills_fix) = skills_fix {
            merge_object_map(Arc::make_mut(&mut self.skills), skills_fix, "skillsCNfix")?;
        }
        if let Some(area_items_fix) = area_items_fix {
            merge_object_map(Arc::make_mut(&mut self.area_items), area_items_fix, "areaItemFix")?;
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
    fn repairing_a_cloned_catalog_keeps_the_original_context_unchanged() {
        let base=BestdoriData::from_values(json!({"1":{"value":1}}),json!({}),
            json!({"1":{"value":2}}),json!({"1":{"value":3}})).unwrap();
        let mut repaired=base.clone();
        repaired.apply_repairs(Some(json!({"1":{"value":10}})),
            Some(json!({"1":{"value":20}})),Some(json!({"1":{"value":30}}))).unwrap();
        for (original,changed,expected) in [
            (&base.cards,&repaired.cards,1),(&base.skills,&repaired.skills,2),
            (&base.area_items,&repaired.area_items,3),
        ] {
            assert_eq!(base.entity(original,"catalog",1).unwrap()["value"],expected);
            assert_eq!(repaired.entity(changed,"catalog",1).unwrap()["value"],expected*10);
        }
    }

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
    fn cn_source_repairs_cover_intermediate_levels_and_skill_id_collisions() {
        // Expectations are from the field-bound CN master, including level 37,
        // rather than interpolating the catalogue's two endpoint records.
        let skill = |percent| json!({
            "duration": [5, 5, 5, 5, 5],
            "activationEffect": {"activateEffectTypes": {
                "score": {"activateEffectValue": [percent]}
            }}
        });
        let mut data = BestdoriData::from_values(
            json!({
                "10005": {"characterId": 16, "rarity": 2, "attribute": "powerful",
                    "skillId": 5, "stat": {"1": {"performance": 1, "technique": 2, "visual": 3}}},
                "10029": {"characterId": 16, "rarity": 5, "attribute": "powerful",
                    "skillId": 66, "stat": {"1": {"performance": 4, "technique": 5, "visual": 6}}}
            }),
            json!({"16": {"bandId": 4}}),
            json!({"5": skill(5), "8": skill(8), "45": skill(45), "66": skill(66)}),
            json!({}),
        ).unwrap();
        let repair = serde_json::from_str(include_str!("../../../../var/game-data/cardsCNfix.json")).unwrap();
        data.apply_repairs(Some(repair), None, None).unwrap();
        let two_star = data.card_definition(10005).unwrap();
        let four_star = data.card_definition(10029).unwrap();
        assert_eq!(two_star.skill.score_up.default, 0.08);
        assert_eq!(four_star.skill.score_up.default, 0.45);
        assert_eq!(four_star.rarity, 4);
        assert_eq!(two_star.level_stats.len(), 30);
        assert_eq!(four_star.level_stats.len(), 60);
        for level in 1..=30 { assert!(data.card_has_level(10005, level).unwrap()); }
        for level in 1..=60 { assert!(data.card_has_level(10029, level).unwrap()); }
        assert_eq!(two_star.level_stats[&15], bangdream_optimize_core::Stat {
            performance: 2935, technique: 3421, visual: 3866,
        });
        assert_eq!(four_star.level_stats[&37], bangdream_optimize_core::Stat {
            performance: 5359, technique: 5508, visual: 5738,
        });
        assert_eq!(four_star.training_stat.performance, 400);
        assert_eq!(four_star.episode_stats[0].performance, 250);
        assert_eq!(four_star.episode_stats[1].performance, 600);
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
