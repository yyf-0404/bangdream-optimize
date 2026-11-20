//! Non-enumerating launch-time bounds for charts without BPM commands.
//!
//! With fixed BPM, each launcher starts at the native f32 lead and receives
//! the same positive f32 update on every frame. For a native update below 64
//! ticks, round-to-nearest addition and one-bar wrap preserve order in the
//! initial lead. Across adjacent bars, the lower beat is at most the f32
//! predecessor of 192 (a gap of 2^-16), while addition below 256 rounds by
//! at most 2^-17; it cannot overtake the upper beat after wrapping. Thus the
//! two lead endpoints enclose every standard speed in the block, even when
//! individual speeds launch batches on different frames. Native batch order
//! imposes `t_i = max(first_reach_i, t_(i-1)+1)` for a nonempty non-bar-zero
//! batch, and `t_i = t_(i-1)+1` for empty/bar-zero batches. The recurrence is
//! monotone in both arguments, so endpoint reach bounds enclose each t_i.
use super::{initial_launcher_lead_bounds, BarProgressBounds, F32Range, FrameRange, SpeedBlock};
use crate::hit_timing::{green::launch::NativeBatch, ReplayError, MAX_SECONDS, REPLAY_FPS};

/// Native update index; input sees a batch launched at update `f` on frame
/// `f+1`. This does not assert that the entire green trajectory is bounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchLaunchBounds {
    pub update_frame: FrameRange,
    /// First launcher reach of this batch position, before one-per-update
    /// queueing. None for empty/bar-zero batches that bypass the gate.
    pub first_reach_update_frame: Option<FrameRange>,
}

/// Per-batch upper bound on `event_frame - first_reach_frame`. `None` marks a
/// bypass batch without a normal launcher reach. For a first reach after frame
/// zero, the previous frame's launcher was strictly below the batch tick, so
/// the first-reach frame of a later batch is separated by a minimum number of
/// updates derived from the chart tick gap and a binary32 local step cap.
pub struct ConstantBpmQueueDelayProof {
    pub max_local_step_ticks: f64,
    pub delay_caps: Vec<Option<u32>>,
}

pub fn constant_bpm_queue_delay_caps(
    basic_bpm: f32,
    batches: &[NativeBatch],
    launch: &[BatchLaunchBounds],
) -> Result<ConstantBpmQueueDelayProof, ReplayError> {
    if batches.len() != launch.len() || batches.is_empty() {
        return Err(ReplayError::Input(
            "invalid native batch queue proof inputs",
        ));
    }
    let step = crate::hit_timing::green::clock::FRAME_SECONDS
        / ((240.0f32 / basic_bpm)
            / crate::hit_timing::green::clock::MUSIC_BAR_DIVISION_COUNT as f32);
    if !step.is_finite() || step <= 0.0 || step >= 64.0 {
        return Err(ReplayError::Input("invalid fixed-BPM queue step"));
    }
    const U: f64 = 1.0 / 16_777_216.0;
    let max_tick = f64::from(batches.last().unwrap().position.absolute()?);
    let max_step = f64::from(step) + 1.0 / 131_072.0 + 2.0 * U * (max_tick + 64.0) + 1e-7;
    let mut caps = Vec::<Option<u32>>::with_capacity(batches.len());
    for (index, batch) in batches.iter().enumerate() {
        let Some(reach) = launch[index].first_reach_update_frame else {
            caps.push(None);
            continue;
        };
        let event = launch[index].update_frame;
        if event.upper < reach.lower {
            return Err(ReplayError::Input("batch event precedes first reach"));
        }
        let mut cap = if index == 0 {
            0
        } else {
            event.upper - reach.lower
        };
        if index > 0 {
            if let (Some(previous_reach), Some(previous_cap)) =
                (launch[index - 1].first_reach_update_frame, caps[index - 1])
            {
                if previous_reach.lower > 0 && reach.lower > 0 {
                    let gap =
                        batch.position.absolute()? - batches[index - 1].position.absolute()?;
                    if gap <= 0 {
                        return Err(ReplayError::Input("unordered native batch queue"));
                    }
                    let minimum_reach_gap =
                        ((f64::from(gap) / max_step).ceil() as u32).saturating_sub(1);
                    cap = cap.min(
                        previous_cap
                            .saturating_add(1)
                            .saturating_sub(minimum_reach_gap),
                    );
                }
            }
        }
        caps.push(Some(cap));
    }
    Ok(ConstantBpmQueueDelayProof {
        max_local_step_ticks: max_step,
        delay_caps: caps,
    })
}

