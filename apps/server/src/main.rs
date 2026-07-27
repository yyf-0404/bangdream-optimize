mod feedback;

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use bangdream_optimize_bangdream_account::{
    persist_path_from_env, BangDreamAccountImporter, ImportError as BangDreamImportError,
    ImportRequest as BangDreamImportRequest,
};
use bangdream_optimize_core::{
    calculate_from_candidates, BuildResult, CalculationMetrics, CandidateBuildRequest, EventType,
    ItemSearchOptions, PlayerConfig, PtMaximizeRequest, ScoreRangeRequest, Server,
};
use bangdream_optimize_data::{
    BestdoriCachedFilesystemCalculator, BestdoriFilesystemCalculator, BestdoriFilesystemConfig,
    BestdoriStaticMirrorConfig, DataError, MaximizeInputBuilder, PlayerConfigStore,
    PtMaximizeInputBuilder, ScoreRangeInputBuilder,
};
use bangdream_optimize_service::{MaximizeService, PtMaximizeService, ScoreRangeService};
use bangdream_optimize_storage_mongodb::MongoPlayerConfigStore;
use feedback::{
    validate_attachment_set, FeedbackAttachment, FeedbackRateLimiter, FeedbackReceipt,
    FeedbackRequest, FeedbackSender, FeedbackSubmission, SmtpFeedbackSender,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env,
    fs::{self, OpenOptions},
    io::ErrorKind,
    io::Write,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
};
use tracing_subscriber::EnvFilter;

const DEFAULT_GAME_DATA_SYNC_INTERVAL_SECONDS: u64 = 60 * 60;
static FEEDBACK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Default)]
struct AppState {
    maximize_service: Option<MaximizeService>,
    score_range_service: Option<ScoreRangeService>,
    score_range_searcher: Option<Arc<dyn ScoreRangeInputBuilder>>,
    pt_maximize_service: Option<PtMaximizeService>,
    pt_maximize_searcher: Option<Arc<dyn PtMaximizeInputBuilder>>,
    bangdream_importer: Option<BangDreamAccountImporter>,
    feedback_sender: Option<Arc<dyn FeedbackSender>>,
    feedback_rate_limiter: FeedbackRateLimiter,
    trust_proxy_headers: bool,
    telemetry: TelemetryLogger,
}

impl AppState {
    async fn from_env() -> Result<Self, String> {
        let player_store = match mongo_uri_from_env() {
            Some(uri) => {
                let db_name = env::var("BANGDREAM_OPTIMIZE_MONGODB_DB")
                    .or_else(|_| env::var("MONGODB_DB"))
                    .unwrap_or_else(|_| "tsugu-bangdream-bot".to_owned());
                Some(Arc::new(
                    MongoPlayerConfigStore::connect(&uri, &db_name)
                        .await
                        .map_err(|err| err.to_string())?,
                ) as Arc<dyn PlayerConfigStore>)
            }
            None => None,
        };

        let maximizer = maximizer_from_env()?;
        let maximize_service = match (player_store.clone(), maximizer) {
            (Some(player_store), Some(calculator)) => {
                Some(MaximizeService::new(player_store, calculator))
            }
            _ => None,
        };
        let score_range_searcher = score_range_searcher_from_env()?;
        let score_range_service = match (player_store.clone(), score_range_searcher.clone()) {
            (Some(player_store), Some(searcher)) => {
                Some(ScoreRangeService::new(player_store, searcher))
            }
            _ => None,
        };
        let pt_maximize_searcher = pt_maximize_searcher_from_env()?;
        let pt_maximize_service = match (player_store, pt_maximize_searcher.clone()) {
            (Some(player_store), Some(searcher)) => {
                Some(PtMaximizeService::new(player_store, searcher))
            }
            _ => None,
        };

        let bangdream_importer = if env_bool("BANGDREAM_OPTIMIZE_ENABLE_BD_IMPORT", true) {
            let persist_path = persist_path_from_env();
            Some(BangDreamAccountImporter::new(persist_path).map_err(|err| err.to_string())?)
        } else {
            None
        };
        let feedback_sender = SmtpFeedbackSender::from_env()?;

        Ok(Self {
            maximize_service,
            score_range_service,
            score_range_searcher,
            pt_maximize_service,
            pt_maximize_searcher,
            bangdream_importer,
            feedback_sender,
            feedback_rate_limiter: FeedbackRateLimiter::from_env(),
            trust_proxy_headers: env_bool("BANGDREAM_OPTIMIZE_FEEDBACK_TRUST_PROXY_HEADERS", false),
            telemetry: TelemetryLogger::from_env(),
        })
    }
}

#[derive(Clone, Default)]
struct TelemetryLogger {
    path: Option<Arc<PathBuf>>,
}

impl TelemetryLogger {
    fn from_env() -> Self {
        Self {
            path: non_empty_env("BANGDREAM_OPTIMIZE_TELEMETRY_JSONL")
                .map(PathBuf::from)
                .map(Arc::new),
        }
    }

