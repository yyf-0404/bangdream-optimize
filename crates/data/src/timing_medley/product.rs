//! Typed application adapter. No saved seed, chart cache or research file path
//! is accepted: raw charts and the current profile construct a fresh problem.
use super::*;
use crate::{
    custom_cards::{calculation_card_configs, enabled_custom_definitions},
    event_songs,
    timing_chart::time_distance::{
        TimeDistanceChart, TimeDistanceScoring, TIME_DISTANCE_PREPARATION_REVISION,
    },
    DataError, EventData,
};
use bangdream_optimize_core::{
    calculate_area_item_percent,
    hit_timing::{
        lazy_orders::reachable_orders, ranges::TimingRangeRequest, replay, ReplayResult,
        REPLAY_FPS, TIMING_REVISION,
    },
    scoring::{DeckPower, ARITHMETIC_REVISION},
    skill_shuffle::MaxScoreTeamOrder,
    BuildResult, CalculationMetrics, CardDefinition, EventType, HitTimingBuildResult,
    HitTimingSongResult, LiveVariant, SongBuildResult, SongSelection,
};
use serde::Deserialize;
use std::collections::BTreeSet;

pub struct TimingSongChart {
    pub song: SongSelection,
    pub play_level: i32,
    pub data: Value,
}

pub struct TimingMaximizeInput<'a> {
    pub event_id: u32,
    pub event: &'a EventData,
    pub player: &'a PlayerConfig,
    pub server: Server,
    pub data: &'a BestdoriData,
    pub card_definitions: &'a BTreeMap<u32, CardDefinition>,
    pub area_item_definitions: &'a BTreeMap<u32, AreaItemDefinition>,
    pub charts: &'a [TimingSongChart],
}

pub type TimingMedleyInput<'a> = TimingMaximizeInput<'a>;

pub fn maximize_timing_in_ranges(
    input: TimingMaximizeInput<'_>,
    restrictions: Option<TimingRangeRequest>,
) -> Result<BuildResult, DataError> {
    match input.event.event_type {
        EventType::Medley => maximize_timing_medley_in_ranges(input, restrictions),
        EventType::Challenge => {
            super::single::maximize(input, restrictions, LiveVariant::ChallengeCp)
        }
        EventType::Versus => super::single::maximize(input, restrictions, LiveVariant::Versus),
        event_type => {
            Err(bangdream_optimize_core::MaximizeError::UnsupportedEventType { event_type }.into())
        }
    }
}

pub(super) fn timing_selections(
    input: &TimingMaximizeInput<'_>,
    count: usize,
    restrictions: Option<&TimingRangeRequest>,
) -> Result<Vec<SongSelection>, DataError> {
    let selections = event_songs(input.player, input.event_id)?;
    if selections.len() != count
        || input.charts.len() != count
        || selections
            .iter()
            .zip(input.charts)
            .any(|(song, chart)| song != &chart.song)
    {
        return Err(engine_error(format!(
            "击打优化需要与当前选曲和难度一致的 {count} 份原始谱面"
        )));
    }
    if let Some(restrictions) = restrictions {
        restrictions.validate_songs(&selections)?;
    }
    Ok(selections)
}

pub(super) struct TimingInventory {
    pub player: PlayerConfig,
    pub data: BestdoriData,
    pub prepared: Vec<PreparedCard>,
    pub native: Vec<PreparedPowerCard>,
    pub item_order: Vec<u32>,
}

pub(super) fn prepare_inventory(
    input: &TimingMaximizeInput<'_>,
    minimum: usize,
) -> Result<TimingInventory, DataError> {
    let custom = enabled_custom_definitions(input.player, input.card_definitions)?;
    let definitions = input
        .card_definitions
        .values()
        .cloned()
        .chain(custom.iter().cloned())
        .collect::<Vec<_>>();
    let mut player = input.player.clone();
    player.card_list = calculation_card_configs(&player);
    let prepared = prepare_cards(
        &definitions,
        &player.card_list,
        &player.character_bouns,
        &input.event.event_bonus,
    )?;
    if prepared.len() != player.card_list.len() || prepared.len() < minimum {
        let message = if minimum == 15 {
            "三曲击打优化需要完整配置的卡池和至少十五张可用卡牌"
        } else {
            "单曲击打优化需要完整配置的卡池和至少五张可用卡牌"
        };
        return Err(engine_error(message));
    }
    let native = prepared
        .iter()
        .map(|card| PreparedPowerCard::new(card, &input.event.event_bonus))
        .collect::<Result<Vec<_>, _>>()
        .map_err(engine_error)?;
    let data = input.data.with_custom_timing_definitions(&custom);
    let mut item_order = player
        .area_item
        .keys()
        .map(|id| id.parse::<u32>().map_err(engine_error))
        .collect::<Result<Vec<_>, _>>()?;
    item_order.sort_unstable();
    Ok(TimingInventory {
        player,
        data,
        prepared,
        native,
        item_order,
    })
}

