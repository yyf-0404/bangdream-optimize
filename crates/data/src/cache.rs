use crate::{
    BestdoriFilesystemCalculationInputBuilder, BestdoriFilesystemConfig, CalculationInputBuilder,
    DataError,
};
use async_trait::async_trait;
use bangdream_optimize_core::{
    BuildResult, ItemSearchOptions, PlayerConfig, Server, SongSelection,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const REQUIRED_CORE_FILES: [&str; 6] = [
    "api/cards/all.5.json",
    "api/characters/main.3.json",
    "api/skills/all.10.json",
    "api/areaItems/main.5.json",
    "api/events/all.6.json",
    "api/songs/all.7.json",
];

const OPTIONAL_REPAIR_FILES: [&str; 4] = [
    "cardsCNfix.json",
    "skillsCNfix.json",
    "areaItemFix.json",
    "eventCharacterParameterBonusFix.json",
];

const DIFFICULTY_NAMES: [&str; 5] = ["easy", "normal", "hard", "expert", "special"];
const CUSTOM_EVENT_ID: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ChartSelection {
    song_id: u32,
    difficulty: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BestdoriStaticMirrorConfig {
    pub cache_root: PathBuf,
    pub base_url: String,
}

impl BestdoriStaticMirrorConfig {
    pub fn new(cache_root: impl Into<PathBuf>, base_url: impl Into<String>) -> Self {
        Self {
            cache_root: cache_root.into(),
            base_url: base_url.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BestdoriCachedFilesystemCalculationInputBuilder {
    config: BestdoriStaticMirrorConfig,
    client: Client,
    calculator: Arc<Mutex<Option<BestdoriFilesystemCalculationInputBuilder>>>,
}

impl BestdoriCachedFilesystemCalculationInputBuilder {
    pub fn new(config: BestdoriStaticMirrorConfig) -> Result<Self, DataError> {
        let client = Client::builder()
            .user_agent("bangdream-optimize-data-cache")
            .build()
            .map_err(|source| DataError::Http {
                url: "client".to_owned(),
                source,
            })?;
        Ok(Self {
            config,
            client,
            calculator: Arc::new(Mutex::new(None)),
        })
    }

    pub fn calculate_result_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        let selected_event_id = event_id
            .or(player.current_event)
            .ok_or(DataError::MissingCurrentEvent)?;
        let song_list = player
            .event_songs
            .get(&selected_event_id.to_string())
            .cloned()
            .ok_or(DataError::MissingEventSongs {
                event_id: selected_event_id,
            })?;

        if self.sync_for_inner(selected_event_id, &song_list, Some(&player))? {
            self.clear_loaded_calculator()?;
        }
        self.calculate_from_cache(player, server, Some(selected_event_id), options)
    }

    pub fn sync_for(&self, event_id: u32, song_list: &[SongSelection]) -> Result<(), DataError> {
        if self.sync_for_inner(event_id, song_list, None)? {
            self.clear_loaded_calculator()?;
        }
        Ok(())
    }

    pub fn sync_core(&self) -> Result<(), DataError> {
        if self.sync_core_inner()? {
            self.clear_loaded_calculator()?;
        }
        Ok(())
    }

    pub fn refresh_core(&self) -> Result<(), DataError> {
        if self.refresh_core_inner()? {
            self.clear_loaded_calculator()?;
        }
        Ok(())
    }

    pub fn sync_all(&self) -> Result<(), DataError> {
        if self.sync_all_inner()? {
            self.clear_loaded_calculator()?;
        }
        Ok(())
    }

    pub fn clear_loaded_calculator(&self) -> Result<(), DataError> {
        *self.calculator_lock()? = None;
        Ok(())
    }

    pub fn cache_root(&self) -> &Path {
        &self.config.cache_root
    }

    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    fn sync_for_inner(
        &self,
        event_id: u32,
        song_list: &[SongSelection],
        player: Option<&PlayerConfig>,
    ) -> Result<bool, DataError> {
        fs::create_dir_all(&self.config.cache_root).map_err(|source| DataError::Io {
            path: self.config.cache_root.display().to_string(),
            source,
        })?;

        let remote_manifest = self.fetch_manifest()?;
        let local_manifest = self.read_local_manifest()?;

        let mut changed = self.sync_core_files(&remote_manifest, local_manifest.as_ref(), false)?;

        if let Some(player) = player {
            let cards = self.read_json("api/cards/all.5.json")?;
            for path in card_detail_paths_for_player(player, &cards)? {
                changed |= self.sync_file_auto_manifest(&path, false)?;
            }
        }

        if let Some(event_path) = event_detail_sync_path(event_id) {
            changed |= self.sync_file_auto_manifest(&event_path, false)?;
        }

        for song in song_list {
            let path = chart_path(song.song_id, song.difficulty)?;
            changed |= self.sync_file_auto_manifest(&path, false)?;
        }

        self.write_json("manifest.json", &remote_manifest)?;
        Ok(changed)
    }

    fn sync_core_inner(&self) -> Result<bool, DataError> {
        self.sync_core_inner_with_refresh(false)
    }

    fn refresh_core_inner(&self) -> Result<bool, DataError> {
        self.sync_core_inner_with_refresh(true)
    }

    fn sync_core_inner_with_refresh(
        &self,
        refresh_without_manifest: bool,
    ) -> Result<bool, DataError> {
        fs::create_dir_all(&self.config.cache_root).map_err(|source| DataError::Io {
            path: self.config.cache_root.display().to_string(),
            source,
        })?;

        let remote_manifest = self.fetch_manifest()?;
        let local_manifest = self.read_local_manifest()?;
        let force_required_files = refresh_without_manifest && remote_manifest.files.is_empty();
        let changed = self.sync_core_files(
            &remote_manifest,
            local_manifest.as_ref(),
            force_required_files,
        )?;
        self.write_json("manifest.json", &remote_manifest)?;
        Ok(changed)
    }

    fn sync_all_inner(&self) -> Result<bool, DataError> {
        let mut changed = self.sync_core_inner()?;
        let cards = self.read_json("api/cards/all.5.json")?;
        for card_id in ids_from_json_object_or_array(&cards, "cards")? {
            changed |= self.sync_file_without_manifest(&format!("api/cards/{card_id}.json"))?;
        }

        let events = self.read_json("api/events/all.6.json")?;
        for event_id in ids_from_json_object_or_array(&events, "events")? {
            changed |= self.sync_file_without_manifest(&format!("api/events/{event_id}.json"))?;
        }

        let songs = self.read_json("api/songs/all.7.json")?;
        for song in chart_selections_from_songs_json(&songs)? {
            changed |=
                self.sync_file_without_manifest(&chart_path(song.song_id, song.difficulty)?)?;
        }

        Ok(changed)
    }

    fn sync_core_files(
        &self,
        remote_manifest: &StaticMirrorManifest,
        local_manifest: Option<&StaticMirrorManifest>,
        force_required_files: bool,
    ) -> Result<bool, DataError> {
        let mut changed = false;
        for path in REQUIRED_CORE_FILES {
            changed |= self.sync_file(
                path,
                remote_manifest,
                local_manifest,
                false,
                force_required_files,
            )?;
        }

        for path in OPTIONAL_REPAIR_FILES {
            changed |= self.sync_file(path, remote_manifest, local_manifest, true, false)?;
        }

        Ok(changed)
    }

    fn fetch_manifest(&self) -> Result<StaticMirrorManifest, DataError> {
        self.fetch_manifest_path("manifest.json")
    }

    fn fetch_manifest_for_path(&self, path: &str) -> Result<StaticMirrorManifest, DataError> {
        let manifest_path = manifest_path_for_file(path);
        self.fetch_manifest_path(&manifest_path)
    }

    fn fetch_manifest_path(&self, path: &str) -> Result<StaticMirrorManifest, DataError> {
        let url = self.url(path);
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|source| DataError::Http {
                url: url.clone(),
                source,
            })?;
        let status = response.status();
        if status.as_u16() == 404 {
            return Ok(StaticMirrorManifest {
                version: None,
                generated_at: None,
                files: BTreeMap::new(),
            });
        }
        if !status.is_success() {
            return Err(DataError::HttpStatus {
                url,
                status: status.as_u16(),
            });
        }
        let data = response.bytes().map_err(|source| DataError::Http {
            url: url.clone(),
            source,
        })?;
        serde_json::from_slice(&data).map_err(|source| DataError::Json { path: url, source })
    }

    fn read_local_manifest(&self) -> Result<Option<StaticMirrorManifest>, DataError> {
        self.read_local_manifest_path("manifest.json")
    }

    fn read_local_manifest_for_path(
        &self,
        path: &str,
    ) -> Result<Option<StaticMirrorManifest>, DataError> {
        let manifest_path = manifest_path_for_file(path);
        self.read_local_manifest_path(&manifest_path)
    }

    fn read_local_manifest_path(
        &self,
        manifest_path: &str,
    ) -> Result<Option<StaticMirrorManifest>, DataError> {
        let path = self.local_path(manifest_path)?;
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(&path).map_err(|source| DataError::Io {
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

    fn sync_file_auto_manifest(&self, path: &str, optional: bool) -> Result<bool, DataError> {
        let remote_manifest = self.fetch_manifest_for_path(path)?;
        let local_manifest = self.read_local_manifest_for_path(path)?;
        let changed = self.sync_file(
            path,
            &remote_manifest,
            local_manifest.as_ref(),
            optional,
            false,
        )?;
        self.write_json(&manifest_path_for_file(path), &remote_manifest)?;
        Ok(changed)
    }

    fn sync_file_without_manifest(&self, path: &str) -> Result<bool, DataError> {
        let manifest = StaticMirrorManifest {
            version: None,
            generated_at: None,
            files: BTreeMap::new(),
        };
        self.sync_file(path, &manifest, None, false, false)
    }

    fn sync_file(
        &self,
        path: &str,
        remote_manifest: &StaticMirrorManifest,
        local_manifest: Option<&StaticMirrorManifest>,
        optional: bool,
        force: bool,
    ) -> Result<bool, DataError> {
        let remote_meta = manifest_file_meta(remote_manifest, path);
        if optional && remote_meta.is_none() {
            return Ok(false);
        }

        let local_path = self.local_path(path)?;
        let local_meta = local_manifest.and_then(|manifest| manifest_file_meta(manifest, path));
        if local_path.exists() && !force && !needs_update(local_meta, remote_meta) {
            return Ok(false);
        }

        let url = self.url(path);
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|source| DataError::Http {
                url: url.clone(),
                source,
            })?;
        let status = response.status();
        if optional && status.as_u16() == 404 {
            return Ok(false);
        }
        if !status.is_success() {
            return Err(DataError::HttpStatus {
                url,
                status: status.as_u16(),
            });
        }
        let data = response.bytes().map_err(|source| DataError::Http {
            url: url.clone(),
            source,
        })?;
        write_file(&local_path, &data)?;
        Ok(true)
    }

    fn calculate_from_cache(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        let mut calculator = self.calculator_lock()?;
        if calculator.is_none() {
            *calculator = Some(BestdoriFilesystemCalculationInputBuilder::load(
                BestdoriFilesystemConfig::from_bestdori_api_root(self.config.cache_root.clone()),
            )?);
        }
        calculator
            .as_ref()
            .expect("calculator was loaded")
            .calculate_result_sync(player, server, event_id, options)
    }

    fn calculator_lock(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, Option<BestdoriFilesystemCalculationInputBuilder>>,
        DataError,
    > {
        self.calculator.lock().map_err(|err| DataError::Storage {
            message: format!("game-data calculator cache lock is poisoned: {err}"),
        })
    }

    fn write_json<T: Serialize>(&self, path: &str, value: &T) -> Result<(), DataError> {
        let data = serde_json::to_vec_pretty(value).map_err(|source| DataError::JsonString {
            message: source.to_string(),
        })?;
        write_file(&self.local_path(path)?, &data)
    }

    fn read_json(&self, path: &str) -> Result<Value, DataError> {
        let local_path = self.local_path(path)?;
        let data = fs::read(&local_path).map_err(|source| DataError::Io {
            path: local_path.display().to_string(),
            source,
        })?;
        serde_json::from_slice(&data).map_err(|source| DataError::Json {
            path: local_path.display().to_string(),
            source,
        })
    }

    fn local_path(&self, path: &str) -> Result<PathBuf, DataError> {
        if path
            .split('/')
            .any(|component| component.is_empty() || component == "..")
        {
            return Err(DataError::InvalidField {
                field: "gameData.path",
                value: path.to_owned(),
            });
        }
        Ok(self.config.cache_root.join(path))
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.config.base_url.trim_end_matches('/'), path)
    }
}

#[async_trait]
impl CalculationInputBuilder for BestdoriCachedFilesystemCalculationInputBuilder {
    async fn calculate_result(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        self.calculate_result_sync(player, server, event_id, options)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StaticMirrorManifest {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    generated_at: Option<String>,
    files: BTreeMap<String, StaticMirrorFileMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StaticMirrorFileMeta {
    #[serde(default)]
    hash: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    etag: Option<String>,
    #[serde(default)]
    last_modified: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

fn needs_update(
    local: Option<&StaticMirrorFileMeta>,
    remote: Option<&StaticMirrorFileMeta>,
) -> bool {
    let Some(remote) = remote else {
        return false;
    };
    let Some(local) = local else {
        return true;
    };

    changed(local.hash.as_deref(), remote.hash.as_deref())
        || changed(local.etag.as_deref(), remote.etag.as_deref())
        || changed(
            local.last_modified.as_deref(),
            remote.last_modified.as_deref(),
        )
        || changed(local.version.as_deref(), remote.version.as_deref())
        || changed(local.updated_at.as_deref(), remote.updated_at.as_deref())
        || remote.size.is_some_and(|size| local.size != Some(size))
}

fn changed(local: Option<&str>, remote: Option<&str>) -> bool {
    remote.is_some_and(|remote| local != Some(remote))
}

fn manifest_path_for_file(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/manifest.json"))
        .unwrap_or_else(|| "manifest.json".to_owned())
}

fn manifest_key_for_file(path: &str) -> &str {
    path.rsplit_once('/').map(|(_, name)| name).unwrap_or(path)
}

fn manifest_file_meta<'a>(
    manifest: &'a StaticMirrorManifest,
    path: &str,
) -> Option<&'a StaticMirrorFileMeta> {
    manifest
        .files
        .get(path)
        .or_else(|| manifest.files.get(manifest_key_for_file(path)))
}

fn chart_path(song_id: u32, difficulty: u8) -> Result<String, DataError> {
    let Some(name) = DIFFICULTY_NAMES.get(difficulty as usize) else {
        return Err(DataError::InvalidField {
            field: "song.difficulty",
            value: difficulty.to_string(),
        });
    };
    Ok(format!("api/charts/{song_id}/{name}.json"))
}

fn card_detail_paths_for_player(
    player: &PlayerConfig,
    cards: &Value,
) -> Result<Vec<String>, DataError> {
    let mut paths = Vec::new();
    for (card_id, player_card) in &player.card_list {
        if player_card.level_is_auto_max() {
            continue;
        }
        let parsed_id = card_id
            .parse::<u32>()
            .map_err(|_| DataError::InvalidField {
                field: "cardList.cardId",
                value: card_id.clone(),
            })?;
        if !card_has_level(cards, card_id, player_card.level)? {
            paths.push(format!("api/cards/{parsed_id}.json"));
        }
    }
    Ok(paths)
}

fn event_detail_sync_path(event_id: u32) -> Option<String> {
    (event_id != CUSTOM_EVENT_ID).then(|| format!("api/events/{event_id}.json"))
}

fn ids_from_json_object_or_array(
    value: &Value,
    field: &'static str,
) -> Result<Vec<u32>, DataError> {
    if let Some(object) = value.as_object() {
        return object
            .keys()
            .map(|key| parse_u32_field(field, key))
            .collect();
    }

    if let Some(array) = value.as_array() {
        return array
            .iter()
            .enumerate()
            .filter(|(_, value)| !value.is_null())
            .map(|(index, _)| {
                u32::try_from(index).map_err(|_| DataError::InvalidField {
                    field,
                    value: index.to_string(),
                })
            })
            .collect();
    }

    Err(DataError::InvalidField {
        field,
        value: "expected object or array".to_owned(),
    })
}

fn chart_selections_from_songs_json(songs: &Value) -> Result<BTreeSet<ChartSelection>, DataError> {
    let mut charts = BTreeSet::new();
    let entries = if let Some(object) = songs.as_object() {
        object
            .iter()
            .map(|(key, value)| (key.clone(), value))
            .collect::<Vec<_>>()
    } else if let Some(array) = songs.as_array() {
        array
            .iter()
            .enumerate()
            .filter(|(_, value)| !value.is_null())
            .map(|(index, value)| (index.to_string(), value))
            .collect::<Vec<_>>()
    } else {
        return Err(DataError::InvalidField {
            field: "songs",
            value: "expected object or array".to_owned(),
        });
    };

    for (song_id, song) in entries {
        let song_id = parse_u32_field("songs.songId", &song_id)?;
        let Some(difficulty) = song.get("difficulty").and_then(Value::as_object) else {
            continue;
        };

        for difficulty_id in difficulty.keys() {
            charts.insert(ChartSelection {
                song_id,
                difficulty: parse_u8_field("songs.difficulty", difficulty_id)?,
            });
        }
    }

    Ok(charts)
}

fn parse_u32_field(field: &'static str, value: &str) -> Result<u32, DataError> {
    value.parse::<u32>().map_err(|_| DataError::InvalidField {
        field,
        value: value.to_owned(),
    })
}

fn parse_u8_field(field: &'static str, value: &str) -> Result<u8, DataError> {
    value.parse::<u8>().map_err(|_| DataError::InvalidField {
        field,
        value: value.to_owned(),
    })
}

fn card_has_level(cards: &Value, card_id: &str, level: u8) -> Result<bool, DataError> {
    let Some(card) = cards.get(card_id) else {
        return Err(DataError::MissingEntity {
            kind: "card",
            id: card_id.to_owned(),
        });
    };
    Ok(card
        .get("stat")
        .and_then(Value::as_object)
        .is_some_and(|stat| stat.contains_key(&level.to_string())))
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), DataError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| DataError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let temp_path = temp_output_path(path);
    fs::write(&temp_path, data).map_err(|source| DataError::Io {
        path: temp_path.display().to_string(),
        source,
    })?;
    fs::rename(&temp_path, path).map_err(|source| DataError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn temp_output_path(path: &Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!("{extension}.tmp"))
        .unwrap_or_else(|| "tmp".to_owned());
    path.with_extension(extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_changed_manifest_metadata() {
        let local = StaticMirrorFileMeta {
            hash: Some("sha256:old".to_owned()),
            size: Some(1),
            source: None,
            etag: Some("\"old\"".to_owned()),
            last_modified: None,
            version: None,
            updated_at: None,
        };
        let remote = StaticMirrorFileMeta {
            hash: Some("sha256:new".to_owned()),
            size: Some(1),
            source: None,
            etag: Some("\"old\"".to_owned()),
            last_modified: None,
            version: None,
            updated_at: None,
        };

        assert!(needs_update(Some(&local), Some(&remote)));
        assert!(!needs_update(Some(&remote), Some(&remote)));
        assert!(needs_update(None, Some(&remote)));
        assert!(!needs_update(Some(&remote), None));
    }

    #[test]
    fn builds_chart_paths() {
        assert_eq!(chart_path(1, 3).unwrap(), "api/charts/1/expert.json");
        assert!(chart_path(1, 9).is_err());
    }

    #[test]
    fn skips_remote_event_detail_for_custom_event() {
        assert_eq!(event_detail_sync_path(0), None);
        assert_eq!(
            event_detail_sync_path(287).as_deref(),
            Some("api/events/287.json")
        );
    }

    #[test]
    fn rejects_unsafe_cache_paths() {
        let builder = BestdoriCachedFilesystemCalculationInputBuilder::new(
            BestdoriStaticMirrorConfig::new("/tmp/cache", "/game-data"),
        )
        .unwrap();

        assert!(builder.local_path("../cards.json").is_err());
        assert!(builder.local_path("charts//expert.json").is_err());
        assert_eq!(
            builder.local_path("api/cards/all.5.json").unwrap(),
            PathBuf::from("/tmp/cache/api/cards/all.5.json")
        );
    }

    #[test]
    fn selects_card_detail_paths_only_for_requested_missing_levels() {
        let player = PlayerConfig {
            mongo_id: None,
            player_id: 1,
            current_event: None,
            event_songs: BTreeMap::new(),
            event_presets: BTreeMap::new(),
            event_overrides: BTreeMap::new(),
            card_list: BTreeMap::from([
                (
                    "1".to_owned(),
                    bangdream_optimize_core::PlayerCardConfig {
                        level: 50,
                        training: true,
                        illust_training_status: true,
                        episodes: [true, true],
                        limit_break_rank: 0,
                        skill_level: 1,
                    },
                ),
                (
                    "2".to_owned(),
                    bangdream_optimize_core::PlayerCardConfig {
                        level: 0,
                        training: true,
                        illust_training_status: true,
                        episodes: [true, true],
                        limit_break_rank: 0,
                        skill_level: 1,
                    },
                ),
                (
                    "3".to_owned(),
                    bangdream_optimize_core::PlayerCardConfig {
                        level: 60,
                        training: true,
                        illust_training_status: true,
                        episodes: [true, true],
                        limit_break_rank: 0,
                        skill_level: 1,
                    },
                ),
            ]),
            area_item: BTreeMap::new(),
            character_bouns: BTreeMap::new(),
        };
        let cards = serde_json::json!({
            "1": {"stat": {"60": {}}},
            "2": {"stat": {"60": {}}},
            "3": {"stat": {"60": {}}}
        });

        assert_eq!(
            card_detail_paths_for_player(&player, &cards).unwrap(),
            vec!["api/cards/1.json"]
        );
    }
}
