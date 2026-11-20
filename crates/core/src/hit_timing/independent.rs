//! Complete single-window optimization in the declared independent-skill model.
//!
//! Every note settles exactly once, including outside hits and the trigger.
//! Geometry is power/effect independent; the same graph supports continuous
//! coefficients and native integer scoring. These whole-chart maxima must NOT
//! be added across skill slots or subtracted as a member-replacement proof.
//! `FixedTimeline` instead uses the shared graph for all actual queued windows
//! at once, optimizing ordinary notes at a supplied native scoring power.
mod fixed;
mod geometry;
mod joint;
mod machine;
mod controls;
mod net;
mod solve;
pub use fixed::{FixedTimeline, FixedTimelineGeometry, FixedTimelineResult};
pub use joint::{JointTimeline, JointTimelineResult};
pub use joint::JointPreparationStats;
pub use joint::FunctionStats;
pub(crate) use joint::JointPreparation;
pub(crate) use net::FrontierNetWindows;

use super::{replay, ReplayError, ReplayInput, ReplaySkill};
use crate::scoring::ScoringPower;
use geometry::{Graph, Prepared};
use machine::{Costs, Machine, Value};
use controls::{Controls, Switch};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

pub const WINDOW_REVISION: &str = "independent-window-touch-order-120-v3";

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowObjective {
    #[default]
    Continuous,
    Native,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowOptions {
    pub max_states: usize,
    pub max_graph_edges: usize,
    pub max_transitions: u64,
    pub max_cached_edges: usize,
    /// Retained for exhaustive compression/oracle comparisons.
    pub collapse_perfect_runs: bool,
    pub prune_request_frames: bool,
    /// Opt-in value-only net queries with calculation-scoped skill/prefix reuse.
    /// False retains the original path for numerical and performance audits.
    pub fast_net_queries: bool,
    /// Share equal event actions and avoid cloning already-interned states.
    /// Kept switchable for identical-edge numerical and performance audits.
    pub reuse_graph_storage: bool,
    /// Ordered sparse arrays with reusable layer buffers; identical state order.
    pub compact_value_states: bool,
    /// Store a single event hit directly, avoiding a per-action allocation.
    pub inline_event_hits: bool,
    /// Calculation-scoped exact component templates; zero disables reuse.
    pub max_cached_component_edges: usize,
    /// Optimize within-frame touch order; false is only a fixed-order audit.
    pub optimize_touch_order: bool,
    /// Expand each within-frame geometry vertex once in consumed-count order.
    /// False retains padded decision layers for exhaustive audits.
    pub touch_dag: bool,
    /// Skip previously closed ordinary edges whose exact input did not improve.
    pub incremental_touch: bool,
    /// Reuse only fully closed ordinary frame DAGs within one graph build.
    pub frame_templates: bool,
}
impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            max_states: 32_768,
            max_graph_edges: 2_000_000,
            max_transitions: 4_000_000,
            max_cached_edges: 1_000_000,
            collapse_perfect_runs: true,
            prune_request_frames: true,
            fast_net_queries: false,
            reuse_graph_storage: true,
            compact_value_states: true,
            inline_event_hits: true,
            max_cached_component_edges: 128_000,
            optimize_touch_order: true,
            touch_dag: false,
            incremental_touch: false,
            frame_templates: true,
        }
    }
}

#[derive(Clone, Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeBlockStats {
    pub value_arena_queries: u64,
    pub value_arena_budget_fallbacks: u64,
    pub carried_entries: u64,
    pub fused_exit_layers: u64,
    pub clock_aliases: usize,
    pub clock_queues: usize,
    pub clock_states: usize,
    pub shared_propagations: u64,
    pub shape_hits: u64,
    pub shape_misses: u64,
    pub operator_hits: u64,
    pub operator_misses: u64,
    pub evaluated_entries: u64,
    pub equal_shape_merges: u64,
    pub scanned_merge_entries: u64,
    /// Writes avoided when the source cannot improve an immutable target set.
    pub dominated_merge_entries_saved: u64,
    pub normalized_entries: u64,
    pub reduction_hits: u64,
    pub reduction_builds: u64,
    pub reduction_entries: u64,
    pub uniform_operators: u64,
    pub uniform_entries_saved: u64,
    pub retained_bytes: usize,
}