    fn log_result(
        &self,
        route: &'static str,
        server: Option<Server>,
        requested_event_id: Option<u32>,
        result: &BuildResult,
    ) {
        let Some(path) = self.path.as_ref() else {
            return;
        };

        if let Err(err) = append_telemetry_event(
            path,
            &TelemetryEvent {
                schema_version: 1,
                timestamp_ms: unix_timestamp_ms(),
                route,
                server,
                requested_event_id,
                event_id: result.event_id,
                event_type: result.event_type,
                song_count: result.songs.len(),
                total_score: result.total_score,
                total_stat: result.total_stat,
                solver: result.solver.as_deref(),
                metrics: result.metrics.as_ref(),
            },
        ) {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "failed to write telemetry event"
            );
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TelemetryEvent<'a> {
    schema_version: u8,
    timestamp_ms: u128,
    route: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<Server>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested_event_id: Option<u32>,
    event_id: u32,
    event_type: EventType,
    song_count: usize,
    total_score: i32,
    total_stat: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    solver: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics: Option<&'a CalculationMetrics>,
}

#[derive(Debug, Clone)]
struct GameDataSyncRuntimeConfig {
    args: Vec<String>,
    interval: Option<Duration>,
    command: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct GameDataSyncCliConfig {
    enabled: Option<bool>,
    interval: Option<u64>,
    out: Option<PathBuf>,
    base_url: Option<String>,
    repair_dir: Option<PathBuf>,
    events: Vec<u32>,
    charts: Vec<(u32, u8)>,
    player_files: Vec<PathBuf>,
    all_event_details: Option<bool>,
    all_charts: Option<bool>,
    all_card_details: Option<bool>,
    concurrency: Option<usize>,
    retries: Option<usize>,
    command: Option<String>,
    extra_args: Vec<String>,
    config_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GameDataSyncConfigFile {
    enabled: Option<bool>,
    interval_seconds: Option<u64>,
    out: Option<String>,
    base_url: Option<String>,
    repair_dir: Option<String>,
    events: Vec<u32>,
    charts: Vec<String>,
    player_files: Vec<String>,
    all_event_details: Option<bool>,
    all_charts: Option<bool>,
    all_card_details: Option<bool>,
    concurrency: Option<usize>,
    retries: Option<usize>,
    command: Option<String>,
    extra_args: Vec<String>,
}

fn append_telemetry_event(
    path: &PathBuf,
    event: &TelemetryEvent<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, event)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn mongo_uri_from_env() -> Option<String> {
    env::var("BANGDREAM_OPTIMIZE_MONGODB_URI")
        .ok()
        .or_else(|| env::var("MONGODB_URI").ok())
}

fn bestdori_config_from_env() -> Option<BestdoriFilesystemConfig> {
    let root = non_empty_env("BANGDREAM_OPTIMIZE_BESTDORI_ROOT")
        .map(PathBuf::from)
        .or_else(game_data_root_for_calculation)?;
    let mut config = BestdoriFilesystemConfig::from_bestdori_api_root(root.clone());

    override_path(&mut config.cards_path, "BANGDREAM_OPTIMIZE_BESTDORI_CARDS");
    override_path(
        &mut config.characters_path,
        "BANGDREAM_OPTIMIZE_BESTDORI_CHARACTERS",
    );
    override_path(
        &mut config.skills_path,
        "BANGDREAM_OPTIMIZE_BESTDORI_SKILLS",
    );
    override_path(
        &mut config.area_items_path,
        "BANGDREAM_OPTIMIZE_BESTDORI_AREA_ITEMS",
    );
    override_path(
        &mut config.events_path,
        "BANGDREAM_OPTIMIZE_BESTDORI_EVENTS",
    );
    override_path(&mut config.songs_path, "BANGDREAM_OPTIMIZE_BESTDORI_SONGS");
    override_optional_path(
        &mut config.score_range_chart_meta_path,
        "BANGDREAM_OPTIMIZE_BESTDORI_SCORE_RANGE_CHART_META",
    );
    override_path(
        &mut config.charts_dir,
        "BANGDREAM_OPTIMIZE_BESTDORI_CHARTS_DIR",
    );
    override_optional_path(
        &mut config.cards_dir,
        "BANGDREAM_OPTIMIZE_BESTDORI_CARDS_DIR",
    );
    override_optional_path(
        &mut config.event_details_dir,
        "BANGDREAM_OPTIMIZE_BESTDORI_EVENT_DETAILS_DIR",
    );
    override_optional_path(
        &mut config.cards_fix_path,
        "BANGDREAM_OPTIMIZE_BESTDORI_CARDS_FIX",
    );
    override_optional_path(
        &mut config.skills_fix_path,
        "BANGDREAM_OPTIMIZE_BESTDORI_SKILLS_FIX",
    );
    override_optional_path(
        &mut config.area_items_fix_path,
        "BANGDREAM_OPTIMIZE_BESTDORI_AREA_ITEMS_FIX",
    );
    override_optional_path(
        &mut config.event_character_parameter_bonus_fix_path,
        "BANGDREAM_OPTIMIZE_BESTDORI_EVENT_CHARACTER_PARAMETER_BONUS_FIX",
    );

    Some(config)
}

fn maximizer_from_env() -> Result<Option<Arc<dyn MaximizeInputBuilder>>, String> {
    if let Some(config) = bestdori_static_mirror_config_from_env() {
        return Ok(Some(Arc::new(
            BestdoriCachedFilesystemCalculator::new(config).map_err(|err| err.to_string())?,
        ) as Arc<dyn MaximizeInputBuilder>));
    }

    match bestdori_config_from_env() {
        Some(config) => Ok(Some(Arc::new(
            BestdoriFilesystemCalculator::load(config).map_err(|err| err.to_string())?,
        ) as Arc<dyn MaximizeInputBuilder>)),
        None => Ok(None),
    }
}

fn score_range_searcher_from_env() -> Result<Option<Arc<dyn ScoreRangeInputBuilder>>, String> {
    if let Some(config) = bestdori_static_mirror_config_from_env() {
        return Ok(Some(Arc::new(
            BestdoriCachedFilesystemCalculator::new(config).map_err(|err| err.to_string())?,
        ) as Arc<dyn ScoreRangeInputBuilder>));
    }

    match bestdori_config_from_env() {
        Some(config) => Ok(Some(Arc::new(
            BestdoriFilesystemCalculator::load(config).map_err(|err| err.to_string())?,
        ) as Arc<dyn ScoreRangeInputBuilder>)),
        None => Ok(None),
    }
}

fn pt_maximize_searcher_from_env() -> Result<Option<Arc<dyn PtMaximizeInputBuilder>>, String> {
    if let Some(config) = bestdori_static_mirror_config_from_env() {
        return Ok(Some(Arc::new(
            BestdoriCachedFilesystemCalculator::new(config).map_err(|err| err.to_string())?,
        ) as Arc<dyn PtMaximizeInputBuilder>));
    }

    match bestdori_config_from_env() {
        Some(config) => Ok(Some(Arc::new(
            BestdoriFilesystemCalculator::load(config).map_err(|err| err.to_string())?,
        ) as Arc<dyn PtMaximizeInputBuilder>)),
        None => Ok(None),
    }
}

fn bestdori_static_mirror_config_from_env() -> Option<BestdoriStaticMirrorConfig> {
    let base_url = env::var("BANGDREAM_OPTIMIZE_GAME_DATA_BASE_URL").ok()?;
    let cache_root = env::var("BANGDREAM_OPTIMIZE_GAME_DATA_CACHE_ROOT").ok()?;
    Some(BestdoriStaticMirrorConfig::new(cache_root, base_url))
}

fn web_root_from_env() -> Option<PathBuf> {
    non_empty_env("BANGDREAM_OPTIMIZE_WEB_ROOT").map(PathBuf::from)
}

fn game_data_static_root_from_env() -> Option<PathBuf> {
    non_empty_env("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT")
        .map(PathBuf::from)
        .or_else(default_game_data_root)
}

fn game_data_root_for_calculation() -> Option<PathBuf> {
    non_empty_env("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT")
        .map(PathBuf::from)
        .or_else(default_game_data_root)
        .filter(|path| path.exists())
}

fn default_game_data_root() -> Option<PathBuf> {
    let path = PathBuf::from("var/game-data");
    path.exists().then_some(path)
}

fn override_path(path: &mut PathBuf, env_key: &'static str) {
    if let Some(value) = non_empty_env(env_key) {
        *path = PathBuf::from(value);
    }
}

fn override_optional_path(path: &mut Option<PathBuf>, env_key: &'static str) {
    if let Some(value) = non_empty_env(env_key) {
        *path = Some(PathBuf::from(value));
    }
}

fn non_empty_env(env_key: &'static str) -> Option<String> {
    env::var(env_key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn env_bool(env_key: &'static str, default: bool) -> bool {
    non_empty_env(env_key)
        .map(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        })
        .unwrap_or(default)
}

fn game_data_sync_config_from_args() -> Result<Option<GameDataSyncRuntimeConfig>, String> {
    let cli = parse_game_data_sync_cli_args(env::args().skip(1))?;
    let file = load_game_data_sync_config_file(cli.config_path.as_ref())?;
    let has_sync_scope_from_file = file.as_ref().is_some_and(file_has_sync_scope);
    let sync_scope_from_args = cli.has_sync_scope();
    let sync_scope_values_from_args = cli.has_sync_scope_values();
    let mut interval_seconds = cli
        .interval
        .or_else(|| file.as_ref().and_then(|config| config.interval_seconds))
        .or_else(|| env_u64("BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_INTERVAL_SECONDS"));

    let file_enabled = file.as_ref().and_then(|config| config.enabled);
    let mut enabled = cli
        .enabled
        .or(file_enabled)
        .unwrap_or_else(|| env_bool("BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED", false));
    if !enabled && sync_scope_from_args {
        enabled = true;
    }
    if !enabled
        && file_enabled.is_none()
        && (has_sync_scope_from_file || interval_seconds.is_some())
    {
        enabled = true;
    }
    if !enabled {
        return Ok(None);
    }
    if interval_seconds.is_none() {
        interval_seconds = Some(DEFAULT_GAME_DATA_SYNC_INTERVAL_SECONDS);
    }

    let interval = interval_seconds
        .filter(|value| *value > 0)
        .map(Duration::from_secs);
    let out = cli
        .out
        .or_else(|| {
            file.as_ref()
                .and_then(|config| config.out.as_ref().map(PathBuf::from))
        })
        .or_else(|| non_empty_env("BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_OUT").map(PathBuf::from))
        .or_else(|| non_empty_env("BANGDREAM_OPTIMIZE_GAME_DATA_CACHE_ROOT").map(PathBuf::from))
        .or_else(|| non_empty_env("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("var/game-data"));
    let base_url = cli
        .base_url
        .or_else(|| file.as_ref().and_then(|config| config.base_url.clone()))
        .or_else(|| non_empty_env("BANGDREAM_OPTIMIZE_BESTDORI_BASE_URL"))
        .unwrap_or_else(|| "https://bestdori.com".to_owned());
    let command = cli
        .command
        .or_else(|| file.as_ref().and_then(|config| config.command.clone()))
        .or_else(|| non_empty_env("BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_COMMAND"));

    let repair_dir = cli
        .repair_dir
        .or_else(|| {
            file.as_ref()
                .and_then(|config| config.repair_dir.as_ref().map(PathBuf::from))
        })
        .or_else(|| non_empty_env("BANGDREAM_OPTIMIZE_REPAIR_DIR").map(PathBuf::from))
        .filter(|path| path.exists());

    let mut events = file
        .as_ref()
        .map(|config| config.events.clone())
        .unwrap_or_default();
    events.extend(cli.events);
    events.sort_unstable();
    events.dedup();

    let mut charts = file
        .as_ref()
        .map(|config| {
            config
                .charts
                .iter()
                .map(|chart| parse_chart(chart))
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();
    charts.extend(cli.charts);
    charts.sort_unstable();
    charts.dedup();

    let mut player_files = file
        .as_ref()
        .map(|config| {
            config
                .player_files
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    player_files.extend(cli.player_files);
    player_files.sort_unstable();
    player_files.dedup();

    let all_event_details = cli
        .all_event_details
        .or_else(|| file.as_ref().and_then(|config| config.all_event_details));
    let all_charts = cli
        .all_charts
        .or_else(|| file.as_ref().and_then(|config| config.all_charts));
    let all_card_details = cli
        .all_card_details
        .or_else(|| file.as_ref().and_then(|config| config.all_card_details));
    let sync_scope_for_defaults =
        sync_scope_values_from_args || has_sync_scope_values_from_file(file.as_ref());
    let all_event_details = if sync_scope_for_defaults {
        all_event_details
    } else {
        Some(all_event_details.unwrap_or(true))
    };
    let all_charts = if sync_scope_for_defaults {
        all_charts
    } else {
        Some(all_charts.unwrap_or(true))
    };
    let all_card_details = if sync_scope_for_defaults {
        all_card_details
    } else {
        Some(all_card_details.unwrap_or(true))
    };
    let concurrency = cli
        .concurrency
        .or_else(|| file.as_ref().and_then(|config| config.concurrency));
    let retries = cli
        .retries
        .or_else(|| file.as_ref().and_then(|config| config.retries));
    if let Some(value) = concurrency {
        if value == 0 {
            return Err(
                "BANGDREAM_OPTIMIZE_GAME_DATA_SYNC concurrency must be greater than 0".to_owned(),
            );
        }
    }
    if let Some(value) = retries {
        if value == 0 {
            return Err(
                "BANGDREAM_OPTIMIZE_GAME_DATA_SYNC retries must be greater than 0".to_owned(),
            );
        }
    }

    let mut extra_args = file.map_or_else(Vec::new, |config| config.extra_args);
    extra_args.extend(cli.extra_args);

    let args = build_game_data_sync_command_args(
        &out,
        &base_url,
        repair_dir.as_deref(),
        &events,
        &charts,
        &player_files,
        all_event_details,
        all_charts,
        all_card_details,
        concurrency,
        retries,
        &extra_args,
    )?;

    Ok(Some(GameDataSyncRuntimeConfig {
        args,
        interval,
        command,
    }))
}

fn load_game_data_sync_config_file(
    path: Option<&PathBuf>,
) -> Result<Option<GameDataSyncConfigFile>, String> {
    let path = match path {
        Some(path) => path.clone(),
        None => {
            let Some(path) = non_empty_env("BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_CONFIG") else {
                return Ok(None);
            };
            PathBuf::from(path)
        }
    };

    let contents = fs::read_to_string(&path).map_err(|error| {
        format!(
            "failed to read game-data sync config file {}: {}",
            path.display(),
            error
        )
    })?;
    let config = serde_json::from_str::<GameDataSyncConfigFile>(&contents).map_err(|error| {
        format!(
            "failed to parse game-data sync config file {}: {}",
            path.display(),
            error
        )
    })?;

    Ok(Some(config))
}

fn build_game_data_sync_command_args(
    out: &FsPath,
    base_url: &str,
    repair_dir: Option<&FsPath>,
    events: &[u32],
    charts: &[(u32, u8)],
    player_files: &[PathBuf],
    all_event_details: Option<bool>,
    all_charts: Option<bool>,
    all_card_details: Option<bool>,
    concurrency: Option<usize>,
    retries: Option<usize>,
    extra_args: &[String],
) -> Result<Vec<String>, String> {
    let mut args = vec![
        "--out".to_owned(),
        out.to_string_lossy().into_owned(),
        "--base-url".to_owned(),
        base_url.to_owned(),
    ];

    if let Some(repair_dir) = repair_dir {
        args.push("--repair-dir".to_owned());
        args.push(repair_dir.to_string_lossy().into_owned());
    }

    for event_id in events {
        args.push("--event".to_owned());
        args.push(event_id.to_string());
    }
    for (song_id, difficulty) in charts {
        args.push("--chart".to_owned());
        args.push(format!("{song_id}:{difficulty}"));
    }
    for player_file in player_files {
        args.push("--player".to_owned());
        args.push(player_file.to_string_lossy().into_owned());
    }
    if all_event_details == Some(true) {
        args.push("--all-event-details".to_owned());
    }
    if all_charts == Some(true) {
        args.push("--all-charts".to_owned());
    }
    if all_card_details == Some(true) {
        args.push("--all-card-details".to_owned());
    }
    if let Some(concurrency) = concurrency {
        args.push("--concurrency".to_owned());
        args.push(concurrency.to_string());
    }
    if let Some(retries) = retries {
        args.push("--retries".to_owned());
        args.push(retries.to_string());
    }
    for arg in extra_args {
        args.push(arg.clone());
    }

    if args.iter().any(|arg| arg.trim().is_empty()) {
        return Err("game-data sync command arguments must not be empty".to_owned());
    }

    Ok(args)
}

fn parse_game_data_sync_cli_args<I>(args: I) -> Result<GameDataSyncCliConfig, String>
where
    I: IntoIterator<Item = String>,
{
    let mut config = GameDataSyncCliConfig::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--game-data-sync" => config.enabled = Some(true),
            "--no-game-data-sync" => config.enabled = Some(false),
            "--game-data-sync-interval" | "--game-data-sync-interval-seconds" => {
                config.interval = Some(parse_u64_arg(
                    "--game-data-sync-interval",
                    &next_cli_arg(&mut args, "--game-data-sync-interval")?,
                )?);
            }
            "--game-data-sync-out" => {
                config.out = Some(PathBuf::from(next_cli_arg(
                    &mut args,
                    "--game-data-sync-out",
                )?));
            }
            "--game-data-sync-base-url" => {
                config.base_url = Some(next_cli_arg(&mut args, "--game-data-sync-base-url")?);
            }
            "--game-data-sync-repair-dir" => {
                config.repair_dir = Some(PathBuf::from(next_cli_arg(
                    &mut args,
                    "--game-data-sync-repair-dir",
                )?));
            }
            "--game-data-sync-event" => {
                config.events.push(parse_u32_arg(
                    "--game-data-sync-event",
                    &next_cli_arg(&mut args, "--game-data-sync-event")?,
                )?);
            }
            "--game-data-sync-events" => {
                let value = next_cli_arg(&mut args, "--game-data-sync-events")?;
                let items = split_csv(&value);
                for item in items {
                    config
                        .events
                        .push(parse_u32_arg("--game-data-sync-events", item)?);
                }
            }
            "--game-data-sync-chart" => {
                config.charts.push(parse_chart(&next_cli_arg(
                    &mut args,
                    "--game-data-sync-chart",
                )?)?);
            }
            "--game-data-sync-charts" => {
                let value = next_cli_arg(&mut args, "--game-data-sync-charts")?;
                let items = split_csv(&value);
                for item in items {
                    config.charts.push(parse_chart(item)?);
                }
            }
            "--game-data-sync-player" | "--game-data-sync-player-file" => {
                config.player_files.push(PathBuf::from(next_cli_arg(
                    &mut args,
                    "--game-data-sync-player",
                )?));
            }
            "--game-data-sync-all-event-details" => config.all_event_details = Some(true),
            "--no-game-data-sync-all-event-details" => config.all_event_details = Some(false),
            "--game-data-sync-all-charts" => config.all_charts = Some(true),
            "--no-game-data-sync-all-charts" => config.all_charts = Some(false),
            "--game-data-sync-all-card-details" => config.all_card_details = Some(true),
            "--no-game-data-sync-all-card-details" => config.all_card_details = Some(false),
            "--game-data-sync-concurrency" => {
                config.concurrency = Some(parse_usize_arg(
                    "--game-data-sync-concurrency",
                    &next_cli_arg(&mut args, "--game-data-sync-concurrency")?,
                )?);
            }
            "--game-data-sync-retries" => {
                config.retries = Some(parse_usize_arg(
                    "--game-data-sync-retries",
                    &next_cli_arg(&mut args, "--game-data-sync-retries")?,
                )?);
            }
            "--game-data-sync-extra-arg" => {
                config
                    .extra_args
                    .push(next_cli_arg(&mut args, "--game-data-sync-extra-arg")?);
            }
            "--game-data-sync-extra-args" => {
                let value = next_cli_arg(&mut args, "--game-data-sync-extra-args")?;
                let parsed = serde_json::from_str::<Vec<String>>(&value).map_err(|err| {
                    format!("--game-data-sync-extra-args must be a JSON array: {err}")
                })?;
                config.extra_args.extend(parsed);
            }
            "--game-data-sync-config" => {
                config.config_path = Some(PathBuf::from(next_cli_arg(
                    &mut args,
                    "--game-data-sync-config",
                )?));
            }
            "--game-data-sync-command" => {
                config.command = Some(next_cli_arg(&mut args, "--game-data-sync-command")?);
            }
            "--help" => {
                print_game_data_sync_help();
                std::process::exit(0);
            }
            arg if arg.starts_with("--game-data-sync") => {
                return Err(format!("unknown game-data sync argument: {arg}"));
            }
            _ => {}
        }
    }

    Ok(config)
}

fn next_cli_arg(
    args: &mut impl Iterator<Item = String>,
    flag: &'static str,
) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_u32_arg(flag: &'static str, value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("{flag} expects an integer value, got {value}"))
}

fn parse_u64_arg(flag: &'static str, value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{flag} expects an integer value, got {value}"))
}

fn parse_usize_arg(flag: &'static str, value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("{flag} expects an integer value, got {value}"))
}

fn parse_chart(raw: &str) -> Result<(u32, u8), String> {
    let (song, difficulty) = raw
        .split_once(':')
        .ok_or_else(|| format!("chart must be in songId:difficulty format, got {raw}"))?;
    Ok((
        parse_u32_arg("--game-data-sync-chart", song)?,
        parse_chart_difficulty(difficulty)?,
    ))
}

fn parse_chart_difficulty(value: &str) -> Result<u8, String> {
    match value {
        "0" | "easy" => Ok(0),
        "1" | "normal" => Ok(1),
        "2" | "hard" => Ok(2),
        "3" | "expert" => Ok(3),
        "4" | "special" => Ok(4),
        _ => Err(format!("invalid chart difficulty: {value}")),
    }
}

fn split_csv(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn env_u64(env_key: &'static str) -> Option<u64> {
    non_empty_env(env_key).and_then(|value| value.parse::<u64>().ok())
}

fn print_game_data_sync_help() {
    println!(
        "Usage: bangdream-optimize-server [sync options]\n\n\
Options:\n\
  --game-data-sync                     Enable game-data sync at startup\n\
  --no-game-data-sync                  Disable game-data sync\n\
  --game-data-sync-interval <sec>      Periodic sync interval in seconds\n\
  --game-data-sync-out <dir>           Target directory for synced game-data\n\
  --game-data-sync-base-url <url>      Bestdori base URL\n\
  --game-data-sync-repair-dir <dir>    Repair JSON directory\n\
  --game-data-sync-config <file>       JSON config file for remaining sync options\n\
  --game-data-sync-command <path>      Use this sync command instead of auto-detection\n\
  --game-data-sync-event <id>          Sync one event detail, can repeat\n\
  --game-data-sync-events <ids>        Comma-separated event ids\n\
  --game-data-sync-chart <song:diff>   Sync one chart, can repeat\n\
  --game-data-sync-charts <items>      Comma-separated chart selections\n\
  --game-data-sync-player <file>        Read one player config file, can repeat\n\
  --game-data-sync-all-event-details    Sync all event details\n\
  --game-data-sync-all-charts           Sync all charts in songs.json\n\
  --game-data-sync-all-card-details     Sync all card details\n\
  --game-data-sync-concurrency <n>      Sync concurrency\n\
  --game-data-sync-retries <n>         Retry count\n\
  --game-data-sync-extra-arg <arg>     Additional sync-bestdori argument\n\
  --game-data-sync-extra-args <json>    JSON array of additional sync-bestdori arguments\n\
  --help                               Show this help text\n\
",
    );
}

fn start_game_data_sync_task(config: GameDataSyncRuntimeConfig) {
    if let Err(error) = sync_game_data_once(&config) {
        tracing::warn!(error = %error, "initial game-data sync failed");
    } else {
        tracing::info!("initial game-data sync completed");
    }

    let Some(interval) = config.interval else {
        return;
    };

    tracing::info!(
        interval_seconds = interval.as_secs(),
        "scheduling periodic game-data sync"
    );
    thread::spawn(move || loop {
        thread::sleep(interval);
        if let Err(error) = sync_game_data_once(&config) {
            tracing::warn!(error = %error, "periodic game-data sync failed");
        } else {
            tracing::info!("periodic game-data sync completed");
        }
    });
}

fn sync_game_data_once(config: &GameDataSyncRuntimeConfig) -> Result<(), String> {
    if let Some(command) = config.command.as_deref() {
        return run_sync_command_with_explicit_program(command, &config.args);
    }

    if let Some(local) = locate_local_sync_binary() {
        match run_sync_command(&local, &config.args, None) {
            Ok(()) => return Ok(()),
            Err(SyncCommandStatus::Failed(error)) => return Err(error),
            Err(SyncCommandStatus::NotFound) => {}
        }
    }

    match run_sync_command("bangdream-optimize-sync-bestdori", &config.args, None) {
        Ok(()) => Ok(()),
        Err(SyncCommandStatus::NotFound) => run_sync_command_via_cargo(&config.args),
        Err(SyncCommandStatus::Failed(error)) => Err(error),
    }
}

enum SyncCommandStatus {
    NotFound,
    Failed(String),
}

fn run_sync_command(
    program: impl AsRef<std::ffi::OsStr>,
    args: &[String],
    cwd: Option<&FsPath>,
) -> Result<(), SyncCommandStatus> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let output = command.output().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            SyncCommandStatus::NotFound
        } else {
            SyncCommandStatus::Failed(format!("failed to run command: {error}"))
        }
    })?;

    if output.status.success() {
        return Ok(());
    }

    let status = output
        .status
        .code()
        .map_or_else(|| "unknown".to_owned(), |status| status.to_string());
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let mut details = format!("sync command exited with status {status}");
    if !stdout.is_empty() {
        details.push_str(&format!("; stdout: {stdout}"));
    }
    if !stderr.is_empty() {
        details.push_str(&format!("; stderr: {stderr}"));
    }
    Err(SyncCommandStatus::Failed(details))
}

fn run_sync_command_with_explicit_program(program: &str, args: &[String]) -> Result<(), String> {
    run_sync_command(program, args, None).map_err(|status| match status {
        SyncCommandStatus::NotFound => {
            format!("failed to run explicit game-data sync command {program}: executable not found")
        }
        SyncCommandStatus::Failed(error) => {
            format!("explicit game-data sync command {program} failed: {error}")
        }
    })
}

fn run_sync_command_via_cargo(args: &[String]) -> Result<(), String> {
    let Some(root) = locate_workspace_root() else {
        return Err(
            "game-data sync command not found and workspace root was not found for cargo fallback"
                .to_owned(),
        );
    };
    let mut command = Command::new("cargo");
    command.current_dir(root);
    command
        .arg("run")
        .arg("-p")
        .arg("bangdream-optimize-sync-bestdori")
        .arg("--bin")
        .arg("bangdream-optimize-sync-bestdori");

    if let Some(profile) = non_empty_env("BANGDREAM_OPTIMIZE_CARGO_PROFILE") {
        if profile == "release" {
            command.arg("--release");
        } else if profile != "dev" {
            command.arg("--profile").arg(profile);
        }
    }

    command.arg("--").args(args);
    let mut details = String::new();
    let output = command.output().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            format!(
                "cargo command not found while trying to run game-data sync fallback (cwd = {})",
                command.get_current_dir().map_or_else(
                    || "unknown".to_owned(),
                    |path| path.to_string_lossy().to_string()
                )
            )
        } else {
            format!("failed to run cargo for game-data sync: {error}")
        }
    })?;

    if output.status.success() {
        return Ok(());
    }

    let status = output
        .status
        .code()
        .map_or_else(|| "unknown".to_owned(), |status| status.to_string());
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !stdout.is_empty() {
        details.push_str(&format!("stdout: {stdout}\n"));
    }
    if !stderr.is_empty() {
        details.push_str(&format!("stderr: {stderr}\n"));
    }
    Err(format!(
        "game-data sync via cargo run failed (status {status}); {details}"
    ))
}

fn locate_local_sync_binary() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let binary_name = if cfg!(windows) {
        "bangdream-optimize-sync-bestdori.exe"
    } else {
        "bangdream-optimize-sync-bestdori"
    };
    let mut current = exe.parent()?;
    loop {
        let candidate = current.join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        current = current.parent()?;
    }
}

fn locate_workspace_root() -> Option<PathBuf> {
    let start = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|path| path.to_path_buf()))
        .or_else(|| env::current_dir().ok())?;

    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        if dir.join("Cargo.toml").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

