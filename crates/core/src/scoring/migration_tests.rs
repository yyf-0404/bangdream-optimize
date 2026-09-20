use super::*;
use crate::model::chart::ExactScoreScratch;
use crate::pt_maximize::distribution::points_for_scenario;
use crate::skill_shuffle::{self, tables};
use crate::{Chart, ChartNode, ChartNodeType, EventType, FixedTeamPtScenario, TeamCardSkill};
use std::collections::BTreeMap;

#[test]
fn physical_layout_power_maximum_probability_and_weighted_pt_match_full_replay() {
    let members = std::array::from_fn(|i| {
        MemberPower::from_bonuses(
            [
                10001 + i as u32 * 1177,
                13679 + i as u32 * 391,
                8731 + i as u32 * 199,
            ],
            [0.0; 3],
            [0.237, 0.317, 0.189],
            37.3,
            [1.25, 0.0, 0.0],
        )
        .unwrap()
    });
    let power = TeamPower::Members(members);
    let groups = power.groups();
    assert!(
        groups.len() > 1,
        "fixture must exercise layout-dependent rounding"
    );
    for queued in [false, true] {
        let interval = if queued { 4.0 } else { 10.0 };
        let mut nodes = (0..6)
            .map(|i| ChartNode {
                time: i as f64 * interval,
                node_type: ChartNodeType::Skill,
            })
            .collect::<Vec<_>>();
        nodes.extend((0..180).map(|i| ChartNode {
            time: i as f64 * 0.35 + 0.13,
            node_type: ChartNodeType::Node,
        }));
        let mut chart = Chart::new(28, nodes);
        chart.init(0, false).unwrap();
        let skills = std::array::from_fn(|i| TeamCardSkill {
            card_id: i as u32 + 1,
            duration: 5.0 + (i % 3) as f64 * 0.5,
            score_up: 0.6 + i as f64 * 0.1,
            rateup: i == 1,
        });
        let scenario = FixedTeamPtScenario::Solo {
            event_type: EventType::Challenge,
            point_bonus_basis_points: 15000,
            mission_support_pt_bonus: 0,
        };
        let mut oracle = BTreeMap::new();
        for group in &groups {
            let scores: [[i32; 120]; 5] = std::array::from_fn(|captain| {
                std::array::from_fn(|order| {
                    let six = std::array::from_fn::<_, 6, _>(|i| {
                        skills[if i == 5 {
                            captain
                        } else {
                            tables().orders[order][i]
                        }]
                    });
                    // Explicit per-note settlement, independent of compressed matrices, cached queue plans and assignment DP.
                    let scorer =
                        NoteScorer::new(group.power, chart.level, chart.count as u32).unwrap();
                    chart
                        .native_score_notes(&six, false)
                        .unwrap()
                        .iter()
                        .map(|n| {
                            scorer
                                .score_with_judgement_multiplier(
                                    n.judgement,
                                    n.combo,
                                    n.skill,
                                    n.fever,
                                )
                                .unwrap()
                                .final_score as i32
                        })
                        .sum()
                })
            });
            oracle.insert(group.power.cache_key(), scores);
        }
        let mut best_max = (0, 0_u16);
        let mut best_by_captain = [(0, 0_u16); 5];
        let mut best_pt = (0_u128, 0_i64);
        for layout in &tables().layouts {
            let row = &oracle[&power.power(layout.cards).cache_key()][layout.cards[2]];
            let maximum = layout
                .weighted_orders
                .iter()
                .map(|o| row[o.index])
                .max()
                .unwrap();
            let probability = layout
                .weighted_orders
                .iter()
                .filter(|o| row[o.index] == maximum)
                .map(|o| o.count)
                .sum::<u16>();
            best_max = best_max.max((maximum, probability));
            best_by_captain[layout.cards[2]] =
                best_by_captain[layout.cards[2]].max((maximum, probability));
            let pt = layout
                .weighted_orders
                .iter()
                .map(|o| {
                    u128::from(points_for_scenario(row[o.index], scenario).unwrap())
                        * u128::from(o.count)
                })
                .sum();
            let mean = layout
                .weighted_orders
                .iter()
                .map(|o| i64::from(row[o.index]) * i64::from(o.count))
                .sum();
            best_pt = best_pt.max((pt, mean));
        }
        let windows = skills.map(|skill| chart.compile_exact_skill_windows(skill).unwrap());
        let maximum = skill_shuffle::maximum_with_power(
            &chart,
            &skills,
            &windows,
            ([0, 1, 2, 3, 4], 2),
            &groups,
            false,
            &mut ExactScoreScratch::compressed(),
        )
        .unwrap();
        assert_eq!(maximum.score, best_max.0);
        let recommendation = skill_shuffle::recommend_max_score(
            &chart,
            &skills,
            power,
            false,
            maximum.captain_index,
        )
        .unwrap();
        assert_eq!(
            recommendation.max_score_path_count,
            best_by_captain[maximum.captain_index].1
        );
        let layout = recommendation
            .recommended_team_card_ids
            .map(|id| (id - 1) as usize);
        let order = recommendation
            .skill_order_card_ids
            .map(|id| (id - 1) as usize);
        assert_eq!(
            oracle[&power.power(layout).cache_key()][layout[2]][skill_shuffle::order_index(order)],
            best_max.0
        );
        let evaluation =
            crate::evaluate_full_team(&chart, &skills, power, false, scenario).unwrap();
        assert_eq!(
            evaluation.average_pt.pt_sum * 1024 / u128::from(evaluation.average_pt.sample_count),
            best_pt.0
        );
        assert_eq!(evaluation.score_distribution.score_sum, best_pt.1);
    }
}

