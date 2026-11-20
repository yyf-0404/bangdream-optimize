//! Intern the exact FIFO clock separately from the finite score progression.
//! Ordinary hits use a clock-local progress ID table, without hashing queues.
//! The only clock memo is one boundary result per actual clock in this query.
use super::*;
mod aliases;
mod uniform;
mod intervals;

pub(super) fn enabled() -> bool {
    // Explicit opt-in: canonical clocks did not reduce the measured state count.
    std::env::var("BANDORI_TIMING_DISABLE_CLOCK_STATES").as_deref() == Ok("0")
}
pub(super) fn value_enabled() -> bool {
    std::env::var("BANDORI_TIMING_DISABLE_INTERNED_CLOCKS").as_deref() != Ok("1")
}

#[derive(Clone, Default, Hash, PartialEq, Eq)]
struct Clock {
    ready: u32,
    windows: Rc<Vec<Window>>,
}

#[derive(Clone, Copy)]
struct Advanced {
    clock: usize,
    reset: bool,
    active: Option<usize>,
}

struct Entry {
    clock: Clock,
    dominance: DominanceProof,
    uniform: Option<uniform::Proof>,
    boundaries: Vec<u32>,
    /// Sparse prefix of reachable progress IDs; absent IDs are never expanded.
    states: Vec<usize>,
    at: Option<(u32, Advanced)>,
}

#[derive(Clone, Copy)]
struct Label {
    clock: usize,
    progress: usize,
}

pub(super) struct ClockCosts<'a> {
    base: JointCosts<'a>,
    clocks: Vec<Entry>,
    ids: HashMap<Clock, usize>,
    states: Vec<Label>,
    aliases: Vec<u8>,
    queued_uniform: bool,
    dominance_proofs: bool,
}

/// All conditions here depend only on this immutable clock and the fixed
/// machine tables. Score/progress remain query-local inputs to the callbacks.
#[derive(Clone, Copy, Default)]
struct DominanceProof { end: u32, slot: u8, rank: u8, plane: u8, window: bool }
impl DominanceProof {
    fn order(strength: Option<usize>) -> u8 {
        match strength { Some(0)=>1, Some(1)=>2, _=>0 }
    }
    fn strength(order:u8,progress:usize)->Option<usize> {
        match order {1=>Some(progress),2=>1usize.checked_sub(progress),_=>None}
    }
    fn new(base:&JointCosts<'_>,clock:&Clock)->Self {
        let Some(window)=clock.windows.first() else {return Self::default();};
        let slot=usize::from(window.slot);
        let machine=&base.model.machines[slot];
        let rank=if base.progress_dominance {Self::order(machine.progress_strength(
            &base.model.input.skills[slot].score,0,base.continuous_dominance))} else {0};
        let active_single=clock.ready==0 && clock.windows.len()==1 && window.start==0;
        let plane=if active_single {Self::order(base.plane_strength(slot,0))} else {0};
        Self {end:window.end,slot:window.slot,rank,plane,
            window:active_single && base.window_dominance && machine.baseline_dominance}
    }
}

impl<'a> ClockCosts<'a> {
    pub(super) fn new(base: JointCosts<'a>) -> Self {
        // Real stress samples retained the same state count. Keep this exact
        // alternative opt-in until a useful reduction is measured.
        let enabled = base.model.controls.enabled(Switch::ClockAliases);
        Self::new_controlled(base, enabled)
    }

    pub(super) fn new_controlled(base: JointCosts<'a>, enabled: bool) -> Self {
        let clock = Clock::default();
        let aliases = aliases::classes(base.model, enabled);
        let queued_uniform = base.model.controls.enabled(Switch::QueuedUniform);
        let dominance_proofs=base.model.controls.enabled(Switch::NativeKernelFastpaths);
        let dominance=if dominance_proofs {DominanceProof::new(&base,&clock)} else {DominanceProof::default()};
        let boundaries=if base.operators.frame_cells() {intervals::boundaries(clock.ready,&clock.windows)} else {vec![]};
        Self {
            base,
            clocks: vec![Entry { uniform: uniform::prove(clock.ready,&clock.windows,queued_uniform),
                boundaries,dominance,clock: clock.clone(), states: vec![0], at: None }],
            ids: HashMap::from([(clock, 0)]),
            states: vec![Label { clock: 0, progress: 0 }],
            aliases,
            queued_uniform,
            dominance_proofs,
        }
    }

