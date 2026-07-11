use bangdream_optimize_storage_mongodb::MongoPlayerConfigStore;
use std::{env, error::Error, fs, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let uri =
        required_env("BANGDREAM_OPTIMIZE_MONGODB_URI").or_else(|_| required_env("MONGODB_URI"))?;
    let db_name = env::var("BANGDREAM_OPTIMIZE_MONGODB_DB")
        .or_else(|_| env::var("MONGODB_DB"))
        .unwrap_or_else(|_| "tsugu-bangdream-bot".to_owned());
    let output = PathBuf::from(required_env("BANGDREAM_OPTIMIZE_FIXTURE_OUTPUT")?);

    let store = MongoPlayerConfigStore::connect(&uri, &db_name).await?;
    let player_id = player_id_from_env()?.unwrap_or(
        store
            .sample_calculation_player_id()
            .await?
            .ok_or("no calculable player document was found")?,
    );
    let mut player = store
        .get(player_id)
        .await?
        .ok_or_else(|| format!("player {player_id} was not found"))?;

    let original_player_id = player.player_id;
    player.mongo_id = None;
    player.player_id = 1;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(&player)?;
    fs::write(&output, data)?;

    eprintln!(
        "wrote fixture {} from player {}: current_event={:?}, event_songs={}, cards={}, area_items={}",
        output.display(),
        original_player_id,
        player.current_event,
        player.event_songs.len(),
        player.card_list.len(),
        player.area_item.len()
    );

    Ok(())
}

fn required_env(key: &'static str) -> Result<String, Box<dyn Error>> {
    let value = env::var(key)?;
    if value.trim().is_empty() {
        return Err(format!("{key} must not be empty").into());
    }
    Ok(value)
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
