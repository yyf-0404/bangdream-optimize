use bangdream_optimize_core::{
    PlayerConfig, ScoreRangeChartMeta, ScoreRangeChartMetaFile, SCORE_RANGE_CHART_META_PATH,
};
use bangdream_optimize_data::chart_from_bestdori;
use reqwest::{
    blocking::Client,
    header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED},
    StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

const DEFAULT_BASE_URL: &str = "https://bestdori.com";
const DEFAULT_OUT_DIR: &str = "var/game-data";
const DEFAULT_CONCURRENCY: usize = 8;
const DEFAULT_RETRIES: usize = 2;
const REPAIR_FILES: [&str; 4] = [
    "cardsCNfix.json",
    "skillsCNfix.json",
    "areaItemFix.json",
    "eventCharacterParameterBonusFix.json",
];

const MAIN_FILES: [(&str, &str); 6] = [
    ("api/cards/all.5.json", "/api/cards/all.5.json"),
    ("api/characters/main.3.json", "/api/characters/main.3.json"),
    ("api/skills/all.10.json", "/api/skills/all.10.json"),
    ("api/areaItems/main.5.json", "/api/areaItems/main.5.json"),
    ("api/events/all.6.json", "/api/events/all.6.json"),
    ("api/songs/all.7.json", "/api/songs/all.7.json"),
];

#[derive(Debug, Error)]
enum SyncError {
    #[error("argument error: {0}")]
    Args(String),

    #[error("http request failed for {url}: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("http request for {url} returned status {status}")]
    HttpStatus { url: String, status: u16 },

    #[error("file {path} could not be read: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("file {path} could not be written: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("file {path} contains invalid JSON: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("file {path} must contain a JSON object or array")]
    InvalidJsonShape { path: String },

    #[error("could not build score-range chart meta for {path}: {message}")]
    ScoreRangeChartMeta { path: String, message: String },

    #[error("sync worker panicked")]
    WorkerPanic,
}