impl GameDataSyncCliConfig {
    fn has_sync_scope(&self) -> bool {
        !self.events.is_empty()
            || !self.charts.is_empty()
            || !self.player_files.is_empty()
            || self.all_event_details.is_some()
            || self.all_charts.is_some()
            || self.all_card_details.is_some()
            || self.concurrency.is_some()
            || self.retries.is_some()
            || !self.extra_args.is_empty()
    }

    fn has_sync_scope_values(&self) -> bool {
        !self.events.is_empty()
            || !self.charts.is_empty()
            || !self.player_files.is_empty()
            || self.all_event_details.is_some()
            || self.all_charts.is_some()
            || self.all_card_details.is_some()
    }
}

fn file_has_sync_scope(config: &GameDataSyncConfigFile) -> bool {
    !config.events.is_empty()
        || !config.charts.is_empty()
        || !config.player_files.is_empty()
        || config.all_event_details.is_some()
        || config.all_charts.is_some()
        || config.all_card_details.is_some()
        || config.concurrency.is_some()
        || config.retries.is_some()
        || !config.extra_args.is_empty()
}

fn has_sync_scope_values_from_file(config: Option<&GameDataSyncConfigFile>) -> bool {
    let Some(config) = config else {
        return false;
    };
    !config.events.is_empty()
        || !config.charts.is_empty()
        || !config.player_files.is_empty()
        || config.all_event_details.is_some()
        || config.all_charts.is_some()
        || config.all_card_details.is_some()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiResponse<T> {
    status: &'static str,
    data: T,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiError {
    status: &'static str,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaximizeRequest {
    player_id: i64,
    server: Server,
    #[serde(default)]
    event_id: Option<u32>,
    #[serde(default)]
    options: ItemSearchOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScoreRangeApiRequest {
    player_id: i64,
    server: Server,
    #[serde(default)]
    event_id: Option<u32>,
    current_pt: u64,
    target_total_pt: u64,
    #[serde(default)]
    auto_base_multiplier: Option<f64>,
    #[serde(default)]
    mission_support_pt_bonus: Option<u64>,
    #[serde(default = "default_score_range_max_results")]
    max_results: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScoreRangeFromConfigApiRequest {
    player: PlayerConfig,
    server: Server,
    #[serde(default)]
    event_id: Option<u32>,
    current_pt: u64,
    target_total_pt: u64,
    #[serde(default)]
    auto_base_multiplier: Option<f64>,
    #[serde(default)]
    mission_support_pt_bonus: Option<u64>,
    #[serde(default = "default_score_range_max_results")]
    max_results: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PtMaximizeApiRequest {
    player_id: i64,
    server: Server,
    #[serde(default)]
    event_id: Option<u32>,
    request: PtMaximizeRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PtMaximizeFromConfigApiRequest {
    player: PlayerConfig,
    server: Server,
    #[serde(default)]
    event_id: Option<u32>,
    request: PtMaximizeRequest,
}

fn default_score_range_max_results() -> usize {
    20
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BangDreamUserDataImportRequest {
    user_id: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BangDreamUserDataImportResponse {
    player_id: i64,
    server: Server,
    card_list: BTreeMap<String, bangdream_optimize_core::PlayerCardConfig>,
    area_item: BTreeMap<String, bangdream_optimize_core::AreaItemConfig>,
    character_bouns: BTreeMap<String, bangdream_optimize_core::CharacterBonusConfig>,
}

impl From<PlayerConfig> for BangDreamUserDataImportResponse {
    fn from(player: PlayerConfig) -> Self {
        Self {
            player_id: player.player_id,
            server: Server::Cn,
            card_list: player.card_list,
            area_item: player.area_item,
            character_bouns: player.character_bouns,
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let game_data_sync =
        game_data_sync_config_from_args().expect("failed to initialize game-data sync settings");
    if let Some(config) = game_data_sync {
        start_game_data_sync_task(config);
    }

    let port = env::var("BANGDREAM_OPTIMIZE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3100);
    let host = env::var("BANGDREAM_OPTIMIZE_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let state = AppState::from_env()
        .await
        .expect("failed to initialize server state");

    let web_root = web_root_from_env();
    let game_data_root = game_data_static_root_from_env();
    let enable_calc_routes = env_bool("BANGDREAM_OPTIMIZE_ENABLE_CALC_ROUTES", true);
    let app = build_app(state, web_root, game_data_root, enable_calc_routes);

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .expect("failed to parse listen address");
    tracing::info!("bangdream-optimize server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind HTTP listener");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("HTTP server failed");
}

fn build_app(
    state: AppState,
    web_root: Option<PathBuf>,
    game_data_root: Option<PathBuf>,
    enable_maximize_routes: bool,
) -> Router {
    let mut app = Router::new().route("/health", get(health)).route(
        "/bestdori/player/{server}/{player_id}",
        get(bestdori_player),
    );
    app = app.route(
        "/api/feedback",
        post(submit_feedback).layer(DefaultBodyLimit::max(12 * 1024 * 1024)),
    );

    if state.bangdream_importer.is_some() {
        app = app.route(
            "/bangdream/user-data/import",
            post(bangdream_user_data_import),
        );
    }

    if enable_maximize_routes {
        app = app
            .route("/v1/maximize", post(maximize_result))
            .route("/v1/score-range", post(score_range_result))
            .route("/v1/score-range/from-config", post(score_range_from_config))
            .route("/v1/pt-maximize", post(pt_maximize_result))
            .route("/v1/pt-maximize/from-config", post(pt_maximize_from_config))
            .route("/v1/calc-result", post(maximize_result))
            .route(
                "/v1/maximize/from-candidates",
                post(maximize_from_candidates),
            )
            .route(
                "/v1/calc-result/from-candidates",
                post(maximize_from_candidates),
            );
    } else {
        tracing::info!("maximize routes are disabled");
    }

    let mut app = app.with_state(state);

    if let Some(root) = game_data_root {
        tracing::info!("serving /game-data from {}", root.display());
        app = app.nest_service("/game-data", ServeDir::new(root));
    }

    if let Some(root) = web_root {
        tracing::info!("serving web UI from {}", root.display());
        app = app.fallback_service(
            ServeDir::new(root.clone()).fallback(ServeFile::new(root.join("index.html"))),
        );
    }

    app.layer(CorsLayer::permissive())
}

async fn health() -> Json<ApiResponse<&'static str>> {
    Json(ApiResponse {
        status: "ok",
        data: "healthy",
    })
}

#[derive(Debug, Deserialize)]
struct BestdoriPlayerPath {
    server: String,
    player_id: u64,
}

#[derive(Debug, Deserialize)]
struct BestdoriPlayerQuery {
    #[serde(default = "default_bestdori_player_mode")]
    mode: u8,
}

fn default_bestdori_player_mode() -> u8 {
    3
}

async fn bestdori_player(
    Path(path): Path<BestdoriPlayerPath>,
    Query(query): Query<BestdoriPlayerQuery>,
) -> impl IntoResponse {
    tracing::info!(
        server = %path.server,
        player_id = path.player_id,
        mode = query.mode,
        "GET /bestdori/player"
    );
    if !matches!(path.server.as_str(), "jp" | "en" | "tw" | "cn" | "kr") {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                status: "error",
                message: format!("unsupported server: {}", path.server),
            }),
        )
            .into_response();
    }
    if query.mode > 3 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                status: "error",
                message: format!("unsupported Bestdori player mode: {}", query.mode),
            }),
        )
            .into_response();
    }

    match fetch_bestdori_player(path.server.clone(), path.player_id, query.mode).await {
        Ok(value) => {
            tracing::info!(
                server = %path.server,
                player_id = path.player_id,
                "bestdori player profile fetched"
            );
            (StatusCode::OK, Json(value)).into_response()
        }
        Err(message) => (
            StatusCode::BAD_GATEWAY,
            Json(ApiError {
                status: "error",
                message,
            }),
        )
            .into_response(),
    }
}

async fn fetch_bestdori_player(server: String, player_id: u64, mode: u8) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let url = format!("https://bestdori.com/api/player/{server}/{player_id}?mode={mode}");
        let response = reqwest::blocking::Client::new()
            .get(&url)
            .send()
            .map_err(|err| format!("Bestdori request failed: {err}"))?;
        let status = response.status();
        if !status.is_success() {
            tracing::warn!(url = %url, status = %status, "Bestdori request failed");
            return Err(format!("Bestdori returned HTTP {status}"));
        }
        response
            .json::<Value>()
            .map_err(|err| format!("Bestdori JSON parse failed: {err}"))
    })
    .await
    .map_err(|err| format!("Bestdori proxy task failed: {err}"))?
}

async fn bangdream_user_data_import(
    State(state): State<AppState>,
    Json(request): Json<BangDreamUserDataImportRequest>,
) -> impl IntoResponse {
    if request.user_id == 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                status: "error",
                message: "userId must be a positive integer".to_owned(),
            }),
        )
            .into_response();
    }

    let Some(importer) = state.bangdream_importer.clone() else {
        return service_unavailable("Bang Dream account import is not configured");
    };

    let user_id = request.user_id;
    match tokio::task::spawn_blocking(move || {
        importer.import_player_config(BangDreamImportRequest { user_id })
    })
    .await
    {
        Ok(Ok(player)) => (
            StatusCode::OK,
            Json(ApiResponse::<BangDreamUserDataImportResponse> {
                status: "ok",
                data: player.into(),
            }),
        )
            .into_response(),
        Ok(Err(err)) => bangdream_import_error_response(err),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(ApiError {
                status: "error",
                message: format!("Bang Dream import task failed: {err}"),
            }),
        )
            .into_response(),
    }
}

