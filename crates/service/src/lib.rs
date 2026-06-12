use bangdream_optimize_core::{BuildResult, ItemSearchOptions, PlayerConfig, Server};
use bangdream_optimize_data::{CalculationInputBuilder, DataError, PlayerConfigStore};
use std::sync::Arc;

#[derive(Clone)]
pub struct OptimizerService {
    player_store: Arc<dyn PlayerConfigStore>,
    calculator: Arc<dyn CalculationInputBuilder>,
}

impl std::fmt::Debug for OptimizerService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("OptimizerService").finish()
    }
}

impl OptimizerService {
    pub fn new(
        player_store: Arc<dyn PlayerConfigStore>,
        calculator: Arc<dyn CalculationInputBuilder>,
    ) -> Self {
        Self {
            player_store,
            calculator,
        }
    }

    pub async fn load_player_config(
        &self,
        player_id: i64,
    ) -> Result<Option<PlayerConfig>, DataError> {
        self.player_store.get_player_config(player_id).await
    }

    pub async fn calculate_for_player(
        &self,
        player_id: i64,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        let player = self
            .load_player_config(player_id)
            .await?
            .ok_or(DataError::PlayerNotFound { player_id })?;
        self.calculate_for_config(player, server, event_id, options)
            .await
    }

    pub async fn calculate_for_config(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        self.calculator
            .calculate_result(player, server, event_id, options)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use bangdream_optimize_core::{EventType, SongBuildResult};
    use std::collections::BTreeMap;

    #[derive(Clone)]
    struct MemoryPlayerStore {
        player: Option<PlayerConfig>,
    }

    #[async_trait]
    impl PlayerConfigStore for MemoryPlayerStore {
        async fn get_player_config(
            &self,
            player_id: i64,
        ) -> Result<Option<PlayerConfig>, DataError> {
            Ok(self
                .player
                .clone()
                .filter(|player| player.player_id == player_id))
        }
    }

    struct StubCalculator;

    #[async_trait]
    impl CalculationInputBuilder for StubCalculator {
        async fn calculate_result(
            &self,
            player: PlayerConfig,
            _server: Server,
            event_id: Option<u32>,
            _options: ItemSearchOptions,
        ) -> Result<BuildResult, DataError> {
            Ok(BuildResult {
                event_id: event_id.or(player.current_event).unwrap_or_default(),
                event_type: EventType::Challenge,
                total_score: player.player_id as i32,
                total_stat: 100,
                songs: vec![SongBuildResult {
                    song_id: 1,
                    difficulty: 3,
                    score: 1000,
                    stat: 100,
                    team_card_ids: vec![1, 2, 3, 4, 5],
                    captain_card_id: 1,
                }],
                items: None,
                solver: None,
                metrics: None,
            })
        }
    }

    #[tokio::test]
    async fn calculates_for_loaded_player_config() {
        let service = OptimizerService::new(
            Arc::new(MemoryPlayerStore {
                player: Some(player(42)),
            }),
            Arc::new(StubCalculator),
        );

        let result = service
            .calculate_for_player(42, Server::Jp, None, ItemSearchOptions::default())
            .await
            .unwrap();

        assert_eq!(result.event_id, 100);
        assert_eq!(result.total_score, 42);
    }

    #[tokio::test]
    async fn reports_missing_player_config() {
        let service = OptimizerService::new(
            Arc::new(MemoryPlayerStore { player: None }),
            Arc::new(StubCalculator),
        );

        let err = service
            .calculate_for_player(404, Server::Jp, None, ItemSearchOptions::default())
            .await
            .unwrap_err();

        assert!(matches!(err, DataError::PlayerNotFound { player_id: 404 }));
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
