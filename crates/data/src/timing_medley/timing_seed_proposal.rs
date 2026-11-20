//! Adapt the shared bounded team proposal to a reachable physical/skill order.
//! Its legacy chart estimates only guide selection; the caller must replay the
//! resulting teams under the declared time-distance model before using a score.
use bangdream_optimize_core::{
    build_team_candidates,
    hit_timing::lazy_orders::reachable_orders,
    medley::{propose_medley_seed, MedleySeedTeam},
    AreaItemPercent, Chart, PreparedCard, SongSelection, TeamGenerationOptions,
};
use bangdream_optimize_data::chart_from_bestdori;
use bangdream_optimize_medley_solver::{
    solve_medley_with, MedleySolverInput, MedleySolverPreference,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet};

pub fn propose(
    source: &Value,
    cards: &[PreparedCard],
    area: &AreaItemPercent,
) -> Result<Value, Box<dyn std::error::Error>> {
    let raw = source["charts"].as_array().ok_or("missing charts")?;
    let mut combo = 0;
    let mut charts = Vec::new();
    let mut selections = Vec::new();
    for row in raw {
        let song = row["songId"].as_u64().ok_or("songId missing")?.to_string();
        let difficulty = row["difficulty"]
            .as_u64()
            .ok_or("difficulty missing")?
            .to_string();
        let level = source["songs"][song]["difficulty"][difficulty]["playLevel"]
            .as_i64()
            .ok_or("song level missing")?;
        let mut chart = chart_from_bestdori(i32::try_from(level)?, &row["data"])?;
        chart.init(combo, true)?;
        combo += chart.count as i32;
        charts.push(chart);
        selections.push(SongSelection {
            song_id: row["songId"].as_u64().ok_or("songId missing")?.try_into()?,
            difficulty: row["difficulty"].as_u64().ok_or("difficulty missing")?.try_into()?,
        });
    }
    propose_prepared(&charts, &selections, cards, area)
}

pub(super) fn propose_prepared(
    charts: &[Chart], selections: &[SongSelection], cards: &[PreparedCard], area: &AreaItemPercent,
) -> Result<Value, Box<dyn std::error::Error>> {
    if charts.len() != 3 || selections.len() != 3 {
        return Err("timing seed requires exactly three charts".into());
    }
    let proposal_start = Instant::now();
    let mut seed = propose_medley_seed(cards, &charts, area)?
        .ok_or("bounded search did not construct a medley seed")?;
    let proposal_seconds = proposal_start.elapsed().as_secs_f64();
    // Jointly repartition only the already selected fifteen cards. This can
    // escape a local optimum requiring exchanges between occupied teams;
    // it never enumerates the full inventory's five-card combinations.
    let repartition_start = Instant::now();
    let selected: BTreeSet<_> = seed.teams.iter().flat_map(|t| t.card_ids).collect();
    if selected.len() != 15 {
        return Err("seed proposal must contain fifteen distinct cards".into());
    }
    let subset: Vec<_> = cards
        .iter()
        .filter(|c| selected.contains(&c.card_id))
        .cloned()
        .collect();
    let candidates = build_team_candidates(
        &subset,
        &charts,
        area,
        &seed.items,
        TeamGenerationOptions {
            max_candidates: 3003,
            ..Default::default()
        },
    )?;
    let plan = solve_medley_with(
        &MedleySolverInput {
            current_best: 0,
            team_masks: candidates.iter().map(|c| c.mask).collect(),
            scores: candidates
                .iter()
                .map(|c| c.scores.as_slice().try_into().unwrap())
                .collect(),
        },
        MedleySolverPreference::StrictExact,
    )?;
    seed.teams = std::array::from_fn(|song| {
        let team = &candidates[plan.indices[song]];
        MedleySeedTeam {
            card_ids: team.ordered_team_card_ids.as_ref().unwrap()[song]
                .as_slice()
                .try_into()
                .unwrap(),
            captain_card_id: team.captain_card_ids[song],
        }
    });
    let repartition_seconds = repartition_start.elapsed().as_secs_f64();
    let mut songs = Vec::new();
    let native_orders = reachable_orders();
    for (song, team) in seed.teams.iter().enumerate() {
        let captain = team
            .card_ids
            .iter()
            .position(|&id| id == team.captain_card_id)
            .ok_or("seed captain is outside team")?;
        // A seed only needs a reachable layout. Final probability/layout
        // optimization remains in the exact timing search. Actual f32 power
        // for this layout is recomputed by the caller before seed replay.
        let order = native_orders
            .iter()
            .find(|o| o.slots[captain] == 2)
            .ok_or("no reachable layout for proposed captain activation")?;
        let mut layout = [0; 5];
        for (activation, &id) in team.card_ids.iter().enumerate() {
            layout[order.slots[activation]] = id;
        }
        songs.push(
            json!({"songId":selections[song].song_id,"difficulty":selections[song].difficulty,
            "captainCardId":team.captain_card_id,"teamOrder":{
                "recommendedTeamCardIds":layout,"skillOrderCardIds":team.card_ids}}),
        );
    }
    Ok(
        json!({"solver":"candidateOnly","items":seed.items,"songs":songs,
        "proposal":{"estimatedItemCount":seed.estimated_item_count,
            "baseProposalSeconds":proposal_seconds,"repartitionSeconds":repartition_seconds,
            "repartitionCards":subset.len(),"repartitionCandidates":candidates.len(),
            "fullCandidateEnumeration":false,"optimalityClaimed":false}}),
    )
}

use bangdream_optimize_core::timing::Instant;
