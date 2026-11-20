//! Joint trigger and ordinary-note dynamic programming for the declared replay
//! model. Geometry tracks input legality; value state tracks the FIFO clock and
//! current skill history. No candidate-family, local-search or work-limit fallback.
use super::*;
#[cfg(test)]
use crate::hit_timing::NoteWindow;
use crate::hit_timing::{
    bounds::countdown, search::SearchError, JudgementCorrection, ReplayResult,
};
use crate::scoring::{Judgement, NoteScorer};
use geometry::{Action, Factor};
use machine::ActionCosts;
use std::rc::Rc;

mod prepared;
mod outside_scores;
mod clock;
mod operators;
mod local;
mod preparation;
mod functions;
mod circuit;
mod buckets;
pub(super) mod quotient;
pub use functions::FunctionStats;
pub(crate) use preparation::JointPreparation;
pub use preparation::JointPreparationStats;
use prepared::{PreparedAction, PreparedActions, ScoreTable};

pub struct JointTimeline {
    input: ReplayInput,
    graph: Arc<Graph>,
    prepared_actions: Arc<PreparedActions>,
    value_layout: Option<Arc<quotient::Layout>>,
    packed_layout: Option<Arc<solve::blocks::packed::Layout>>,
    machines: Vec<Machine>,
    playing: Vec<u32>,
    stats: WindowStats,
    geometry_key: Vec<(u8, u32, u32)>,
    local_geometry: bool,
    geometry_bytes: usize,
    functions: Arc<std::sync::Mutex<functions::Cache>>,
    function_plan: Option<functions::Plan>,
    controls: Controls,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JointTimelineResult {
    pub result: ReplayResult,
    pub stats: WindowStats,
}

fn options() -> WindowOptions {
    options_with(Controls::dynamic())
}
fn options_with(controls: Controls) -> WindowOptions {
    WindowOptions {
        max_states: usize::MAX,
        max_graph_edges: usize::MAX,
        max_transitions: u64::MAX,
        max_cached_component_edges: 0,
        touch_dag: controls.enabled(Switch::TouchDag),
        // Retained as an exact experiment; measured default uses frame operators.
        incremental_touch: controls.enabled(Switch::IncrementalTouch),
        ..WindowOptions::default()
    }
}

impl JointTimeline {
    pub fn new(input: &ReplayInput) -> Result<Self, ReplayError> {
        Self::new_with_geometry(input, local::enabled())
    }

    fn new_with_geometry(input: &ReplayInput, local_geometry: bool) -> Result<Self, ReplayError> {
        replay(input)?;
        let prepared = Prepared::new(input)?;
        let row = prepared.joint_row();
        Self::from_row(input, prepared, row, local_geometry, true)
    }