#[test]
fn physical_power_plateaus_merge_only_identical_first_stage_note_scores() {
    let mut chart = Chart::new(
        28,
        (0..186)
            .map(|i| ChartNode {
                time: i as f64 * 0.5,
                node_type: if i % 31 == 0 {
                    ChartNodeType::Skill
                } else {
                    ChartNodeType::Node
                },
            })
            .collect(),
    );
    chart.init(0, false).unwrap();
    let mut merged = false;
    let mut split = false;
    for offset in 0..1500 {
        let members = std::array::from_fn(|i| {
            MemberPower::from_bonuses(
                [
                    10001 + i as u32 * 1177 + offset,
                    13679 + i as u32 * 391,
                    8731 + i as u32 * 199,
                ],
                [0.0; 3],
                [0.237, 0.317, 0.189],
                37.3,
                [1.25, 0.0, 0.0],
            )
            .unwrap()
        });
        let power = TeamPower::Members(members);
        let exact = power.groups();
        if exact.len() <= 1 {
            continue;
        }
        let groups = power.groups_for_charts(std::slice::from_ref(&chart), false);
        merged |= groups.len() == 1;
        split |= groups.len() > 1;
        for group in groups {
            let representative =
                NoteScorer::new(group.power, chart.level, chart.count as u32).unwrap();
            for &layout in group.layouts.iter() {
                let actual = NoteScorer::new(
                    power.power(tables().layouts[layout].cards),
                    chart.level,
                    chart.count as u32,
                )
                .unwrap();
                for combo in [1, 21, 51, 101, 151, 186] {
                    let rate = normal_combo_multiplier(combo);
                    assert_eq!(
                        representative
                            .score(Judgement::Perfect, rate, 1.0, 1.0)
                            .unwrap(),
                        actual.score(Judgement::Perfect, rate, 1.0, 1.0).unwrap()
                    );
                }
            }
        }
        if merged && split {
            break;
        }
    }
    assert!(
        merged && split,
        "exercise both plateau merging and integer boundary preservation"
    );
}