#[derive(Clone, Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_blocks: Option<NativeBlockStats>,
    pub prepared_frames: usize,
    pub prepared_intervals: usize,
    pub request_frames: usize,
    pub skipped_requests: usize,
    pub infeasible_requests: usize,
    pub budget_requests: usize,
    pub graph_builds: usize,
    pub graph_hits: usize,
    pub graph_edges: usize,
    pub graph_actions: usize,
    pub graph_action_reuses: usize,
    pub graph_key_reuses: usize,
    pub graph_reuse_builds: usize,
    pub inline_event_builds: usize,
    pub component_cache_enabled_builds: usize,
    pub component_cache_queries: usize,
    pub component_cache_hits: usize,
    pub component_cache_stores: usize,
    pub component_cache_skips: usize,
    pub component_replayed_edges: usize,
    pub component_cached_edges: usize,
    pub graph_peak_states: usize,
    pub graph_touch_dag_frames: usize,
    pub graph_touch_dag_edges: usize,
    pub graph_incremental_frames: usize,
    pub graph_incremental_edges: usize,
    pub collapsed_notes: usize,
    /// Proven fixed Perfect hits extracted from components containing skill notes.
    pub outside_pinned_notes: usize,
    pub temporal_intervals: usize,
    pub temporal_candidate_frames: usize,
    pub temporal_geometry_rejects: usize,
    pub temporal_skipped_frames: usize,
    pub temporal_skipped_layers: usize,
    pub temporal_skipped_intervals: usize,
    pub temporal_clock_fallbacks: usize,
    pub temporal_stability_probes: u64,
    pub temporal_carried_value_states: u64,
    pub temporal_partial_intervals: usize,
    pub temporal_parked_value_states: u64,
    pub temporal_partial_peak_states: usize,
    pub value_graph_layers: usize,
    pub value_graph_edges: usize,
    pub touch_dag_layers: usize,
    pub touch_entry_transitions: u64,
    pub touch_hit_transitions: u64,
    pub incremental_candidate_transitions: u64,
    pub incremental_skipped_transitions: u64,
    pub incremental_changed_transitions: u64,
    pub incremental_clock_fallback_transitions: u64,
    pub incremental_new_transitions: u64,
    pub incremental_stability_probes: u64,
    pub joint_frame_operator_hits: u64,
    pub joint_frame_operator_misses: u64,
    pub joint_frame_operator_bytes: usize,
    pub transitions: u64,
    pub peak_value_states: usize,
    pub dominated_value_states: u64,
    pub compact_value_queries: usize,
    pub prefix_tables: usize,
    pub net_machine_builds: usize,
    pub net_machine_hits: usize,
    pub net_setup_seconds: f64,
    pub net_row_seconds: f64,
    pub net_graph_seconds: f64,
    pub net_value_seconds: f64,
    pub net_envelope_seconds: f64,
}
impl WindowStats {
    pub(crate) fn accumulate(&mut self, other: &Self) {
        if let Some(b) = &other.native_blocks {
            let a = self.native_blocks.get_or_insert_with(NativeBlockStats::default);
            a.value_arena_queries += b.value_arena_queries;
            a.value_arena_budget_fallbacks += b.value_arena_budget_fallbacks;
            a.carried_entries += b.carried_entries;
            a.fused_exit_layers += b.fused_exit_layers;
            a.clock_aliases += b.clock_aliases;
            a.clock_queues += b.clock_queues;
            a.clock_states += b.clock_states;
            a.shared_propagations += b.shared_propagations;
            a.shape_hits += b.shape_hits;
            a.shape_misses += b.shape_misses;
            a.operator_hits += b.operator_hits;
            a.operator_misses += b.operator_misses;
            a.evaluated_entries += b.evaluated_entries;
            a.equal_shape_merges += b.equal_shape_merges;
            a.scanned_merge_entries += b.scanned_merge_entries;
            a.normalized_entries += b.normalized_entries;
            a.reduction_hits += b.reduction_hits;
            a.reduction_builds += b.reduction_builds;
            a.reduction_entries += b.reduction_entries;
            a.uniform_operators += b.uniform_operators;
            a.uniform_entries_saved += b.uniform_entries_saved;
            a.retained_bytes = a.retained_bytes.max(b.retained_bytes);
        }
        self.prepared_frames = self.prepared_frames.max(other.prepared_frames);
        self.prepared_intervals = self.prepared_intervals.max(other.prepared_intervals);
        self.request_frames += other.request_frames;
        self.skipped_requests += other.skipped_requests;
        self.infeasible_requests += other.infeasible_requests;
        self.budget_requests += other.budget_requests;
        self.graph_builds += other.graph_builds;
        self.graph_hits += other.graph_hits;
        self.graph_edges += other.graph_edges;
        self.graph_actions += other.graph_actions;
        self.graph_action_reuses += other.graph_action_reuses;
        self.graph_key_reuses += other.graph_key_reuses;
        self.graph_reuse_builds += other.graph_reuse_builds;
        self.inline_event_builds += other.inline_event_builds;
        self.component_cache_enabled_builds += other.component_cache_enabled_builds;
        self.component_cache_queries += other.component_cache_queries;
        self.component_cache_hits += other.component_cache_hits;
        self.component_cache_stores += other.component_cache_stores;
        self.component_cache_skips += other.component_cache_skips;
        self.component_replayed_edges += other.component_replayed_edges;
        self.component_cached_edges = self
            .component_cached_edges
            .max(other.component_cached_edges);
        self.graph_peak_states = self.graph_peak_states.max(other.graph_peak_states);
        self.graph_touch_dag_frames += other.graph_touch_dag_frames;
        self.graph_touch_dag_edges += other.graph_touch_dag_edges;
        self.graph_incremental_frames += other.graph_incremental_frames;
        self.graph_incremental_edges += other.graph_incremental_edges;
        self.collapsed_notes += other.collapsed_notes;
        self.outside_pinned_notes += other.outside_pinned_notes;
        self.temporal_intervals += other.temporal_intervals;
        self.temporal_candidate_frames += other.temporal_candidate_frames;
        self.temporal_geometry_rejects += other.temporal_geometry_rejects;
        self.temporal_skipped_frames += other.temporal_skipped_frames;
        self.temporal_skipped_layers += other.temporal_skipped_layers;
        self.temporal_skipped_intervals += other.temporal_skipped_intervals;
        self.temporal_clock_fallbacks += other.temporal_clock_fallbacks;
        self.temporal_stability_probes += other.temporal_stability_probes;
        self.temporal_carried_value_states += other.temporal_carried_value_states;
        self.temporal_partial_intervals += other.temporal_partial_intervals;
        self.temporal_parked_value_states += other.temporal_parked_value_states;
        self.temporal_partial_peak_states = self.temporal_partial_peak_states.max(other.temporal_partial_peak_states);
        self.value_graph_layers += other.value_graph_layers;
        self.value_graph_edges += other.value_graph_edges;
        self.touch_dag_layers += other.touch_dag_layers;
        self.touch_entry_transitions += other.touch_entry_transitions;
        self.touch_hit_transitions += other.touch_hit_transitions;
        self.incremental_candidate_transitions += other.incremental_candidate_transitions;
        self.incremental_skipped_transitions += other.incremental_skipped_transitions;
        self.incremental_changed_transitions += other.incremental_changed_transitions;
        self.incremental_clock_fallback_transitions += other.incremental_clock_fallback_transitions;
        self.incremental_new_transitions += other.incremental_new_transitions;
        self.incremental_stability_probes += other.incremental_stability_probes;
        self.joint_frame_operator_hits += other.joint_frame_operator_hits;
        self.joint_frame_operator_misses += other.joint_frame_operator_misses;
        self.joint_frame_operator_bytes = self.joint_frame_operator_bytes.max(other.joint_frame_operator_bytes);
        self.transitions += other.transitions;
        self.peak_value_states = self.peak_value_states.max(other.peak_value_states);
        self.dominated_value_states += other.dominated_value_states;
        self.compact_value_queries += other.compact_value_queries;
        self.prefix_tables += other.prefix_tables;
        self.net_machine_builds += other.net_machine_builds;
        self.net_machine_hits += other.net_machine_hits;
        self.net_setup_seconds += other.net_setup_seconds;
        self.net_row_seconds += other.net_row_seconds;
        self.net_graph_seconds += other.net_graph_seconds;
        self.net_value_seconds += other.net_value_seconds;
        self.net_envelope_seconds += other.net_envelope_seconds;
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowWitness {
    pub request_frame: u32,
    pub last_skill_frame: u32,
    pub continuous_coefficient: f64,
    pub native_score: u64,
    pub hit_frames: Vec<u32>,
    pub hit_order: Vec<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowMaximum {
    pub revision: &'static str,
    pub objective: WindowObjective,
    /// False means an explicit resource limit left unresolved requests.
    pub complete: bool,
    /// A feasible lower bound until `complete` is true. None means none found.
    pub best: Option<WindowWitness>,
    /// The caller's fixed baseline, replayed with every skill disabled. None if
    /// that baseline requires correction; no fictitious outside score is used.
    pub reference_coefficient: Option<f64>,
    /// Signed whole-plan difference; not an additive per-slot pruning bound.
    pub witness_net_coefficient: Option<f64>,
    /// Same objective/units as requested. None means the entire domain is infeasible.
    pub upper_bound: Option<f64>,
    /// Exact integer representation; avoids f64 serialization losing large totals.
    pub native_upper_bound: Option<u64>,
    pub stats: WindowStats,
}

/// One calculation-scoped cache per chart/Combo/Fever/routing configuration.
/// Different powers and skill magnitudes share graphs, never score winners.
pub struct IndependentWindows {
    prepared: Prepared,
    options: WindowOptions,
    graphs: HashMap<(usize, u32, u32, bool, u32), Arc<Graph>>,
    cached_edges: usize,
}
impl IndependentWindows {
    pub fn new(input: &ReplayInput, options: WindowOptions) -> Result<Self, ReplayError> {
        replay(input)?;
        if options.max_states == 0 || options.max_graph_edges == 0 || options.max_transitions == 0 {
            return Err(ReplayError::Input("window budgets must be positive"));
        }
        Ok(Self {
            prepared: Prepared::new(input)?,
            options,
            graphs: HashMap::new(),
            cached_edges: 0,
        })
    }

    /// `slot` selects the original trigger note. Only this one skill is active;
    /// the original order of the other trigger notes remains a legality constraint.
    pub fn maximum(
        &mut self,
        slot: usize,
        skill: &ReplaySkill,
        objective: WindowObjective,
        power: ScoringPower,
    ) -> Result<WindowMaximum, ReplayError> {
        skill.validate(slot)?;
        let trigger = self
            .prepared
            .input
            .notes
            .iter()
            .position(|n| n.trigger == Some(slot))
            .ok_or(ReplayError::Input(
                "independent window requires an existing trigger",
            ))?;
        let playing = super::bounds::countdown(skill.duration) + 1;
        let correction = skill.cache_key()[1];
        let machine = Machine::new(skill, self.prepared.input.notes.len())?;
        let collapse = self.options.collapse_perfect_runs && machine.monotone;
        let mut costs = Costs::new(&self.prepared.input, power, &machine)?;
        let mut stats = WindowStats {
            prepared_frames: self.prepared.frame_count,
            prepared_intervals: self.prepared.interval_count,
            ..WindowStats::default()
        };
        let requests = self.prepared.requests(trigger);
        stats.request_frames = requests.len();
        // Each row only contains compressed action intervals/envelope terms.
        // Detailed frontier graphs are materialized after the upper-bound test.
        let mut rows = Vec::new();
        for request in requests {
            match self.prepared.row(trigger, request, playing, correction) {
                Some(row) => {
                    let upper = costs.upper(&row.envelope)?;
                    rows.push((request, row, upper));
                }
                None => {
                    stats.infeasible_requests += 1;
                }
            }
        }
        rows.sort_by(|a, b| costs.compare(b.2, a.2, objective).then(a.0.cmp(&b.0)));
        // A real complete plan is an incumbent; a relaxed table value is not.
        let mut seed = self.prepared.isolated(trigger, skill);
        seed.power = power.value();
        let mut best = replay(&seed).ok().map(|r| {
            let value = costs.trace_value(&r);
            let request_frame = seed.hit_frames[trigger];
            (
                value,
                WindowWitness {
                    request_frame,
                    last_skill_frame: request_frame + playing,
                    continuous_coefficient: value.coefficient,
                    native_score: value.score,
                    hit_frames: r.hit_frames,
                    hit_order: r.hit_order,
                },
            )
        });
        let mut unresolved: Option<Value> = None;
        for (request, row, upper) in rows {
            if self.options.prune_request_frames
                && best
                    .as_ref()
                    .is_some_and(|(v, _)| costs.compare(upper, *v, objective).is_le())
            {
                stats.skipped_requests += 1;
                continue;
            }
            let key = (slot, playing, correction, collapse, request);
            let graph = if let Some(graph) = self.graphs.get(&key) {
                stats.graph_hits += 1;
                Some(graph.clone())
            } else {
                stats.graph_builds += 1;
                self.prepared
                    .compile(&row, collapse, self.options, &mut stats)
                    .map(|g| {
                        let graph = Arc::new(g);
                        if self.cached_edges + graph.edge_count <= self.options.max_cached_edges {
                            self.cached_edges += graph.edge_count;
                            self.graphs.insert(key, graph.clone());
                        }
                        graph
                    })
            };
            let answer = match graph.as_ref() {
                Some(graph) => {
                    solve::evaluate_ordered(graph, &mut costs, objective, self.options, &mut stats)?
                }
                None => None,
            };
            match answer {
                None => {
                    stats.budget_requests += 1;
                    if unresolved.is_none_or(|v| costs.compare(upper, v, objective).is_gt()) {
                        unresolved = Some(upper);
                    }
                }
                Some(None) => stats.infeasible_requests += 1,
                Some(Some((value, hits))) => {
                    if best
                        .as_ref()
                        .is_none_or(|(v, _)| costs.compare(value, *v, objective).is_gt())
                    {
                        best = Some((
                            value,
                            WindowWitness {
                                request_frame: request,
                                last_skill_frame: request + playing,
                                continuous_coefficient: value.coefficient,
                                native_score: value.score,
                                hit_frames: hits.frames,
                                hit_order: hits.order,
                            },
                        ));
                    }
                }
            }
        }
        // A later feasible winner may also close an earlier budget-limited row.
        let complete = unresolved.is_none_or(|upper| {
            best.as_ref()
                .is_some_and(|(v, _)| costs.compare(upper, *v, objective).is_le())
        });
        let upper = match (best.as_ref().map(|(v, _)| *v), unresolved) {
            (Some(a), Some(b)) => Some(if costs.compare(a, b, objective).is_ge() {
                a
            } else {
                b
            }),
            (a, b) => a.or(b),
        };
        if let Some((_, witness)) = &best {
            let mut audit = self.prepared.isolated(trigger, skill);
            audit.power = power.value();
            audit.hit_frames = witness.hit_frames.clone();
            audit.hit_order = witness.hit_order.clone();
            if replay(&audit)?.total != witness.native_score {
                return Err(ReplayError::Input(
                    "independent-window winner differs from native replay",
                ));
            }
        }
        stats.prefix_tables = costs.prefix_count();
        Ok(WindowMaximum {
            revision: WINDOW_REVISION,
            objective,
            complete,
            reference_coefficient: self.prepared.reference_coefficient,
            witness_net_coefficient: best.as_ref().and_then(|(v, _)| {
                self.prepared
                    .reference_coefficient
                    .map(|base| v.coefficient - base)
            }),
            upper_bound: upper.map(|v| match objective {
                WindowObjective::Continuous => v.coefficient,
                WindowObjective::Native => v.score as f64,
            }),
            native_upper_bound: (objective == WindowObjective::Native)
                .then(|| upper.map(|v| v.score))
                .flatten(),
            best: best.map(|(_, w)| w),
            stats,
        })
    }
}

#[cfg(test)]
mod tests;
