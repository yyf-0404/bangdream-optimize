use super::*;
use bangdream_optimize_core::hit_timing::{
    replay, NoteWindow, ReplayInput, ReplayNote, REPLAY_FPS,
};
use bangdream_optimize_core::scoring::Judgement;
use serde_json::json;

fn snapshot() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../var/game-data/skillsCNfix.json"
    ))
    .unwrap()
}

fn custom_definition(id:u32) -> bangdream_optimize_core::CardDefinition {
    serde_json::from_value(json!({
        "cardId":id,"characterId":id,"bandId":1,"rarity":4,"attribute":"happy",
        "levelStats":{"60":{"performance":1,"technique":1,"visual":1}},
        "trainingStat":{"performance":0,"technique":0,"visual":0},
        "episodeStats":[{"performance":0,"technique":0,"visual":0},{"performance":0,"technique":0,"visual":0}],
        "skill":{"durations":[5.0,5.5,6.0,6.5,7.0],"scoreUp":{"default":1.0,
          "unificationActivateEffectValue":1.25,"unificationActivateConditionBandId":1,"unificationActivateConditionType":"happy"}}
    })).unwrap()
}

#[test]
fn custom_skill_uses_actual_mixed_team_conditions_and_selected_growth_level() {
    let raw=BestdoriData::from_values(json!({"1":{"characterId":1,"attribute":"happy","skillId":1}}),
        json!({"1":{"bandId":1}}),snapshot(),json!({})).unwrap();
    let mut definitions:Vec<_>=(1_000_000_001..=1_000_000_004).map(custom_definition).collect();
    let team=[1_000_000_001,1_000_000_002,1_000_000_003,1_000_000_004,1]
        .map(|card_id| TimingTeamCard{card_id,skill_level:3});
    let matched=raw.with_custom_timing_definitions(&definitions).hit_timing_team_skills(Server::Cn,&team).unwrap();
    assert!(matched[0].unification_applied);
    assert_eq!(matched[0].skill.duration,6.0);
    assert_eq!(matched[0].skill.score,ScoreEffect::Plain{percent:125.0});
    for mismatch in 0..2 {
        if mismatch==0 {definitions[3].band_id=2;}else {definitions[3].band_id=1;definitions[3].attribute=Attribute::Pure;}
        let resolved=raw.with_custom_timing_definitions(&definitions).hit_timing_team_skills(Server::Cn,&team).unwrap();
        assert!(!resolved[0].unification_applied);
        assert_eq!(resolved[0].skill.score,ScoreEffect::Plain{percent:100.0});
    }
    let mut crescendo=custom_definition(1_000_000_005);
    crescendo.skill.rateup=true; crescendo.skill.score_up.unification_activate_effect_value=None;
    let resolved=custom_skill(&crescendo,context()).unwrap();
    assert_eq!(resolved.skill.score,ScoreEffect::Crescendo{percent:100.0,step:0.5,cap:150});
    crescendo.skill.score_up.unification_activate_effect_value=Some(1.25);
    assert!(custom_skill(&crescendo,context()).is_err());
}

fn context() -> TimingSkillContext {
    TimingSkillContext {
        server: Server::Cn,
        skill_level: 5,
        common_band: None,
        common_attribute: None,
    }
}

fn resolve(id: u32, context: TimingSkillContext) -> ResolvedTimingSkill {
    resolve_full_life_skill(id, &snapshot()[id.to_string()], context).unwrap()
}

fn replay_grades(skill: ReplaySkill) -> bangdream_optimize_core::hit_timing::ReplayResult {
    replay(&ReplayInput {
        power: 4000.0 / 3.0,
        play_level: 5,
        fps: REPLAY_FPS,
        notes: [0.0, 0.5, 1.0, 1.5]
            .into_iter()
            .enumerate()
            .map(|(i, time)| ReplayNote {
                time,
                trigger: (i == 0).then_some(0),
                window: NoteWindow::Standard,
            })
            .collect(),
        hit_frames: vec![0, 60, 128, 180],
        skills: vec![skill],
        initial_combo: 0,
        medley_combo: false,
        fever_frames: None,
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    })
    .unwrap()
}

#[test]
fn server_values_do_not_use_skill_level_or_maximum() {
    let mut source = snapshot()["1"].clone();
    source["activationEffect"]["activateEffectTypes"]["score"]["activateEffectValue"] =
        json!([10, 90, 20, 40, 70]);
    for (server, expected) in [
        (Server::Jp, 10.0),
        (Server::En, 90.0),
        (Server::Tw, 20.0),
        (Server::Cn, 40.0),
        (Server::Kr, 70.0),
    ] {
        for (level, duration) in [(1, 5.0), (5, 7.0)] {
            let resolved = resolve_full_life_skill(
                1,
                &source,
                TimingSkillContext {
                    server,
                    skill_level: level,
                    ..context()
                },
            )
            .unwrap();
            assert_eq!(resolved.skill.duration, duration);
            assert!(
                matches!(resolved.skill.score, ScoreEffect::Plain { percent } if percent == expected)
            );
            assert_eq!(resolved.value_sources["score"], server);
        }
    }
}

