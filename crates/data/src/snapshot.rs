use bangdream_optimize_core::{
    AreaItemDefinition, CardDefinition, Chart, EventBonus, EventType, PreferredItemTarget,
    ScoreRangeChartMeta,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct EventData {
    pub event_type: EventType,
    pub event_bonus: EventBonus,
    pub preferred: Option<PreferredItemTarget>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GameDataSnapshot {
    pub card_definitions: BTreeMap<u32, CardDefinition>,
    pub area_item_definitions: BTreeMap<u32, AreaItemDefinition>,
    pub events: BTreeMap<u32, EventData>,
    charts: BTreeMap<(u32, u8), Chart>,
    score_range_charts: BTreeMap<(u32, u8), (i32, ScoreRangeChartMeta)>,
}

impl GameDataSnapshot {
    pub fn new(
        card_definitions: BTreeMap<u32, CardDefinition>,
        area_item_definitions: BTreeMap<u32, AreaItemDefinition>,
        events: BTreeMap<u32, EventData>,
    ) -> Self {
        Self {
            card_definitions,
            area_item_definitions,
            events,
            charts: BTreeMap::new(),
            score_range_charts: BTreeMap::new(),
        }
    }

    pub fn insert_chart(&mut self, song_id: u32, difficulty: u8, chart: Chart) {
        self.charts.insert((song_id, difficulty), chart);
    }

    pub fn chart(&self, song_id: u32, difficulty: u8) -> Option<&Chart> {
        self.charts.get(&(song_id, difficulty))
    }

    pub fn chart_selections(&self) -> Vec<bangdream_optimize_core::SongSelection> {
        self.charts
            .keys()
            .map(
                |&(song_id, difficulty)| bangdream_optimize_core::SongSelection {
                    song_id,
                    difficulty,
                },
            )
            .collect()
    }

    pub fn insert_score_range_chart(
        &mut self,
        song_id: u32,
        difficulty: u8,
        level: i32,
        meta: ScoreRangeChartMeta,
    ) {
        self.score_range_charts
            .insert((song_id, difficulty), (level, meta));
    }

    pub fn score_range_chart(
        &self,
        song_id: u32,
        difficulty: u8,
    ) -> Option<(i32, &ScoreRangeChartMeta)> {
        self.score_range_charts
            .get(&(song_id, difficulty))
            .map(|(level, meta)| (*level, meta))
    }

    pub fn score_range_chart_selections(&self) -> Vec<bangdream_optimize_core::SongSelection> {
        let source = if self.score_range_charts.is_empty() {
            self.charts.keys().copied().collect::<Vec<_>>()
        } else {
            self.score_range_charts.keys().copied().collect::<Vec<_>>()
        };
        source
            .into_iter()
            .map(
                |(song_id, difficulty)| bangdream_optimize_core::SongSelection {
                    song_id,
                    difficulty,
                },
            )
            .collect()
    }
}

pub type CalculationDataSnapshot = GameDataSnapshot;
pub type EventCalculationData = EventData;
