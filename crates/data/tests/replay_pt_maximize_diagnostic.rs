use std::path::PathBuf;

use bangdream_optimize_core::{PlayerConfig, PtMaximizeRequest, Server, TeamCardSkill};
use bangdream_optimize_data::filesystem::{
    BestdoriFilesystemCalculationInputBuilder, BestdoriFilesystemConfig,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PtMaximizeDiagnostic {
    event_id: Option<u32>,
    player: PlayerConfig,
    server: Server,
    calculation_request: PtMaximizeRequest,
}

#[test]
#[ignore = "manual diagnostic replay; set BANGDREAM_OPTIMIZE_DIAGNOSTIC_FIXTURE"]
fn replays_pt_maximize_diagnostic() {
    let fixture_path = PathBuf::from(
        std::env::var("BANGDREAM_OPTIMIZE_DIAGNOSTIC_FIXTURE")
            .expect("BANGDREAM_OPTIMIZE_DIAGNOSTIC_FIXTURE must point to a diagnostic JSON"),
    );
    let diagnostic: PtMaximizeDiagnostic =
        serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
    let root = std::env::var("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../var/game-data"));
    let builder = BestdoriFilesystemCalculationInputBuilder::load(
        BestdoriFilesystemConfig::from_bestdori_api_root(&root),
    )
    .unwrap();

    let event_id = diagnostic
        .event_id
        .or(diagnostic.player.current_event)
        .expect("diagnostic has an event");
    let snapshot = builder
        .snapshot_for(
            &diagnostic.player,
            event_id,
            &diagnostic.calculation_request.songs,
            diagnostic.server,
        )
        .unwrap();
    let mut request = diagnostic.calculation_request.clone();
    if let Ok(value) = std::env::var("BANGDREAM_OPTIMIZE_MISSION_SUPPORT_PT_BONUS") {
        request.mission_support_pt_bonus =
            Some(value.parse().expect(
                "BANGDREAM_OPTIMIZE_MISSION_SUPPORT_PT_BONUS must be a non-negative integer",
            ));
    }
    let result = builder
        .pt_maximize_sync(
            diagnostic.player,
            diagnostic.server,
            Some(event_id),
            request,
        )
        .unwrap();
    let team = result.team.as_ref().expect("single-song result has a team");
    assert_eq!(
        team.team_card_ids.get(team.evaluation.captain_index),
        Some(&team.captain_card_id),
        "captainIndex must address captainCardId in the serialized team order",
    );
    eprintln!(
        "replayed PT maximize: outer={:?} evaluation={:?} average_score={} average_pt={:.6} stat={} cards={:?}",
        result.event_type,
        team.evaluation.event_type,
        team.evaluation.score_distribution.score_sum as f64
            / team.evaluation.score_distribution.sample_count as f64,
        team.evaluation.average_pt.as_f64(),
        team.total_stat,
        team.team_card_ids,
    );

    let song = diagnostic
        .calculation_request
        .songs
        .first()
        .expect("diagnostic has a song");
    let raw_chart = snapshot
        .chart(song.song_id, song.difficulty)
        .expect("snapshot has the diagnostic chart");
    let skill = TeamCardSkill {
        card_id: team.captain_card_id,
        duration: 7.0,
        score_up: 1.5,
        rateup: false,
    };
    let skills = [skill; 6];
    let mut no_fever_chart = raw_chart.clone();
    no_fever_chart.init(0, false).unwrap();
    let no_fever_score = no_fever_chart
        .get_score(&skills, team.total_stat, false)
        .unwrap();
    let mut fever_chart = raw_chart.clone();
    fever_chart.init_with_fever(0, false).unwrap();
    let fever_score = fever_chart
        .get_score(&skills, team.total_stat, false)
        .unwrap();
    eprintln!(
        "fixed identical skills: no_fever={} current_fever={} fever_increment={}",
        no_fever_score,
        fever_score,
        fever_score - no_fever_score,
    );
    assert_eq!(team.evaluation.event_type, result.event_type);
}