pub(super) fn timing_result(
    songs: Vec<HitTimingSongResult>,
    pool_card_count: usize,
    equipment_combinations: usize,
    range_restrictions: Option<TimingRangeRequest>,
) -> HitTimingBuildResult {
    HitTimingBuildResult {
        range_restrictions,
        timing_revision: TIMING_REVISION.to_owned(),
        preparation_revision: TIME_DISTANCE_PREPARATION_REVISION.to_owned(),
        model: "chartTimeVisibleNodes".to_owned(),
        fps: REPLAY_FPS,
        offset_frames: 0,
        complete: true,
        pool_card_count,
        equipment_combinations,
        probability_semantics: "perOrderOptimized".to_owned(),
        songs,
    }
}

pub(super) fn timing_team_order(
    team: [u32; 5],
    skill_order: [u32; 6],
    maximum: &LayoutMaximum,
) -> Result<MaxScoreTeamOrder, DataError> {
    let slots = skill_order.map(|id| {
        team.iter()
            .position(|&card| card == id)
            .unwrap_or(usize::MAX)
    });
    let orders = reachable_orders();
    let order = orders
        .iter()
        .find(|order| order.slots == slots)
        .ok_or_else(|| engine_error("unreachable winning order"))?;
    Ok(MaxScoreTeamOrder {
        recommended_team_card_ids: team,
        skill_order_card_ids: skill_order[..5].try_into().unwrap(),
        skill_order_path_count: order.paths,
        max_score_path_count: maximum.max_score_path_count,
        optimal_order_count: maximum
            .optimal_order_count
            .try_into()
            .map_err(engine_error)?,
        total_path_count: maximum.total_path_count,
    })
}

/// Highest three-team sum score over the complete configured pool and common
/// equipment domain, under the declared chart-time / visible-node model.
pub fn maximize_timing_medley(input: TimingMedleyInput<'_>) -> Result<BuildResult, DataError> {
    maximize_timing_medley_in_ranges(input, None)
}

