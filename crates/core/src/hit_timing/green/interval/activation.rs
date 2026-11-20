//! Fixed-BPM activation overshoot premise without speed-variant comparison.
use super::{
    error::{constant_bpm_clock_error, ConstantBpmClockError},
    launch::{constant_bpm_batch_launch_bounds, BatchLaunchBounds},
    F32Range, FrameRange, SpeedBlock,
};
use crate::hit_timing::{
    green::{clock::MUSIC_BAR_DIVISION_COUNT, launch::NativeBatch, trajectory::NativeGreenChain},
    ReplayError,
};

/// Integer activationAdjust substep counts from a zero-progress Wait/Launch
/// state. A branch may still contain several counts; the native while loop is
/// not replaced by one rounded real-valued target time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActivationSubstepCounts {
    pub lower: u32,
    pub upper: u32,
    /// Progress immediately after activateAdjust. The endpoints need not
    /// occur at the same speed as the count endpoints.
    pub seed_progress: F32Range,
}

/// A discrete joint branch for two activations driven by the same fixed-BPM
/// launcher. The branch can contain several speeds, but it preserves their
/// shared frame gap and the integer numbers of activation substeps.
#[derive(Clone, Copy, Debug)]
pub struct PairedActivationBranch {
    pub frame_gap: u32,
    pub root_substeps: u32,
    pub after_substeps: u32,
    pub root_seed_progress: F32Range,
    pub after_seed_progress: F32Range,
}

#[derive(Clone, Copy, Debug)]
pub struct ActivationNodeBounds {
    pub absolute_tick: i32,
    pub input_frames: FrameRange,
    pub launcher_position: F32Range,
    pub seed: ActivationSubstepCounts,
    /// Optional certified upper bound on launcher position minus node tick at
    /// this activation. For a first reach after frame zero, one local launcher
    /// update bounds the overshoot.
    pub overshoot_cap_ticks: Option<f64>,
}

fn progress_for_substeps(
    count: u32,
    delta: f32,
    arrival: F32Range,
) -> Result<F32Range, ReplayError> {
    let lower_increment = delta / arrival.upper;
    let upper_increment = delta / arrival.lower;
    let mut lower = 0.0f32;
    let mut upper = 0.0f32;
    for _ in 0..count {
        lower += lower_increment;
        upper += upper_increment;
    }
    F32Range::new(lower, upper)
}

