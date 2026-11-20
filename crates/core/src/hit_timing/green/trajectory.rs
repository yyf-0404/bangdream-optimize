//! Preparation adapter from native launch/clock facts to green windows.
//! Fixed B=0 / 120 FPS; root then ordered after-node updates, as NoteSlide.OnUpdate.
//! Positions use supplied final runtime geometry, not the scene's launcher value.
//! A source adapter must still certify its factory records and coordinate origin.
use super::{
    activation::MovementState,
    compact::GreenChainNode,
    launch::{LaunchStep, LaunchTimeline, NativeBatch},
    lifetime::{LifetimeUpdate, UnsettledLifetime, VisibleAfter, VisibleState},
    motion::{judge_table, GreenGeometry, GreenJudgeTable},
    projection::{HiddenBridge, HiddenGate},
    speed::SpeedVariant,
    GreenRole,
};
#[cfg(test)]
use super::{clock::FRAME_SECONDS, compact::GreenFrameProfile, launch::BatchDisposition};
use crate::hit_timing::{ReplayError, MAX_SECONDS, REPLAY_FPS};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeGreenNode {
    pub absolute_position: i32,
    pub note_bpm: f32,
    /// None for a hidden node; otherwise an index into the scorer's note array.
    pub note_index: Option<usize>,
    pub role: GreenRole,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeGreenChain {
    pub root_batch: usize,
    /// Exact original after-list order. No chart-time sort is applied here.
    pub nodes: Vec<NativeGreenNode>,
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    AfterWait,
    Move,
    HeadWait,
    AfterStop,
}
#[cfg(test)]
struct MovingNode {
    phase: Phase,
    movement: MovementState,
    local_y: f32,
    first_frame: Option<u32>,
    ys: Vec<f32>,
    life: UnsettledLifetime,
    expires_after: Option<u32>,
    first_middle: Option<u32>,
}
#[cfg(test)]
impl MovingNode {
    fn waiting() -> Self {
        Self {
            phase: Phase::AfterWait,
            movement: MovementState::default(),
            local_y: f32::MAX,
            first_frame: None,
            ys: vec![],
            life: UnsettledLifetime::default(),
            expires_after: None,
            first_middle: None,
        }
    }
    fn snapshot(&mut self, frame: u32, table: &[f32; 16]) {
        if self.phase == Phase::AfterWait || self.expires_after.is_some() {
            return;
        }
        self.first_frame.get_or_insert(frame);
        self.ys.push(self.local_y);
        if self.local_y <= table[7] {
            self.first_middle.get_or_insert(frame);
        }
    }
    fn refresh_y(&mut self, g: GreenGeometry, table: &[f32; 16]) -> Result<(), ReplayError> {
        // No movement call at exact launch: VirtualPosY still has its native sentinel.
        if self.movement.progress > 0. {
            self.local_y = g.position_y(self.movement.progress)?.max(table[8]);
        }
        Ok(())
    }
}
fn validate(chain: &NativeGreenChain, batches: &[NativeBatch]) -> Result<(), ReplayError> {
    let n = &chain.nodes;
    if n.len() < 2
        || n[0].note_index.is_none()
        || n.last().unwrap().note_index.is_none()
        || !matches!(n[0].role, GreenRole::Head)
        || !matches!(
            n.last().unwrap().role,
            GreenRole::PlainEnd | GreenRole::HeldArrowEnd
        )
        || n[1..n.len() - 1]
            .iter()
            .any(|x| !matches!(x.role, GreenRole::HeldMiddle))
        || n.windows(2)
            .any(|w| w[0].absolute_position >= w[1].absolute_position)
        || n.iter()
            .any(|x| x.absolute_position < 0 || !x.note_bpm.is_finite() || x.note_bpm <= 0.)
    {
        return Err(ReplayError::Input("invalid native green factory chain"));
    }
    let batch = batches
        .get(chain.root_batch)
        .ok_or(ReplayError::Input("missing native root batch"))?;
    if batch.empty || batch.position.absolute()? != n[0].absolute_position {
        return Err(ReplayError::Input("native root batch and chain disagree"));
    }
    let ids: std::collections::HashSet<_> = n.iter().filter_map(|x| x.note_index).collect();
    if ids.len() != n.iter().filter(|x| x.note_index.is_some()).count() {
        return Err(ReplayError::Input("duplicate native scoring node"));
    }
    Ok(())
}

/// One clock trace shared by every chain at a speed. Native recurrences are
/// evaluated once; later chains reuse exact f32 values without recomputation.
struct SharedTimeline {
    driver: LaunchTimeline,
    steps: Vec<LaunchStep>,
    roots: Vec<Option<usize>>,
}
impl SharedTimeline {
    fn new(speed: f32, bpm: f32, batches: &[NativeBatch]) -> Result<Self, ReplayError> {
        Ok(Self {
            driver: LaunchTimeline::new(speed, bpm, batches.to_vec())?,
            steps: vec![],
            roots: vec![None; batches.len()],
        })
    }
    fn step(&mut self, frame: usize) -> Result<&LaunchStep, ReplayError> {
        while self.steps.len() <= frame {
            let s = self.driver.update_120()?;
            if let Some(b) = s.batch {
                self.roots[b.index] = Some(self.steps.len());
            }
            self.steps.push(s);
        }
        Ok(&self.steps[frame])
    }
    fn root(&mut self, batch: usize) -> Result<LaunchStep, ReplayError> {
        loop {
            if let Some(i) = self.roots[batch] {
                return Ok(self.steps[i].clone());
            }
            self.step(self.steps.len())?;
        }
    }
}

/// Generate one complete speed variant. This is chart preparation only: neither
/// positions, hidden nodes nor native clocks are stored in the team-search state.
/// Inputs must use one shared native frame origin (MusicClock120 frame zero).
/// On complete plays root settlement cannot affect an after-node's stop if the
/// natural root Wait already precedes that node's first button crossing. Check
/// that condition from the actual trajectory instead of silently assuming it.
pub fn prepare_chain_variant(
    chain: &NativeGreenChain,
    basic_bpm: f32,
    batches: &[NativeBatch],
    geometry: GreenGeometry,
) -> Result<SpeedVariant, ReplayError> {
    let mut timeline = SharedTimeline::new(geometry.specific_speed, basic_bpm, batches)?;
    prepare_with_clock(chain, batches, geometry, &mut timeline)
}
// The full state walk is retained below as a test oracle.
fn prepare_with_clock(
    chain: &NativeGreenChain,
    batches: &[NativeBatch],
    geometry: GreenGeometry,
    timeline: &mut SharedTimeline,
) -> Result<SpeedVariant, ReplayError> {
    validate(chain, batches)?;
    if geometry.button_world_y != geometry.button_local_y {
        return Err(ReplayError::Input(
            "native green trajectory needs a local Y transform",
        ));
    }
    let table = judge_table(geometry, 0)?;
    let crossings = progress_crossings(geometry, &table)?;
    prepare_with_table(chain, geometry, &crossings, timeline)
}
fn progress_crossings(
    geometry: GreenGeometry,
    table: &GreenJudgeTable,
) -> Result<[f32; 7], ReplayError> {
    let upper = table.progress_rates[15];
    Ok([
        geometry.first_progress_at_or_below(table.positions[0], upper)?,
        geometry.first_progress_at_or_below(table.positions[1], upper)?,
        geometry.first_progress_at_or_below(table.positions[2], upper)?,
        geometry.first_progress_at_or_below(table.positions[5], upper)?,
        geometry.first_progress_at_or_below(table.positions[7], upper)?,
        geometry.first_progress_at_or_below(table.positions[8], upper)?,
        geometry.first_progress_at_or_below(geometry.button_local_y, upper)?,
    ])
}
// Inputs and geometry are checked once by the public caller. The immutable
// crossings belong to this exact speed/geometry, not to a particular chain.
fn prepare_with_table(
    chain: &NativeGreenChain,
    geometry: GreenGeometry,
    crossings: &[f32; 7],
    timeline: &mut SharedTimeline,
) -> Result<SpeedVariant, ReplayError> {
    let speed = geometry.specific_speed;
    let root = timeline.root(chain.root_batch)?;
    let root_seed = root.root_seed(
        chain.nodes[0].absolute_position,
        chain.nodes[0].note_bpm,
        speed,
    )?;
    let mut starts = Vec::with_capacity(chain.nodes.len());
    let mut stops: Vec<u32> = vec![];
    // All relevant raw grade gates are reached by the natural stop. Moving the
    // unjudged virtual position afterwards cannot change these grade cuts.
    for (i, node) in chain.nodes.iter().enumerate() {
        let (first, mut movement) = if i == 0 {
            (root.next_input_frame, root_seed)
        } else {
            let min = root.next_input_frame as usize;
            let target = node.absolute_position as f32;
            while timeline.steps.len() <= min
                || timeline
                    .steps
                    .last()
                    .unwrap()
                    .existing_notes_clock
                    .launcher
                    .absolute_position()?
                    < target
            {
                timeline.step(timeline.steps.len())?;
            }
            let found = timeline
                .steps
                .partition_point(|s| {
                    s.existing_notes_clock.launcher.absolute_position().unwrap() < target
                })
                .max(min);
            let clock = timeline.step(found)?.existing_notes_clock;
            let mut m = MovementState::default();
            // Wait's MoveSecond counter is not read by progress, position or
            // lifetime; RealMoveSecond remains zero until activateAdjust.
            m.activate(
                clock.launcher.absolute_position()?,
                node.absolute_position,
                node.note_bpm,
                speed,
            )?;
            (found as u32 + 1, m)
        };
        let role = node.role;
        let progress_gates = match role {
            GreenRole::Head | GreenRole::PlainEnd => {
                [crossings[0], crossings[1], crossings[2], crossings[3]]
            }
            GreenRole::HeldMiddle => [crossings[4]; 4],
            GreenRole::HeldArrowEnd => [crossings[5]; 4],
        };
        let mut cuts = [u32::MAX; 4];
        let mut sample = |f: u32, m: &MovementState| {
            for (cut, &gate) in cuts.iter_mut().zip(&progress_gates) {
                if m.progress > 0. && m.progress >= gate {
                    *cut = (*cut).min(f);
                }
            }
        };
        sample(first, &movement);
        let stop_progress = if i > 0 && i + 1 < chain.nodes.len() {
            crossings[6]
        } else {
            crossings[5]
        };
        let mut stopped = None;
        for frame in first + 1..=MAX_SECONDS * REPLAY_FPS {
            movement.move_120(speed)?;
            sample(frame, &movement);
            if movement.progress > 1. && movement.progress >= stop_progress {
                if i > 0 && i + 1 < chain.nodes.len() && frame < stops[0] {
                    return Err(ReplayError::Input(
                        "root hit time can change green after-node stop",
                    ));
                }
                stopped = Some(frame);
                break;
            }
        }
        let stop = stopped.ok_or(ReplayError::Input("unterminated native green movement"))?;
        if cuts[3] > stop {
            return Err(ReplayError::Input(
                "green grade still changes after stopping",
            ));
        }
        starts.push(cuts);
        stops.push(stop);
    }
    let mut expiry = vec![];
    for (i, node) in chain.nodes.iter().enumerate() {
        let next = (i + 1..chain.nodes.len()).find(|&j| chain.nodes[j].note_index.is_some());
        let mut life = UnsettledLifetime::default();
        let mut end = None;
        for frame in stops[i]..MAX_SECONDS * REPLAY_FPS {
            let clock = timeline
                .step(frame as usize)?
                .existing_notes_clock
                .judgement_clock_b0()?;
            let after = next.map(|j| VisibleAfter {
                absolute_pos: chain.nodes[j].absolute_position,
                state: if stops[j] <= frame {
                    VisibleState::Stop
                } else {
                    VisibleState::Move
                },
            });
            let update = if i == 0 {
                LifetimeUpdate::HeadWait {
                    clock,
                    next_visible_pos: after.unwrap().absolute_pos,
                }
            } else {
                LifetimeUpdate::AfterStop {
                    clock,
                    has_direct_after: i + 1 < chain.nodes.len(),
                    next_visible: after,
                    arrow_terminal: matches!(node.role, GreenRole::HeldArrowEnd),
                    execute_frame: 0.5,
                    adjust_b: 0,
                }
            };
            if life.update(node.absolute_position, update)?.is_some() {
                end = Some(frame);
                break;
            }
        }
        expiry.push(end.ok_or(ReplayError::Input("unterminated native green lifetime"))?);
    }
    let mut nodes = vec![];
    let mut bridges = vec![];
    let mut hidden = vec![];
    for (i, s) in chain.nodes.iter().enumerate() {
        if let Some(note_index) = s.note_index {
            if !nodes.is_empty() {
                bridges.push(HiddenBridge {
                    hidden: std::mem::take(&mut hidden),
                    successor: s.role,
                });
            }
            nodes.push(GreenChainNode {
                note_index,
                window: crate::hit_timing::NoteWindow::NativeGreen {
                    first_frames: starts[i],
                    last_frame: expiry[i],
                },
                minimum_gap_from_previous: if nodes.is_empty() { 0 } else { 1 },
            });
        } else {
            hidden.push(HiddenGate {
                first_ready_frame: Some(starts[i][3]),
                expires_after_frame: Some(expiry[i]),
            });
        }
    }
    Ok(SpeedVariant { nodes, bridges })
}

#[cfg(test)]
fn prepare_reference_with_clock(
    chain: &NativeGreenChain,
    batches: &[NativeBatch],
    geometry: GreenGeometry,
    timeline: &mut SharedTimeline,
) -> Result<SpeedVariant, ReplayError> {
    validate(chain, batches)?;
    // Current official source has identity Y parents. Reject a nonidentity
    // transform; world Y cannot be compared directly with a local judge table.
    if geometry.button_world_y != geometry.button_local_y {
        return Err(ReplayError::Input(
            "native green trajectory needs a local Y transform",
        ));
    }
    let speed = geometry.specific_speed;
    let table = judge_table(geometry, 0)?.positions;
    let mut moving: Vec<_> = chain.nodes.iter().map(|_| MovingNode::waiting()).collect();
    let next_visible: Vec<_> = (0..chain.nodes.len())
        .map(|i| (i + 1..chain.nodes.len()).find(|&j| chain.nodes[j].note_index.is_some()))
        .collect();
    let root_step = timeline.root(chain.root_batch)?;
    if root_step.batch.unwrap().disposition != BatchDisposition::Launched {
        return Err(ReplayError::Input("green root was not launched"));
    }
    moving[0].movement = root_step.root_seed(
        chain.nodes[0].absolute_position,
        chain.nodes[0].note_bpm,
        speed,
    )?;
    moving[0].phase = Phase::Move;
    moving[0].refresh_y(geometry, &table)?;
    for frame in root_step.next_input_frame..MAX_SECONDS * REPLAY_FPS {
        {
            for n in &mut moving {
                n.snapshot(frame, &table);
            }
        }
        let step = timeline.step(frame as usize)?;
        {
            let clock = step.existing_notes_clock;
            for i in 0..moving.len() {
                let source = &chain.nodes[i];
                let after = next_visible[i].map(|j| VisibleAfter {
                    absolute_pos: chain.nodes[j].absolute_position,
                    state: match moving[j].phase {
                        Phase::AfterWait => VisibleState::Wait,
                        Phase::Move => VisibleState::Move,
                        Phase::AfterStop => VisibleState::Stop,
                        Phase::HeadWait => VisibleState::Wait,
                    },
                });
                let root_waiting = moving[0].phase == Phase::HeadWait;
                let has_after = i + 1 < moving.len();
                let n = &mut moving[i];
                let update = match n.phase {
                    Phase::AfterWait => {
                        if n.movement.wait_120(
                            clock,
                            source.absolute_position,
                            source.note_bpm,
                            speed,
                        )? {
                            n.phase = Phase::Move;
                            n.refresh_y(geometry, &table)?;
                        }
                        LifetimeUpdate::Move
                    }
                    Phase::Move => {
                        n.movement.move_120(speed)?;
                        let raw_y = geometry.position_y(n.movement.progress)?;
                        n.refresh_y(geometry, &table)?;
                        if i > 0
                            && has_after
                            && n.movement.progress > 1.
                            && raw_y <= geometry.button_local_y
                            && !root_waiting
                        {
                            return Err(ReplayError::Input(
                                "root hit time can change green after-node stop",
                            ));
                        }
                        let stop_line = if i > 0 && has_after {
                            geometry.button_local_y
                        } else {
                            table[8]
                        };
                        if n.movement.progress > 1. && raw_y <= stop_line {
                            n.phase = if i == 0 {
                                Phase::HeadWait
                            } else {
                                Phase::AfterStop
                            };
                        }
                        // Changing state does not also execute the new state's handler.
                        LifetimeUpdate::Move
                    }
                    Phase::HeadWait => LifetimeUpdate::HeadWait {
                        clock: clock.judgement_clock_b0()?,
                        next_visible_pos: after
                            .ok_or(ReplayError::Input("head missing visible successor"))?
                            .absolute_pos,
                    },
                    Phase::AfterStop => {
                        if has_after && after.is_some() {
                            n.movement.virtual_move_120(speed)?;
                            n.refresh_y(geometry, &table)?;
                        } else {
                            n.movement.move_seconds += FRAME_SECONDS;
                            n.movement.virtual_move_seconds += FRAME_SECONDS;
                        }
                        LifetimeUpdate::AfterStop {
                            clock: clock.judgement_clock_b0()?,
                            has_direct_after: has_after,
                            next_visible: after,
                            arrow_terminal: matches!(source.role, GreenRole::HeldArrowEnd),
                            execute_frame: 0.5,
                            adjust_b: 0,
                        }
                    }
                };
                if n.phase != Phase::AfterWait
                    && n.expires_after.is_none()
                    && n.life.update(source.absolute_position, update)?.is_some()
                {
                    n.expires_after = Some(frame);
                }
            }
            if moving.iter().all(|n| n.expires_after.is_some()) {
                break;
            }
        }
    }

    if moving.iter().any(|n| n.expires_after.is_none()) {
        return Err(ReplayError::Input(
            "unterminated native green chart trajectory",
        ));
    }
    let mut nodes = vec![];
    let mut bridges = vec![];
    let mut hidden = vec![];
    for (s, n) in chain.nodes.iter().zip(moving) {
        if let Some(index) = s.note_index {
            let window = GreenFrameProfile::compile(
                &table,
                n.first_frame
                    .ok_or(ReplayError::Input("unlaunched green note"))?,
                n.ys,
            )?
            .window(s.role)?;
            if !nodes.is_empty() {
                bridges.push(HiddenBridge {
                    hidden: std::mem::take(&mut hidden),
                    successor: s.role,
                });
            }
            nodes.push(GreenChainNode {
                note_index: index,
                window,
                minimum_gap_from_previous: if nodes.is_empty() { 0 } else { 1 },
            });
        } else {
            hidden.push(HiddenGate {
                first_ready_frame: n.first_middle,
                expires_after_frame: n.expires_after,
            });
        }
    }
    Ok(SpeedVariant { nodes, bridges })
}

fn prepare_speed_variants(
    chains: &[NativeGreenChain],
    basic_bpm: f32,
    batches: &[NativeBatch],
    geometry: GreenGeometry,
) -> Result<Vec<Vec<SpeedVariant>>, ReplayError> {
    let count = super::speed::StandardSpeed::all().len();
    let mut all: Vec<Vec<SpeedVariant>> =
        chains.iter().map(|_| Vec::with_capacity(count)).collect();
    for speed in super::speed::StandardSpeed::all() {
        let g = GreenGeometry {
            specific_speed: speed.value(),
            ..geometry
        };
        let table = judge_table(g, 0)?;
        let crossings = progress_crossings(g, &table)?;
        let mut clock = SharedTimeline::new(speed.value(), basic_bpm, batches)?;
        for (chain, out) in chains.iter().zip(&mut all) {
            out.push(prepare_with_table(chain, g, &crossings, &mut clock)?);
        }
    }
    Ok(all)
}

/// Prepare the native windows at one supplied speed using one shared chart
/// clock. This exposes the unmerged windows for development audits; it does not
/// imply that the supplied speed represents the other standard settings.
pub fn prepare_chart_speed_variants(
    chains: &[NativeGreenChain],
    basic_bpm: f32,
    batches: &[NativeBatch],
    geometry: GreenGeometry,
) -> Result<Vec<SpeedVariant>, ReplayError> {
    for chain in chains {
        validate(chain, batches)?;
    }
    if geometry.button_world_y != geometry.button_local_y {
        return Err(ReplayError::Input(
            "native green trajectory needs a local Y transform",
        ));
    }
    let table = judge_table(geometry, 0)?;
    let crossings = progress_crossings(geometry, &table)?;
    let mut clock = SharedTimeline::new(geometry.specific_speed, basic_bpm, batches)?;
    chains
        .iter()
        .map(|chain| prepare_with_table(chain, geometry, &crossings, &mut clock))
        .collect()
}

pub fn prepare_chart_standard_speeds(
    chains: &[NativeGreenChain],
    basic_bpm: f32,
    batches: &[NativeBatch],
    geometry: GreenGeometry,
) -> Result<Vec<super::speed::AllSpeedChain>, ReplayError> {
    for chain in chains {
        validate(chain, batches)?;
    }
    if geometry.button_world_y != geometry.button_local_y {
        return Err(ReplayError::Input(
            "native green trajectory needs a local Y transform",
        ));
    }
    let all = prepare_speed_variants(chains, basic_bpm, batches, geometry)?;
    all.into_iter()
        .map(|variants| {
            let mut it = variants.into_iter();
            super::speed::compile_standard_speeds(|_| {
                it.next()
                    .ok_or(ReplayError::Input("missing standard speed variant"))
            })
        })
        .collect()
}

/// Install all chains as one transaction. The caller supplies a feasible baseline
/// on the same native frame origin; a failed baseline never changes the chart.
/// This feeds the existing replay, skill-window optimizer and team scorer.
pub fn install_prepared_chart(
    prepared: &[super::speed::AllSpeedChain],
    input: &mut crate::hit_timing::ReplayInput,
) -> Result<(), ReplayError> {
    let mut trial = input.clone();
    let mut seen = std::collections::HashSet::new();
    for chain in prepared {
        if chain.evaluated_settings != 1101 {
            return Err(ReplayError::Input("incomplete standard speed coverage"));
        }
        for &(i, w) in &chain.compiled.windows {
            if !seen.insert(i) {
                return Err(ReplayError::Input(
                    "green scoring node occurs in multiple chains",
                ));
            }
            trial
                .notes
                .get_mut(i)
                .ok_or(ReplayError::Input("native green node is absent from chart"))?
                .window = w;
        }
        for c in &chain.compiled.constraints {
            if !trial.constraints.contains(c) {
                trial.constraints.push(c.clone());
            }
        }
    }
    crate::hit_timing::replay(&trial)?;
    *input = trial;
    Ok(())
}

/// Build a feasible all-Perfect green baseline explicitly. Ordinary-note frames
/// and their ordering are unchanged. Earliest predecessor placement minimizes
/// the monotone hidden-arrival bound, so a later choice cannot rescue a failure.
/// This is preparation, not a skill-aware scoring heuristic or a score optimum.
pub fn install_with_perfect_baseline(
    prepared: &[super::speed::AllSpeedChain],
    input: &mut crate::hit_timing::ReplayInput,
) -> Result<(), ReplayError> {
    install_with_minimum_baseline(prepared, input, 3)
}

/// Prefer the skill-independent Perfect baseline, then Great. If neither exists,
/// retain a supplied feasible plan (including active Good/Bad correction). The
/// baseline policy never removes lower raw grades from the actual search domain.
pub fn install_with_feasible_baseline(
    prepared: &[super::speed::AllSpeedChain],
    input: &mut crate::hit_timing::ReplayInput,
) -> Result<(), ReplayError> {
    if install_with_minimum_baseline(prepared, input, 3).is_ok()
        || install_with_minimum_baseline(prepared, input, 2).is_ok()
    {
        return Ok(());
    }
    install_prepared_chart(prepared, input)
}
fn install_with_minimum_baseline(
    prepared: &[super::speed::AllSpeedChain],
    input: &mut crate::hit_timing::ReplayInput,
    grade: usize,
) -> Result<(), ReplayError> {
    use crate::hit_timing::NoteWindow;
    let mut trial = input.clone();
    for chain in prepared {
        if chain.compiled.constraints.len() + 1 != chain.compiled.windows.len() {
            return Err(ReplayError::Input("prepared green chain topology differs"));
        }
        for (j, &(i, w)) in chain.compiled.windows.iter().enumerate() {
            let NoteWindow::NativeGreen {
                first_frames,
                last_frame,
            } = w
            else {
                return Err(ReplayError::Input("prepared green window has wrong type"));
            };
            let mut frame = first_frames[grade];
            if j > 0 {
                let c = &chain.compiled.constraints[j - 1];
                let before = chain.compiled.windows[j - 1].0;
                if c.before != before || c.after != i {
                    return Err(ReplayError::Input("prepared green chain topology differs"));
                }
                frame = frame.max(c.required_after(trial.hit_frames[before]).ok_or(
                    ReplayError::Input("no green baseline at requested minimum grade"),
                )?);
            }
            if frame > last_frame {
                return Err(ReplayError::Input(
                    "no green baseline at requested minimum grade",
                ));
            }
            *trial
                .hit_frames
                .get_mut(i)
                .ok_or(ReplayError::Input("native green node is absent from chart"))? = frame;
        }
    }
    install_prepared_chart(prepared, &mut trial)?;
    *input = trial;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{launch::NativePosition, speed::compile_standard_speeds};
    use super::*;
    fn fixture() -> (NativeGreenChain, Vec<NativeBatch>, GreenGeometry) {
        let ticks = [384, 432, 480, 528, 576];
        let batches = ticks
            .iter()
            .map(|&x| NativeBatch {
                position: NativePosition {
                    bar: x / 192,
                    numerator: x % 192,
                    denominator: 192,
                },
                empty: false,
                bpm: None,
            })
            .collect();
        let chain = NativeGreenChain {
            root_batch: 0,
            nodes: ticks
                .iter()
                .enumerate()
                .map(|(i, &t)| NativeGreenNode {
                    absolute_position: t,
                    note_bpm: 120.,
                    note_index: if i == 2 { None } else { Some(i) },
                    role: if i == 0 {
                        GreenRole::Head
                    } else if i == 4 {
                        GreenRole::HeldArrowEnd
                    } else {
                        GreenRole::HeldMiddle
                    },
                })
                .collect(),
        };
        // Native setupLauncherPosition derives this from the first button.
        let button = -3.45f32;
        let apex = button + (-6.6f32) * f32::from_bits(0xbfac0636);
        let start = button + (1.0f32 - 0.05f32) * (apex - button);
        (
            chain,
            batches,
            GreenGeometry {
                specific_speed: 10.,
                start_y: start,
                button_world_y: button,
                button_local_y: button,
            },
        )
    }
    #[test]
    fn launch_to_projection_scans_all_standard_speeds() {
        let (chain, batches, g) = fixture();
        for s in super::super::speed::StandardSpeed::all() {
            let g = GreenGeometry {
                specific_speed: s.value(),
                ..g
            };
            let mut clock = SharedTimeline::new(s.value(), 120., &batches).unwrap();
            let fast = prepare_with_clock(&chain, &batches, g, &mut clock).unwrap();
            let slow = prepare_reference_with_clock(&chain, &batches, g, &mut clock).unwrap();
            for (a, b) in fast.nodes.iter().zip(&slow.nodes) {
                assert_eq!(
                    serde_json::to_value(a.window).unwrap(),
                    serde_json::to_value(b.window).unwrap(),
                    "speed {}",
                    s.hundredths()
                );
            }
            for (a, b) in fast.bridges.iter().zip(&slow.bridges) {
                for (a, b) in a.hidden.iter().zip(&b.hidden) {
                    assert_eq!(a.first_ready_frame, b.first_ready_frame);
                    assert_eq!(a.expires_after_frame, b.expires_after_frame);
                }
            }
        }
        let p = compile_standard_speeds(|s| {
            prepare_chain_variant(
                &chain,
                120.,
                &batches,
                GreenGeometry {
                    specific_speed: s.value(),
                    ..g
                },
            )
        })
        .unwrap();
        assert_eq!(p.evaluated_settings, 1101);
        assert_eq!(p.compiled.windows.len(), 4);
        assert_eq!(p.compiled.constraints.len(), 3);
        assert!(!p.representative_settings.is_empty());
        use crate::hit_timing::{replay, search, NoteWindow, ReplayInput, ReplayNote};
        let mut input = ReplayInput {
            power: 300040.75,
            play_level: 27,
            fps: 120,
            notes: (0..5)
                .map(|i| ReplayNote {
                    time: (4. + i as f64 * 0.5),
                    trigger: None,
                    window: NoteWindow::Standard,
                })
                .collect(),
            hit_frames: vec![480, 540, 600, 660, 720],
            hit_order: vec![],
            skills: vec![],
            initial_combo: 49,
            medley_combo: false,
            fever_frames: None,
            constraints: vec![],
            native_time_routing: None,
        };
        for &(i, w) in &p.compiled.windows {
            let NoteWindow::NativeGreen { first_frames, .. } = w else {
                unreachable!()
            };
            input.hit_frames[i] = first_frames[3];
        }
        install_prepared_chart(&[p], &mut input).unwrap();
        assert_eq!(input.notes.len(), 5);
        assert_eq!(input.constraints.len(), 3);
        let found = search::optimize(&input, search::SearchOptions::default()).unwrap();
        assert!(found.complete);
        assert!(replay(&input).is_ok());
        for c in &input.constraints {
            assert!(c.allows_frames(
                found.result.hit_frames[c.before],
                found.result.hit_frames[c.after]
            ));
        }
    }
    #[test]
    fn hidden_nodes_do_not_add_scoring_entries() {
        let (chain, batches, g) = fixture();
        let v = prepare_chain_variant(&chain, 120., &batches, g).unwrap();
        assert_eq!(
            v.nodes.iter().map(|n| n.note_index).collect::<Vec<_>>(),
            vec![0, 1, 3, 4]
        );
        assert_eq!(v.bridges[1].hidden.len(), 1);
        assert!(v.bridges[1].hidden[0].first_ready_frame.is_some());
    }
    #[test]
    fn foreign_root_or_local_transform_is_rejected() {
        let (mut chain, batches, g) = fixture();
        chain.root_batch = 1;
        assert!(prepare_chain_variant(&chain, 120., &batches, g).is_err());
        chain.root_batch = 0;
        assert!(prepare_chain_variant(
            &chain,
            120.,
            &batches,
            GreenGeometry {
                button_local_y: 0.,
                ..g
            }
        )
        .is_err());
    }
    #[test]
    fn preparation_baseline_does_not_require_all_perfect_or_strip_correction_grades() {
        use super::super::{
            compact::{compile_chain, GreenChainNode},
            speed::AllSpeedChain,
        };
        use crate::hit_timing::{
            replay, JudgementCorrection, NoteWindow, ReplayInput, ReplayNote, ReplaySkill,
            ScoreEffect,
        };
        let mut input = ReplayInput {
            power: 300040.75,
            play_level: 27,
            fps: 120,
            notes: [0., 1., 1.02]
                .into_iter()
                .enumerate()
                .map(|(i, time)| ReplayNote {
                    time,
                    trigger: (i == 0).then_some(0),
                    window: NoteWindow::Standard,
                })
                .collect(),
            hit_frames: vec![0, 129, 130],
            hit_order: vec![],
            skills: vec![ReplaySkill {
                duration: 3.,
                score: ScoreEffect::Plain { percent: 100. },
                correction: None,
            }],
            initial_combo: 0,
            medley_combo: false,
            fever_frames: None,
            constraints: vec![],
            native_time_routing: None,
        };
        let prepare = |great: bool| AllSpeedChain {
            evaluated_settings: 1101,
            representative_settings: vec![100],
            compiled: compile_chain(
                &(1..=2)
                    .map(|i| GreenChainNode {
                        note_index: i,
                        window: NoteWindow::NativeGreen {
                            first_frames: if great {
                                [119 + i as u32, 119 + i as u32, 119 + i as u32, 126]
                            } else {
                                [119 + i as u32, 126, 126, 126]
                            },
                            last_frame: 125,
                        },
                        minimum_gap_from_previous: if i == 1 { 0 } else { 1 },
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        };
        let good = vec![prepare(true)];
        assert!(install_with_perfect_baseline(&good, &mut input).is_err());
        install_with_feasible_baseline(&good, &mut input).unwrap();
        assert_eq!(input.hit_frames, vec![0, 120, 121]);
        assert_eq!(
            replay(&input).unwrap().trace[1].raw,
            crate::scoring::Judgement::Great
        );
        let bad = vec![prepare(false)];
        let before = serde_json::to_value(&input).unwrap();
        assert!(install_with_feasible_baseline(&bad, &mut input).is_err());
        assert_eq!(before, serde_json::to_value(&input).unwrap());
        input.skills[0].correction = Some(JudgementCorrection::Bad);
        install_with_feasible_baseline(&bad, &mut input).unwrap();
        let result = replay(&input).unwrap();
        assert_eq!(result.trace[1].raw, crate::scoring::Judgement::Bad);
        assert_eq!(
            result.trace[1].judgement,
            crate::scoring::Judgement::Perfect
        );
    }
}
