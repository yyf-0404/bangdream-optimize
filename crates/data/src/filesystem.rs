use crate::{
    chart_from_bestdori, event_bonus, published_score_range_song_selections,
    utils::into_object_map, BestdoriData, DataError, EventData, GameDataSnapshot,
    MaximizeInputBuilder, ScoreRangeInputBuilder, SnapshotMaximizeInputBuilder,
    SnapshotScoreRangeInputBuilder,
};
use async_trait::async_trait;
use bangdream_optimize_core::{
    AreaItemDefinition, BuildResult, CardDefinition, EventType, ItemSearchOptions,
    PlayerCardConfig, PlayerConfig, PreferredItemTarget, ScoreRangeChartMeta,
    ScoreRangeChartMetaFile, ScoreRangeRequest, ScoreRangeResult, Server, SongSelection,
    SCORE_RANGE_CHART_META_PATH,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const CUSTOM_EVENT_ID: u32 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BestdoriFilesystemConfig {
    pub cards_path: PathBuf,
    pub characters_path: PathBuf,
    pub skills_path: PathBuf,
    pub area_items_path: PathBuf,
    pub events_path: PathBuf,
    pub songs_path: PathBuf,
    pub charts_dir: PathBuf,
    pub score_range_chart_meta_path: Option<PathBuf>,
    pub cards_dir: Option<PathBuf>,
    pub event_details_dir: Option<PathBuf>,
    pub cards_fix_path: Option<PathBuf>,
    pub skills_fix_path: Option<PathBuf>,
    pub area_items_fix_path: Option<PathBuf>,
    pub event_character_parameter_bonus_fix_path: Option<PathBuf>,
}

impl BestdoriFilesystemConfig {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        if root.join("api/cards/all.5.json").exists() {
            return Self::from_bestdori_api_root(root);
        }
        Self {
            cards_path: root.join("cards.json"),
            characters_path: root.join("characters.json"),
            skills_path: root.join("skills.json"),
            area_items_path: root.join("areaItems.json"),
            events_path: root.join("events.json"),
            songs_path: root.join("songs.json"),
            charts_dir: root.join("charts"),
            score_range_chart_meta_path: Some(root.join("scoreRangeChartMeta.1.json")),
            cards_dir: Some(root.join("cards")),
            event_details_dir: Some(root.join("events")),
            cards_fix_path: optional_default_path(&root, "cardsCNfix.json"),
            skills_fix_path: optional_default_path(&root, "skillsCNfix.json"),
            area_items_fix_path: optional_default_path(&root, "areaItemFix.json"),
            event_character_parameter_bonus_fix_path: optional_default_path(
                &root,
                "eventCharacterParameterBonusFix.json",
            ),
        }
    }

    pub fn from_bestdori_api_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            cards_path: root.join("api/cards/all.5.json"),
            characters_path: root.join("api/characters/main.3.json"),
            skills_path: root.join("api/skills/all.10.json"),
            area_items_path: root.join("api/areaItems/main.5.json"),
            events_path: root.join("api/events/all.6.json"),
            songs_path: root.join("api/songs/all.7.json"),
            charts_dir: root.join("api/charts"),
            score_range_chart_meta_path: Some(root.join(SCORE_RANGE_CHART_META_PATH)),
            cards_dir: Some(root.join("api/cards")),
            event_details_dir: Some(root.join("api/events")),
            cards_fix_path: optional_default_path(&root, "cardsCNfix.json"),
            skills_fix_path: optional_default_path(&root, "skillsCNfix.json"),
            area_items_fix_path: optional_default_path(&root, "areaItemFix.json"),
            event_character_parameter_bonus_fix_path: optional_default_path(
                &root,
                "eventCharacterParameterBonusFix.json",
            ),
        }
    }
}

pub fn update_all_score_range_chart_meta(
    config: &BestdoriFilesystemConfig,
) -> Result<bool, DataError> {
    let songs = read_json(&config.songs_path)?;
    let selections = all_chart_selections(&songs)?;
    update_score_range_chart_meta(config, &songs, &selections)
}

pub fn update_published_score_range_chart_meta(
    config: &BestdoriFilesystemConfig,
    server: Server,
) -> Result<bool, DataError> {
    let songs = read_json(&config.songs_path)?;
    let song_records = into_object_map(songs.clone(), "songs")?;
    let now_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let selections = published_score_range_song_selections(
        &song_records,
        server,
        now_millis,
        |song_id, difficulty| chart_path(&config.charts_dir, song_id, difficulty).is_ok(),
    )?;
    update_score_range_chart_meta(config, &songs, &selections)
}

