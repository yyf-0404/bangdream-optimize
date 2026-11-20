//! Complete ordinary-note placement for fixed skill request frames, within the
//! declared 120 FPS replay domains and native routing model. The geometry is
//! power independent; each query maximizes the actual native integer score.
use super::*;
use crate::hit_timing::{clock::Timeline, search::SearchError, ScoreEffect};
use geometry::Factor;
use machine::ActionCosts;

pub struct FixedTimeline {
    prepared: Prepared,
    // Index 0 is the outside-window machine, followed by the real skill slots.
    machines: Vec<Machine>,
}

pub struct FixedTimelineGeometry<'a> {
    owner: &'a FixedTimeline,
    graph: Graph,
    stats: WindowStats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedTimelineResult {
    pub score: u64,
    pub hit_frames: Vec<u32>,
    pub hit_order: Vec<usize>,
    pub stats: WindowStats,
}

impl FixedTimeline {
    pub fn new(input: &ReplayInput) -> Result<Self, ReplayError> {
        // Validate structure, original domains, native routing and the baseline.
        // Later schedules may make old ordinary positions invalid; they are
        // solved afresh instead of requiring that old plan to replay first.
        replay(input)?;
        let outside = ReplaySkill {
            duration: 1.,
            score: ScoreEffect::Plain { percent: 0. },
            correction: None,
        };
        let machines = std::iter::once(&outside)
            .chain(&input.skills)
            .map(|s| Machine::new(s, input.notes.len()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            prepared: Prepared::new(input)?,
            machines,
        })
    }

    /// Pin all triggers and optimize every ordinary note. None means that this
    /// fixed schedule has no legal full-chart placement, never a budget cutoff.
    pub fn prepare(
        &self,
        frames: &[u32],
    ) -> Result<Option<FixedTimelineGeometry<'_>>, ReplayError> {
        self.prepare_ordered(
            frames,
            &vec![true; self.prepared.input.notes.len()],
            true,
            true,
        )
    }

    pub(crate) fn prepare_selected(
        &self,
        frames: &[u32],
        movable: &[bool],
        collapse: bool,
    ) -> Result<Option<FixedTimelineGeometry<'_>>, ReplayError> {
        // This helper belongs to the explicitly incomplete legacy local search.
        self.prepare_ordered(frames, movable, collapse, false)
    }

    fn prepare_ordered(
        &self,
        frames: &[u32],
        movable: &[bool],
        collapse: bool,
        optimize_touch_order: bool,
    ) -> Result<Option<FixedTimelineGeometry<'_>>, ReplayError> {
        let input = &self.prepared.input;
        if movable.len() != input.notes.len() {
            return Err(ReplayError::Input("invalid ordinary placement mask"));
        }
        let key = Timeline::key_with_touch_order(input, frames, optimize_touch_order)?;
        let timeline = Timeline::from_requests(&input.skills, &key)?;
        let Some(row) = self.prepared.timeline_row(&timeline, frames, movable) else {
            return Ok(None);
        };
        let mut stats = WindowStats {
            prepared_frames: self.prepared.frame_count,
            prepared_intervals: self.prepared.interval_count,
            graph_builds: 1,
            ..WindowStats::default()
        };
        // Cache capacity is bounded; exploration is not. No truncated graph or
        // state table is accepted as the complete ordinary-note optimum.
        let collapse = collapse && self.machines.iter().all(|m| m.monotone);
        let graph = self
            .prepared
            .compile(
                &row,
                collapse,
                WindowOptions {
                    optimize_touch_order,
                    ..exact_options()
                },
                &mut stats,
            )
            .expect("fixed timeline has no graph/state exploration budget");
        Ok(Some(FixedTimelineGeometry {
            owner: self,
            graph,
            stats,
        }))
    }
}

fn exact_options() -> WindowOptions {
    WindowOptions {
        max_states: usize::MAX,
        max_graph_edges: usize::MAX,
        max_transitions: u64::MAX,
        ..WindowOptions::default()
    }
}

impl FixedTimelineGeometry<'_> {
    /// Re-evaluate the whole DP at this power. Reusing a plan that won at a
    /// reference power would lose optima due to the native floating-point casts.
    pub fn maximum(&self, power: ScoringPower) -> Result<Option<FixedTimelineResult>, SearchError> {
        let input = &self.owner.prepared.input;
        let mut costs = TimelineCosts::new(input, power, &self.owner.machines)?;
        let mut stats = self.stats.clone();
        let answer = solve::evaluate_ordered(
            &self.graph,
            &mut costs,
            WindowObjective::Native,
            exact_options(),
            &mut stats,
        )?
        .expect("fixed timeline has no value exploration budget");
        stats.prefix_tables += costs.costs.iter().map(Costs::prefix_count).sum::<usize>();
        let Some((value, hit_frames)) = answer else {
            return Ok(None);
        };
        let mut witness = input.clone();
        witness.power = power.value();
        witness.hit_frames = hit_frames.frames.clone();
        witness.hit_order = hit_frames.order.clone();
        let actual = replay(&witness)?;
        if value.score != actual.total {
            return Err(SearchError::Mismatch {
                cached: value.score,
                replayed: actual.total,
            });
        }
        Ok(Some(FixedTimelineResult {
            score: value.score,
            hit_frames: hit_frames.frames,
            hit_order: hit_frames.order,
            stats,
        }))
    }
}

struct TimelineCosts<'a> {
    costs: Vec<Costs<'a>>,
    offsets: Vec<usize>,
}
impl<'a> TimelineCosts<'a> {
    fn new(
        input: &'a ReplayInput,
        power: ScoringPower,
        machines: &'a [Machine],
    ) -> Result<Self, ReplayError> {
        let mut offsets = vec![0];
        for m in machines {
            offsets.push(offsets.last().unwrap() + m.values.len());
        }
        Ok(Self {
            costs: machines
                .iter()
                .map(|m| Costs::new(input, power, m))
                .collect::<Result<_, _>>()?,
            offsets,
        })
    }
    fn local(&self, f: Factor, state: usize) -> (usize, usize) {
        let slot = usize::from(f.slot);
        // FIFO skills never return to an earlier slot. State from a completed
        // skill is dead; entering a later skill starts its own clean machine.
        let local = if (self.offsets[slot]..self.offsets[slot + 1]).contains(&state) {
            state - self.offsets[slot]
        } else {
            0
        };
        (slot, local)
    }
}
impl ActionCosts for TimelineCosts<'_> {
    fn hit(&mut self, f: Factor, state: usize) -> Result<(usize, Value), ReplayError> {
        let (slot, state) = self.local(f, state);
        let (state, value) = self.costs[slot].hit(f, state)?;
        Ok((self.offsets[slot] + state, value))
    }
    fn run(
        &mut self,
        f: Factor,
        state: usize,
        count: usize,
    ) -> Result<(usize, Value), ReplayError> {
        let (slot, state) = self.local(f, state);
        let (state, value) = self.costs[slot].run(f, state, count)?;
        Ok((self.offsets[slot] + state, value))
    }
}

#[cfg(test)]
mod tests;
