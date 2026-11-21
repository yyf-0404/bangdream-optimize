use super::CalculationError;
use crate::medley::team::{adjusted_card_stats, ItemScoreBounds};
use crate::model::chart::Chart;
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::{
    BuildResult, CalculationMetrics, EventType, SelectedAreaItems, SingleCalculationMetrics,
    SongBuildResult, SongSelection,
};
use crate::single::{find_single_song, SingleSongCandidate, SingleSongError};
use crate::timing::Timer;
use crate::SongMode;

pub(super) fn calculate_single_result_for_items(
    event_id: u32,
    event_type: EventType,
    song: &SongSelection,
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    modes: &[SongMode],
    bounds: Option<&mut ItemScoreBounds<'_>>,
    minimum_score: i32,
) -> Result<BuildResult, CalculationError> {
    let solve_start = Timer::start();
    let mode_count = modes.len();
    let powers = adjusted_card_stats(cards, area_item_percent, selected_items);
    // A bound over the entire inventory can be unavailable for an imported
    // skill that the normal mode search would discard. Fall through in that case.
    if minimum_score > 0 && bounds.is_some_and(|bounds| {
        bounds.score_for_powers(&powers).is_ok_and(|upper| upper < minimum_score)
    }) {
        return Err(SingleSongError::NoResult.into());
    }
    let mut valid_mode_count = 0;
    let mut best: Option<SingleSongCandidate> = None;
    for &mode in modes {
        // Preserve ties across equipment. Within one item set the first mode
        // reaching the maximum wins, so subsequent modes need a higher score.
        let cutoff = best
            .as_ref()
            .map_or(minimum_score, |best| best.score.saturating_add(1));
        match find_single_song(cards, chart, &powers, mode, cutoff) {
            Ok(result) => {
                valid_mode_count += 1;
                if let Some(result) = result
                    .filter(|result| best.as_ref().is_none_or(|best| result.score > best.score))
                {
                    best = Some(result);
                }
            }
            Err(SingleSongError::NotEnoughCards { .. }) => {}
            Err(error) => return Err(error.into()),
        }
    }
    let best = best.ok_or(SingleSongError::NoResult)?;

    Ok(BuildResult {
        hit_timing: None,
        scoring_revision: Some(crate::scoring::ARITHMETIC_REVISION.to_owned()),
        skill_queue_notices: Vec::new(),
        event_id,
        event_type,
        total_score: best.score,
        total_stat: 0,
        songs: vec![single_song_result(song, best)],
        items: Some(selected_items.clone()),
        solver: Some("exact".to_owned()),
        metrics: Some(CalculationMetrics {
            single: Some(SingleCalculationMetrics {
                mode_count,
                valid_mode_count,
                solve_ms: solve_start.elapsed_ms(),
            }),
            ..Default::default()
        }),
    })
}

fn single_song_result(song: &SongSelection, result: SingleSongCandidate) -> SongBuildResult {
    SongBuildResult {
        team_order: None,
        song_id: song.song_id,
        difficulty: song.difficulty,
        score: result.score,
        stat: 0,
        team_card_ids: result.team_card_ids,
        captain_card_id: result.captain_card_id,
        skill_queue_risk: false,
    }
}