    #[cfg(test)]
    pub(super) fn alias_classes(&self) -> &[u8] { &self.aliases }

    #[cfg(test)]
    pub(super) fn set_dominance_proofs(&mut self,enabled:bool) {
        self.dominance_proofs=enabled;
        if enabled {
            for entry in &mut self.clocks {entry.dominance=DominanceProof::new(&self.base,&entry.clock);}
        }
    }

    #[cfg(test)]
    pub(super) fn check_dominance_proofs(&mut self) {
        assert_eq!(std::mem::size_of::<DominanceProof>(),8);
        for clock in 0..self.clocks.len() {
            for progress in [0,1,2,u32::MAX as usize,usize::MAX] {
                let state=self.states.len();
                self.states.push(Label {clock,progress});
                self.dominance_proofs=true;
                let a=(self.dominance_rank(state),self.dominance_rank_extra(state),self.dominance_plane(state));
                self.dominance_proofs=false;
                let b=(self.dominance_rank(state),self.dominance_rank_extra(state),self.dominance_plane(state));
                assert_eq!(a,b,"clock={clock} progress={progress}");
                self.states.pop();
            }
        }
        self.dominance_proofs=true;
    }

    #[cfg(test)]
    pub(super) fn set_queued_uniform(&mut self,enabled:bool) {
        self.queued_uniform=enabled;
        for entry in &mut self.clocks {
            entry.uniform=uniform::prove(entry.clock.ready,&entry.clock.windows,enabled);
        }
    }

    fn intern_clock(&mut self, clock: Clock) -> usize {
        if let Some(&id) = self.ids.get(&clock) { return id; }
        let id = self.clocks.len();
        self.ids.insert(clock.clone(), id);
        let boundaries=if self.base.operators.frame_cells() {intervals::boundaries(clock.ready,&clock.windows)} else {vec![]};
        let dominance=if self.dominance_proofs {DominanceProof::new(&self.base,&clock)} else {DominanceProof::default()};
        self.clocks.push(Entry { uniform: uniform::prove(clock.ready,&clock.windows,self.queued_uniform),
            boundaries,dominance,clock, states: vec![], at: None });
        id
    }

    fn intern_state(&mut self, clock: usize, progress: usize) -> usize {
        let ids = &mut self.clocks[clock].states;
        if ids.len() <= progress { ids.resize(progress + 1, usize::MAX); }
        if ids[progress] == usize::MAX {
            ids[progress] = self.states.len();
            self.states.push(Label { clock, progress });
        }
        ids[progress]
    }

    fn advance(&mut self, id: usize, frame: u32) -> Advanced {
        if let Some((old_frame, result)) = self.clocks[id].at {
            if old_frame == frame { return result; }
        }
        let old = &self.clocks[id].clock;
        let expired = old.windows.partition_point(|w| w.end < frame);
        let ready = if old.ready <= frame { 0 } else { old.ready };
        let normalize = self.base.bucket_clocks
            && old.windows[expired..].iter().any(|w| w.start != 0 && w.start < frame);
        let clock = if expired == 0 && ready == old.ready && !normalize {
            id
        } else {
            let mut windows = if expired == 0 { Rc::clone(&old.windows) }
                else { Rc::new(old.windows[expired..].to_vec()) };
            if normalize {
                for window in Rc::make_mut(&mut windows) {
                    if window.start < frame { window.start = 0; }
                }
            }
            self.intern_clock(Clock { ready, windows })
        };
        let active = self.clocks[clock].clock.windows.first()
            .filter(|w| w.start < frame).map(|w| usize::from(w.slot));
        let result = Advanced { clock, reset: expired != 0, active };
        self.clocks[id].at = Some((frame, result));
        result
    }

