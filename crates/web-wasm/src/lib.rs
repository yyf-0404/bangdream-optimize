use bangdream_optimize_core::{
    AreaItemDefinition, BuildResult, CardDefinition, EventType, ItemSearchOptions, PlayerConfig,
    PreferredItemTarget, PtMaximizeRequest, PtMaximizeResult, ScoreRangeChartMetaFile,
    ScoreRangeRequest, ScoreRangeResult, Server,
};
use bangdream_optimize_data::{
    chart_from_bestdori, event_bonus, published_score_range_song_selections, BestdoriData,
    DataError, GameDataSnapshot, SnapshotMaximizeInputBuilder, SnapshotPtMaximizeInputBuilder,
    SnapshotScoreRangeInputBuilder,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebCalculationPayload {
    pub cards: Value,
    pub characters: Value,
    pub skills: Value,
    pub area_items: Value,
    #[serde(default)]
    pub cards_fix: Option<Value>,
    #[serde(default)]
    pub skills_fix: Option<Value>,
    #[serde(default)]
    pub area_items_fix: Option<Value>,
    pub event: Value,
    pub songs: BTreeMap<String, Value>,
    pub charts: Vec<WebChartInput>,
    pub player: PlayerConfig,
    pub server: Server,
    #[serde(default)]
    pub event_id: Option<u32>,
    #[serde(default)]
    pub options: ItemSearchOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebChartInput {
    pub song_id: u32,
    pub difficulty: u8,
    pub data: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebScoreRangePayload {
    pub cards: Value,
    pub characters: Value,
    pub skills: Value,
    pub area_items: Value,
    #[serde(default)]
    pub cards_fix: Option<Value>,
    #[serde(default)]
    pub skills_fix: Option<Value>,
    #[serde(default)]
    pub area_items_fix: Option<Value>,
    pub event: Value,
    pub songs: BTreeMap<String, Value>,
    pub score_range_chart_meta: ScoreRangeChartMetaFile,
    pub player: PlayerConfig,
    pub server: Server,
    #[serde(default)]
    pub event_id: Option<u32>,
    pub request: ScoreRangeRequest,
    pub now_millis: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebPtMaximizePayload {
    pub cards: Value,
    pub characters: Value,
    pub skills: Value,
    pub area_items: Value,
    #[serde(default)]
    pub cards_fix: Option<Value>,
    #[serde(default)]
    pub skills_fix: Option<Value>,
    #[serde(default)]
    pub area_items_fix: Option<Value>,
    pub event: Value,
    pub songs: BTreeMap<String, Value>,
    pub charts: Vec<WebChartInput>,
    pub player: PlayerConfig,
    pub server: Server,
    #[serde(default)]
    pub event_id: Option<u32>,
    pub request: PtMaximizeRequest,
}

#[wasm_bindgen(js_name = calculateFromStaticData)]
pub fn calculate_from_static_data(payload_json: &str) -> Result<String, JsValue> {
    let payload: WebCalculationPayload =
        serde_json::from_str(payload_json).map_err(|err| JsValue::from_str(&err.to_string()))?;
    calculate_payload(payload)
        .and_then(|result| {
            serde_json::to_string(&result).map_err(|err| DataError::JsonString {
                message: err.to_string(),
            })
        })
        .map_err(|err| JsValue::from_str(&err.to_string()))
}

#[wasm_bindgen(js_name = scoreRangeFromStaticData)]
pub fn score_range_from_static_data(payload_json: &str) -> Result<String, JsValue> {
    let payload: WebScoreRangePayload =
        serde_json::from_str(payload_json).map_err(|err| JsValue::from_str(&err.to_string()))?;
    score_range_payload(payload)
        .and_then(|result| {
            serde_json::to_string(&result).map_err(|err| DataError::JsonString {
                message: err.to_string(),
            })
        })
        .map_err(|err| JsValue::from_str(&err.to_string()))
}

#[wasm_bindgen(js_name = ptMaximizeFromStaticData)]
pub fn pt_maximize_from_static_data(payload_json: &str) -> Result<String, JsValue> {
    let payload: WebPtMaximizePayload =
        serde_json::from_str(payload_json).map_err(|err| JsValue::from_str(&err.to_string()))?;
    pt_maximize_payload(payload)
        .and_then(|result| {
            serde_json::to_string(&result).map_err(|err| DataError::JsonString {
                message: err.to_string(),
            })
        })
        .map_err(|err| JsValue::from_str(&err.to_string()))
}

pub fn calculate_payload(payload: WebCalculationPayload) -> Result<BuildResult, DataError> {
    let event_id = payload
        .event_id
        .or(payload.player.current_event)
        .ok_or(DataError::MissingCurrentEvent)?;
    let event_type = event_type(&payload.event)?;
    let mut game_data = BestdoriData::from_values(
        payload.cards,
        payload.characters,
        payload.skills,
        payload.area_items,
    )?;
    game_data.apply_repairs(
        payload.cards_fix,
        payload.skills_fix,
        payload.area_items_fix,
    )?;

    let card_definitions = card_definitions(&payload.player, &game_data)?;
    let area_item_definitions = area_item_definitions(&payload.player, payload.server, &game_data)?;
    let mut snapshot = GameDataSnapshot::new(
        card_definitions,
        area_item_definitions,
        BTreeMap::from([(
            event_id,
            bangdream_optimize_data::EventData {
                event_type,
                event_bonus: event_bonus(&payload.event)?,
                preferred: preferred_item_target(&payload.event),
            },
        )]),
    );

    for chart in payload.charts {
        let level = song_level(&payload.songs, chart.song_id, chart.difficulty)?;
        snapshot.insert_chart(
            chart.song_id,
            chart.difficulty,
            chart_from_bestdori(level, &chart.data)?,
        );
    }

    SnapshotMaximizeInputBuilder::new(snapshot).maximize_sync(
        payload.player,
        payload.server,
        Some(event_id),
        payload.options,
    )
}

pub fn score_range_payload(
    payload: WebScoreRangePayload,
) -> Result<Vec<ScoreRangeResult>, DataError> {
    let event_id = payload
        .event_id
        .or(payload.player.current_event)
        .ok_or(DataError::MissingCurrentEvent)?;
    let event_type = event_type(&payload.event)?;
    payload
        .score_range_chart_meta
        .validate()
        .map_err(|message| DataError::InvalidField {
            field: "scoreRangeChartMeta",
            value: message,
        })?;
    let mut game_data = BestdoriData::from_values(
        payload.cards,
        payload.characters,
        payload.skills,
        payload.area_items,
    )?;
    game_data.apply_repairs(
        payload.cards_fix,
        payload.skills_fix,
        payload.area_items_fix,
    )?;

    let card_definitions = card_definitions(&payload.player, &game_data)?;
    let area_item_definitions = area_item_definitions(&payload.player, payload.server, &game_data)?;
    let mut snapshot = GameDataSnapshot::new(
        card_definitions,
        area_item_definitions,
        BTreeMap::from([(
            event_id,
            bangdream_optimize_data::EventData {
                event_type,
                event_bonus: event_bonus(&payload.event)?,
                preferred: preferred_item_target(&payload.event),
            },
        )]),
    );
    let selections = published_score_range_song_selections(
        &payload.songs,
        payload.server,
        payload.now_millis,
        |song_id, difficulty| {
            payload
                .score_range_chart_meta
                .contains_chart(song_id, difficulty)
        },
    )?;
    for selection in selections {
        let level = song_level(&payload.songs, selection.song_id, selection.difficulty)?;
        let meta = payload
            .score_range_chart_meta
            .chart(selection.song_id, selection.difficulty)
            .cloned()
            .ok_or(DataError::MissingEntity {
                kind: "score-range chart meta",
                id: format!("{}:{}", selection.song_id, selection.difficulty),
            })?;
        snapshot.insert_score_range_chart(selection.song_id, selection.difficulty, level, meta);
    }

    SnapshotScoreRangeInputBuilder::new(snapshot).score_range_sync(
        payload.player,
        payload.server,
        Some(event_id),
        payload.request,
    )
}

pub fn pt_maximize_payload(payload: WebPtMaximizePayload) -> Result<PtMaximizeResult, DataError> {
    let event_id = payload
        .event_id
        .or(payload.player.current_event)
        .ok_or(DataError::MissingCurrentEvent)?;
    let event_type = event_type(&payload.event)?;
    let mut game_data = BestdoriData::from_values(
        payload.cards,
        payload.characters,
        payload.skills,
        payload.area_items,
    )?;
    game_data.apply_repairs(
        payload.cards_fix,
        payload.skills_fix,
        payload.area_items_fix,
    )?;

    let card_definitions = card_definitions(&payload.player, &game_data)?;
    let area_item_definitions = area_item_definitions(&payload.player, payload.server, &game_data)?;
    let mut snapshot = GameDataSnapshot::new(
        card_definitions,
        area_item_definitions,
        BTreeMap::from([(
            event_id,
            bangdream_optimize_data::EventData {
                event_type,
                event_bonus: event_bonus(&payload.event)?,
                preferred: preferred_item_target(&payload.event),
            },
        )]),
    );
    for chart in payload.charts {
        let level = song_level(&payload.songs, chart.song_id, chart.difficulty)?;
        snapshot.insert_chart(
            chart.song_id,
            chart.difficulty,
            chart_from_bestdori(level, &chart.data)?,
        );
    }

    SnapshotPtMaximizeInputBuilder::new(snapshot).pt_maximize_sync(
        payload.player,
        payload.server,
        Some(event_id),
        payload.request,
    )
}

fn card_definitions(
    player: &PlayerConfig,
    game_data: &BestdoriData,
) -> Result<BTreeMap<u32, CardDefinition>, DataError> {
    player
        .card_list
        .keys()
        .map(|card_id| {
            let parsed_id = parse_id(card_id, "cardList.cardId")?;
            Ok((parsed_id, game_data.card_definition(parsed_id)?))
        })
        .collect()
}

fn area_item_definitions(
    player: &PlayerConfig,
    server: Server,
    game_data: &BestdoriData,
) -> Result<BTreeMap<u32, AreaItemDefinition>, DataError> {
    let all_definitions = game_data.area_item_definitions(server)?;
    let mut definitions = player
        .area_item
        .keys()
        .map(|area_item_id| {
            let parsed_id = parse_id(area_item_id, "areaItem.areaItemId")?;
            let definition =
                all_definitions
                    .get(&parsed_id)
                    .cloned()
                    .ok_or(DataError::MissingEntity {
                        kind: "areaItem",
                        id: area_item_id.clone(),
                    })?;
            Ok((parsed_id, definition))
        })
        .collect::<Result<BTreeMap<_, _>, DataError>>()?;

    for area_item_id in [59, 72] {
        if let Some(definition) = all_definitions.get(&area_item_id) {
            definitions
                .entry(area_item_id)
                .or_insert_with(|| definition.clone());
        }
    }

    Ok(definitions)
}

fn parse_id(value: &str, field: &'static str) -> Result<u32, DataError> {
    value.parse::<u32>().map_err(|_| DataError::InvalidField {
        field,
        value: value.to_owned(),
    })
}

fn event_type(event: &Value) -> Result<EventType, DataError> {
    match event.get("eventType").and_then(Value::as_str) {
        Some("medley") => Ok(EventType::Medley),
        Some("versus") => Ok(EventType::Versus),
        Some("challenge") => Ok(EventType::Challenge),
        Some("festival") => Ok(EventType::Festival),
        Some("live_try") => Ok(EventType::LiveTry),
        Some("mission_live") => Ok(EventType::MissionLive),
        Some(value) => Err(DataError::InvalidField {
            field: "eventType",
            value: value.to_owned(),
        }),
        None => Err(DataError::MissingField { field: "eventType" }),
    }
}

fn preferred_item_target(event: &Value) -> Option<PreferredItemTarget> {
    let attribute = event
        .get("attributes")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("attribute"))
        .and_then(Value::as_str)?;

    Some(PreferredItemTarget {
        band: event_band(event).unwrap_or(1000).to_string(),
        attribute: attribute.to_owned(),
    })
}

fn event_band(event: &Value) -> Option<u32> {
    event
        .get("preferredBandId")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
}

fn song_level(
    songs: &BTreeMap<String, Value>,
    song_id: u32,
    difficulty: u8,
) -> Result<i32, DataError> {
    songs
        .get(&song_id.to_string())
        .and_then(|song| song.get("difficulty"))
        .and_then(|difficulty_map| difficulty_map.get(difficulty.to_string()))
        .and_then(|difficulty| difficulty.get("playLevel"))
        .and_then(Value::as_i64)
        .map(|value| value as i32)
        .ok_or(DataError::MissingField {
            field: "song.difficulty.playLevel",
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bangdream_optimize_core::{
        AreaItemConfig, CharacterBonusConfig, PlayerCardConfig, SongSelection, StatRate,
    };
    use serde_json::json;

    #[test]
    fn parses_festival_event_type() {
        assert_eq!(
            event_type(&json!({ "eventType": "festival" })).unwrap(),
            EventType::Festival,
        );
    }

    #[test]
    fn parses_mission_live_event_type() {
        assert_eq!(
            event_type(&json!({ "eventType": "mission_live" })).unwrap(),
            EventType::MissionLive,
        );
    }

    #[test]
    fn parses_live_try_event_type() {
        assert_eq!(
            event_type(&json!({ "eventType": "live_try" })).unwrap(),
            EventType::LiveTry,
        );
    }

    #[test]
    fn calculates_from_static_payload() {
        let result = calculate_payload(payload()).unwrap();

        assert_eq!(result.event_id, 100);
        assert_eq!(result.event_type, EventType::Challenge);
        assert_eq!(result.songs.len(), 1);
        assert!(result.total_score > 0);
    }

    fn payload() -> WebCalculationPayload {
        WebCalculationPayload {
            cards: cards_json(),
            characters: characters_json(),
            skills: skills_json(),
            area_items: area_items_json(),
            cards_fix: None,
            skills_fix: None,
            area_items_fix: None,
            event: json!({
                "eventType": "challenge",
                "preferredBandId": 1,
                "attributes": [{"attribute": "cool", "percent": 0}],
                "characters": [{"characterId": 1, "percent": 0}],
                "members": [],
                "eventAttributeAndCharacterBonus": {"parameterPercent": 0},
                "limitBreaks": []
            }),
            songs: BTreeMap::from([(
                "1".to_owned(),
                json!({
                    "difficulty": {
                        "3": {"playLevel": 25}
                    }
                }),
            )]),
            charts: vec![WebChartInput {
                song_id: 1,
                difficulty: 3,
                data: chart_json(),
            }],
            player: player(),
            server: Server::Jp,
            event_id: Some(100),
            options: ItemSearchOptions::default(),
        }
    }

    fn player() -> PlayerConfig {
        PlayerConfig {
            mongo_id: None,
            player_id: 123,
            current_event: Some(100),
            event_songs: BTreeMap::from([(
                "100".to_owned(),
                vec![SongSelection {
                    song_id: 1,
                    difficulty: 3,
                }],
            )]),
            event_presets: BTreeMap::new(),
            event_overrides: BTreeMap::new(),
            card_list: (1..=5)
                .map(|id| {
                    (
                        id.to_string(),
                        PlayerCardConfig {
                            level: 60,
                            training: true,
                            illust_training_status: true,
                            episodes: [true, true],
                            limit_break_rank: 0,
                            skill_level: 5,
                        },
                    )
                })
                .collect(),
            area_item: BTreeMap::from([
                ("10".to_owned(), AreaItemConfig { level: 1 }),
                ("20".to_owned(), AreaItemConfig { level: 1 }),
                ("80".to_owned(), AreaItemConfig { level: 1 }),
            ]),
            character_bouns: (1..=5)
                .map(|id| {
                    (
                        id.to_string(),
                        CharacterBonusConfig {
                            potential: StatRate {
                                performance: 0.0,
                                technique: 0.0,
                                visual: 0.0,
                            },
                            character_task: StatRate {
                                performance: 0.0,
                                technique: 0.0,
                                visual: 0.0,
                            },
                        },
                    )
                })
                .collect(),
        }
    }

    fn cards_json() -> Value {
        let mut cards = serde_json::Map::new();
        for id in 1..=5 {
            cards.insert(
                id.to_string(),
                json!({
                    "characterId": id,
                    "rarity": 4,
                    "attribute": "cool",
                    "skillId": 1,
                    "stat": {
                        "60": {"performance": 1000, "technique": 1000, "visual": 1000},
                        "training": {"performance": 0, "technique": 0, "visual": 0},
                        "episodes": []
                    }
                }),
            );
        }
        Value::Object(cards)
    }

    fn characters_json() -> Value {
        let mut characters = serde_json::Map::new();
        for id in 1..=5 {
            characters.insert(id.to_string(), json!({"bandId": 1}));
        }
        Value::Object(characters)
    }

    fn skills_json() -> Value {
        json!({
            "1": {
                "duration": [5, 5, 5, 5, 5],
                "activationEffect": {
                    "activateEffectTypes": {
                        "score": {"activateEffectValue": [100]}
                    }
                }
            }
        })
    }

    fn area_items_json() -> Value {
        json!({
            "10": {
                "targetBandIds": [1],
                "targetAttributes": [],
                "performance": {"1": ["10"]},
                "technique": {"1": ["10"]},
                "visual": {"1": ["10"]}
            },
            "20": {
                "targetBandIds": [],
                "targetAttributes": ["cool"],
                "performance": {"1": ["10"]},
                "technique": {"1": ["10"]},
                "visual": {"1": ["10"]}
            },
            "80": {
                "targetBandIds": [],
                "targetAttributes": [],
                "performance": {"1": ["10"]},
                "technique": {"1": ["0"]},
                "visual": {"1": ["0"]}
            }
        })
    }

    fn chart_json() -> Value {
        json!([
            {"type": "BPM", "beat": 0, "bpm": 120},
            {"type": "Single", "beat": 1, "skill": true},
            {"type": "Single", "beat": 2},
            {"type": "Single", "beat": 3, "skill": true},
            {"type": "Single", "beat": 4},
            {"type": "Single", "beat": 5, "skill": true},
            {"type": "Single", "beat": 6},
            {"type": "Single", "beat": 7, "skill": true},
            {"type": "Single", "beat": 8},
            {"type": "Single", "beat": 9, "skill": true},
            {"type": "Single", "beat": 10},
            {"type": "Single", "beat": 11, "skill": true}
        ])
    }
}