fn exact_bar(value: f32) -> Result<BarProgressBounds, ReplayError> {
    BarProgressBounds::from_lead(F32Range::new(value, value)?)?.ok_or(ReplayError::Input(
        "exact launcher bar could not be determined",
    ))
}

fn advance_exact(state: BarProgressBounds, bpm: f32) -> Result<BarProgressBounds, ReplayError> {
    state
        .advance(bpm)?
        .ok_or(ReplayError::Input("exact native clock branch unresolved"))
}

/// Launcher absolute-position envelope at an inclusive range of input frames.
/// Frame zero is the initial lead, and frame `n` is the native clock after
/// `n` fixed-BPM updates. Endpoint lead trajectories enclose every standard
/// speed in the block; the launcher is increasing in the frame index too.
/// This carries no batch queue or BPM-command semantics.
pub fn constant_bpm_launcher_position_at_frames(
    block: SpeedBlock,
    basic_bpm: f32,
    input_frames: FrameRange,
) -> Result<F32Range, ReplayError> {
    if input_frames.lower > input_frames.upper || input_frames.upper > MAX_SECONDS * REPLAY_FPS {
        return Err(ReplayError::Input("invalid launcher frame envelope"));
    }
    let lead = initial_launcher_lead_bounds(block, basic_bpm)?;
    let mut lower = exact_bar(lead.lower)?;
    let mut upper = exact_bar(lead.upper)?;
    let mut position_lower = None;
    for frame in 0..=input_frames.upper {
        if frame > 0 {
            lower = advance_exact(lower, basic_bpm)?;
            upper = advance_exact(upper, basic_bpm)?;
        }
        if frame == input_frames.lower {
            position_lower = Some(lower.absolute_position()?.lower);
        }
    }
    F32Range::new(
        position_lower.ok_or(ReplayError::Input("missing launcher lower position"))?,
        upper.absolute_position()?.upper,
    )
}