fn bangdream_import_error_response(err: BangDreamImportError) -> axum::response::Response {
    let login_required = matches!(&err, BangDreamImportError::HttpStatus { status: 405, .. });
    let status = match &err {
        BangDreamImportError::MissingPersistField(_)
        | BangDreamImportError::Crypto(_)
        | BangDreamImportError::Protobuf(_) => StatusCode::BAD_GATEWAY,
        BangDreamImportError::ReadPersist(_) | BangDreamImportError::ParsePersist(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        BangDreamImportError::Http(_)
        | BangDreamImportError::HttpStatus { .. }
        | BangDreamImportError::MissingHeader(_) => StatusCode::BAD_GATEWAY,
    };
    let message = if login_required {
        "请使用自己的设备登陆一次该账号后重试".to_owned()
    } else {
        err.to_string()
    };

    (
        status,
        Json(ApiError {
            status: "error",
            message,
        }),
    )
        .into_response()
}

async fn maximize_result(
    State(state): State<AppState>,
    Json(request): Json<MaximizeRequest>,
) -> impl IntoResponse {
    let Some(maximize_service) = state.maximize_service.as_ref() else {
        return service_unavailable("maximize service is not configured");
    };

    match maximize_service
        .maximize_for_player(
            request.player_id,
            request.server,
            request.event_id,
            request.options,
        )
        .await
    {
        Ok(result) => {
            state
                .telemetry
                .log_result("maximize", Some(request.server), request.event_id, &result);
            ok_response(result)
        }
        Err(err) => data_error_response(err),
    }
}

async fn score_range_result(
    State(state): State<AppState>,
    Json(request): Json<ScoreRangeApiRequest>,
) -> impl IntoResponse {
    let Some(service) = state.score_range_service.as_ref() else {
        return service_unavailable("score-range service is not configured");
    };
    let search = ScoreRangeRequest {
        // The data adapter replaces this placeholder with the selected event's actual type.
        event_type: EventType::Festival,
        current_pt: request.current_pt,
        target_total_pt: request.target_total_pt,
        auto_base_multiplier: request.auto_base_multiplier,
        mission_support_pt_bonus: request.mission_support_pt_bonus,
        max_results: request.max_results.clamp(1, 100),
    };
    match service
        .score_range_for_player(request.player_id, request.server, request.event_id, search)
        .await
    {
        Ok(result) => ok_response(result),
        Err(err) => data_error_response(err),
    }
}

async fn score_range_from_config(
    State(state): State<AppState>,
    Json(request): Json<ScoreRangeFromConfigApiRequest>,
) -> impl IntoResponse {
    let Some(searcher) = state.score_range_searcher.as_ref() else {
        return service_unavailable("score-range searcher is not configured");
    };
    let search = ScoreRangeRequest {
        // The data adapter replaces this placeholder with the selected event's actual type.
        event_type: EventType::Festival,
        current_pt: request.current_pt,
        target_total_pt: request.target_total_pt,
        auto_base_multiplier: request.auto_base_multiplier,
        mission_support_pt_bonus: request.mission_support_pt_bonus,
        max_results: request.max_results.clamp(1, 100),
    };
    match searcher
        .score_range(request.player, request.server, request.event_id, search)
        .await
    {
        Ok(result) => ok_response(result),
        Err(err) => data_error_response(err),
    }
}

async fn submit_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> impl IntoResponse {
    let (request, attachments) = match parse_feedback_multipart(multipart).await {
        Ok(parsed) => parsed,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    status: "error",
                    message,
                }),
            )
                .into_response();
        }
    };
    let feedback_id = next_feedback_id();
    if !request.website.trim().is_empty() {
        return ok_response(FeedbackReceipt { feedback_id });
    }
    let Some(sender) = state.feedback_sender.as_ref() else {
        return service_unavailable("feedback service is not configured");
    };
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok());
    let submission = match FeedbackSubmission::validate(request, feedback_id.clone(), user_agent) {
        Ok(submission) => submission,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    status: "error",
                    message,
                }),
            )
                .into_response();
        }
    };
    let client_key = feedback_client_key(&headers, state.trust_proxy_headers);
    if let Err(retry_after) = state.feedback_rate_limiter.check(client_key) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ApiError {
                status: "error",
                message: format!("反馈提交过于频繁，请在 {retry_after} 秒后重试"),
            }),
        )
            .into_response();
    }
    if let Err(err) = sender.send(&submission, &attachments).await {
        tracing::error!(
            feedback_id = %submission.feedback_id,
            error = %err,
            "failed to submit feedback email"
        );
        return (
            StatusCode::BAD_GATEWAY,
            Json(ApiError {
                status: "error",
                message: "反馈邮件发送失败，请稍后重试".to_owned(),
            }),
        )
            .into_response();
    }
    tracing::info!(
        feedback_id = %submission.feedback_id,
        category = %submission.category,
        "feedback email submitted"
    );
    ok_response(FeedbackReceipt { feedback_id })
}

