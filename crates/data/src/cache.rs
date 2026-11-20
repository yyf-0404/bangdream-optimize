use crate::{
    published_score_range_song_selections, update_score_range_chart_meta,
    BestdoriFilesystemCalculationInputBuilder, BestdoriFilesystemConfig, DataError,
    MaximizeInputBuilder, PtMaximizeInputBuilder, ScoreRangeInputBuilder,
};
use async_trait::async_trait;
use bangdream_optimize_core::{
    BuildResult, ItemSearchOptions, PlayerConfig, PtEvaluateRequest, PtEvaluateResult,
    PtMaximizeRequest, PtMaximizeResult, ScoreRangeRequest, ScoreRangeResult, Server,
    SongSelection,
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
const CHART_SYNC_CONCURRENCY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ChartSelection {
    song_id: u32,
    difficulty: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BestdoriStaticMirrorConfig {
    pub cache_root: PathBuf,
    pub base_url: String,
    source: RemoteGameDataSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteGameDataSource {
    StaticMirror,
    BestdoriApi,
}

impl BestdoriStaticMirrorConfig {
    pub fn new(cache_root: impl Into<PathBuf>, base_url: impl Into<String>) -> Self {
        Self {
            cache_root: cache_root.into(),
            base_url: base_url.into(),
            source: RemoteGameDataSource::StaticMirror,
        }
    }

    pub fn from_bestdori_api(cache_root: impl Into<PathBuf>, base_url: impl Into<String>) -> Self {
        Self {
            cache_root: cache_root.into(),
            base_url: base_url.into(),
            source: RemoteGameDataSource::BestdoriApi,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BestdoriCachedFilesystemCalculationInputBuilder {
    config: BestdoriStaticMirrorConfig,
    client: Client,
    calculator: Arc<Mutex<Option<BestdoriFilesystemCalculationInputBuilder>>>,
    card_repairs: Option<Arc<Value>>,
    area_item_repairs: Option<Arc<Value>>,
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
            card_repairs: None,
            area_item_repairs: None,
        })
    }

    /// Apply immutable application corrections after every remote catalogue
    /// load. A mirror refresh cannot restore an obsolete correction table.
    pub fn with_card_repairs(mut self, repairs: Value) -> Result<Self, DataError> {
        if !repairs.is_object() {
            return Err(DataError::InvalidField {
                field: "cardsCNfix", value: "expected object".to_owned(),
            });
        }
        self.card_repairs = Some(Arc::new(repairs));
        // Changing a cloned builder's source contract detaches its loaded cache.
        self.calculator = Arc::new(Mutex::new(None));
        Ok(self)
    }

    /// Keep application item corrections authoritative after remote refreshes.
    pub fn with_area_item_repairs(mut self, repairs: Value) -> Result<Self, DataError> {
        if !repairs.is_object() {
            return Err(DataError::InvalidField {
                field: "areaItemFix", value: "expected object".to_owned(),
            });
        }
        self.area_item_repairs = Some(Arc::new(repairs));
        self.calculator = Arc::new(Mutex::new(None));
        Ok(self)
    }

    pub fn maximize_sync(
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
        let selected_event_id = event_id
            .or(player.current_event)
            .ok_or(DataError::MissingCurrentEvent)?;
        let changed = self.sync_score_range_for_inner(selected_event_id, &player, server)?;
        if changed {
            self.clear_loaded_calculator()?;
        }
        self.score_range_from_cache(player, server, Some(selected_event_id), request)
    }

    pub fn pt_maximize_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: PtMaximizeRequest,
    ) -> Result<PtMaximizeResult, DataError> {
        let selected_event_id = event_id
            .or(player.current_event)
            .ok_or(DataError::MissingCurrentEvent)?;
        if self.sync_for_inner(selected_event_id, &request.songs, Some(&player))? {
            self.clear_loaded_calculator()?;
        }
        self.pt_maximize_from_cache(player, server, Some(selected_event_id), request)
    }

    pub fn pt_evaluate_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: PtEvaluateRequest,
    ) -> Result<PtEvaluateResult, DataError> {
        let selected_event_id = event_id
            .or(player.current_event)
            .ok_or(DataError::MissingCurrentEvent)?;
        if self.sync_for_inner(selected_event_id, &request.songs, Some(&player))? {
            self.clear_loaded_calculator()?;
        }
        self.pt_evaluate_from_cache(player, server, Some(selected_event_id), request)
    }

    fn sync_score_range_for_inner(
        &self,
        event_id: u32,
        player: &PlayerConfig,
        server: Server,
    ) -> Result<bool, DataError> {
        let mut changed = self.sync_for_inner(event_id, &[], Some(player))?;
        let songs = self.read_json("api/songs/all.7.json")?;
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let songs_by_id = songs
            .as_object()
            .ok_or(DataError::InvalidField {
                field: "songs",
                value: "expected object".to_owned(),
            })?
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let selections =
            published_score_range_song_selections(&songs_by_id, server, now_millis, |_, _| true)?;
        changed |= self.sync_chart_files(&selections)?;
        let selections = self.cached_chart_selections(&selections)?;
        let config =
            BestdoriFilesystemConfig::from_bestdori_api_root(self.config.cache_root.clone());
        changed |= update_score_range_chart_meta(&config, &songs, &selections)?;
        Ok(changed)
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

    pub fn sync_card_detail(&self, card_id: u32) -> Result<Value, DataError> {
        let path = format!("api/cards/{card_id}.json");
        if self.sync_remote_file(&path, false)? {
            self.clear_loaded_calculator()?;
        }
        self.read_json(&path)
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

        let mut changed = if self.uses_manifest() {
            let remote_manifest = self.fetch_manifest()?;
            let local_manifest = self.read_local_manifest()?;
            let changed = self.sync_core_files(&remote_manifest, local_manifest.as_ref(), false)?;
            self.write_json("manifest.json", &remote_manifest)?;
            changed
        } else {
            self.sync_core_files_from_bestdori(false)?
        };

        if let Some(player) = player {
            let mut cards = self.read_json("api/cards/all.5.json")?;
            if let Some(repairs) = &self.card_repairs {
                let cards = cards.as_object_mut().ok_or(DataError::MissingField { field: "cards" })?;
                for (id, card) in repairs.as_object().expect("validated card repairs") {
                    cards.insert(id.clone(), card.clone());
                }
            }
            for path in card_detail_paths_for_player(player, &cards)? {
                changed |= self.sync_remote_file(&path, false)?;
            }
        }

        if let Some(event_path) = event_detail_sync_path(event_id) {
            changed |= self.sync_remote_file(&event_path, false)?;
        }

        for song in song_list {
            let path = chart_path(song.song_id, song.difficulty)?;
            changed |= self.sync_remote_file(&path, false)?;
        }
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

        if self.uses_manifest() {
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
        } else {
            self.sync_core_files_from_bestdori(refresh_without_manifest)
        }
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
        let selections = chart_selections_from_songs_json(&songs)?
            .into_iter()
            .map(|selection| SongSelection {
                song_id: selection.song_id,
                difficulty: selection.difficulty,
            })
            .collect::<Vec<_>>();
        changed |= self.sync_chart_files(&selections)?;
        let selections = self.cached_chart_selections(&selections)?;
        let config =
            BestdoriFilesystemConfig::from_bestdori_api_root(self.config.cache_root.clone());
        changed |= update_score_range_chart_meta(&config, &songs, &selections)?;

        Ok(changed)
    }

    fn sync_chart_files(&self, selections: &[SongSelection]) -> Result<bool, DataError> {
        let mut changed = false;
        for chunk in selections.chunks(CHART_SYNC_CONCURRENCY) {
            let results = std::thread::scope(|scope| {
                chunk
                    .iter()
                    .map(|selection| {
                        scope.spawn(move || {
                            let path = chart_path(selection.song_id, selection.difficulty)?;
                            self.sync_optional_file_without_manifest(&path)
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|handle| {
                        handle.join().map_err(|_| DataError::Storage {
                            message: "chart sync worker panicked".to_owned(),
                        })?
                    })
                    .collect::<Result<Vec<_>, DataError>>()
            })?;
            changed |= results.into_iter().any(|result| result);
        }
        Ok(changed)
    }

    fn cached_chart_selections(
        &self,
        selections: &[SongSelection],
    ) -> Result<Vec<SongSelection>, DataError> {
        selections
            .iter()
            .filter_map(|selection| {
                let path = chart_path(selection.song_id, selection.difficulty);
                match path.and_then(|path| self.local_path(&path)) {
                    Ok(path) if path.is_file() => Some(Ok(selection.clone())),
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .collect()
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

    fn sync_core_files_from_bestdori(&self, force: bool) -> Result<bool, DataError> {
        let mut changed = false;
        for path in REQUIRED_CORE_FILES {
            changed |= self.sync_file_without_manifest_inner(path, false, force)?;
        }
        Ok(changed)
    }

    fn uses_manifest(&self) -> bool {
        self.config.source == RemoteGameDataSource::StaticMirror
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

    fn sync_remote_file(&self, path: &str, optional: bool) -> Result<bool, DataError> {
        if self.uses_manifest() {
            self.sync_file_auto_manifest(path, optional)
        } else {
            self.sync_file_without_manifest_inner(path, optional, false)
        }
    }

    fn sync_file_without_manifest(&self, path: &str) -> Result<bool, DataError> {
        self.sync_file_without_manifest_inner(path, false, false)
    }

    fn sync_optional_file_without_manifest(&self, path: &str) -> Result<bool, DataError> {
        self.sync_file_without_manifest_inner(path, true, false)
    }

    fn sync_file_without_manifest_inner(
        &self,
        path: &str,
        allow_missing: bool,
        force: bool,
    ) -> Result<bool, DataError> {
        let local_path = self.local_path(path)?;
        if local_path.exists() && !force {
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
        if allow_missing && status.as_u16() == 404 {
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
            *calculator = Some(self.load_calculator()?);
        }
        calculator
            .as_ref()
            .expect("calculator was loaded")
            .calculate_result_sync(player, server, event_id, options)
    }

    fn score_range_from_cache(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: ScoreRangeRequest,
    ) -> Result<Vec<ScoreRangeResult>, DataError> {
        let mut calculator = self.calculator_lock()?;
        if calculator.is_none() {
            *calculator = Some(self.load_calculator()?);
        }
        calculator
            .as_ref()
            .expect("calculator was loaded")
            .score_range_sync(player, server, event_id, request)
    }

    fn pt_maximize_from_cache(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: PtMaximizeRequest,
    ) -> Result<PtMaximizeResult, DataError> {
        let mut calculator = self.calculator_lock()?;
        if calculator.is_none() {
            *calculator = Some(self.load_calculator()?);
        }
        calculator
            .as_ref()
            .expect("calculator was loaded")
            .pt_maximize_sync(player, server, event_id, request)
    }

    fn pt_evaluate_from_cache(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: PtEvaluateRequest,
    ) -> Result<PtEvaluateResult, DataError> {
        let mut calculator = self.calculator_lock()?;
        if calculator.is_none() {
            *calculator = Some(self.load_calculator()?);
        }
        calculator
            .as_ref()
            .expect("calculator was loaded")
            .pt_evaluate_sync(player, server, event_id, request)
    }

    fn load_calculator(&self) -> Result<BestdoriFilesystemCalculationInputBuilder, DataError> {
        let mut calculator = BestdoriFilesystemCalculationInputBuilder::load(
            BestdoriFilesystemConfig::from_bestdori_api_root(self.config.cache_root.clone()),
        )?;
        if let Some(repairs) = &self.card_repairs {
            calculator.apply_card_repairs(repairs)?;
        }
        if let Some(repairs) = &self.area_item_repairs {
            calculator.apply_area_item_repairs(repairs)?;
        }
        Ok(calculator)
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
impl MaximizeInputBuilder for BestdoriCachedFilesystemCalculationInputBuilder {
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
impl ScoreRangeInputBuilder for BestdoriCachedFilesystemCalculationInputBuilder {
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

#[async_trait]
impl PtMaximizeInputBuilder for BestdoriCachedFilesystemCalculationInputBuilder {
    async fn pt_maximize(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: PtMaximizeRequest,
    ) -> Result<PtMaximizeResult, DataError> {
        self.pt_maximize_sync(player, server, event_id, request)
    }

    async fn pt_evaluate(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: PtEvaluateRequest,
    ) -> Result<PtEvaluateResult, DataError> {
        self.pt_evaluate_sync(player, server, event_id, request)
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
    fn application_card_and_item_repairs_survive_reloads_and_detach_cloned_contexts() {
        let root = std::env::temp_dir().join(format!(
            "bangdream-optimize-card-overlay-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
        ));
        let old_card = serde_json::json!({"characterId":16,"rarity":2,"attribute":"powerful",
            "skillId":5,"stat":{"15":{"performance":1,"technique":2,"visual":3}}});
        let skill = |value| serde_json::json!({"duration":[5,5,5,5,5],
            "activationEffect":{"activateEffectTypes":{"score":{"activateEffectValue":[value]}}}});
        for (path, value) in [
            ("api/cards/all.5.json", serde_json::json!({"10005":old_card.clone()})),
            ("api/characters/main.3.json", serde_json::json!({"16":{"bandId":4}})),
            ("api/skills/all.10.json", serde_json::json!({"5":skill(5),"8":skill(8)})),
            ("api/areaItems/main.5.json", serde_json::json!({})),
            ("api/events/all.6.json", serde_json::json!({"1":{
                "eventType":"medley", "attributes":[], "characters":[],
            }})),
            ("api/songs/all.7.json", serde_json::json!({})),
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        let original = BestdoriCachedFilesystemCalculationInputBuilder::new(
            BestdoriStaticMirrorConfig::new(&root, "https://unused.invalid"),
        ).unwrap();
        let mut player: PlayerConfig = serde_json::from_value(serde_json::json!({"playerId":0})).unwrap();
        player.card_list.insert("10005".to_owned(), serde_json::from_value(serde_json::json!({
            "level":15,"training":false,"episodes":[false,false],"limitBreakRank":0,"skillLevel":5,
        })).unwrap());
        for id in [59, 68, 72] {
            player.area_item.insert(id.to_string(), serde_json::from_value(serde_json::json!({"level":5})).unwrap());
        }
        let item_repairs: Value = serde_json::from_str(include_str!("../../../var/game-data/areaItemFix.json")).unwrap();
        let mut obsolete_items = item_repairs.clone();
        for item in obsolete_items.as_object_mut().unwrap().values_mut() {
            item["targetBandIds"] = serde_json::json!([1]);
        }
        fs::write(root.join("areaItemFix.json"), serde_json::to_vec(&obsolete_items).unwrap()).unwrap();
        *original.calculator_lock().unwrap() = Some(original.load_calculator().unwrap());
        let repairs = serde_json::from_str(include_str!("../../../var/game-data/cardsCNfix.json")).unwrap();
        let corrected = original.clone().with_card_repairs(repairs).unwrap()
            .with_area_item_repairs(item_repairs).unwrap();
        assert!(!Arc::ptr_eq(&original.calculator, &corrected.calculator));
        assert!(original.calculator_lock().unwrap().is_some());
        assert!(corrected.calculator_lock().unwrap().is_none());
        // A refreshed mirror writes an obsolete correction, before the loader
        // installs the application's immutable source overlay.
        fs::write(root.join("cardsCNfix.json"), serde_json::to_vec(
            &serde_json::json!({"10005":old_card}),
        ).unwrap()).unwrap();
        for _ in 0..2 {
            let loaded = corrected.load_calculator().unwrap();
            let definitions = loaded.card_definitions(&player).unwrap();
            let card = &definitions[&10005];
            assert_eq!(card.level_stats[&15].performance, 2935);
            assert_eq!(card.level_stats[&15].technique, 3421);
            assert_eq!(card.skill.score_up.default, 0.08);
            let snapshot = loaded.snapshot_for(&player, 1, &[], Server::Cn).unwrap();
            for id in [59, 68, 72] {
                assert!(snapshot.area_item_definitions[&id].target_band_ids.contains(&45));
            }
        }
        let original_guard = original.calculator_lock().unwrap();
        assert_eq!(original_guard.as_ref().unwrap().card_definitions(&player).unwrap()[&10005]
            .level_stats[&15].performance, 1);
        let old_snapshot = original_guard.as_ref().unwrap().snapshot_for(&player, 1, &[], Server::Cn).unwrap();
        assert!(!old_snapshot.area_item_definitions[&59].target_band_ids.contains(&45));
        drop(original_guard);
        fs::remove_dir_all(&root).unwrap();
    }
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::{SystemTime, UNIX_EPOCH},
    };

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
    fn optional_chart_sync_skips_http_404() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let cache_root = std::env::temp_dir().join(format!(
            "bangdream-optimize-chart-404-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let builder = BestdoriCachedFilesystemCalculationInputBuilder::new(
            BestdoriStaticMirrorConfig::new(&cache_root, format!("http://{address}")),
        )
        .unwrap();

        assert!(!builder
            .sync_optional_file_without_manifest("api/charts/1/expert.json")
            .unwrap());
        assert!(!cache_root.join("api/charts/1/expert.json").exists());

        server.join().unwrap();
        let _ = fs::remove_dir_all(cache_root);
    }

    #[test]
    fn bestdori_api_syncs_core_files_without_requesting_manifest() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut paths = Vec::new();
            for _ in 0..REQUIRED_CORE_FILES.len() {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let count = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap()
                    .to_owned();
                paths.push(path);
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    )
                    .unwrap();
            }
            paths
        });
        let cache_root = std::env::temp_dir().join(format!(
            "bangdream-optimize-bestdori-api-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let builder = BestdoriCachedFilesystemCalculationInputBuilder::new(
            BestdoriStaticMirrorConfig::from_bestdori_api(&cache_root, format!("http://{address}")),
        )
        .unwrap();

        builder.sync_core().unwrap();
        let paths = server.join().unwrap();

        assert_eq!(
            paths,
            REQUIRED_CORE_FILES
                .iter()
                .map(|path| format!("/{path}"))
                .collect::<Vec<_>>()
        );
        assert!(paths.iter().all(|path| !path.ends_with("manifest.json")));
        assert!(!cache_root.join("manifest.json").exists());
        for path in REQUIRED_CORE_FILES {
            assert!(cache_root.join(path).is_file(), "{path} was not cached");
        }

        let _ = fs::remove_dir_all(cache_root);
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
            custom_cards: Default::default(),
            next_custom_card_id: 0,
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