/// Certify batch launch frame ranges without reading any speed variant.
/// A BPM command changes launcher rate at launch and music rate later; callers
/// must use a branch-aware interval timeline for those charts.
pub fn constant_bpm_batch_launch_bounds(
    block: SpeedBlock,
    basic_bpm: f32,
    batches: &[NativeBatch],
) -> Result<Vec<BatchLaunchBounds>, ReplayError> {
    if batches.is_empty() || batches.iter().any(|b| b.bpm.is_some()) {
        return Err(ReplayError::Input(
            "constant BPM interval launch requires batches without BPM commands",
        ));
    }
    let step = crate::hit_timing::green::clock::FRAME_SECONDS
        / ((240.0f32 / basic_bpm)
            / crate::hit_timing::green::clock::MUSIC_BAR_DIVISION_COUNT as f32);
    if !step.is_finite() || step <= 0.0 || step >= 64.0 {
        return Err(ReplayError::Input(
            "fixed BPM update is outside proven launcher order range",
        ));
    }
    let mut positions = Vec::with_capacity(batches.len());
    let mut previous = None;
    let mut timed = Vec::new();
    for (i, batch) in batches.iter().enumerate() {
        let absolute = batch.position.absolute()?;
        if previous.is_some_and(|p| p >= absolute) {
            return Err(ReplayError::Input(
                "native batches must already be grouped and ordered",
            ));
        }
        previous = Some(absolute);
        positions.push(absolute as f32);
        if !batch.empty && batch.position.bar != 0 {
            timed.push(i);
        }
    }
    let lead = initial_launcher_lead_bounds(block, basic_bpm)?;
    let mut lower_launcher = exact_bar(lead.lower)?;
    let mut upper_launcher = exact_bar(lead.upper)?;
    let mut music = exact_bar(0.0)?;
    let mut music_after_update = Vec::new();
    let mut first_possible = vec![None; batches.len()];
    let mut first_guaranteed = vec![None; batches.len()];
    let mut possible_cursor = 0;
    let mut guaranteed_cursor = 0;
    let limit = MAX_SECONDS * REPLAY_FPS;
    while guaranteed_cursor < timed.len() {
        let frame = u32::try_from(music_after_update.len()).map_err(|_| {
            ReplayError::Input("native launch trace exceeds supported chart duration")
        })?;
        if frame >= limit {
            return Err(ReplayError::Input(
                "speed block could not certify native batch launch",
            ));
        }
        lower_launcher = advance_exact(lower_launcher, basic_bpm)?;
        upper_launcher = advance_exact(upper_launcher, basic_bpm)?;
        music = advance_exact(music, basic_bpm)?;
        let lower_pos = lower_launcher.absolute_position()?.lower;
        let upper_pos = upper_launcher.absolute_position()?.upper;
        if lower_pos > upper_pos {
            return Err(ReplayError::Input("native launcher interval order failed"));
        }
        while possible_cursor < timed.len() && positions[timed[possible_cursor]] <= upper_pos {
            first_possible[timed[possible_cursor]] = Some(frame);
            possible_cursor += 1;
        }
        while guaranteed_cursor < timed.len() && positions[timed[guaranteed_cursor]] <= lower_pos {
            first_guaranteed[timed[guaranteed_cursor]] = Some(frame);
            guaranteed_cursor += 1;
        }
        music_after_update.push(music);
    }
    let mut result = Vec::with_capacity(batches.len());
    let mut previous_event: Option<FrameRange> = None;
    for (i, batch) in batches.iter().enumerate() {
        let next_input = previous_event.map_or(FrameRange { lower: 0, upper: 0 }, |p| FrameRange {
            lower: p.lower.saturating_add(1),
            upper: p.upper.saturating_add(1),
        });
        let event = if batch.empty || batch.position.bar == 0 {
            next_input
        } else {
            let possible = first_possible[i]
                .ok_or(ReplayError::Input("missing possible native batch reach"))?;
            let guaranteed = first_guaranteed[i]
                .ok_or(ReplayError::Input("missing guaranteed native batch reach"))?;
            FrameRange {
                lower: next_input.lower.max(possible),
                upper: next_input.upper.max(guaranteed),
            }
        };
        if event.upper >= limit {
            return Err(ReplayError::Input(
                "speed block could not certify native batch launch",
            ));
        }
        while music_after_update.len() <= event.upper as usize {
            music = advance_exact(music, basic_bpm)?;
            music_after_update.push(music);
        }
        if !batch.empty && batch.position.bar != 0 {
            let at_latest = music_after_update[event.upper as usize];
            let music_position = at_latest.absolute_position()?.upper;
            if batch.position.bar < at_latest.bar || positions[i] - music_position <= 0.0 {
                return Err(ReplayError::Input(
                    "speed block cannot prove batch remains nonstale",
                ));
            }
        }
        previous_event = Some(event);
        result.push(BatchLaunchBounds {
            update_frame: event,
            first_reach_update_frame: if batch.empty || batch.position.bar == 0 {
                None
            } else {
                Some(FrameRange {
                    lower: first_possible[i].unwrap(),
                    upper: first_guaranteed[i].unwrap(),
                })
            },
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::green::clock::MusicClock120;
    use crate::hit_timing::green::clock::MUSIC_BAR_DIVISION_COUNT;
    use crate::hit_timing::green::launch::{LaunchTimeline, NativePosition};

    fn batch(tick: i32, empty: bool) -> NativeBatch {
        NativeBatch {
            position: NativePosition {
                bar: tick / MUSIC_BAR_DIVISION_COUNT,
                numerator: tick % MUSIC_BAR_DIVISION_COUNT,
                denominator: MUSIC_BAR_DIVISION_COUNT,
            },
            empty,
            bpm: None,
        }
    }

    #[test]
    fn bounds_contain_every_native_launch_in_experimental_chart() {
        let batches = vec![
            batch(0, false),
            batch(192, true),
            batch(384, false),
            batch(385, false),
            batch(386, false),
            batch(576, false),
        ];
        let block = SpeedBlock::all();
        let bounded = constant_bpm_batch_launch_bounds(block, 120.0, &batches).unwrap();
        for q in block.first()..=block.last() {
            let mut actual =
                LaunchTimeline::new(f32::from(q) / 100.0, 120.0, batches.clone()).unwrap();
            let mut seen = vec![None; batches.len()];
            while seen.last().unwrap().is_none() {
                let step = actual.update_120().unwrap();
                if let Some(batch) = step.batch {
                    seen[batch.index] = Some(step.next_input_frame - 1);
                }
            }
            for (actual, bound) in seen.into_iter().zip(&bounded) {
                let frame = actual.unwrap();
                assert!(
                    bound.update_frame.lower <= frame && frame <= bound.update_frame.upper,
                    "q={q} frame={frame} bound={bound:?}"
                );
            }
        }
    }

    #[test]
    fn launcher_position_encloses_native_clock_at_bounded_frames() {
        for block in [
            SpeedBlock::new(100, 108).unwrap(),
            SpeedBlock::new(1098, 1104).unwrap(),
        ] {
            for frames in [
                FrameRange::new(0, 0).unwrap(),
                FrameRange::new(313, 316).unwrap(),
                FrameRange::new(618, 620).unwrap(),
            ] {
                let enclosure =
                    constant_bpm_launcher_position_at_frames(block, 185.0, frames).unwrap();
                for q in block.first()..=block.last() {
                    let mut clock = MusicClock120::new(f32::from(q) / 100.0, 185.0).unwrap();
                    for frame in 0..=frames.upper {
                        if frame > 0 {
                            clock.update_120().unwrap();
                        }
                        if frame >= frames.lower {
                            let actual = clock.launcher.absolute_position().unwrap();
                            assert!(
                                enclosure.lower <= actual && actual <= enclosure.upper,
                                "q={q} frame={frame} enclosure={enclosure:?} actual={actual}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn queue_delay_caps_contain_dense_native_batch_schedule() {
        let batches = [384, 385, 386, 400, 403, 406, 800, 803].map(|tick| batch(tick, false));
        let bounded = constant_bpm_batch_launch_bounds(SpeedBlock::all(), 185.0, &batches).unwrap();
        let queue = constant_bpm_queue_delay_caps(185.0, &batches, &bounded).unwrap();
        for q in [100u16, 500, 1000, 1200] {
            let mut timeline =
                LaunchTimeline::new(f32::from(q) / 100.0, 185.0, batches.to_vec()).unwrap();
            let mut first_reach = vec![None; batches.len()];
            let mut event = vec![None; batches.len()];
            while event.last().unwrap().is_none() {
                let step = timeline.update_120().unwrap();
                let launcher = step
                    .after_launch_clock
                    .launcher
                    .absolute_position()
                    .unwrap();
                for (index, batch) in batches.iter().enumerate() {
                    if first_reach[index].is_none()
                        && launcher >= batch.position.absolute().unwrap() as f32
                    {
                        first_reach[index] = Some(step.next_input_frame - 1);
                    }
                }
                if let Some(batch) = step.batch {
                    event[batch.index] = Some(step.next_input_frame - 1);
                }
            }
            for index in 0..batches.len() {
                let delay = event[index].unwrap() - first_reach[index].unwrap();
                assert!(
                    delay <= queue.delay_caps[index].unwrap(),
                    "q={q} index={index}"
                );
            }
        }
    }
}