    fn outside_step(&mut self, action: &Action, state: usize, score: u64)
        -> Result<Option<(usize, Value)>, ReplayError> {
        let label = self.states[state];
        let advanced = self.advance(label.clock, action.hits.last().unwrap().1);
        debug_assert!(advanced.active.is_none());
        let progress = if advanced.reset { 0 } else { label.progress };
        Ok(Some((self.intern_state(advanced.clock, progress), Value { score, coefficient: 0. })))
    }

    fn step(&mut self, action: &Action, state: usize, prepared: Option<PreparedAction>)
        -> Result<Option<(usize, Value)>, ReplayError> {
        let (note, frame) = action.hits[0];
        let label = self.states[state];
        let advanced = self.advance(label.clock, frame);
        let progress = if advanced.reset { 0 } else { label.progress };
        let raw = match prepared {
            Some(action) => action.raw,
            None => self.base.model.input.notes[note].raw_judgement(frame)?,
        };
        let Some((progress, value)) = self.base.hit_value(
            action, raw, advanced.active, progress, prepared,
        )? else { return Ok(None); };
        let mut clock = advanced.clock;
        if let Some(slot) = self.base.model.input.notes[note].trigger {
            let old = &self.clocks[clock].clock;
            let start = frame.max(old.ready);
            let end = start + self.base.model.playing[slot];
            let mut windows = Rc::clone(&old.windows);
            let ready = end + self.base.cooling + 1;
            let (mut window, ready) = if self.base.bucket_clocks {
                self.base.model.prepared_actions.buckets.window(slot, start, end, ready)
            } else { (Window { start, end, slot: slot as u8 }, ready) };
            // Quantize against this trigger's actual future requests first.
            // Only its scoring-machine label may be shared with another slot.
            window.slot = self.aliases[slot];
            Rc::make_mut(&mut windows).push(window);
            clock = self.intern_clock(Clock { ready, windows });
        }
        Ok(Some((self.intern_state(clock, progress),
            Value { score: u64::from(value), coefficient: 0. })))
    }
}

