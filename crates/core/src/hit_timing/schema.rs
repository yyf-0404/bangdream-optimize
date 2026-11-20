use crate::scoring::Judgement;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayInput {
    /// Already prepared, unrounded scoring power, never a UI display integer.
    pub power: f32,
    pub play_level: i32,
    /// Must be REPLAY_FPS (120); frames from other sampling rates are rejected.
    pub fps: u32,
    pub notes: Vec<ReplayNote>,
    pub hit_frames: Vec<u32>,
    /// Explicit witness order; only ties in hit_frames use this permutation.
    /// Empty preserves legacy routing.eventOrder, then note-index order.
    #[serde(default)]
    pub hit_order: Vec<usize>,
    pub skills: Vec<ReplaySkill>,
    #[serde(default)]
    pub initial_combo: u32,
    /// Use the extended Medley Combo table; `initial_combo` carries prior songs.
    #[serde(default)]
    pub medley_combo: bool,
    #[serde(default)]
    pub fever_frames: Option<[u32; 2]>,
    #[serde(default)]
    pub constraints: Vec<NoteConstraint>,
    #[serde(default)]
    pub native_time_routing: Option<super::NativeTimeRouting>,
}

impl ReplayInput {
    pub(crate) fn combo_multiplier(&self, combo: u32) -> f32 {
        if self.medley_combo {
            crate::model::chart::get_combo_mod(combo.min(i32::MAX as u32) as i32, true) as f32
        } else {
            crate::scoring::normal_combo_multiplier(combo)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayNote {
    pub time: f64,
    #[serde(default)]
    pub trigger: Option<usize>,
    pub window: NoteWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NoteWindow {
    Standard,
    /// CN node-specific gates projected into nominal chart time (B=0).
    /// Unlike NativeGreen, this does not claim a measured spatial trajectory.
    SlideTime {
        role: SlideTimeRole,
        /// Chart adapters supply the BPM/clock-derived final input frame.
        /// None is the constant nominal-time projection for isolated research.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_frame: Option<u32>,
    },
    /// Resolved native green-node position/state domain, not an ordinary time
    /// window. Starts are inclusive for Bad, Good, Great, Perfect respectively.
    /// Coincident starts represent absent grades (e.g. held middle nodes are P).
    /// The adapter must supply native lifetime and chain dependencies separately.
    NativeGreen {
        first_frames: [u32; 4],
        last_frame: u32,
    },
    /// Caller must verify native endpoint type; never infer it from geometry.
    WidenedEndpoint,
    /// Explicit assumption for an unimplemented node. This is not a judgment window.
    PinnedPerfect {
        frame: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlideTimeRole {
    Head,
    Middle,
    End,
    FlickEnd,
    DirectionalEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteConstraint {
    pub before: usize,
    pub after: usize,
    #[serde(default)]
    pub minimum_gap_frames: u32,
    /// A causal edge also orders successful dispatches within the same frame.
    /// False preserves legacy constraints that compare timestamps only.
    #[serde(default)]
    pub require_dispatch_order: bool,
    /// Preprocessed successor reachability F(before_frame). Outside this table,
    /// or at an unreachable entry, the pair is illegal. Hidden nodes themselves
    /// never become scoring nodes or search states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earliest_after: Option<ArrivalCurve>,
}

/// Consecutive constant or slope-one runs. Bounds are inclusive; the next run
/// starts at the preceding through_frame + 1. Consumers assume no monotonicity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArrivalCurve {
    pub first_before_frame: u32,
    pub runs: Vec<ArrivalRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArrivalRun {
    pub through_frame: u32,
    pub first_after_frame: Option<u32>,
    pub follows_before: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaySkill {
    pub duration: f32,
    pub score: ScoreEffect,
    #[serde(default)]
    pub correction: Option<JudgementCorrection>,
}

/// Resolved effect parameters, after server/level/team-condition selection.
/// These variants cannot be reconstructed from the old AP-only scalar bonus.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ScoreEffect {
    Plain { percent: f32 },
    PerfectOnly { percent: f32 },
    PerfectOrZero { percent: f32 },
    PerfectOrHalf { percent: f32 },
    Continuous { high: f32, low: f32 },
    Crescendo { percent: f32, step: f32, cap: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JudgementCorrection {
    Great,
    Good,
    Bad,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayResult {
    pub scoring_revision: String,
    pub timing_revision: String,
    pub routing_coverage: super::RoutingCoverage,
    pub fps: u32,
    pub scoring_power: f32,
    pub total: u64,
    pub trace: Vec<HitTrace>,
    pub skill_events: Vec<SkillEvent>,
    pub hit_frames: Vec<u32>,
    pub hit_order: Vec<usize>,
    pub pinned_note_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitTrace {
    pub note_index: usize,
    pub frame: u32,
    pub hit_time: f64,
    pub offset_ms: f64,
    pub raw: Judgement,
    pub judgement: Judgement,
    pub combo: u32,
    pub pinned_perfect: bool,
    pub active_skill: Option<usize>,
    pub skill_multiplier: f32,
    pub skill_state: Option<SkillStateSnapshot>,
    pub fever_multiplier: f32,
    pub before_skill: u32,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillStateSnapshot {
    pub worst_judgement: Judgement,
    pub extra_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SkillEvent {
    Request {
        index: usize,
        frame: u32,
    },
    Start {
        index: usize,
        after_frame: u32,
        first_input_frame: u32,
    },
    End {
        index: usize,
        after_frame: u32,
        last_input_frame: u32,
    },
}