pub fn maximize_timing_medley_in_ranges(
    input: TimingMedleyInput<'_>,
    restrictions: Option<TimingRangeRequest>,
) -> Result<BuildResult, DataError> {
    let start = Instant::now();
    if input.event.event_type != EventType::Medley {
        return Err(engine_error("巡回演出击打优化需要三曲最高分请求"));
    }
    let selections = timing_selections(&input, 3, restrictions.as_ref())?;
    let charts = input
        .charts
        .iter()
        .map(|c| TimeDistanceChart::from_bestdori(&c.data))
        .collect::<Result<Vec<_>, _>>()?;
    let note_count: usize = charts.iter().map(TimeDistanceChart::note_count).sum();
    if note_count > 9_999 {
        return Err(engine_error(
            "三曲 Combo 超出已验证的 Medley 表范围 [0, 9999]",
        ));
    }

    let TimingInventory {
        player,
        data,
        prepared,
        native,
        item_order,
    } = prepare_inventory(&input, 15)?;
    let mut pool = prepared.iter().map(|card| card.card_id).collect::<Vec<_>>();
    pool.sort_unstable();
    let area = calculate_area_item_percent(&player.area_item, input.area_item_definitions)?;
    let mut legacy_charts = Vec::new();
    let mut combo = 0;
    for (raw, chart) in input.charts.iter().zip(&charts) {
        let mut legacy = crate::chart_from_bestdori(raw.play_level, &raw.data)?;
        legacy.init(combo, true)?;
        // Legacy scores only select a feasible seed; the fresh replay below is
        // the sole lower-bound score used by the timing search.
        if legacy.count != chart.note_count() {
            return Err(engine_error("种子谱面与击打谱面的可见节点数量不同"));
        }
        combo += chart.note_count() as i32;
        legacy_charts.push(legacy);
    }
    let seed_selection =
        timing_seed_proposal::propose_prepared(&legacy_charts, &selections, &prepared, &area)
            .map_err(engine_error)?;
    let items: SelectedAreaItems =
        serde_json::from_value(seed_selection["items"].clone()).map_err(engine_error)?;
    let equipment = selected_equipment(
        &player.area_item,
        input.area_item_definitions,
        &items,
        &item_order,
    )
    .map_err(engine_error)?;
    let seed_powers: BTreeMap<_, _> = native
        .iter()
        .map(|c| c.power(&equipment).map(|p| (c.card_id, p)))
        .collect::<Result<_, _>>()
        .map_err(engine_error)?;

    let mut inputs = Vec::new();
    let mut winners = Vec::new();
    let mut used = BTreeSet::new();
    let mut initial_combo = 0;
    for (song, chart) in charts.iter().enumerate() {
        let seed = &seed_selection["songs"][song];
        let team: [u32; 5] =
            serde_json::from_value(seed["teamOrder"]["recommendedTeamCardIds"].clone())
                .map_err(engine_error)?;
        if team
            .iter()
            .any(|id| !pool.contains(id) || !used.insert(*id))
        {
            return Err(engine_error("种子队伍的卡牌不在卡池内或三队重复使用卡牌"));
        }
        let first: [u32; 5] =
            serde_json::from_value(seed["teamOrder"]["skillOrderCardIds"].clone())
                .map_err(engine_error)?;
        let order = [first[0], first[1], first[2], first[3], first[4], team[2]];
        let slots = order.map(|id| team.iter().position(|&c| c == id).unwrap_or(usize::MAX));
        if !reachable_orders().iter().any(|o| o.slots == slots) {
            return Err(engine_error("种子技能顺序在推荐站位中不可达"));
        }
        let skills = data.hit_timing_team_skills(
            input.server,
            &team.map(|card_id| TimingTeamCard {
                card_id,
                skill_level: player.card_list[&card_id.to_string()].skill_level,
            }),
        )?;
        let mut replay_input = chart.input(TimeDistanceScoring {
            power: ScoringPower::from_member_parameters(
                &team.map(|id| seed_powers[&id].final_parameters()),
            )
            .map_err(engine_error)?
            .value(),
            play_level: input.charts[song].play_level,
            skills: slots.map(|i| skills[i].skill.clone()).to_vec(),
            initial_combo,
            medley_combo: true,
            fever: false,
        })?;
        if let Some(ranges) = &restrictions {
            let range = &ranges.songs[song];
            let limits = range.limits(&replay_input)?;
            fit_seed_to_ranges(&mut replay_input, &limits)?;
        }
        let replay_result = replay(&replay_input)?;
        initial_combo += chart.note_count() as u32;
        winners.push(json!({"team":team,"skillOrder":order,"input":replay_input,
            "result":{"hitFrames":replay_result.hit_frames,"hitOrder":replay_result.hit_order,"total":replay_result.total}}));
        inputs.push(replay_input);
    }
    let preparation_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut request = production_request(
        inputs
            .try_into()
            .map_err(|_| engine_error("three inputs required"))?,
        pool.clone(),
    )?;
    if let Some(ranges) = &restrictions {
        let limits = ranges
            .songs
            .iter()
            .zip(&request.inputs)
            .map(|(range, chart)| range.limits(chart))
            .collect::<Result<Vec<_>, _>>()?;
        if limits.iter().flatten().any(Option::is_some) {
            request.frame_ranges = Some(
                limits
                    .try_into()
                    .map_err(|_| engine_error("three limits required"))?,
            );
        }
    }
    let problem = Problem {
        saved: seed_selection,
        seed: Some(json!({"output":{"winners":winners}})),
        player,
        server: input.server,
        data,
        characters: prepared
            .iter()
            .map(|c| (c.card_id, c.character_id))
            .collect(),
        prepared,
        native,
        defs: input.area_item_definitions.clone(),
        item_order,
    };
    let output = solve(request, &problem).map_err(engine_error)?;
    let mut result = result_from_output(
        input.event_id,
        &selections,
        &problem,
        output,
        preparation_ms,
        start,
    )?;
    if let Some(timing) = &mut result.hit_timing {
        for (index, song) in timing.songs.iter_mut().enumerate() {
            let limits = match &restrictions {
                Some(r) => r.songs[index].limits(&song.input)?,
                None => vec![None; song.input.notes.len()],
            };
            song.result = crate::timing_plan::simplify::restore_chart_positions(
                &song.input,
                &limits,
                song.result.clone(),
            );
            song.input.hit_frames = song.result.hit_frames.clone();
            song.input.hit_order = song.result.hit_order.clone();
        }
        timing.range_restrictions = restrictions;
    }
    Ok(result)
}

