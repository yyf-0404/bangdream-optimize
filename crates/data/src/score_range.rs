//! Data preparation adapter for score-range searches.

use crate::{prepare_event_context, DataError, GameDataSnapshot, ScoreRangeInputBuilder};
use async_trait::async_trait;
use bangdream_optimize_core::{
    prepare_score_range_team_domain, score_range_item_combinations, search_score_range, EventType,
    PlayerConfig, ScoreRangeChartMeta, ScoreRangeRequest, ScoreRangeResult, ScoreRangeSong,
    ScoreRangeTeamDomain, Server, SongSelection,
};
use serde_json::Value;
use std::collections::BTreeMap;

// Bestdori represents permanently available songs with close timestamps in 2099-2100.
// Any earlier close timestamp is a real limited-availability deadline and excludes the song
// even before that deadline is reached.
const PERMANENT_CLOSE_SENTINEL_MIN_MILLIS: u64 = 4_070_908_800_000; // 2099-01-01 UTC

#[derive(Debug, Clone)]
pub struct PreparedScoreRangeInput {
    pub event_id: u32,
    pub event_type: EventType,
    pub teams: ScoreRangeTeamDomain,
    pub songs: Vec<ScoreRangeSong>,
}

#[derive(Debug, Clone)]
pub struct SnapshotScoreRangeInputBuilder {
    data: GameDataSnapshot,
}

impl SnapshotScoreRangeInputBuilder {
    pub fn new(data: GameDataSnapshot) -> Self {
        Self { data }
    }

    pub fn score_range_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        mut request: ScoreRangeRequest,
    ) -> Result<Vec<ScoreRangeResult>, DataError> {
        let songs = self.data.score_range_chart_selections();
        let auto_base_multiplier =
            requested_auto_base_multiplier(request.auto_base_multiplier, server)?;
        let input =
            prepare_score_range_input(&self.data, &player, event_id, &songs, auto_base_multiplier)?;
        request.event_type = input.event_type;
        search_score_range(&request, &input.teams, &input.songs).map_err(Into::into)
    }
}

#[async_trait]
impl ScoreRangeInputBuilder for SnapshotScoreRangeInputBuilder {
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

pub fn prepare_score_range_input(
    data: &GameDataSnapshot,
    player: &PlayerConfig,
    event_id: Option<u32>,
    published_songs: &[SongSelection],
    auto_base_multiplier: f64,
) -> Result<PreparedScoreRangeInput, DataError> {
    let context = prepare_event_context(data, player, event_id)?;
    let cards = context.score_range_cards();
    let items = score_range_item_combinations(&context.area_item_percent);
    let teams = prepare_score_range_team_domain(
        cards,
        &context.area_item_percent,
        &items,
        &context.point_bonus_micros,
    );
    let mut songs = Vec::with_capacity(published_songs.len());
    for selection in published_songs {
        let (level, meta) = if let Some((level, meta)) =
            data.score_range_chart(selection.song_id, selection.difficulty)
        {
            (level, meta.clone())
        } else {
            let chart = data
                .chart(selection.song_id, selection.difficulty)
                .cloned()
                .ok_or(DataError::MissingEntity {
                    kind: "score-range chart meta",
                    id: format!("{}:{}", selection.song_id, selection.difficulty),
                })?;
            (chart.level, ScoreRangeChartMeta::from_chart(chart)?)
        };
        if !meta.is_searchable() {
            continue;
        }
        songs.push(ScoreRangeSong::from_meta_with_base_multiplier(
            selection.clone(),
            level,
            meta,
            auto_base_multiplier,
        ));
    }

    Ok(PreparedScoreRangeInput {
        event_id: context.event_id,
        event_type: context.event_type,
        teams,
        songs,
    })
}

fn requested_auto_base_multiplier(
    requested: Option<f64>,
    server: Server,
) -> Result<f64, DataError> {
    let multiplier =
        requested.unwrap_or_else(|| bangdream_optimize_core::auto_base_multiplier(server));
    if [0.5, 0.75].contains(&multiplier) {
        return Ok(multiplier);
    }
    Err(DataError::InvalidField {
        field: "autoBaseMultiplier",
        value: multiplier.to_string(),
    })
}

pub fn is_score_range_song_available(
    published_at: Option<u64>,
    closed_at: Option<u64>,
    now_millis: u64,
) -> bool {
    published_at.is_some_and(|published_at| published_at <= now_millis)
        && closed_at.is_none_or(|closed_at| closed_at >= PERMANENT_CLOSE_SENTINEL_MIN_MILLIS)
}

pub fn published_score_range_song_selections(
    songs: &BTreeMap<String, Value>,
    server: Server,
    now_millis: u64,
    mut has_chart_meta: impl FnMut(u32, u8) -> bool,
) -> Result<Vec<SongSelection>, DataError> {
    let server_index = match server {
        Server::Jp => 0,
        Server::En => 1,
        Server::Tw => 2,
        Server::Cn => 3,
        Server::Kr => 4,
    };
    let mut result = Vec::new();
    for (song_id, song) in songs {
        let song_id = song_id
            .parse::<u32>()
            .map_err(|_| DataError::InvalidField {
                field: "songs.songId",
                value: song_id.clone(),
            })?;
        let published_at = song
            .get("publishedAt")
            .and_then(Value::as_array)
            .and_then(|values| values.get(server_index))
            .and_then(value_as_u64);
        let closed_at = song
            .get("closedAt")
            .and_then(Value::as_array)
            .and_then(|values| values.get(server_index))
            .and_then(value_as_u64);
        if !is_score_range_song_available(published_at, closed_at, now_millis) {
            continue;
        }
        let Some(difficulties) = song.get("difficulty").and_then(Value::as_object) else {
            continue;
        };
        for (difficulty, definition) in difficulties {
            let difficulty = difficulty
                .parse::<u8>()
                .map_err(|_| DataError::InvalidField {
                    field: "songs.difficulty",
                    value: difficulty.clone(),
                })?;
            if !difficulty_is_published(difficulty, definition, server_index, now_millis)
                || !has_chart_meta(song_id, difficulty)
            {
                continue;
            }
            result.push(SongSelection {
                song_id,
                difficulty,
            });
        }
    }
    result.sort_by_key(|song| (song.song_id, song.difficulty));
    Ok(result)
}

fn value_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

pub(crate) fn difficulty_is_published(
    difficulty: u8,
    definition: &Value,
    server_index: usize,
    now_millis: u64,
) -> bool {
    if difficulty != 4 || definition.get("publishedAt").is_none() {
        return true;
    }
    definition
        .get("publishedAt")
        .and_then(Value::as_array)
        .and_then(|values| values.get(server_index))
        .and_then(value_as_u64)
        .is_some_and(|published_at| published_at <= now_millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_must_be_published_and_not_closed_on_the_server() {
        assert!(is_score_range_song_available(Some(100), None, 200));
        assert!(!is_score_range_song_available(Some(100), Some(300), 200));
        assert!(is_score_range_song_available(
            Some(100),
            Some(PERMANENT_CLOSE_SENTINEL_MIN_MILLIS),
            200
        ));
        assert!(!is_score_range_song_available(Some(300), None, 200));
        assert!(!is_score_range_song_available(Some(100), Some(200), 200));
        assert!(!is_score_range_song_available(None, None, 200));
    }
}
