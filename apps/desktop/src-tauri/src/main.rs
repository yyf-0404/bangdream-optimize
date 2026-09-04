#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bangdream_optimize_core::{
    BuildResult, ItemSearchOptions, PlayerConfig, PtEvaluateRequest, PtEvaluateResult,
    PtMaximizeRequest, PtMaximizeResult,
    ScoreRangeRequest, ScoreRangeResult, Server,
};
use bangdream_optimize_desktop::{
    DesktopConfig, DesktopGameDataSource, DesktopOptimizer, DesktopReferenceData,
    DesktopRuntimeInfo, UserConfigProfile,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env,
    fmt::Display,
    fs,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::Duration,
};
use tauri::{AppHandle, Manager, State};

struct AppState {
    optimizer: Mutex<DesktopOptimizer>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserConfigList {
    profiles: Vec<UserConfigProfile>,
    active_id: Option<String>,
}

#[tauri::command]
fn load_player_config(state: State<'_, AppState>) -> Result<Option<Value>, String> {
    optimizer(&state)?
        .load_active_user_config_value()
        .map_err(command_error)
}

#[tauri::command]
fn save_player_config(state: State<'_, AppState>, player: Value) -> Result<(), String> {
    optimizer(&state)?
        .save_active_user_config_value(player)
        .map_err(command_error)
}

#[tauri::command]
fn list_player_configs(state: State<'_, AppState>) -> Result<UserConfigList, String> {
    let optimizer = optimizer(&state)?;
    if optimizer
        .list_user_config_profiles()
        .map_err(command_error)?
        .is_empty()
    {
        optimizer
            .create_user_config_value("默认配置", default_user_config())
            .map_err(command_error)?;
    }
    Ok(UserConfigList {
        profiles: optimizer
            .list_user_config_profiles()
            .map_err(command_error)?,
        active_id: optimizer.active_user_config_id().map_err(command_error)?,
    })
}

#[tauri::command]
fn select_player_config(
    state: State<'_, AppState>,
    config_id: String,
) -> Result<Option<Value>, String> {
    let optimizer = optimizer(&state)?;
    optimizer
        .set_active_user_config_id(&config_id)
        .map_err(command_error)?;
    optimizer
        .load_user_config_value(&config_id)
        .map_err(command_error)
}

#[tauri::command]
fn create_player_config(
    state: State<'_, AppState>,
    name: String,
    player: Value,
) -> Result<UserConfigProfile, String> {
    optimizer(&state)?
        .create_user_config_value(name, player)
        .map_err(command_error)
}

#[tauri::command]
fn duplicate_player_config(
    state: State<'_, AppState>,
    name: String,
    player: Value,
) -> Result<UserConfigProfile, String> {
    optimizer(&state)?
        .create_user_config_value(name, player)
        .map_err(command_error)
}

#[tauri::command]
fn rename_player_config(
    state: State<'_, AppState>,
    config_id: String,
    name: String,
) -> Result<UserConfigProfile, String> {
    optimizer(&state)?
        .rename_user_config(&config_id, name)
        .map_err(command_error)
}

#[tauri::command]
fn delete_player_config(
    state: State<'_, AppState>,
    config_id: String,
) -> Result<Option<Value>, String> {
    let optimizer = optimizer(&state)?;
    optimizer
        .delete_user_config(&config_id)
        .map_err(command_error)?;
    optimizer
        .load_active_user_config_value()
        .map_err(command_error)
}

#[tauri::command]
async fn import_bestdori_player_profile(
    server: String,
    player_id: u64,
    mode: u8,
) -> Result<Value, String> {
    if !matches!(server.as_str(), "jp" | "en" | "tw" | "cn" | "kr") {
        return Err(format!("unsupported server: {server}"));
    }
    if mode > 3 {
        return Err(format!("unsupported Bestdori player mode: {mode}"));
    }
    run_blocking_task(move || {
        let url = format!("https://bestdori.com/api/player/{server}/{player_id}?mode={mode}");
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(command_error)?;
        let response = client.get(url).send().map_err(command_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("Bestdori returned HTTP {status}"));
        }
        response.json::<Value>().map_err(command_error)
    })
    .await
}

#[tauri::command]
async fn clear_game_cache(state: State<'_, AppState>) -> Result<(), String> {
    run_optimizer_task(state, |optimizer| optimizer.clear_game_cache()).await
}

#[tauri::command]
async fn sync_reference_data(state: State<'_, AppState>) -> Result<DesktopReferenceData, String> {
    run_optimizer_task(state, |optimizer| optimizer.sync_reference_data()).await
}

#[tauri::command]
async fn sync_all_game_data(state: State<'_, AppState>) -> Result<(), String> {
    run_optimizer_task(state, |optimizer| optimizer.sync_all_game_data()).await
}

#[tauri::command]
async fn refresh_core_game_data(state: State<'_, AppState>) -> Result<(), String> {
    run_optimizer_task(state, |optimizer| optimizer.refresh_core_game_data()).await
}

#[tauri::command]
fn runtime_info(state: State<'_, AppState>) -> Result<DesktopRuntimeInfo, String> {
    Ok(optimizer(&state)?.runtime_info())
}

#[tauri::command]
async fn calculate_for_config(
    state: State<'_, AppState>,
    player: PlayerConfig,
    server: Server,
    event_id: Option<u32>,
    options: ItemSearchOptions,
) -> Result<BuildResult, String> {
    run_optimizer_task(state, move |optimizer| {
        optimizer.calculate_for_config(player, server, event_id, options)
    })
    .await
}