pub fn update_score_range_chart_meta(
    config: &BestdoriFilesystemConfig,
    songs: &Value,
    selections: &[SongSelection],
) -> Result<bool, DataError> {
    let output_path =
        config
            .score_range_chart_meta_path
            .as_deref()
            .ok_or(DataError::MissingField {
                field: "scoreRangeChartMetaPath",
            })?;
    let existing: Option<ScoreRangeChartMetaFile> = read_optional_typed_json(Some(output_path))?;
    let existing_is_valid = existing
        .as_ref()
        .is_some_and(|meta| meta.validate().is_ok());
    let mut output = existing
        .filter(|meta| meta.validate().is_ok())
        .unwrap_or_default();
    let output_modified = existing_is_valid
        .then(|| file_modified(output_path))
        .transpose()?;
    let mut changed = !existing_is_valid;

    for selection in selections {
        let path = chart_path(&config.charts_dir, selection.song_id, selection.difficulty)?;
        let chart_modified = file_modified(&path)?;
        if output.contains_chart(selection.song_id, selection.difficulty)
            && output_modified.is_some_and(|modified| chart_modified <= modified)
        {
            continue;
        }

        let level = song_level_from_value(songs, selection.song_id, selection.difficulty)?;
        let chart_data = read_json(&path)?;
        let chart = chart_from_bestdori(level, &chart_data)?;
        let meta = ScoreRangeChartMeta::from_chart(chart)?;
        let contained = output.contains_chart(selection.song_id, selection.difficulty);
        if meta.is_searchable() {
            output.insert(selection.song_id, selection.difficulty, meta);
            changed = true;
        } else {
            output.remove(selection.song_id, selection.difficulty);
            changed |= contained;
        }
    }

    if !changed {
        return Ok(false);
    }
    output.validate().map_err(|value| DataError::InvalidField {
        field: "scoreRangeChartMeta",
        value,
    })?;
    write_typed_json(output_path, &output)?;
    Ok(true)
}

#[derive(Debug, Clone)]
pub struct BestdoriFilesystemCalculationInputBuilder {
    game_data: BestdoriData,
    characters: BTreeMap<String, Value>,
    events: BTreeMap<String, Value>,
    songs: BTreeMap<String, Value>,
    charts_dir: PathBuf,
    score_range_chart_meta: Option<ScoreRangeChartMetaFile>,
    cards_dir: Option<PathBuf>,
    event_details_dir: Option<PathBuf>,
    event_character_parameter_bonus_fix: Option<BTreeMap<String, Value>>,
}

impl BestdoriFilesystemCalculationInputBuilder {
    pub fn load(config: BestdoriFilesystemConfig) -> Result<Self, DataError> {
        let cards = read_json(&config.cards_path)?;
        let characters = read_json(&config.characters_path)?;
        let skills = read_json(&config.skills_path)?;
        let area_items = read_json(&config.area_items_path)?;
        let events = read_json(&config.events_path)?;
        let songs = read_json(&config.songs_path)?;
        let score_range_chart_meta: Option<ScoreRangeChartMetaFile> =
            read_optional_typed_json(config.score_range_chart_meta_path.as_deref())?;
        if let Some(meta) = &score_range_chart_meta {
            meta.validate().map_err(|message| DataError::InvalidField {
                field: "scoreRangeChartMeta",
                value: message,
            })?;
        }

        let mut game_data =
            BestdoriData::from_values(cards, characters.clone(), skills, area_items)?;
        game_data.apply_repairs(
            read_optional_json(config.cards_fix_path.as_deref())?,
            read_optional_json(config.skills_fix_path.as_deref())?,
            read_optional_json(config.area_items_fix_path.as_deref())?,
        )?;

        Ok(Self {
            game_data,
            characters: into_object_map(characters, "characters")?,
            events: into_object_map(events, "events")?,
            songs: into_object_map(songs, "songs")?,
            charts_dir: config.charts_dir,
            score_range_chart_meta,
            cards_dir: config.cards_dir,
            event_details_dir: config.event_details_dir,
            event_character_parameter_bonus_fix: read_optional_json(
                config.event_character_parameter_bonus_fix_path.as_deref(),
            )?
            .map(|value| into_object_map(value, "eventCharacterParameterBonusFix"))
            .transpose()?,
        })
    }

    pub fn maximize_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        let event_id = event_id
            .or(player.current_event)
            .ok_or(DataError::MissingCurrentEvent)?;
        let song_list = player
            .event_songs
            .get(&event_id.to_string())
            .cloned()
            .ok_or(DataError::MissingEventSongs { event_id })?;
        let snapshot = self.snapshot_for(&player, event_id, &song_list, server)?;

