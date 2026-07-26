use async_trait::async_trait;
use bangdream_optimize_core::PlayerConfig;
use bangdream_optimize_data::{DataError, PlayerConfigRepository, PlayerConfigStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub struct LocalPlayerConfigStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivePlayer {
    player_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserConfigProfile {
    pub id: String,
    pub name: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveUserConfig {
    config_id: String,
}

impl LocalPlayerConfigStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn get(&self, player_id: i64) -> Result<Option<PlayerConfig>, DataError> {
        let path = self.player_path(player_id);
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read(&path).map_err(|source| io_error(&path, source))?;
        serde_json::from_slice(&data)
            .map(Some)
            .map_err(|source| DataError::Json {
                path: path.display().to_string(),
                source,
            })
    }

    pub fn save(&self, player: PlayerConfig) -> Result<(), DataError> {
        let path = self.player_path(player.player_id);
        let data = serde_json::to_vec_pretty(&player).map_err(|err| DataError::JsonString {
            message: err.to_string(),
        })?;
        write_file(&path, &data)
    }

    pub fn get_active(&self) -> Result<Option<PlayerConfig>, DataError> {
        let Some(player_id) = self.active_player_id()? else {
            return Ok(None);
        };
        self.get(player_id)
    }

    pub fn save_active(&self, player: PlayerConfig) -> Result<(), DataError> {
        let player_id = player.player_id;
        self.save(player)?;
        self.set_active_player_id(player_id)
    }

    pub fn active_player_id(&self) -> Result<Option<i64>, DataError> {
        let path = self.active_player_path();
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read(&path).map_err(|source| io_error(&path, source))?;
        serde_json::from_slice::<ActivePlayer>(&data)
            .map(|active| Some(active.player_id))
            .map_err(|source| DataError::Json {
                path: path.display().to_string(),
                source,
            })
    }

    pub fn set_active_player_id(&self, player_id: i64) -> Result<(), DataError> {
        let data = serde_json::to_vec_pretty(&ActivePlayer { player_id }).map_err(|err| {
            DataError::JsonString {
                message: err.to_string(),
            }
        })?;
        write_file(&self.active_player_path(), &data)
    }

    pub fn delete(&self, player_id: i64) -> Result<bool, DataError> {
        let path = self.player_path(player_id);
        if !path.exists() {
            return Ok(false);
        }

        fs::remove_file(&path).map_err(|source| io_error(&path, source))?;
        Ok(true)
    }

    pub fn list_ids(&self) -> Result<Vec<i64>, DataError> {
        let dir = self.players_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::new();
        for entry in fs::read_dir(&dir).map_err(|source| io_error(&dir, source))? {
            let entry = entry.map_err(|source| io_error(&dir, source))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if let Ok(id) = stem.parse::<i64>() {
                ids.push(id);
            }
        }

        ids.sort_unstable();
        Ok(ids)
    }

    pub fn purge_legacy_player_cache(&self) -> Result<(), DataError> {
        let active_path = self.active_player_path();
        if active_path.exists() {
            fs::remove_file(&active_path).map_err(|source| io_error(&active_path, source))?;
        }

        let players_dir = self.players_dir();
        if players_dir.exists() {
            fs::remove_dir_all(&players_dir).map_err(|source| io_error(&players_dir, source))?;
        }

        Ok(())
    }

    pub fn list_user_config_profiles(&self) -> Result<Vec<UserConfigProfile>, DataError> {
        self.read_user_config_profiles()
    }

    pub fn load_user_config(&self, config_id: &str) -> Result<Option<PlayerConfig>, DataError> {
        self.load_user_config_value(config_id)?
            .map(player_config_from_value)
            .transpose()
    }

    pub fn load_user_config_value(&self, config_id: &str) -> Result<Option<Value>, DataError> {
        validate_config_id(config_id)?;
        let path = self.user_config_path(config_id);
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read(&path).map_err(|source| io_error(&path, source))?;
        serde_json::from_slice(&data)
            .map(Some)
            .map_err(|source| DataError::Json {
                path: path.display().to_string(),
                source,
            })
    }

    pub fn load_active_user_config(&self) -> Result<Option<PlayerConfig>, DataError> {
        self.load_active_user_config_value()?
            .map(player_config_from_value)
            .transpose()
    }

    pub fn load_active_user_config_value(&self) -> Result<Option<Value>, DataError> {
        let Some(config_id) = self.active_user_config_id()? else {
            return Ok(None);
        };
        self.load_user_config_value(&config_id)
    }

    pub fn save_active_user_config(&self, player: PlayerConfig) -> Result<(), DataError> {
        self.save_active_user_config_value(player_config_to_value(player)?)
    }

    pub fn save_active_user_config_value(&self, player: Value) -> Result<(), DataError> {
        if let Some(config_id) = self.active_user_config_id()? {
            self.save_user_config_value(&config_id, player)?;
        } else {
            self.create_user_config_value("默认配置", player)?;
        }
        Ok(())
    }

    pub fn create_user_config(
        &self,
        name: impl Into<String>,
        player: PlayerConfig,
    ) -> Result<UserConfigProfile, DataError> {
        self.create_user_config_value(name, player_config_to_value(player)?)
    }

    pub fn create_user_config_value(
        &self,
        name: impl Into<String>,
        player: Value,
    ) -> Result<UserConfigProfile, DataError> {
        let mut profiles = self.read_user_config_profiles()?;
        let profile = UserConfigProfile {
            id: unique_config_id(),
            name: normalize_profile_name(name.into(), "新配置"),
            updated_at: now_millis(),
        };
        self.write_user_config_value(&profile.id, &player)?;
        profiles.push(profile.clone());
        self.write_user_config_profiles(&profiles)?;
        self.set_active_user_config_id(&profile.id)?;
        Ok(profile)
    }

    pub fn save_user_config(&self, config_id: &str, player: PlayerConfig) -> Result<(), DataError> {
        self.save_user_config_value(config_id, player_config_to_value(player)?)
    }

    pub fn save_user_config_value(&self, config_id: &str, player: Value) -> Result<(), DataError> {
        validate_config_id(config_id)?;
        let mut profiles = self.read_user_config_profiles()?;
        let Some(profile) = profiles.iter_mut().find(|profile| profile.id == config_id) else {
            return Err(DataError::JsonString {
                message: format!("user config does not exist: {config_id}"),
            });
        };
        profile.updated_at = now_millis();
        self.write_user_config_value(config_id, &player)?;
        self.write_user_config_profiles(&profiles)
    }

    pub fn rename_user_config(
        &self,
        config_id: &str,
        name: impl Into<String>,
    ) -> Result<UserConfigProfile, DataError> {
        validate_config_id(config_id)?;
        let mut profiles = self.read_user_config_profiles()?;
        let Some(profile) = profiles.iter_mut().find(|profile| profile.id == config_id) else {
            return Err(DataError::JsonString {
                message: format!("user config does not exist: {config_id}"),
            });
        };
        profile.name = normalize_profile_name(name.into(), &profile.name);
        profile.updated_at = now_millis();
        let profile = profile.clone();
        self.write_user_config_profiles(&profiles)?;
        Ok(profile)
    }

    pub fn delete_user_config(&self, config_id: &str) -> Result<Option<String>, DataError> {
        validate_config_id(config_id)?;
        let mut profiles = self.read_user_config_profiles()?;
        if profiles.len() <= 1 {
            return Err(DataError::JsonString {
                message: "at least one user config must remain".to_string(),
            });
        }

        let old_len = profiles.len();
        profiles.retain(|profile| profile.id != config_id);
        if profiles.len() == old_len {
            return Err(DataError::JsonString {
                message: format!("user config does not exist: {config_id}"),
            });
        }

        let path = self.user_config_path(config_id);
        if path.exists() {
            fs::remove_file(&path).map_err(|source| io_error(&path, source))?;
        }
        self.write_user_config_profiles(&profiles)?;

        let active_id = self.active_user_config_id()?;
        if active_id.as_deref() == Some(config_id) {
            let next_id = profiles.first().map(|profile| profile.id.clone());
            if let Some(next_id) = &next_id {
                self.set_active_user_config_id(next_id)?;
            }
            Ok(next_id)
        } else {
            Ok(active_id)
        }
    }

    pub fn active_user_config_id(&self) -> Result<Option<String>, DataError> {
        let path = self.active_user_config_path();
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read(&path).map_err(|source| io_error(&path, source))?;
        serde_json::from_slice::<ActiveUserConfig>(&data)
            .map(|active| Some(active.config_id))
            .map_err(|source| DataError::Json {
                path: path.display().to_string(),
                source,
            })
    }

    pub fn set_active_user_config_id(&self, config_id: &str) -> Result<(), DataError> {
        validate_config_id(config_id)?;
        let data = serde_json::to_vec_pretty(&ActiveUserConfig {
            config_id: config_id.to_string(),
        })
        .map_err(|err| DataError::JsonString {
            message: err.to_string(),
        })?;
        write_file(&self.active_user_config_path(), &data)
    }

    fn players_dir(&self) -> PathBuf {
        self.root.join("players")
    }

    fn player_path(&self, player_id: i64) -> PathBuf {
        self.players_dir().join(format!("{player_id}.json"))
    }

    fn active_player_path(&self) -> PathBuf {
        self.root.join("active-player.json")
    }

    fn user_configs_dir(&self) -> PathBuf {
        self.root.join("user-configs")
    }

    fn user_config_path(&self, config_id: &str) -> PathBuf {
        self.user_configs_dir().join(format!("{config_id}.json"))
    }

    fn user_config_profiles_path(&self) -> PathBuf {
        self.root.join("user-config-profiles.json")
    }

    fn active_user_config_path(&self) -> PathBuf {
        self.root.join("active-user-config.json")
    }

    fn read_user_config_profiles(&self) -> Result<Vec<UserConfigProfile>, DataError> {
        let path = self.user_config_profiles_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let data = fs::read(&path).map_err(|source| io_error(&path, source))?;
        let mut profiles: Vec<UserConfigProfile> =
            serde_json::from_slice(&data).map_err(|source| DataError::Json {
                path: path.display().to_string(),
                source,
            })?;
        profiles.retain(|profile| validate_config_id(&profile.id).is_ok());
        Ok(profiles)
    }

    fn write_user_config_profiles(&self, profiles: &[UserConfigProfile]) -> Result<(), DataError> {
        let data = serde_json::to_vec_pretty(profiles).map_err(|err| DataError::JsonString {
            message: err.to_string(),
        })?;
        write_file(&self.user_config_profiles_path(), &data)
    }

    fn write_user_config_value(&self, config_id: &str, player: &Value) -> Result<(), DataError> {
        validate_config_id(config_id)?;
        let data = serde_json::to_vec_pretty(player).map_err(|err| DataError::JsonString {
            message: err.to_string(),
        })?;
        write_file(&self.user_config_path(config_id), &data)
    }
}

fn player_config_to_value(player: PlayerConfig) -> Result<Value, DataError> {
    serde_json::to_value(player).map_err(|err| DataError::JsonString {
        message: err.to_string(),
    })
}

fn player_config_from_value(value: Value) -> Result<PlayerConfig, DataError> {
    serde_json::from_value(value).map_err(|err| DataError::JsonString {
        message: err.to_string(),
    })
}

fn validate_config_id(config_id: &str) -> Result<(), DataError> {
    let valid = !config_id.is_empty()
        && config_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(DataError::JsonString {
            message: format!("invalid user config id: {config_id}"),
        })
    }
}

fn unique_config_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("cfg-{}-{nanos}", std::process::id())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn normalize_profile_name(value: String, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

#[async_trait]
impl PlayerConfigStore for LocalPlayerConfigStore {
    async fn get_player_config(&self, player_id: i64) -> Result<Option<PlayerConfig>, DataError> {
        self.get(player_id)
    }
}

#[async_trait]
impl PlayerConfigRepository for LocalPlayerConfigStore {
    async fn save_player_config(&self, player: PlayerConfig) -> Result<(), DataError> {
        self.save(player)
    }

    async fn delete_player_config(&self, player_id: i64) -> Result<bool, DataError> {
        self.delete(player_id)
    }

    async fn list_player_ids(&self) -> Result<Vec<i64>, DataError> {
        self.list_ids()
    }
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), DataError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    }

    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, data).map_err(|source| io_error(&temp_path, source))?;
    fs::rename(&temp_path, path).map_err(|source| io_error(path, source))
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
    use bangdream_optimize_core::PlayerConfig;
    use std::{
        collections::BTreeMap,
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
                "bangdream-optimize-storage-local-test-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("failed to create test dir");
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
    fn saves_loads_lists_and_deletes_player_configs() {
        let dir = TestDir::new();
        let store = LocalPlayerConfigStore::new(dir.path());

        assert!(store.get(42).unwrap().is_none());
        assert_eq!(store.list_ids().unwrap(), Vec::<i64>::new());

        store.save(player(42)).unwrap();
        store.save(player(7)).unwrap();

        let loaded = store.get(42).unwrap().unwrap();
        assert_eq!(loaded.player_id, 42);
        assert_eq!(loaded.mongo_id, None);
        assert_eq!(store.list_ids().unwrap(), vec![7, 42]);

        assert!(store.delete(7).unwrap());
        assert!(!store.delete(7).unwrap());
        assert_eq!(store.list_ids().unwrap(), vec![42]);
    }

    #[test]
    fn saves_and_loads_active_player_config_explicitly() {
        let dir = TestDir::new();
        let store = LocalPlayerConfigStore::new(dir.path());

        assert_eq!(store.active_player_id().unwrap(), None);
        assert!(store.get_active().unwrap().is_none());

        store.save(player(42)).unwrap();
        assert!(store.get_active().unwrap().is_none());

        store.save_active(player(7)).unwrap();
        store.save_active(player(42)).unwrap();

        assert_eq!(store.active_player_id().unwrap(), Some(42));
        assert_eq!(store.get_active().unwrap().unwrap().player_id, 42);
        assert_eq!(store.list_ids().unwrap(), vec![7, 42]);

        assert!(store.delete(42).unwrap());
        assert_eq!(store.active_player_id().unwrap(), Some(42));
        assert!(store.get_active().unwrap().is_none());
    }

    #[test]
    fn manages_user_configs_independently_from_player_id() {
        let dir = TestDir::new();
        let store = LocalPlayerConfigStore::new(dir.path());

        let first = store.create_user_config("主力", player(7)).unwrap();
        let second = store.create_user_config("备用", player(42)).unwrap();

        assert_ne!(first.id, second.id);
        assert_eq!(
            store.active_user_config_id().unwrap(),
            Some(second.id.clone())
        );
        assert_eq!(store.list_user_config_profiles().unwrap().len(), 2);
        assert_eq!(
            store.load_active_user_config().unwrap().unwrap().player_id,
            42
        );

        let mut edited = store.load_user_config(&first.id).unwrap().unwrap();
        edited.player_id = 10086;
        store.save_user_config(&first.id, edited).unwrap();
        store.set_active_user_config_id(&first.id).unwrap();

        assert_eq!(
            store.load_active_user_config().unwrap().unwrap().player_id,
            10086
        );
        assert!(store.load_user_config(&second.id).unwrap().is_some());

        let renamed = store.rename_user_config(&first.id, "主力修改").unwrap();
        assert_eq!(renamed.name, "主力修改");

        assert_eq!(
            store.delete_user_config(&first.id).unwrap(),
            Some(second.id.clone())
        );
        assert_eq!(store.active_user_config_id().unwrap(), Some(second.id));
    }

    #[test]
    fn user_config_values_preserve_frontend_only_settings() {
        let dir = TestDir::new();
        let store = LocalPlayerConfigStore::new(dir.path());
        let value = serde_json::json!({
            "playerId": 42,
            "server": "jp",
            "calculationMode": "ptMaximize",
            "ptMaximize": {
                "liveVariantByEventType": {
                    "challenge": "cooperative",
                    "festival": "solo"
                },
                "cooperativeLeaderMode": "specified",
                "cooperativeSpecifiedLeader": 3
            },
            "cardList": {},
            "areaItem": {},
            "characterBouns": {}
        });

        let profile = store
            .create_user_config_value("完整前端配置", value.clone())
            .unwrap();
        assert_eq!(
            store.load_user_config_value(&profile.id).unwrap(),
            Some(value.clone())
        );

        let mut edited = value;
        edited["ptMaximize"]["liveVariantByEventType"]["festival"] =
            Value::String("festival".to_string());
        store.save_active_user_config_value(edited.clone()).unwrap();
        assert_eq!(store.load_active_user_config_value().unwrap(), Some(edited));
    }

    #[test]
    fn purges_legacy_player_cache() {
        let dir = TestDir::new();
        let store = LocalPlayerConfigStore::new(dir.path());

        store.save_active(player(42)).unwrap();
        assert!(store.active_player_path().exists());
        assert!(store.players_dir().exists());

        store.purge_legacy_player_cache().unwrap();

        assert!(!store.active_player_path().exists());
        assert!(!store.players_dir().exists());
    }

    fn player(player_id: i64) -> PlayerConfig {
        PlayerConfig {
            mongo_id: None,
            player_id,
            current_event: Some(100),
            event_songs: BTreeMap::new(),
            event_presets: BTreeMap::new(),
            event_overrides: BTreeMap::new(),
            card_list: BTreeMap::new(),
            area_item: BTreeMap::new(),
            character_bouns: BTreeMap::new(),
        }
    }
}