impl ActionCosts for ClockCosts<'_> {
    fn value_controls(&self) -> Controls { self.base.value_controls() }
    fn supports_native_blocks(&self) -> bool { self.base.supports_native_blocks() }
    fn native_block_operator(&self, index: usize) -> Option<(u32, u8, usize)> {
        self.base.native_block_operator(index)
    }
    fn native_uniform_state(&self, state: usize) -> Option<machine::NativeUniformState> {
        let label = self.states[state];
        self.clocks[label.clock].uniform.map(|proof|machine::NativeUniformState {
            class:(proof.active.unwrap_or(usize::MAX),label.progress),
            first:proof.first,last:proof.last,
        })
    }
    fn native_frame_interval(&self,state:usize,frame:u32) -> Option<(u32,u32)> {
        self.base.operators.frame_cells().then(||intervals::cell(
            &self.clocks[self.states[state].clock].boundaries,frame))
    }
    fn finish_stats(&self, stats: &mut WindowStats) {
        self.base.operators.stats(stats);
        if let Some(blocks) = stats.native_blocks.as_mut() {
            blocks.clock_aliases = self.aliases.iter().enumerate()
                .filter(|&(slot, &class)| slot != usize::from(class)).count();
            blocks.clock_queues = self.clocks.len();
            blocks.clock_states = self.states.len();
        }
    }
    fn supports_temporal_intervals(&self) -> bool { true }

    fn stable_interval(&self, state: usize, first: u32, last: u32) -> bool {
        let clock = &self.clocks[self.states[state].clock].clock;
        let changes = |boundary: u64| u64::from(first) < boundary && boundary <= u64::from(last);
        !changes(u64::from(clock.ready)) && clock.windows.iter().all(|w| {
            !changes(u64::from(w.start) + 1) && !changes(u64::from(w.end) + 1)
        })
    }

    fn supports_dominance(&self) -> bool { self.base.supports_dominance() }
    fn dominance_plane(&self,state:usize) -> Option<(usize,usize,usize)> {
        let label=self.states[state];
        if self.dominance_proofs {
            let proof=self.clocks[label.clock].dominance;
            return Some((usize::from(proof.slot),
                DominanceProof::strength(proof.plane,label.progress)?,proof.end as usize));
        }
        let clock=&self.clocks[label.clock].clock;
        if clock.ready!=0 || clock.windows.len()!=1 {return None;}
        let window=clock.windows.first()?;
        let slot=usize::from(window.slot);
        if window.start!=0 {return None;}
        let strength=self.base.plane_strength(slot,label.progress)?;
        Some((slot,strength,window.end as usize))
    }
    fn dominance_rank_extra(&self, state: usize) -> Option<(usize, u32, usize)> {
        if self.dominance_proofs {
            let label=self.states[state];
            let proof=self.clocks[label.clock].dominance;
            return proof.window.then_some((usize::from(proof.slot),label.progress as u32,proof.end as usize));
        }
        if !self.base.window_dominance { return None; }
        let label = self.states[state];
        let clock = &self.clocks[label.clock].clock;
        if clock.ready != 0 || clock.windows.len() != 1 { return None; }
        let window = clock.windows.first()?;
        if window.start != 0 || !self.base.model.machines[usize::from(window.slot)].baseline_dominance {
            return None;
        }
        Some((usize::from(window.slot), label.progress as u32, window.end as usize))
    }

    fn dominance_rank(&self, state: usize) -> Option<(usize, u32, usize)> {
        if self.dominance_proofs {
            let label=self.states[state];
            let proof=self.clocks[label.clock].dominance;
            return Some((label.clock,0,DominanceProof::strength(proof.rank,label.progress)?));
        }
        if !self.base.progress_dominance { return None; }
        let label = self.states[state];
        let slot = usize::from(self.clocks[label.clock].clock.windows.first()?.slot);
        let strength=self.base.model.machines[slot].progress_strength(
            &self.base.model.input.skills[slot].score,label.progress,self.base.continuous_dominance)?;
        // Canonical clock IDs compare complete queue contents and ready times.
        Some((label.clock, 0, strength))
    }

    fn action(&mut self, action: &Action, state: usize)
        -> Result<Option<(usize, Value)>, ReplayError> {
        if matches!(action.hits, geometry::ActionHits::Run(_)) {
            let score = JointCosts::outside_score(self.base.model, self.base.scorer, action)?;
            self.outside_step(action, state, score)
        } else { self.step(action, state, None) }
    }

    fn action_at(&mut self, index: usize, action: &Action, state: usize)
        -> Result<Option<(usize, Value)>, ReplayError> {
        if matches!(action.hits, geometry::ActionHits::Run(_)) {
            return self.outside_step(action, state, self.base.outside_scores[&index]?);
        }
        let prepared = self.base.score_table.as_ref()
            .map(|_| self.base.model.prepared_actions.actions[index]);
        let (note, frame) = action.hits[0];
        let operator = prepared.filter(|_| self.base.operators.enabled()
            && self.base.model.input.notes[note].trigger.is_none());
        let frame_key=operators::Operators::frame_key(frame,
            operator.and_then(|_|self.native_frame_interval(state,frame)));
        if let Some(prepared) = operator {
            if let Some(result) = self.base.operators.get(frame_key, state, prepared) { return Ok(result); }
        }
        let result = self.step(action, state, prepared)?;
        if let Some(prepared) = operator { self.base.operators.put(frame_key, state, prepared, result); }
        Ok(result)
    }

    fn hit(&mut self, _: Factor, _: usize) -> Result<(usize, Value), ReplayError> { unreachable!() }
    fn run(&mut self, _: Factor, _: usize, _: usize) -> Result<(usize, Value), ReplayError> { unreachable!() }
}