        SnapshotMaximizeInputBuilder::new(snapshot).maximize_sync(
            player,
            server,
            Some(event_id),
            options,
        )
    }

    pub fn calculate_result_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        self.maximize_sync(player, server, event_id, options)
    }

    pub fn score_range_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: ScoreRangeRequest,
    ) -> Result<Vec<ScoreRangeResult>, DataError> {
        let event_id = event_id
            .or(player.current_event)
            .ok_or(DataError::MissingCurrentEvent)?;
        let song_list = self.published_score_range_songs(server)?;
        let snapshot = self.snapshot_for_score_range(&player, event_id, &song_list, server)?;
        SnapshotScoreRangeInputBuilder::new(snapshot).score_range_sync(
            player,
            server,
            Some(event_id),
            request,
        )
    }

    pub fn published_score_range_songs(
        &self,
        server: Server,
    ) -> Result<Vec<SongSelection>, DataError> {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        published_score_range_song_selections(
            &self.songs,
            server,
            now_millis,
            |song_id, difficulty| {
                self.score_range_chart_meta
                    .as_ref()
                    .is_some_and(|meta| meta.contains_chart(song_id, difficulty))
                    || chart_path(&self.charts_dir, song_id, difficulty).is_ok()
            },
        )
    }

    pub fn snapshot_for(
        &self,
        player: &PlayerConfig,
        event_id: u32,
        song_list: &[SongSelection],
        server: Server,
    ) -> Result<GameDataSnapshot, DataError> {
        let card_definitions = self.card_definitions(player)?;
        let area_item_definitions = self.area_item_definitions(player, server)?;
        let event_data = self.event_calculation_data(player, event_id)?;
        let mut snapshot = GameDataSnapshot::new(
            card_definitions,
            area_item_definitions,
            BTreeMap::from([(event_id, event_data)]),
        );

        for song in song_list {
            let chart = self.chart(song.song_id, song.difficulty)?;
            snapshot.insert_chart(song.song_id, song.difficulty, chart);
        }

        Ok(snapshot)
    }

    pub fn snapshot_for_score_range(
        &self,
        player: &PlayerConfig,
        event_id: u32,
        song_list: &[SongSelection],
        server: Server,
    ) -> Result<GameDataSnapshot, DataError> {
        let card_definitions = self.card_definitions(player)?;
        let area_item_definitions = self.area_item_definitions(player, server)?;
        let event_data = self.event_calculation_data(player, event_id)?;
        let mut snapshot = GameDataSnapshot::new(
            card_definitions,
            area_item_definitions,
            BTreeMap::from([(event_id, event_data)]),
        );

        for song in song_list {
            if let Some(meta) = self
                .score_range_chart_meta
                .as_ref()
                .and_then(|meta| meta.chart(song.song_id, song.difficulty))
                .cloned()
            {
                let level = self.song_level(song.song_id, song.difficulty)?;
                snapshot.insert_score_range_chart(song.song_id, song.difficulty, level, meta);
            } else {
                let chart = self.chart(song.song_id, song.difficulty)?;
                snapshot.insert_chart(song.song_id, song.difficulty, chart);
            }
        }
        Ok(snapshot)
    }

    fn card_definitions(
        &self,
        player: &PlayerConfig,
    ) -> Result<BTreeMap<u32, CardDefinition>, DataError> {
        player
            .card_list
            .iter()
            .map(|(card_id, player_card)| {
                let parsed_id = parse_id(card_id, "cardList.cardId")?;
                Ok((parsed_id, self.card_definition(parsed_id, player_card)?))
            })
            .collect()
    }

    fn card_definition(
        &self,
        card_id: u32,
        player_card: &PlayerCardConfig,
    ) -> Result<CardDefinition, DataError> {
        if self.card_detail_is_needed(card_id, player_card)? {
            if let Some(detail) = self.card_detail_value(card_id)? {
                return self.game_data.card_definition_with_detail(card_id, &detail);
            }
        }

        self.game_data.card_definition(card_id)
    }

    fn card_detail_is_needed(
        &self,
        card_id: u32,
        player_card: &PlayerCardConfig,
    ) -> Result<bool, DataError> {
        if player_card.level_is_auto_max() {
            return Ok(false);
        }
        Ok(!self.game_data.card_has_level(card_id, player_card.level)?)
    }

    fn card_detail_value(&self, card_id: u32) -> Result<Option<Value>, DataError> {
        let Some(dir) = &self.cards_dir else {
            return Ok(None);
        };
        let path = dir.join(format!("{card_id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        read_json(&path).map(Some)
    }

    fn area_item_definitions(
        &self,
        player: &PlayerConfig,
        server: Server,
    ) -> Result<BTreeMap<u32, AreaItemDefinition>, DataError> {
        let all_definitions = self.game_data.area_item_definitions(server)?;
        player
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
            .collect()
    }

    fn event_calculation_data(
        &self,
        player: &PlayerConfig,
        event_id: u32,
    ) -> Result<EventData, DataError> {
        let event = player_event_value(player, event_id)
            .map(Ok)
            .unwrap_or_else(|| self.event_value(event_id))?;
        let event = self.apply_event_character_parameter_bonus_fix(event_id, event);
        let event_type = event_type(&event)?;
        let preferred = preferred_item_target(&event, &self.characters)?;

        Ok(EventData {
            event_type,
            event_bonus: event_bonus(&event)?,
            preferred,
        })
    }

    fn event_value(&self, event_id: u32) -> Result<Value, DataError> {
        if event_id == CUSTOM_EVENT_ID {
            return Err(DataError::JsonString {
                message: "custom event parameters are missing".to_owned(),
            });
        }

        let event = if let Some(path) = self.event_detail_path(event_id) {
            read_json(&path)?
        } else {
            self.events
                .get(&event_id.to_string())
                .cloned()
                .ok_or(DataError::MissingEntity {
                    kind: "event",
                    id: event_id.to_string(),
                })?
        };

        Ok(event)
    }

    fn apply_event_character_parameter_bonus_fix(&self, event_id: u32, mut event: Value) -> Value {
        if event.get("eventCharacterParameterBonus").is_none() {
            if let Some(fix) = self
                .event_character_parameter_bonus_fix
                .as_ref()
                .and_then(|fixes| fixes.get(&event_id.to_string()))
            {
                event["eventCharacterParameterBonus"] = fix.clone();
            }
        }

        event
    }

    fn event_detail_path(&self, event_id: u32) -> Option<PathBuf> {
        let dir = self.event_details_dir.as_ref()?;
        let path = dir.join(format!("{event_id}.json"));
        path.exists().then_some(path)
    }

    fn chart(
        &self,
        song_id: u32,
        difficulty: u8,
    ) -> Result<bangdream_optimize_core::Chart, DataError> {
        let level = self.song_level(song_id, difficulty)?;
        let chart_path = chart_path(&self.charts_dir, song_id, difficulty)?;
        let chart_data = read_json(&chart_path)?;

        chart_from_bestdori(level, &chart_data)
    }

    fn song_level(&self, song_id: u32, difficulty: u8) -> Result<i32, DataError> {
        self.songs
            .get(&song_id.to_string())
            .ok_or(DataError::MissingEntity {
                kind: "song",
                id: song_id.to_string(),
            })?
            .get("difficulty")
            .and_then(|value| value.get(difficulty.to_string()))
            .and_then(|value| value.get("playLevel"))
            .and_then(Value::as_i64)
            .ok_or(DataError::MissingField {
                field: "song.difficulty.playLevel",
            })
            .map(|level| level as i32)
    }
}

#[async_trait]
impl MaximizeInputBuilder for BestdoriFilesystemCalculationInputBuilder {
    async fn maximize(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        self.maximize_sync(player, server, event_id, options)
    }
}

#[async_trait]
impl ScoreRangeInputBuilder for BestdoriFilesystemCalculationInputBuilder {
    async fn score_range(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: ScoreRangeRequest,
    ) -> Result<Vec<ScoreRangeResult>, DataError> {
        self.score_range_sync(player, server, event_id, request)
    }
}

fn read_json(path: &Path) -> Result<Value, DataError> {
    let data = fs::read(path).map_err(|source| DataError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_slice(&data).map_err(|source| DataError::Json {
        path: path.display().to_string(),
        source,
    })
}

fn write_typed_json(path: &Path, value: &impl Serialize) -> Result<(), DataError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| DataError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let data = serde_json::to_vec(value).map_err(|source| DataError::JsonString {
        message: source.to_string(),
    })?;
    fs::write(path, data).map_err(|source| DataError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn file_modified(path: &Path) -> Result<std::time::SystemTime, DataError> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|source| DataError::Io {
            path: path.display().to_string(),
            source,
        })
}

fn all_chart_selections(songs: &Value) -> Result<Vec<SongSelection>, DataError> {
    let entries = if let Some(object) = songs.as_object() {
        object
            .iter()
            .map(|(song_id, song)| (song_id.clone(), song))
            .collect::<Vec<_>>()
    } else if let Some(array) = songs.as_array() {
        array
            .iter()
            .enumerate()
            .filter(|(_, song)| !song.is_null())
            .map(|(song_id, song)| (song_id.to_string(), song))
            .collect::<Vec<_>>()
    } else {
        return Err(DataError::InvalidField {
            field: "songs",
            value: "expected object or array".to_owned(),
        });
    };

    let mut selections = Vec::new();
    for (song_id, song) in entries {
        let song_id = parse_id(&song_id, "songs.songId")?;
        let Some(difficulties) = song.get("difficulty").and_then(Value::as_object) else {
            continue;
        };
        for difficulty in difficulties.keys() {
            selections.push(SongSelection {
                song_id,
                difficulty: difficulty
                    .parse::<u8>()
                    .map_err(|_| DataError::InvalidField {
                        field: "songs.difficulty",
                        value: difficulty.clone(),
                    })?,
            });
        }
    }
    selections.sort_by_key(|selection| (selection.song_id, selection.difficulty));
    Ok(selections)
}

fn song_level_from_value(songs: &Value, song_id: u32, difficulty: u8) -> Result<i32, DataError> {
    songs
        .get(song_id.to_string())
        .and_then(|song| song.get("difficulty"))
        .and_then(|difficulties| difficulties.get(difficulty.to_string()))
        .and_then(|definition| definition.get("playLevel"))
        .and_then(Value::as_i64)
        .map(|level| level as i32)
        .ok_or(DataError::MissingField {
            field: "song.difficulty.playLevel",
        })
}

fn read_optional_json(path: Option<&Path>) -> Result<Option<Value>, DataError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    read_json(path).map(Some)
}

