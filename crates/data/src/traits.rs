use crate::DataError;
use async_trait::async_trait;
use bangdream_optimize_core::{BuildResult, ItemSearchOptions, PlayerConfig, Server};

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
pub trait CalculationInputBuilder: Send + Sync {
    async fn calculate_result(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError>;
}
