//! Offline differential check against the downloaded Bestdori chart catalog.
//! cargo run -p bangdream-optimize-data --release --example check_skill_queue -- <data-root>
use bangdream_optimize_core::{ChartNodeType, ScoreRule, SkillQueueKind, TeamCardSkill};
use bangdream_optimize_data::chart_from_bestdori;
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Instant};

fn orders() -> Vec<[usize; 5]> {
    let mut result = Vec::new();
    for a in 0..5 {
        for b in 0..5 {
            for c in 0..5 {
                for d in 0..5 {
                    for e in 0..5 {
                        let order = [a, b, c, d, e];
                        if order.iter().fold(0u8, |mask, &i| mask | (1 << i)) == 31 {
                            result.push(order);
                        }
                    }
                }
            }
        }
    }
    result
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).ok_or("pass the downloaded data directory")?);
    let songs: Value = serde_json::from_slice(&fs::read(root.join("api/songs/all.7.json"))?)?;
    let names = ["easy", "normal", "hard", "expert", "special"];
    let orders = orders();
    let mut parsed = 0;
    let mut irregular = Vec::new();
    let mut comparisons = 0;
    let mut chained = 0;
    let mut state_counts = Vec::new();
    let mut fast_time = std::time::Duration::ZERO;
    let mut brute_time = std::time::Duration::ZERO;
    for (id, song) in songs.as_object().ok_or("invalid songs catalog")? {
        let Some(difficulties) = song["difficulty"].as_object() else {
            continue;
        };
        for (difficulty, info) in difficulties {
            let d: usize = difficulty.parse()?;
            let data: Value = serde_json::from_slice(&fs::read(
                root.join(format!("api/charts/{id}/{}.json", names[d])),
            )?)?;
            let original = chart_from_bestdori(
                info["playLevel"].as_i64().ok_or("missing level")? as i32,
                &data,
            )?;
            parsed += 1;
            if original
                .nodes
                .iter()
                .filter(|n| n.node_type == ChartNodeType::Skill)
                .count()
                != 6
            {
                irregular.push(format!("{id}/{}", names[d]));
                continue;
            }
            for auto in [false, true] {
                let mut chart = original.clone();
                chart.init_with_rule(
                    0,
                    true,
                    if auto {
                        ScoreRule::auto_with_base_multiplier(0.75)
                    } else {
                        ScoreRule::STANDARD
                    },
                )?;
                for (profile, durations) in [
                    [5.0, 5.5, 6.0, 6.5, 7.0],
                    [7.5, 7.0, 6.8, 6.2, 3.0],
                    [8.0, 7.5, 7.2, 5.0, 6.0],
                ]
                .into_iter()
                .enumerate()
                {
                    let kind = chart.skill_queue_kind(durations.into_iter().fold(0.0, f64::max));
                    match kind {
                        SkillQueueKind::None => continue,
                        SkillQueueKind::Chain => {
                            chained += 1;
                            if profile == 2 {
                                state_counts.push(json!({"songId":id,"difficulty":d,"auto":auto,"states":chart.queue_state_counts()}));
                            }
                        }
                        SkillQueueKind::Single => {}
                    }
                    let skills: [TeamCardSkill; 5] = std::array::from_fn(|i| TeamCardSkill {
                        card_id: i as u32 + 1,
                        duration: durations[i],
                        score_up: [0.8, 1.0, 1.65, 1.0, 1.3][i],
                        rateup: profile == 0 && (i == 1 || i == 3),
                    });
                    let start = Instant::now();
                    let fast = chart.get_max_score_order(&skills, 280001, true)?;
                    fast_time += start.elapsed();
                    let start = Instant::now();
                    let mut brute = i32::MIN;
                    let mut brute_order = [0; 5];
                    let mut brute_captain = 0;
                    for order in &orders {
                        for captain in 0..5 {
                            let activations: [TeamCardSkill; 6] = std::array::from_fn(|p| {
                                skills[if p == 5 { captain } else { order[p] }]
                            });
                            let score = chart.get_score(&activations, 280001, true)?;
                            // Orders/captains are enumerated lexicographically.
                            if score > brute {
                                brute = score;
                                brute_order = *order;
                                brute_captain = captain;
                            }
                        }
                    }
                    brute_time += start.elapsed();
                    assert_eq!(
                        fast.score, brute,
                        "{id}/{} auto={auto} profile={profile}",
                        names[d]
                    );
                    if kind == SkillQueueKind::Single {
                        assert_eq!(
                            (fast.order_indices.as_slice(), fast.captain_index),
                            (brute_order.as_slice(), brute_captain),
                            "order/captain: {id}/{} auto={auto} profile={profile}",
                            names[d]
                        );
                    }
                    let selected: [TeamCardSkill; 6] = std::array::from_fn(|p| {
                        skills[if p == 5 {
                            fast.captain_index
                        } else {
                            fast.order_indices[p]
                        }]
                    });
                    assert_eq!(chart.get_score(&selected, 280001, true)?, brute);
                    comparisons += 1;
                }
            }
        }
    }
    let report = json!({"parsedCharts":parsed,"irregularSkillCount":irregular,
        "singleQueueCases":comparisons-chained,"chainQueueCases":chained,"strictOrdersChecked":comparisons*600,
        "ordersAndCaptainsChecked":true,
        "chainStateCounts":state_counts,
        "fastScoreMs":fast_time.as_secs_f64()*1000.0,"bruteScoreMs":brute_time.as_secs_f64()*1000.0});
    let text = serde_json::to_string_pretty(&report)?;
    if let Some(path) = args.get(2) {
        fs::write(path, &text)?;
    }
    println!("{text}");
    Ok(())
}