fn read_optional_typed_json<T: DeserializeOwned>(
    path: Option<&Path>,
) -> Result<Option<T>, DataError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(path).map_err(|source| DataError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_slice(&data)
        .map(Some)
        .map_err(|source| DataError::Json {
            path: path.display().to_string(),
            source,
        })
}

fn optional_default_path(root: &Path, filename: &str) -> Option<PathBuf> {
    let path = root.join(filename);
    path.exists().then_some(path)
}

fn parse_id(value: &str, field: &'static str) -> Result<u32, DataError> {
    value.parse::<u32>().map_err(|_| DataError::InvalidField {
        field,
        value: value.to_owned(),
    })
}

fn player_event_value(player: &PlayerConfig, event_id: u32) -> Option<Value> {
    let key = event_id.to_string();
    let mut event = player.event_presets.get(&key).cloned();
    if event_id == CUSTOM_EVENT_ID {
        if let Some(override_event) = player.event_overrides.get(&key) {
            merge_event_value(&mut event, override_event.clone());
        }
    }
    event
}

fn merge_event_value(target: &mut Option<Value>, patch: Value) {
    match (target, patch) {
        (Some(Value::Object(target)), Value::Object(patch)) => {
            for (key, value) in patch {
                target.insert(key, value);
            }
        }
        (target, patch) => {
            *target = Some(patch);
        }
    }
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

fn preferred_item_target(
    event: &Value,
    characters: &BTreeMap<String, Value>,
) -> Result<Option<PreferredItemTarget>, DataError> {
    let Some(attribute) = event
        .get("attributes")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("attribute"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };

    let character_ids = event
        .get("characters")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|value| value.get("characterId").and_then(Value::as_u64))
        .map(|value| value as u32)
        .collect::<Vec<_>>();

    let band = unified_event_band(&character_ids, characters)?.unwrap_or(1000);

    Ok(Some(PreferredItemTarget {
        band: band.to_string(),
        attribute: attribute.to_owned(),
    }))
}