async fn parse_feedback_multipart(
    mut multipart: Multipart,
) -> Result<(FeedbackRequest, Vec<FeedbackAttachment>), String> {
    let mut request = None;
    let mut attachments = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| format!("无法读取反馈表单：{err}"))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "payload" => {
                if request.is_some() {
                    return Err("反馈表单包含重复 payload".to_owned());
                }
                let text = field
                    .text()
                    .await
                    .map_err(|err| format!("无法读取反馈内容：{err}"))?;
                if text.len() > 64 * 1024 {
                    return Err("反馈表单内容过大".to_owned());
                }
                request = Some(
                    serde_json::from_str::<FeedbackRequest>(&text)
                        .map_err(|_| "反馈表单格式无效".to_owned())?,
                );
            }
            "attachment" => {
                if attachments.len() >= feedback::MAX_ATTACHMENT_COUNT {
                    return Err(format!(
                        "最多只能上传 {} 个附件",
                        feedback::MAX_ATTACHMENT_COUNT
                    ));
                }
                let file_name = field.file_name().map(str::to_owned);
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|err| format!("无法读取附件：{err}"))?;
                attachments.push(FeedbackAttachment::validate(
                    file_name.as_deref(),
                    bytes.to_vec(),
                )?);
            }
            _ => return Err(format!("不支持的反馈表单字段：{name}")),
        }
    }
    let request = request.ok_or_else(|| "反馈表单缺少 payload".to_owned())?;
    validate_attachment_set(&attachments)?;
    Ok((request, attachments))
}

