use crate::{
    event_songs, initialized_charts, prepare_event_context, DataError, GameDataSnapshot,
    MaximizeInputBuilder,
};
use async_trait::async_trait;
use bangdream_optimize_core::{
    maximize_result_for_items, BuildResult, MaximizeOptions, PlayerConfig, Server,
};

#[derive(Debug, Clone)]
pub struct SnapshotMaximizeInputBuilder {
    data: GameDataSnapshot,
}

impl SnapshotMaximizeInputBuilder {
    pub fn new(data: GameDataSnapshot) -> Self {
        Self { data }
    }

    pub fn maximize_sync(
        &self,
        player: PlayerConfig,
        _server: Server,
        event_id: Option<u32>,
        mut options: MaximizeOptions,
    ) -> Result<BuildResult, DataError> {
        let context = prepare_event_context(&self.data, &player, event_id)?;
        let song_list = event_songs(&player, context.event_id)?;
        let charts = initialized_charts(&self.data, &song_list, context.event_type)?;

        if options.preferred.is_none() {
            options.preferred = context.preferred.clone();
        }

        maximize_result_for_items(
            context.event_id,
            context.event_type,
            song_list,
            context.maximize_cards(),
            &charts,
            &context.area_item_percent,
            options,
        )
        .map_err(Into::into)
    }

    pub fn calculate_result_sync(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: MaximizeOptions,
    ) -> Result<BuildResult, DataError> {
        self.maximize_sync(player, server, event_id, options)
    }
}

#[async_trait]
impl MaximizeInputBuilder for SnapshotMaximizeInputBuilder {
    async fn maximize(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: MaximizeOptions,
    ) -> Result<BuildResult, DataError> {
        self.maximize_sync(player, server, event_id, options)
    }
}

pub type SnapshotCalculationInputBuilder = SnapshotMaximizeInputBuilder;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventData, GameDataSnapshot};
    use bangdream_optimize_core::{
        preparation::StatRate as PrepStatRate, AreaItemConfig, AreaItemDefinition, Attribute,
        CardDefinition, CharacterBonusConfig, Chart, ChartNode, ChartNodeType, EventAttributeBonus,
        EventBonus, EventType, MaximizeOptions, PlayerCardConfig, ScoreUp, SkillDefinition,
        SongSelection, Stat, StatRate,
    };
    use std::collections::BTreeMap;

    fn chart() -> Chart {
        let mut nodes = Vec::new();
        for idx in 0..6 {
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: idx as f64 * 10.0,
            });
            nodes.push(ChartNode {
                node_type: ChartNodeType::Node,
                time: idx as f64 * 10.0 + 1.0,
            });
        }
        Chart::new(25, nodes)
    }

    fn card(card_id: u32, character_id: u32) -> CardDefinition {
        CardDefinition {
            card_id,
            character_id,
            band_id: 1,
            rarity: 4,
            attribute: Attribute::Cool,
            level_stats: BTreeMap::from([(
                60,
                Stat {
                    performance: 1000,
                    technique: 1000,
                    visual: 1000,
                },
            )]),
            training_stat: Stat {
                performance: 0,
                technique: 0,
                visual: 0,
            },
            episode_stats: [
                Stat {
                    performance: 0,
                    technique: 0,
                    visual: 0,
                },
                Stat {
                    performance: 0,
                    technique: 0,
                    visual: 0,
                },
            ],
            skill: SkillDefinition {
                durations: vec![5.0; 5],
                score_up: ScoreUp {
                    default: 1.0,
                    unification_activate_effect_value: None,
                    unification_activate_condition_band_id: None,
                    unification_activate_condition_type: None,
                },
                rateup: false,
            },
        }
    }

    fn snapshot() -> GameDataSnapshot {
        let card_definitions = (1..=5).map(|id| (id, card(id, id))).collect();
        let area_item_definitions = BTreeMap::from([
            (
                10,
                AreaItemDefinition {
                    area_item_id: 10,
                    target_band_ids: vec![1],
                    target_attributes: vec![],
                    percents: BTreeMap::from([(1, PrepStatRate::all(0.1))]),
                },
            ),
            (
                20,
                AreaItemDefinition {
                    area_item_id: 20,
                    target_band_ids: vec![],
                    target_attributes: vec![Attribute::Cool],
                    percents: BTreeMap::from([(1, PrepStatRate::all(0.1))]),
                },
            ),
            (
                80,
                AreaItemDefinition {
                    area_item_id: 80,
                    target_band_ids: vec![],
                    target_attributes: vec![],
                    percents: BTreeMap::from([(
                        1,
                        PrepStatRate {
                            performance: 0.1,
                            technique: 0.0,
                            visual: 0.0,
                        },
                    )]),
                },
            ),
        ]);
        let events = BTreeMap::from([(
            100,
            EventData {
                event_type: EventType::Challenge,
                event_bonus: EventBonus {
                    attributes: vec![EventAttributeBonus {
                        attribute: Attribute::Cool,
                        percent: 50.0,
                    }],
                    characters: vec![],
                    members: vec![],
                    event_character_parameter_bonus: None,
                    event_attribute_and_character_parameter_percent: 0.0,
                    event_attribute_and_character_point_percent: 0.0,
                    limit_breaks: BTreeMap::new(),
                },
                preferred: None,
            },
        )]);
        let mut snapshot = GameDataSnapshot::new(card_definitions, area_item_definitions, events);
        snapshot.insert_chart(1, 3, chart());
        snapshot
    }

    fn player() -> PlayerConfig {
        PlayerConfig {
            mongo_id: None,
            player_id: 123,
            current_event: Some(100),
            event_songs: BTreeMap::from([(
                "100".to_owned(),
                vec![SongSelection {
                    song_id: 1,
                    difficulty: 3,
                }],
            )]),
            event_presets: BTreeMap::new(),
            event_overrides: BTreeMap::new(),
            card_list: (1..=5)
                .map(|id| {
                    (
                        id.to_string(),
                        PlayerCardConfig {
                            level: 60,
                            training: true,
                            illust_training_status: true,
                            episodes: [true, true],
                            limit_break_rank: 0,
                            skill_level: 5,
                        },
                    )
                })
                .collect(),
            area_item: BTreeMap::from([
                ("10".to_owned(), AreaItemConfig { level: 1 }),
                ("20".to_owned(), AreaItemConfig { level: 1 }),
                ("80".to_owned(), AreaItemConfig { level: 1 }),
            ]),
            character_bouns: (1..=5)
                .map(|id| {
                    (
                        id.to_string(),
                        CharacterBonusConfig {
                            potential: StatRate {
                                performance: 0.0,
                                technique: 0.0,
                                visual: 0.0,
                            },
                            character_task: StatRate {
                                performance: 0.0,
                                technique: 0.0,
                                visual: 0.0,
                            },
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn challenge_maximize_applies_event_bonus_to_team_stat() {
        let builder = SnapshotMaximizeInputBuilder::new(snapshot());

        let result = builder
            .maximize_sync(player(), Server::Jp, None, MaximizeOptions::default())
            .unwrap();

        assert_eq!(result.event_id, 100);
        assert_eq!(result.event_type, EventType::Challenge);
        assert_eq!(result.songs.len(), 1);
        assert_eq!(result.songs[0].team_card_ids.len(), 5);
        assert_eq!(result.total_stat, 26_000);
        assert!(result.total_score > 0);
    }
}
