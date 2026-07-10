use super::{mode_candidates, CalculationError};
use crate::model::chart::Chart;
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::{
    BuildResult, CalculationMetrics, EventType, SelectedAreaItems, SingleCalculationMetrics,
    SongBuildResult, SongSelection,
};
use crate::single::{calculate_single_song_dp, SingleSongDpError, SingleSongDpResult};
use crate::timing::Timer;

pub(super) fn calculate_single_result_for_items(
    event_id: u32,
    event_type: EventType,
    song: &SongSelection,
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
) -> Result<BuildResult, CalculationError> {
    let solve_start = Timer::start();
    let mut mode_count = 0;
    let mut valid_mode_count = 0;
    let best = mode_candidates(cards)
        .into_iter()
        .filter_map(|mode| {
            mode_count += 1;
            match calculate_single_song_dp(cards, chart, area_item_percent, selected_items, mode) {
                Ok(result) => {
                    valid_mode_count += 1;
                    Some(Ok(result))
                }
                Err(SingleSongDpError::NotEnoughCards { .. } | SingleSongDpError::NoResult) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .try_fold(None, |best: Option<SingleSongDpResult>, result| {
            let result = result?;
            Ok::<_, SingleSongDpError>(Some(match best {
                Some(best) if best.score >= result.score => best,
                _ => result,
            }))
        })?
        .ok_or(SingleSongDpError::NoResult)?;

    Ok(BuildResult {
        event_id,
        event_type,
        total_score: best.score,
        total_stat: best.stat,
        songs: vec![single_song_result(song, best)],
        items: Some(selected_items.clone()),
        solver: Some("dp".to_owned()),
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

fn single_song_result(song: &SongSelection, result: SingleSongDpResult) -> SongBuildResult {
    SongBuildResult {
        song_id: song.song_id,
        difficulty: song.difficulty,
        score: result.score,
        stat: result.stat,
        team_card_ids: result.team_card_ids,
        captain_card_id: result.captain_card_id,
    }
}
