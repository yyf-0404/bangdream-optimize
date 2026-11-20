//! Compare each node's final labels with the carried, previously closed labels.
//! A certified edge is redundant only while its prefix has not improved and
//! the actual FIFO clock has no intervening boundary. New labels still expand.
use super::*;
use geometry::{incremental::Proof, TouchDag};

#[derive(Default)]
pub(super) struct Scratch {
    inputs: Vec<usize>,
    flags: Vec<u8>,
    stable: Vec<(usize, bool)>,
    epoch: usize,
}

impl Scratch {
    pub(super) fn begin<'a>(&mut self, dag: &'a TouchDag, count: usize,
        previous_layer: Option<usize>, costs: &impl machine::ActionCosts,
        objective: WindowObjective, options: WindowOptions) -> Option<&'a Proof> {
        if !options.incremental_touch || objective != WindowObjective::Native
            || !costs.supports_temporal_intervals() { return None; }
        let proof = dag.incremental.as_deref()
            .filter(|p| previous_layer == Some(p.previous_layer))?;
        self.inputs.clear();
        self.inputs.resize(count, usize::MAX);
        for (input, &node) in dag.inputs.iter().enumerate() { self.inputs[node] = input; }
        self.epoch += 1;
        Some(proof)
    }

    pub(super) fn node(&mut self, node: usize, entries: &[(usize, Point)],
        carried: &[Vec<(usize, Point)>], proof: &Proof, costs: &impl machine::ActionCosts,
        objective: WindowObjective, stats: &mut WindowStats) {
        self.flags.clear();
        let input = self.inputs[node];
        debug_assert_ne!(input, usize::MAX);
        let old = &carried[input];
        let mut previous = 0;
        for &(state, point) in entries {
            while old.get(previous).is_some_and(|&(id, _)| id < state) { previous += 1; }
            let unchanged = old.get(previous).is_some_and(|&(id, old)| {
                id == state && costs.compare(old.value, point.value, objective).is_ge()
            });
            let flag = if unchanged {
                if self.stable.len() <= state { self.stable.resize(state + 1, (0, false)); }
                if self.stable[state].0 != self.epoch {
                    stats.incremental_stability_probes += 1;
                    self.stable[state] = (self.epoch,
                        costs.stable_interval(state, proof.first_frame, proof.last_frame));
                }
                if self.stable[state].1 { 1 } else { 2 }
            } else { 0 };
            self.flags.push(flag);
        }
    }

    pub(super) fn skip(&self, entry: usize, stats: &mut WindowStats) -> bool {
        stats.incremental_candidate_transitions += 1;
        match self.flags[entry] {
            1 => { stats.incremental_skipped_transitions += 1; true }
            2 => { stats.incremental_clock_fallback_transitions += 1; false }
            _ => { stats.incremental_changed_transitions += 1; false }
        }
    }
}