fn next_feedback_id() -> String {
    let timestamp = unix_timestamp_ms();
    let sequence = FEEDBACK_SEQUENCE.fetch_add(1, Ordering::Relaxed) % 10_000;
    format!("F-{timestamp}-{sequence:04}")
}

fn feedback_client_key(headers: &HeaderMap, trust_proxy_headers: bool) -> String {
    if trust_proxy_headers {
        for header in ["x-real-ip", "x-forwarded-for"] {
            if let Some(value) = headers.get(header).and_then(|value| value.to_str().ok()) {
                if let Some(first) = value
                    .split(',')
                    .next()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                {
                    return first.chars().take(80).collect();
                }
            }
        }
    }
    "direct-client".to_owned()
}

async fn pt_maximize_result(
    State(state): State<AppState>,
    Json(request): Json<PtMaximizeApiRequest>,
) -> impl IntoResponse {
    let Some(service) = state.pt_maximize_service.as_ref() else {
        return service_unavailable("PT-maximize service is not configured");
    };
    match service
        .pt_maximize_for_player(
            request.player_id,
            request.server,
            request.event_id,
            request.request,
        )
        .await
    {
        Ok(result) => ok_response(result),
        Err(err) => data_error_response(err),
    }
}

async fn pt_maximize_from_config(
    State(state): State<AppState>,
    Json(request): Json<PtMaximizeFromConfigApiRequest>,
) -> impl IntoResponse {
    let Some(searcher) = state.pt_maximize_searcher.as_ref() else {
        return service_unavailable("PT-maximize searcher is not configured");
    };
    match searcher
        .pt_maximize(
            request.player,
            request.server,
            request.event_id,
            request.request,
        )
        .await
    {
        Ok(result) => ok_response(result),
        Err(err) => data_error_response(err),
    }
}