/// Retain only activation count combinations consistent with the same
/// launcher trajectory. The candidate list is over small *discrete state*
/// (frame gap and substep counts), never over speed settings. A rejected
/// combination has disjoint overshoot intervals even after binary32 rounding
/// allowances for target division, absolute-position reads and local clock
/// updates. Root and after activation must already be proved to occur with
/// launcher position at or beyond their ticks and after strictly later.
pub fn paired_activation_substep_branches(
    block: SpeedBlock,
    root: ActivationNodeBounds,
    after: ActivationNodeBounds,
    note_bpm: f32,
    step_ticks: f32,
) -> Result<Vec<PairedActivationBranch>, ReplayError> {
    if root.absolute_tick < 192
        || after.absolute_tick <= root.absolute_tick
        || after.absolute_tick >= 16_777_216
        || !note_bpm.is_finite()
        || note_bpm <= 0.0
        || !step_ticks.is_finite()
        || step_ticks <= 0.0
        || step_ticks.to_bits()
            != (crate::hit_timing::green::clock::FRAME_SECONDS
                / ((240.0f32 / note_bpm) / MUSIC_BAR_DIVISION_COUNT as f32))
                .to_bits()
        || root.input_frames.lower > root.input_frames.upper
        || after.input_frames.lower > after.input_frames.upper
        || root.seed.lower > root.seed.upper
        || after.seed.lower > after.seed.upper
        || F32Range::new(root.launcher_position.lower, root.launcher_position.upper).is_err()
        || F32Range::new(after.launcher_position.lower, after.launcher_position.upper).is_err()
        || after.input_frames.upper <= root.input_frames.lower
        || after.input_frames.upper - root.input_frames.lower > 64
        || [root.overshoot_cap_ticks, after.overshoot_cap_ticks]
            .into_iter()
            .flatten()
            .any(|cap| !cap.is_finite() || cap < 0.0)
    {
        return Err(ReplayError::Input("invalid paired activation premises"));
    }
    let arrival = super::arrival_seconds_bounds(block)?;
    let lead = super::initial_launcher_lead_bounds(block, note_bpm)?;
    let x = (note_bpm * MUSIC_BAR_DIVISION_COUNT as f32) / 14_400.0f32;
    let delta = 1.0f32 / (120.0f32 + x * 60.0f32);
    if !delta.is_finite() || delta <= 0.0 {
        return Err(ReplayError::Input("invalid paired activation substep"));
    }
    const U: f64 = 1.0 / 16_777_216.0;
    const PAD: f64 = 1e-7;
    let offset = |node: ActivationNodeBounds,
                  count: u32|
     -> Result<Option<(f64, f64, F32Range)>, ReplayError> {
        let progress = progress_for_substeps(count, delta, arrival)?;
        let tick = f64::from(node.absolute_tick);
        let position = node.launcher_position;
        let subtraction_error = U * (f64::from(position.upper.abs()) + tick) + PAD;
        let (from_count_lower, from_count_upper) = if count == 0 {
            (0.0, 0.0)
        } else {
            let before = progress_for_substeps(count - 1, delta, arrival)?;
            (
                f64::from(before.lower) * f64::from(lead.lower) / (1.0 + U)
                    - subtraction_error
                    - PAD,
                f64::from(progress.upper) * f64::from(lead.upper) / (1.0 - U)
                    + subtraction_error
                    + PAD,
            )
        };
        let lower = from_count_lower
            .max(f64::from(position.lower) - tick)
            .max(0.0);
        let upper = from_count_upper
            .min(f64::from(position.upper) - tick)
            .min(node.overshoot_cap_ticks.unwrap_or(f64::INFINITY));
        Ok((lower <= upper + PAD).then_some((lower, upper, progress)))
    };
    let mut root_options = Vec::new();
    for count in root.seed.lower..=root.seed.upper {
        if let Some(bounds) = offset(root, count)? {
            root_options.push((count, bounds));
        }
    }
    let mut after_options = Vec::new();
    for count in after.seed.lower..=after.seed.upper {
        if let Some(bounds) = offset(after, count)? {
            after_options.push((count, bounds));
        }
    }
    let gap_min = after
        .input_frames
        .lower
        .saturating_sub(root.input_frames.upper)
        .max(1);
    let gap_max = after.input_frames.upper - root.input_frames.lower;
    let mut branches = Vec::new();
    for frame_gap in gap_min..=gap_max {
        let gap = f64::from(frame_gap);
        let change =
            gap * f64::from(step_ticks) - f64::from(after.absolute_tick - root.absolute_tick);
        // beat+step rounds within 2^-17 tick below 256. Wrap subtraction
        // is exact by Sterbenz; two absolute-position reads each cost u*P.
        let change_error = gap / 131_072.0
            + U * (f64::from(root.launcher_position.upper.abs())
                + f64::from(after.launcher_position.upper.abs()))
            + PAD;
        for &(root_substeps, (root_lower, root_upper, root_progress)) in &root_options {
            for &(after_substeps, (after_lower, after_upper, after_progress)) in &after_options {
                if root_lower + change - change_error > after_upper + PAD
                    || root_upper + change + change_error < after_lower - PAD
                {
                    continue;
                }
                branches.push(PairedActivationBranch {
                    frame_gap,
                    root_substeps,
                    after_substeps,
                    root_seed_progress: root_progress,
                    after_seed_progress: after_progress,
                });
            }
        }
    }
    if branches.is_empty() {
        return Err(ReplayError::Input("empty paired activation branch set"));
    }
    Ok(branches)
}

