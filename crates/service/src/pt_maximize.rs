use bangdream_optimize_core::{PlayerConfig, PtMaximizeRequest, PtMaximizeResult, Server};
use bangdream_optimize_data::{DataError, PlayerConfigStore, PtMaximizeInputBuilder};
use std::sync::Arc;

#[derive(Clone)]
pub struct PtMaximizeService {
    player_store: Arc<dyn PlayerConfigStore>,
    searcher: Arc<dyn PtMaximizeInputBuilder>,
}

impl std::fmt::Debug for PtMaximizeService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PtMaximizeService").finish()
    }
}

impl PtMaximizeService {
    pub fn new(
        player_store: Arc<dyn PlayerConfigStore>,
        searcher: Arc<dyn PtMaximizeInputBuilder>,
    ) -> Self {
        Self {
            player_store,
            searcher,
        }
    }

    pub async fn pt_maximize_for_player(
        &self,
        player_id: i64,
        server: Server,
        event_id: Option<u32>,
        request: PtMaximizeRequest,
    ) -> Result<PtMaximizeResult, DataError> {
        let player = self
            .player_store
            .get_player_config(player_id)
            .await?
            .ok_or(DataError::PlayerNotFound { player_id })?;
        self.pt_maximize_for_config(player, server, event_id, request)
            .await
    }

    pub async fn pt_maximize_for_config(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: PtMaximizeRequest,
    ) -> Result<PtMaximizeResult, DataError> {
        self.searcher
            .pt_maximize(player, server, event_id, request)
            .await
    }
}