impl SyncError {
    fn is_retryable(&self) -> bool {
        match self {
            SyncError::Http { .. } => true,
            SyncError::HttpStatus { status, .. } => {
                *status == 408 || *status == 429 || (500..=599).contains(status)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncOptions {
    base_url: String,
    out_dir: PathBuf,
    repair_dir: Option<PathBuf>,
    events: BTreeSet<u32>,
    charts: BTreeSet<ChartSelection>,
    player_files: Vec<PathBuf>,
    all_event_details: bool,
    all_charts: bool,
    all_card_details: bool,
    generate_score_range_meta_only: bool,
    concurrency: usize,
    retries: usize,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            out_dir: PathBuf::from(DEFAULT_OUT_DIR),
            repair_dir: None,
            events: BTreeSet::new(),
            charts: BTreeSet::new(),
            player_files: Vec::new(),
            all_event_details: false,
            all_charts: false,
            all_card_details: false,
            generate_score_range_meta_only: false,
            concurrency: DEFAULT_CONCURRENCY,
            retries: DEFAULT_RETRIES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ChartSelection {
    song_id: u32,
    difficulty: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteJob {
    path: String,
    url: String,
    check_updates: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: String,
    generated_at: String,
    files: BTreeMap<String, ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    hash: String,
    size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_modified: Option<String>,
}

#[derive(Debug)]
enum RemoteFetch {
    Updated {
        data: Vec<u8>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
    NotModified,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), SyncError> {
    let options = parse_args(env::args().skip(1))?;
    sync_bestdori(options)
}

fn sync_bestdori(mut options: SyncOptions) -> Result<(), SyncError> {
    fs::create_dir_all(&options.out_dir).map_err(|source| SyncError::Write {
        path: options.out_dir.display().to_string(),
        source,
    })?;

    if options.generate_score_range_meta_only {
        return generate_score_range_chart_meta_only(&options.out_dir);
    }

    let mut player_card_ids = BTreeSet::new();
    for player_file in &options.player_files {
        let player = read_player_config(player_file)?;
        if let Some(event_id) = player.current_event {
            options.events.insert(event_id);
        }
        for songs in player.event_songs.values() {
            for song in songs {
                options.charts.insert(ChartSelection {
                    song_id: song.song_id,
                    difficulty: song.difficulty,
                });
            }
        }
        for card_id in player.card_list.keys() {
            player_card_ids.insert(parse_u32_arg("player card id", card_id)?);
        }
    }

    let client = Client::builder()
        .user_agent("bangdream-optimize-sync-bestdori")
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|source| SyncError::Http {
            url: "client".to_owned(),
            source,
        })?;
    let previous_manifest = read_existing_manifest(&options.out_dir)?;
    let generated_at = unix_timestamp();
    let mut manifest = Manifest {
        version: generated_at.clone(),
        generated_at: generated_at.clone(),
        files: BTreeMap::new(),
    };

    manifest.files.extend(sync_remote_jobs(
        &client,
        &options.out_dir,
        &main_jobs(&options.base_url),
        previous_manifest.as_ref(),
        options.concurrency,
        options.retries,
    )?);

    sync_repair_files(
        &options.out_dir,
        options.repair_dir.as_deref(),
        &mut manifest,
    )?;

    if options.all_event_details {
        let events = read_json_value(&options.out_dir.join("api/events/all.6.json"))?;
        options.events.extend(event_ids_from_events_json(&events)?);
    }

    if options.all_charts {
        let songs = read_json_value(&options.out_dir.join("api/songs/all.7.json"))?;
        options
            .charts
            .extend(chart_selections_from_songs_json(&songs)?);
    }

    let mut detail_jobs = if options.all_card_details {
        let cards = read_json_value(&options.out_dir.join("api/cards/all.5.json"))?;
        card_detail_jobs(&options.base_url, &cards)?
    } else {
        card_detail_jobs_from_ids(&options.base_url, &player_card_ids)
    };
    detail_jobs.extend(event_jobs(&options.base_url, &options.events));
    detail_jobs.extend(chart_jobs(&options.base_url, &options.charts)?);
    manifest.files.extend(sync_remote_jobs(
        &client,
        &options.out_dir,
        &detail_jobs,
        previous_manifest.as_ref(),
        options.concurrency,
        options.retries,
    )?);

    if !options.charts.is_empty() {
        let (path, file) =
            generate_score_range_chart_meta(&options.out_dir, &options.charts, options.all_charts)?;
        manifest.files.insert(path, file);
    } else if options.out_dir.join(SCORE_RANGE_CHART_META_PATH).exists() {
        let data = read_bytes(&options.out_dir.join(SCORE_RANGE_CHART_META_PATH))?;
        manifest.files.insert(
            SCORE_RANGE_CHART_META_PATH.to_owned(),
            ManifestFile {
                hash: sha256_hex(&data),
                size: data.len(),
                source: None,
                etag: None,
                last_modified: None,
            },
        );
    }

    write_split_manifests(&options.out_dir, &manifest, &generated_at)?;

    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<SyncOptions, SyncError> {
    let mut options = SyncOptions::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--base-url" => options.base_url = next_value(&mut args, "--base-url")?,
            "--out" => options.out_dir = PathBuf::from(next_value(&mut args, "--out")?),
            "--repair-dir" => {
                options.repair_dir = Some(PathBuf::from(next_value(&mut args, "--repair-dir")?));
            }
            "--event" => {
                options.events.insert(parse_u32_arg(
                    "--event",
                    &next_value(&mut args, "--event")?,
                )?);
            }
            "--events" => {
                for event_id in split_csv(&next_value(&mut args, "--events")?) {
                    options.events.insert(parse_u32_arg("--events", event_id)?);
                }
            }
            "--chart" => {
                options
                    .charts
                    .insert(parse_chart(&next_value(&mut args, "--chart")?)?);
            }
            "--charts" => {
                for chart in split_csv(&next_value(&mut args, "--charts")?) {
                    options.charts.insert(parse_chart(chart)?);
                }
            }
            "--player" => {
                options
                    .player_files
                    .push(PathBuf::from(next_value(&mut args, "--player")?));
            }
            "--all-event-details" => options.all_event_details = true,
            "--all-charts" => options.all_charts = true,
            "--all-card-details" => options.all_card_details = true,
            "--generate-score-range-meta-only" => {
                options.generate_score_range_meta_only = true;
            }
            "--concurrency" => {
                options.concurrency =
                    parse_usize_arg("--concurrency", &next_value(&mut args, "--concurrency")?)?;
                if options.concurrency == 0 {
                    return Err(SyncError::Args(
                        "--concurrency must be greater than 0".to_owned(),
                    ));
                }
            }
            "--retries" => {
                options.retries =
                    parse_usize_arg("--retries", &next_value(&mut args, "--retries")?)?;
            }
            _ => return Err(SyncError::Args(format!("unknown argument: {arg}"))),
        }
    }

    Ok(options)
}

fn print_help() {
    println!(
        "Usage: bangdream-optimize-sync-bestdori [options]\n\
\n\
Options:\n\
  --out <dir>          Output directory, default var/game-data\n\
  --base-url <url>     Bestdori base URL, default https://bestdori.com\n\
  --repair-dir <dir>   Directory containing optional repair JSON files\n\
  --event <id>         Fetch one full event detail, can be repeated\n\
  --events <ids>       Comma-separated event ids\n\
  --chart <song:diff>  Fetch one chart, diff can be 0-4 or easy/special\n\
  --charts <items>     Comma-separated chart selections\n\
  --player <file>      Read PlayerConfig JSON and sync its event/charts\n\
  --all-event-details  Fetch full event detail for every event in events.json\n\
  --all-charts         Fetch every chart listed in songs.json\n\
  --all-card-details   Fetch every cards/{{id}}.json detail listed in cards.json\n\
  --generate-score-range-meta-only\n\
                       Rebuild score-range chart meta from existing local files\n\
  --concurrency <n>    Parallel remote downloads, default 8\n\
  --retries <n>        Retry transient remote errors, default 2\n\
"
    );
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &'static str,
) -> Result<String, SyncError> {
    args.next()
        .ok_or_else(|| SyncError::Args(format!("{flag} requires a value")))
}

fn split_csv(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

fn parse_u32_arg(flag: &'static str, value: &str) -> Result<u32, SyncError> {
    value
        .parse::<u32>()
        .map_err(|_| SyncError::Args(format!("{flag} expects an integer, got {value}")))
}

fn parse_usize_arg(flag: &'static str, value: &str) -> Result<usize, SyncError> {
    value
        .parse::<usize>()
        .map_err(|_| SyncError::Args(format!("{flag} expects an integer, got {value}")))
}

fn parse_chart(value: &str) -> Result<ChartSelection, SyncError> {
    let (song_id, difficulty) = value
        .split_once(':')
        .ok_or_else(|| SyncError::Args(format!("chart must be songId:difficulty, got {value}")))?;
    Ok(ChartSelection {
        song_id: parse_u32_arg("--chart", song_id)?,
        difficulty: parse_difficulty(difficulty)?,
    })
}

fn parse_difficulty(value: &str) -> Result<u8, SyncError> {
    match value {
        "0" | "easy" => Ok(0),
        "1" | "normal" => Ok(1),
        "2" | "hard" => Ok(2),
        "3" | "expert" => Ok(3),
        "4" | "special" => Ok(4),
        _ => Err(SyncError::Args(format!("invalid difficulty: {value}"))),
    }
}

fn difficulty_name(difficulty: u8) -> Result<&'static str, SyncError> {
    match difficulty {
        0 => Ok("easy"),
        1 => Ok("normal"),
        2 => Ok("hard"),
        3 => Ok("expert"),
        4 => Ok("special"),
        _ => Err(SyncError::Args(format!("invalid difficulty: {difficulty}"))),
    }
}

fn main_jobs(base_url: &str) -> Vec<RemoteJob> {
    MAIN_FILES
        .iter()
        .map(|(path, api_path)| RemoteJob {
            path: (*path).to_owned(),
            url: absolute_url(base_url, api_path),
            check_updates: true,
        })
        .collect()
}

fn event_jobs(base_url: &str, events: &BTreeSet<u32>) -> Vec<RemoteJob> {
    events
        .iter()
        .map(|event_id| RemoteJob {
            path: format!("api/events/{event_id}.json"),
            url: absolute_url(base_url, &format!("/api/events/{event_id}.json")),
            check_updates: false,
        })
        .collect()
}

fn card_detail_jobs(base_url: &str, cards: &Value) -> Result<Vec<RemoteJob>, SyncError> {
    Ok(card_detail_jobs_from_ids(
        base_url,
        &card_ids_from_cards_json(cards)?.into_iter().collect(),
    ))
}

fn card_detail_jobs_from_ids(base_url: &str, card_ids: &BTreeSet<u32>) -> Vec<RemoteJob> {
    card_ids
        .iter()
        .map(|card_id| RemoteJob {
            path: format!("api/cards/{card_id}.json"),
            url: absolute_url(base_url, &format!("/api/cards/{card_id}.json")),
            check_updates: false,
        })
        .collect()
}

fn card_ids_from_cards_json(cards: &Value) -> Result<Vec<u32>, SyncError> {
    let mut ids = Vec::new();
    for (card_id, _) in object_entries(cards, "cards.json")? {
        ids.push(parse_u32_arg("cards.json card id", &card_id)?);
    }
    Ok(ids)
}

fn chart_jobs(
    base_url: &str,
    charts: &BTreeSet<ChartSelection>,
) -> Result<Vec<RemoteJob>, SyncError> {
    charts
        .iter()
        .map(|chart| {
            let difficulty = difficulty_name(chart.difficulty)?;
            Ok(RemoteJob {
                path: format!("api/charts/{}/{difficulty}.json", chart.song_id),
                url: absolute_url(
                    base_url,
                    &format!("/api/charts/{}/{difficulty}.json", chart.song_id),
                ),
                check_updates: false,
            })
        })
        .collect()
}

fn event_ids_from_events_json(events: &Value) -> Result<BTreeSet<u32>, SyncError> {
    object_entries(events, "events.json")?
        .into_iter()
        .map(|(event_id, _)| parse_u32_arg("events.json event id", &event_id))
        .collect()
}

fn chart_selections_from_songs_json(songs: &Value) -> Result<BTreeSet<ChartSelection>, SyncError> {
    let mut charts = BTreeSet::new();
    for (song_id, song) in object_entries(songs, "songs.json")? {
        let song_id = parse_u32_arg("songs.json song id", &song_id)?;
        let Some(difficulty) = song.get("difficulty").and_then(Value::as_object) else {
            continue;
        };

        for difficulty_id in difficulty.keys() {
            charts.insert(ChartSelection {
                song_id,
                difficulty: parse_difficulty(difficulty_id)?,
            });
        }
    }
    Ok(charts)
}

fn generate_score_range_chart_meta(
    out_dir: &Path,
    charts: &BTreeSet<ChartSelection>,
    replace_existing: bool,
) -> Result<(String, ManifestFile), SyncError> {
    let output_path = out_dir.join(SCORE_RANGE_CHART_META_PATH);
    let mut output = if replace_existing || !output_path.exists() {
        ScoreRangeChartMetaFile::new()
    } else {
        let data = read_bytes(&output_path)?;
        serde_json::from_slice::<ScoreRangeChartMetaFile>(&data).map_err(|source| {
            SyncError::Json {
                path: output_path.display().to_string(),
                source,
            }
        })?
    };
    let songs_path = out_dir.join("api/songs/all.7.json");
    let songs = read_json_value(&songs_path)?;

    for chart in charts {
        let difficulty = difficulty_name(chart.difficulty)?;
        let relative_path = format!("api/charts/{}/{difficulty}.json", chart.song_id);
        let chart_path = out_dir.join(&relative_path);
        let chart_data = read_json_value(&chart_path)?;
        let level = song_level(&songs, chart.song_id, chart.difficulty)?;
        let chart_data = chart_from_bestdori(level, &chart_data).map_err(|error| {
            SyncError::ScoreRangeChartMeta {
                path: relative_path.clone(),
                message: error.to_string(),
            }
        })?;
        let meta = ScoreRangeChartMeta::from_chart(chart_data).map_err(|error| {
            SyncError::ScoreRangeChartMeta {
                path: relative_path,
                message: error.to_string(),
            }
        })?;
        if meta.is_searchable() {
            output.insert(chart.song_id, chart.difficulty, meta);
        } else {
            output.remove(chart.song_id, chart.difficulty);
        }
    }

    output
        .validate()
        .map_err(|message| SyncError::ScoreRangeChartMeta {
            path: SCORE_RANGE_CHART_META_PATH.to_owned(),
            message,
        })?;
    let data = serde_json::to_vec(&output).map_err(|source| SyncError::Json {
        path: SCORE_RANGE_CHART_META_PATH.to_owned(),
        source,
    })?;
    write_file(&output_path, &data)?;
    Ok((
        SCORE_RANGE_CHART_META_PATH.to_owned(),
        ManifestFile {
            hash: sha256_hex(&data),
            size: data.len(),
            source: None,
            etag: None,
            last_modified: None,
        },
    ))
}

fn generate_score_range_chart_meta_only(out_dir: &Path) -> Result<(), SyncError> {
    let songs = read_json_value(&out_dir.join("api/songs/all.7.json"))?;
    let charts = chart_selections_from_songs_json(&songs)?;
    let generated_at = unix_timestamp();
    let mut manifest = read_existing_manifest(out_dir)?.unwrap_or(Manifest {
        version: generated_at.clone(),
        generated_at: generated_at.clone(),
        files: BTreeMap::new(),
    });
    manifest.version = generated_at.clone();
    manifest.generated_at = generated_at.clone();
    let (path, file) = generate_score_range_chart_meta(out_dir, &charts, true)?;
    manifest.files.insert(path, file);
    write_split_manifests(out_dir, &manifest, &generated_at)
}

fn song_level(songs: &Value, song_id: u32, difficulty: u8) -> Result<i32, SyncError> {
    songs
        .get(song_id.to_string())
        .and_then(|song| song.get("difficulty"))
        .and_then(|difficulties| difficulties.get(difficulty.to_string()))
        .and_then(|difficulty| difficulty.get("playLevel"))
        .and_then(Value::as_i64)
        .map(|level| level as i32)
        .ok_or_else(|| SyncError::ScoreRangeChartMeta {
            path: "api/songs/all.7.json".to_owned(),
            message: format!("missing playLevel for {song_id}:{difficulty}"),
        })
}

fn object_entries<'a>(
    value: &'a Value,
    path: &'static str,
) -> Result<Vec<(String, &'a Value)>, SyncError> {
    if let Some(object) = value.as_object() {
        return Ok(object
            .iter()
            .map(|(key, value)| (key.clone(), value))
            .collect());
    }

    if let Some(array) = value.as_array() {
        return Ok(array
            .iter()
            .enumerate()
            .filter(|(_, value)| !value.is_null())
            .map(|(index, value)| (index.to_string(), value))
            .collect());
    }

    Err(SyncError::InvalidJsonShape {
        path: path.to_owned(),
    })
}

fn sync_remote_jobs(
    client: &Client,
    out_dir: &Path,
    jobs: &[RemoteJob],
    previous_manifest: Option<&Manifest>,
    concurrency: usize,
    retries: usize,
) -> Result<BTreeMap<String, ManifestFile>, SyncError> {
    let concurrency = concurrency.max(1);
    let mut files = BTreeMap::new();

    for chunk in jobs.chunks(concurrency) {
        let results = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(chunk.len());
            for job in chunk {
                let client = client.clone();
                let job = job.clone();
                handles.push(scope.spawn(move || {
                    sync_remote_job(&client, out_dir, &job, previous_manifest, retries)
                }));
            }

            handles
                .into_iter()
                .map(|handle| handle.join().map_err(|_| SyncError::WorkerPanic))
                .collect::<Result<Vec<_>, SyncError>>()
        })?;

        for result in results {
            let (path, file) = result?;
            files.insert(path, file);
        }
    }

    Ok(files)
}

fn sync_remote_job(
    client: &Client,
    out_dir: &Path,
    job: &RemoteJob,
    previous_manifest: Option<&Manifest>,
    retries: usize,
) -> Result<(String, ManifestFile), SyncError> {
    let output_path = out_dir.join(&job.path);
    let previous_file = previous_manifest.and_then(|manifest| manifest.files.get(&job.path));
    if !job.check_updates && output_path.exists() {
        return Ok((
            job.path.clone(),
            manifest_file_for_existing(&output_path, previous_file, &job.url)?,
        ));
    }

    let can_reuse = previous_file.is_some() && out_dir.join(&job.path).exists();

    let file = match fetch_remote_with_retries(client, &job.url, previous_file, can_reuse, retries)?
    {
        RemoteFetch::Updated {
            data,
            etag,
            last_modified,
        } => {
            write_file(&out_dir.join(&job.path), &data)?;
            ManifestFile {
                hash: sha256_hex(&data),
                size: data.len(),
                source: Some(job.url.clone()),
                etag,
                last_modified,
            }
        }
        RemoteFetch::NotModified => {
            let mut file = previous_file
                .cloned()
                .expect("304 reuse requires previous manifest file metadata");
            file.source = Some(job.url.clone());
            file
        }
    };

    Ok((job.path.clone(), file))
}

fn manifest_file_for_existing(
    path: &Path,
    previous_file: Option<&ManifestFile>,
    source: &str,
) -> Result<ManifestFile, SyncError> {
    let mut file = if let Some(previous_file) = previous_file {
        previous_file.clone()
    } else {
        let data = read_bytes(path)?;
        ManifestFile {
            hash: sha256_hex(&data),
            size: data.len(),
            source: None,
            etag: None,
            last_modified: None,
        }
    };

    file.source = Some(source.to_owned());
    Ok(file)
}

fn fetch_remote_with_retries(
    client: &Client,
    url: &str,
    previous_file: Option<&ManifestFile>,
    can_reuse: bool,
    retries: usize,
) -> Result<RemoteFetch, SyncError> {
    let mut attempt = 0;
    loop {
        match fetch_remote(client, url, previous_file, can_reuse) {
            Ok(result) => return Ok(result),
            Err(error) if attempt < retries && error.is_retryable() => {
                attempt += 1;
                std::thread::sleep(retry_delay(attempt));
            }
            Err(error) => return Err(error),
        }
    }
}

fn retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(250 * attempt.min(8) as u64)
}

fn fetch_remote(
    client: &Client,
    url: &str,
    previous_file: Option<&ManifestFile>,
    can_reuse: bool,
) -> Result<RemoteFetch, SyncError> {
    let mut request = client.get(url);
    if can_reuse {
        if let Some(etag) = previous_file.and_then(|file| file.etag.as_deref()) {
            request = request.header(IF_NONE_MATCH, etag);
        }
        if let Some(last_modified) = previous_file.and_then(|file| file.last_modified.as_deref()) {
            request = request.header(IF_MODIFIED_SINCE, last_modified);
        }
    }

    let response = request.send().map_err(|source| SyncError::Http {
        url: url.to_owned(),
        source,
    })?;
    let status = response.status();
    if status == StatusCode::NOT_MODIFIED && can_reuse {
        return Ok(RemoteFetch::NotModified);
    }
    if !status.is_success() {
        return Err(SyncError::HttpStatus {
            url: url.to_owned(),
            status: status.as_u16(),
        });
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let last_modified = response
        .headers()
        .get(LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let data = response
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|source| SyncError::Http {
            url: url.to_owned(),
            source,
        })?;
    Ok(RemoteFetch::Updated {
        data,
        etag,
        last_modified,
    })
}

fn write_output(
    out_dir: &Path,
    path: &str,
    data: &[u8],
    source: Option<String>,
    manifest: &mut Manifest,
) -> Result<(), SyncError> {
    let output_path = out_dir.join(path);
    write_file(&output_path, data)?;
    manifest.files.insert(
        path.to_owned(),
        ManifestFile {
            hash: sha256_hex(data),
            size: data.len(),
            source,
            etag: None,
            last_modified: None,
        },
    );
    Ok(())
}

fn sync_repair_files(
    out_dir: &Path,
    repair_dir: Option<&Path>,
    manifest: &mut Manifest,
) -> Result<(), SyncError> {
    for filename in REPAIR_FILES {
        let path = repair_dir
            .map(|repair_dir| repair_dir.join(filename))
            .unwrap_or_else(|| out_dir.join(filename));
        if path.exists() {
            let data = read_bytes(&path)?;
            write_output(
                out_dir,
                filename,
                &data,
                Some(path.display().to_string()),
                manifest,
            )?;
        }
    }
    Ok(())
}

fn write_split_manifests(
    out_dir: &Path,
    flat_manifest: &Manifest,
    generated_at: &str,
) -> Result<(), SyncError> {
    let mut directories: BTreeMap<String, BTreeMap<String, ManifestFile>> = BTreeMap::new();
    for (path, file) in &flat_manifest.files {
        let (dir, name) = split_manifest_path(path);
        directories
            .entry(dir)
            .or_default()
            .insert(name, file.clone());
        if is_core_api_file(path) {
            directories
                .entry(String::new())
                .or_default()
                .insert(path.clone(), file.clone());
        }
    }

    let mut directory_names = directories.keys().cloned().collect::<Vec<_>>();
    directory_names.sort();

    let mut written_manifests = BTreeSet::new();
    for dir in directory_names {
        let files = directories.remove(&dir).unwrap_or_default();
        let manifest = Manifest {
            version: generated_at.to_owned(),
            generated_at: generated_at.to_owned(),
            files,
        };
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|source| SyncError::Json {
                path: manifest_path_for_dir(&dir),
                source,
            })?;
        let manifest_path = manifest_path_for_dir(&dir);
        let output_path = out_dir.join(&manifest_path);
        write_file(&output_path, &manifest_bytes)?;
        written_manifests.insert(manifest_path);
    }

    if !written_manifests.contains("manifest.json") {
        let manifest = Manifest {
            version: generated_at.to_owned(),
            generated_at: generated_at.to_owned(),
            files: BTreeMap::new(),
        };
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|source| SyncError::Json {
                path: "manifest.json".to_owned(),
                source,
            })?;
        write_file(&out_dir.join("manifest.json"), &manifest_bytes)?;
        written_manifests.insert("manifest.json".to_owned());
    }

