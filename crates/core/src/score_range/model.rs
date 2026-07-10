use crate::{
    AreaItemPercent, Chart, CompressedAutoScore, EventType, PreparedCard, SelectedAreaItems,
    Server, SongMode, SongSelection, TeamCardSkill,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FIRE_MULTIPLIERS: [u32; 4] = [1, 5, 10, 15];
pub const SCORE_RANGE_CHART_META_SCHEMA_VERSION: u32 = 1;
pub const SCORE_RANGE_CHART_META_PATH: &str = "api/scoreRangeChartMeta.1.json";
pub const SCORE_RANGE_SKILL_DURATIONS_MILLIS: [i32; 17] = [
    3000, 3500, 4000, 4500, 5000, 5500, 5600, 5700, 6000, 6200, 6400, 6500, 6800, 7000, 7200, 7500,
    8000,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongKey {
    pub song_id: u32,
    pub difficulty: u8,
}

impl From<SongSelection> for SongKey {
    fn from(value: SongSelection) -> Self {
        Self {
            song_id: value.song_id,
            difficulty: value.difficulty,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreRangeDurationTemplate {
    pub inactive_nodes: u32,
    pub active_nodes: [u32; 6],
    pub tail_risk: bool,
}

impl ScoreRangeDurationTemplate {
    pub fn active_node_count(self) -> u32 {
        self.active_nodes
            .into_iter()
            .fold(0_u32, u32::saturating_add)
    }

    pub fn node_count(self) -> u32 {
        self.inactive_nodes.saturating_add(self.active_node_count())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScoreRangeChartMeta {
    templates: Vec<ScoreRangeDurationTemplate>,
}

impl ScoreRangeChartMeta {
    pub fn from_chart(mut chart: Chart) -> Result<Self, crate::ChartError> {
        chart.init_auto()?;
        if chart.skill_node_count() != 6 {
            return Ok(Self {
                templates: Vec::new(),
            });
        }

        let mut templates = Vec::with_capacity(SCORE_RANGE_SKILL_DURATIONS_MILLIS.len());
        for duration_millis in SCORE_RANGE_SKILL_DURATIONS_MILLIS {
            let counts = chart
                .score_range_skill_window_counts(duration_millis as f64 / 1000.0)?
                .expect("six-skill chart produces window counts");
            templates.push(ScoreRangeDurationTemplate {
                inactive_nodes: counts.inactive_nodes,
                active_nodes: counts.active_nodes,
                tail_risk: counts.tail_risk,
            });
        }
        Ok(Self { templates })
    }

    pub fn is_searchable(&self) -> bool {
        self.templates.len() == SCORE_RANGE_SKILL_DURATIONS_MILLIS.len()
    }

    pub fn template(&self, duration_millis: i32) -> Option<ScoreRangeDurationTemplate> {
        let index = SCORE_RANGE_SKILL_DURATIONS_MILLIS
            .binary_search(&duration_millis)
            .ok()?;
        self.templates.get(index).copied()
    }

    fn validate(&self) -> Result<(), String> {
        if !self.is_searchable() {
            return Err(format!(
                "expected {} duration templates, got {}",
                SCORE_RANGE_SKILL_DURATIONS_MILLIS.len(),
                self.templates.len()
            ));
        }
        let Some(first) = self.templates.first().copied() else {
            return Err("duration templates are empty".to_owned());
        };
        let node_count = first.node_count();
        if node_count == 0 {
            return Err("duration template node count is zero".to_owned());
        }
        if self
            .templates
            .iter()
            .any(|template| template.node_count() != node_count)
        {
            return Err("duration templates disagree on node count".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreRangeChartMetaFile {
    pub schema_version: u32,
    pub durations_millis: Vec<i32>,
    charts: BTreeMap<String, ScoreRangeChartMeta>,
}

impl Default for ScoreRangeChartMetaFile {
    fn default() -> Self {
        Self::new()
    }
}

impl ScoreRangeChartMetaFile {
    pub fn new() -> Self {
        Self {
            schema_version: SCORE_RANGE_CHART_META_SCHEMA_VERSION,
            durations_millis: SCORE_RANGE_SKILL_DURATIONS_MILLIS.to_vec(),
            charts: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, song_id: u32, difficulty: u8, meta: ScoreRangeChartMeta) {
        self.charts
            .insert(score_range_chart_key(song_id, difficulty), meta);
    }

    pub fn chart(&self, song_id: u32, difficulty: u8) -> Option<&ScoreRangeChartMeta> {
        self.charts.get(&score_range_chart_key(song_id, difficulty))
    }

    pub fn contains_chart(&self, song_id: u32, difficulty: u8) -> bool {
        self.chart(song_id, difficulty).is_some()
    }

    pub fn remove(&mut self, song_id: u32, difficulty: u8) {
        self.charts
            .remove(&score_range_chart_key(song_id, difficulty));
    }

    pub fn chart_count(&self) -> usize {
        self.charts.len()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCORE_RANGE_CHART_META_SCHEMA_VERSION {
            return Err(format!(
                "unsupported schema version {}, expected {}",
                self.schema_version, SCORE_RANGE_CHART_META_SCHEMA_VERSION
            ));
        }
        if self.durations_millis != SCORE_RANGE_SKILL_DURATIONS_MILLIS {
            return Err("duration list does not match the score-range schema".to_owned());
        }
        for (key, meta) in &self.charts {
            meta.validate()
                .map_err(|error| format!("chart {key}: {error}"))?;
        }
        Ok(())
    }
}

fn score_range_chart_key(song_id: u32, difficulty: u8) -> String {
    format!("{song_id}:{difficulty}")
}

#[derive(Debug, Clone)]
pub struct ScoreRangeSong {
    pub selection: SongSelection,
    pub level: i32,
    pub meta: ScoreRangeChartMeta,
    pub auto_base_multiplier: f64,
}

impl ScoreRangeSong {
    pub fn new(selection: SongSelection, chart: Chart) -> Result<Self, crate::ChartError> {
        Self::new_with_base_multiplier(selection, chart, 0.5)
    }

    pub fn new_for_server(
        selection: SongSelection,
        chart: Chart,
        server: Server,
    ) -> Result<Self, crate::ChartError> {
        Self::new_with_base_multiplier(selection, chart, auto_base_multiplier(server))
    }

    pub fn new_with_base_multiplier(
        selection: SongSelection,
        chart: Chart,
        auto_base_multiplier: f64,
    ) -> Result<Self, crate::ChartError> {
        let level = chart.level;
        let meta = ScoreRangeChartMeta::from_chart(chart)?;
        Ok(Self {
            selection,
            level,
            meta,
            auto_base_multiplier,
        })
    }

    pub fn from_meta(
        selection: SongSelection,
        level: i32,
        meta: ScoreRangeChartMeta,
        server: Server,
    ) -> Self {
        Self::from_meta_with_base_multiplier(selection, level, meta, auto_base_multiplier(server))
    }

    pub fn from_meta_with_base_multiplier(
        selection: SongSelection,
        level: i32,
        meta: ScoreRangeChartMeta,
        auto_base_multiplier: f64,
    ) -> Self {
        Self {
            selection,
            level,
            meta,
            auto_base_multiplier,
        }
    }

    pub fn key(&self) -> SongKey {
        self.selection.clone().into()
    }

    pub fn compressed_score(
        &self,
        skill: TeamCardSkill,
    ) -> Result<CompressedAutoScore, crate::ChartError> {
        Ok(self
            .duration_model((skill.duration * 1000.0).round() as i32)?
            .compressed_score(skill.score_up, skill.rateup))
    }

    pub fn is_safe_for_duration(&self, duration: f64) -> Result<bool, crate::ChartError> {
        Ok(!self
            .duration_model((duration * 1000.0).round() as i32)?
            .template
            .tail_risk)
    }

    pub(crate) fn optimistic_auto_score_terms(&self) -> Result<(f64, usize), crate::ChartError> {
        self.auto_score_terms()
    }

    pub(crate) fn pessimistic_auto_score_terms(&self) -> Result<(f64, usize), crate::ChartError> {
        self.auto_score_terms()
    }

    pub(crate) fn duration_model(
        &self,
        duration_millis: i32,
    ) -> Result<ScoreRangeSongDuration, crate::ChartError> {
        let template = self.meta.template(duration_millis).ok_or(
            crate::ChartError::UnsupportedSkillDuration {
                duration: duration_millis as f64 / 1000.0,
            },
        )?;
        let node_count = template.node_count();
        if node_count == 0 {
            return Err(crate::ChartError::EmptyChart);
        }
        Ok(ScoreRangeSongDuration {
            base_factor: auto_score_base_factor(self.level, node_count, self.auto_base_multiplier),
            template,
        })
    }

    fn auto_score_terms(&self) -> Result<(f64, usize), crate::ChartError> {
        let model = self.duration_model(SCORE_RANGE_SKILL_DURATIONS_MILLIS[0])?;
        Ok((model.base_factor, model.template.node_count() as usize))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScoreRangeSongDuration {
    base_factor: f64,
    template: ScoreRangeDurationTemplate,
}

impl ScoreRangeSongDuration {
    pub(crate) fn has_skill_tail_risk(self) -> bool {
        self.template.tail_risk
    }

    pub(crate) fn score(self, stat: i32, score_up: f64) -> i32 {
        self.compressed_score(score_up, false).score(stat)
    }

    pub(crate) fn compressed_score(self, score_up: f64, rateup: bool) -> CompressedAutoScore {
        CompressedAutoScore::from_score_range_counts(
            self.base_factor,
            self.template.inactive_nodes,
            self.template.active_nodes,
            score_up,
            rateup,
        )
    }
}

fn auto_score_base_factor(level: i32, node_count: u32, base_multiplier: f64) -> f64 {
    3.0 * (1.0 + 0.01 * (level as f64 - 5.0)) / node_count as f64 * base_multiplier
}

pub const fn auto_base_multiplier(server: Server) -> f64 {
    match server {
        Server::Jp => 0.75,
        Server::En | Server::Tw | Server::Cn | Server::Kr => 0.5,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SkillBucketKey {
    pub duration_millis: i32,
    pub score_up_millionths: i64,
    pub rateup: bool,
}

impl SkillBucketKey {
    pub fn from_skill(skill: TeamCardSkill) -> Self {
        Self {
            duration_millis: (skill.duration * 1000.0).round() as i32,
            score_up_millionths: (skill.score_up * 1_000_000.0).round() as i64,
            rateup: skill.rateup,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScoreRangeTeam {
    pub card_ids: [u32; 5],
    pub stat: i32,
    pub skill: TeamCardSkill,
    pub point_bonus_basis_points: u32,
    pub items: SelectedAreaItems,
    pub(crate) recovery_mode: Option<SongMode>,
}

#[derive(Debug, Clone)]
pub struct ScoreRangeTeamDomain {
    pub teams: Vec<ScoreRangeTeam>,
    pub(crate) recovery: TeamRecoveryData,
}

#[derive(Debug, Clone)]
pub(crate) struct TeamRecoveryData {
    pub cards: Vec<PreparedCard>,
    pub area_item_percent: AreaItemPercent,
    pub point_bonus_micros: BTreeMap<u32, u64>,
    pub items: Vec<SelectedAreaItems>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreRangeRequest {
    pub event_type: EventType,
    pub current_pt: u64,
    pub target_total_pt: u64,
    #[serde(default)]
    pub auto_base_multiplier: Option<f64>,
    #[serde(default)]
    pub mission_support_pt_bonus: Option<u64>,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

fn default_max_results() -> usize {
    20
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreRangePlay {
    pub song_id: u32,
    pub difficulty: u8,
    pub fire_multiplier: u32,
    pub score: i32,
    pub pt: u64,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreRangeResult {
    pub event_type: EventType,
    pub target_delta_pt: u64,
    pub team_card_ids: Vec<u32>,
    pub total_stat: i32,
    pub point_bonus_basis_points: u32,
    pub items: SelectedAreaItems,
    pub play_count: u32,
    pub distinct_song_count: usize,
    pub total_fire_cost: u32,
    pub total_fire_multiplier: u32,
    pub plays: Vec<ScoreRangePlay>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChartNode, ChartNodeType};

    #[test]
    fn auto_base_multiplier_depends_on_server() {
        assert_eq!(auto_base_multiplier(Server::Jp), 0.75);
        for server in [Server::En, Server::Tw, Server::Cn, Server::Kr] {
            assert_eq!(auto_base_multiplier(server), 0.5);
        }
    }

    #[test]
    fn jp_song_uses_three_quarter_auto_base() {
        let chart = Chart::new(
            5,
            (0..6)
                .map(|index| ChartNode {
                    node_type: ChartNodeType::Skill,
                    time: index as f64 * 10.0,
                })
                .collect(),
        );
        let selection = SongSelection {
            song_id: 1,
            difficulty: 3,
        };
        let jp =
            ScoreRangeSong::new_for_server(selection.clone(), chart.clone(), Server::Jp).unwrap();
        let en = ScoreRangeSong::new_for_server(selection, chart, Server::En).unwrap();
        let skill = TeamCardSkill {
            card_id: 1,
            duration: 5.0,
            score_up: 0.0,
            rateup: false,
        };

        assert_eq!(jp.compressed_score(skill).unwrap().score(600), 1_350);
        assert_eq!(en.compressed_score(skill).unwrap().score(600), 900);
    }
}
