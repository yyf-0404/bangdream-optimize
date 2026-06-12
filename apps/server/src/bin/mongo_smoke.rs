use bangdream_optimize_core::{BuildResult, ItemSearchOptions, Server, SongSelection};
use bangdream_optimize_data::{
    BestdoriCachedFilesystemCalculationInputBuilder, BestdoriFilesystemCalculationInputBuilder,
    BestdoriFilesystemConfig, BestdoriStaticMirrorConfig, CalculationInputBuilder,
};
use bangdream_optimize_storage_mongodb::MongoPlayerConfigStore;
use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
    time::Instant,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let uri =
        required_env("BANGDREAM_OPTIMIZE_MONGODB_URI").or_else(|_| required_env("MONGODB_URI"))?;
    let db_name = env::var("BANGDREAM_OPTIMIZE_MONGODB_DB")
        .or_else(|_| env::var("MONGODB_DB"))
        .unwrap_or_else(|_| "tsugu-bangdream-bot".to_owned());

    let store = MongoPlayerConfigStore::connect(&uri, &db_name).await?;
    let player_id = match player_id_from_env()? {
        Some(player_id) => player_id,
        None => match optional_i32("BANGDREAM_OPTIMIZE_MIN_CARD_COUNT")? {
            Some(min_cards) => {
                let players = store.large_calculation_players(min_cards, 10).await?;
                if players.is_empty() {
                    return Err(
                        format!("no calculable player has at least {min_cards} cards").into(),
                    );
                }
                for player in &players {
                    eprintln!(
                        "large player candidate: player={} current_event={:?} cards={} event_song_sets={}",
                        player.player_id,
                        player.current_event,
                        player.card_count,
                        player.event_song_count,
                    );
                }
                players[0].player_id
            }
            None => store
                .sample_calculation_player_id()
                .await?
                .ok_or("no calculable player document was found")?,
        },
    };
    let mut player = store
        .get(player_id)
        .await?
        .ok_or_else(|| format!("player {player_id} was not found"))?;

    let server = server_from_env()?;
    let event_id = optional_u32("BANGDREAM_OPTIMIZE_EVENT_ID")?;
    if let Some(songs) = event_songs_from_env()? {
        let target_event_id = event_id.or(player.current_event).ok_or(
            "BANGDREAM_OPTIMIZE_EVENT_SONGS requires BANGDREAM_OPTIMIZE_EVENT_ID or currentEvent",
        )?;
        let summary = songs
            .iter()
            .map(|song| format!("{}:{}", song.song_id, song.difficulty))
            .collect::<Vec<_>>()
            .join(",");
        eprintln!("overriding event {target_event_id} songs={summary}");
        player
            .event_songs
            .insert(target_event_id.to_string(), songs);
    }
    let calculator = calculator_from_env()?;

    eprintln!(
        "loaded player {player_id}: current_event={:?}, event_songs={}, cards={}, area_items={}",
        player.current_event,
        player.event_songs.len(),
        player.card_list.len(),
        player.area_item.len(),
    );
    if let Some(selected_event_id) = event_id.or(player.current_event) {
        if let Some(songs) = player.event_songs.get(&selected_event_id.to_string()) {
            let song_summary = songs
                .iter()
                .map(|song| format!("{}:{}", song.song_id, song.difficulty))
                .collect::<Vec<_>>()
                .join(",");
            eprintln!("selected event {selected_event_id} songs={song_summary}");
        }
    }

    let started = Instant::now();
    let result = calculator
        .calculate_result(player, server, event_id, ItemSearchOptions::default())
        .await?;
    print_result(&result, started.elapsed().as_millis());

    Ok(())
}

fn required_env(key: &'static str) -> Result<String, Box<dyn Error>> {
    let value = env::var(key)?;
    if value.trim().is_empty() {
        return Err(format!("{key} must not be empty").into());
    }
    Ok(value)
}

