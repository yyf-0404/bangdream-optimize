//! Bestdori CN master records and complete charts from the local game-data snapshot.
//! The expected teams and path counts were captured before two-stage layout search.
use bangdream_optimize_core::{
    hit_timing::{independent::JointTimeline, layouts::physical_layouts, replay},
    prepare_cards,
    scoring::{
        preparation::{selected_equipment, PreparedPowerCard},
        ScoringPower,
    },
    BuildResult,
};
use bangdream_optimize_data::{event_bonus, BestdoriData};
use bangdream_optimize_web_wasm::{calculate_payload, WebCalculationPayload};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

fn suite() -> Value {
    serde_json::from_str(include_str!("fixtures/real-card-extremes.json")).unwrap()
}

fn payload(suite: &Value, case: &Value, timing: bool) -> WebCalculationPayload {
    let mut value = suite["shared"].clone();
    let event = case["eventId"].to_string();
    let charts: Vec<_> = case["charts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &suite["charts"][i.as_u64().unwrap() as usize])
        .collect();
    let songs: Vec<_> = charts
        .iter()
        .map(|c| {
            json!({
                "songId": c["songId"], "difficulty": c["difficulty"]
            })
        })
        .collect();
    let owned: Map<_, _> = case["cards"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, growth)| {
            (
                id.clone(),
                suite["growth"][growth.as_str().unwrap()].clone(),
            )
        })
        .collect();
    value["eventId"] = case["eventId"].clone();
    value["event"] = suite["events"][&event].clone();
    value["charts"] = json!(charts);
    value["options"] = json!({"hitTiming": timing});
    value["player"] = suite["player"].clone();
    value["player"]["currentEvent"] = case["eventId"].clone();
    value["player"]["eventSongs"] = json!({event: songs});
    value["player"]["cardList"] = Value::Object(owned);
    serde_json::from_value(value).unwrap()
}

fn check_case(suite: &Value, name: &str) -> BuildResult {
    let case = &suite["cases"][name];
    let run = |mode: &str, timing: bool| {
        let result = calculate_payload(payload(suite, case, timing)).unwrap();
        let mut used = BTreeSet::new();
        let mut songs = Vec::new();
        for song in &result.songs {
            let mut characters = BTreeSet::new();
            assert_eq!(song.team_card_ids.len(), 5);
            for &id in &song.team_card_ids {
                let key = id.to_string();
                assert!(case["cards"].get(&key).is_some() && used.insert(id));
                assert!(characters.insert(
                    suite["shared"]["cards"][key]["characterId"]
                        .as_u64()
                        .unwrap()
                ));
            }
            let order = song.team_order.as_ref().unwrap();
            assert_eq!(song.captain_card_id, order.recommended_team_card_ids[2]);
            songs.push(json!({
                "score": song.score, "teamCardIds": song.team_card_ids, "teamOrder": song.team_order
            }));
        }
        assert_eq!(json!(songs), case["expected"][mode], "{name} {mode}");
        assert_eq!(
            result.total_score,
            result.songs.iter().map(|song| song.score).sum::<i32>()
        );
        assert_eq!(result.hit_timing.is_some(), timing);
        if let Some(hit) = &result.hit_timing {
            assert!(hit.complete);
            assert_eq!(
                hit.pool_card_count,
                case["cards"].as_object().unwrap().len()
            );
            assert_eq!(hit.songs.len(), result.songs.len());
            for (song, plan) in result.songs.iter().zip(&hit.songs) {
                assert_eq!(
                    (song.song_id, song.difficulty),
                    (plan.song_id, plan.difficulty)
                );
                assert_eq!(replay(&plan.input).unwrap(), plan.result);
                assert_eq!(plan.result.total, song.score as u64);
            }
        }
        result
    };
    run("off", false);
    run("on", true)
}

#[test]
fn sixteen_five_stars_with_the_same_skill_tie_on_every_native_order() {
    check_case(&suite(), "same_skill_medley");
}

#[test]
fn adding_240_untrained_cards_preserves_the_medley_optimum_and_witness() {
    let suite = suite();
    assert_eq!(
        suite["cases"]["wide_medley"]["expected"],
        suite["cases"]["same_skill_medley"]["expected"]
    );
    check_case(&suite, "wide_medley");
}

#[test]
fn three_teams_compete_for_five_characters_with_one_spare_card() {
    check_case(&suite(), "five_characters");
}