/// Enclose the native `activateAdjust` while-loop count for any speed in the
/// block and any launcher position in the supplied enclosure. The caller must
/// prove that enclosure at the same activation frame and that the state enters
/// with zero progress. This is an interval transfer, with the exact native
/// binary32 operation order for target division and each progress addition.
pub fn activation_substep_count_bounds(
    block: SpeedBlock,
    launcher_position: F32Range,
    absolute_tick: i32,
    note_bpm: f32,
) -> Result<ActivationSubstepCounts, ReplayError> {
    if !(0..16_777_216).contains(&absolute_tick) || !note_bpm.is_finite() || note_bpm <= 0.0 {
        return Err(ReplayError::Input("invalid activation substep premises"));
    }
    let launcher_position = F32Range::new(launcher_position.lower, launcher_position.upper)?;
    let arrival = super::arrival_seconds_bounds(block)?;
    let denominator = super::initial_launcher_lead_bounds(block, note_bpm)?;
    let tick = absolute_tick as f32;
    let numerator_lower = (launcher_position.lower - tick).max(0.0);
    let numerator_upper = (launcher_position.upper - tick).max(0.0);
    let target_lower = numerator_lower / denominator.upper;
    let target_upper = numerator_upper / denominator.lower;
    if !target_lower.is_finite() || !target_upper.is_finite() || target_lower > target_upper {
        return Err(ReplayError::Input("invalid activation target interval"));
    }
    let x = (note_bpm * MUSIC_BAR_DIVISION_COUNT as f32) / 14_400.0f32;
    let delta = 1.0f32 / (120.0f32 + x * 60.0f32);
    let increment_lower = delta / arrival.upper;
    let increment_upper = delta / arrival.lower;
    if !increment_lower.is_finite() || increment_lower <= 0.0 || !increment_upper.is_finite() {
        return Err(ReplayError::Input("invalid activation increment interval"));
    }
    let mut progress_lower = 0.0f32;
    let mut progress_upper = 0.0f32;
    let mut first_possible = (target_lower <= 0.0).then_some(0);
    let mut guaranteed_by = (target_upper <= 0.0).then_some(0);
    let mut seed_lower = (target_lower <= 0.0).then_some(0.0f32);
    if guaranteed_by.is_some() {
        return Ok(ActivationSubstepCounts {
            lower: 0,
            upper: 0,
            seed_progress: F32Range::new(0.0, 0.0)?,
        });
    }
    for count in 1..=10_000 {
        let previous_lower = progress_lower;
        let previous_upper = progress_upper;
        progress_lower = if count == 1 {
            increment_lower
        } else {
            progress_lower + increment_lower
        };
        progress_upper = if count == 1 {
            increment_upper
        } else {
            progress_upper + increment_upper
        };
        if !progress_lower.is_finite()
            || !progress_upper.is_finite()
            || progress_lower <= 0.0
            || progress_upper < progress_lower
            || progress_lower <= previous_lower
            || progress_upper <= previous_upper
        {
            return Err(ReplayError::Input("activation progress interval invalid"));
        }
        if first_possible.is_none() && progress_upper >= target_lower {
            first_possible = Some(count);
            seed_lower = Some(progress_lower);
        }
        if guaranteed_by.is_none() && progress_lower >= target_upper {
            guaranteed_by = Some(count);
        }
        if guaranteed_by.is_some() {
            break;
        }
    }
    let lower =
        first_possible.ok_or(ReplayError::Input("activation count exceeds proof horizon"))?;
    let upper =
        guaranteed_by.ok_or(ReplayError::Input("activation count exceeds proof horizon"))?;
    Ok(ActivationSubstepCounts {
        lower,
        upper,
        seed_progress: F32Range::new(
            seed_lower.ok_or(ReplayError::Input("missing activation seed lower bound"))?,
            progress_upper,
        )?,
    })
}

