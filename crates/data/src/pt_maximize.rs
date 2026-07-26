//! Data preparation adapter for average event-PT maximization.

use crate::initialized_charts;
use crate::{prepare_event_context, DataError, GameDataSnapshot, PtMaximizeInputBuilder};
use async_trait::async_trait;
use bangdream_optimize_core::{
    event_bonus_application, search_medley_with_metrics, search_single_song_with_metrics,
    LiveVariant, PlayerConfig, PtMaximizeError, PtMaximizeMetrics, PtMaximizeRequest,
    PtMaximizeResult, Server,
};

#[derive(Debug, Clone)]
pub struct SnapshotPtMaximizeInputBuilder {
    data: GameDataSnapshot,
}

impl SnapshotPtMaximizeInputBuilder {
    pub fn new(data: GameDataSnapshot) -> Self {
        Self { data }
    }

    pub fn pt_maximize_sync(
        &self,
        player: PlayerConfig,
        _server: Server,
        event_id: Option<u32>,
        mut request: PtMaximizeRequest,
    ) -> Result<PtMaximizeResult, DataError> {
        let context = prepare_event_context(&self.data, &player, event_id)?;
        request.event_type = context.event_type;
        let scenario_summary = request.scenario_summary();
        if request.live_variant == LiveVariant::Medley {
            request.search_scenario()?;
            if request.songs.len() != 3 {
                return Err(PtMaximizeError::InvalidMedleySongCount {
                    count: request.songs.len(),
                }
                .into());
            }
            let charts = initialized_charts(&self.data, &request.songs, context.event_type)?;
            let cards = context.pt_maximize_cards(event_bonus_application(
                context.event_type,
                request.live_variant,
            ));
            let card_count = cards.len();
            let (medley, search_metrics) =
                search_medley_with_metrics(cards, &charts, &context.area_item_percent)?;
            return Ok(PtMaximizeResult {
                event_id: context.event_id,
                event_type: context.event_type,
                live_variant: request.live_variant,
                songs: request.songs,
                scenario: scenario_summary,
                metrics: PtMaximizeMetrics::medley(card_count, search_metrics),
                team: None,
                medley: Some(medley),
            });
        }
        let scenario = request.search_scenario()?;
        if request.songs.len() != 1 {
            return Err(PtMaximizeError::InvalidSingleSongCount {
                count: request.songs.len(),
            }
            .into());
        }
        let selection = request.songs[0].clone();
        let mut chart = self
            .data
            .chart(selection.song_id, selection.difficulty)
            .cloned()
            .ok_or(DataError::MissingEntity {
                kind: "chart",
                id: format!("{}:{}", selection.song_id, selection.difficulty),
            })?;
        chart.init(0, false)?;

        let application = event_bonus_application(context.event_type, request.live_variant);
        let cards = context.pt_maximize_cards(application);
        let card_count = cards.len();
        let (team, search_metrics) = search_single_song_with_metrics(
            cards,
            &chart,
            &context.area_item_percent,
            context.pt_maximize_point_bonus_micros(application),
            request.minimum_personal_stat,
            scenario,
        )?;
        Ok(PtMaximizeResult {
            event_id: context.event_id,
            event_type: context.event_type,
            live_variant: request.live_variant,
            songs: request.songs,
            scenario: scenario_summary,
            metrics: PtMaximizeMetrics::single(card_count, search_metrics),
            team: Some(team),
            medley: None,
        })
    }
}

#[async_trait]
impl PtMaximizeInputBuilder for SnapshotPtMaximizeInputBuilder {
    async fn pt_maximize(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        request: PtMaximizeRequest,
    ) -> Result<PtMaximizeResult, DataError> {
        self.pt_maximize_sync(player, server, event_id, request)
    }
}