#[tauri::command]
async fn score_range_for_config(
    state: State<'_, AppState>,
    player: PlayerConfig,
    server: Server,
    event_id: Option<u32>,
    request: ScoreRangeRequest,
) -> Result<Vec<ScoreRangeResult>, String> {
    run_optimizer_task(state, move |optimizer| {
        optimizer.score_range_for_config(player, server, event_id, request)
    })
    .await
}

#[tauri::command]
async fn pt_maximize_for_config(
    state: State<'_, AppState>,
    player: PlayerConfig,
    server: Server,
    event_id: Option<u32>,
    request: PtMaximizeRequest,
) -> Result<PtMaximizeResult, String> {
    run_optimizer_task(state, move |optimizer| {
        optimizer.pt_maximize_for_config(player, server, event_id, request)
    })
    .await
}

#[tauri::command]
async fn pt_evaluate_for_config(
    state: State<'_, AppState>,
    player: PlayerConfig,
    server: Server,
    event_id: Option<u32>,
    request: PtEvaluateRequest,
) -> Result<PtEvaluateResult, String> {
    run_optimizer_task(state, move |optimizer| {
        optimizer.pt_evaluate_for_config(player, server, event_id, request)
    })
    .await
}

#[tauri::command]
async fn save_json_file(file_name: String, text: String) -> Result<bool, String> {
    run_blocking_task(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("导出诊断")
            .set_file_name(file_name)
            .add_filter("JSON 文件", &["json"])
            .save_file()
        else {
            return Ok(false);
        };
        fs::write(&path, text)
            .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
        Ok(true)
    })
    .await
}

fn optimizer<'a>(
    state: &'a State<'_, AppState>,
) -> Result<MutexGuard<'a, DesktopOptimizer>, String> {
    state
        .optimizer
        .lock()
        .map_err(|err| format!("desktop optimizer lock is poisoned: {err}"))
}

async fn run_optimizer_task<T, E, F>(state: State<'_, AppState>, task: F) -> Result<T, String>
where
    T: Send + 'static,
    E: Display + Send + 'static,
    F: FnOnce(DesktopOptimizer) -> Result<T, E> + Send + 'static,
{
    let optimizer = optimizer(&state)?.clone();
    run_blocking_task(move || task(optimizer).map_err(command_error)).await
}

async fn run_blocking_task<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|err| format!("desktop background task failed: {err}"))?
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let optimizer = DesktopOptimizer::new(desktop_config(app.handle())?)
                .map_err(|err| setup_error(err.to_string()))?;
            app.manage(AppState {
                optimizer: Mutex::new(optimizer),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_player_config,
            save_player_config,
            list_player_configs,
            select_player_config,
            create_player_config,
            duplicate_player_config,
            rename_player_config,
            delete_player_config,
            import_bestdori_player_profile,
            clear_game_cache,
            sync_reference_data,
            sync_all_game_data,
            refresh_core_game_data,
            runtime_info,
            calculate_for_config,
            score_range_for_config,
            pt_maximize_for_config,
            pt_evaluate_for_config,
            save_json_file,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run bangdream-optimize desktop app");
}

fn desktop_config(app: &AppHandle) -> Result<DesktopConfig, Box<dyn std::error::Error>> {
    let app_data = app.path().app_data_dir()?;
    let user_data_root = env_path("BANGDREAM_OPTIMIZE_DESKTOP_USER_DATA_ROOT")
        .unwrap_or_else(|| app_data.join("user-data"));
    let cache_root = env_path("BANGDREAM_OPTIMIZE_DESKTOP_GAME_DATA_CACHE_ROOT")
        .unwrap_or_else(|| app_data.join("game-data"));

    let game_data = if let Some(root) = env_path("BANGDREAM_OPTIMIZE_DESKTOP_GAME_DATA_ROOT")
        .or_else(|| env_path("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT"))
    {
        DesktopGameDataSource::Filesystem { root }
    } else if let Some(base_url) = env_string("BANGDREAM_OPTIMIZE_DESKTOP_GAME_DATA_BASE_URL")
        .or_else(|| env_string("BANGDREAM_OPTIMIZE_GAME_DATA_BASE_URL"))
    {
        DesktopGameDataSource::StaticMirror {
            base_url,
            cache_root,
        }
    } else if let Some(root) = project_game_data_root() {
        DesktopGameDataSource::Filesystem { root }
    } else {
        DesktopGameDataSource::BestdoriApi {
            base_url: "https://bestdori.com".to_owned(),
            cache_root,
        }
    };

    Ok(DesktopConfig {
        user_data_root,
        game_data,
    })
}

fn env_path(key: &str) -> Option<PathBuf> {
    env_string(key).map(PathBuf::from)
}

fn project_game_data_root() -> Option<PathBuf> {
    let root = env::current_dir().ok()?.join("var/game-data");
    root.exists().then_some(root)
}

fn env_string(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn default_player_config() -> PlayerConfig {
    PlayerConfig {
        mongo_id: None,
        player_id: 0,
        current_event: None,
        event_songs: BTreeMap::new(),
        event_presets: BTreeMap::new(),
        event_overrides: BTreeMap::new(),
        card_list: BTreeMap::new(),
        area_item: BTreeMap::new(),
        character_bouns: BTreeMap::new(),
    }
}

fn default_user_config() -> Value {
    serde_json::to_value(default_player_config()).expect("default player config is serializable")
}

fn command_error(err: impl std::fmt::Display) -> String {
    err.to_string()
}

fn setup_error(message: String) -> Box<dyn std::error::Error> {
    Box::new(std::io::Error::new(std::io::ErrorKind::Other, message))
}