/// Every root and after-node activation has `(launcher-T)/lead <= max_ratio`.
/// `launcher>=T` follows from the native gate/after wait condition. The proof
/// also establishes that the music position is still before T at activation.
pub struct ConstantBpmActivationRatio {
    pub max_ratio: f64,
    pub clock: ConstantBpmClockError,
    pub batch_launch_bounds: Vec<BatchLaunchBounds>,
    /// Same chain/node order as the supplied source; input frames after native
    /// activation, including the root's one-update after-node wait rule.
    pub node_activation_frames: Vec<Vec<FrameRange>>,
}

pub fn constant_bpm_activation_ratio_bound(
    block: SpeedBlock,
    basic_bpm: f32,
    batches: &[NativeBatch],
    chains: &[NativeGreenChain],
    max_frames: u32,
) -> Result<ConstantBpmActivationRatio, ReplayError> {
    if chains.is_empty() {
        return Err(ReplayError::Input("empty native green activation proof"));
    }
    let clock = constant_bpm_clock_error(block, basic_bpm, max_frames)?;
    let launch = constant_bpm_batch_launch_bounds(block, basic_bpm, batches)?;
    let batch_ticks = batches
        .iter()
        .map(|batch| batch.position.absolute())
        .collect::<Result<Vec<_>, _>>()?;
    let lead_lower = super::initial_launcher_lead_bounds(block, basic_bpm)?.lower as f64;
    let step = f64::from(clock.step_ticks);
    let drift = clock.clock_ticks;
    // A launcher can cross T by at most one native update. Its music clock is
    // still before T whenever the minimum lead exceeds that update plus the
    // two launcher and two music affine deviations.
    if lead_lower <= step + 4.0 * drift + 0.001 {
        return Err(ReplayError::Input(
            "cannot prove after-node activation precedes music position",
        ));
    }
    let mut latest_tick = 0i32;
    let mut node_activation_frames = Vec::with_capacity(chains.len());
    for chain in chains {
        if chain.nodes.len() < 2 {
            return Err(ReplayError::Input("invalid native green activation chain"));
        }
        let root = &chain.nodes[0];
        let batch = batches
            .get(chain.root_batch)
            .ok_or(ReplayError::Input("missing native green root batch"))?;
        if batch.empty || batch.position.absolute()? != root.absolute_position {
            return Err(ReplayError::Input("green root batch position differs"));
        }
        let root_event = launch[chain.root_batch].update_frame;
        let root_first_input = root_event.upper + 1;
        if root_first_input + 1 > max_frames {
            return Err(ReplayError::Input("root activation exceeds clock proof"));
        }
        let mut previous_tick = None;
        let mut this_chain = Vec::with_capacity(chain.nodes.len());
        for (index, node) in chain.nodes.iter().enumerate() {
            let tick = node.absolute_position;
            if !(192..16_777_216).contains(&tick)
                || previous_tick.is_some_and(|p| p >= tick)
                || node.note_bpm.to_bits() != basic_bpm.to_bits()
            {
                return Err(ReplayError::Input(
                    "native green node is outside fixed-BPM activation proof",
                ));
            }
            previous_tick = Some(tick);
            latest_tick = latest_tick.max(tick);
            let activation = if index == 0 {
                FrameRange {
                    lower: root_event.lower + 1,
                    upper: root_event.upper + 1,
                }
            } else {
                let batch_index = batch_ticks
                    .binary_search(&tick)
                    .map_err(|_| ReplayError::Input("after-node has no native batch position"))?;
                let reach =
                    launch[batch_index]
                        .first_reach_update_frame
                        .ok_or(ReplayError::Input(
                            "after-node batch bypasses launcher gate",
                        ))?;
                FrameRange {
                    lower: (root_event.lower + 1).max(reach.lower) + 1,
                    upper: (root_event.upper + 1).max(reach.upper) + 1,
                }
            };
            if activation.upper > max_frames {
                return Err(ReplayError::Input("node activation exceeds clock proof"));
            }
            this_chain.push(activation);
            if index > 0 {
                // prepare_with_table checks the after-node at update index
                // max(root.next_input_frame, first launcher reach). If the
                // first reach is earlier, the first possible activation is
                // one update after root; its music clock is speed-independent.
                let music_after_root = f64::from(root_first_input + 1) * step + drift + 0.001;
                if music_after_root >= f64::from(tick) {
                    return Err(ReplayError::Input(
                        "cannot prove after-node remains ahead of music",
                    ));
                }
            }
        }
        node_activation_frames.push(this_chain);
    }
    // If a node has not been reached by the root's next update, it is reached
    // by max_frames: launcher >= lead_min+n*step-drift. The first crossing
    // uses the one-update argument above; no speed trajectory is evaluated.
    if lead_lower + f64::from(max_frames) * step - drift < f64::from(latest_tick) + 0.001 {
        return Err(ReplayError::Input(
            "after-node first reach exceeds clock proof",
        ));
    }
    // At activation, music<T. The two clock deviations give
    // launcher-music <= lead+2*drift, hence (launcher-T)/lead
    // < 1+2*drift/lead_min. An additional 2^-24 is outward padding.
    let max_ratio = 1.0 + 2.0 * drift / lead_lower + 1.0 / 16_777_216.0;
    Ok(ConstantBpmActivationRatio {
        max_ratio,
        clock,
        batch_launch_bounds: launch,
        node_activation_frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::green::{
        activation::MovementState, clock::MusicClock120, launch::NativePosition,
        trajectory::NativeGreenNode, GreenRole,
    };

    fn position(tick: i32) -> NativePosition {
        NativePosition {
            bar: tick / 192,
            numerator: tick % 192,
            denominator: 192,
        }
    }

    #[test]
    fn proves_activation_ratio_without_speed_walk() {
        let batches = [384, 432, 480].map(|tick| NativeBatch {
            position: position(tick),
            empty: false,
            bpm: None,
        });
        let chain = NativeGreenChain {
            root_batch: 0,
            nodes: [384, 432, 480]
                .into_iter()
                .enumerate()
                .map(|(i, tick)| NativeGreenNode {
                    absolute_position: tick,
                    note_bpm: 185.0,
                    note_index: Some(i),
                    role: if i == 0 {
                        GreenRole::Head
                    } else if i == 2 {
                        GreenRole::PlainEnd
                    } else {
                        GreenRole::HeldMiddle
                    },
                })
                .collect(),
        };
        let proof = constant_bpm_activation_ratio_bound(
            SpeedBlock::all(),
            185.0,
            &batches,
            &[chain],
            13_000,
        )
        .unwrap();
        assert!(proof.max_ratio < 1.004);
        assert_eq!(proof.batch_launch_bounds.len(), batches.len());
    }

    #[test]
    fn activation_seed_bounds_contain_native_substep_loop() {
        for block in [
            SpeedBlock::new(100, 108).unwrap(),
            SpeedBlock::new(1098, 1104).unwrap(),
        ] {
            for (tick, frame) in [(1200, 313), (1203, 316), (5000, 1200)] {
                let position = super::super::launch::constant_bpm_launcher_position_at_frames(
                    block,
                    185.0,
                    FrameRange::new(frame, frame).unwrap(),
                )
                .unwrap();
                let bound = activation_substep_count_bounds(block, position, tick, 185.0).unwrap();
                for q in block.first()..=block.last() {
                    let speed = f32::from(q) / 100.0;
                    let mut clock = MusicClock120::new(speed, 185.0).unwrap();
                    for _ in 0..frame {
                        clock.update_120().unwrap();
                    }
                    let mut movement = MovementState::default();
                    let steps = movement
                        .activate(
                            clock.launcher.absolute_position().unwrap(),
                            tick,
                            185.0,
                            speed,
                        )
                        .unwrap();
                    assert!(
                        u64::from(bound.lower) <= steps && steps <= u64::from(bound.upper),
                        "q={q} tick={tick} frame={frame} bound={bound:?} steps={steps}"
                    );
                    assert!(
                        bound.seed_progress.lower <= movement.progress
                            && movement.progress <= bound.seed_progress.upper,
                        "q={q} tick={tick} frame={frame} bound={bound:?} progress={}",
                        movement.progress
                    );
                }
            }
        }
    }

    #[test]
    fn paired_discrete_branches_contain_same_speed_native_activations() {
        use super::super::launch::{
            constant_bpm_batch_launch_bounds, constant_bpm_launcher_position_at_frames,
            constant_bpm_queue_delay_caps,
        };
        let batches = [1128, 1131].map(|tick| NativeBatch {
            position: position(tick),
            empty: false,
            bpm: None,
        });
        for block in [
            SpeedBlock::new(100, 102).unwrap(),
            SpeedBlock::new(1099, 1103).unwrap(),
        ] {
            let launch = constant_bpm_batch_launch_bounds(block, 185.0, &batches).unwrap();
            let queue = constant_bpm_queue_delay_caps(185.0, &batches, &launch).unwrap();
            assert_eq!(queue.delay_caps[0], Some(0));
            let root_frames = FrameRange::new(
                launch[0].update_frame.lower + 1,
                launch[0].update_frame.upper + 1,
            )
            .unwrap();
            let reach = launch[1].first_reach_update_frame.unwrap();
            let after_frames = FrameRange::new(reach.lower + 1, reach.upper + 1).unwrap();
            let root_position =
                constant_bpm_launcher_position_at_frames(block, 185.0, root_frames).unwrap();
            let after_position =
                constant_bpm_launcher_position_at_frames(block, 185.0, after_frames).unwrap();
            let root = ActivationNodeBounds {
                absolute_tick: 1128,
                input_frames: root_frames,
                launcher_position: root_position,
                seed: activation_substep_count_bounds(block, root_position, 1128, 185.0).unwrap(),
                overshoot_cap_ticks: Some(queue.max_local_step_ticks),
            };
            let after = ActivationNodeBounds {
                absolute_tick: 1131,
                input_frames: after_frames,
                launcher_position: after_position,
                seed: activation_substep_count_bounds(block, after_position, 1131, 185.0).unwrap(),
                overshoot_cap_ticks: Some(queue.max_local_step_ticks),
            };
            let step = crate::hit_timing::green::clock::FRAME_SECONDS / (240.0f32 / 185.0 / 192.0);
            let branches =
                paired_activation_substep_branches(block, root, after, 185.0, step).unwrap();
            for q in block.first()..=block.last() {
                let speed = f32::from(q) / 100.0;
                let mut clock = MusicClock120::new(speed, 185.0).unwrap();
                let mut found = [None; 2];
                for frame in 1..=2_000 {
                    clock.update_120().unwrap();
                    let launcher = clock.launcher.absolute_position().unwrap();
                    for (index, tick) in [1128, 1131].into_iter().enumerate() {
                        if found[index].is_none() && launcher >= tick as f32 {
                            let mut state = MovementState::default();
                            let count = state.activate(launcher, tick, 185.0, speed).unwrap();
                            found[index] = Some((frame, count as u32));
                        }
                    }
                    if found[1].is_some() {
                        break;
                    }
                }
                let (root_frame, root_count) = found[0].unwrap();
                let (after_frame, after_count) = found[1].unwrap();
                assert!(
                    branches.iter().any(|branch| {
                        branch.frame_gap == after_frame - root_frame
                            && branch.root_substeps == root_count
                            && branch.after_substeps == after_count
                    }),
                    "q={q} native=({root_frame},{root_count},{after_frame},{after_count}) branches={branches:?}"
                );
            }
        }
    }
}
