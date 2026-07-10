use bangdream_optimize_core::{PlayerConfig, ScoreRangeRequest, ScoreRangeResult, Server};
use bangdream_optimize_data::{DataError, PlayerConfigStore, ScoreRangeInputBuilder};
use std::sync::Arc;

#[derive(Clone)]
pub struct ScoreRangeService {
    player_store: Arc<dyn PlayerConfigStore>,
    searcher: Arc<dyn ScoreRangeInputBuilder>,
}

impl std::fmt::Debug for ScoreRangeService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ScoreRangeService").finish()
    }
}

impl ScoreRangeService {
    pub fn new(
        player_store: Arc<dyn PlayerConfigStore>,
        searcher: Arc<dyn ScoreRangeInputBuilder>,
    ) -> Self {
        Self {
            player_store,
            searcher,
        }
    }

    pub async fn score_range_for_player(
        &self,
        player_id: i64,
        server: Server,
        event_id: Option<u32>,
        request: ScoreRangeRequest,
    ) -> Result<Vec<ScoreRangeResult>, DataError> {
        let player = self
            .player_store
            .get_player_config(player_id)
            .await?
            .ok_or(DataError::PlayerNotFound { player_id })?;
        self.score_range_for_config(player, server, event_id, request)
            .await
    }

    pub async fn score_range_for_config(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: ScoreRangeRequest,
    ) -> Result<Vec<ScoreRangeResult>, DataError> {
        self.searcher
            .score_range(player, server, event_id, request)
            .await
    }
}
