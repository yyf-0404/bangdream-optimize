use bangdream_optimize_core::{hit_timing::{replay,lazy_orders::reachable_orders,ranges::RANGE_REVISION}, BuildResult, EventType};
use bangdream_optimize_web_wasm::calculate_payload;
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn fixture() -> Value { serde_json::from_str(include_str!("fixtures/timing-medley.json")).unwrap() }

#[test]
fn product_solves_complete_equipment_and_exports_exact_roundtrip_witnesses() {
    let result=calculate_payload(serde_json::from_value(fixture()).unwrap()).unwrap();
    let roundtrip: BuildResult=serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
    assert_eq!(result,roundtrip);
    let timing=result.hit_timing.as_ref().unwrap();
    assert_eq!((timing.pool_card_count,timing.equipment_combinations,timing.fps,timing.offset_frames),(15,135,120,0));
    assert!(timing.complete);
    assert_eq!(timing.probability_semantics,"perOrderOptimized");
    let mut used=BTreeSet::new();
    let mut combo=0;
    let mut total=0;
    for (song,hit) in result.songs.iter().zip(&timing.songs) {
        assert_eq!(hit.input.initial_combo,combo);
        assert!(hit.input.medley_combo);
        assert_eq!(hit.input.fever_frames,None);
        combo+=hit.input.notes.len() as u32;
        assert_eq!(replay(&hit.input).unwrap(),hit.result);
        assert_eq!(song.score as u64,hit.result.total);
        total+=hit.result.total;
        assert_eq!(song.team_card_ids[2],song.captain_card_id);
        for id in &song.team_card_ids { assert!(used.insert(*id)); }
        let order=song.team_order.as_ref().unwrap();
        let first=order.skill_order_card_ids.map(|id| song.team_card_ids.iter().position(|c| *c==id).unwrap());
        assert!(reachable_orders().iter().any(|o| o.slots[..5]==first && o.slots[5]==2));
        assert!(order.max_score_path_count>=order.skill_order_path_count);
        assert!(order.max_score_path_count<=order.total_path_count);
    }
    assert_eq!(used.len(),15);
    assert_eq!(result.total_score as u64,total);
}

#[test]
fn product_rejects_unsupported_mode_missing_raw_charts_and_incomplete_pool() {
    let mut cases=Vec::new();
    let mut wrong_mode=fixture();wrong_mode["event"]["eventType"]=json!("festival");
    cases.push((wrong_mode,"not supported by maximize"));
    let mut missing=fixture();missing["charts"].as_array_mut().unwrap().pop();
    cases.push((missing,"3 份原始谱面"));
    let mut wrong_order=fixture();wrong_order["charts"].as_array_mut().unwrap().swap(0,1);
    cases.push((wrong_order,"当前选曲和难度"));
    let mut short=fixture();short["player"]["cardList"].as_object_mut().unwrap().remove("4");
    cases.push((short,"至少十五张"));
    for (case,message) in cases {
        let error=calculate_payload(serde_json::from_value(case).unwrap()).unwrap_err();
        assert!(error.to_string().contains(message),"{error}");
    }
}

#[test]
fn product_rejects_invalid_growth_ranks_and_skill_levels_before_search() {
    for (field, value, message) in [
        ("limitBreakRank", 5, "limit break rank 5 is invalid for card 4"),
        ("limitBreakRank", 255, "limit break rank 255 is invalid for card 4"),
        ("skillLevel", 0, "skill level 0 is invalid for card 4"),
        ("skillLevel", 6, "skill level 6 is invalid for card 4"),
    ] {
        let mut case = fixture();
        case["player"]["cardList"]["4"][field] = json!(value);
        let error = calculate_payload(serde_json::from_value(case).unwrap()).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
}

fn single_fixture(event_type: &str) -> Value {
    let mut payload = fixture();
    payload["event"]["eventType"] = json!(event_type);
    let event_id = payload["eventId"].as_u64().unwrap().to_string();
    payload["player"]["eventSongs"][event_id].as_array_mut().unwrap().truncate(1);
    payload["charts"].as_array_mut().unwrap().truncate(1);
    payload["player"]["cardList"].as_object_mut().unwrap()
        .retain(|id, _| ["4", "16", "102", "118", "126", "131"].contains(&id.as_str()));
    payload
}

#[test]
fn single_highest_score_replays_and_applies_limits_for_both_events() {
    for event_type in [EventType::Challenge, EventType::Versus] {
        let mut payload = single_fixture(event_type.as_str());
        let best = calculate_payload(serde_json::from_value(payload.clone()).unwrap()).unwrap();
        assert_eq!(best.event_type, event_type);
        assert_eq!(best.songs.len(), 1);
        assert_eq!(best.total_stat, best.songs[0].stat);
        assert_eq!(best.total_score, best.songs[0].score);
        assert_eq!(best.songs[0].team_card_ids[2], best.songs[0].captain_card_id);
        let timing = best.hit_timing.as_ref().unwrap();
        assert_eq!(timing.pool_card_count, 6);
        assert!(timing.equipment_combinations > 1);
        let song = &timing.songs[0];
        assert!(!song.input.medley_combo);
        assert_eq!(song.input.initial_combo, 0);
        assert!(song.input.fever_frames.is_none());
        assert_eq!(replay(&song.input).unwrap(), song.result);

        payload["options"]["hitTimingRanges"] = json!({
            "revision": RANGE_REVISION,
            "songs": [{"songId": song.song_id, "difficulty": song.difficulty,
                "reference": song.input, "ranges": [{"noteIndex": 1, "firstOffset": 0, "lastOffset": 0}]}],
        });
        let next = calculate_payload(serde_json::from_value(payload).unwrap()).unwrap();
        let timing = next.hit_timing.unwrap();
        assert!(next.total_score <= best.total_score);
        assert_eq!(timing.range_restrictions.unwrap().songs.len(), 1);
        let song = &timing.songs[0];
        assert_eq!(song.input.hit_frames[1], song.input.chart_reference_frame(1).unwrap());
        assert_eq!(replay(&song.input).unwrap(), song.result);
    }
}

#[test]
fn single_highest_score_rejects_missing_range_bindings() {
    let mut payload = single_fixture("challenge");
    payload["options"]["hitTimingRanges"] = json!({"revision": RANGE_REVISION, "songs": []});
    let error = calculate_payload(serde_json::from_value(payload).unwrap()).unwrap_err();
    assert!(error.to_string().contains("范围与当前选曲"), "{error}");
}