    remove_stale_manifests(out_dir, &written_manifests)?;

    Ok(())
}

fn split_manifest_path(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir.to_owned(), name.to_owned()),
        None => (String::new(), path.to_owned()),
    }
}

fn manifest_path_for_dir(dir: &str) -> String {
    if dir.is_empty() {
        "manifest.json".to_owned()
    } else {
        format!("{dir}/manifest.json")
    }
}

fn is_core_api_file(path: &str) -> bool {
    MAIN_FILES.iter().any(|(core_path, _)| *core_path == path)
}

fn remove_stale_manifests(out_dir: &Path, expected: &BTreeSet<String>) -> Result<(), SyncError> {
    for path in collect_manifest_paths(out_dir)? {
        let Some(relative_path) = relative_manifest_path(out_dir, &path) else {
            continue;
        };
        if !expected.contains(&relative_path) {
            fs::remove_file(&path).map_err(|source| SyncError::Write {
                path: path.display().to_string(),
                source,
            })?;
        }
    }
    Ok(())
}

fn relative_manifest_path(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(path_to_manifest_dir)
        .filter(|path| !path.is_empty())
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), SyncError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| SyncError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let temp_path = temp_output_path(path);
    fs::write(&temp_path, data).map_err(|source| SyncError::Write {
        path: temp_path.display().to_string(),
        source,
    })?;
    fs::rename(&temp_path, path).map_err(|source| SyncError::Write {
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

fn read_player_config(path: &Path) -> Result<PlayerConfig, SyncError> {
    let data = read_bytes(path)?;
    let value: Value = serde_json::from_slice(&data).map_err(|source| SyncError::Json {
        path: path.display().to_string(),
        source,
    })?;
    let player = value.get("player").cloned().unwrap_or(value);
    serde_json::from_value(player).map_err(|source| SyncError::Json {
        path: path.display().to_string(),
        source,
    })
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, SyncError> {
    fs::read(path).map_err(|source| SyncError::Read {
        path: path.display().to_string(),
        source,
    })
}

fn read_json_value(path: &Path) -> Result<Value, SyncError> {
    let data = read_bytes(path)?;
    serde_json::from_slice(&data).map_err(|source| SyncError::Json {
        path: path.display().to_string(),
        source,
    })
}

fn read_existing_manifest(out_dir: &Path) -> Result<Option<Manifest>, SyncError> {
    let manifest_paths = collect_manifest_paths(out_dir)?;
    if manifest_paths.is_empty() {
        return Ok(None);
    }

    let mut combined = Manifest {
        version: String::new(),
        generated_at: String::new(),
        files: BTreeMap::new(),
    };
    for path in manifest_paths {
        let data = read_bytes(&path)?;
        let manifest: Manifest =
            serde_json::from_slice(&data).map_err(|source| SyncError::Json {
                path: path.display().to_string(),
                source,
            })?;
        if combined.version.is_empty() {
            combined.version = manifest.version;
            combined.generated_at = manifest.generated_at;
        }
        let dir = path
            .parent()
            .and_then(|parent| parent.strip_prefix(out_dir).ok())
            .map(path_to_manifest_dir)
            .unwrap_or_default();
        for (file_path, file) in manifest.files {
            if file_path.ends_with("manifest.json") {
                continue;
            }
            let full_path = if dir.is_empty() || file_path.contains('/') {
                file_path
            } else {
                format!("{dir}/{file_path}")
            };
            combined.files.insert(full_path, file);
        }
    }

    Ok(Some(combined))
}

fn collect_manifest_paths(root: &Path) -> Result<Vec<PathBuf>, SyncError> {
    let mut paths = Vec::new();
    collect_manifest_paths_inner(root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_manifest_paths_inner(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), SyncError> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|source| SyncError::Read {
        path: dir.display().to_string(),
        source,
    })? {
        let entry = entry.map_err(|source| SyncError::Read {
            path: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_manifest_paths_inner(&path, paths)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("manifest.json") {
            paths.push(path);
        }
    }
    Ok(())
}

fn path_to_manifest_dir(path: &Path) -> String {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn absolute_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    let mut result = String::with_capacity("sha256:".len() + hash.len() * 2);
    result.push_str("sha256:");
    for byte in hash {
        result.push(nibble_to_hex(byte >> 4));
        result.push(nibble_to_hex(byte & 0x0f));
    }
    result
}

fn nibble_to_hex(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!("nibble is always <= 15"),
    }
}

fn unix_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chart_selection() {
        assert_eq!(
            parse_chart("123:expert").unwrap(),
            ChartSelection {
                song_id: 123,
                difficulty: 3
            }
        );
        assert_eq!(
            parse_chart("123:4").unwrap(),
            ChartSelection {
                song_id: 123,
                difficulty: 4
            }
        );
    }

    #[test]
    fn parses_cli_options() {
        let options = parse_args([
            "--out".to_owned(),
            "dist/game-data".to_owned(),
            "--events".to_owned(),
            "1,2".to_owned(),
            "--charts".to_owned(),
            "10:expert,11:special".to_owned(),
            "--player".to_owned(),
            "player.json".to_owned(),
            "--all-event-details".to_owned(),
            "--all-charts".to_owned(),
            "--all-card-details".to_owned(),
            "--concurrency".to_owned(),
            "16".to_owned(),
            "--retries".to_owned(),
            "4".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.out_dir, PathBuf::from("dist/game-data"));
        assert_eq!(options.events, BTreeSet::from([1, 2]));
        assert!(options.charts.contains(&ChartSelection {
            song_id: 10,
            difficulty: 3
        }));
        assert!(options.charts.contains(&ChartSelection {
            song_id: 11,
            difficulty: 4
        }));
        assert_eq!(options.player_files, vec![PathBuf::from("player.json")]);
        assert!(options.all_event_details);
        assert!(options.all_charts);
        assert!(options.all_card_details);
        assert_eq!(options.concurrency, 16);
        assert_eq!(options.retries, 4);
    }

    #[test]
    fn extracts_event_ids_from_object_or_array_json() {
        let object_events = serde_json::json!({
            "100": {"eventName": ["event"]},
            "101": {"eventName": ["event"]}
        });
        assert_eq!(
            event_ids_from_events_json(&object_events).unwrap(),
            BTreeSet::from([100, 101])
        );

        let array_events = serde_json::json!([
            null,
            {"eventName": ["event"]},
            {"eventName": ["event"]}
        ]);
        assert_eq!(
            event_ids_from_events_json(&array_events).unwrap(),
            BTreeSet::from([1, 2])
        );
    }

    #[test]
    fn extracts_chart_selections_from_songs_json() {
        let songs = serde_json::json!({
            "10": {
                "difficulty": {
                    "0": {"playLevel": 5},
                    "3": {"playLevel": 25}
                }
            },
            "11": {
                "difficulty": {
                    "special": {"playLevel": 26}
                }
            },
            "12": {}
        });

        assert_eq!(
            chart_selections_from_songs_json(&songs).unwrap(),
            BTreeSet::from([
                ChartSelection {
                    song_id: 10,
                    difficulty: 0
                },
                ChartSelection {
                    song_id: 10,
                    difficulty: 3
                },
                ChartSelection {
                    song_id: 11,
                    difficulty: 4
                }
            ])
        );
    }

    #[test]
    fn builds_remote_jobs() {
        assert_eq!(
            event_jobs("https://example.test", &BTreeSet::from([100])),
            vec![RemoteJob {
                path: "api/events/100.json".to_owned(),
                url: "https://example.test/api/events/100.json".to_owned(),
                check_updates: false,
            }]
        );

        assert_eq!(
            chart_jobs(
                "https://example.test",
                &BTreeSet::from([ChartSelection {
                    song_id: 1,
                    difficulty: 3
                }])
            )
            .unwrap(),
            vec![RemoteJob {
                path: "api/charts/1/expert.json".to_owned(),
                url: "https://example.test/api/charts/1/expert.json".to_owned(),
                check_updates: false,
            }]
        );
    }

    #[test]
    fn hashes_with_sha256_prefix() {
        assert_eq!(
            sha256_hex(b"hello"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn extracts_all_card_ids_from_cards_json() {
        let cards = serde_json::json!({
            "1": {
                "levelLimit": 3
            },
            "2": {
                "levelLimit": 2
            }
        });

        assert_eq!(card_ids_from_cards_json(&cards).unwrap(), vec![1, 2]);
    }

    #[test]
    fn builds_card_detail_jobs_for_all_cards() {
        let cards = serde_json::json!({
            "1": {
                "levelLimit": 3
            },
            "2": {
                "levelLimit": 2
            }
        });

        assert_eq!(
            card_detail_jobs("https://example.test", &cards).unwrap(),
            vec![
                RemoteJob {
                    path: "api/cards/1.json".to_owned(),
                    url: "https://example.test/api/cards/1.json".to_owned(),
                    check_updates: false,
                },
                RemoteJob {
                    path: "api/cards/2.json".to_owned(),
                    url: "https://example.test/api/cards/2.json".to_owned(),
                    check_updates: false,
                }
            ]
        );
    }

    #[test]
    fn deserializes_manifest_without_http_metadata() {
        let manifest: Manifest = serde_json::from_str(
            r#"{
                "version": "1",
                "generatedAt": "1",
                "files": {
                    "cards.json": {
                        "hash": "sha256:abc",
                        "size": 12,
                        "source": "https://example.test/cards.json"
                    }
                }
            }"#,
        )
        .unwrap();

        let file = &manifest.files["cards.json"];
        assert_eq!(file.etag, None);
        assert_eq!(file.last_modified, None);
    }

    #[test]
    fn serializes_manifest_http_metadata_as_camel_case() {
        let file = ManifestFile {
            hash: "sha256:abc".to_owned(),
            size: 12,
            source: Some("https://example.test/cards.json".to_owned()),
            etag: Some("\"abc\"".to_owned()),
            last_modified: Some("Tue, 02 Jun 2026 08:00:00 GMT".to_owned()),
        };

        let value = serde_json::to_value(file).unwrap();

        assert_eq!(value["etag"], "\"abc\"");
        assert_eq!(value["lastModified"], "Tue, 02 Jun 2026 08:00:00 GMT");
    }
}