#[test]
fn missing_server_fallback_is_explicit_and_preserves_zero() {
    let mut source = snapshot()["1"].clone();
    let path = "/activationEffect/activateEffectTypes/score/activateEffectValue";
    for (values, expected, server) in [
        (json!([50, 90, 70, 0, 80]), 0.0, Server::Cn),
        (json!([50, 90, 70, null, 80]), 50.0, Server::Jp),
        (json!([null, 90, 70, 40, 80]), 40.0, Server::Cn),
        (json!([null, 90, 70, null, 80]), 90.0, Server::En),
    ] {
        *source.pointer_mut(path).unwrap() = values;
        let ctx = TimingSkillContext {
            server: if server == Server::Cn && expected == 40.0 {
                Server::Jp
            } else {
                Server::Cn
            },
            ..context()
        };
        let resolved = resolve_full_life_skill(1, &source, ctx).unwrap();
        assert_eq!(resolved.value_sources["score"], server);
        assert!(
            matches!(resolved.skill.score, ScoreEffect::Plain { percent } if percent == expected)
        );
    }
    for invalid in [
        json!([null, null, null, null, null]),
        json!([50, 90, 70, "bad", 80]),
        json!([50, 90, 70, -1, 80]),
    ] {
        *source.pointer_mut(path).unwrap() = invalid;
        assert!(resolve_full_life_skill(1, &source, context()).is_err());
    }
    let resolved = resolve(67, context());
    assert_eq!(resolved.value_sources["judge"], Server::Jp);
    assert_eq!(resolved.value_sources["score"], Server::Jp);
    assert!(matches!(
        resolved.skill.correction,
        Some(JudgementCorrection::Bad)
    ));
}

#[test]
fn unification_preserves_perfect_requirement_and_continuous_fallback() {
    let matched = TimingSkillContext {
        common_band: Some(1),
        ..context()
    };
    let p = replay_grades(resolve(27, matched).skill);
    assert_eq!(
        (p.trace[1].score, p.trace[2].score, p.trace[3].score),
        (2475, 800, 2475)
    );
    let g = replay_grades(resolve(37, matched).skill);
    assert_eq!(
        (g.trace[1].score, g.trace[2].score, g.trace[3].score),
        (2365, 1440, 1980)
    );
    let g_unmatched = replay_grades(resolve(37, context()).skill);
    assert_eq!(
        (
            g_unmatched.trace[1].score,
            g_unmatched.trace[2].score,
            g_unmatched.trace[3].score
        ),
        (2145, 1440, 1980)
    );
    assert!(!resolve(27, context()).unification_applied);
    assert!(resolve(27, matched).unification_applied);
    let mut source = snapshot()["27"].clone();
    source["activationEffect"]["unificationActivateConditionType"] = json!("COOL");
    assert!(
        !resolve_full_life_skill(27, &source, matched)
            .unwrap()
            .unification_applied
    );
    assert!(
        resolve_full_life_skill(
            27,
            &source,
            TimingSkillContext {
                common_attribute: Some(Attribute::Cool),
                ..matched
            }
        )
        .unwrap()
        .unification_applied
    );
}

#[test]
fn great_zero_half_crescendo_and_correction_reach_the_replay() {
    for (id, great, next) in [
        (20, 800, 2420),
        (59, 0, 2475),
        (62, 400, 2475),
        (26, 1520, 2090),
        (61, 1604, 2211),
    ] {
        let result = replay_grades(resolve(id, context()).skill);
        assert_eq!(
            (result.trace[2].score, result.trace[3].score),
            (great, next),
            "skill {id}"
        );
    }
    for id in [5, 6, 7, 11, 12, 15, 16, 64, 67] {
        let result = replay_grades(resolve(id, context()).skill);
        assert_eq!(result.trace[2].raw, Judgement::Great);
        assert_eq!(result.trace[2].judgement, Judgement::Perfect);
    }
    assert!(resolve(61, context()).known_crescendo_parameters);
    assert!(resolve_full_life_skill(999, &snapshot()["61"], context()).is_err());
}

#[test]
fn full_life_only_resolves_known_life_and_survival_effects() {
    for (id, percent) in [
        (17, 65.0),
        (18, 110.0),
        (21, 40.0),
        (22, 80.0),
        (23, 10.0),
        (60, 90.0),
    ] {
        let resolved = resolve(id, context());
        assert!(resolved.requires_full_life);
        assert!(
            matches!(resolved.skill.score, ScoreEffect::Plain { percent: actual } if actual == percent)
        );
    }
    let mut source = snapshot()["21"].clone();
    source["activationEffect"]["activateEffectTypes"]["score_over_life"]["activateConditionLife"] =
        json!(1100);
    assert!(resolve_full_life_skill(21, &source, context()).is_err());
}