fn unified_event_band(
    character_ids: &[u32],
    characters: &BTreeMap<String, Value>,
) -> Result<Option<u32>, DataError> {
    let Some(first_id) = character_ids.first() else {
        return Ok(None);
    };
    let first_band = character_band(*first_id, characters)?;
    for character_id in &character_ids[1..] {
        if character_band(*character_id, characters)? != first_band {
            return Ok(None);
        }
    }
    Ok(Some(first_band))
}

fn character_band(
    character_id: u32,
    characters: &BTreeMap<String, Value>,
) -> Result<u32, DataError> {
    characters
        .get(&character_id.to_string())
        .and_then(|value| value.get("bandId"))
        .and_then(Value::as_u64)
        .map(|value| value as u32)
        .ok_or(DataError::MissingEntity {
            kind: "character",
            id: character_id.to_string(),
        })
}

fn chart_path(charts_dir: &Path, song_id: u32, difficulty: u8) -> Result<PathBuf, DataError> {
    let difficulty_name = difficulty_name(difficulty)?;
    let candidates = [
        charts_dir
            .join(song_id.to_string())
            .join(format!("{difficulty_name}.json")),
        charts_dir.join(format!("{song_id}.{difficulty_name}.json")),
        charts_dir
            .join(song_id.to_string())
            .join(format!("{difficulty}.json")),
        charts_dir.join(format!("{song_id}.{difficulty}.json")),
    ];

    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or(DataError::MissingEntity {
            kind: "chart",
            id: format!("{song_id}:{difficulty}"),
        })
}

