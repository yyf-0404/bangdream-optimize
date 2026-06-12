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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartNodeType {
    Node,
    Skill,
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
            score_factors: Vec::new(),
        }
    }

    pub fn init(&mut self, combo: i32, is_medley: bool) -> Result<(), ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        self.combo = combo;
        self.score_as_medley = is_medley;
        self.warning.clear();
        self.meta = ChartMeta::default();
        self.score_factors.clear();
        self.score_factors.reserve(self.nodes.len());

        let mut combo_cursor = combo;
        let base = 3.0 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64 * 1.1;

        for (node_idx, node) in self.nodes.iter().enumerate() {
            combo_cursor += 1;
            let score_factor = base * get_combo_mod(combo_cursor, is_medley);
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
                    value += base * get_combo_mod(temp_combo, is_medley);
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
                    value += base * get_combo_mod(temp_combo, is_medley) * skill_mod / 200.0;
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

        base_score(base, combo, is_medley)
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

fn base_score(base: f64, combo: i32, is_medley: bool) -> f64 {
    (base * get_combo_mod(combo, is_medley) * 1.1).floor()
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