#[test]
fn fifteen_cards_with_only_two_copies_of_a_required_character_are_infeasible() {
    let suite = suite();
    let case = &suite["cases"]["one_copy_short"];
    let mut counts = BTreeMap::new();
    for id in case["cards"].as_object().unwrap().keys() {
        *counts
            .entry(
                suite["shared"]["cards"][id]["characterId"]
                    .as_u64()
                    .unwrap(),
            )
            .or_insert(0) += 1;
    }
    let mut counts: Vec<_> = counts.into_values().collect();
    counts.sort_unstable();
    assert_eq!(counts, [2, 3, 3, 3, 4]);
    for (timing, reason) in [
        (false, "no valid medley plan"),
        (true, "did not construct a medley seed"),
    ] {
        let err = calculate_payload(payload(&suite, case, timing)).unwrap_err();
        assert!(err.to_string().contains(reason), "{err}");
    }
}

#[test]
fn versus_selects_among_different_skill_and_growth_levels() {
    check_case(&suite(), "mixed_growth_versus");
}

// Score every reachable power independently of the representative chosen by the layout search.
fn scores_for_physical_layouts(
    input: WebCalculationPayload,
    result: &BuildResult,
) -> BTreeMap<ScoringPower, u64> {
    let bonus = event_bonus(&input.event).unwrap();
    let mut data = BestdoriData::from_values(
        input.cards,
        input.characters,
        input.skills,
        input.area_items,
    )
    .unwrap();
    data.apply_repairs(input.cards_fix, input.skills_fix, input.area_items_fix)
        .unwrap();
    let team = &result.songs[0].team_card_ids;
    let cards: Vec<_> = team
        .iter()
        .map(|&id| data.card_definition(id).unwrap())
        .collect();
    let cards = prepare_cards(
        &cards,
        &input.player.card_list,
        &input.player.character_bouns,
        &bonus,
    )
    .unwrap();
    let items = &input.player.area_item;
    let mut order: Vec<u32> = items.keys().map(|key| key.parse().unwrap()).collect();
    order.sort_unstable();
    let equipped = selected_equipment(
        items,
        &data.area_item_definitions(input.server).unwrap(),
        result.items.as_ref().unwrap(),
        &order,
    )
    .unwrap();
    let parameters: Vec<_> = cards
        .iter()
        .map(|card| {
            PreparedPowerCard::new(card, &bonus)
                .unwrap()
                .power(&equipped)
                .unwrap()
                .final_parameters()
        })
        .collect();
    let hit = &result.hit_timing.as_ref().unwrap().songs[0].input;
    assert!(hit.skills.windows(2).all(|w| w[0] == w[1]));
    let model = JointTimeline::new(hit).unwrap();
    let mut scores = BTreeMap::new();
    let layouts = physical_layouts();
    assert_eq!(layouts.len(), 120);
    for layout in layouts {
        let power = ScoringPower::from_member_parameters(&layout.map(|i| parameters[i])).unwrap();
        scores
            .entry(power)
            .or_insert_with(|| model.score(power).unwrap());
    }
    scores
}

fn compare_adjacent_powers(name: &str) -> ([u64; 2], bool) {
    let suite = suite();
    let case = &suite["cases"][name];
    let result = check_case(&suite, name);
    let scores = scores_for_physical_layouts(payload(&suite, case, true), &result);
    let chosen =
        ScoringPower::new(result.hit_timing.as_ref().unwrap().songs[0].input.power).unwrap();
    assert_eq!(scores.len(), 2);
    let (&low, &low_score) = scores.first_key_value().unwrap();
    let (&high, &high_score) = scores.last_key_value().unwrap();
    assert_eq!(high.cache_key(), low.cache_key() + 1);
    assert_eq!(scores[&chosen], *scores.values().max().unwrap());
    assert_eq!(scores[&chosen], result.total_score as u64);
    ([low_score, high_score], chosen == high)
}

#[test]
fn lower_power_layout_can_win_an_exact_score_tie() {
    assert_eq!(
        compare_adjacent_powers("rounding_single"),
        ([1_491_302, 1_491_302], false)
    );
}

#[test]
fn a_single_float_step_can_change_the_optimum_by_130_points() {
    assert_eq!(
        compare_adjacent_powers("rounding_crossing"),
        ([1_479_597, 1_479_727], true)
    );
}
