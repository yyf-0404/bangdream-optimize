//! Data preparation adapter for exact user-specified team PT evaluation.

use crate::{initialized_charts, prepare_event_context, DataError, PtMaximizeInputBuilder};
use bangdream_optimize_core::{
    evaluate_specified_teams_with_elapsed, event_bonus_application, LiveVariant, PlayerConfig,
    PtEvaluateMetrics, PtEvaluateRequest, PtEvaluateResult, PtEvaluateScoreMode, ScoreRule, Server,
};

impl super::pt_maximize::SnapshotPtMaximizeInputBuilder {
    pub fn pt_evaluate_sync(
        &self,
        player: PlayerConfig,
        _server: Server,
        event_id: Option<u32>,
        mut request: PtEvaluateRequest,
    ) -> Result<PtEvaluateResult, DataError> {
        let context = prepare_event_context(self.snapshot(), &player, event_id)?;
        request.event_type = context.event_type;
        request.validate_shape()?;
        let mut charts = match request.score_mode {
            PtEvaluateScoreMode::Manual => {
                initialized_charts(self.snapshot(), &request.songs, context.event_type)?
            }
            PtEvaluateScoreMode::Auto { base_multiplier } => request
                .songs
                .iter()
                .map(|song| {
                    let mut chart = self
                        .snapshot()
                        .chart(song.song_id, song.difficulty)
                        .cloned()
                        .ok_or(DataError::MissingEntity {
                            kind: "chart",
                            id: format!("{}:{}", song.song_id, song.difficulty),
                        })?;
                    chart.init_with_rule(
                        0,
                        request.live_variant == LiveVariant::Medley,
                        ScoreRule::auto_with_base_multiplier(base_multiplier),
                    )?;
                    Ok(chart)
                })
                .collect::<Result<Vec<_>, DataError>>()?,
        };
        // Manual initialization already marks Medley charts. Auto has no combo bonus,
        // so each chart can use its standalone template while exact scoring receives
        // the Medley flag below.
        if request.live_variant != LiveVariant::Medley {
            charts.truncate(1);
        }
        let application = event_bonus_application(context.event_type, request.live_variant);
        let cards = context.pt_maximize_cards(application);
        let (team, medley, total_elapsed_ms) = evaluate_specified_teams_with_elapsed(
            cards,
            &charts,
            &context.area_item_percent,
            context.pt_maximize_point_bonus_micros(application),
            &request,
        )?;
        let scenario = request.scenario_summary();
        Ok(PtEvaluateResult {
            event_id: context.event_id,
            event_type: context.event_type,
            live_variant: request.live_variant,
            songs: request.songs,
            scenario,
            score_mode: request.score_mode,
            metrics: PtEvaluateMetrics {
                core_version: env!("CARGO_PKG_VERSION").to_owned(),
                total_elapsed_ms,
            },
            team,
            medley,
        })
    }
}

#[allow(dead_code)]
async fn _assert_trait_object_is_compatible(
    builder: &dyn PtMaximizeInputBuilder,
    player: PlayerConfig,
    server: Server,
    event_id: Option<u32>,
    request: PtEvaluateRequest,
) -> Result<PtEvaluateResult, DataError> {
    builder.pt_evaluate(player, server, event_id, request).await
}
