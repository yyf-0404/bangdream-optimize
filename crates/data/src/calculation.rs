use crate::{CalculationInputBuilder, DataError};
use async_trait::async_trait;
use bangdream_optimize_core::{
    calculate_area_item_percent, calculate_best_result_for_items, AreaItemDefinition, BuildResult,
    CardDefinition, Chart, EventBonus, EventType, ItemSearchOptions, PlayerConfig,
    PreferredItemTarget, Server, SongSelection,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct EventCalculationData {
    pub event_type: EventType,
    pub event_bonus: EventBonus,
    pub preferred: Option<PreferredItemTarget>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalculationDataSnapshot {
    pub card_definitions: BTreeMap<u32, CardDefinition>,
    pub area_item_definitions: BTreeMap<u32, AreaItemDefinition>,
    pub events: BTreeMap<u32, EventCalculationData>,
    charts: BTreeMap<(u32, u8), Chart>,
}

impl CalculationDataSnapshot {
    pub fn new(
        card_definitions: BTreeMap<u32, CardDefinition>,
        area_item_definitions: BTreeMap<u32, AreaItemDefinition>,
        events: BTreeMap<u32, EventCalculationData>,
    ) -> Self {
        Self {
            card_definitions,
            area_item_definitions,
            events,
            charts: BTreeMap::new(),
        }
    }

    pub fn insert_chart(&mut self, song_id: u32, difficulty: u8, chart: Chart) {
        self.charts.insert((song_id, difficulty), chart);
    }

    pub fn chart(&self, song_id: u32, difficulty: u8) -> Option<&Chart> {
        self.charts.get(&(song_id, difficulty))
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotCalculationInputBuilder {
    data: CalculationDataSnapshot,
}

impl SnapshotCalculationInputBuilder {
    pub fn new(data: CalculationDataSnapshot) -> Self {
        Self { data }
    }

    pub fn calculate_result_sync(
        &self,
        player: PlayerConfig,
        _server: Server,
        event_id: Option<u32>,
        mut options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        let event_id = event_id
            .or(player.current_event)
            .ok_or(DataError::MissingCurrentEvent)?;
        let event = self
            .data
            .events
            .get(&event_id)
            .ok_or(DataError::MissingEntity {
                kind: "event",
                id: event_id.to_string(),
            })?;
        let song_list = event_songs(&player, event_id)?;
        let card_definitions = player_card_definitions(&player, &self.data)?;
        let cards = bangdream_optimize_core::prepare_cards(
            &card_definitions,
            &player.card_list,
            &player.character_bouns,
            &event.event_bonus,
        )?;
        let area_item_percent =
            calculate_area_item_percent(&player.area_item, &self.data.area_item_definitions)?;
        let charts = initialized_charts(&self.data, &song_list, event.event_type)?;

        if options.preferred.is_none() {
            options.preferred = event.preferred.clone();
        }

        calculate_best_result_for_items(
            event_id,
            event.event_type,
            song_list,
            &cards,
            &charts,
            &area_item_percent,
            options,
        )
        .map_err(Into::into)
    }
}

#[async_trait]
impl CalculationInputBuilder for SnapshotCalculationInputBuilder {
    async fn calculate_result(
        &self,
        player: PlayerConfig,
        server: Server,
        event_id: Option<u32>,
        options: ItemSearchOptions,
    ) -> Result<BuildResult, DataError> {
        self.calculate_result_sync(player, server, event_id, options)
    }
}

fn event_songs(player: &PlayerConfig, event_id: u32) -> Result<Vec<SongSelection>, DataError> {
    player
        .event_songs
        .get(&event_id.to_string())
        .cloned()
        .ok_or(DataError::MissingEventSongs { event_id })
}

fn player_card_definitions(
    player: &PlayerConfig,
    data: &CalculationDataSnapshot,
) -> Result<Vec<CardDefinition>, DataError> {
    player
        .card_list
        .keys()
        .map(|card_id| {
            let parsed_id = card_id
                .parse::<u32>()
                .map_err(|_| DataError::InvalidField {
                    field: "cardList.cardId",
                    value: card_id.clone(),
                })?;
            data.card_definitions
                .get(&parsed_id)
                .cloned()
                .ok_or(DataError::MissingEntity {
                    kind: "card",
                    id: card_id.clone(),
                })
        })
        .collect()
}

fn initialized_charts(
    data: &CalculationDataSnapshot,
    song_list: &[SongSelection],
    event_type: EventType,
) -> Result<Vec<Chart>, DataError> {
    let is_medley = event_type == EventType::Medley;
    let mut combo = 0;
    let mut charts = Vec::with_capacity(song_list.len());

    for song in song_list {
        let mut chart =
            data.chart(song.song_id, song.difficulty)
                .cloned()
                .ok_or(DataError::MissingEntity {
                    kind: "chart",
                    id: format!("{}:{}", song.song_id, song.difficulty),
                })?;
        chart.init(combo, is_medley)?;
        combo += chart.count as i32;
        charts.push(chart);
    }

    Ok(charts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bangdream_optimize_core::{
        preparation::StatRate as PrepStatRate, AreaItemConfig, Attribute, CharacterBonusConfig,
        ChartNode, ChartNodeType, PlayerCardConfig, ScoreUp, SkillDefinition, Stat, StatRate,
    };

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

    fn snapshot() -> CalculationDataSnapshot {
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
            EventCalculationData {
                event_type: EventType::Challenge,
                event_bonus: EventBonus {
                    attributes: vec![],
                    characters: vec![],
                    members: vec![],
                    event_character_parameter_bonus: None,
                    event_attribute_and_character_parameter_percent: 0.0,
                    limit_breaks: BTreeMap::new(),
                },
                preferred: None,
            },
        )]);
        let mut snapshot =
            CalculationDataSnapshot::new(card_definitions, area_item_definitions, events);
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
    fn calculates_result_from_snapshot_and_player_config() {
        let builder = SnapshotCalculationInputBuilder::new(snapshot());

        let result = builder
            .calculate_result_sync(player(), Server::Jp, None, ItemSearchOptions::default())
            .unwrap();

        assert_eq!(result.event_id, 100);
        assert_eq!(result.event_type, EventType::Challenge);
        assert_eq!(result.songs.len(), 1);
        assert_eq!(result.songs[0].team_card_ids.len(), 5);
        assert!(result.total_score > 0);
    }
}
