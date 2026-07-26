use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use thiserror::Error;

use crate::timing::Timer;

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

const EPS: f64 = 0.001;
const SKILL_DURATIONS: [f64; 17] = [
    3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 5.6, 5.7, 6.0, 6.2, 6.4, 6.5, 6.8, 7.0, 7.2, 7.5, 8.0,
];
const RATEUP_DURATIONS: [f64; 5] = [5.0, 5.5, 6.0, 6.5, 7.0];
const SKILL_FINISHING_SECONDS: f64 = 0.75;
const MIN_SUPPORTED_FPS: f64 = 60.0;
const MAX_SUPPORTED_FPS: f64 = 120.0;
const IDEAL_FPS: i64 = 60;
const IDEAL_FRAME_EPS: f64 = 1.0e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SkillTimingInterval {
    pub(crate) lower: f64,
    pub(crate) upper: f64,
}

/// Interval containing the last judgement time at which a skill can still be active.
///
/// At a fixed frame rate `f`, the end lies in `[duration + 1/f, duration + 2/f)`.
/// Taking the envelope of the supported 60/120 FPS timings gives the interval below.
pub(crate) fn skill_end_interval(duration: f64) -> SkillTimingInterval {
    SkillTimingInterval {
        lower: duration + 1.0 / MAX_SUPPORTED_FPS,
        upper: duration + 2.0 / MIN_SUPPORTED_FPS,
    }
}

/// Interval of trigger gaps at which the following skill changes from certainly queued to
/// certainly not queued. This includes the 0.75-second Finishing state and the state-transition
/// frames around it.
///
/// `gap <= lower` is queued on every supported timing, `gap > upper` is never queued, and the
/// region in between depends on frame rate and the trigger's sub-frame offset.
pub(crate) fn skill_queue_gap_interval(duration: f64) -> SkillTimingInterval {
    SkillTimingInterval {
        lower: duration + SKILL_FINISHING_SECONDS + 2.0 / MAX_SUPPORTED_FPS,
        upper: duration + SKILL_FINISHING_SECONDS + 3.0 / MIN_SUPPORTED_FPS,
    }
}

#[derive(Debug, Clone, Copy)]
struct ScheduledSkillWindow {
    start: f64,
    end: f64,
    queue_risk: bool,
}

/// Collapses the timing interval to its score-maximising endpoint. Guaranteed queues are delayed;
/// timing-dependent queues use the nonqueued branch while retaining an explicit risk flag.
#[derive(Debug, Clone, Copy, Default)]
struct EnvelopeSkillScheduler {
    must_queue_until: Option<f64>,
    may_queue_until: Option<f64>,
    scheduled_end: Option<f64>,
}

impl EnvelopeSkillScheduler {
    fn schedule(&mut self, trigger: f64, duration: f64) -> ScheduledSkillWindow {
        let queued = self
            .must_queue_until
            .is_some_and(|deadline| sgn(trigger - deadline) <= 0);
        let queue_risk = self
            .may_queue_until
            .is_some_and(|deadline| sgn(trigger - deadline) <= 0);
        let start = if queued {
            trigger.max(
                self.scheduled_end
                    .expect("a queue deadline always has a scheduled end")
                    + SKILL_FINISHING_SECONDS,
            )
        } else {
            trigger
        };
        let end = start + skill_end_interval(duration).upper;
        self.scheduled_end = Some(end);
        let queue_gap = skill_queue_gap_interval(duration);
        self.must_queue_until = Some(start + queue_gap.lower);
        self.may_queue_until = Some(start + queue_gap.upper);
        ScheduledSkillWindow {
            start,
            end,
            queue_risk,
        }
    }
}

#[inline]
fn ideal_judgement_frame(time: f64) -> i64 {
    (time.mul_add(IDEAL_FPS as f64, -IDEAL_FRAME_EPS)).ceil() as i64
}

#[inline]
fn ideal_duration_frames(duration: f64) -> i64 {
    (duration * IDEAL_FPS as f64).round() as i64
}

#[inline]
fn ideal_frame_time(frame: i64) -> f64 {
    frame as f64 / IDEAL_FPS as f64
}

#[inline]
fn ideal_skill_end_time(trigger: f64, duration: f64) -> f64 {
    ideal_frame_time(ideal_judgement_frame(trigger) + ideal_duration_frames(duration) + 1)
}

#[inline]
fn ideal_skills_queue(first_trigger: f64, duration: f64, next_trigger: f64) -> bool {
    ideal_judgement_frame(next_trigger) - ideal_judgement_frame(first_trigger)
        <= ideal_duration_frames(duration) + 47
}

#[derive(Debug, Clone, Copy, Default)]
struct Ideal60SkillScheduler {
    queue_until_frame: Option<i64>,
    next_queued_start_frame: Option<i64>,
}

