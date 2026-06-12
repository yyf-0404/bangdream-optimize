use bangdream_optimize_core::{
    calculate_area_item_percent, prepare_cards, Attribute, BuildResult, PlayerConfig, PreparedCard,
    Server, SongSelection, TeamCardSkill,
};
use bangdream_optimize_data::{
    BestdoriFilesystemCalculationInputBuilder, BestdoriFilesystemConfig,
};
use serde::Deserialize;
use std::{collections::BTreeMap, env, error::Error, fs, path::PathBuf};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostic {
    server: Server,
    event_id: Option<u32>,
    result: BuildResult,
    player: PlayerConfig,
}

#[derive(Debug)]
struct RecalculatedSong {
    score: i32,
    stat: i32,
    captain_card_id: u32,
    skill_order: Vec<u32>,
    score_ups: Vec<f64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("score_check failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = env::args().collect::<Vec<_>>();
    let Some(diagnostic_path) = args.get(1) else {
        eprintln!(
            "usage: cargo run -p bangdream-optimize-data --bin score_check -- <diagnostic.json> [game-data-root]"
        );
        std::process::exit(2);
    };
    let game_data_root = args
        .get(2)
        .map(PathBuf::from)
        .or_else(|| {
            env::var("BANGDREAM_OPTIMIZE_GAME_DATA_ROOT")
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("var/game-data"));

    let diagnostic: Diagnostic = serde_json::from_slice(&fs::read(diagnostic_path)?)?;
    let event_id = diagnostic
        .event_id
        .or(diagnostic.player.current_event)
        .unwrap_or(diagnostic.result.event_id);
    let songs = diagnostic
        .result
        .songs
        .iter()
        .map(|song| SongSelection {
            song_id: song.song_id,
            difficulty: song.difficulty,
        })
        .collect::<Vec<_>>();
    let items = diagnostic
        .result
        .items
        .clone()
        .ok_or("diagnostic result has no selected area items")?;

    let builder = BestdoriFilesystemCalculationInputBuilder::load(
        BestdoriFilesystemConfig::from_bestdori_api_root(game_data_root),
    )?;
    let snapshot = builder.snapshot_for(&diagnostic.player, event_id, &songs, diagnostic.server)?;
    let event = snapshot
        .events
        .get(&event_id)
        .ok_or("snapshot is missing selected event")?;
    let card_definitions = diagnostic
        .player
        .card_list
        .keys()
        .map(|card_id| {
            let parsed = card_id.parse::<u32>()?;
            snapshot
                .card_definitions
                .get(&parsed)
                .cloned()
                .ok_or_else(|| format!("snapshot is missing card definition {parsed}").into())
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let prepared = prepare_cards(
        &card_definitions,
        &diagnostic.player.card_list,
        &diagnostic.player.character_bouns,
        &event.event_bonus,
    )?;
    let prepared_by_id = prepared
        .iter()
        .map(|card| (card.card_id, card))
        .collect::<BTreeMap<_, _>>();
    let area = calculate_area_item_percent(
        &diagnostic.player.area_item,
        &snapshot.area_item_definitions,
    )?;

    let mut combo = 0;
    let charts = songs
        .iter()
        .map(|song| {
            let mut chart = snapshot
                .chart(song.song_id, song.difficulty)
                .ok_or_else(|| {
                    format!(
                        "snapshot is missing chart {}:{}",
                        song.song_id, song.difficulty
                    )
                })?
                .clone();
            chart.init(
                combo,
                diagnostic.result.event_type == bangdream_optimize_core::EventType::Medley,
            )?;
            combo += chart.count as i32;
            Ok(chart)
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    let mut failed = false;
    let mut total_score = 0;
    let mut total_stat = 0;
    println!(
        "score_check: event={} items={:?} songs={}",
        event_id,
        items,
        diagnostic.result.songs.len()
    );

    for (song_idx, reported) in diagnostic.result.songs.iter().enumerate() {
        let team = reported
            .team_card_ids
            .iter()
            .map(|card_id| {
                prepared_by_id
                    .get(card_id)
                    .copied()
                    .ok_or_else(|| format!("prepared cards are missing card {card_id}").into())
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
        let recalculated = recalculate_song(
            &team,
            &area,
            &items,
            &charts[song_idx],
            diagnostic.result.event_type == bangdream_optimize_core::EventType::Medley,
        )?;
        total_score += recalculated.score;
        total_stat += recalculated.stat;

        let ok = reported.score == recalculated.score
            && reported.stat == recalculated.stat
            && reported.captain_card_id == recalculated.captain_card_id;
        failed |= !ok;
        println!(
            "song#{} {}: reported score={} stat={} captain={} | computed score={} stat={} captain={} order={:?} scoreUps={:?}",
            song_idx + 1,
            if ok { "ok" } else { "mismatch" },
            reported.score,
            reported.stat,
            reported.captain_card_id,
            recalculated.score,
            recalculated.stat,
            recalculated.captain_card_id,
            recalculated.skill_order,
            recalculated.score_ups,
        );
    }

    let total_ok =
        diagnostic.result.total_score == total_score && diagnostic.result.total_stat == total_stat;
    failed |= !total_ok;
    println!(
        "total {}: reported score={} stat={} | computed score={} stat={}",
        if total_ok { "ok" } else { "mismatch" },
        diagnostic.result.total_score,
        diagnostic.result.total_stat,
        total_score,
        total_stat,
    );

    if failed {
        Err("score check found mismatches".into())
    } else {
        Ok(())
    }
}

fn recalculate_song(
    team: &[&PreparedCard],
    area: &bangdream_optimize_core::AreaItemPercent,
    items: &bangdream_optimize_core::SelectedAreaItems,
    chart: &bangdream_optimize_core::Chart,
    score_as_medley: bool,
) -> Result<RecalculatedSong, Box<dyn Error>> {
    let band = unified_band_for_cards(team);
    let attribute = unified_attribute_for_cards(team);
    let stat = team
        .iter()
        .map(|card| card.add_up_stat(area, &items.band, &items.attribute, items.magazine.as_str()))
        .sum::<f64>()
        .floor() as i32;
    let skills = team
        .iter()
        .map(|card| TeamCardSkill {
            card_id: card.card_id,
            duration: card.skill.duration,
            score_up: card.score_up.resolve(band, attribute),
            rateup: card.skill.rateup,
        })
        .collect::<Vec<_>>();
    let order = chart.get_max_meta_order(&skills)?;
    let skill_order = order
        .order_indices
        .iter()
        .map(|&idx| skills[idx])
        .chain(std::iter::once(skills[order.captain_index]))
        .collect::<Vec<_>>();
    let score = chart.get_score(&skill_order, stat, score_as_medley)?;

    Ok(RecalculatedSong {
        score,
        stat,
        captain_card_id: skills[order.captain_index].card_id,
        skill_order: skill_order.iter().map(|skill| skill.card_id).collect(),
        score_ups: skill_order.iter().map(|skill| skill.score_up).collect(),
    })
}

fn unified_band_for_cards(cards: &[&PreparedCard]) -> Option<u32> {
    let first = cards.first()?.band_id;
    cards
        .iter()
        .all(|card| card.band_id == first)
        .then_some(first)
}

fn unified_attribute_for_cards(cards: &[&PreparedCard]) -> Option<Attribute> {
    let first = cards.first()?.attribute;
    cards
        .iter()
        .all(|card| card.attribute == first)
        .then_some(first)
}