fn difficulty_name(difficulty: u8) -> Result<&'static str, DataError> {
    match difficulty {
        0 => Ok("easy"),
        1 => Ok("normal"),
        2 => Ok("hard"),
        3 => Ok("expert"),
        4 => Ok("special"),
        _ => Err(DataError::InvalidField {
            field: "difficulty",
            value: difficulty.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score_range::difficulty_is_published;
    use bangdream_optimize_core::{
        AreaItemConfig, CharacterBonusConfig, PlayerCardConfig, StatRate,
    };
    use serde::Deserialize;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_festival_as_single_event_type() {
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
    fn parses_live_try_for_score_range() {
        assert_eq!(
            event_type(&json!({ "eventType": "live_try" })).unwrap(),
            EventType::LiveTry,
        );
    }

    #[test]
    fn filters_special_by_its_server_release_time() {
        let future = json!({ "playLevel": 27, "publishedAt": [300, null, null, null, null] });
        let released = json!({ "playLevel": 27, "publishedAt": [100, null, null, null, null] });
        let released_with_song = json!({ "playLevel": 27 });

        assert!(!difficulty_is_published(4, &future, 0, 200));
        assert!(difficulty_is_published(4, &released, 0, 200));
        assert!(!difficulty_is_published(4, &released, 1, 200));
        assert!(difficulty_is_published(4, &released_with_song, 0, 200));
        assert!(difficulty_is_published(3, &future, 0, 200));
    }

    #[test]
    fn published_score_range_songs_skip_missing_chart_files() {
        let root = temp_root();
        fs::create_dir_all(root.join("charts/1")).unwrap();
        write_json(root.join("cards.json"), cards_json());
        write_json(root.join("characters.json"), characters_json());
        write_json(root.join("skills.json"), skills_json());
        write_json(root.join("areaItems.json"), area_items_json());
        write_json(root.join("events.json"), events_json());
        write_json(
            root.join("songs.json"),
            json!({
                "1": {
                    "publishedAt": [1, 1, 1, 1, 1],
                    "closedAt": [null, null, null, null, null],
                    "difficulty": {
                        "3": {"playLevel": 25},
                        "4": {"playLevel": 27}
                    }
                }
            }),
        );
        write_json(root.join("charts/1/expert.json"), chart_json());
        let builder = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_root(&root),
        )
        .unwrap();

        assert_eq!(
            builder.published_score_range_songs(Server::Jp).unwrap(),
            vec![SongSelection {
                song_id: 1,
                difficulty: 3,
            }],
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generates_and_reuses_local_score_range_chart_meta() {
        let root = temp_root();
        fs::create_dir_all(root.join("charts/1")).unwrap();
        write_json(root.join("songs.json"), songs_json());
        write_json(root.join("charts/1/expert.json"), chart_json());
        let config = BestdoriFilesystemConfig::from_root(&root);

        assert!(update_all_score_range_chart_meta(&config).unwrap());
        let output_path = root.join("scoreRangeChartMeta.1.json");
        let output: ScoreRangeChartMetaFile =
            serde_json::from_slice(&fs::read(&output_path).unwrap()).unwrap();
        assert!(output.contains_chart(1, 3));
        assert!(!update_all_score_range_chart_meta(&config).unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_meta_generation_skips_closed_and_missing_charts() {
        let root = temp_root();
        fs::create_dir_all(root.join("charts/1")).unwrap();
        write_json(
            root.join("songs.json"),
            json!({
                "1": {
                    "publishedAt": [1, null, null, null, null],
                    "closedAt": [null, null, null, null, null],
                    "difficulty": {"3": {"playLevel": 25}}
                },
                "2": {
                    "publishedAt": [1, null, null, null, null],
                    "closedAt": [2, null, null, null, null],
                    "difficulty": {"3": {"playLevel": 25}}
                }
            }),
        );
        write_json(root.join("charts/1/expert.json"), chart_json());
        let config = BestdoriFilesystemConfig::from_root(&root);

        assert!(update_published_score_range_chart_meta(&config, Server::Jp).unwrap());
        let output: ScoreRangeChartMetaFile =
            serde_json::from_slice(&fs::read(root.join("scoreRangeChartMeta.1.json")).unwrap())
                .unwrap();
        assert!(output.contains_chart(1, 3));
        assert!(!output.contains_chart(2, 3));
        assert!(!update_published_score_range_chart_meta(&config, Server::Jp).unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_event_context_prepares_both_score_range_bonus_modes() {
        let root = temp_root();
        fs::create_dir_all(root.join("charts/1")).unwrap();
        write_json(root.join("cards.json"), cards_json());
        write_json(root.join("characters.json"), characters_json());
        write_json(root.join("skills.json"), skills_json());
        write_json(root.join("areaItems.json"), area_items_json());
        let mut events = events_json();
        events["100"]["attributes"][0]["percent"] = json!(20);
        events["100"]["characters"][0]["percent"] = json!(10);
        write_json(root.join("events.json"), events);
        write_json(root.join("songs.json"), songs_json());
        write_json(root.join("charts/1/expert.json"), chart_json());
        let builder = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_root(&root),
        )
        .unwrap();
        let player = player();
        let snapshot = builder
            .snapshot_for(
                &player,
                100,
                &[SongSelection {
                    song_id: 1,
                    difficulty: 3,
                }],
                Server::Jp,
            )
            .unwrap();

        let context = crate::prepare_event_context(&snapshot, &player, Some(100)).unwrap();

        assert!(context
            .maximize_cards()
            .iter()
            .any(|card| card.event_add_stat.sum() > 0.0));
        assert!(context
            .score_range_cards()
            .iter()
            .all(|card| card.event_add_stat == bangdream_optimize_core::StatValue::zero()));
        assert_eq!(context.point_bonus_micros[&1], 30_000_000);
        assert_eq!(context.point_bonus_micros[&2], 20_000_000);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn calculates_result_from_local_bestdori_files() {
        let root = temp_root();
        fs::create_dir_all(root.join("charts/1")).unwrap();
        write_json(root.join("cards.json"), cards_json());
        write_json(root.join("characters.json"), characters_json());
        write_json(root.join("skills.json"), skills_json());
        write_json(root.join("areaItems.json"), area_items_json());
        write_json(root.join("events.json"), events_json());
        write_json(root.join("songs.json"), songs_json());
        write_json(root.join("charts/1/expert.json"), chart_json());

        let builder = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_root(&root),
        )
        .unwrap();

        let result = builder
            .calculate_result_sync(player(), Server::Jp, None, ItemSearchOptions::default())
            .unwrap();

        assert_eq!(result.event_id, 100);
        assert_eq!(result.event_type, EventType::Challenge);
        assert_eq!(result.songs.len(), 1);
        assert!(result.total_score > 0);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn calculates_requested_card_level_from_detail_file() {
        let root = temp_root();
        fs::create_dir_all(root.join("cards")).unwrap();
        fs::create_dir_all(root.join("charts/1")).unwrap();
        write_json(root.join("cards.json"), cards_json());
        write_json(
            root.join("cards/1.json"),
            json!({
                "stat": {
                    "50": {"performance": 900, "technique": 900, "visual": 900},
                    "training": {"performance": 0, "technique": 0, "visual": 0},
                    "episodes": []
                }
            }),
        );
        write_json(root.join("characters.json"), characters_json());
        write_json(root.join("skills.json"), skills_json());
        write_json(root.join("areaItems.json"), area_items_json());
        write_json(root.join("events.json"), events_json());
        write_json(root.join("songs.json"), songs_json());
        write_json(root.join("charts/1/expert.json"), chart_json());

        let builder = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_root(&root),
        )
        .unwrap();
        let mut player = player();
        player.card_list.get_mut("1").unwrap().level = 50;

        let result = builder
            .calculate_result_sync(player, Server::Jp, None, ItemSearchOptions::default())
            .unwrap();

        assert_eq!(result.event_id, 100);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn custom_event_uses_player_override_without_event_detail_file() {
        let root = temp_root();
        fs::create_dir_all(root.join("events")).unwrap();
        write_json(root.join("cards.json"), cards_json());
        write_json(root.join("characters.json"), characters_json());
        write_json(root.join("skills.json"), skills_json());
        write_json(root.join("areaItems.json"), area_items_json());
        write_json(root.join("events.json"), events_json());
        write_json(root.join("songs.json"), songs_json());
        write_json(
            root.join("events/0.json"),
            json!({ "eventType": "challenge" }),
        );

        let builder = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_root(&root),
        )
        .unwrap();
        let mut player = player();
        player.current_event = Some(CUSTOM_EVENT_ID);
        player.event_overrides.insert(
            CUSTOM_EVENT_ID.to_string(),
            json!({
                "eventType": "medley",
                "attributes": [],
                "characters": [],
                "members": [],
                "eventAttributeAndCharacterBonus": {"pointPercent": 0, "parameterPercent": 0}
            }),
        );

        let event = builder
            .event_calculation_data(&player, CUSTOM_EVENT_ID)
            .unwrap();
        assert_eq!(event.event_type, EventType::Medley);

        player.event_overrides.clear();
        let err = builder
            .event_calculation_data(&player, CUSTOM_EVENT_ID)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("custom event parameters are missing"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn player_event_preset_uses_event_character_parameter_bonus_fix() {
        let root = temp_root();
        fs::create_dir_all(root.join("charts/1")).unwrap();
        write_json(root.join("cards.json"), cards_json());
        write_json(root.join("characters.json"), characters_json());
        write_json(root.join("skills.json"), skills_json());
        write_json(root.join("areaItems.json"), area_items_json());
        write_json(root.join("events.json"), events_json());
        write_json(root.join("songs.json"), songs_json());
        write_json(root.join("charts/1/expert.json"), chart_json());
        write_json(
            root.join("eventCharacterParameterBonusFix.json"),
            json!({
                "100": {"performance": 50, "technique": 0, "visual": 0}
            }),
        );

        let builder = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_root(&root),
        )
        .unwrap();
        let mut player = player();
        player.event_presets.insert(
            "100".to_owned(),
            json!({
                "eventType": "challenge",
                "attributes": [{"attribute": "happy", "percent": 20}],
                "characters": [{"characterId": 1, "percent": 20}],
                "members": [],
                "eventAttributeAndCharacterBonus": {"pointPercent": 0, "parameterPercent": 20},
                "limitBreaks": []
            }),
        );

        let event = builder.event_calculation_data(&player, 100).unwrap();
        let parameter_bonus = event.event_bonus.event_character_parameter_bonus.unwrap();
        assert_eq!(parameter_bonus.performance, 50.0);
        assert_eq!(parameter_bonus.technique, 0.0);
        assert_eq!(parameter_bonus.visual, 0.0);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_mongodb_player_fixture() {
        let player = mongodb_player_fixture();

        assert_eq!(player.mongo_id, None);
        assert_eq!(player.player_id, 1);
        assert_eq!(player.current_event, Some(287));
        assert_eq!(player.event_songs["287"].len(), 3);
        assert_eq!(player.card_list.len(), 25);
        assert!(!player.area_item.is_empty());
        assert!(!player.character_bouns.is_empty());
    }

    #[test]
    #[ignore = "slow; uses the MongoDB player fixture and local Bestdori game data"]
    fn calculates_mongodb_player_fixture_from_local_bestdori_files() {
        let player = mongodb_player_fixture();
        let card_count = player.card_list.len();
        let current_event = player.current_event;
        assert_eq!(current_event, Some(287));

        eprintln!(
            "running MongoDB fixture calculation: event={:?} cards={card_count}",
            current_event
        );
        let root = std::env::var("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../var/game-data")
            });

        let builder = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_root(&root),
        )
        .unwrap();
        let started_at = std::time::Instant::now();
        let result = builder
            .calculate_result_sync(player, Server::Jp, None, ItemSearchOptions::default())
            .unwrap();

        eprintln!(
            "mongodb player fixture: event={} type={:?} cards={} songs={} total_score={} elapsed_ms={:.3}",
            result.event_id,
            result.event_type,
            card_count,
            result.songs.len(),
            result.total_score,
            started_at.elapsed().as_secs_f64() * 1000.0
        );

        assert_eq!(result.event_type, EventType::Medley);
        assert_eq!(result.songs.len(), 3);
        assert!(result.total_score > 0);
    }

    #[test]
    fn parses_full_diagnostic_fixture() {
        let diagnostic = full_diagnostic_fixture();

        assert_eq!(diagnostic.server, Server::Cn);
        assert_eq!(diagnostic.event_id, Some(0));
        assert_eq!(diagnostic.result.event_id, 0);
        assert_eq!(diagnostic.result.event_type, EventType::Medley);
        assert_eq!(diagnostic.result.total_score, 11880244);
        assert_eq!(diagnostic.result.total_stat, 1487827);
        assert_eq!(diagnostic.result.songs.len(), 3);
        assert!(diagnostic
            .result
            .songs
            .iter()
            .all(|song| song.song_id == 306 && song.difficulty == 3));
        assert_eq!(diagnostic.player.card_list.len(), 1414);
        assert_eq!(diagnostic.player.area_item.len(), 74);
        assert_eq!(diagnostic.player.character_bouns.len(), 40);
    }

    #[test]
    #[ignore = "slow; recalculates the full desktop diagnostic fixture against local Bestdori game data"]
    fn calculates_full_diagnostic_fixture_from_local_bestdori_files() {
        let diagnostic = full_diagnostic_fixture();
        let event_id = diagnostic
            .event_id
            .or(diagnostic.player.current_event)
            .unwrap_or(diagnostic.result.event_id);
        let root = std::env::var("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../var/game-data")
            });

        eprintln!(
            "running full diagnostic fixture calculation: event={} cards={} songs={}",
            event_id,
            diagnostic.player.card_list.len(),
            diagnostic.result.songs.len()
        );
        let builder = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_bestdori_api_root(&root),
        )
        .unwrap();
        let started_at = std::time::Instant::now();
        let result = builder
            .calculate_result_sync(
                diagnostic.player,
                diagnostic.server,
                Some(event_id),
                ItemSearchOptions::default(),
            )
            .unwrap();

        eprintln!(
            "full diagnostic fixture: event={} type={:?} cards={} songs={} total_score={} elapsed_ms={:.3} metrics={:?}",
            result.event_id,
            result.event_type,
            result.metrics.as_ref().map(|metrics| metrics.card_count).unwrap_or_default(),
            result.songs.len(),
            result.total_score,
            started_at.elapsed().as_secs_f64() * 1000.0,
            result.metrics
        );
        eprintln!(
            "full diagnostic fixture detail: total_stat={} items={:?} solver={:?} songs={:?}",
            result.total_stat, result.items, result.solver, result.songs
        );

        assert_eq!(result.event_id, diagnostic.result.event_id);
        assert_eq!(result.event_type, diagnostic.result.event_type);
        assert_eq!(result.total_score, diagnostic.result.total_score);
        assert_eq!(result.total_stat, diagnostic.result.total_stat);
        assert_eq!(result.items, diagnostic.result.items);
        assert_eq!(result.songs, diagnostic.result.songs);

        let metrics = result.metrics.unwrap();
        let expected_metrics = diagnostic.result.metrics.unwrap();
        assert_eq!(metrics.card_count, expected_metrics.card_count);
        assert_eq!(metrics.song_count, expected_metrics.song_count);
        assert_eq!(
            metrics.item_combinations_before,
            expected_metrics.item_combinations_before
        );
        assert_eq!(
            metrics.item_combinations_after,
            expected_metrics.item_combinations_after
        );
    }

    #[test]
    #[ignore = "slow; benchmarks score range with the full 1,414-card diagnostic fixture"]
    fn benchmarks_score_range_with_full_diagnostic_fixture() {
        let diagnostic = full_diagnostic_fixture();
        let event_id = diagnostic
            .event_id
            .or(diagnostic.player.current_event)
            .unwrap_or(diagnostic.result.event_id);
        let root = std::env::var("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../var/game-data")
            });
        let builder = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_bestdori_api_root(&root),
        )
        .unwrap();
        let started_at = std::time::Instant::now();
        let songs = builder
            .published_score_range_songs(diagnostic.server)
            .unwrap();
        eprintln!(
            "score-range phase songs: count={} elapsed_ms={:.3}",
            songs.len(),
            started_at.elapsed().as_secs_f64() * 1000.0,
        );
        let phase = std::time::Instant::now();
        let snapshot = builder
            .snapshot_for(&diagnostic.player, event_id, &songs, diagnostic.server)
            .unwrap();
        eprintln!(
            "score-range phase snapshot: elapsed_ms={:.3}",
            phase.elapsed().as_secs_f64() * 1000.0,
        );
        let phase = std::time::Instant::now();
        let input = crate::prepare_score_range_input(
            &snapshot,
            &diagnostic.player,
            Some(event_id),
            &songs,
            bangdream_optimize_core::auto_base_multiplier(diagnostic.server),
        )
        .unwrap();
        eprintln!(
            "score-range phase prepare: teams={} songs={} elapsed_ms={:.3}",
            input.teams.teams.len(),
            input.songs.len(),
            phase.elapsed().as_secs_f64() * 1000.0,
        );
        let target_pts = std::env::var("BANGDREAM_OPTIMIZE_SCORE_RANGE_TARGET_PTS")
            .map(|values| {
                values
                    .split(',')
                    .map(|value| value.trim().parse::<u64>().unwrap())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|_| vec![1_000]);
        let all_searches = std::time::Instant::now();
        for target_pt in target_pts {
            let phase = std::time::Instant::now();
            let request = ScoreRangeRequest {
                event_type: input.event_type,
                current_pt: 0,
                target_total_pt: target_pt,
                auto_base_multiplier: None,
                mission_support_pt_bonus: None,
                max_results: 20,
            };
            let results =
                bangdream_optimize_core::search_score_range(&request, &input.teams, &input.songs)
                    .unwrap();
            eprintln!(
                "score-range target: pt={} results={} elapsed_ms={:.3}",
                target_pt,
                results.len(),
                phase.elapsed().as_secs_f64() * 1000.0,
            );
            let Some(result) = results.first() else {
                continue;
            };
            eprintln!(
                "first score-range result: target_pt={} plays={} total_fire_cost={} songs={} stat={} cards={:?}",
                target_pt,
                result.play_count,
                result.total_fire_cost,
                result.distinct_song_count,
                result.total_stat,
                result.team_card_ids,
            );
        }
        eprintln!(
            "full score-range fixture: event={} search_elapsed_ms={:.3} total_elapsed_ms={:.3}",
            event_id,
            all_searches.elapsed().as_secs_f64() * 1000.0,
            started_at.elapsed().as_secs_f64() * 1000.0,
        );
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DiagnosticFixture {
        server: Server,
        event_id: Option<u32>,
        result: BuildResult,
        player: PlayerConfig,
    }

    fn full_diagnostic_fixture() -> DiagnosticFixture {
        let fixture_path = std::env::var("BANGDREAM_OPTIMIZE_DIAGNOSTIC_FIXTURE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
                    "tests/fixtures/bangdream-optimize-diagnostic-0-2026-06-13T13-41-23-273Z.json",
                )
            });
        serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap()
    }

    fn mongodb_player_fixture() -> PlayerConfig {
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mongodb-player-medley.json");
        serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap()
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bangdream-optimize-data-test-{}-{nanos}",
            std::process::id()
        ))
    }

    fn write_json(path: impl AsRef<Path>, value: Value) {
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
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

    fn events_json() -> Value {
        json!({
            "100": {
                "eventType": "challenge",
                "attributes": [{"attribute": "cool", "percent": 0}],
                "characters": [{"characterId": 1, "percent": 0}],
                "members": [],
                "eventAttributeAndCharacterBonus": {"parameterPercent": 0},
                "limitBreaks": []
            }
        })
    }

    fn songs_json() -> Value {
        json!({
            "1": {
                "difficulty": {
                    "3": {"playLevel": 25}
                }
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