impl Ideal60SkillScheduler {
    fn schedule(&mut self, trigger: f64, duration: f64) -> ScheduledSkillWindow {
        let trigger_frame = ideal_judgement_frame(trigger);
        let duration_frames = ideal_duration_frames(duration);
        let queued = self
            .queue_until_frame
            .is_some_and(|deadline| trigger_frame <= deadline);
        let start_frame = if queued {
            self.next_queued_start_frame
                .expect("a queue deadline always has a queued start frame")
        } else {
            trigger_frame + 1
        };
        let end_frame = start_frame + duration_frames;

        // From the first active frame, the current skill accepts queued triggers through
        // duration + 46 frames and a queued skill becomes active two frames after that.
        self.queue_until_frame = Some(start_frame + duration_frames + 46);
        self.next_queued_start_frame = Some(start_frame + duration_frames + 48);

        ScheduledSkillWindow {
            // Existing scorers activate an unqueued skill immediately after its skill node so
            // exact-simultaneous manual notes can be delayed one frame and receive the bonus.
            // A queued start event uses the previous frame boundary: `node_time > threshold`
            // is equivalent to `ceil(node_time * 60) >= start_frame`.
            start: if queued {
                ideal_frame_time(start_frame - 1)
            } else {
                trigger
            },
            end: ideal_frame_time(end_frame),
            queue_risk: queued,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SkillScheduler {
    Ideal60(Ideal60SkillScheduler),
    Envelope(EnvelopeSkillScheduler),
}

impl SkillScheduler {
    fn new(ideal_60: bool) -> Self {
        if ideal_60 {
            Self::Ideal60(Ideal60SkillScheduler::default())
        } else {
            Self::Envelope(EnvelopeSkillScheduler::default())
        }
    }

    fn schedule(&mut self, trigger: f64, duration: f64) -> ScheduledSkillWindow {
        match self {
            Self::Ideal60(scheduler) => scheduler.schedule(trigger, duration),
            Self::Envelope(scheduler) => scheduler.schedule(trigger, duration),
        }
    }

    fn event_is_due(self, node_time: f64, event_time: f64) -> bool {
        match self {
            Self::Ideal60(_) => node_time > event_time,
            Self::Envelope(_) => sgn(node_time - event_time) > 0,
        }
    }
}

#[derive(Debug, Error)]
pub enum ChartError {
    #[error("chart count must be positive")]
    EmptyChart,

    #[error("team must contain exactly five cards, got {count}")]
    InvalidTeamSize { count: usize },

    #[error("compiled scorer medley mode does not match initialized chart mode")]
    ScoreModeMismatch,

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CompiledSixSkillScore {
    multipliers: Vec<f64>,
}

impl CompiledSixSkillScore {
    fn score(&self, score_factors: &[f64], stat: i32) -> i32 {
        debug_assert_eq!(score_factors.len(), self.multipliers.len());
        if avx2_available() {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                // SAFETY: runtime AVX2 support is checked above and both slices have equal length.
                return unsafe {
                    compiled_six_skill_score_avx2(score_factors, &self.multipliers, stat)
                };
            }
        }
        score_factors
            .iter()
            .zip(&self.multipliers)
            .map(|(&factor, &multiplier)| {
                let no_skill = (stat as f64 * factor).floor();
                (no_skill * multiplier).floor() as i32
            })
            .sum()
    }
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
    skill_queue_risk: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScoreRangeSkillWindowCounts {
    pub inactive_nodes: u32,
    pub active_nodes: [u32; 6],
    pub tail_risk: bool,
    pub skill_queue_risk: bool,
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
pub struct MaxScoreOrder {
    pub score: i32,
    pub order_indices: Vec<usize>,
    pub captain_index: usize,
    pub score_up_order: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactSkillOrder {
    pub(crate) score: i32,
    pub(crate) order_indices: [usize; 5],
    pub(crate) captain_index: usize,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ExactSkillWindow {
    range_start: usize,
    range_end: usize,
    score_up: f64,
    rateup: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ExactScoreScratch {
    base_scores: Vec<i32>,
    rateup_profiles: [RateUpProfileScratch; 5],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndependentSkillScoreMatrix {
    pub(crate) base_score: i32,
    pub(crate) deltas: [[i32; 6]; 5],
}

#[derive(Debug, Default)]
struct RateUpProfileScratch {
    score_up_bits: u64,
    initialized: bool,
    multipliers: Vec<f64>,
}

impl RateUpProfileScratch {
    fn prepare(&mut self, score_up: f64, len: usize) {
        let score_up_bits = score_up.to_bits();
        if !self.initialized || self.score_up_bits != score_up_bits {
            self.score_up_bits = score_up_bits;
            self.initialized = true;
            self.multipliers.clear();
        }

        let mut multiplier = self.multipliers.last().copied().unwrap_or(1.0 + score_up);
        while self.multipliers.len() < len {
            if sgn(multiplier - 2.5) < 0 {
                multiplier += 0.005;
            }
            self.multipliers.push(multiplier);
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ExactSkillOrderProfile {
    pub(crate) non_overlapping_calls: usize,
    pub(crate) overlapping_calls: usize,
    pub(crate) exact_skill_delta_calls: usize,
    pub(crate) overlap_check_ms: f64,
    pub(crate) base_score_ms: f64,
    pub(crate) assignment_ms: f64,
    pub(crate) exact_skill_ms: f64,
}

impl ExactSkillOrderProfile {
    pub(crate) fn add(&mut self, other: &Self) {
        self.non_overlapping_calls += other.non_overlapping_calls;
        self.overlapping_calls += other.overlapping_calls;
        self.exact_skill_delta_calls += other.exact_skill_delta_calls;
        self.overlap_check_ms += other.overlap_check_ms;
        self.base_score_ms += other.base_score_ms;
        self.assignment_ms += other.assignment_ms;
        self.exact_skill_ms += other.exact_skill_ms;
    }
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
    fever_start: Option<f64>,
    fever_enabled: bool,
    score_rule: ScoreRule,
    score_factors: Vec<f64>,
    skill_node_indices: Vec<usize>,
}

impl Chart {
    #[inline]
    fn uses_ideal_60fps_timing(&self) -> bool {
        self.score_rule.simultaneous_skill_order == SimultaneousSkillOrder::BeforeNotes
    }

    #[inline]
    fn scoring_skill_end_time(&self, trigger: f64, duration: f64) -> f64 {
        if self.uses_ideal_60fps_timing() {
            ideal_skill_end_time(trigger, duration)
        } else {
            trigger + skill_end_interval(duration).upper
        }
    }

    #[inline]
    fn is_after_scoring_boundary(&self, time: f64, boundary: f64) -> bool {
        if self.uses_ideal_60fps_timing() {
            time > boundary
        } else {
            sgn(time - boundary) > 0
        }
    }

    pub fn new(level: i32, nodes: Vec<ChartNode>) -> Self {
        Self::new_with_fever_start(level, nodes, None)
    }

    pub fn new_with_fever_start(
        level: i32,
        mut nodes: Vec<ChartNode>,
        fever_start: Option<f64>,
    ) -> Self {
        nodes.sort_by(|a, b| {
            let time_order = sgn(a.time - b.time);
            if time_order != 0 {
                return time_order.cmp(&0);
            }
            node_sort_value(a.node_type).cmp(&node_sort_value(b.node_type))
        });
        let count = nodes.len();
        let skill_node_indices = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| (node.node_type == ChartNodeType::Skill).then_some(index))
            .collect();
        Self {
            nodes,
            level,
            count,
            combo: 0,
            score_as_medley: false,
            warning: Vec::new(),
            meta: ChartMeta::default(),
            fever_start,
            fever_enabled: false,
            score_rule: ScoreRule::STANDARD,
            score_factors: Vec::new(),
            skill_node_indices,
        }
    }

    pub fn init(&mut self, combo: i32, is_medley: bool) -> Result<(), ChartError> {
        self.init_with_rule_and_fever(combo, is_medley, ScoreRule::STANDARD, false)
    }

    pub fn init_with_fever(&mut self, combo: i32, is_medley: bool) -> Result<(), ChartError> {
        self.init_with_rule_and_fever(combo, is_medley, ScoreRule::STANDARD, true)
    }

    pub fn has_fever_section(&self) -> bool {
        self.fever_start.is_some()
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
        // The compressed score uses the same ideal 60 FPS, phase-zero scheduler as
        // maximize. Auto ordering is still preserved: every node at the skill's
        // timestamp is counted before the skill becomes active.
        let mut scheduler = SkillScheduler::Ideal60(Ideal60SkillScheduler::default());

        for node in &self.nodes {
            while events
                .front()
                .is_some_and(|event| scheduler.event_is_due(node.time, event.time))
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
            let scheduled = scheduler.schedule(node.time, duration);
            if scheduled.start > node.time {
                events.push_back(WindowEvent {
                    time: scheduled.start,
                    active_window: Some(window),
                });
                events.push_back(WindowEvent {
                    time: scheduled.end,
                    active_window: None,
                });
            } else {
                active_window = Some(window);
                events.push_back(WindowEvent {
                    time: scheduled.end,
                    active_window: None,
                });
            }
        }

        // Risk detection deliberately remains a separate 60/120 FPS envelope.
        // A risky duration bucket is filtered before score-range search uses the
        // ideal compressed counts above.
        let (skill_indices, skill_count) = self.six_skill_indices()?;
        let timeline = self.non_rateup_timeline_for_indices(
            duration,
            &skill_indices[..usize::from(skill_count)],
        )?;
        Ok(Some(ScoreRangeSkillWindowCounts {
            inactive_nodes,
            active_nodes,
            tail_risk: self.timeline_has_skill_tail_risk(duration, &timeline),
            skill_queue_risk: timeline.skill_queue_risk,
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

    pub fn has_skill_queue_risk(&self, duration: f64) -> Result<bool, ChartError> {
        Ok(self.non_rateup_timeline(duration)?.skill_queue_risk)
    }

    fn timeline_has_skill_tail_risk(&self, duration: f64, timeline: &NonRateupTimeline) -> bool {
        timeline
            .effective_starts
            .iter()
            .any(|&start| self.has_skill_tail_risk_at(start, duration))
    }

    fn has_skill_tail_risk_at(&self, start: f64, duration: f64) -> bool {
        let end = skill_end_interval(duration);
        let lower = start + end.lower;
        let upper = start + end.upper;
        let first = self
            .nodes
            .partition_point(|node| sgn(node.time - lower) < 0);
        self.nodes
            .get(first)
            .is_some_and(|node| sgn(node.time - upper) <= 0)
    }

    pub fn init_with_rule(
        &mut self,
        combo: i32,
        is_medley: bool,
        score_rule: ScoreRule,
    ) -> Result<(), ChartError> {
        self.init_with_rule_and_fever(combo, is_medley, score_rule, false)
    }

    fn init_with_rule_and_fever(
        &mut self,
        combo: i32,
        is_medley: bool,
        score_rule: ScoreRule,
        fever_enabled: bool,
    ) -> Result<(), ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        sort_nodes(&mut self.nodes, score_rule.simultaneous_skill_order);
        self.combo = combo;
        self.score_as_medley = is_medley;
        self.fever_enabled = fever_enabled;
        self.score_rule = score_rule;
        self.warning.clear();
        self.meta = ChartMeta::default();
        self.score_factors.clear();
        self.score_factors.reserve(self.nodes.len());
        self.skill_node_indices.clear();
        self.skill_node_indices.reserve(6);

        let mut combo_cursor = combo;
        let base = 3.0 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64;

        for (node_idx, node) in self.nodes.iter().enumerate() {
            combo_cursor += 1;
            let score_factor = base
                * score_rule.base_multiplier
                * combo_mod(combo_cursor, is_medley, score_rule.combo_mode);
            self.score_factors.push(score_factor);
            self.meta.no_skill += score_factor * self.fever_multiplier_at_node(node_idx);

            if node.node_type != ChartNodeType::Skill {
                continue;
            }
            self.skill_node_indices.push(node_idx);

            let mut skill = BTreeMap::new();
            for duration in SKILL_DURATIONS {
                let mut temp_combo = combo_cursor;
                let mut value = 0.0;
                let deadline = self.scoring_skill_end_time(node.time, duration);
                for (later_idx, later) in self.nodes.iter().enumerate().skip(node_idx + 1) {
                    if self.is_after_scoring_boundary(later.time, deadline) {
                        break;
                    }
                    temp_combo += 1;
                    value += base
                        * score_rule.base_multiplier
                        * combo_mod(temp_combo, is_medley, score_rule.combo_mode)
                        * self.fever_multiplier_at_node(later_idx);
                }
                skill.insert(duration_key(duration), value);
            }
            self.meta.skill.push(skill);

            let mut rateup_skill = BTreeMap::new();
            for duration in RATEUP_DURATIONS {
                let mut temp_combo = combo_cursor;
                let mut skill_mod = 200.0;
                let mut value = 0.0;
                let deadline = self.scoring_skill_end_time(node.time, duration);
                for (later_idx, later) in self.nodes.iter().enumerate().skip(node_idx + 1) {
                    if skill_mod < 300.0 {
                        skill_mod += 1.0;
                    }
                    if self.is_after_scoring_boundary(later.time, deadline) {
                        break;
                    }
                    temp_combo += 1;
                    value += base
                        * score_rule.base_multiplier
                        * combo_mod(temp_combo, is_medley, score_rule.combo_mode)
                        * skill_mod
                        / 200.0
                        * self.fever_multiplier_at_node(later_idx);
                }
                rateup_skill.insert(duration_key(duration), value);
            }
            self.meta.rateup.push(rateup_skill);
        }

        for idx in 0..self.skill_node_indices.len().saturating_sub(1) {
            let first_time = self.nodes[self.skill_node_indices[idx]].time;
            let next_time = self.nodes[self.skill_node_indices[idx + 1]].time;
            let time_gap = next_time - first_time;
            let may_queue = if self.uses_ideal_60fps_timing() {
                ideal_skills_queue(first_time, 8.0, next_time)
            } else {
                sgn(time_gap - skill_queue_gap_interval(8.0).upper) <= 0
            };
            if may_queue {
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
        self.skill_delta_at_stat_for_mode(activation, skill, stat, self.score_as_medley)
    }

    fn skill_delta_at_stat_for_mode(
        &self,
        activation: usize,
        skill: TeamCardSkill,
        stat: i32,
        is_medley: bool,
    ) -> Result<f64, ChartError> {
        Ok(self.skill_delta_at_stat_i32(activation, skill, stat, is_medley)? as f64)
    }

    fn skill_delta_at_stat_i32(
        &self,
        activation: usize,
        skill: TeamCardSkill,
        stat: i32,
        is_medley: bool,
    ) -> Result<i32, ChartError> {
        let window = self.compile_exact_skill_window(activation, skill)?;
        Ok(self.skill_delta_for_window_i32(window, stat, is_medley))
    }

    pub(crate) fn compile_exact_skill_window(
        &self,
        activation: usize,
        skill: TeamCardSkill,
    ) -> Result<ExactSkillWindow, ChartError> {
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
            .skill_node_indices
            .get(activation)
            .copied()
            .ok_or(ChartError::MissingSkillMeta { activation })?;
        let skill_time = self.nodes[skill_node_idx].time;
        let deadline = self.scoring_skill_end_time(skill_time, skill.duration);
        let range_end = self
            .nodes
            .partition_point(|node| !self.is_after_scoring_boundary(node.time, deadline));
        Ok(ExactSkillWindow {
            range_start: skill_node_idx + 1,
            range_end,
            score_up: skill.score_up,
            rateup: skill.rateup,
        })
    }

    #[inline]
    fn skill_delta_for_window_i32(
        &self,
        window: ExactSkillWindow,
        stat: i32,
        is_medley: bool,
    ) -> i32 {
        let base = 3.0 * stat as f64 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64;
        if !self.fever_enabled
            && !window.rateup
            && self.score_as_medley == is_medley
            && avx2_available()
        {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                // SAFETY: runtime AVX2 support is checked above and the score
                // factors cover every chart node.
                return unsafe {
                    score_factor_range_delta_sum_avx2(
                        &self.score_factors[window.range_start..window.range_end],
                        stat,
                        1.0 + window.score_up,
                    )
                };
            }
        }
        let mut total = 0i32;
        let mut rateup_mod = 1.0 + window.score_up;

        for node_idx in window.range_start..window.range_end {
            let combo = self.combo + node_idx as i32 + 1;
            let no_skill = (self.exact_base_score_at_node(node_idx, stat, combo, is_medley, base)
                * self.fever_multiplier_at_node(node_idx)) as i32;
            let skill_mod = if window.rateup {
                if sgn(rateup_mod - 2.5) < 0 {
                    rateup_mod += 0.005;
                }
                rateup_mod
            } else {
                1.0 + window.score_up
            };
            total += (no_skill as f64 * skill_mod).floor() as i32 - no_skill;
        }

        total
    }

    pub fn no_skill_score_at_stat(&self, stat: i32) -> Result<f64, ChartError> {
        Ok(self.exact_no_skill_score_i32(stat, self.score_as_medley)? as f64)
    }

    fn exact_no_skill_score_i32(&self, stat: i32, is_medley: bool) -> Result<i32, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }

        if !self.fever_enabled && self.score_as_medley == is_medley && avx2_available() {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                // SAFETY: runtime AVX2 support is checked above.
                return Ok(unsafe { score_factor_range_sum_avx2(&self.score_factors, stat, 1.0) });
            }
        }

        let base = 3.0 * stat as f64 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64;
        Ok(self
            .nodes
            .iter()
            .enumerate()
            .map(|(node_idx, _)| {
                (self.exact_base_score_at_node(
                    node_idx,
                    stat,
                    self.combo + node_idx as i32 + 1,
                    is_medley,
                    base,
                ) * self.fever_multiplier_at_node(node_idx)) as i32
            })
            .sum())
    }

    fn populate_exact_base_scores(
        &self,
        stat: i32,
        is_medley: bool,
        output: &mut Vec<i32>,
    ) -> Result<i32, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }
        output.resize(self.nodes.len(), 0);

        if !self.fever_enabled && self.score_as_medley == is_medley && avx2_available() {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                // SAFETY: runtime AVX2 support is checked above and the output
                // has exactly one slot for every score factor.
                return Ok(unsafe {
                    score_factor_base_scores_avx2(&self.score_factors, stat, output)
                });
            }
        }

        let base = 3.0 * stat as f64 * (1.0 + 0.01 * (self.level as f64 - 5.0)) / self.count as f64;
        let mut total = 0i32;
        for (node_idx, score) in output.iter_mut().enumerate() {
            let combo = self.combo + node_idx as i32 + 1;
            *score = (self.exact_base_score_at_node(node_idx, stat, combo, is_medley, base)
                * self.fever_multiplier_at_node(node_idx)) as i32;
            total += *score;
        }
        Ok(total)
    }

    #[inline]
    fn constant_delta_from_base_scores(&self, base_scores: &[i32], skill_mod: f64) -> i32 {
        if avx2_available() {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                // SAFETY: runtime AVX2 support is checked above.
                return unsafe { base_score_constant_delta_sum_avx2(base_scores, skill_mod) };
            }
        }

        base_scores
            .iter()
            .map(|&base| (base as f64 * skill_mod).floor() as i32 - base)
            .sum()
    }

    #[inline]
    fn rateup_delta_from_base_scores(&self, base_scores: &[i32], multipliers: &[f64]) -> i32 {
        debug_assert_eq!(base_scores.len(), multipliers.len());
        if avx2_available() {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                // SAFETY: runtime AVX2 support is checked above and both slices
                // have identical lengths.
                return unsafe { base_score_multiplier_delta_sum_avx2(base_scores, multipliers) };
            }
        }

        base_scores
            .iter()
            .zip(multipliers)
            .map(|(&base, &multiplier)| (base as f64 * multiplier).floor() as i32 - base)
            .sum()
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

    pub fn get_max_score_order(
        &self,
        team: &[TeamCardSkill],
        stat: i32,
        is_medley: bool,
    ) -> Result<MaxScoreOrder, ChartError> {
        let team: &[TeamCardSkill; 5] = team
            .try_into()
            .map_err(|_| ChartError::InvalidTeamSize { count: team.len() })?;
        let seed = self.get_max_meta_order(team)?;
        let seed_order_indices: [usize; 5] = seed
            .order_indices
            .as_slice()
            .try_into()
            .expect("max-meta order contains exactly five cards");
        let mut skill_windows = [[ExactSkillWindow::default(); 6]; 5];
        for card_idx in 0..5 {
            for activation in 0..6 {
                skill_windows[card_idx][activation] =
                    self.compile_exact_skill_window(activation, team[card_idx])?;
            }
        }
        let mut scratch = ExactScoreScratch::default();
        let exact = self.get_max_score_order_from_exact_windows(
            team,
            stat,
            is_medley,
            seed_order_indices,
            seed.captain_index,
            &skill_windows,
            &mut scratch,
        )?;
        let mut score_up_order = exact
            .order_indices
            .iter()
            .map(|&idx| team[idx].score_up)
            .collect::<Vec<_>>();
        score_up_order.push(team[exact.captain_index].score_up);

        Ok(MaxScoreOrder {
            score: exact.score,
            order_indices: exact.order_indices.to_vec(),
            captain_index: exact.captain_index,
            score_up_order,
        })
    }

    pub(crate) fn get_max_score_order_from_exact_windows(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        seed_order_indices: [usize; 5],
        seed_captain_index: usize,
        skill_windows: &[[ExactSkillWindow; 6]; 5],
        scratch: &mut ExactScoreScratch,
    ) -> Result<ExactSkillOrder, ChartError> {
        self.get_max_score_order_from_exact_windows_internal::<false>(
            team,
            stat,
            is_medley,
            seed_order_indices,
            seed_captain_index,
            skill_windows,
            scratch,
            None,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn get_independent_medley_score_order_from_exact_windows(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        seed_order_indices: [usize; 5],
        seed_captain_index: usize,
        skill_windows: &[[ExactSkillWindow; 6]; 5],
        scratch: &mut ExactScoreScratch,
    ) -> Result<ExactSkillOrder, ChartError> {
        self.get_max_score_order_from_exact_windows_internal::<false>(
            team,
            stat,
            is_medley,
            seed_order_indices,
            seed_captain_index,
            skill_windows,
            scratch,
            None,
            false,
        )
    }

    /// Builds the exact additive 5x6 skill matrix when the selected skills do not queue.
    /// Returns `None` when the strict queued timeline is required.
    pub(crate) fn independent_skill_score_matrix(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        scratch: &mut ExactScoreScratch,
    ) -> Result<Option<IndependentSkillScoreMatrix>, ChartError> {
        if self.team_skills_may_overlap(team)? {
            return Ok(None);
        }
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }
        let (_, skill_count) = self.six_skill_indices()?;
        if skill_count != 6 {
            return Err(ChartError::MissingSkillProfile {
                activation: usize::from(skill_count),
            });
        }

        let mut skill_windows = [[ExactSkillWindow::default(); 6]; 5];
        for card_idx in 0..5 {
            skill_windows[card_idx] = self.compile_exact_skill_windows(team[card_idx])?;
        }

        self.independent_skill_score_matrix_from_windows(
            team,
            stat,
            is_medley,
            &skill_windows,
            scratch,
        )
        .map(Some)
    }

    pub(crate) fn compile_exact_skill_windows(
        &self,
        skill: TeamCardSkill,
    ) -> Result<[ExactSkillWindow; 6], ChartError> {
        let mut windows = [ExactSkillWindow::default(); 6];
        for (activation, window) in windows.iter_mut().enumerate() {
            *window = self.compile_exact_skill_window(activation, skill)?;
        }
        Ok(windows)
    }

    pub(crate) fn independent_skill_score_matrix_from_windows(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        skill_windows: &[[ExactSkillWindow; 6]; 5],
        scratch: &mut ExactScoreScratch,
    ) -> Result<IndependentSkillScoreMatrix, ChartError> {
        let base_score =
            self.populate_exact_base_scores(stat, is_medley, &mut scratch.base_scores)?;
        for card_idx in 0..5 {
            if !team[card_idx].rateup {
                continue;
            }
            let max_len = skill_windows[card_idx]
                .iter()
                .map(|window| window.range_end - window.range_start)
                .max()
                .unwrap_or(0);
            let padded_len = max_len.saturating_add(3) & !3;
            scratch.rateup_profiles[card_idx].prepare(team[card_idx].score_up, padded_len);
        }

        let mut deltas = [[0i32; 6]; 5];
        if avx2_available() {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            for activation in 0..6 {
                let windows = std::array::from_fn(|card_idx| skill_windows[card_idx][activation]);
                let profiles: [&[f64]; 5] = std::array::from_fn(|card_idx| {
                    scratch.rateup_profiles[card_idx].multipliers.as_slice()
                });
                // SAFETY: runtime AVX2 support is checked above. All windows for one
                // activation share their start, and rate-up profiles are vector-padded.
                let activation_deltas = unsafe {
                    five_skill_deltas_from_base_scores_avx2(
                        &scratch.base_scores,
                        &windows,
                        &profiles,
                    )
                };
                for card_idx in 0..5 {
                    deltas[card_idx][activation] = activation_deltas[card_idx];
                }
            }
        } else {
            for activation in 0..6 {
                let windows = std::array::from_fn(|card_idx| skill_windows[card_idx][activation]);
                let representatives = skill_window_representatives(&windows);
                for card_idx in 0..5 {
                    let representative = representatives[card_idx];
                    if representative != card_idx {
                        deltas[card_idx][activation] = deltas[representative][activation];
                        continue;
                    }
                    let window = skill_windows[card_idx][activation];
                    let base_scores = &scratch.base_scores[window.range_start..window.range_end];
                    deltas[card_idx][activation] = if window.rateup {
                        self.rateup_delta_from_base_scores(
                            base_scores,
                            &scratch.rateup_profiles[card_idx].multipliers[..base_scores.len()],
                        )
                    } else {
                        self.constant_delta_from_base_scores(base_scores, 1.0 + window.score_up)
                    };
                }
            }
        }

        Ok(IndependentSkillScoreMatrix { base_score, deltas })
    }

    // Retained for diagnostics and strict queued-timeline verification. Production Medley uses
    // the independent-overlap entries so every chart follows the same 5x6 matrix path.
    #[allow(dead_code, clippy::too_many_arguments)]
    pub(crate) fn get_max_score_order_from_exact_windows_profiled(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        seed_order_indices: [usize; 5],
        seed_captain_index: usize,
        skill_windows: &[[ExactSkillWindow; 6]; 5],
        scratch: &mut ExactScoreScratch,
        profile: &mut ExactSkillOrderProfile,
    ) -> Result<ExactSkillOrder, ChartError> {
        self.get_max_score_order_from_exact_windows_internal::<true>(
            team,
            stat,
            is_medley,
            seed_order_indices,
            seed_captain_index,
            skill_windows,
            scratch,
            Some(profile),
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn get_independent_medley_score_order_from_exact_windows_profiled(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        seed_order_indices: [usize; 5],
        seed_captain_index: usize,
        skill_windows: &[[ExactSkillWindow; 6]; 5],
        scratch: &mut ExactScoreScratch,
        profile: &mut ExactSkillOrderProfile,
    ) -> Result<ExactSkillOrder, ChartError> {
        self.get_max_score_order_from_exact_windows_internal::<true>(
            team,
            stat,
            is_medley,
            seed_order_indices,
            seed_captain_index,
            skill_windows,
            scratch,
            Some(profile),
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn get_max_score_order_from_exact_windows_internal<const PROFILE: bool>(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        seed_order_indices: [usize; 5],
        seed_captain_index: usize,
        skill_windows: &[[ExactSkillWindow; 6]; 5],
        scratch: &mut ExactScoreScratch,
        mut profile: Option<&mut ExactSkillOrderProfile>,
        use_queued_timeline_on_overlap: bool,
    ) -> Result<ExactSkillOrder, ChartError> {
        let started = PROFILE.then(Timer::start);
        let overlaps = self.team_skills_may_overlap(team)?;
        if let (Some(profile), Some(started)) = (profile.as_deref_mut(), started) {
            profile.overlap_check_ms += started.elapsed_ms();
        }
        if overlaps {
            if let Some(profile) = profile.as_deref_mut() {
                profile.overlapping_calls += 1;
            }
            if use_queued_timeline_on_overlap {
                return self.overlapping_max_score_order(
                    team,
                    stat,
                    is_medley,
                    seed_order_indices,
                    seed_captain_index,
                );
            }
        } else if let Some(profile) = profile.as_deref_mut() {
            profile.non_overlapping_calls += 1;
        }

        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }
        let (_, skill_count) = self.six_skill_indices()?;
        if skill_count != 6 {
            return Err(ChartError::MissingSkillProfile {
                activation: usize::from(skill_count),
            });
        }

        let started = PROFILE.then(Timer::start);
        let base_score =
            self.populate_exact_base_scores(stat, is_medley, &mut scratch.base_scores)?;
        if let (Some(profile), Some(started)) = (profile.as_deref_mut(), started) {
            profile.base_score_ms += started.elapsed_ms();
        }

        let started = PROFILE.then(Timer::start);
        for card_idx in 0..5 {
            if !team[card_idx].rateup {
                continue;
            }
            let max_len = skill_windows[card_idx]
                .iter()
                .map(|window| window.range_end - window.range_start)
                .max()
                .unwrap_or(0);
            let padded_len = max_len.saturating_add(3) & !3;
            scratch.rateup_profiles[card_idx].prepare(team[card_idx].score_up, padded_len);
        }

        let mut deltas = [[0i32; 6]; 5];
        let mut exact_skill_delta_calls = 0usize;
        if avx2_available() {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            for activation in 0..6 {
                let windows = std::array::from_fn(|card_idx| skill_windows[card_idx][activation]);
                if PROFILE {
                    let representatives = skill_window_representatives(&windows);
                    exact_skill_delta_calls += representatives
                        .iter()
                        .enumerate()
                        .filter(|&(card_idx, &representative)| card_idx == representative)
                        .count();
                }
                let profiles: [&[f64]; 5] = std::array::from_fn(|card_idx| {
                    scratch.rateup_profiles[card_idx].multipliers.as_slice()
                });
                // SAFETY: runtime AVX2 support is checked above. All five
                // windows for one activation share their range start, and the
                // rate-up profiles are padded to a full vector.
                let activation_deltas = unsafe {
                    five_skill_deltas_from_base_scores_avx2(
                        &scratch.base_scores,
                        &windows,
                        &profiles,
                    )
                };
                for card_idx in 0..5 {
                    deltas[card_idx][activation] = activation_deltas[card_idx];
                }
            }
        } else {
            for activation in 0..6 {
                let windows = std::array::from_fn(|card_idx| skill_windows[card_idx][activation]);
                let representatives = skill_window_representatives(&windows);
                if PROFILE {
                    exact_skill_delta_calls += representatives
                        .iter()
                        .enumerate()
                        .filter(|&(card_idx, &representative)| card_idx == representative)
                        .count();
                }
                for card_idx in 0..5 {
                    let representative = representatives[card_idx];
                    if representative != card_idx {
                        deltas[card_idx][activation] = deltas[representative][activation];
                        continue;
                    }
                    let window = skill_windows[card_idx][activation];
                    let base_scores = &scratch.base_scores[window.range_start..window.range_end];
                    deltas[card_idx][activation] = if window.rateup {
                        self.rateup_delta_from_base_scores(
                            base_scores,
                            &scratch.rateup_profiles[card_idx].multipliers[..base_scores.len()],
                        )
                    } else {
                        self.constant_delta_from_base_scores(base_scores, 1.0 + window.score_up)
                    };
                }
            }
        }
        if let (Some(profile), Some(started)) = (profile.as_deref_mut(), started) {
            profile.exact_skill_ms += started.elapsed_ms();
            profile.exact_skill_delta_calls += exact_skill_delta_calls;
        }

        let started = PROFILE.then(Timer::start);
        let (best_delta, order_indices, captain_index) = max_independent_skill_delta(&deltas);
        if let (Some(profile), Some(started)) = (profile.as_deref_mut(), started) {
            profile.assignment_ms += started.elapsed_ms();
        }

        let seed_delta = seed_order_indices
            .iter()
            .enumerate()
            .map(|(activation, &card_idx)| deltas[card_idx][activation])
            .sum::<i32>()
            + deltas[seed_captain_index][5];
        if seed_delta == best_delta {
            return Ok(ExactSkillOrder {
                score: base_score + seed_delta,
                order_indices: seed_order_indices,
                captain_index: seed_captain_index,
            });
        }

        Ok(ExactSkillOrder {
            score: base_score + best_delta,
            order_indices,
            captain_index,
        })
    }

    pub(crate) fn team_skills_may_overlap(
        &self,
        team: &[TeamCardSkill; 5],
    ) -> Result<bool, ChartError> {
        if self.warning.is_empty() {
            return Ok(false);
        }
        let (skill_indices, skill_count) = self.six_skill_indices()?;
        if skill_count != 6 {
            return Err(ChartError::MissingSkillProfile {
                activation: usize::from(skill_count),
            });
        }
        let max_duration = team
            .iter()
            .map(|skill| skill.duration)
            .fold(0.0_f64, f64::max);
        Ok(skill_indices.windows(2).any(|pair| {
            let first = self.nodes[pair[0]].time;
            let next = self.nodes[pair[1]].time;
            if self.uses_ideal_60fps_timing() {
                ideal_skills_queue(first, max_duration, next)
            } else {
                sgn(next - first - skill_queue_gap_interval(max_duration).upper) <= 0
            }
        }))
    }

    fn overlapping_max_score_order(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        seed_order_indices: [usize; 5],
        seed_captain_index: usize,
    ) -> Result<ExactSkillOrder, ChartError> {
        self.exhaustive_max_score_order_fallback(
            team,
            stat,
            is_medley,
            seed_order_indices,
            seed_captain_index,
        )
    }

    fn exhaustive_max_score_order_fallback(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        seed_order_indices: [usize; 5],
        seed_captain_index: usize,
    ) -> Result<ExactSkillOrder, ChartError> {
        let mut best = ExactSkillOrder {
            score: self.score_five_skill_order(
                team,
                stat,
                is_medley,
                seed_order_indices,
                seed_captain_index,
            )?,
            order_indices: seed_order_indices,
            captain_index: seed_captain_index,
        };
        let mut order_indices = [0, 1, 2, 3, 4];
        loop {
            for captain_index in 0..5 {
                let score = self.score_five_skill_order(
                    team,
                    stat,
                    is_medley,
                    order_indices,
                    captain_index,
                )?;
                if score > best.score {
                    best = ExactSkillOrder {
                        score,
                        order_indices,
                        captain_index,
                    };
                }
            }
            if !next_permutation(&mut order_indices) {
                break;
            }
        }
        Ok(best)
    }

    fn score_five_skill_order(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        order_indices: [usize; 5],
        captain_index: usize,
    ) -> Result<i32, ChartError> {
        let skill_order: [TeamCardSkill; 6] = std::array::from_fn(|activation| {
            if activation == 5 {
                team[captain_index]
            } else {
                team[order_indices[activation]]
            }
        });
        self.get_score_for_six_skills(&skill_order, stat, is_medley)
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
        let mut scheduler = SkillScheduler::new(self.uses_ideal_60fps_timing());

        for (node_idx, node) in self.nodes.iter().enumerate() {
            while events
                .front()
                .map(|event| scheduler.event_is_due(node.time, event.time))
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
            result +=
                (no_skill * skill_mod * self.fever_multiplier_at_node(node_idx)).floor() as i32;

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

            let scheduled = scheduler.schedule(node.time, skill.duration);
            if sgn(scheduled.start - node.time) > 0 {
                events.push_back(SkillEvent {
                    time: scheduled.start,
                    skill_mod: 1.0 + skill.score_up,
                    rateup: skill.rateup,
                });
                events.push_back(SkillEvent {
                    time: scheduled.end,
                    skill_mod: 1.0,
                    rateup: false,
                });
            } else {
                skill_mod = 1.0 + skill.score_up;
                rateup = skill.rateup;
                events.push_back(SkillEvent {
                    time: scheduled.end,
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
        if !self.fever_enabled && self.score_as_medley == is_medley && avx2_available() {
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

    pub(crate) fn compile_six_skill_score(
        &self,
        skill_order: &[TeamCardSkill; 6],
        is_medley: bool,
    ) -> Result<CompiledSixSkillScore, ChartError> {
        if self.score_as_medley != is_medley {
            return Err(ChartError::ScoreModeMismatch);
        }
        let mut multipliers = Vec::with_capacity(self.nodes.len());
        self.for_each_multiplier_for_six_skills(skill_order, |value| multipliers.push(value))?;
        Ok(CompiledSixSkillScore { multipliers })
    }

    pub(crate) fn score_compiled_six_skills(
        &self,
        compiled: &CompiledSixSkillScore,
        stat: i32,
    ) -> i32 {
        compiled.score(&self.score_factors, stat)
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
        let mut scheduler = SkillScheduler::new(self.uses_ideal_60fps_timing());

        for (node_idx, node) in self.nodes.iter().enumerate() {
            while events
                .front()
                .map(|event| scheduler.event_is_due(node.time, event.time))
                .unwrap_or(false)
            {
                let event = events.pop_front().expect("front event exists");
                skill_mod = event.skill_mod;
                rateup = event.rateup;
            }

            if rateup && sgn(skill_mod - 2.5) < 0 {
                skill_mod += 0.005;
            }
            visit(skill_mod * self.fever_multiplier_at_node(node_idx));

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

            let scheduled = scheduler.schedule(node.time, skill.duration);
            if sgn(scheduled.start - node.time) > 0 {
                events.push_back(SkillEvent {
                    time: scheduled.start,
                    skill_mod: 1.0 + skill.score_up,
                    rateup: skill.rateup,
                });
                events.push_back(SkillEvent {
                    time: scheduled.end,
                    skill_mod: 1.0,
                    rateup: false,
                });
            } else {
                skill_mod = 1.0 + skill.score_up;
                rateup = skill.rateup;
                events.push_back(SkillEvent {
                    time: scheduled.end,
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
        for (skill_count, &index) in self.skill_node_indices.iter().enumerate() {
            let Some(slot) = skill_indices.get_mut(skill_count) else {
                return Err(ChartError::MissingSkillProfile {
                    activation: skill_count,
                });
            };
            *slot = index;
        }
        Ok((skill_indices, self.skill_node_indices.len() as u8))
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
        let mut skill_queue_risk = false;
        let mut scheduler = EnvelopeSkillScheduler::default();

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
                // Several queued start/end events may all fall between the
                // same two chart nodes. They must be applied at the same
                // target node so that only the last event determines its
                // multiplier.
                event_min_index = event_index;
                continue;
            }

            let Some(skill_index) = next_skill_index else {
                break;
            };
            next_skill += 1;
            let node = &self.nodes[skill_index];
            let scheduled = scheduler.schedule(node.time, duration);
            skill_queue_risk |= scheduled.queue_risk;
            effective_starts.push(scheduled.start);
            if sgn(scheduled.start - node.time) <= 0 {
                if active_start.is_none() {
                    active_start = Some(skill_index.saturating_add(1));
                }
                events.push_back(SkillEvent {
                    time: scheduled.end,
                    skill_mod: 1.0,
                    rateup: false,
                });
                event_min_index = skill_index.saturating_add(1);
            } else {
                events.push_back(SkillEvent {
                    time: scheduled.start,
                    skill_mod: 2.0,
                    rateup: false,
                });
                events.push_back(SkillEvent {
                    time: scheduled.end,
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
            skill_queue_risk,
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
        let mut scheduler = SkillScheduler::new(self.uses_ideal_60fps_timing());

        for (node_idx, node) in self.nodes.iter().enumerate() {
            while let Some(event) = events.pop_front_if_after(node.time, scheduler) {
                skill_mod = event.skill_mod;
                rateup = event.rateup;
            }

            combo += 1;
            if rateup && sgn(skill_mod - 2.5) < 0 {
                skill_mod += 0.005;
            }

            let no_skill =
                self.exact_base_score_at_node(node_idx, stat, combo, is_medley, base) as f64;
            result +=
                (no_skill * skill_mod * self.fever_multiplier_at_node(node_idx)).floor() as i32;

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

            let scheduled = scheduler.schedule(node.time, skill.duration);
            if sgn(scheduled.start - node.time) > 0 {
                if !events.push_back(SkillEvent {
                    time: scheduled.start,
                    skill_mod: 1.0 + skill.score_up,
                    rateup: skill.rateup,
                }) || !events.push_back(SkillEvent {
                    time: scheduled.end,
                    skill_mod: 1.0,
                    rateup: false,
                }) {
                    return self.get_score(skill_order, stat, is_medley);
                }
            } else {
                skill_mod = 1.0 + skill.score_up;
                rateup = skill.rateup;
                if !events.push_back(SkillEvent {
                    time: scheduled.end,
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
        let mut scheduler = SkillScheduler::new(self.uses_ideal_60fps_timing());
        let mut node_idx = 0usize;

        while node_idx < self.nodes.len() {
            let node = &self.nodes[node_idx];
            while let Some(event) = events.pop_front_if_after(node.time, scheduler) {
                skill_mod = event.skill_mod;
                rateup = event.rateup;
            }

            if !rateup && node.node_type != ChartNodeType::Skill {
                let range_start = node_idx;
                let mut range_end = node_idx + 1;
                while range_end < self.nodes.len()
                    && self.nodes[range_end].node_type != ChartNodeType::Skill
                    && !events.should_pop_at(self.nodes[range_end].time, scheduler)
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

            let scheduled = scheduler.schedule(node.time, skill.duration);
            if sgn(scheduled.start - node.time) > 0 {
                if !events.push_back(SkillEvent {
                    time: scheduled.start,
                    skill_mod: 1.0 + skill.score_up,
                    rateup: skill.rateup,
                }) || !events.push_back(SkillEvent {
                    time: scheduled.end,
                    skill_mod: 1.0,
                    rateup: false,
                }) {
                    return self.get_score_for_six_skills_scalar(skill_order, stat, is_medley);
                }
            } else {
                skill_mod = 1.0 + skill.score_up;
                rateup = skill.rateup;
                if !events.push_back(SkillEvent {
                    time: scheduled.end,
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

    /// Continuous, floor-free upper bound for one skill at any effective start time.
    /// It remains conservative when closely spaced skills are queued into later chart regions.
    pub(crate) fn optimistic_skill_meta_any_window(
        &self,
        skill: TeamCardSkill,
    ) -> Result<f64, ChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart);
        }
        let window = skill_end_interval(skill.duration).upper;
        let mut best = 0.0_f64;
        for first in 0..self.nodes.len() {
            let start_time = self.nodes[first].time;
            let mut total = 0.0_f64;
            let mut multiplier = 1.0 + skill.score_up;
            for idx in first..self.nodes.len() {
                if sgn(self.nodes[idx].time - start_time - window) > 0 {
                    break;
                }
                if skill.rateup && sgn(multiplier - 2.5) < 0 {
                    multiplier += 0.005;
                }
                total += self.score_factors[idx]
                    * self.fever_multiplier_at_node(idx)
                    * (multiplier - 1.0);
            }
            best = best.max(total);
        }
        Ok(best)
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

    #[inline]
    fn fever_multiplier_at_node(&self, node_idx: usize) -> f64 {
        if self.fever_enabled
            && self
                .fever_start
                .is_some_and(|start| self.nodes[node_idx].time > start)
        {
            2.0
        } else {
            1.0
        }
    }
}

fn max_independent_skill_delta(deltas: &[[i32; 6]; 5]) -> (i32, [usize; 5], usize) {
    if deltas[1..].iter().all(|row| row == &deltas[0]) {
        return (
            deltas[0][..5].iter().sum::<i32>() + deltas[0][5],
            [0, 1, 2, 3, 4],
            0,
        );
    }

    let mut dp = [i32::MIN; 1 << 5];
    let mut chosen = [usize::MAX; 1 << 5];
    dp[0] = 0;
    for mask in 0usize..(1 << 5) - 1 {
        let activation = FIVE_CARD_MASK_POPCOUNT[mask] as usize;
        let mut available = (!mask) & ((1 << 5) - 1);
        while available != 0 {
            let card_idx = available.trailing_zeros() as usize;
            available &= available - 1;
            let next = mask | (1 << card_idx);
            let value = dp[mask] + deltas[card_idx][activation];
            if value > dp[next] {
                dp[next] = value;
                chosen[next] = card_idx;
            }
        }
    }

    let mut captain_index = 0usize;
    for card_idx in 1..5 {
        if deltas[card_idx][5] > deltas[captain_index][5] {
            captain_index = card_idx;
        }
    }

    let mut order_indices = [0usize; 5];
    let mut mask = (1 << 5) - 1;
    for activation in (0..5).rev() {
        let card_idx = chosen[mask];
        order_indices[activation] = card_idx;
        mask ^= 1 << card_idx;
    }
    (
        dp[(1 << 5) - 1] + deltas[captain_index][5],
        order_indices,
        captain_index,
    )
}

const FIVE_CARD_MASK_POPCOUNT: [u8; 32] = [
    0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4, 1, 2, 2, 3, 2, 3, 3, 4, 2, 3, 3, 4, 3, 4, 4, 5,
];

#[inline]
fn skill_window_representatives(windows: &[ExactSkillWindow; 5]) -> [usize; 5] {
    let mut representatives = [0, 1, 2, 3, 4];
    for card_idx in 1..5 {
        for previous in 0..card_idx {
            if windows[card_idx].range_start == windows[previous].range_start
                && windows[card_idx].range_end == windows[previous].range_end
                && windows[card_idx].score_up.to_bits() == windows[previous].score_up.to_bits()
                && windows[card_idx].rateup == windows[previous].rateup
            {
                representatives[card_idx] = representatives[previous];
                break;
            }
        }
    }
    representatives
}

fn next_permutation(values: &mut [usize]) -> bool {
    let Some(pivot) = (0..values.len().saturating_sub(1))
        .rev()
        .find(|&idx| values[idx] < values[idx + 1])
    else {
        return false;
    };
    let successor = (pivot + 1..values.len())
        .rev()
        .find(|&idx| values[pivot] < values[idx])
        .expect("a permutation pivot has a successor");
    values.swap(pivot, successor);
    values[pivot + 1..].reverse();
    true
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

    fn pop_front_if_after(&mut self, time: f64, scheduler: SkillScheduler) -> Option<SkillEvent> {
        if self.len == 0 || !scheduler.event_is_due(time, self.events[self.head].time) {
            return None;
        }

        let event = self.events[self.head];
        self.head = (self.head + 1) % self.events.len();
        self.len -= 1;
        Some(event)
    }

    fn should_pop_at(&self, time: f64, scheduler: SkillScheduler) -> bool {
        self.len > 0 && scheduler.event_is_due(time, self.events[self.head].time)
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
unsafe fn score_factor_base_scores_avx2(factors: &[f64], stat: i32, output: &mut [i32]) -> i32 {
    debug_assert_eq!(factors.len(), output.len());
    let stat_vec = _mm256_set1_pd(stat as f64);
    let mut sum_vec = _mm256_setzero_pd();
    let mut idx = 0usize;
    while idx + 4 <= factors.len() {
        let factors_vec = _mm256_loadu_pd(factors.as_ptr().add(idx));
        let base = _mm256_floor_pd(_mm256_mul_pd(factors_vec, stat_vec));
        let base_i32 = _mm256_cvttpd_epi32(base);
        _mm_storeu_si128(output.as_mut_ptr().add(idx).cast(), base_i32);
        sum_vec = _mm256_add_pd(sum_vec, base);
        idx += 4;
    }

    let mut lanes = [0.0; 4];
    _mm256_storeu_pd(lanes.as_mut_ptr(), sum_vec);
    let mut sum = lanes.iter().sum::<f64>();
    for (&factor, slot) in factors[idx..].iter().zip(&mut output[idx..]) {
        *slot = (stat as f64 * factor).floor() as i32;
        sum += f64::from(*slot);
    }
    sum as i32
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn five_skill_deltas_from_base_scores_avx2(
    base_scores: &[i32],
    windows: &[ExactSkillWindow; 5],
    rateup_profiles: &[&[f64]; 5],
) -> [i32; 5] {
    let range_start = windows[0].range_start;
    debug_assert!(windows
        .iter()
        .all(|window| window.range_start == range_start));
    let representatives = skill_window_representatives(windows);
    let range_end = windows
        .iter()
        .map(|window| window.range_end)
        .max()
        .unwrap_or(range_start);
    debug_assert!(range_end <= base_scores.len());

    let constant_mods = windows.map(|window| _mm256_set1_pd(1.0 + window.score_up));
    let mut sums = [_mm256_setzero_pd(); 5];
    let mut idx = range_start;
    while idx + 4 <= range_end {
        let base_i32 = _mm_loadu_si128(base_scores.as_ptr().add(idx).cast());
        let base = _mm256_cvtepi32_pd(base_i32);
        let local_idx = idx - range_start;
        for card_idx in 0..5 {
            if representatives[card_idx] != card_idx {
                continue;
            }
            let active = windows[card_idx].range_end.saturating_sub(idx).min(4);
            if active == 0 {
                continue;
            }
            let multiplier = if windows[card_idx].rateup {
                debug_assert!(local_idx + 4 <= rateup_profiles[card_idx].len());
                _mm256_loadu_pd(rateup_profiles[card_idx].as_ptr().add(local_idx))
            } else {
                constant_mods[card_idx]
            };
            let score = _mm256_floor_pd(_mm256_mul_pd(base, multiplier));
            let mut delta = _mm256_sub_pd(score, base);
            if active < 4 {
                let mask = match active {
                    1 => _mm256_set_epi64x(0, 0, 0, -1),
                    2 => _mm256_set_epi64x(0, 0, -1, -1),
                    3 => _mm256_set_epi64x(0, -1, -1, -1),
                    _ => unreachable!(),
                };
                delta = _mm256_and_pd(delta, _mm256_castsi256_pd(mask));
            }
            sums[card_idx] = _mm256_add_pd(sums[card_idx], delta);
        }
        idx += 4;
    }

    let mut result = [0i32; 5];
    for card_idx in 0..5 {
        if representatives[card_idx] != card_idx {
            continue;
        }
        let mut lanes = [0.0; 4];
        _mm256_storeu_pd(lanes.as_mut_ptr(), sums[card_idx]);
        let mut sum = lanes.iter().sum::<f64>();
        for node_idx in idx..windows[card_idx].range_end {
            let base = base_scores[node_idx];
            let multiplier = if windows[card_idx].rateup {
                rateup_profiles[card_idx][node_idx - range_start]
            } else {
                1.0 + windows[card_idx].score_up
            };
            sum += (base as f64 * multiplier).floor() - f64::from(base);
        }
        result[card_idx] = sum as i32;
    }
    for card_idx in 0..5 {
        result[card_idx] = result[representatives[card_idx]];
    }
    result
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn base_score_constant_delta_sum_avx2(base_scores: &[i32], skill_mod: f64) -> i32 {
    let skill_mod_vec = _mm256_set1_pd(skill_mod);
    let mut sum_vec = _mm256_setzero_pd();
    let mut idx = 0usize;
    while idx + 4 <= base_scores.len() {
        let base_i32 = _mm_loadu_si128(base_scores.as_ptr().add(idx).cast());
        let base = _mm256_cvtepi32_pd(base_i32);
        let score = _mm256_floor_pd(_mm256_mul_pd(base, skill_mod_vec));
        sum_vec = _mm256_add_pd(sum_vec, _mm256_sub_pd(score, base));
        idx += 4;
    }

    let mut lanes = [0.0; 4];
    _mm256_storeu_pd(lanes.as_mut_ptr(), sum_vec);
    let mut sum = lanes.iter().sum::<f64>();
    for &base in &base_scores[idx..] {
        sum += (base as f64 * skill_mod).floor() - f64::from(base);
    }
    sum as i32
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn base_score_multiplier_delta_sum_avx2(base_scores: &[i32], multipliers: &[f64]) -> i32 {
    debug_assert_eq!(base_scores.len(), multipliers.len());
    let mut sum_vec = _mm256_setzero_pd();
    let mut idx = 0usize;
    while idx + 4 <= base_scores.len() {
        let base_i32 = _mm_loadu_si128(base_scores.as_ptr().add(idx).cast());
        let base = _mm256_cvtepi32_pd(base_i32);
        let multiplier = _mm256_loadu_pd(multipliers.as_ptr().add(idx));
        let score = _mm256_floor_pd(_mm256_mul_pd(base, multiplier));
        sum_vec = _mm256_add_pd(sum_vec, _mm256_sub_pd(score, base));
        idx += 4;
    }

    let mut lanes = [0.0; 4];
    _mm256_storeu_pd(lanes.as_mut_ptr(), sum_vec);
    let mut sum = lanes.iter().sum::<f64>();
    for (&base, &multiplier) in base_scores[idx..].iter().zip(&multipliers[idx..]) {
        sum += (base as f64 * multiplier).floor() - f64::from(base);
    }
    sum as i32
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

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn score_factor_range_delta_sum_avx2(factors: &[f64], stat: i32, skill_mod: f64) -> i32 {
    let stat_vec = _mm256_set1_pd(stat as f64);
    let skill_mod_vec = _mm256_set1_pd(skill_mod);
    let mut sum_vec = _mm256_setzero_pd();
    let mut idx = 0usize;

    while idx + 4 <= factors.len() {
        let factors_vec = _mm256_loadu_pd(factors.as_ptr().add(idx));
        let no_skill = _mm256_floor_pd(_mm256_mul_pd(factors_vec, stat_vec));
        let score = _mm256_floor_pd(_mm256_mul_pd(no_skill, skill_mod_vec));
        sum_vec = _mm256_add_pd(sum_vec, _mm256_sub_pd(score, no_skill));
        idx += 4;
    }

    let mut lanes = [0.0; 4];
    _mm256_storeu_pd(lanes.as_mut_ptr(), sum_vec);
    let mut sum = lanes.iter().sum::<f64>();
    for &factor in &factors[idx..] {
        let no_skill = (stat as f64 * factor).floor();
        sum += (no_skill * skill_mod).floor() - no_skill;
    }
    sum as i32
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn compiled_six_skill_score_avx2(factors: &[f64], multipliers: &[f64], stat: i32) -> i32 {
    debug_assert_eq!(factors.len(), multipliers.len());
    let stat_vec = _mm256_set1_pd(stat as f64);
    let mut sum_vec = _mm256_setzero_pd();
    let mut idx = 0usize;
    while idx + 4 <= factors.len() {
        let factor_vec = _mm256_loadu_pd(factors.as_ptr().add(idx));
        let multiplier_vec = _mm256_loadu_pd(multipliers.as_ptr().add(idx));
        let no_skill = _mm256_floor_pd(_mm256_mul_pd(factor_vec, stat_vec));
        let score = _mm256_floor_pd(_mm256_mul_pd(no_skill, multiplier_vec));
        sum_vec = _mm256_add_pd(sum_vec, score);
        idx += 4;
    }
    let mut lanes = [0.0; 4];
    _mm256_storeu_pd(lanes.as_mut_ptr(), sum_vec);
    let mut sum = lanes.iter().sum::<f64>();
    for idx in idx..factors.len() {
        let no_skill = (stat as f64 * factors[idx]).floor();
        sum += (no_skill * multipliers[idx]).floor();
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
    fn rateup_profile_avx2_matches_scalar_through_cap() {
        if !avx2_available() {
            return;
        }

        let mut profile = RateUpProfileScratch::default();
        profile.prepare(1.0, 160);
        let base_scores = (0..160).map(|idx| 10_000 + idx * 37).collect::<Vec<_>>();
        let scalar = base_scores
            .iter()
            .zip(&profile.multipliers)
            .map(|(&base, &multiplier)| (base as f64 * multiplier).floor() as i32 - base)
            .sum::<i32>();

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let avx2 =
            unsafe { base_score_multiplier_delta_sum_avx2(&base_scores, &profile.multipliers) };

        assert_eq!(profile.multipliers[0], 2.005);
        assert_eq!(
            profile.multipliers[159],
            *profile.multipliers.last().unwrap()
        );
        assert_eq!(avx2, scalar);
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
    fn standard_rule_allows_simultaneous_note_to_use_triggered_skill() {
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
            ],
        );
        chart.init(0, false).unwrap();

        assert_eq!(chart.nodes[0].node_type, ChartNodeType::Skill);
        assert_eq!(
            chart.get_score(&[skill(1, 1.0)], 1000, false).unwrap(),
            4950
        );
    }

    #[test]
    fn skill_timing_intervals_include_end_and_finishing_transition_frames() {
        let end = skill_end_interval(5.0);
        assert!((end.lower - (5.0 + 1.0 / 120.0)).abs() < f64::EPSILON);
        assert!((end.upper - (5.0 + 1.0 / 30.0)).abs() < f64::EPSILON);

        let queue = skill_queue_gap_interval(5.0);
        assert!((queue.lower - (5.0 + 0.75 + 2.0 / 120.0)).abs() < f64::EPSILON);
        assert!((queue.upper - (5.0 + 0.75 + 3.0 / 60.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn envelope_scheduler_detects_queue_after_active_skill_has_ended() {
        let mut scheduler = EnvelopeSkillScheduler::default();
        let first = scheduler.schedule(0.0, 1.0);
        assert!(first.end < 1.5);

        let second = scheduler.schedule(1.5, 1.0);
        assert!(second.queue_risk);
        assert!(second.start > 1.5);
        assert!((second.start - (1.0 + 1.0 / 30.0 + 0.75)).abs() < f64::EPSILON);
    }

    #[test]
    fn envelope_timing_dependent_queue_uses_nonqueued_branch() {
        let duration = 1.0;
        let gap = skill_queue_gap_interval(duration);
        let trigger = (gap.lower + gap.upper) / 2.0;
        let mut scheduler = EnvelopeSkillScheduler::default();
        scheduler.schedule(0.0, duration);

        let second = scheduler.schedule(trigger, duration);
        assert!(second.queue_risk);
        assert_eq!(second.start, trigger);
    }

    #[test]
    fn ideal_60fps_timing_uses_phase_zero_integer_frames() {
        let duration = 1.0;
        assert_eq!(ideal_judgement_frame(0.0), 0);
        assert_eq!(ideal_judgement_frame(0.001), 1);
        assert_eq!(ideal_skill_end_time(0.0, duration), 61.0 / 60.0);
        assert_eq!(ideal_skill_end_time(0.001, duration), 62.0 / 60.0);

        assert!(ideal_skills_queue(0.0, duration, 107.0 / 60.0));
        assert!(!ideal_skills_queue(0.0, duration, 108.0 / 60.0));
    }

    #[test]
    fn ideal_60fps_queued_skill_starts_after_finishing_transitions() {
        let mut scheduler = Ideal60SkillScheduler::default();
        let first = scheduler.schedule(0.0, 1.0);
        assert_eq!(first.end, 61.0 / 60.0);

        let queued = scheduler.schedule(107.0 / 60.0, 1.0);
        assert!(queued.queue_risk);
        assert_eq!(queued.start, 108.0 / 60.0);
        assert_eq!(queued.end, 169.0 / 60.0);
    }

    #[test]
    fn standard_exact_window_uses_phase_zero_end_frame() {
        let duration = 3.0;
        let trigger = 0.001;
        let deadline = 182.0 / 60.0;
        let mut chart = Chart::new(
            20,
            vec![
                ChartNode {
                    time: trigger,
                    node_type: ChartNodeType::Skill,
                },
                ChartNode {
                    time: deadline,
                    node_type: ChartNodeType::Node,
                },
                ChartNode {
                    time: deadline + 0.0001,
                    node_type: ChartNodeType::Node,
                },
            ],
        );
        chart.init(0, false).unwrap();
        let mut exact_skill = skill(1, 1.0);
        exact_skill.duration = duration;

        let window = chart.compile_exact_skill_window(0, exact_skill).unwrap();
        assert_eq!(window.range_start, 1);
        assert_eq!(window.range_end, 2);
    }

    #[test]
    fn standard_queue_detection_uses_exact_ideal_frames() {
        let make_chart = |second_frame: i64| {
            let mut nodes = vec![ChartNode {
                time: 0.0,
                node_type: ChartNodeType::Skill,
            }];
            nodes.push(ChartNode {
                time: ideal_frame_time(second_frame),
                node_type: ChartNodeType::Skill,
            });
            for activation in 2..6 {
                nodes.push(ChartNode {
                    time: activation as f64 * 20.0,
                    node_type: ChartNodeType::Skill,
                });
            }
            let mut chart = Chart::new(20, nodes);
            chart.init(0, true).unwrap();
            chart
        };
        let mut exact_skill = skill(1, 1.0);
        exact_skill.duration = 3.0;
        let team = [exact_skill; 5];

        assert!(make_chart(227).team_skills_may_overlap(&team).unwrap());
        assert!(!make_chart(228).team_skills_may_overlap(&team).unwrap());
    }

    #[test]
    fn queue_risk_uses_closed_conservative_gap_interval() {
        let duration = 1.0;
        let upper = skill_queue_gap_interval(duration).upper;
        let make_chart = |second_time| {
            let mut nodes = (0..6)
                .map(|activation| ChartNode {
                    time: if activation == 0 {
                        0.0
                    } else if activation == 1 {
                        second_time
                    } else {
                        20.0 * activation as f64
                    },
                    node_type: ChartNodeType::Skill,
                })
                .collect::<Vec<_>>();
            nodes.sort_by(|left, right| left.time.total_cmp(&right.time));
            Chart::new(20, nodes)
        };

        assert!(make_chart(upper).has_skill_queue_risk(duration).unwrap());
        assert!(!make_chart(upper + 2.0 * EPS)
            .has_skill_queue_risk(duration)
            .unwrap());
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
        let compiled = chart.compile_six_skill_score(&skills, true).unwrap();
        for stat in [1, 12_345, 500_000] {
            assert_eq!(
                chart.score_compiled_six_skills(&compiled, stat),
                chart.get_score_for_six_skills(&skills, stat, true).unwrap()
            );
        }
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
    fn queued_skill_timeline_drains_all_events_before_sparse_node() {
        let mut nodes = (0..6)
            .map(|activation| ChartNode {
                time: activation as f64 * 0.1,
                node_type: ChartNodeType::Skill,
            })
            .collect::<Vec<_>>();
        nodes.push(ChartNode {
            time: 10.0,
            node_type: ChartNodeType::Node,
        });
        let mut chart = Chart::new(25, nodes);
        chart.init_auto_with_base_multiplier(0.75).unwrap();

        let skills = [skill(1, 1.0); 6];
        let compiled = chart.compile_six_skill_score(&skills, false).unwrap();
        // Before t=10, skill 1 has ended, skill 2 has started and ended, and
        // skill 3 has started. The sparse node must therefore use skill 3.
        assert_eq!(compiled.multipliers.last().copied(), Some(2.0));

        let windows = chart
            .score_range_skill_window_counts(skills[0].duration)
            .unwrap()
            .unwrap();
        assert_eq!(windows.active_nodes[2], 1);
        assert!(windows.skill_queue_risk);
        assert!(chart.has_skill_queue_risk(skills[0].duration).unwrap());

        for stat in [1, 12_345, 543_210] {
            let expected = chart.get_score(&skills, stat, false).unwrap();
            assert_eq!(chart.score_compiled_six_skills(&compiled, stat), expected);
            assert_eq!(
                chart
                    .get_score_for_six_skills_scalar(&skills, stat, false)
                    .unwrap(),
                expected
            );
            assert_eq!(
                chart
                    .get_score_for_six_skills(&skills, stat, false)
                    .unwrap(),
                expected
            );
            assert_eq!(
                chart.compressed_auto_score(skills[0]).unwrap().score(stat),
                expected
            );
        }
    }

    #[test]
    fn score_range_counts_use_ideal_60fps_while_retaining_envelope_risk() {
        let duration = 1.0;
        let mut nodes = (0..6)
            .map(|activation| ChartNode {
                time: activation as f64 * 20.0,
                node_type: ChartNodeType::Skill,
            })
            .collect::<Vec<_>>();
        // This note is inside the old score-maximising envelope end (1 + 1/30),
        // but after the ideal phase-zero end frame (61/60).
        nodes.push(ChartNode {
            time: duration + 1.0 / 40.0,
            node_type: ChartNodeType::Node,
        });
        let mut chart = Chart::new(20, nodes);
        chart.init_auto().unwrap();

        let counts = chart
            .score_range_skill_window_counts(duration)
            .unwrap()
            .unwrap();
        assert_eq!(counts.active_nodes, [0; 6]);
        assert_eq!(counts.inactive_nodes, 7);
        assert!(counts.tail_risk);
        assert!(!counts.skill_queue_risk);
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
