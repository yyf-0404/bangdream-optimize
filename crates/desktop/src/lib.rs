use async_trait::async_trait;
use bangdream_optimize_core::{
    BuildResult, ItemSearchOptions, PlayerConfig, ScoreRangeRequest, ScoreRangeResult, Server,
};
use bangdream_optimize_data::{
    update_published_score_range_chart_meta, BestdoriCachedFilesystemCalculator,
    BestdoriFilesystemCalculator, BestdoriFilesystemConfig, BestdoriStaticMirrorConfig, DataError,
    MaximizeInputBuilder, ScoreRangeInputBuilder,
};
use bangdream_optimize_service::MaximizeService;
use bangdream_optimize_storage_local::LocalPlayerConfigStore;
pub use bangdream_optimize_storage_local::UserConfigProfile;
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const EMBEDDED_FIX_FILES: [(&str, &[u8]); 4] = [
    (
        "cardsCNfix.json",
        include_bytes!("../../../var/game-data/cardsCNfix.json"),
    ),
    (
        "skillsCNfix.json",
        include_bytes!("../../../var/game-data/skillsCNfix.json"),
    ),
    (
        "areaItemFix.json",
        include_bytes!("../../../var/game-data/areaItemFix.json"),
    ),
    (
        "eventCharacterParameterBonusFix.json",
        include_bytes!("../../../var/game-data/eventCharacterParameterBonusFix.json"),
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopConfig {
    pub user_data_root: PathBuf,
    pub game_data: DesktopGameDataSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopGameDataSource {
    Filesystem {
        root: PathBuf,
    },
    StaticMirror {
        base_url: String,
        cache_root: PathBuf,
    },
    BestdoriApi {
        base_url: String,
        cache_root: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub struct DesktopOptimizer {
    player_store: LocalPlayerConfigStore,
    calculator: DesktopCalculator,
    service: MaximizeService,
}

#[derive(Debug, Clone)]
enum DesktopCalculator {
    Filesystem {
        root: PathBuf,
        calculator: Arc<Mutex<Option<BestdoriFilesystemCalculator>>>,
    },
    Remote {
        source: DesktopRemoteSource,
        calculator: BestdoriCachedFilesystemCalculator,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopRemoteSource {
    StaticMirror,
    BestdoriApi,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRuntimeInfo {
    pub runtime: &'static str,
    pub user_data_root: String,
    pub game_data: DesktopGameDataInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopGameDataInfo {
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopReferenceData {
    pub cards: Value,
    pub characters: Value,
    pub skills: Value,
    pub area_items: Value,
    pub events: Value,
    pub songs: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cards_fix: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_fix: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_items_fix: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_character_parameter_bonus_fix: Option<Value>,
}

impl DesktopOptimizer {
    pub fn new(config: DesktopConfig) -> Result<Self, DataError> {
        let player_store = LocalPlayerConfigStore::new(config.user_data_root);
        player_store.purge_legacy_player_cache()?;
        let calculator = match config.game_data {
            DesktopGameDataSource::Filesystem { root } => DesktopCalculator::Filesystem {
                root,
                calculator: Arc::new(Mutex::new(None)),
            },
            DesktopGameDataSource::StaticMirror {
                base_url,
                cache_root,
            } => {
                ensure_embedded_fix_files(&cache_root)?;
                let config = BestdoriStaticMirrorConfig::new(cache_root, base_url);
                DesktopCalculator::Remote {
                    source: DesktopRemoteSource::StaticMirror,
                    calculator: BestdoriCachedFilesystemCalculator::new(config)?,
                }
            }
            DesktopGameDataSource::BestdoriApi {
                base_url,
                cache_root,
            } => {
                ensure_embedded_fix_files(&cache_root)?;
                let config = BestdoriStaticMirrorConfig::from_bestdori_api(cache_root, base_url);
                DesktopCalculator::Remote {
                    source: DesktopRemoteSource::BestdoriApi,
                    calculator: BestdoriCachedFilesystemCalculator::new(config)?,
                }
            }
        };

        let service =
            MaximizeService::new(Arc::new(player_store.clone()), Arc::new(calculator.clone()));

        Ok(Self {
            player_store,
            calculator,
            service,
        })
    }

    pub fn player_store(&self) -> &LocalPlayerConfigStore {
        &self.player_store
    }

    pub fn load_player_config(&self, player_id: i64) -> Result<Option<PlayerConfig>, DataError> {
        self.player_store.get(player_id)
    }

    pub fn save_player_config(&self, player: PlayerConfig) -> Result<(), DataError> {
        self.player_store.save(player)
    }

    pub fn load_active_player_config(&self) -> Result<Option<PlayerConfig>, DataError> {
        self.player_store.get_active()
    }

    pub fn save_active_player_config(&self, player: PlayerConfig) -> Result<(), DataError> {
        self.player_store.save_active(player)
    }

    pub fn list_user_config_profiles(&self) -> Result<Vec<UserConfigProfile>, DataError> {
        self.player_store.list_user_config_profiles()
    }

    pub fn active_user_config_id(&self) -> Result<Option<String>, DataError> {
        self.player_store.active_user_config_id()
    }

    pub fn load_user_config(&self, config_id: &str) -> Result<Option<PlayerConfig>, DataError> {
        self.player_store.load_user_config(config_id)
    }

    pub fn load_active_user_config(&self) -> Result<Option<PlayerConfig>, DataError> {
        self.player_store.load_active_user_config()
    }

    pub fn save_active_user_config(&self, player: PlayerConfig) -> Result<(), DataError> {
        self.player_store.save_active_user_config(player)
    }

    pub fn create_user_config(
        &self,
        name: impl Into<String>,
        player: PlayerConfig,
    ) -> Result<UserConfigProfile, DataError> {
        self.player_store.create_user_config(name, player)
    }

    pub fn save_user_config(&self, config_id: &str, player: PlayerConfig) -> Result<(), DataError> {
        self.player_store.save_user_config(config_id, player)
    }

    pub fn rename_user_config(
        &self,
        config_id: &str,
        name: impl Into<String>,
    ) -> Result<UserConfigProfile, DataError> {
        self.player_store.rename_user_config(config_id, name)
    }

    pub fn delete_user_config(&self, config_id: &str) -> Result<Option<String>, DataError> {
        self.player_store.delete_user_config(config_id)
    }

    pub fn set_active_user_config_id(&self, config_id: &str) -> Result<(), DataError> {
        self.player_store.set_active_user_config_id(config_id)
    }

    pub fn delete_player_config(&self, player_id: i64) -> Result<bool, DataError> {
        self.player_store.delete(player_id)
    }

    pub fn list_player_ids(&self) -> Result<Vec<i64>, DataError> {
        self.player_store.list_ids()
    }

    pub fn sync_reference_data(&self) -> Result<DesktopReferenceData, DataError> {
        self.calculator.sync_reference_data()
    }

    pub fn refresh_core_game_data(&self) -> Result<(), DataError> {
        self.calculator.refresh_core_game_data()
    }

    pub fn sync_all_game_data(&self) -> Result<(), DataError> {
        self.calculator.sync_all_game_data()
    }

    pub fn clear_game_cache(&self) -> Result<(), DataError> {
        self.calculator.clear_game_cache()
    }

    pub fn runtime_info(&self) -> DesktopRuntimeInfo {
        DesktopRuntimeInfo {
            runtime: "desktop",
            user_data_root: self.player_store.root().display().to_string(),
            game_data: self.calculator.game_data_info(),
        }
    }

    pub fn calculate_for_player(
        &self,
        player_id: i64,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        let player = self
            .load_player_config(player_id)?
            .ok_or(DataError::PlayerNotFound { player_id })?;
        self.calculate_for_config(player, server, event_id, options)
    }

    pub fn calculate_for_config(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        self.calculator
            .calculate_result_sync(player, server, event_id, options)
    }

    pub fn score_range_for_config(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: ScoreRangeRequest,
    ) -> Result<Vec<ScoreRangeResult>, DataError> {
        self.calculator
            .score_range_sync(player, server, event_id, request)
    }

    pub async fn calculate_for_player_async(
        &self,
        player_id: i64,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        self.service
            .calculate_for_player(player_id, server, event_id, options)
            .await
    }

    pub async fn calculate_for_config_async(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        self.service
            .calculate_for_config(player, server, event_id, options)
            .await
    }
}

impl DesktopCalculator {
    fn calculate_result_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        match self {
            Self::Filesystem { root, calculator } => calculate_from_cached_filesystem(
                root, calculator, player, server, event_id, options,
            ),
            Self::Remote { calculator, .. } => {
                if ensure_embedded_fix_files(calculator.cache_root())? {
                    calculator.clear_loaded_calculator()?;
                }
                calculator.calculate_result_sync(player, server, event_id, options)
            }
        }
    }

    fn score_range_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: ScoreRangeRequest,
    ) -> Result<Vec<ScoreRangeResult>, DataError> {
        match self {
            Self::Filesystem { root, calculator } => score_range_from_cached_filesystem(
                root, calculator, player, server, event_id, request,
            ),
            Self::Remote { calculator, .. } => {
                if ensure_embedded_fix_files(calculator.cache_root())? {
                    calculator.clear_loaded_calculator()?;
                }
                calculator.score_range_sync(player, server, event_id, request)
            }
        }
    }

    fn sync_reference_data(&self) -> Result<DesktopReferenceData, DataError> {
        match self {
            Self::Filesystem { root, .. } => read_reference_data(root),
            Self::Remote { calculator, .. } => {
                calculator.sync_core()?;
                if ensure_embedded_fix_files(calculator.cache_root())? {
                    calculator.clear_loaded_calculator()?;
                }
                read_reference_data_from_config(BestdoriFilesystemConfig::from_bestdori_api_root(
                    calculator.cache_root(),
                ))
            }
        }
    }

    fn clear_game_cache(&self) -> Result<(), DataError> {
        match self {
            Self::Filesystem { calculator, .. } => {
                *calculator_lock(calculator)? = None;
                Ok(())
            }
            Self::Remote { calculator, .. } => {
                let root = calculator.cache_root();
                if root.exists() {
                    fs::remove_dir_all(root).map_err(|source| io_error(root, source))?;
                }
                calculator.clear_loaded_calculator()?;
                Ok(())
            }
        }
    }

    fn refresh_core_game_data(&self) -> Result<(), DataError> {
        match self {
            Self::Filesystem { .. } => Ok(()),
            Self::Remote { calculator, .. } => {
                calculator.refresh_core()?;
                if ensure_embedded_fix_files(calculator.cache_root())? {
                    calculator.clear_loaded_calculator()?;
                }
                Ok(())
            }
        }
    }

    fn sync_all_game_data(&self) -> Result<(), DataError> {
        match self {
            Self::Filesystem { .. } => Ok(()),
            Self::Remote { calculator, .. } => {
                calculator.sync_all()?;
                if ensure_embedded_fix_files(calculator.cache_root())? {
                    calculator.clear_loaded_calculator()?;
                }
                Ok(())
            }
        }
    }

    fn game_data_info(&self) -> DesktopGameDataInfo {
        match self {
            Self::Filesystem { root, .. } => DesktopGameDataInfo {
                source: "filesystem",
                root: Some(root.display().to_string()),
                base_url: None,
                cache_root: None,
            },
            Self::Remote { source, calculator } => DesktopGameDataInfo {
                source: match source {
                    DesktopRemoteSource::StaticMirror => "staticMirror",
                    DesktopRemoteSource::BestdoriApi => "bestdoriApi",
                },
                root: None,
                base_url: Some(calculator.base_url().to_owned()),
                cache_root: Some(calculator.cache_root().display().to_string()),
            },
        }
    }
}

#[async_trait]
impl MaximizeInputBuilder for DesktopCalculator {
    async fn maximize(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        self.calculate_result_sync(player, server, event_id, options)
    }
}

#[async_trait]
impl ScoreRangeInputBuilder for DesktopCalculator {
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

fn calculate_from_cached_filesystem(
    root: &Path,
    calculator: &Mutex<Option<BestdoriFilesystemCalculator>>,
    player: PlayerConfig,
    server: Server,
    event_id: Option<u32>,
    options: ItemSearchOptions,
) -> Result<BuildResult, DataError> {
    let mut calculator = calculator_lock(calculator)?;
    if calculator.is_none() {
        *calculator = Some(BestdoriFilesystemCalculator::load(
            BestdoriFilesystemConfig::from_root(root.to_path_buf()),
        )?);
    }
    calculator
        .as_ref()
        .expect("filesystem calculator was loaded")
        .calculate_result_sync(player, server, event_id, options)
}

fn score_range_from_cached_filesystem(
    root: &Path,
    calculator: &Mutex<Option<BestdoriFilesystemCalculator>>,
    player: PlayerConfig,
    server: Server,
    event_id: Option<u32>,
    request: ScoreRangeRequest,
) -> Result<Vec<ScoreRangeResult>, DataError> {
    let config = BestdoriFilesystemConfig::from_root(root.to_path_buf());
    let generated = update_published_score_range_chart_meta(&config, server)?;
    let mut calculator = calculator_lock(calculator)?;
    if generated {
        *calculator = None;
    }
    if calculator.is_none() {
        *calculator = Some(BestdoriFilesystemCalculator::load(config)?);
    }
    calculator
        .as_ref()
        .expect("filesystem calculator was loaded")
        .score_range_sync(player, server, event_id, request)
}

fn calculator_lock(
    calculator: &Mutex<Option<BestdoriFilesystemCalculator>>,
) -> Result<std::sync::MutexGuard<'_, Option<BestdoriFilesystemCalculator>>, DataError> {
    calculator.lock().map_err(|err| DataError::Storage {
        message: format!("desktop game-data calculator cache lock is poisoned: {err}"),
    })
}

fn read_reference_data(root: &Path) -> Result<DesktopReferenceData, DataError> {
    read_reference_data_from_config(BestdoriFilesystemConfig::from_root(root))
}

fn read_reference_data_from_config(
    config: BestdoriFilesystemConfig,
) -> Result<DesktopReferenceData, DataError> {
    Ok(DesktopReferenceData {
        cards: read_json(&config.cards_path)?,
        characters: read_json(&config.characters_path)?,
        skills: read_json(&config.skills_path)?,
        area_items: read_json(&config.area_items_path)?,
        events: read_json(&config.events_path)?,
        songs: read_json(&config.songs_path)?,
        cards_fix: read_optional_json(config.cards_fix_path.as_deref())?,
        skills_fix: read_optional_json(config.skills_fix_path.as_deref())?,
        area_items_fix: read_optional_json(config.area_items_fix_path.as_deref())?,
        event_character_parameter_bonus_fix: read_optional_json(
            config.event_character_parameter_bonus_fix_path.as_deref(),
        )?,
    })
}

fn read_json(path: &Path) -> Result<Value, DataError> {
    let data = fs::read(path).map_err(|source| io_error(path, source))?;
    serde_json::from_slice(&data).map_err(|source| DataError::Json {
        path: path.display().to_string(),
        source,
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

fn ensure_embedded_fix_files(root: &Path) -> Result<bool, DataError> {
    let mut changed = false;
    fs::create_dir_all(root).map_err(|source| io_error(root, source))?;
    for (file_name, data) in EMBEDDED_FIX_FILES {
        let path = root.join(file_name);
        if path.exists() {
            continue;
        }
        fs::write(&path, data).map_err(|source| io_error(&path, source))?;
        changed = true;
    }
    Ok(changed)
}

fn io_error(path: &Path, source: std::io::Error) -> DataError {
    DataError::Io {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bangdream_optimize_core::{
        AreaItemConfig, CharacterBonusConfig, EventType, PlayerCardConfig, SongSelection, StatRate,
    };
    use serde_json::{json, Value};
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "bangdream-optimize-desktop-test-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("failed to create test directory");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn calculates_from_saved_local_player_config() {
        let fixture = TestDir::new();
        let game_data_root = fixture.path().join("game-data");
        write_game_data_fixture(&game_data_root);

        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: fixture.path().join("user-data"),
            game_data: DesktopGameDataSource::Filesystem {
                root: game_data_root,
            },
        })
        .unwrap();

        optimizer.save_player_config(player()).unwrap();

        assert_eq!(optimizer.list_player_ids().unwrap(), vec![123]);

        let result = optimizer
            .calculate_for_player(123, Server::Jp, None, ItemSearchOptions::default())
            .unwrap();

        assert_eq!(result.event_id, 100);
        assert_eq!(result.event_type, EventType::Challenge);
        assert_eq!(result.songs.len(), 1);
        assert!(result.total_score > 0);
    }

    #[test]
    fn filesystem_calculator_cache_reloads_after_clear() {
        let fixture = TestDir::new();
        let game_data_root = fixture.path().join("game-data");
        write_game_data_fixture(&game_data_root);

        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: fixture.path().join("user-data"),
            game_data: DesktopGameDataSource::Filesystem {
                root: game_data_root.clone(),
            },
        })
        .unwrap();

        let initial = optimizer
            .calculate_for_config(player(), Server::Jp, None, ItemSearchOptions::default())
            .unwrap();

        write_json(
            game_data_root.join("cards.json"),
            cards_json_with_stat(2000),
        );

        let cached = optimizer
            .calculate_for_config(player(), Server::Jp, None, ItemSearchOptions::default())
            .unwrap();
        assert_eq!(cached.total_stat, initial.total_stat);

        optimizer.clear_game_cache().unwrap();

        let refreshed = optimizer
            .calculate_for_config(player(), Server::Jp, None, ItemSearchOptions::default())
            .unwrap();
        assert_ne!(refreshed.total_stat, initial.total_stat);
    }

    #[test]
    fn syncs_reference_data_for_shared_web_ui() {
        let fixture = TestDir::new();
        let game_data_root = fixture.path().join("game-data");
        write_game_data_fixture(&game_data_root);

        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: fixture.path().join("user-data"),
            game_data: DesktopGameDataSource::Filesystem {
                root: game_data_root,
            },
        })
        .unwrap();

        let reference = optimizer.sync_reference_data().unwrap();
        assert!(reference.cards.get("1").is_some());
        assert!(reference.characters.get("1").is_some());
        assert!(reference.area_items.get("10").is_some());
        assert!(reference.songs.get("1").is_some());
        assert!(reference.cards_fix.is_none());
    }

    #[test]
    fn reports_desktop_runtime_info() {
        let fixture = TestDir::new();
        let game_data_root = fixture.path().join("game-data");
        write_game_data_fixture(&game_data_root);
        let user_data_root = fixture.path().join("user-data");

        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: user_data_root.clone(),
            game_data: DesktopGameDataSource::Filesystem {
                root: game_data_root.clone(),
            },
        })
        .unwrap();

        let info = optimizer.runtime_info();

        assert_eq!(info.runtime, "desktop");
        assert_eq!(info.user_data_root, user_data_root.display().to_string());
        assert_eq!(info.game_data.source, "filesystem");
        assert_eq!(
            info.game_data.root,
            Some(game_data_root.display().to_string())
        );
        assert_eq!(info.game_data.base_url, None);
        assert_eq!(info.game_data.cache_root, None);
    }

    #[test]
    fn static_mirror_cache_includes_embedded_fix_files() {
        let fixture = TestDir::new();
        let cache_root = fixture.path().join("cache");

        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: fixture.path().join("user-data"),
            game_data: DesktopGameDataSource::StaticMirror {
                base_url: "https://mirror.example/game-data".to_owned(),
                cache_root: cache_root.clone(),
            },
        })
        .unwrap();

        let info = optimizer.runtime_info();
        assert_eq!(info.game_data.source, "staticMirror");
        for (file_name, _) in EMBEDDED_FIX_FILES {
            assert!(
                cache_root.join(file_name).is_file(),
                "{file_name} is missing"
            );
        }
    }

    #[test]
    fn reports_bestdori_api_as_a_distinct_remote_source() {
        let fixture = TestDir::new();
        let cache_root = fixture.path().join("cache");
        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: fixture.path().join("user-data"),
            game_data: DesktopGameDataSource::BestdoriApi {
                base_url: "https://bestdori.com".to_owned(),
                cache_root: cache_root.clone(),
            },
        })
        .unwrap();

        let info = optimizer.runtime_info();
        assert_eq!(info.game_data.source, "bestdoriApi");
        assert_eq!(
            info.game_data.base_url.as_deref(),
            Some("https://bestdori.com")
        );
        assert_eq!(
            info.game_data.cache_root.as_deref(),
            Some(cache_root.to_string_lossy().as_ref())
        );
    }

    #[tokio::test]
    async fn async_service_path_calculates_from_saved_local_player_config() {
        let fixture = TestDir::new();
        let game_data_root = fixture.path().join("game-data");
        write_game_data_fixture(&game_data_root);

        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: fixture.path().join("user-data"),
            game_data: DesktopGameDataSource::Filesystem {
                root: game_data_root,
            },
        })
        .unwrap();

        optimizer.save_player_config(player()).unwrap();

        let result = optimizer
            .calculate_for_player_async(123, Server::Jp, None, ItemSearchOptions::default())
            .await
            .unwrap();

        assert_eq!(result.event_id, 100);
        assert!(result.total_score > 0);
    }

    #[test]
    fn creates_filesystem_optimizer_without_existing_game_data() {
        let fixture = TestDir::new();
        let missing_game_data_root = fixture.path().join("missing-game-data");

        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: fixture.path().join("user-data"),
            game_data: DesktopGameDataSource::Filesystem {
                root: missing_game_data_root,
            },
        })
        .unwrap();

        assert!(optimizer.sync_reference_data().is_err());
    }

    #[test]
    fn saves_and_loads_active_player_config() {
        let fixture = TestDir::new();
        let game_data_root = fixture.path().join("game-data");
        write_game_data_fixture(&game_data_root);

        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: fixture.path().join("user-data"),
            game_data: DesktopGameDataSource::Filesystem {
                root: game_data_root,
            },
        })
        .unwrap();

        assert!(optimizer.load_active_player_config().unwrap().is_none());

        let mut active_player = player();
        active_player.player_id = 42;
        optimizer.save_player_config(player()).unwrap();
        optimizer.save_active_player_config(active_player).unwrap();

        assert_eq!(
            optimizer
                .load_active_player_config()
                .unwrap()
                .unwrap()
                .player_id,
            42
        );
        assert_eq!(optimizer.list_player_ids().unwrap(), vec![42, 123]);
    }

    #[test]
    fn reports_missing_saved_player_config() {
        let fixture = TestDir::new();
        let game_data_root = fixture.path().join("game-data");
        write_game_data_fixture(&game_data_root);
        let optimizer = DesktopOptimizer::new(DesktopConfig {
            user_data_root: fixture.path().join("user-data"),
            game_data: DesktopGameDataSource::Filesystem {
                root: game_data_root,
            },
        })
        .unwrap();

        let err = optimizer
            .calculate_for_player(404, Server::Jp, None, ItemSearchOptions::default())
            .unwrap_err();

        assert!(matches!(err, DataError::PlayerNotFound { player_id: 404 }));
    }

    fn write_game_data_fixture(root: &Path) {
        fs::create_dir_all(root.join("charts/1")).unwrap();
        write_json(root.join("cards.json"), cards_json());
        write_json(root.join("characters.json"), characters_json());
        write_json(root.join("skills.json"), skills_json());
        write_json(root.join("areaItems.json"), area_items_json());
        write_json(root.join("events.json"), events_json());
        write_json(root.join("songs.json"), songs_json());
        write_json(root.join("charts/1/expert.json"), chart_json());
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
        cards_json_with_stat(1000)
    }

    fn cards_json_with_stat(stat: i32) -> Value {
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
                        "60": {"performance": stat, "technique": stat, "visual": stat},
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