#[test]
fn actual_team_members_and_selected_levels_determine_unification() {
    let mut cards = json!({});
    let mut characters = json!({});
    for i in 1..=5 {
        cards[i.to_string()] = json!({"characterId": i, "attribute": "cool", "skillId": 27});
        characters[i.to_string()] = json!({"bandId": 1});
    }
    let members = std::array::from_fn(|i| TimingTeamCard {
        card_id: i as u32 + 1,
        skill_level: i as u8 + 1,
    });
    let data = BestdoriData::from_values(cards.clone(), characters.clone(), snapshot(), json!({}))
        .unwrap();
    let result = data.hit_timing_team_skills(Server::Cn, &members).unwrap();
    assert!(result.iter().all(|s| s.unification_applied));
    for (member, actual) in members.iter().zip(&result) {
        let cases = data.hit_timing_skill_cases(Server::Cn, *member).unwrap();
        assert_eq!(
            serde_json::to_value(&actual.skill).unwrap(),
            serde_json::to_value(&cases[3].skill).unwrap()
        );
        assert!(cases[1].unification_applied);
        assert!(!cases[0].unification_applied);
        assert!(!cases[2].unification_applied);
    }
    assert_eq!(result[0].skill.duration, 5.0);
    assert_eq!(result[4].skill.duration, 7.0);
    characters["5"]["bandId"] = json!(2);
    let data = BestdoriData::from_values(cards.clone(), characters.clone(), snapshot(), json!({}))
        .unwrap();
    assert!(data
        .hit_timing_team_skills(Server::Cn, &members)
        .unwrap()
        .iter()
        .all(|s| !s.unification_applied));
    for i in 1..=5 {
        cards[i.to_string()]["skillId"] = json!(34);
    }
    let data = BestdoriData::from_values(cards.clone(), characters.clone(), snapshot(), json!({}))
        .unwrap();
    assert!(data
        .hit_timing_team_skills(Server::Cn, &members)
        .unwrap()
        .iter()
        .all(|s| s.unification_applied));
    cards["5"]["attribute"] = json!("pure");
    let data = BestdoriData::from_values(cards, characters, snapshot(), json!({})).unwrap();
    for member in members {
        let cases = data.hit_timing_skill_cases(Server::Cn, member).unwrap();
        assert!(!cases[0].unification_applied);
        assert!(!cases[1].unification_applied);
        // Skill 34 requires Cool specifically; the fifth card is now Pure.
        // A uniform Pure team must not receive the Cool condition's bonus.
        assert_eq!(cases[2].unification_applied, member.card_id != 5);
        assert_eq!(cases[3].unification_applied, member.card_id != 5);
    }
    assert!(data
        .hit_timing_team_skills(Server::Cn, &members)
        .unwrap()
        .iter()
        .all(|s| !s.unification_applied));
}

#[test]
fn unknown_effects_conditions_and_incomplete_data_are_rejected() {
    for pointer in [
        "/activationEffect/activateEffectTypes/score/activateCondition",
        "/activationEffect/activateEffectTypes/score/activateEffectValueType",
    ] {
        let mut source = snapshot()["1"].clone();
        *source.pointer_mut(pointer).unwrap() = json!("unknown");
        assert!(resolve_full_life_skill(1, &source, context()).is_err());
    }
    let mut source = snapshot()["1"].clone();
    source["activationEffect"]["activateEffectTypes"]["new_score"] =
        source["activationEffect"]["activateEffectTypes"]["score"].clone();
    assert!(resolve_full_life_skill(1, &source, context()).is_err());
    let mut source = snapshot()["1"].clone();
    source["duration"] = json!([5]);
    assert!(resolve_full_life_skill(1, &source, context()).is_err());
    for skill_level in [0, 6] {
        assert!(resolve_full_life_skill(
            1,
            &snapshot()["1"],
            TimingSkillContext {
                skill_level,
                ..context()
            }
        )
        .is_err());
    }
    let mut source = snapshot()["61"].clone();
    source["activationEffect"]["activateEffectTypes"]["score"]["activateEffectValue"] =
        json!([101]);
    assert!(resolve_full_life_skill(61, &source, context()).is_err());
}

#[test]
fn complete_snapshot_resolves_and_replays_all_servers_and_levels() {
    let skills = snapshot();
    assert_eq!(skills.as_object().unwrap().len(), 69);
    for (id, source) in skills.as_object().unwrap() {
        for server in [Server::Jp, Server::Cn, Server::En, Server::Tw, Server::Kr] {
            for skill_level in 1..=5 {
                let resolved = resolve_full_life_skill(
                    id.parse().unwrap(),
                    source,
                    TimingSkillContext {
                        server,
                        skill_level,
                        ..context()
                    },
                )
                .unwrap();
                let result = replay_grades(resolved.skill);
                assert_eq!(result.fps, 120);
                assert_eq!(result.trace.len(), 4);
            }
        }
    }
}