#[test]
fn optimized_scoring_rejects_invalid_and_overflowing_inputs() {
    let mut chart = Chart::new(
        28,
        (0..180)
            .map(|i| ChartNode {
                time: i as f64,
                node_type: if i % 30 == 0 {
                    ChartNodeType::Skill
                } else {
                    ChartNodeType::Node
                },
            })
            .collect(),
    );
    chart.init(0, false).unwrap();
    let mut skills: [TeamCardSkill; 5] = std::array::from_fn(|i| TeamCardSkill {
        card_id: i as u32,
        score_up: 1.0,
        duration: 5.0,
        rateup: false,
    });
    assert!(chart.get_max_score_order(&skills, 1.0e12, false).is_err());
    skills[0].score_up = f64::INFINITY;
    assert!(chart.get_max_score_order(&skills, 300000, false).is_err());
    skills[0].score_up = -1.0;
    assert!(chart.get_max_score_order(&skills, 300000, false).is_err());
}

#[test]
fn fractional_power_pruning_and_single_search_match_all_legal_teams() {
    use crate::medley::test_support::{prepared_card, selected_cool_items};
    use crate::{preparation::StatValue, AreaItemPercent, Attribute, SongMode};
    let cards = (0..8)
        .map(|i| {
            let mut card = prepared_card(i + 1, i % 5 + 1, 1, Attribute::Cool);
            card.stat = StatValue {
                performance: 10001.0 + i as f64 * 121.0,
                technique: 13679.0 - i as f64 * 59.0,
                visual: 8731.0 + i as f64 * 77.0,
            };
            card.skill.score_up = 0.75 + (i % 4) as f64 * 0.15;
            card.skill.duration = 5.0 + (i % 3) as f64 * 0.5;
            card.skill.rateup = i == 3;
            card.score_up.default = card.skill.score_up;
            card.native_power = Some(preparation::PreparedPowerCard {
                card_id: card.card_id,
                band_id: 1,
                attribute: Attribute::Cool,
                original: [
                    card.stat.performance as u32,
                    card.stat.technique as u32,
                    card.stat.visual as u32,
                ],
                event_percent: 37.3,
                extra_event: [0.3, 0.7, 0.0],
            });
            card
        })
        .collect::<Vec<_>>();
    let area = AreaItemPercent::empty();
    let items = selected_cool_items();
    for spacing in [10.0, 4.0] {
        let mut nodes = (0..6)
            .map(|i| ChartNode {
                time: i as f64 * spacing,
                node_type: ChartNodeType::Skill,
            })
            .collect::<Vec<_>>();
        nodes.extend((0..180).map(|i| ChartNode {
            time: i as f64 * 0.35 + 0.1,
            node_type: ChartNodeType::Node,
        }));
        let mut chart = Chart::new(28, nodes);
        chart.init(0, false).unwrap();
        let mut brute = 0;
        for mask in 0_u16..(1 << cards.len()) {
            if mask.count_ones() != 5 {
                continue;
            }
            let team = cards
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, c)| c)
                .collect::<Vec<_>>();
            if team
                .iter()
                .map(|c| c.character_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != 5
            {
                continue;
            }
            let skills = std::array::from_fn(|i| team[i].skill);
            let power = TeamPower::Members(std::array::from_fn(|i| {
                team[i].member_power(
                    &area,
                    &items.band,
                    &items.attribute,
                    items.magazine.as_str(),
                )
            }));
            for group in power.groups() {
                let scores =
                    skill_shuffle::exact_order_scores(&chart, &skills, group.power, false).unwrap();
                for (captain, row) in scores.iter().enumerate() {
                    for (order, &score) in row.iter().enumerate() {
                        if group.reachable[captain] & (1 << order) != 0 {
                            brute = brute.max(score);
                        }
                    }
                }
            }
        }
        let result =
            crate::calculate_single_song(&cards, &chart, &area, &items, SongMode::Mixed).unwrap();
        assert_eq!(result.score, brute, "spacing={spacing}");
    }
}
