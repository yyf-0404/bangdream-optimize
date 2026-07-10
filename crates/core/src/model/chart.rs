use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use thiserror::Error;

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

const EPS: f64 = 0.001;
const SKILL_DURATIONS: [f64; 17] = [
    3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 5.6, 5.7, 6.0, 6.2, 6.4, 6.5, 6.8, 7.0, 7.2, 7.5, 8.0,
];
const RATEUP_DURATIONS: [f64; 5] = [5.0, 5.5, 6.0, 6.5, 7.0];

#[derive(Debug, Error)]
pub enum ChartError {
    #[error("chart count must be positive")]
    EmptyChart,

    #[error("team must contain exactly five cards, got {count}")]
    InvalidTeamSize { count: usize },

    #[error("score calculation needs skill profile at activation {activation}")]
    MissingSkillProfile { activation: usize },

    #[error("chart is missing skill meta for activation {activation}")]
    MissingSkillMeta { activation: usize },

    #[error("duration {duration} is not supported by chart meta")]
    UnsupportedSkillDuration { duration: f64 },

    #[error("compressed Auto scoring requires one common base factor for every chart node")]
    NonUniformAutoBaseFactor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartNodeType {
    Node,
    Skill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComboMode {
    Standard,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimultaneousSkillOrder {
    BeforeNotes,
    AfterNotes,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreRule {
    pub base_multiplier: f64,
    pub combo_mode: ComboMode,
    pub simultaneous_skill_order: SimultaneousSkillOrder,
}

impl ScoreRule {
    pub const STANDARD: Self = Self {
        base_multiplier: 1.1,
        combo_mode: ComboMode::Standard,
        simultaneous_skill_order: SimultaneousSkillOrder::BeforeNotes,
    };

    pub const AUTO: Self = Self {
        base_multiplier: 0.5,
        combo_mode: ComboMode::None,
        simultaneous_skill_order: SimultaneousSkillOrder::AfterNotes,
    };

    pub const fn auto_with_base_multiplier(base_multiplier: f64) -> Self {
        Self {
            base_multiplier,
            combo_mode: ComboMode::None,
            simultaneous_skill_order: SimultaneousSkillOrder::AfterNotes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoMultiplierGroup {
    pub multiplier: f64,
    pub count: usize,
}

/// Exact compressed scorer for an Auto chart and one resolved skill bucket.
///
/// Auto has no combo multiplier, so every scoring node has the same base factor. Nodes are
/// grouped by the skill multiplier active when they score; evaluating a stat then only needs one
/// floor per distinct multiplier instead of one per chart node.
#[derive(Debug, Clone, PartialEq)]
pub struct CompressedAutoScore {
    base_factor: f64,
    groups: Vec<AutoMultiplierGroup>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct NonRateupAutoScoreTemplate {
    base_factor: f64,
    inactive_count: usize,
    active_count: usize,
    tail_risk: bool,
}

struct NonRateupTimeline {
    #[allow(dead_code)]
    active_count: usize,
    effective_starts: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScoreRangeSkillWindowCounts {
    pub inactive_nodes: u32,
    pub active_nodes: [u32; 6],
    pub tail_risk: bool,
}

#[cfg(test)]
impl NonRateupAutoScoreTemplate {
    pub(crate) fn has_skill_tail_risk(self) -> bool {
        self.tail_risk
    }

    pub(crate) fn score(self, stat: i32, score_up: f64) -> i32 {
        let no_skill = (stat.max(0) as f64 * self.base_factor).floor();
        let inactive = (no_skill as i64).saturating_mul(self.inactive_count as i64);
        let active =
            ((no_skill * (1.0 + score_up)).floor() as i64).saturating_mul(self.active_count as i64);
        inactive
            .saturating_add(active)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }

    pub(crate) fn with_score_up(self, score_up: f64) -> CompressedAutoScore {
        let active_multiplier = 1.0 + score_up;
        let groups = if active_multiplier.to_bits() == 1.0_f64.to_bits() {
            vec![AutoMultiplierGroup {
                multiplier: 1.0,
                count: self.inactive_count.saturating_add(self.active_count),
            }]
        } else {
            let mut groups = Vec::with_capacity(2);
            if self.inactive_count != 0 {
                groups.push(AutoMultiplierGroup {
                    multiplier: 1.0,
                    count: self.inactive_count,
                });
            }
            if self.active_count != 0 {
                groups.push(AutoMultiplierGroup {
                    multiplier: active_multiplier,
                    count: self.active_count,
                });
            }
            groups
        };
        CompressedAutoScore {
            base_factor: self.base_factor,
            groups,
        }
    }
}

impl CompressedAutoScore {
    pub(crate) fn from_score_range_counts(
        base_factor: f64,
        inactive_nodes: u32,
        active_nodes: [u32; 6],
        score_up: f64,
        rateup: bool,
    ) -> Self {
        let mut grouped = BTreeMap::<u64, AutoMultiplierGroup>::new();
        let mut add_group = |multiplier: f64, count: u32| {
            if count == 0 {
                return;
            }
            grouped
                .entry(multiplier.to_bits())
                .and_modify(|group| group.count = group.count.saturating_add(count as usize))
                .or_insert(AutoMultiplierGroup {
                    multiplier,
                    count: count as usize,
                });
        };

        add_group(1.0, inactive_nodes);
        if rateup {
            for active_count in active_nodes {
                let mut multiplier = 1.0 + score_up;
                for _ in 0..active_count {
                    if sgn(multiplier - 2.5) < 0 {
                        multiplier += 0.005;
                    }
                    add_group(multiplier, 1);
                }
            }
        } else {
            add_group(
                1.0 + score_up,
                active_nodes.into_iter().fold(0_u32, u32::saturating_add),
            );
        }

        Self {
            base_factor,
            groups: grouped.into_values().collect(),
        }
    }

    pub fn score(&self, stat: i32) -> i32 {
        let no_skill = (stat.max(0) as f64 * self.base_factor).floor();
        self.groups
            .iter()
            .fold(0_i64, |total, group| {
                total.saturating_add(
                    ((no_skill * group.multiplier).floor() as i64)
                        .saturating_mul(group.count as i64),
                )
            })
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }

    pub fn groups(&self) -> &[AutoMultiplierGroup] {
        &self.groups
    }

    pub(crate) fn lower_bound_stat(&self, target: u64) -> Option<i32> {
        if target > i32::MAX as u64 {
            return None;
        }
        let target = target as i32;
        if target <= self.score(0) {
            return Some(0);
        }

        let group_count = self
            .groups
            .iter()
            .fold(0_usize, |count, group| count.saturating_add(group.count));
        let estimated_no_skill = (target as u64)
            .div_ceil(group_count.max(1) as u64)
            .saturating_add(1);
        let estimate = if self.base_factor.is_finite() && self.base_factor > 0.0 {
            (estimated_no_skill as f64 / self.base_factor).ceil()
        } else {
            i32::MAX as f64
        };
        let mut high = estimate.clamp(1.0, i32::MAX as f64) as i32;
        while self.score(high) < target {
            if high == i32::MAX {
                return None;
            }
            high = high.saturating_mul(2).max(high.saturating_add(1));
        }

        let mut low = 0_i32;
        while low < high {
            let middle = low + (high - low) / 2;
            if self.score(middle) >= target {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        Some(low)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartNode {
    pub node_type: ChartNodeType,
    pub time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamCardSkill {
    pub card_id: u32,
    pub duration: f64,
    pub score_up: f64,
    #[serde(default)]
    pub rateup: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillWarning {
    pub id: usize,
    pub time_gap: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartMeta {
    pub no_skill: f64,
    pub skill: Vec<BTreeMap<i32, f64>>,
    pub rateup: Vec<BTreeMap<i32, f64>>,
}

impl Default for ChartMeta {
    fn default() -> Self {
        Self {
            no_skill: 0.0,
            skill: Vec::new(),
            rateup: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaxMetaOrder {
    pub meta: f64,
    pub order_indices: Vec<usize>,
    pub captain_index: usize,
    pub score_up_order: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    pub nodes: Vec<ChartNode>,
    pub level: i32,
    pub count: usize,
    pub combo: i32,
    pub score_as_medley: bool,
    pub warning: Vec<SkillWarning>,
    pub meta: ChartMeta,
    score_rule: ScoreRule,
    score_factors: Vec<f64>,
}

impl Chart {
    pub fn new(level: i32, mut nodes: Vec<ChartNode>) -> Self {
        nodes.sort_by(|a, b| {
            let time_order = sgn(a.time - b.time);
            if time_order != 0 {
                return time_order.cmp(&0);
            }
            node_sort_value(a.node_type).cmp(&node_sort_value(b.node_type))
        });
        let count = nodes.len();
        Self {
            nodes,
            level,
            count,
            combo: 0,
            score_as_medley: false,
            warning: Vec::new(),
            meta: ChartMeta::default(),
            score_rule: ScoreRule::STANDARD,
            score_factors: Vec::new(),
        }
    }

    pub fn init(&mut self, combo: i32, is_medley: bool) -> Result<(), ChartError> {
        self.init_with_rule(combo, is_medley, ScoreRule::STANDARD)
    }

    pub fn init_auto(&mut self) -> Result<(), ChartError> {
        self.init_with_rule(0, false, ScoreRule::AUTO)
    }

    pub fn init_auto_with_base_multiplier(
        &mut self,
        base_multiplier: f64,
    ) -> Result<(), ChartError> {
        self.init_with_rule(
            0,
            false,
            ScoreRule::auto_with_base_multiplier(base_multiplier),
        )
    }

    pub fn skill_node_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.node_type == ChartNodeType::Skill)
            .count()
    }

    pub(crate) fn score_range_skill_window_counts(
        &self,
        duration: f64,
    ) -> Result<Option<ScoreRangeSkillWindowCounts>, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }
        if self.skill_node_count() != 6 {
            return Ok(None);
        }

        #[derive(Clone, Copy)]
        struct WindowEvent {
            time: f64,
            active_window: Option<usize>,
        }

        let mut inactive_nodes = 0_u32;
        let mut active_nodes = [0_u32; 6];
        let mut active_window = None;
        let mut skill_count = 0_usize;
        let mut events = VecDeque::<WindowEvent>::new();
        let mut effective_starts = Vec::with_capacity(6);

        for node in &self.nodes {
            if events
                .front()
                .is_some_and(|event| sgn(node.time - event.time) > 0)
            {
                active_window = events
                    .pop_front()
                    .expect("front event exists")
                    .active_window;
            }

            if let Some(window) = active_window {
                active_nodes[window] = active_nodes[window].saturating_add(1);
            } else {
                inactive_nodes = inactive_nodes.saturating_add(1);
            }

            if node.node_type != ChartNodeType::Skill {
                continue;
            }

            let window = skill_count;
            skill_count += 1;
            if !events.is_empty() {
                let start_time = events.back().expect("back event exists").time + 0.75;
                effective_starts.push(start_time);
                events.push_back(WindowEvent {
                    time: start_time,
                    active_window: Some(window),
                });
                events.push_back(WindowEvent {
                    time: start_time + duration + 1.0 / 30.0,
                    active_window: None,
                });
            } else {
                active_window = Some(window);
                effective_starts.push(node.time);
                events.push_back(WindowEvent {
                    time: node.time + duration + 1.0 / 30.0,
                    active_window: None,
                });
            }
        }

        let timeline = NonRateupTimeline {
            active_count: active_nodes.into_iter().map(|count| count as usize).sum(),
            effective_starts,
        };
        Ok(Some(ScoreRangeSkillWindowCounts {
            inactive_nodes,
            active_nodes,
            tail_risk: self.timeline_has_skill_tail_risk(duration, &timeline),
        }))
    }

    pub fn compressed_auto_score(
        &self,
        skill: TeamCardSkill,
    ) -> Result<CompressedAutoScore, ChartError> {
        let base_factor = *self.score_factors.first().ok_or(ChartError::EmptyChart)?;
        if self
            .score_factors
            .iter()
            .any(|factor| factor.to_bits() != base_factor.to_bits())
        {
            return Err(ChartError::NonUniformAutoBaseFactor);
        }

        let skill_order = [skill; 6];
        let mut grouped = BTreeMap::<u64, AutoMultiplierGroup>::new();
        self.for_each_multiplier_for_six_skills(&skill_order, |multiplier| {
            grouped
                .entry(multiplier.to_bits())
                .and_modify(|group| group.count += 1)
                .or_insert(AutoMultiplierGroup {
                    multiplier,
                    count: 1,
                });
        })?;
        Ok(CompressedAutoScore {
            base_factor,
            groups: grouped.into_values().collect(),
        })
    }

    #[cfg(test)]
    pub(crate) fn non_rateup_auto_score_template(
        &self,
        duration: f64,
    ) -> Result<NonRateupAutoScoreTemplate, ChartError> {
        let base_factor = *self.score_factors.first().ok_or(ChartError::EmptyChart)?;
        if self
            .score_factors
            .iter()
            .any(|factor| factor.to_bits() != base_factor.to_bits())
        {
            return Err(ChartError::NonUniformAutoBaseFactor);
        }
        let (skill_indices, skill_count) = self.six_skill_indices()?;
        self.non_rateup_auto_score_template_from_parts(
            duration,
            base_factor,
            skill_indices,
            skill_count,
        )
    }

    #[cfg(test)]
    fn non_rateup_auto_score_template_from_parts(
        &self,
        duration: f64,
        base_factor: f64,
        skill_indices: [usize; 6],
        skill_count: u8,
    ) -> Result<NonRateupAutoScoreTemplate, ChartError> {
        let timeline = self.non_rateup_timeline_for_indices(
            duration,
            &skill_indices[..usize::from(skill_count)],
        )?;
        let tail_risk = self.timeline_has_skill_tail_risk(duration, &timeline);
        Ok(NonRateupAutoScoreTemplate {
            base_factor,
            inactive_count: self.nodes.len().saturating_sub(timeline.active_count),
            active_count: timeline.active_count,
            tail_risk,
        })
    }

    /// Returns the maximum per-node Auto base factor and node count for score upper bounds.
    #[cfg(test)]
    pub(crate) fn optimistic_auto_score_terms(&self) -> Result<(f64, usize), ChartError> {
        let max_base_factor = self
            .score_factors
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or(ChartError::EmptyChart)?;
        Ok((max_base_factor, self.count))
    }

    /// Returns the minimum per-node Auto base factor and node count for score lower bounds.
    #[cfg(test)]
    pub(crate) fn pessimistic_auto_score_terms(&self) -> Result<(f64, usize), ChartError> {
        let min_base_factor = self
            .score_factors
            .iter()
            .copied()
            .reduce(f64::min)
            .ok_or(ChartError::EmptyChart)?;
        Ok((min_base_factor, self.count))
    }

    /// Treats every node as if it had both maximum base factor and maximum skill multiplier.
    pub(crate) fn optimistic_auto_score_from_terms(
        terms: (f64, usize),
        stat: i32,
        max_multiplier: f64,
    ) -> i32 {
        let (max_base_factor, count) = terms;
        let no_skill = (stat.max(0) as f64 * max_base_factor).floor();
        let per_node = (no_skill * max_multiplier.max(1.0)).ceil() as i64;
        per_node
            .saturating_mul(count as i64)
            .clamp(0, i32::MAX as i64) as i32
    }

    /// Treats every node as if it had both minimum base factor and minimum skill multiplier.
    pub(crate) fn pessimistic_auto_score_from_terms(
        terms: (f64, usize),
        stat: i32,
        min_multiplier: f64,
    ) -> i32 {
        let (min_base_factor, count) = terms;
        let no_skill = (stat.max(0) as f64 * min_base_factor).floor();
        let per_node = (no_skill * min_multiplier.clamp(0.0, 1.0)).floor() as i64;
        per_node
            .saturating_mul(count as i64)
            .clamp(0, i32::MAX as i64) as i32
    }

    /// Returns true when a scoring node lies in the uncertain tail between 1/120 and 1/30
    /// seconds after the nominal skill duration. Effective starts follow the same queued skill
    /// scheduling as exact scoring.
    pub fn has_skill_tail_risk(&self, duration: f64) -> Result<bool, ChartError> {
        let timeline = self.non_rateup_timeline(duration)?;
        Ok(self.timeline_has_skill_tail_risk(duration, &timeline))
    }

    fn timeline_has_skill_tail_risk(&self, duration: f64, timeline: &NonRateupTimeline) -> bool {
        const TAIL_START: f64 = 1.0 / 120.0;
        const TAIL_END: f64 = 1.0 / 30.0;

        timeline.effective_starts.iter().any(|&start| {
            let lower = start + duration + TAIL_START;
            let upper = start + duration + TAIL_END;
            let first = self
                .nodes
                .partition_point(|node| sgn(node.time - lower) < 0);
            self.nodes
                .get(first)
                .is_some_and(|node| sgn(node.time - upper) <= 0)
        })
    }

    pub fn init_with_rule(
        &mut self,
        combo: i32,
        is_medley: bool,
        score_rule: ScoreRule,
    ) -> Result<(), ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        sort_nodes(&mut self.nodes, score_rule.simultaneous_skill_order);
        self.combo = combo;
        self.score_as_medley = is_medley;
        self.score_rule = score_rule;
        self.warning.clear();
        self.meta = ChartMeta::default();
        self.score_factors.clear();
        self.score_factors.reserve(self.nodes.len());

        let mut combo_cursor = combo;
        let base = 3.0 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64;

        for (node_idx, node) in self.nodes.iter().enumerate() {
            combo_cursor += 1;
            let score_factor = base
                * score_rule.base_multiplier
                * combo_mod(combo_cursor, is_medley, score_rule.combo_mode);
            self.score_factors.push(score_factor);
            self.meta.no_skill += score_factor;

            if node.node_type != ChartNodeType::Skill {
                continue;
            }

            let mut skill = BTreeMap::new();
            for duration in SKILL_DURATIONS {
                let mut temp_combo = combo_cursor;
                let mut value = 0.0;
                for later in self.nodes.iter().skip(node_idx + 1) {
                    if sgn(later.time - node.time - duration - 1.0 / 30.0) > 0 {
                        break;
                    }
                    temp_combo += 1;
                    value += base
                        * score_rule.base_multiplier
                        * combo_mod(temp_combo, is_medley, score_rule.combo_mode);
                }
                skill.insert(duration_key(duration), value);
            }
            self.meta.skill.push(skill);

            let mut rateup_skill = BTreeMap::new();
            for duration in RATEUP_DURATIONS {
                let mut temp_combo = combo_cursor;
                let mut skill_mod = 200.0;
                let mut value = 0.0;
                for later in self.nodes.iter().skip(node_idx + 1) {
                    if skill_mod < 300.0 {
                        skill_mod += 1.0;
                    }
                    if sgn(later.time - node.time - duration - 1.0 / 30.0) > 0 {
                        break;
                    }
                    temp_combo += 1;
                    value += base
                        * score_rule.base_multiplier
                        * combo_mod(temp_combo, is_medley, score_rule.combo_mode)
                        * skill_mod
                        / 200.0;
                }
                rateup_skill.insert(duration_key(duration), value);
            }
            self.meta.rateup.push(rateup_skill);
        }

        let skill_nodes: Vec<_> = self
            .nodes
            .iter()
            .filter(|node| node.node_type == ChartNodeType::Skill)
            .collect();
        for idx in 0..skill_nodes.len().saturating_sub(1) {
            let time_gap = skill_nodes[idx + 1].time - skill_nodes[idx].time;
            if time_gap < 8.75 {
                self.warning.push(SkillWarning {
                    id: idx + 1,
                    time_gap,
                });
            }
        }

        Ok(())
    }

    pub fn skill_delta_at_stat(
        &self,
        activation: usize,
        skill: TeamCardSkill,
        stat: i32,
    ) -> Result<f64, ChartError> {
        let key = duration_key(skill.duration);
        if skill.rateup {
            self.meta
                .rateup
                .get(activation)
                .ok_or(ChartError::MissingSkillMeta { activation })?
                .get(&key)
                .ok_or(ChartError::UnsupportedSkillDuration {
                    duration: skill.duration,
                })?;
        } else {
            self.meta
                .skill
                .get(activation)
                .ok_or(ChartError::MissingSkillMeta { activation })?
                .get(&key)
                .ok_or(ChartError::UnsupportedSkillDuration {
                    duration: skill.duration,
                })?;
        }

        let skill_node_idx = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.node_type == ChartNodeType::Skill)
            .nth(activation)
            .map(|(idx, _)| idx)
            .ok_or(ChartError::MissingSkillMeta { activation })?;
        let skill_time = self.nodes[skill_node_idx].time;
        let base = 3.0 * stat as f64 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64;
        let mut total = 0.0;
        let mut rateup_mod = 1.0 + skill.score_up;

        for (node_idx, node) in self.nodes.iter().enumerate().skip(skill_node_idx + 1) {
            if sgn(node.time - skill_time - skill.duration - 1.0 / 30.0) > 0 {
                break;
            }

            let combo = self.combo + node_idx as i32 + 1;
            let no_skill =
                self.exact_base_score_at_node(node_idx, stat, combo, self.score_as_medley, base)
                    as f64;
            let skill_mod = if skill.rateup {
                if sgn(rateup_mod - 2.5) < 0 {
                    rateup_mod += 0.005;
                }
                rateup_mod
            } else {
                1.0 + skill.score_up
            };
            total += (no_skill * skill_mod).floor() - no_skill;
        }

        Ok(total)
    }

    pub fn no_skill_score_at_stat(&self, stat: i32) -> Result<f64, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        let base = 3.0 * stat as f64 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64;
        Ok(self
            .nodes
            .iter()
            .enumerate()
            .map(|(node_idx, _)| {
                self.exact_base_score_at_node(
                    node_idx,
                    stat,
                    self.combo + node_idx as i32 + 1,
                    self.score_as_medley,
                    base,
                ) as f64
            })
            .sum())
    }

    pub fn get_max_meta_order(&self, team: &[TeamCardSkill]) -> Result<MaxMetaOrder, ChartError> {
        if team.len() != 5 {
            return Err(ChartError::InvalidTeamSize { count: team.len() });
        }

        let mut dp = [0.0; 1 << 5];
        let mut choose = [0usize; 1 << 5];
        dp[0] = self.meta.no_skill;

        for mask in 0..(1usize << 5) {
            let activation = mask.count_ones() as usize;
            for card_idx in 0..5 {
                if mask >> card_idx & 1 == 1 {
                    continue;
                }
                let value = dp[mask] + self.skill_meta(activation, team[card_idx])?;
                let next_mask = mask | (1 << card_idx);
                if value > dp[next_mask] {
                    dp[next_mask] = value;
                    choose[next_mask] = card_idx;
                }
            }
        }

        let mut captain_index = 0;
        let mut captain_meta = 0.0;
        let mut captain_score_up = 0.0;
        for card_idx in 0..5 {
            let value = self.skill_meta(5, team[card_idx])?;
            if value > captain_meta {
                captain_meta = value;
                captain_index = card_idx;
                captain_score_up = team[card_idx].score_up;
            }
        }

        let mut order_indices = Vec::with_capacity(5);
        let mut mask = (1usize << 5) - 1;
        while mask != 0 {
            let card_idx = choose[mask];
            order_indices.push(card_idx);
            mask ^= 1 << card_idx;
        }
        order_indices.reverse();

        let mut score_up_order: Vec<f64> = order_indices
            .iter()
            .map(|&idx| team[idx].score_up)
            .collect();
        score_up_order.push(captain_score_up);

        Ok(MaxMetaOrder {
            meta: captain_meta + dp[(1 << 5) - 1],
            order_indices,
            captain_index,
            score_up_order,
        })
    }

    pub fn get_score(
        &self,
        skill_order: &[TeamCardSkill],
        stat: i32,
        is_medley: bool,
    ) -> Result<i32, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        let base = 3.0 * stat as f64 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64;
        let mut result = 0i32;
        let mut skill_count = 0usize;
        let mut combo = self.combo;
        let mut skill_mod = 1.0;
        let mut rateup = false;
        let mut events: VecDeque<SkillEvent> = VecDeque::new();

        for (node_idx, node) in self.nodes.iter().enumerate() {
            if events
                .front()
                .map(|event| sgn(node.time - event.time) > 0)
                .unwrap_or(false)
            {
                let event = events.pop_front().expect("front event exists");
                skill_mod = event.skill_mod;
                rateup = event.rateup;
            }

            combo += 1;
            if rateup && sgn(skill_mod - 2.5) < 0 {
                skill_mod += 0.005;
            }

            let no_skill =
                self.exact_base_score_at_node(node_idx, stat, combo, is_medley, base) as f64;
            result += (no_skill * skill_mod).floor() as i32;

            if node.node_type != ChartNodeType::Skill {
                continue;
            }

            let skill =
                skill_order
                    .get(skill_count)
                    .copied()
                    .ok_or(ChartError::MissingSkillProfile {
                        activation: skill_count,
                    })?;

            if !events.is_empty() {
                let start_time = events.back().expect("back event exists").time + 0.75;
                events.push_back(SkillEvent {
                    time: start_time,
                    skill_mod: 1.0 + skill.score_up,
                    rateup: skill.rateup,
                });
                events.push_back(SkillEvent {
                    time: start_time + skill.duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                });
            } else {
                skill_mod = 1.0 + skill.score_up;
                rateup = skill.rateup;
                events.push_back(SkillEvent {
                    time: node.time + skill.duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                });
            }

            skill_count += 1;
        }

        Ok(result)
    }

    pub(crate) fn get_score_for_six_skills(
        &self,
        skill_order: &[TeamCardSkill; 6],
        stat: i32,
        is_medley: bool,
    ) -> Result<i32, ChartError> {
        if self.score_as_medley == is_medley && avx2_available() {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                // SAFETY: runtime AVX2 support is checked above. The AVX2
                // path only vectorizes contiguous note ranges whose modifiers
                // are constant; event handling stays scalar.
                return unsafe { self.get_score_for_six_skills_avx2(skill_order, stat, is_medley) };
            }
        }

        self.get_score_for_six_skills_scalar(skill_order, stat, is_medley)
    }

    fn for_each_multiplier_for_six_skills(
        &self,
        skill_order: &[TeamCardSkill; 6],
        mut visit: impl FnMut(f64),
    ) -> Result<(), ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        let mut skill_count = 0usize;
        let mut skill_mod = 1.0;
        let mut rateup = false;
        let mut events: VecDeque<SkillEvent> = VecDeque::new();

        for node in &self.nodes {
            if events
                .front()
                .map(|event| sgn(node.time - event.time) > 0)
                .unwrap_or(false)
            {
                let event = events.pop_front().expect("front event exists");
                skill_mod = event.skill_mod;
                rateup = event.rateup;
            }

            if rateup && sgn(skill_mod - 2.5) < 0 {
                skill_mod += 0.005;
            }
            visit(skill_mod);

            if node.node_type != ChartNodeType::Skill {
                continue;
            }
            let skill =
                skill_order
                    .get(skill_count)
                    .copied()
                    .ok_or(ChartError::MissingSkillProfile {
                        activation: skill_count,
                    })?;

            if !events.is_empty() {
                let start_time = events.back().expect("back event exists").time + 0.75;
                events.push_back(SkillEvent {
                    time: start_time,
                    skill_mod: 1.0 + skill.score_up,
                    rateup: skill.rateup,
                });
                events.push_back(SkillEvent {
                    time: start_time + skill.duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                });
            } else {
                skill_mod = 1.0 + skill.score_up;
                rateup = skill.rateup;
                events.push_back(SkillEvent {
                    time: node.time + skill.duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                });
            }
            skill_count += 1;
        }
        Ok(())
    }

    fn non_rateup_timeline(&self, duration: f64) -> Result<NonRateupTimeline, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        let (skill_indices, skill_count) = self.six_skill_indices()?;
        self.non_rateup_timeline_for_indices(duration, &skill_indices[..usize::from(skill_count)])
    }

    fn six_skill_indices(&self) -> Result<([usize; 6], u8), ChartError> {
        let mut skill_indices = [0_usize; 6];
        let mut skill_count = 0_usize;
        for (index, node) in self.nodes.iter().enumerate() {
            if node.node_type != ChartNodeType::Skill {
                continue;
            }
            let Some(slot) = skill_indices.get_mut(skill_count) else {
                return Err(ChartError::MissingSkillProfile {
                    activation: skill_count,
                });
            };
            *slot = index;
            skill_count += 1;
        }
        Ok((skill_indices, skill_count as u8))
    }

    fn non_rateup_timeline_for_indices(
        &self,
        duration: f64,
        skill_indices: &[usize],
    ) -> Result<NonRateupTimeline, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        let mut effective_starts = Vec::with_capacity(skill_indices.len());
        let mut events: VecDeque<SkillEvent> = VecDeque::new();
        let mut next_skill = 0_usize;
        let mut event_min_index = 0_usize;
        let mut active_start = None;
        let mut active_count = 0_usize;

        loop {
            let next_skill_index = skill_indices.get(next_skill).copied();
            let next_event_index = events.front().and_then(|event| {
                let index = self
                    .nodes
                    .partition_point(|node| sgn(node.time - event.time) <= 0)
                    .max(event_min_index);
                (index < self.nodes.len()).then_some(index)
            });

            if next_event_index.is_some_and(|event_index| {
                next_skill_index.is_none_or(|skill_index| event_index <= skill_index)
            }) {
                let event_index = next_event_index.expect("event index was checked above");
                let event = events.pop_front().expect("front event exists");
                let becomes_active = event.skill_mod.to_bits() != 1.0_f64.to_bits();
                match (active_start, becomes_active) {
                    (Some(start), false) => {
                        active_count = active_count.saturating_add(event_index - start);
                        active_start = None;
                    }
                    (None, true) => active_start = Some(event_index),
                    _ => {}
                }
                event_min_index = event_index.saturating_add(1);
                continue;
            }

            let Some(skill_index) = next_skill_index else {
                break;
            };
            next_skill += 1;
            let node = &self.nodes[skill_index];
            if events.is_empty() {
                effective_starts.push(node.time);
                if active_start.is_none() {
                    active_start = Some(skill_index.saturating_add(1));
                }
                events.push_back(SkillEvent {
                    time: node.time + duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                });
                event_min_index = skill_index.saturating_add(1);
            } else {
                let start_time = events.back().expect("back event exists").time + 0.75;
                effective_starts.push(start_time);
                events.push_back(SkillEvent {
                    time: start_time,
                    skill_mod: 2.0,
                    rateup: false,
                });
                events.push_back(SkillEvent {
                    time: start_time + duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                });
            }
        }

        if let Some(start) = active_start {
            active_count = active_count.saturating_add(self.nodes.len().saturating_sub(start));
        }
        Ok(NonRateupTimeline {
            active_count,
            effective_starts,
        })
    }

    fn get_score_for_six_skills_scalar(
        &self,
        skill_order: &[TeamCardSkill; 6],
        stat: i32,
        is_medley: bool,
    ) -> Result<i32, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        let base = 3.0 * stat as f64 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64;
        let mut result = 0i32;
        let mut skill_count = 0usize;
        let mut combo = self.combo;
        let mut skill_mod = 1.0;
        let mut rateup = false;
        let mut events = FixedSkillEventQueue::new();

        for (node_idx, node) in self.nodes.iter().enumerate() {
            if let Some(event) = events.pop_front_if_after(node.time) {
                skill_mod = event.skill_mod;
                rateup = event.rateup;
            }

            combo += 1;
            if rateup && sgn(skill_mod - 2.5) < 0 {
                skill_mod += 0.005;
            }

            let no_skill =
                self.exact_base_score_at_node(node_idx, stat, combo, is_medley, base) as f64;
            result += (no_skill * skill_mod).floor() as i32;

            if node.node_type != ChartNodeType::Skill {
                continue;
            }

            let skill =
                skill_order
                    .get(skill_count)
                    .copied()
                    .ok_or(ChartError::MissingSkillProfile {
                        activation: skill_count,
                    })?;

            if !events.is_empty() {
                let start_time = events.back_time().expect("non-empty queue has a back") + 0.75;
                if !events.push_back(SkillEvent {
                    time: start_time,
                    skill_mod: 1.0 + skill.score_up,
                    rateup: skill.rateup,
                }) || !events.push_back(SkillEvent {
                    time: start_time + skill.duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                }) {
                    return self.get_score(skill_order, stat, is_medley);
                }
            } else {
                skill_mod = 1.0 + skill.score_up;
                rateup = skill.rateup;
                if !events.push_back(SkillEvent {
                    time: node.time + skill.duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                }) {
                    return self.get_score(skill_order, stat, is_medley);
                }
            }

            skill_count += 1;
        }

        Ok(result)
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[target_feature(enable = "avx2")]
    unsafe fn get_score_for_six_skills_avx2(
        &self,
        skill_order: &[TeamCardSkill; 6],
        stat: i32,
        is_medley: bool,
    ) -> Result<i32, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        let mut result = 0i32;
        let mut skill_count = 0usize;
        let mut skill_mod = 1.0;
        let mut rateup = false;
        let mut events = FixedSkillEventQueue::new();
        let mut node_idx = 0usize;

        while node_idx < self.nodes.len() {
            let node = &self.nodes[node_idx];
            if let Some(event) = events.pop_front_if_after(node.time) {
                skill_mod = event.skill_mod;
                rateup = event.rateup;
            }

            if !rateup && node.node_type != ChartNodeType::Skill {
                let range_start = node_idx;
                let mut range_end = node_idx + 1;
                while range_end < self.nodes.len()
                    && self.nodes[range_end].node_type != ChartNodeType::Skill
                    && !events.should_pop_at(self.nodes[range_end].time)
                {
                    range_end += 1;
                }

                result += score_factor_range_sum_avx2(
                    &self.score_factors[range_start..range_end],
                    stat,
                    skill_mod,
                );
                node_idx = range_end;
                continue;
            }

            if rateup && sgn(skill_mod - 2.5) < 0 {
                skill_mod += 0.005;
            }

            let no_skill = (stat as f64 * self.score_factors[node_idx]).floor();
            result += (no_skill * skill_mod).floor() as i32;

            if node.node_type != ChartNodeType::Skill {
                node_idx += 1;
                continue;
            }

            let skill =
                skill_order
                    .get(skill_count)
                    .copied()
                    .ok_or(ChartError::MissingSkillProfile {
                        activation: skill_count,
                    })?;

            if !events.is_empty() {
                let start_time = events.back_time().expect("non-empty queue has a back") + 0.75;
                if !events.push_back(SkillEvent {
                    time: start_time,
                    skill_mod: 1.0 + skill.score_up,
                    rateup: skill.rateup,
                }) || !events.push_back(SkillEvent {
                    time: start_time + skill.duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                }) {
                    return self.get_score_for_six_skills_scalar(skill_order, stat, is_medley);
                }
            } else {
                skill_mod = 1.0 + skill.score_up;
                rateup = skill.rateup;
                if !events.push_back(SkillEvent {
                    time: node.time + skill.duration + 1.0 / 30.0,
                    skill_mod: 1.0,
                    rateup: false,
                }) {
                    return self.get_score_for_six_skills_scalar(skill_order, stat, is_medley);
                }
            }

            skill_count += 1;
            node_idx += 1;
        }

        Ok(result)
    }

    pub(crate) fn skill_meta_value(
        &self,
        activation: usize,
        skill: TeamCardSkill,
    ) -> Result<f64, ChartError> {
        self.skill_meta(activation, skill)
    }

    fn skill_meta(&self, activation: usize, skill: TeamCardSkill) -> Result<f64, ChartError> {
        let key = duration_key(skill.duration);
        if skill.rateup {
            return self
                .meta
                .rateup
                .get(activation)
                .ok_or(ChartError::MissingSkillMeta { activation })?
                .get(&key)
                .copied()
                .ok_or(ChartError::UnsupportedSkillDuration {
                    duration: skill.duration,
                });
        }

        let value = self
            .meta
            .skill
            .get(activation)
            .ok_or(ChartError::MissingSkillMeta { activation })?
            .get(&key)
            .copied()
            .ok_or(ChartError::UnsupportedSkillDuration {
                duration: skill.duration,
            })?;
        Ok(value * skill.score_up)
    }

    fn exact_base_score_at_node(
        &self,
        node_idx: usize,
        stat: i32,
        combo: i32,
        is_medley: bool,
        base: f64,
    ) -> f64 {
        if self.score_as_medley == is_medley {
            if let Some(&factor) = self.score_factors.get(node_idx) {
                return (stat as f64 * factor).floor();
            }
        }

        base_score(base, combo, is_medley, self.score_rule)
    }
}

#[derive(Debug, Clone, Copy)]
struct SkillEvent {
    time: f64,
    skill_mod: f64,
    rateup: bool,
}

struct FixedSkillEventQueue {
    events: [SkillEvent; 16],
    head: usize,
    len: usize,
}

impl FixedSkillEventQueue {
    fn new() -> Self {
        Self {
            events: [SkillEvent {
                time: 0.0,
                skill_mod: 1.0,
                rateup: false,
            }; 16],
            head: 0,
            len: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn back_time(&self) -> Option<f64> {
        (self.len > 0).then(|| {
            let index = (self.head + self.len - 1) % self.events.len();
            self.events[index].time
        })
    }

    fn pop_front_if_after(&mut self, time: f64) -> Option<SkillEvent> {
        if self.len == 0 || sgn(time - self.events[self.head].time) <= 0 {
            return None;
        }

        let event = self.events[self.head];
        self.head = (self.head + 1) % self.events.len();
        self.len -= 1;
        Some(event)
    }

    fn should_pop_at(&self, time: f64) -> bool {
        self.len > 0 && sgn(time - self.events[self.head].time) > 0
    }

    fn push_back(&mut self, event: SkillEvent) -> bool {
        if self.len == self.events.len() {
            return false;
        }

        let index = (self.head + self.len) % self.events.len();
        self.events[index] = event;
        self.len += 1;
        true
    }
}

fn avx2_available() -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        std::is_x86_feature_detected!("avx2")
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        false
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn score_factor_range_sum_avx2(factors: &[f64], stat: i32, skill_mod: f64) -> i32 {
    let stat_vec = _mm256_set1_pd(stat as f64);
    let skill_mod_vec = _mm256_set1_pd(skill_mod);
    let mut sum_vec = _mm256_setzero_pd();
    let mut idx = 0usize;

    while idx + 4 <= factors.len() {
        let factors_vec = _mm256_loadu_pd(factors.as_ptr().add(idx));
        let no_skill = _mm256_floor_pd(_mm256_mul_pd(factors_vec, stat_vec));
        let score = _mm256_floor_pd(_mm256_mul_pd(no_skill, skill_mod_vec));
        sum_vec = _mm256_add_pd(sum_vec, score);
        idx += 4;
    }

    let mut lanes = [0.0; 4];
    _mm256_storeu_pd(lanes.as_mut_ptr(), sum_vec);
    let mut sum = lanes.iter().sum::<f64>();

    for &factor in &factors[idx..] {
        let no_skill = (stat as f64 * factor).floor();
        sum += (no_skill * skill_mod).floor();
    }

    sum as i32
}

pub fn get_combo_mod(combo: i32, is_medley: bool) -> f64 {
    if combo <= 20 {
        return 1.0;
    }
    if combo <= 300 {
        return 1.0 + 0.01 * div_ceil(combo, 50) as f64;
    }
    if combo <= 700 || is_medley && combo <= 3000 {
        return 1.03 + 0.01 * div_ceil(combo, 100) as f64;
    }
    if !is_medley {
        return 1.11;
    }
    1.34
}

fn combo_mod(combo: i32, is_medley: bool, combo_mode: ComboMode) -> f64 {
    match combo_mode {
        ComboMode::Standard => get_combo_mod(combo, is_medley),
        ComboMode::None => 1.0,
    }
}

fn base_score(base: f64, combo: i32, is_medley: bool, score_rule: ScoreRule) -> f64 {
    (base * combo_mod(combo, is_medley, score_rule.combo_mode) * score_rule.base_multiplier).floor()
}

fn div_ceil(value: i32, divisor: i32) -> i32 {
    value.div_euclid(divisor) + i32::from(value.rem_euclid(divisor) != 0)
}

fn duration_key(duration: f64) -> i32 {
    (duration * 1000.0).round() as i32
}

fn node_sort_value(node_type: ChartNodeType) -> i32 {
    match node_type {
        ChartNodeType::Skill => 0,
        ChartNodeType::Node => 1,
    }
}

fn sort_nodes(nodes: &mut [ChartNode], simultaneous_skill_order: SimultaneousSkillOrder) {
    nodes.sort_by(|a, b| {
        let time_order = sgn(a.time - b.time);
        if time_order != 0 {
            return time_order.cmp(&0);
        }
        let value = |node_type| match simultaneous_skill_order {
            SimultaneousSkillOrder::BeforeNotes => node_sort_value(node_type),
            SimultaneousSkillOrder::AfterNotes => -node_sort_value(node_type),
        };
        value(a.node_type).cmp(&value(b.node_type))
    });
}

fn sgn(value: f64) -> i32 {
    i32::from(value > EPS) - i32::from(value < -EPS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(card_id: u32, score_up: f64) -> TeamCardSkill {
        TeamCardSkill {
            card_id,
            duration: 3.0,
            score_up,
            rateup: false,
        }
    }

    #[test]
    fn combo_mod_matches_bandori_steps() {
        assert_eq!(get_combo_mod(20, false), 1.0);
        assert_eq!(get_combo_mod(21, false), 1.01);
        assert_eq!(get_combo_mod(300, false), 1.06);
        assert_eq!(get_combo_mod(301, false), 1.07);
        assert_eq!(get_combo_mod(701, false), 1.11);
        assert_eq!(get_combo_mod(3000, true), 1.33);
        assert_eq!(get_combo_mod(3001, true), 1.34);
    }

    #[test]
    fn score_applies_skill_mod_until_duration_end() {
        let mut chart = Chart::new(
            5,
            vec![
                ChartNode {
                    node_type: ChartNodeType::Skill,
                    time: 0.0,
                },
                ChartNode {
                    node_type: ChartNodeType::Node,
                    time: 1.0,
                },
                ChartNode {
                    node_type: ChartNodeType::Node,
                    time: 2.0,
                },
                ChartNode {
                    node_type: ChartNodeType::Node,
                    time: 9.0,
                },
            ],
        );
        chart.init(0, false).unwrap();

        let score = chart.get_score(&[skill(1, 1.0)], 1000, false).unwrap();

        assert_eq!(score, 4950);
        assert_eq!(chart.no_skill_score_at_stat(1000).unwrap(), 3300.0);
        assert_eq!(
            chart.skill_delta_at_stat(0, skill(1, 1.0), 1000).unwrap(),
            1650.0
        );
    }

    #[test]
    fn auto_rule_uses_half_multiplier_without_combo_bonus() {
        let mut chart = Chart::new(
            5,
            (0..4)
                .map(|index| ChartNode {
                    node_type: ChartNodeType::Node,
                    time: index as f64,
                })
                .collect(),
        );
        chart.init_auto().unwrap();

        assert_eq!(chart.no_skill_score_at_stat(1000).unwrap(), 1500.0);
        assert_eq!(chart.get_score(&[], 1000, false).unwrap(), 1500);
    }

    #[test]
    fn auto_rule_applies_simultaneous_skill_after_other_notes() {
        let mut chart = Chart::new(
            5,
            vec![
                ChartNode {
                    node_type: ChartNodeType::Skill,
                    time: 0.0,
                },
                ChartNode {
                    node_type: ChartNodeType::Node,
                    time: 0.0,
                },
                ChartNode {
                    node_type: ChartNodeType::Node,
                    time: 1.0,
                },
            ],
        );
        chart.init_auto().unwrap();

        assert_eq!(chart.nodes[0].node_type, ChartNodeType::Node);
        assert_eq!(chart.nodes[1].node_type, ChartNodeType::Skill);
        assert_eq!(chart.get_score(&[skill(1, 1.0)], 900, false).unwrap(), 1800);
    }

    #[test]
    fn max_meta_order_returns_five_ordered_cards_and_captain() {
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
        let mut chart = Chart::new(5, nodes);
        chart.init(0, false).unwrap();

        let team = [
            skill(1, 0.6),
            skill(2, 0.7),
            skill(3, 0.8),
            skill(4, 0.9),
            skill(5, 1.0),
        ];
        let order = chart.get_max_meta_order(&team).unwrap();

        assert_eq!(order.order_indices.len(), 5);
        assert!(order.captain_index < 5);
        assert_eq!(order.score_up_order.len(), 6);
        assert!(order.meta > chart.meta.no_skill);
    }

    #[test]
    fn six_skill_fast_score_matches_generic_score() {
        let mut nodes = Vec::new();
        for idx in 0..6 {
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: idx as f64 * 4.0,
            });
            for offset in 1..=3 {
                nodes.push(ChartNode {
                    node_type: ChartNodeType::Node,
                    time: idx as f64 * 4.0 + offset as f64,
                });
            }
        }
        let mut chart = Chart::new(18, nodes);
        chart.init(20, true).unwrap();

        let mut skills = [
            skill(1, 0.6),
            skill(2, 0.7),
            skill(3, 0.8),
            skill(4, 0.9),
            skill(5, 1.0),
            skill(6, 1.1),
        ];
        skills[2].duration = 5.0;
        skills[2].rateup = true;
        skills[5].duration = 6.5;

        assert_eq!(
            chart.get_score(&skills, 12345, true).unwrap(),
            chart
                .get_score_for_six_skills(&skills, 12345, true)
                .unwrap()
        );
    }

    #[test]
    fn compressed_auto_score_matches_exact_six_skill_path() {
        let mut nodes = Vec::new();
        for activation in 0..6 {
            nodes.push(ChartNode {
                time: activation as f64 * 12.0,
                node_type: ChartNodeType::Skill,
            });
            for offset in [0.5, 1.0, 2.0, 4.0, 6.0] {
                nodes.push(ChartNode {
                    time: activation as f64 * 12.0 + offset,
                    node_type: ChartNodeType::Node,
                });
            }
        }
        let mut chart = Chart::new(27, nodes);
        chart.init_auto_with_base_multiplier(0.75).unwrap();

        let mut rateup = skill(1, 1.0);
        rateup.duration = 7.0;
        rateup.rateup = true;
        for skill in [skill(7, 1.2), rateup] {
            let compressed = chart.compressed_auto_score(skill).unwrap();
            let template = chart
                .non_rateup_auto_score_template(skill.duration)
                .unwrap();
            let templated = (!skill.rateup).then(|| template.with_score_up(skill.score_up));
            assert_eq!(
                template.has_skill_tail_risk(),
                chart.has_skill_tail_risk(skill.duration).unwrap()
            );
            for stat in [1, 12_345, 100_000, 543_210] {
                assert_eq!(
                    compressed.score(stat),
                    chart
                        .get_score_for_six_skills(&[skill; 6], stat, false)
                        .unwrap(),
                );
                assert!(
                    compressed.score(stat)
                        <= Chart::optimistic_auto_score_from_terms(
                            chart.optimistic_auto_score_terms().unwrap(),
                            stat,
                            2.51,
                        )
                );
                if let Some(templated) = &templated {
                    assert_eq!(templated.score(stat), compressed.score(stat));
                }
                if !skill.rateup {
                    assert_eq!(template.score(stat, skill.score_up), compressed.score(stat));
                } else {
                    assert!(template.score(stat, skill.score_up) <= compressed.score(stat));
                    assert!(compressed.score(stat) <= template.score(stat, 1.51));
                }
            }
        }
    }

    #[test]
    fn non_rateup_template_matches_queued_skill_timeline() {
        let mut nodes = Vec::new();
        for step in 0..120 {
            nodes.push(ChartNode {
                time: step as f64 * 0.25,
                node_type: if step < 18 && step % 3 == 0 {
                    ChartNodeType::Skill
                } else {
                    ChartNodeType::Node
                },
            });
        }
        let mut chart = Chart::new(25, nodes);
        chart.init_auto_with_base_multiplier(0.75).unwrap();

        for duration in [0.0, 0.5, 1.0, 2.25, 5.0, 7.5] {
            let mut skill = skill(1, 1.3);
            skill.duration = duration;
            let template = chart
                .non_rateup_auto_score_template(skill.duration)
                .unwrap();
            let compressed = chart.compressed_auto_score(skill).unwrap();
            assert_eq!(
                template.has_skill_tail_risk(),
                chart.has_skill_tail_risk(skill.duration).unwrap()
            );

            for stat in [1, 12_345, 100_000, 543_210] {
                assert_eq!(template.score(stat, skill.score_up), compressed.score(stat));

                let mut rateup = skill;
                rateup.score_up = 1.0;
                rateup.rateup = true;
                let rateup_score = chart.compressed_auto_score(rateup).unwrap().score(stat);
                assert!(template.score(stat, rateup.score_up) <= rateup_score);
                assert!(rateup_score <= template.score(stat, 1.51));
            }
        }
    }

    #[test]
    fn skill_tail_risk_uses_closed_conservative_window() {
        let duration = 5.0;
        let mut risky_nodes = (0..6)
            .map(|activation| ChartNode {
                time: activation as f64 * 20.0,
                node_type: ChartNodeType::Skill,
            })
            .collect::<Vec<_>>();
        risky_nodes.push(ChartNode {
            time: duration + 1.0 / 120.0,
            node_type: ChartNodeType::Node,
        });
        let risky = Chart::new(20, risky_nodes);
        assert!(risky.has_skill_tail_risk(duration).unwrap());

        let mut safe_nodes = (0..6)
            .map(|activation| ChartNode {
                time: activation as f64 * 20.0,
                node_type: ChartNodeType::Skill,
            })
            .collect::<Vec<_>>();
        safe_nodes.push(ChartNode {
            time: duration + 0.05,
            node_type: ChartNodeType::Node,
        });
        let safe = Chart::new(20, safe_nodes);
        assert!(!safe.has_skill_tail_risk(duration).unwrap());
    }

    #[test]
    fn warns_when_skill_nodes_are_too_close() {
        let mut chart = Chart::new(
            5,
            vec![
                ChartNode {
                    node_type: ChartNodeType::Skill,
                    time: 0.0,
                },
                ChartNode {
                    node_type: ChartNodeType::Skill,
                    time: 8.0,
                },
            ],
        );

        chart.init(0, false).unwrap();

        assert_eq!(chart.warning.len(), 1);
        assert_eq!(chart.warning[0].id, 1);
    }
}