fn fit_seed_to_ranges(
    input: &mut ReplayInput,
    limits: &[Option<[u32; 2]>],
) -> Result<(), DataError> {
    use bangdream_optimize_core::{hit_timing::independent::JointTimeline, scoring::Judgement};
    let original = input.clone();
    let references = input.chart_reference_frames()?;
    for (index, (note, limit)) in input.notes.iter().zip(limits).enumerate() {
        let allowed = |f: &u32| limit.is_none_or(|[a, b]| a <= *f && *f <= b);
        let raw = note.frame_domain(Judgement::Bad)?;
        if !raw.iter().any(allowed) {
            return Err(engine_error(format!(
                "第 {} 个键的限制与该键型的可击打帧没有交集",
                index + 1
            )));
        }
        let base = references[index];
        if let Some(frame) = note
            .frame_domain(Judgement::Perfect)?
            .into_iter()
            .filter(allowed)
            .min_by_key(|f| f.abs_diff(base))
        {
            input.hit_frames[index] = frame;
        }
    }
    input.hit_order.clear();
    if replay(input).is_ok()
        && limits
            .iter()
            .zip(&input.hit_frames)
            .all(|(limit, &f)| limit.is_none_or(|[a, b]| a <= f && f <= b))
    {
        return Ok(());
    }
    // Individually valid references can still conflict through same-lane
    // selection or dispatch order. Keep the validated unrestricted seed while
    // constructing a restricted witness, without widening any note domain.
    let winner = JointTimeline::maximum_in_frame_ranges(&original, limits)
        .map_err(engine_error)?
        .ok_or_else(|| engine_error("当前种子队伍未能在限制内构造可行方案，请放宽限制"))?;
    input.hit_frames = winner.result.hit_frames;
    input.hit_order = winner.result.hit_order;
    replay(input)?;
    Ok(())
}

#[cfg(test)]
mod typed_range_tests {
    use super::*;

    #[test]
    fn fractional_tail_zero_offset_uses_its_legal_reference() {
        let chart = TimeDistanceChart::from_bestdori(&json!([
            {"type":"BPM","beat":0,"bpm":137},
            {"type":"Slide","connections":[{"beat":2,"lane":0},{"beat":4,"lane":1,"flick":true}]}
        ]))
        .unwrap();
        let mut input = chart
            .input(TimeDistanceScoring {
                power: 300000.,
                play_level: 27,
                skills: vec![],
                initial_combo: 0,
                medley_combo: true,
                fever: false,
            })
            .unwrap();
        let base = input.chart_reference_frame(1).unwrap();
        assert_eq!((base, input.hit_frames[1]), (211, 211));
        assert!(
            fit_seed_to_ranges(&mut input.clone(), &[None, Some([base - 1, base - 1])])
                .unwrap_err()
                .to_string()
                .contains("第 2 个键")
        );
        fit_seed_to_ranges(&mut input, &[None, Some([base, base])]).unwrap();
        assert_eq!(input.hit_frames[1], 211);
        replay(&input).unwrap();
    }
}

