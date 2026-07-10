use super::mitm::search_raw_domain;
use super::{
    supports_event_type, total_fire_cost, ScoreRangePlay, ScoreRangePtError, ScoreRangeRequest,
    ScoreRangeResult, ScoreRangeSong, ScoreRangeTeam, ScoreRangeTeamDomain,
};
use crate::{ChartError, EventType};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScoreRangeError {
    #[error("event type {event_type:?} is not supported by score range")]
    UnsupportedEventType { event_type: EventType },
    #[error("target total PT {target_total_pt} is below current PT {current_pt}")]
    TargetBelowCurrent {
        current_pt: u64,
        target_total_pt: u64,
    },
    #[error("missionSupportPtBonus is required for mission_live score-range requests")]
    MissingMissionSupportPtBonus,
    #[error(transparent)]
    Chart(#[from] ChartError),
    #[error(transparent)]
    Point(#[from] ScoreRangePtError),
    #[error("failed to recover cards for score-range team at stat {stat}")]
    TeamRecovery { stat: i32 },
}

pub fn search_score_range(
    request: &ScoreRangeRequest,
    domain: &ScoreRangeTeamDomain,
    songs: &[ScoreRangeSong],
) -> Result<Vec<ScoreRangeResult>, ScoreRangeError> {
    if !supports_event_type(request.event_type) {
        return Err(ScoreRangeError::UnsupportedEventType {
            event_type: request.event_type,
        });
    }
    if request.event_type == EventType::MissionLive && request.mission_support_pt_bonus.is_none() {
        return Err(ScoreRangeError::MissingMissionSupportPtBonus);
    }
    let target_delta = request
        .target_total_pt
        .checked_sub(request.current_pt)
        .ok_or(ScoreRangeError::TargetBelowCurrent {
            current_pt: request.current_pt,
            target_total_pt: request.target_total_pt,
        })?;

    search_raw_domain(request, domain, songs, target_delta)?
        .into_iter()
        .map(|(team, plan)| {
            Ok(make_result(
                request,
                target_delta,
                &team,
                team.card_ids,
                plan,
            ))
        })
        .collect()
}

fn make_result(
    request: &ScoreRangeRequest,
    target_delta: u64,
    team: &ScoreRangeTeam,
    card_ids: [u32; 5],
    plays: Vec<ScoreRangePlay>,
) -> ScoreRangeResult {
    ScoreRangeResult {
        event_type: request.event_type,
        target_delta_pt: target_delta,
        team_card_ids: card_ids.to_vec(),
        total_stat: team.stat,
        point_bonus_basis_points: team.point_bonus_basis_points,
        items: team.items.clone(),
        play_count: plays.iter().map(|play| play.count).sum(),
        distinct_song_count: distinct_song_count(&plays),
        total_fire_cost: u32::try_from(total_fire_cost(&plays)).unwrap_or(u32::MAX),
        total_fire_multiplier: plays
            .iter()
            .map(|play| play.count * play.fire_multiplier)
            .sum(),
        plays,
    }
}

fn distinct_song_count(plan: &[ScoreRangePlay]) -> usize {
    plan.iter()
        .map(|play| (play.song_id, play.difficulty))
        .collect::<BTreeSet<_>>()
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{prepare_score_range_team_domain, AreaItemPercent};
    use std::collections::BTreeMap;

    #[test]
    fn mission_live_requires_explicit_support_bonus_even_when_zero() {
        let domain =
            prepare_score_range_team_domain(&[], &AreaItemPercent::empty(), &[], &BTreeMap::new());
        let mut request = ScoreRangeRequest {
            event_type: EventType::MissionLive,
            current_pt: 0,
            target_total_pt: 100,
            auto_base_multiplier: None,
            mission_support_pt_bonus: None,
            max_results: 20,
        };

        assert!(matches!(
            search_score_range(&request, &domain, &[]),
            Err(ScoreRangeError::MissingMissionSupportPtBonus)
        ));

        request.mission_support_pt_bonus = Some(0);
        assert_eq!(search_score_range(&request, &domain, &[]).unwrap(), vec![]);
    }
}
