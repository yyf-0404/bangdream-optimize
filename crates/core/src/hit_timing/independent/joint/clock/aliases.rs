//! Native value clocks observe effect machines, never the original slot label.
//! Durations and FIFO ready times continue to come from the actual trigger.
use super::*;

pub(super) fn classes(model: &JointTimeline, enabled: bool) -> Vec<u8> {
    let mut classes: Vec<u8> = Vec::with_capacity(model.machines.len());
    for (slot, machine) in model.machines.iter().enumerate() {
        let equivalent = enabled.then(|| (0..slot).find(|&other| {
            let candidate = &model.machines[other];
            model.input.skills[slot].cache_key()[1] == model.input.skills[other].cache_key()[1]
                && machine.next == candidate.next
                && machine.monotone == candidate.monotone
                && machine.baseline_dominance == candidate.baseline_dominance
                && machine.values.len() == candidate.values.len()
                && machine.values.iter().zip(&candidate.values)
                    .all(|(a, b)| a.map(f32::to_bits) == b.map(f32::to_bits))
        })).flatten();
        classes.push(equivalent.map_or(slot as u8, |other| classes[other]));
    }
    classes
}