fn optional_u32(key: &'static str) -> Result<Option<u32>, Box<dyn Error>> {
    let Some(value) = env::var(key).ok().filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    value
        .parse::<u32>()
        .map(Some)
        .map_err(|err| format!("{key} must be a u32: {err}").into())
}

fn optional_i32(key: &'static str) -> Result<Option<i32>, Box<dyn Error>> {
    let Some(value) = env::var(key).ok().filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    value
        .parse::<i32>()
        .map(Some)
        .map_err(|err| format!("{key} must be an i32: {err}").into())
}

fn event_songs_from_env() -> Result<Option<Vec<SongSelection>>, Box<dyn Error>> {
    let Some(value) = env::var("BANGDREAM_OPTIMIZE_EVENT_SONGS")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };

    let mut songs = Vec::new();
    for item in value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let (song_id, difficulty) = item
            .split_once(':')
            .ok_or_else(|| format!("invalid BANGDREAM_OPTIMIZE_EVENT_SONGS item: {item}"))?;
        songs.push(SongSelection {
            song_id: song_id
                .parse()
                .map_err(|err| format!("invalid song id in {item}: {err}"))?,
            difficulty: difficulty
                .parse()
                .map_err(|err| format!("invalid difficulty in {item}: {err}"))?,
        });
    }

    Ok(Some(songs))
}

fn player_id_from_env() -> Result<Option<i64>, Box<dyn Error>> {
    let Some(value) = env::var("BANGDREAM_OPTIMIZE_PLAYER_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    value
        .parse::<i64>()
        .map(Some)
        .map_err(|err| format!("BANGDREAM_OPTIMIZE_PLAYER_ID must be an i64: {err}").into())
}

fn server_from_env() -> Result<Server, Box<dyn Error>> {
    let value = env::var("BANGDREAM_OPTIMIZE_SERVER").unwrap_or_else(|_| "jp".to_owned());
    match value.trim().to_ascii_lowercase().as_str() {
        "jp" => Ok(Server::Jp),
        "en" => Ok(Server::En),
        "tw" => Ok(Server::Tw),
        "cn" => Ok(Server::Cn),
        "kr" => Ok(Server::Kr),
        value => Err(format!("unsupported BANGDREAM_OPTIMIZE_SERVER: {value}").into()),
    }
}

fn calculator_from_env() -> Result<Box<dyn CalculationInputBuilder>, Box<dyn Error>> {
    if let Some(base_url) = env::var("BANGDREAM_OPTIMIZE_GAME_DATA_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let cache_root = required_env("BANGDREAM_OPTIMIZE_GAME_DATA_CACHE_ROOT")?;
        return Ok(Box::new(
            BestdoriCachedFilesystemCalculationInputBuilder::new(BestdoriStaticMirrorConfig::new(
                cache_root, base_url,
            ))?,
        ));
    }

    let root = env::var("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("var/game-data"));
    if !Path::new(&root).exists() {
        return Err(format!(
            "game-data root does not exist: {}; set BANGDREAM_OPTIMIZE_GAME_DATA_ROOT",
            root.display()
        )
        .into());
    }

    Ok(Box::new(BestdoriFilesystemCalculationInputBuilder::load(
        BestdoriFilesystemConfig::from_root(root),
    )?))
}

fn print_result(result: &BuildResult, elapsed_ms: u128) {
    println!(
        "event={} type={:?} solver={} total_score={} total_stat={} elapsed_ms={elapsed_ms}",
        result.event_id,
        result.event_type,
        result.solver.as_deref().unwrap_or("unknown"),
        result.total_score,
        result.total_stat,
    );
    for (idx, song) in result.songs.iter().enumerate() {
        println!(
            "song#{idx}: song={} diff={} score={} stat={} captain={} cards={:?}",
            song.song_id,
            song.difficulty,
            song.score,
            song.stat,
            song.captain_card_id,
            song.team_card_ids,
        );
    }
    if let Some(items) = &result.items {
        println!(
            "items: band={} attribute={} magazine={:?}",
            items.band, items.attribute, items.magazine,
        );
    }
}
