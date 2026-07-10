use crate::DataError;
use async_trait::async_trait;
use bangdream_optimize_core::{
    BuildResult, MaximizeOptions, PlayerConfig, ScoreRangeRequest, ScoreRangeResult, Server,
};

#[async_trait]
pub trait PlayerConfigStore: Send + Sync {
    async fn get_player_config(&self, player_id: i64) -> Result<Option<PlayerConfig>, DataError>;
}

#[async_trait]
pub trait PlayerConfigRepository: PlayerConfigStore {
    async fn save_player_config(&self, player: PlayerConfig) -> Result<(), DataError>;

    async fn delete_player_config(&self, player_id: i64) -> Result<bool, DataError>;

    async fn list_player_ids(&self) -> Result<Vec<i64>, DataError>;
}

#[async_trait]
pub trait MaximizeInputBuilder: Send + Sync {
    async fn maximize(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: MaximizeOptions,
    ) -> Result<BuildResult, DataError>;

    async fn calculate_result(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: MaximizeOptions,
    ) -> Result<BuildResult, DataError> {
        self.maximize(player, server, event_id, options).await
    }
}

pub use MaximizeInputBuilder as CalculationInputBuilder;

#[async_trait]
pub trait ScoreRangeInputBuilder: Send + Sync {
    async fn score_range(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: ScoreRangeRequest,
    ) -> Result<Vec<ScoreRangeResult>, DataError>;
}