    /// Exact fixed-team/fixed-skill-order query with inclusive per-note hit
    /// limits. Every query owns its geometry; restrictions never enter the
    /// unrestricted model cache. None means the restricted domain is infeasible.
    pub fn maximum_in_frame_ranges(input: &ReplayInput, limits: &[Option<[u32; 2]>])
        -> Result<Option<JointTimelineResult>, SearchError> {
        let Some(model) = Self::in_frame_ranges(input, limits)? else { return Ok(None); };
        let power = ScoringPower::new(input.power).map_err(ReplayError::from)?;
        let result = model.maximum_optional(power)?;
        if let Some(winner) = &result {
            for (&frame, limit) in winner.result.hit_frames.iter().zip(limits) {
                if limit.is_some_and(|[a, b]| frame < a || frame > b) {
                    return Err(ReplayError::Input("restricted witness escaped its frame limits").into());
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn in_frame_ranges(input: &ReplayInput, limits: &[Option<[u32; 2]>])
        -> Result<Option<Self>, ReplayError> {
        replay(input)?;
        let prepared = Prepared::new(input)?;
        let Some(row) = prepared.restrict_row(prepared.joint_row(), limits)? else {
            return Ok(None);
        };
        Self::from_row(input, prepared, row, local::enabled(), false).map(Some)
    }

    pub(crate) fn maximum_optional(&self, power: ScoringPower)
        -> Result<Option<JointTimelineResult>, SearchError> {
        let costs = JointCosts::new_controlled(self, power, false)?;
        if self.controls.enabled(Switch::ClockStates) {
            self.maximum_optional_with(power, &mut clock::ClockCosts::new(costs))
        } else {
            self.maximum_optional_with(power, &mut { costs })
        }
    }

    pub(crate) fn score_optional(&self, power: ScoringPower) -> Result<Option<u64>, SearchError> {
        // Restricted domains need not contain the original baseline. Keep
        // infeasibility separate from a feasible zero score and from errors.
        let costs = JointCosts::new_controlled(self, power, false)?;
        let result = if self.controls.enabled(Switch::ClockStates) {
            self.score_optional_with::<false>(&mut clock::ClockCosts::new(costs))?
        } else { self.score_optional_with::<false>(&mut { costs })? };
        Ok(result.map(|(score, _)| score))
    }

    fn from_row(input: &ReplayInput, prepared: Prepared, row: geometry::Row,
        local_geometry: bool, prepare_functions: bool) -> Result<Self, ReplayError> {
        let mut stats = WindowStats {
            prepared_frames: prepared.frame_count,
            prepared_intervals: prepared.interval_count,
            graph_builds: 1,
            ..WindowStats::default()
        };
        let mut graph = if local_geometry {
            prepared.compile_local(&row, &local::protected(input)?, options(), &mut stats)
        } else {
            prepared.compile(&row, false, options(), &mut stats)
        }
            .expect("joint geometry has no exploration budget");
        let prepared_actions = Arc::new(PreparedActions::new(input, &graph)?);
        let value_layout = quotient::Layout::new(input, &graph, &prepared_actions);
        let (value_layout, packed_layout) = Self::value_views(&graph, value_layout);
        graph.freeze();
        let geometry_bytes = Self::geometry_footprint(&graph, &prepared_actions)
            + value_layout.as_ref().map_or(0, |l| l.bytes())
            + packed_layout.as_ref().map_or(0, |l| l.bytes());
        let model = Self {
            input: input.clone(),
            geometry_key: local::geometry_key(input, local_geometry)?,
            local_geometry,
            geometry_bytes,
            graph: Arc::new(graph),
            prepared_actions,
            value_layout,
            packed_layout,
            stats,
            functions: Default::default(),
            function_plan: None,
            controls: Controls::prepared(),
            machines: input
                .skills
                .iter()
                .map(|s| Machine::new(s, input.notes.len()))
                .collect::<Result<_, _>>()?,
            playing: input
                .skills
                .iter()
                .map(|s| countdown(s.duration) + 1)
                .collect(),
        };
        if prepare_functions { model.with_functions() } else { Ok(model) }
    }

    /// Reuse only geometry, never value states or a winner. Within this cache
    /// the chart/routing/trigger identities are fixed; skill magnitudes and
    /// durations affect the value DP, and correction changes the raw frame set.
    pub(crate) fn with_skills(&self, skills: &[ReplaySkill]) -> Result<Self, ReplayError> {
        let mut input = self.input.clone();
        input.skills = skills.to_vec();
        replay(&input)?;
        let geometry_key = local::geometry_key(&input, self.local_geometry)?;
        self.with_validated_input(input, skills, geometry_key)
    }

    pub(crate) fn with_skills_key(&self, skills: &[ReplaySkill], geometry_key: Vec<(u8, u32, u32)>)
        -> Result<Self, ReplayError> {
        let mut input = self.input.clone();
        input.skills = skills.to_vec();
        replay(&input)?;
        self.with_validated_input(input, skills, geometry_key)
    }

    /// The fixed-chart preparation owner has already validated this baseline.
    pub(crate) fn with_validated_skills_key(&self, skills: &[ReplaySkill],
        geometry_key: Vec<(u8, u32, u32)>) -> Result<Self, ReplayError> {
        let mut input = self.input.clone();
        input.skills = skills.to_vec();
        self.with_validated_input(input, skills, geometry_key)
    }

    fn with_validated_input(&self, input: ReplayInput, skills: &[ReplaySkill],
        geometry_key: Vec<(u8, u32, u32)>) -> Result<Self, ReplayError> {
        if geometry_key != self.geometry_key {
            return Self::new_with_geometry(&input, self.local_geometry);
        }
        Self {
            input,
            graph: Arc::clone(&self.graph),
            prepared_actions: Arc::clone(&self.prepared_actions),
            value_layout: self.value_layout.clone(),
            packed_layout: self.packed_layout.clone(),
            geometry_key,
            local_geometry: self.local_geometry,
            geometry_bytes: self.geometry_bytes,
            stats: self.stats.clone(),
            functions: Arc::clone(&self.functions),
            function_plan: None,
            controls: self.controls,
            machines: skills
                .iter()
                .map(|s| Machine::new(s, self.input.notes.len()))
                .collect::<Result<_, _>>()?,
            playing: skills.iter().map(|s| countdown(s.duration) + 1).collect(),
        }.with_functions()
    }

    fn with_functions(mut self) -> Result<Self, ReplayError> {
        self.function_plan = if self.functions_enabled() { functions::prepare(&self)? } else { None };
        Ok(self)
    }

    pub(crate) fn geometry_key(&self) -> &[(u8, u32, u32)] {
        &self.geometry_key
    }
    pub(crate) fn geometry_identity(&self) -> usize {
        Arc::as_ptr(&self.graph) as usize
    }

    /// Within this fixed model, power affects transitions only through the
    /// integer score before skills/Fever. Cover both accepted effective grades
    /// and every possible Combo rank; equal vectors imply equal edge weights
    /// for every reachable clock/skill state, not merely for one winning plan.
    pub(crate) fn score_signature(&self, power: ScoringPower) -> Option<Vec<u32>> {
        let scorer = NoteScorer::new(power, self.input.play_level, self.input.notes.len() as u32).ok()?;
        let mut signature = Vec::new();
        let mut previous = None;
        for rank in 1..=self.input.notes.len() as u32 {
            let combo = self.input.combo_multiplier(self.input.initial_combo + rank);
            if previous == Some(combo.to_bits()) {
                continue;
            }
            previous = Some(combo.to_bits());
            for grade in [Judgement::Great, Judgement::Perfect] {
                // Zero final multipliers expose the native first truncation
                // without introducing overflow from an unused second stage.
                signature.push(scorer.score(grade, combo, 0., 0.).ok()?.before_skill);
            }
        }
        Some(signature)
    }

    pub fn score(&self, power: ScoringPower) -> Result<u64, SearchError> {
        if !self.controls.enabled(Switch::FastValueStats) {
            self.score_with_stats(power).map(|(score, _)| score)
        } else { self.score_query::<false>(power).map(|(score, _)| score) }
    }

    pub fn function_stats(&self) -> FunctionStats {
        self.functions.lock().unwrap().stats()
    }

    /// A fresh value query with its actual work counters, without a witness.
    pub fn score_with_stats(&self, power: ScoringPower) -> Result<(u64, WindowStats), SearchError> {
        self.score_query::<true>(power)
    }

    fn score_query<const RECORD: bool>(&self, power: ScoringPower) -> Result<(u64, WindowStats), SearchError> {
        if self.controls.enabled(Switch::ScoreCircuits) {
            if let Some(answer) = circuit::score(self, power)? { return Ok(answer); }
        }
        if self.functions_enabled() && !self.controls.enabled(Switch::ClockStates) {
            if let Some(answer) = functions::score(self, power)? { return Ok(answer); }
        }
        let costs = JointCosts::new(self, power)?;
        if self.controls.enabled(Switch::ClockStates)
            || (costs.bucket_clocks && self.controls.enabled(Switch::InternedClocks)) {
            self.score_with_record::<RECORD>(&mut clock::ClockCosts::new(costs))
        } else {
            self.score_with_record::<RECORD>(&mut { costs })
        }
    }

    #[cfg(test)]
    fn score_with(&self, costs: &mut impl ActionCosts) -> Result<(u64, WindowStats), SearchError> {
        self.score_with_record::<true>(costs)
    }

    fn functions_enabled(&self) -> bool {
        self.controls.enabled(Switch::ComponentFunctions) && self.controls.enabled(Switch::ValueBlocks)
    }

    fn value_views(graph: &Graph, layout: Option<Arc<quotient::Layout>>)
        -> (Option<Arc<quotient::Layout>>, Option<Arc<solve::blocks::packed::Layout>>) {
        if std::env::var("BANDORI_TIMING_DISABLE_PACKED_VALUE_GRAPH").as_deref() == Ok("1")
            || std::env::var("BANDORI_TIMING_DISABLE_FUSED_EXITS").as_deref() == Ok("0") {
            return (layout, None);
        }
        let packed = if let Some(view) = &layout {
            solve::blocks::packed::Layout::new(&view.layers, &view.touches, &view.temporal)
        } else {
            solve::blocks::packed::Layout::new(&graph.layers, &graph.touches, &graph.temporal)
        };
        if packed.is_some() { (None, packed) } else { (layout, None) }
    }

    fn score_with_record<const RECORD: bool>(&self, costs: &mut impl ActionCosts) -> Result<(u64, WindowStats), SearchError> {
        self.score_optional_with::<RECORD>(costs)?.ok_or_else(||
            ReplayError::Input("validated baseline disappeared from joint geometry").into())
    }

    fn score_optional_with<const RECORD: bool>(&self, costs: &mut impl ActionCosts) -> Result<Option<(u64, WindowStats)>, SearchError> {
        let mut stats = WindowStats::default();
        let opts = options_with(self.controls);
        let answer = if let Some(layout) = self.packed_layout.as_ref()
            .filter(|_| costs.supports_native_blocks() && opts.compact_value_states && !opts.incremental_touch) {
            solve::blocks::packed::evaluate::<RECORD>(&self.graph, layout, costs, &mut stats)
        } else if let Some(layout) = self.value_layout.as_ref()
            .filter(|_| costs.supports_native_blocks() && opts.compact_value_states && !opts.incremental_touch) {
            solve::blocks::evaluate_layout_record::<RECORD>(&self.graph, layout, costs, &mut stats)
        } else {
            solve::evaluate_value_record::<RECORD>(&self.graph, costs,
                WindowObjective::Native, opts, &mut stats)
        }?
        .expect("joint value DP has no exploration budget");
        if RECORD { costs.finish_stats(&mut stats); }
        Ok(answer.map(|answer| (answer.score, stats)))
    }

    pub fn maximum(&self, power: ScoringPower) -> Result<JointTimelineResult, SearchError> {
        // Preserve the original ordered witness and tie traversal. The value
        // query may quotient clocks without changing any attainable score.
        let costs = JointCosts::new_controlled(self, power, false)?;
        if self.controls.enabled(Switch::ClockStates) {
            self.maximum_with(power, &mut clock::ClockCosts::new(costs))
        } else {
            self.maximum_with(power, &mut { costs })
        }
    }

    fn maximum_with(&self, power: ScoringPower, costs: &mut impl ActionCosts)
        -> Result<JointTimelineResult, SearchError> {
        self.maximum_optional_with(power, costs)?.ok_or_else(||
            ReplayError::Input("validated baseline disappeared from joint geometry").into())
    }

    fn maximum_optional_with(&self, power: ScoringPower, costs: &mut impl ActionCosts)
        -> Result<Option<JointTimelineResult>, SearchError> {
        let mut stats = self.stats.clone();
        let Some((value, frames)) = solve::evaluate_ordered(
            &self.graph,
            costs,
            WindowObjective::Native,
            options_with(self.controls),
            &mut stats,
        )?
        .expect("joint value DP has no exploration budget") else { return Ok(None); };
        costs.finish_stats(&mut stats);
        let mut request = self.input.clone();
        request.power = power.value();
        request.hit_frames = frames.frames;
        request.hit_order = frames.order;
        let result = replay(&request)?;
        if value.score != result.total {
            return Err(SearchError::Mismatch {
                cached: value.score,
                replayed: result.total,
            });
        }
        Ok(Some(JointTimelineResult { result, stats }))
    }

    pub(crate) fn footprint(&self) -> usize {
        self.geometry_bytes
    }

    fn geometry_footprint(graph: &Graph, prepared: &PreparedActions) -> usize {
        prepared.footprint()
            + graph.actions.bytes()
            + graph.layers.capacity() * std::mem::size_of::<geometry::Layer>()
            + graph.touches.capacity() * std::mem::size_of::<geometry::TouchDag>()
            + graph.touches.iter().map(|f| f.inputs.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
            + graph.touches.iter().filter_map(|f| f.incremental.as_ref())
                .map(|p| std::mem::size_of::<geometry::incremental::Proof>() + p.repeated.capacity())
                .sum::<usize>()
            + graph.temporal.capacity() * std::mem::size_of::<geometry::TemporalBlock>()
            + graph.temporal.iter().map(|b| b.permutation.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
            + graph
                .layers
                .iter()
                .map(|l| l.edges.bytes())
                .sum::<usize>()
    }
}

#[derive(Clone, Copy, Default, Debug, Hash, PartialEq, Eq)]
struct Window {
    start: u32,
    end: u32,
    slot: u8,
}

#[derive(Clone, Default, Debug, Hash, PartialEq, Eq)]
struct State {
    ready: u32,
    // Most note transitions only read this queue. Copy it only when a trigger
    // is appended or a prefix expires; interned states remain immutable.
    windows: Rc<Vec<Window>>,
    progress: usize,
}
impl State {
    fn advance(&mut self, frame: u32) {
        let expired = self.windows.partition_point(|w| w.end < frame);
        if expired != 0 {
            Rc::make_mut(&mut self.windows).drain(..expired);
            self.progress = 0;
        }
        if self.ready <= frame {
            self.ready = 0;
        }
    }
    fn normalize_active_starts(&mut self, frame: u32) {
        if self.windows.iter().any(|w| w.start != 0 && w.start < frame) {
            for window in Rc::make_mut(&mut self.windows) {
                if window.start < frame { window.start = 0; }
            }
        }
    }
}

struct JointCosts<'a> {
    model: &'a JointTimeline,
    scorer: NoteScorer,
    score_table: Option<ScoreTable>,
    outside_scores: HashMap<usize, Result<u64, crate::scoring::ScoringError>>,
    states: Vec<State>,
    ids: HashMap<State, usize>,
    cooling: u32,
    progress_dominance: bool,
    continuous_dominance: bool,
    continuous_window_dominance: bool,
    operators: operators::Operators,
    bucket_clocks: bool,
    window_dominance: bool,
    window_pareto: bool,
}
impl<'a> JointCosts<'a> {
    fn new(model: &'a JointTimeline, power: ScoringPower) -> Result<Self, ReplayError> {
        Self::new_controlled(model,power,
            model.controls.enabled(Switch::ClockBuckets))
    }
    fn new_controlled(model: &'a JointTimeline, power: ScoringPower, bucket_clocks: bool)
        -> Result<Self, ReplayError> {
        let scorer = NoteScorer::new(
            power,
            model.input.play_level,
            model.input.notes.len() as u32,
        )?;
        // Ordered witness queries pass false and retain their original ties.
        let continuous_dominance = bucket_clocks
            && model.controls.enabled(Switch::ContinuousDominance);
        Ok(Self {
            model,
            continuous_dominance,
            continuous_window_dominance: continuous_dominance &&
                model.controls.enabled(Switch::ContinuousWindowDominance),
            window_pareto: model.controls.enabled(Switch::WindowPareto),
            bucket_clocks,
            window_dominance: bucket_clocks && model.controls.enabled(Switch::WindowDominance),
            scorer,
            score_table: model.controls.enabled(Switch::JointPrecompute)
                .then(|| ScoreTable::new(model, scorer, prepared::MAX_SCORE_CELLS))
                .flatten(),
            outside_scores: if model.controls.enabled(Switch::OutsideScoreProgram) {
                model.prepared_actions.outside.iter().map(|run| (run.index,run.score(scorer))).collect()
            } else { model.graph.actions.iter().enumerate()
                .filter(|(_, action)| matches!(action.hits, geometry::ActionHits::Run(_)))
                .map(|(i, action)| (i, Self::outside_score(model, scorer, &action))).collect() },
            states: vec![State::default()],
            ids: HashMap::from([(State::default(), 0)]),
            cooling: countdown(0.75) + 1,
            operators: operators::Operators::new(model.controls),
            progress_dominance: model.controls.enabled(Switch::ProgressDominance)
                && model.input.skills.iter().zip(&model.machines).any(|(skill, machine)| {
                    machine.progress_strength(&skill.score,0,continuous_dominance).is_some()
                }),
        })
    }

    fn plane_strength(&self,slot:usize,progress:usize) -> Option<usize> {
        if !(self.window_dominance && self.progress_dominance
            && self.model.machines[slot].baseline_dominance) {return None;}
        let effect=&self.model.input.skills[slot].score;
        match effect {
            crate::hit_timing::ScoreEffect::Continuous {..} if self.continuous_window_dominance =>
                self.model.machines[slot].progress_strength(effect,progress,true),
            crate::hit_timing::ScoreEffect::Crescendo {..} if self.window_pareto =>
                self.model.machines[slot].progress_strength(effect,progress,false),
            _=>None,
        }
    }
}
impl ActionCosts for JointCosts<'_> {
    fn value_controls(&self) -> Controls { self.model.controls }
    fn supports_native_blocks(&self) -> bool {
        self.model.controls.enabled(Switch::ValueBlocks)
    }
    fn native_block_operator(&self, index: usize) -> Option<(u32, u8, usize)> {
        let action = &self.model.graph.actions.get(index);
        if matches!(action.hits, geometry::ActionHits::Run(_)) { return None; }
        let (note, frame) = action.hits[0];
        if self.model.input.notes[note].trigger.is_some() { return None; }
        let prepared = self.model.prepared_actions.actions[index];
        Some((frame, prepared.raw as u8, prepared.band))
    }
    fn finish_stats(&self, stats: &mut WindowStats) { self.operators.stats(stats); }
    fn supports_temporal_intervals(&self) -> bool { true }
    fn stable_interval(&self, state: usize, first: u32, last: u32) -> bool {
        let state = &self.states[state];
        let changes = |boundary: u64| u64::from(first) < boundary && boundary <= u64::from(last);
        !changes(u64::from(state.ready)) && state.windows.iter().all(|window| {
            // start==frame is inactive; end==frame remains active. Advancing
            // removes the same FIFO prefix and clears the same ready field.
            !changes(u64::from(window.start) + 1) && !changes(u64::from(window.end) + 1)
        })
    }
    fn supports_dominance(&self) -> bool {
        self.progress_dominance || self.window_dominance
    }
    fn dominance_plane(&self, state: usize) -> Option<(usize,usize,usize)> {
        let state=&self.states[state];
        if state.ready!=0 || state.windows.len()!=1 {return None;}
        let window=state.windows.first()?;
        let slot=usize::from(window.slot);
        if window.start!=0 {return None;}
        let strength=self.plane_strength(slot,state.progress)?;
        // No future trigger observes cooling: buckets erased ready only after
        // proving every later trigger follows the actual playing/cooling end.
        // Higher native strength remains higher under Great/Perfect; a later
        // end additionally scores at least the baseline until both expire.
        // Subsequent triggers therefore see the same reset progress/queue.
        Some((slot,strength,window.end as usize))
    }
    fn dominance_rank_extra(&self, state: usize) -> Option<(usize,u32,usize)> {
        if !self.window_dominance { return None; }
        let state=&self.states[state];
        if state.ready != 0 || state.windows.len()!=1 { return None; }
        let window=state.windows.first()?;
        if window.start != 0 || !self.model.machines[usize::from(window.slot)].baseline_dominance {
            return None;
        }
        // Same slot/progress, no pending skill, both already active, and future
        // triggers cannot observe cooling. A later end simulates every earlier
        // end: extra active hits score at least their outside value, and both
        // reset before any subsequent activation. No f32 reassociation occurs.
        Some((usize::from(window.slot),state.progress as u32,window.end as usize))
    }
    fn dominance_rank(&self, state: usize) -> Option<(usize, u32, usize)> {
        if !self.progress_dominance { return None; }
        let state = &self.states[state];
        let slot = usize::from(state.windows.first()?.slot);
        let strength=self.model.machines[slot].progress_strength(
            &self.model.input.skills[slot].score,state.progress,self.continuous_dominance)?;
        // Every interned state retains its immutable queue until this DP ends,
        // so its Rc allocation cannot be recycled while it identifies a group.
        // Equal queue identity and ready time give identical future legality
        // and clocks. Native Crescendo progress and Continuous clean/dirty
        // order are checked against the actual tables; hits preserve their
        // strength order until both histories reset on the same expiry.
        Some((Rc::as_ptr(&state.windows) as usize, state.ready, strength))
    }
    fn action(
        &mut self,
        action: &Action,
        state: usize,
    ) -> Result<Option<(usize, Value)>, ReplayError> {
        self.step(action, state, None)
    }
    fn action_at(
        &mut self,
        index: usize,
        action: &Action,
        state: usize,
    ) -> Result<Option<(usize, Value)>, ReplayError> {
        if matches!(action.hits, geometry::ActionHits::Run(_)) {
            return self.outside_step(action, state, self.outside_scores[&index]?);
        }
        let prepared = self.score_table.as_ref()
            .map(|_| self.model.prepared_actions.actions[index]);
        let (note, frame) = action.hits[0];
        let operator = prepared.filter(|_| self.operators.enabled()
            && self.model.input.notes[note].trigger.is_none());
        if let Some(prepared) = operator {
            if let Some(result) = self.operators.get(u64::from(frame), state, prepared) { return Ok(result); }
        }
        let result = self.step(action, state, prepared)?;
        if let Some(prepared) = operator { self.operators.put(u64::from(frame), state, prepared, result); }
        Ok(result)
    }
    fn hit(&mut self, _: Factor, _: usize) -> Result<(usize, Value), ReplayError> {
        unreachable!("joint scoring requires the hit event")
    }
    fn run(&mut self, _: Factor, _: usize, _: usize) -> Result<(usize, Value), ReplayError> {
        unreachable!("joint geometry does not collapse unproved runs")
    }
}

impl JointCosts<'_> {
    fn outside_score(
        model: &JointTimeline,
        scorer: NoteScorer,
        action: &Action,
    ) -> Result<u64, crate::scoring::ScoringError> {
        let mut total = 0u64;
        for offset in 0..action.hits.len() {
            let combo = model.input.combo_multiplier(
                model.input.initial_combo + (action.factor.rank + offset) as u32);
            let score = scorer.score(Judgement::Perfect, combo, 1.,
                if action.factor.fever { 2. } else { 1. })?.final_score;
            total = total.checked_add(u64::from(score))
                .ok_or(crate::scoring::ScoringError::TotalScoreOverflow)?;
        }
        Ok(total)
    }

    /// Power independent operands from the same incoming FIFO/progress state.
    /// The actual state transition still goes through the authoritative step.
    fn symbolic_terms(&self, index: usize, previous: usize)
        -> Option<Vec<crate::hit_timing::compiled::Term>> {
        let action = &self.model.graph.actions.get(index);
        if matches!(action.hits, geometry::ActionHits::Run(_)) {
            return Some((0..action.hits.len()).map(|offset| [
                Judgement::Perfect.multiplier().to_bits(),
                self.model.input.combo_multiplier(self.model.input.initial_combo
                    + (action.factor.rank + offset) as u32).to_bits(),
                (if action.factor.fever { 2_f32 } else { 1_f32 }).to_bits(),
            ]).collect());
        }
        let mut state = self.states[previous].clone();
        state.advance(action.hits[0].1);
        let active = state.windows.first().filter(|w| w.start < action.hits[0].1)
            .map(|w| usize::from(w.slot));
        let raw = self.model.prepared_actions.actions[index].raw;
        let corrected = active.and_then(|s| self.model.input.skills[s].correction)
            .is_some_and(|c| raw >= match c {
                JudgementCorrection::Great => Judgement::Great,
                JudgementCorrection::Good => Judgement::Good,
                JudgementCorrection::Bad => Judgement::Bad,
            });
        let grade = if corrected { Judgement::Perfect } else { raw };
        if grade < Judgement::Great { return None; }
        let multiplier = active.map_or(1., |slot|
            self.model.machines[slot].values[state.progress][usize::from(grade == Judgement::Perfect)]);
        let combined = (if action.factor.fever { 2_f32 } else { 1_f32 }) * multiplier;
        Some(vec![ [grade.multiplier().to_bits(), self.model.input.combo_multiplier(
            self.model.input.initial_combo + action.factor.rank as u32).to_bits(),
            (if combined == 0. { 0. } else { combined }).to_bits()] ])
    }

    fn intern_state(&mut self, state: State, previous: usize) -> usize {
        if state == self.states[previous] { return previous; }
        *self.ids.entry(state.clone()).or_insert_with(|| {
            let id = self.states.len();
            self.states.push(state);
            id
        })
    }

    fn outside_step(
        &mut self,
        action: &Action,
        previous: usize,
        score: u64,
    ) -> Result<Option<(usize, Value)>, ReplayError> {
        let mut state = self.states[previous].clone();
        let frame = action.hits.last().unwrap().1;
        // No hit of this transfer can be in an active skill. Advancing directly
        // to its last hit removes exactly the same expired FIFO prefix and
        // resets the same progress/ready fields as stepping through every hit.
        state.advance(frame);
        if self.bucket_clocks { state.normalize_active_starts(frame); }
        debug_assert!(state.windows.first().is_none_or(|window| window.start >= frame));
        Ok(Some((self.intern_state(state, previous), Value { score, coefficient: 0. })))
    }

    fn step(
        &mut self,
        action: &Action,
        state: usize,
        prepared: Option<PreparedAction>,
    ) -> Result<Option<(usize, Value)>, ReplayError> {
        if matches!(action.hits, geometry::ActionHits::Run(_)) {
            return self.outside_step(action, state, Self::outside_score(self.model, self.scorer, action)?);
        }
        debug_assert_eq!(action.hits.len(), 1);
        let (note, frame) = action.hits[0];
        let note = &self.model.input.notes[note];
        let previous = state;
        let mut state = self.states[previous].clone();
        state.advance(frame);
        if self.bucket_clocks { state.normalize_active_starts(frame); }
        let active = state
            .windows
            .first()
            .filter(|w| w.start < frame)
            .map(|w| usize::from(w.slot));
        let raw = match prepared {
            Some(action) => action.raw,
            None => note.raw_judgement(frame)?,
        };
        let Some((progress, value)) = self.hit_value(action, raw, active, state.progress, prepared)?
            else { return Ok(None); };
        state.progress = progress;
        // Inputs precede the clock update: this trigger cannot benefit from its
        // own skill, nor start before the previous skill's native cooling ends.
        if let Some(slot) = note.trigger {
            let start = frame.max(state.ready);
            let end = start + self.model.playing[slot];
            let window = Window {
                start,
                end,
                slot: slot as u8,
            };
            let ready = end + self.cooling + 1;
            let (window,ready) = if self.bucket_clocks {
                self.model.prepared_actions.buckets.window(slot,start,end,ready)
            } else {(window,ready)};
            Rc::make_mut(&mut state.windows).push(window);
            state.ready = ready;
        }
        let next = self.intern_state(state, previous);
        Ok(Some((next, Value { score: u64::from(value), coefficient: 0. })))
    }

    /// Shared authoritative grade/progress/score arithmetic for both registries.
    fn hit_value(&self, action: &Action, raw: Judgement, active: Option<usize>,
        progress: usize, prepared: Option<PreparedAction>)
        -> Result<Option<(usize, u32)>, ReplayError> {
        let corrected = active
            .and_then(|s| self.model.input.skills[s].correction)
            .is_some_and(|c| {
                raw >= match c {
                    JudgementCorrection::Great => Judgement::Great,
                    JudgementCorrection::Good => Judgement::Good,
                    JudgementCorrection::Bad => Judgement::Bad,
                }
            });
        let grade = if corrected { Judgement::Perfect } else { raw };
        if grade < Judgement::Great {
            return Ok(None);
        }
        let mut next_progress = progress;
        let multiplier = if let Some(slot) = active {
            let machine = &self.model.machines[slot];
            let grade = usize::from(grade == Judgement::Perfect);
            let multiplier = machine.values[progress][grade];
            next_progress = machine.next[progress][grade];
            multiplier
        } else {
            1.
        };
        let value = if let Some(action) = prepared {
            self.score_table.as_ref().unwrap()
                .score(action.band, active, progress, grade)?
        } else { self
            .scorer
            .score(
                grade,
                self.model
                    .input
                    .combo_multiplier(self.model.input.initial_combo + action.factor.rank as u32),
                multiplier,
                if action.factor.fever { 2. } else { 1. },
            )?
            .final_score };
        Ok(Some((next_progress, value)))
    }
}

#[cfg(test)]
mod tests;