async fn maximize_from_candidates(
    State(state): State<AppState>,
    Json(request): Json<CandidateBuildRequest>,
) -> impl IntoResponse {
    let requested_event_id = Some(request.event_id);
    match calculate_from_candidates(request) {
        Ok(result) => {
            state
                .telemetry
                .log_result("maximizeFromCandidates", None, requested_event_id, &result);
            ok_response(result)
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                status: "error",
                message: err.to_string(),
            }),
        )
            .into_response(),
    }
}

fn ok_response<T: Serialize>(result: T) -> axum::response::Response {
    (
        StatusCode::OK,
        Json(ApiResponse::<T> {
            status: "ok",
            data: result,
        }),
    )
        .into_response()
}

fn service_unavailable(message: &'static str) -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiError {
            status: "error",
            message: message.to_owned(),
        }),
    )
        .into_response()
}

fn data_error_response(err: DataError) -> axum::response::Response {
    let status = match err {
        DataError::PlayerNotFound { .. }
        | DataError::MissingCurrentEvent
        | DataError::MissingEventSongs { .. }
        | DataError::MissingEntity { .. } => StatusCode::NOT_FOUND,
        DataError::InvalidField { .. } | DataError::MissingField { .. } => StatusCode::BAD_REQUEST,
        DataError::Storage { .. }
        | DataError::Io { .. }
        | DataError::Json { .. }
        | DataError::Http { .. }
        | DataError::HttpStatus { .. }
        | DataError::JsonString { .. }
        | DataError::NotImplemented => StatusCode::SERVICE_UNAVAILABLE,
        DataError::Chart(_)
        | DataError::Preparation(_)
        | DataError::Maximize(_)
        | DataError::ScoreRange(_)
        | DataError::PtMaximize(_) => StatusCode::BAD_REQUEST,
    };

    (
        status,
        Json(ApiError {
            status: "error",
            message: err.to_string(),
        }),
    )
        .into_response()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install terminate signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        body::{to_bytes, Body},
        http::{Method, Request},
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tower::ServiceExt;

    #[test]
    fn score_range_api_distinguishes_missing_mission_support_from_explicit_zero() {
        let missing: ScoreRangeApiRequest = serde_json::from_str(
            r#"{"playerId":1,"server":"jp","currentPt":0,"targetTotalPt":100}"#,
        )
        .unwrap();
        let explicit_zero: ScoreRangeApiRequest = serde_json::from_str(
            r#"{"playerId":1,"server":"jp","currentPt":0,"targetTotalPt":100,"missionSupportPtBonus":0}"#,
        )
        .unwrap();

        assert_eq!(missing.mission_support_pt_bonus, None);
        assert_eq!(explicit_zero.mission_support_pt_bonus, Some(0));
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "bangdream-optimize-server-test-{}-{nanos}",
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

    #[tokio::test]
    async fn serves_api_web_ui_and_game_data_from_same_router() {
        let fixture = TestDir::new();
        let web_root = fixture.path().join("web");
        let game_data_root = fixture.path().join("var/game-data");
        fs::create_dir_all(&game_data_root).expect("failed to create game-data directory");
        fs::create_dir_all(&web_root).expect("failed to create web directory");
        fs::write(
            web_root.join("index.html"),
            "<!doctype html><title>bangdream-optimize</title><main>web app</main>",
        )
        .expect("failed to write index.html");
        fs::write(game_data_root.join("manifest.json"), r#"{"files":[]}"#)
            .expect("failed to write manifest.json");

        let app = build_app(
            AppState::default(),
            Some(web_root),
            Some(game_data_root),
            true,
        );

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("failed to build health request"),
            )
            .await
            .expect("health request should complete");
        assert_eq!(response.status(), StatusCode::OK);
        assert_body_contains(response, "healthy").await;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/game-data/manifest.json")
                    .body(Body::empty())
                    .expect("failed to build manifest request"),
            )
            .await
            .expect("manifest request should complete");
        assert_eq!(response.status(), StatusCode::OK);
        assert_body_contains(response, r#""files":[]"#).await;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/client/route")
                    .body(Body::empty())
                    .expect("failed to build fallback request"),
            )
            .await
            .expect("fallback request should complete");
        assert_eq!(response.status(), StatusCode::OK);
        assert_body_contains(response, "web app").await;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/maximize")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"playerId":1,"server":"jp"}"#))
                    .expect("failed to build maximize request"),
            )
            .await
            .expect("maximize request should complete");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_body_contains(response, "maximize service is not configured").await;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/score-range")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"playerId":1,"server":"jp","currentPt":0,"targetTotalPt":100}"#,
                    ))
                    .expect("failed to build score-range request"),
            )
            .await
            .expect("score-range request should complete");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_body_contains(response, "score-range service is not configured").await;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/calc-result")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"playerId":1,"server":"jp"}"#))
                    .expect("failed to build calc-result request"),
            )
            .await
            .expect("calc-result request should complete");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_body_contains(response, "maximize service is not configured").await;
    }

    #[tokio::test]
    async fn can_disable_maximize_routes_for_bestdori_proxy_only_deployments() {
        let app = build_app(AppState::default(), None, None, false);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/calc-result")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"playerId":1,"server":"jp"}"#))
                    .expect("failed to build calc-result request"),
            )
            .await
            .expect("calc-result request should complete");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/calc-result/from-candidates")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("failed to build candidates request"),
            )
            .await
            .expect("from-candidates request should complete");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn bangdream_import_route_is_disabled_without_importer() {
        let app = build_app(AppState::default(), None, None, false);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/bangdream/user-data/import")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"userId":1008131441}"#))
                    .expect("failed to build Bang Dream import request"),
            )
            .await
            .expect("Bang Dream import request should complete");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn bangdream_import_rejects_bad_user_id_before_network() {
        let fixture = TestDir::new();
        let persist_path = fixture.path().join("persist.json");
        fs::write(&persist_path, "{}").expect("failed to write persist placeholder");
        let app = build_app(
            AppState {
                bangdream_importer: Some(
                    BangDreamAccountImporter::new(persist_path)
                        .expect("importer should accept persist path"),
                ),
                ..Default::default()
            },
            None,
            None,
            false,
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/bangdream/user-data/import")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"userId":0}"#))
                    .expect("failed to build Bang Dream import request"),
            )
            .await
            .expect("Bang Dream import request should complete");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_body_contains(response, "userId must be a positive integer").await;
    }

    #[tokio::test]
    async fn bangdream_import_maps_upstream_405_to_account_login_hint() {
        let response = bangdream_import_error_response(BangDreamImportError::HttpStatus {
            status: 405,
            context: "Unity login failed".to_owned(),
        });

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_body_contains(response, "请使用自己的设备登陆一次该账号后重试").await;
    }

    #[derive(Default)]
    struct RecordingFeedbackSender {
        submissions: Mutex<Vec<(String, Vec<String>)>>,
    }

    #[async_trait]
    impl FeedbackSender for RecordingFeedbackSender {
        async fn send(
            &self,
            submission: &FeedbackSubmission,
            attachments: &[FeedbackAttachment],
        ) -> Result<(), String> {
            self.submissions.lock().unwrap().push((
                submission.feedback_id.clone(),
                attachments
                    .iter()
                    .map(|attachment| attachment.file_name.clone())
                    .collect(),
            ));
            Ok(())
        }
    }

    #[tokio::test]
    async fn feedback_route_validates_and_sends_through_configured_sender() {
        let sender = Arc::new(RecordingFeedbackSender::default());
        let app = build_app(
            AppState {
                feedback_sender: Some(sender.clone()),
                feedback_rate_limiter: FeedbackRateLimiter::new(3, Duration::from_secs(60)),
                ..Default::default()
            },
            None,
            None,
            false,
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/feedback")
                    .header(
                        "content-type",
                        "multipart/form-data; boundary=feedback-boundary",
                    )
                    .header("user-agent", "feedback-test")
                    .body(Body::from(multipart_feedback_body(
                        "feedback-boundary",
                        r#"{
                            "category":"problem",
                            "subject":"测试反馈",
                            "content":"这里是反馈内容",
                            "contactEmail":"user@example.com",
                            "context":{"runtime":"browser","appVersion":"0.3.2","page":"activity"}
                        }"#,
                        Some(("diagnostic.json", r#"{"ok":true}"#)),
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_body_contains(response, r#""feedbackId":"F-"#).await;
        let submissions = sender.submissions.lock().unwrap();
        assert_eq!(submissions.len(), 1);
        assert_eq!(submissions[0].1, vec!["diagnostic.json"]);
    }

    #[tokio::test]
    async fn feedback_route_rejects_invalid_contact_without_sending() {
        let sender = Arc::new(RecordingFeedbackSender::default());
        let app = build_app(
            AppState {
                feedback_sender: Some(sender.clone()),
                ..Default::default()
            },
            None,
            None,
            false,
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/feedback")
                    .header(
                        "content-type",
                        "multipart/form-data; boundary=feedback-boundary",
                    )
                    .body(Body::from(multipart_feedback_body(
                        "feedback-boundary",
                        r#"{
                            "category":"problem",
                            "subject":"测试反馈",
                            "content":"这里是反馈内容",
                            "contactEmail":"invalid"
                        }"#,
                        None,
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(sender.submissions.lock().unwrap().len(), 0);
    }

    fn multipart_feedback_body(
        boundary: &str,
        payload: &str,
        attachment: Option<(&str, &str)>,
    ) -> String {
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"payload\"\r\n\r\n{payload}\r\n"
        );
        if let Some((file_name, content)) = attachment {
            body.push_str(&format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"{file_name}\"\r\nContent-Type: application/json\r\n\r\n{content}\r\n"
            ));
        }
        body.push_str(&format!("--{boundary}--\r\n"));
        body
    }

    #[tokio::test]
    async fn bestdori_player_proxy_uses_unversioned_route() {
        let app = build_app(AppState::default(), None, None, false);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/bestdori/player/bad/1")
                    .body(Body::empty())
                    .expect("failed to build bestdori request"),
            )
            .await
            .expect("bestdori request should complete");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_body_contains(response, "unsupported server").await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/bestdori/player/bad/1")
                    .body(Body::empty())
                    .expect("failed to build old bestdori request"),
            )
            .await
            .expect("old bestdori request should complete");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn appends_telemetry_jsonl() {
        let fixture = TestDir::new();
        let path = fixture.path().join("telemetry/internal.jsonl");
        let metrics = CalculationMetrics {
            card_count: 123,
            song_count: 3,
            ..Default::default()
        };

        append_telemetry_event(
            &path,
            &TelemetryEvent {
                schema_version: 1,
                timestamp_ms: 42,
                route: "calcResult",
                server: Some(Server::Jp),
                requested_event_id: Some(287),
                event_id: 287,
                event_type: EventType::Medley,
                song_count: 3,
                total_score: 1_000_000,
                total_stat: 500_000,
                solver: Some("avx2"),
                metrics: Some(&metrics),
            },
        )
        .expect("telemetry event should be written");

        let contents = fs::read_to_string(path).expect("telemetry file should be readable");
        assert!(contents.ends_with('\n'));
        assert!(contents.contains(r#""schemaVersion":1"#));
        assert!(contents.contains(r#""route":"calcResult""#));
        assert!(contents.contains(r#""cardCount":123"#));
    }

    #[test]
    fn parses_game_data_chart_token() {
        assert_eq!(
            parse_chart("232:expert").expect("chart should parse"),
            (232, 3)
        );
        assert_eq!(parse_chart("669:0").expect("chart should parse"), (669, 0));
        assert!(parse_chart("bad").is_err());
    }

    #[test]
    fn builds_game_data_sync_command_args_for_scoped_sync() {
        let args = build_game_data_sync_command_args(
            Path::new("var/game-data"),
            "https://bestdori.com",
            None,
            &[287],
            &[(232, 3), (86, 3)],
            &[],
            Some(true),
            Some(false),
            Some(false),
            None,
            Some(2),
            &["--foo".to_owned()],
        )
        .expect("should build command args");

        assert!(args.contains(&"--event".to_owned()));
        assert!(args.contains(&"287".to_owned()));
        assert!(args.contains(&"--chart".to_owned()));
        assert!(args.contains(&"232:3".to_owned()));
        assert!(args.contains(&"--all-event-details".to_owned()));
        assert!(args.contains(&"--retries".to_owned()));
        assert!(args.contains(&"2".to_owned()));
        assert_eq!(args.last(), Some(&"--foo".to_owned()));
    }

    #[test]
    fn parses_game_data_sync_cli_scope_flags() {
        let cli = parse_game_data_sync_cli_args([
            "--game-data-sync-event".to_owned(),
            "287".to_owned(),
            "--game-data-sync-chart".to_owned(),
            "232:expert".to_owned(),
            "--game-data-sync-retries".to_owned(),
            "3".to_owned(),
        ])
        .expect("parse cli");

        assert_eq!(cli.events, vec![287]);
        assert_eq!(cli.charts, vec![(232, 3)]);
        assert_eq!(cli.retries, Some(3));
        assert!(cli.has_sync_scope());
    }

    async fn assert_body_contains(response: axum::response::Response, expected: &str) {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("failed to collect response body");
        let body = String::from_utf8(body.to_vec()).expect("response body should be UTF-8");
        assert!(
            body.contains(expected),
            "expected response body to contain {expected:?}, got {body:?}"
        );
    }
}