fn production_request(inputs: [ReplayInput; 3], pool: Vec<u32>) -> Result<Request, DataError> {
    // The accepted complete-search settings. Limits guard resource exhaustion;
    // any incomplete return is rejected by result_from_output.
    let mut request: Request = serde_json::from_value(json!({
        "sourcePath":"", "resultPath":"", "seedPath":"memory", "inputs":inputs, "pool":pool,
        "timingModel":"typedTimeDistance", "candidateLifetimeModel":"greenVisibleMidpoint",
        "allItems":true, "maxTeams":1_000_000, "maxNodes":50_000_000,
        "shareEquivalentSkills":true, "batchPowers":true, "poolScan":true,
        "preparedCache":true, "pairBounds":true, "pricedPrefix":true,
        "signaturePruning":true, "crossDurationPruning":true, "crossCharacterPruning":true,
        "affinePruning":true, "framePruning":true, "netBounds":true, "stateNetBounds":true,
        "adjacentNetBounds":true, "compactMasks":true, "streamCompetitive":true,
        "companionBounds":true, "keyPairBounds":true, "earlyKeyPairBounds":true,
        "prefixKeyPairBounds":true, "refinePools":true, "refineCutoffs":true, "refineReplacements":true,
    })).map_err(engine_error)?;
    // Match the accepted full-process benchmark and keep application memory
    // independent of whole-score reuse. Shared geometric templates still apply.
    request.equivalent_score_reuse = Some(false);
    Ok(request)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Winner {
    team: [u32; 5],
    skill_order: [u32; 6],
    maximum: LayoutMaximum,
    input: ReplayInput,
    result: ReplayResult,
}

fn result_from_output(
    event_id: u32,
    selections: &[SongSelection],
    problem: &Problem,
    output: Value,
    _preparation_ms: f64,
    start: Instant,
) -> Result<BuildResult, DataError> {
    if output["incomplete"] != false || output["allItems"]["complete"] != true {
        return Err(engine_error(
            "击打搜索达到资源上限，未完成全部候选的最高分证明",
        ));
    }
    let items: SelectedAreaItems =
        serde_json::from_value(output["selectedItems"].clone()).map_err(engine_error)?;
    let winners: Vec<Winner> =
        serde_json::from_value(output["winners"].clone()).map_err(engine_error)?;
    if winners.len() != 3 {
        return Err(engine_error("missing winning teams"));
    }
    let powers = problem.powers(&items).map_err(engine_error)?;
    let mut songs = Vec::new();
    let mut timing_songs = Vec::new();
    let mut display = [0.0; 3];
    let mut total_score = 0_u64;
    for (index, (selection, mut winner)) in selections.iter().zip(winners).enumerate() {
        winner.input.hit_frames = winner.result.hit_frames.clone();
        winner.input.hit_order = winner.result.hit_order.clone();
        let replayed = replay(&winner.input)?;
        if replayed != winner.result || replayed.total != winner.maximum.score {
            return Err(engine_error(
                "winning timing witness does not replay exactly",
            ));
        }
        let deck =
            DeckPower::from_members(&winner.team.map(|id| powers[&id])).map_err(engine_error)?;
        if deck.scoring().value().to_bits() != winner.input.power.to_bits() {
            return Err(engine_error(
                "winning scoring power differs from the physical layout",
            ));
        }
        display[index] = deck.display_unrounded();
        total_score += replayed.total;
        songs.push(SongBuildResult {
            song_id: selection.song_id,
            difficulty: selection.difficulty,
            score: int_score(replayed.total).map_err(engine_error)?,
            stat: deck
                .display_for_live(LiveVariant::Medley)
                .map_err(engine_error)?,
            team_card_ids: winner.team.to_vec(),
            captain_card_id: winner.team[2],
            team_order: Some(timing_team_order(
                winner.team,
                winner.skill_order,
                &winner.maximum,
            )?),
            skill_queue_risk: false,
        });
        timing_songs.push(HitTimingSongResult {
            song_id: selection.song_id,
            difficulty: selection.difficulty,
            input: winner.input,
            result: replayed,
        });
    }
    if output["lazy"]["plan"]["score"].as_u64() != Some(total_score) {
        return Err(engine_error(
            "three-song witness sum differs from the proven maximum",
        ));
    }
    let combinations = output["allItems"]["rawCombinations"]
        .as_u64()
        .ok_or_else(|| engine_error("missing equipment coverage"))? as usize;
    Ok(BuildResult {
        scoring_revision: Some(ARITHMETIC_REVISION.to_owned()),
        skill_queue_notices: Vec::new(),
        event_id,
        event_type: EventType::Medley,
        total_score: int_score(total_score).map_err(engine_error)?,
        total_stat: DeckPower::display_total_values(display).map_err(engine_error)?,
        songs,
        items: Some(items),
        solver: Some("timingExact".to_owned()),
        metrics: Some(CalculationMetrics {
            card_count: problem.prepared.len(),
            song_count: 3,
            item_combinations_before: combinations,
            item_combinations_after: output["allItems"]["afterDominance"].as_u64().unwrap_or(0)
                as usize,
            total_elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
            ..Default::default()
        }),
        hit_timing: Some(timing_result(
            timing_songs,
            problem.prepared.len(),
            combinations,
            None,
        )),
    })
}
