//! Sound speed-block arithmetic for the native green preparation path.
//!
//! These bounds cover every *standard* speed in the integer block. They are
//! transfer results for the native f32 operation order, not sampled extrema.
//! Full trajectory/branch bounds are a separate obligation.
pub mod activation;
pub mod bridge;
pub mod error;
pub mod launch;
pub mod lifetime;
use super::{
    clock::{FRAME_SECONDS, MUSIC_BAR_DIVISION_COUNT},
    launch::{LaunchGate, NativePosition},
};
use crate::hit_timing::ReplayError;

/// Inclusive hundredth-speed domain. Values outside 1.00..12.00 and UI button
/// histories are not part of the declared standard-speed policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeedBlock {
    first: u16,
    last: u16,
}

impl SpeedBlock {
    pub fn new(first: u16, last: u16) -> Result<Self, ReplayError> {
        if first < 100 || last > 1200 || first > last {
            return Err(ReplayError::Input("invalid standard speed block"));
        }
        Ok(Self { first, last })
    }

    pub fn all() -> Self {
        Self {
            first: 100,
            last: 1200,
        }
    }

    pub fn first(self) -> u16 {
        self.first
    }

    pub fn last(self) -> u16 {
        self.last
    }

    /// The first nonlinear control boundary in GetNoteArrivalSeconds. The
    /// compare uses the f32 11.01 bit pattern; standard 11.01 belongs to the
    /// lower branch, and 11.02 belongs to the upper branch.
    pub fn split_arrival_branch(self) -> (Option<Self>, Option<Self>) {
        let lower = (self.first <= 1101).then(|| Self {
            first: self.first,
            last: self.last.min(1101),
        });
        let upper = (self.last >= 1102).then(|| Self {
            first: self.first.max(1102),
            last: self.last,
        });
        (lower, upper)
    }

    pub fn bisect(self) -> Option<(Self, Self)> {
        (self.first < self.last).then(|| {
            let middle = self.first + (self.last - self.first) / 2;
            (
                Self {
                    first: self.first,
                    last: middle,
                },
                Self {
                    first: middle + 1,
                    last: self.last,
                },
            )
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct F32Range {
    pub lower: f32,
    pub upper: f32,
}

impl F32Range {
    pub fn new(lower: f32, upper: f32) -> Result<Self, ReplayError> {
        if !lower.is_finite() || !upper.is_finite() || lower > upper {
            return Err(ReplayError::Input("invalid native f32 speed bound"));
        }
        Ok(Self { lower, upper })
    }

    fn hull(self, other: Self) -> Self {
        Self {
            lower: self.lower.min(other.lower),
            upper: self.upper.max(other.upper),
        }
    }

    // IEEE binary32 round-to-nearest is monotone on these finite, signed
    // intervals. Each transfer is the SAME primitive and order as the native
    // ARM expression; endpoint arithmetic is therefore an enclosure, not a
    // real-number approximation with a guessed rounding margin.
    fn add(self, rhs: f32) -> Result<Self, ReplayError> {
        Self::new(self.lower + rhs, self.upper + rhs)
    }

    fn subtract(self, rhs: f32) -> Result<Self, ReplayError> {
        Self::new(self.lower - rhs, self.upper - rhs)
    }

    fn multiply(self, rhs: f32) -> Result<Self, ReplayError> {
        if !rhs.is_finite() {
            return Err(ReplayError::Input("invalid native f32 speed bound"));
        }
        if rhs >= 0.0 {
            Self::new(self.lower * rhs, self.upper * rhs)
        } else {
            Self::new(self.upper * rhs, self.lower * rhs)
        }
    }

    fn divide(self, rhs: f32) -> Result<Self, ReplayError> {
        if !rhs.is_finite() || rhs == 0.0 {
            return Err(ReplayError::Input("invalid native f32 speed bound"));
        }
        if rhs > 0.0 {
            Self::new(self.lower / rhs, self.upper / rhs)
        } else {
            Self::new(self.upper / rhs, self.lower / rhs)
        }
    }

    fn add_range(self, rhs: Self) -> Result<Self, ReplayError> {
        Self::new(self.lower + rhs.lower, self.upper + rhs.upper)
    }

    fn divide_positive_range(self, rhs: Self) -> Result<Self, ReplayError> {
        if self.lower < 0.0 || rhs.lower <= 0.0 {
            return Err(ReplayError::Input("invalid native f32 division bound"));
        }
        Self::new(self.lower / rhs.upper, self.upper / rhs.lower)
    }
}

fn speed_values(block: SpeedBlock) -> Result<F32Range, ReplayError> {
    F32Range::new(
        f32::from(block.first) / 100.0f32,
        f32::from(block.last) / 100.0f32,
    )
}

/// Interval counterpart of CalcProgressRate. A block may contain both the
/// initial `progress == 0` branch and later accumulation; their union is kept.
/// The returned `native_validity_proven` flag is mandatory for a certificate:
/// a broad input interval may not establish that every nonzero progress grows.
#[derive(Clone, Copy, Debug)]
pub struct ProgressStepBounds {
    pub progress: F32Range,
    pub native_validity_proven: bool,
}

pub fn next_progress_bounds(
    progress: F32Range,
    arrival: F32Range,
    delta: f32,
    real_move_seconds: F32Range,
) -> Result<ProgressStepBounds, ReplayError> {
    if progress.lower < 0.0
        || arrival.lower <= 0.0
        || !delta.is_finite()
        || delta <= 0.0
        || real_move_seconds.lower < 0.0
    {
        return Err(ReplayError::Input("invalid native green progress bound"));
    }
    let initial = (progress.lower == 0.0)
        .then(|| real_move_seconds.divide_positive_range(arrival))
        .transpose()?;
    let continuing = (progress.upper > 0.0)
        .then(|| {
            let step = F32Range::new(delta, delta)?.divide_positive_range(arrival)?;
            progress.add_range(step)
        })
        .transpose()?;
    let value = match (initial, continuing) {
        (Some(a), Some(b)) => a.hull(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => unreachable!("nonnegative interval always includes a branch"),
    };
    // This sufficient condition is deliberately strict. False asks the
    // caller to split/refine; it is not evidence of a failing actual speed.
    let continuing_grows = continuing.is_none_or(|next| next.lower > progress.upper);
    Ok(ProgressStepBounds {
        progress: value,
        native_validity_proven: continuing_grows,
    })
}

/// Enclose NoteUtility.GetNoteArrivalSeconds for every standard setting in the
/// block. The two f32 branches have opposite-side discontinuity at 11.01/11.02,
/// so a block spanning the boundary is evaluated as a union before hulling.
pub fn arrival_seconds_bounds(block: SpeedBlock) -> Result<F32Range, ReplayError> {
    let (lower_branch, upper_branch) = block.split_arrival_branch();
    let mut result = None;
    if let Some(part) = lower_branch {
        let values = speed_values(part)?
            .subtract(1.0)?
            .multiply(-0.5)?
            .add(5.5)?;
        result = Some(values);
    }
    if let Some(part) = upper_branch {
        let values = speed_values(part)?
            .subtract(11.0)?
            .divide(-10.0)?
            .add(0.5)?;
        result = Some(result.map_or(values, |old: F32Range| old.hull(values)));
    }
    result.ok_or(ReplayError::Input("invalid standard speed block"))
}

/// Enclose NoteManager.ExecAwakeEnd's initial launcher lead. `basic_bpm` and
/// `192` are fixed over the block; only the speed-dependent arrival varies.
pub fn initial_launcher_lead_bounds(
    block: SpeedBlock,
    basic_bpm: f32,
) -> Result<F32Range, ReplayError> {
    if !basic_bpm.is_finite() || basic_bpm <= 0.0 {
        return Err(ReplayError::Input("invalid native music clock rate"));
    }
    arrival_seconds_bounds(block)?
        .multiply(basic_bpm)?
        .divide(240.0)?
        .multiply(192.0)
}

/// One native BarProgress state when all speeds in a block have the same
/// integer bar. `None` from a constructor/update means split the speed block
/// or retain multiple discrete states; it never means the real path is invalid.
#[derive(Clone, Copy, Debug)]
pub struct BarProgressBounds {
    pub bar: i32,
    pub beat: F32Range,
}

impl BarProgressBounds {
    fn from_lead(lead: F32Range) -> Result<Option<Self>, ReplayError> {
        if lead.lower < 0.0 {
            return Err(ReplayError::Input("invalid native launcher lead"));
        }
        let separate = |mut value: f32| -> Result<(i32, f32), ReplayError> {
            let mut bar = 0i32;
            while value >= MUSIC_BAR_DIVISION_COUNT as f32 {
                let next = value - MUSIC_BAR_DIVISION_COUNT as f32;
                if next >= value {
                    return Err(ReplayError::Input(
                        "native launcher lead stopped progressing",
                    ));
                }
                value = next;
                bar = bar
                    .checked_add(1)
                    .ok_or(ReplayError::Input("native bar position overflow"))?;
            }
            Ok((bar, value))
        };
        let lower = separate(lead.lower)?;
        let upper = separate(lead.upper)?;
        if lower.0 != upper.0 {
            return Ok(None);
        }
        Ok(Some(Self {
            bar: lower.0,
            beat: F32Range::new(lower.1, upper.1)?,
        }))
    }

    pub fn absolute_position(self) -> Result<F32Range, ReplayError> {
        let base = self
            .bar
            .checked_mul(MUSIC_BAR_DIVISION_COUNT)
            .ok_or(ReplayError::Input("native bar position overflow"))? as f32;
        self.beat.add(base)
    }

    /// Exact native per-frame scalar step, with interval addition and the same
    /// single-wrap rule. A mixed wrap is an unresolved discrete branch.
    pub fn advance(self, bpm: f32) -> Result<Option<Self>, ReplayError> {
        if !bpm.is_finite() || bpm <= 0.0 {
            return Err(ReplayError::Input("invalid native music clock rate"));
        }
        let step = FRAME_SECONDS / ((240.0f32 / bpm) / MUSIC_BAR_DIVISION_COUNT as f32);
        if !step.is_finite() || step <= 0.0 {
            return Err(ReplayError::Input("native music clock stopped progressing"));
        }
        if self.beat.upper + step <= self.beat.upper {
            return Ok(None);
        }
        let next = self.beat.add(step)?;
        let bar_size = MUSIC_BAR_DIVISION_COUNT as f32;
        if next.upper < bar_size {
            return Ok(Some(Self {
                bar: self.bar,
                beat: next,
            }));
        }
        if next.lower >= bar_size {
            return Ok(Some(Self {
                bar: self
                    .bar
                    .checked_add(1)
                    .ok_or(ReplayError::Input("native bar position overflow"))?,
                beat: next.subtract(bar_size)?,
            }));
        }
        Ok(None)
    }
}

pub fn initial_launcher_bar_bounds(
    block: SpeedBlock,
    basic_bpm: f32,
) -> Result<Option<BarProgressBounds>, ReplayError> {
    BarProgressBounds::from_lead(initial_launcher_lead_bounds(block, basic_bpm)?)
}

/// A launch gate is returned only when the native f32 comparisons have the
/// same result for every speed in the block. Mixed outcomes require refinement.
pub fn launch_gate_bounds(
    position: NativePosition,
    music: BarProgressBounds,
    launcher: BarProgressBounds,
) -> Result<Option<LaunchGate>, ReplayError> {
    let absolute = position.absolute()? as f32;
    if position.bar < music.bar {
        return Ok(Some(LaunchGate::Stale));
    }
    let music_pos = music.absolute_position()?;
    if absolute - music_pos.lower <= 0.0 {
        return Ok(Some(LaunchGate::Stale));
    }
    if absolute - music_pos.upper <= 0.0 {
        return Ok(None);
    }
    let launcher_pos = launcher.absolute_position()?;
    if absolute <= launcher_pos.lower {
        Ok(Some(LaunchGate::Ready))
    } else if absolute > launcher_pos.upper {
        Ok(Some(LaunchGate::Waiting))
    } else {
        Ok(None)
    }
}

/// Inclusive interval containing a first-grade frame, expiry frame, or edge
/// requirement for every speed in one block. A proof producer must establish
/// these bounds from native transfer semantics before calling the merger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRange {
    pub lower: u32,
    pub upper: u32,
}

impl FrameRange {
    pub fn new(lower: u32, upper: u32) -> Result<Self, ReplayError> {
        if lower > upper {
            return Err(ReplayError::Input("invalid native green frame bound"));
        }
        Ok(Self { lower, upper })
    }

    fn max_extremum(self, other: Self) -> Self {
        Self {
            lower: self.lower.max(other.lower),
            upper: self.upper.max(other.upper),
        }
    }

    fn min_extremum(self, other: Self) -> Self {
        Self {
            lower: self.lower.min(other.lower),
            upper: self.upper.min(other.upper),
        }
    }
}

fn check_partition(blocks: impl IntoIterator<Item = SpeedBlock>) -> Result<(), ReplayError> {
    let mut expected_first = 100u16;
    for block in blocks {
        if block.first != expected_first {
            return Err(ReplayError::Input(
                "incomplete standard speed bound coverage",
            ));
        }
        expected_first = block.last + 1;
    }
    if expected_first != 1201 {
        return Err(ReplayError::Input(
            "incomplete standard speed bound coverage",
        ));
    }
    Ok(())
}

/// Enclose a vector of maxima over all standard speeds. Use it for normalized
/// fixed gaps and for `F_s(a)` at each predecessor frame after the possible
/// common predecessor domain is known. For `F`, encode None as `u32::MAX`.
pub fn merge_maximum_frame_bounds(
    blocks: &[(SpeedBlock, Vec<FrameRange>)],
) -> Result<Vec<FrameRange>, ReplayError> {
    check_partition(blocks.iter().map(|x| x.0))?;
    let mut merged: Option<Vec<FrameRange>> = None;
    for (_, values) in blocks {
        if values.is_empty() {
            return Err(ReplayError::Input("empty native green edge bounds"));
        }
        if let Some(result) = &mut merged {
            if result.len() != values.len() {
                return Err(ReplayError::Input("speed bounds disagree on green edges"));
            }
            for (common, value) in result.iter_mut().zip(values) {
                *common = common.max_extremum(*value);
            }
        } else {
            merged = Some(values.clone());
        }
    }
    merged.ok_or(ReplayError::Input("empty native green edge bounds"))
}

/// Per-speed-block enclosure of one *normalized* visible node, after that
/// speed's compile_chain propagation. The lower/upper values are both needed:
/// the safe model uses the restrictive sides, while the possible model uses
/// the permissive sides. Neither side requires an attaining speed witness.
#[derive(Clone, Copy, Debug)]
pub struct WindowBounds {
    pub note_index: usize,
    pub first_frames: [FrameRange; 4],
    pub last_frame: FrameRange,
}

/// Bounds on the extrema consumed by compile_strict_chain. For grade `g`,
/// `first_frames[g]` contains max_speed(first_g); `last_frame` contains
/// min_speed(last). These are NOT bounds for an arbitrary single speed.
#[derive(Clone, Copy, Debug)]
pub struct CommonWindowBounds {
    pub note_index: usize,
    pub first_frames: [FrameRange; 4],
    pub last_frame: FrameRange,
}

/// Merge a complete, ordered partition of 1.00..12.00. This is algebra over
/// enclosures only: it neither evaluates nor compares actual speed variants.
pub fn merge_common_window_bounds(
    blocks: &[(SpeedBlock, Vec<WindowBounds>)],
) -> Result<Vec<CommonWindowBounds>, ReplayError> {
    check_partition(blocks.iter().map(|x| x.0))?;
    let mut merged: Vec<CommonWindowBounds> = Vec::new();
    for (_, windows) in blocks {
        if windows.is_empty() {
            return Err(ReplayError::Input("empty native green window bounds"));
        }
        if merged.is_empty() {
            merged = windows
                .iter()
                .map(|w| CommonWindowBounds {
                    note_index: w.note_index,
                    first_frames: w.first_frames,
                    last_frame: w.last_frame,
                })
                .collect();
            continue;
        }
        if merged.len() != windows.len() {
            return Err(ReplayError::Input("speed bounds disagree on green chain"));
        }
        for (common, window) in merged.iter_mut().zip(windows) {
            if common.note_index != window.note_index {
                return Err(ReplayError::Input("speed bounds disagree on green chain"));
            }
            for (value, other) in common.first_frames.iter_mut().zip(window.first_frames) {
                *value = value.max_extremum(other);
            }
            common.last_frame = common.last_frame.min_extremum(window.last_frame);
        }
    }
    Ok(merged)
}

impl CommonWindowBounds {
    /// Guaranteed all-speed valid domain. Empty means the current bounds are
    /// too loose to certify even one frame; it does not prove the true common
    /// domain is empty.
    pub fn safe_window(self) -> Option<crate::hit_timing::NoteWindow> {
        self.window(true)
    }

    /// Permissive domain containing every potentially all-speed valid frame.
    /// This may include frames that the native model rejects at some speed.
    pub fn possible_window(self) -> Option<crate::hit_timing::NoteWindow> {
        self.window(false)
    }

    fn window(self, safe: bool) -> Option<crate::hit_timing::NoteWindow> {
        let last = if safe {
            self.last_frame.lower
        } else {
            self.last_frame.upper
        };
        let mut first = self
            .first_frames
            .map(|v| if safe { v.upper } else { v.lower });
        if safe {
            for i in 1..4 {
                first[i] = first[i].max(first[i - 1]);
            }
        } else {
            for i in (0..3).rev() {
                first[i] = first[i].min(first[i + 1]);
            }
        }
        if first[0] > last || last == u32::MAX {
            return None;
        }
        Some(crate::hit_timing::NoteWindow::NativeGreen {
            first_frames: first.map(|f| f.min(last + 1)),
            last_frame: last,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::green::{clock::initial_launcher_lead, motion::arrival_seconds};

    #[test]
    fn standard_domain_splits_at_the_native_strict_branch() {
        let (low, high) = SpeedBlock::all().split_arrival_branch();
        assert_eq!(low.unwrap(), SpeedBlock::new(100, 1101).unwrap());
        assert_eq!(high.unwrap(), SpeedBlock::new(1102, 1200).unwrap());
        assert_eq!(
            SpeedBlock::new(1101, 1102)
                .unwrap()
                .bisect()
                .unwrap()
                .0
                .last(),
            1101
        );
        assert!(SpeedBlock::new(99, 1200).is_err());
    }

    #[test]
    fn interval_contains_every_standard_arrival_and_lead_in_experiment() {
        // Differential testing is diagnostic only. The transfer proof above
        // uses monotonic binary32 operations on each native control branch.
        for first in (100..=1200).step_by(37) {
            let last = (first + 83).min(1200);
            let block = SpeedBlock::new(first, last).unwrap();
            let arrival = arrival_seconds_bounds(block).unwrap();
            let lead = initial_launcher_lead_bounds(block, 173.0).unwrap();
            for q in first..=last {
                let speed = f32::from(q) / 100.0;
                let a = arrival_seconds(speed).unwrap();
                let l = initial_launcher_lead(speed, 173.0).unwrap();
                assert!(arrival.lower <= a && a <= arrival.upper, "q={q}");
                assert!(lead.lower <= l && l <= lead.upper, "q={q}");
            }
        }
        let around_jump = arrival_seconds_bounds(SpeedBlock::new(1101, 1102).unwrap()).unwrap();
        assert_eq!(
            around_jump.lower.to_bits(),
            arrival_seconds(11.01).unwrap().to_bits()
        );
        assert_eq!(
            around_jump.upper.to_bits(),
            arrival_seconds(11.02).unwrap().to_bits()
        );
    }

    #[test]
    fn complete_block_bounds_sandwich_internal_extrema_without_witnesses() {
        let range = |a, b| FrameRange::new(a, b).unwrap();
        let one = |note_index, first, last| WindowBounds {
            note_index,
            first_frames: [range(first, first + 2); 4],
            last_frame: range(last - 2, last),
        };
        let blocks = vec![
            (SpeedBlock::new(100, 481).unwrap(), vec![one(7, 100, 140)]),
            (SpeedBlock::new(482, 1007).unwrap(), vec![one(7, 105, 133)]),
            (SpeedBlock::new(1008, 1200).unwrap(), vec![one(7, 102, 138)]),
        ];
        let combined = merge_common_window_bounds(&blocks).unwrap();
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].first_frames[0], range(105, 107));
        assert_eq!(combined[0].last_frame, range(131, 133));
        assert!(matches!(
            combined[0].safe_window(),
            Some(crate::hit_timing::NoteWindow::NativeGreen {
                first_frames: [107, 107, 107, 107],
                last_frame: 131
            })
        ));
        assert!(matches!(
            combined[0].possible_window(),
            Some(crate::hit_timing::NoteWindow::NativeGreen {
                first_frames: [105, 105, 105, 105],
                last_frame: 133
            })
        ));
        let missing = &blocks[..2];
        assert!(merge_common_window_bounds(missing).is_err());
    }

    #[test]
    fn maximum_bounds_include_hidden_impossibility_and_reject_coverage_gaps() {
        let r = |a, b| FrameRange::new(a, b).unwrap();
        let blocks = vec![
            (
                SpeedBlock::new(100, 481).unwrap(),
                vec![r(120, 122), r(124, 125)],
            ),
            (
                SpeedBlock::new(482, 1007).unwrap(),
                vec![r(130, 132), r(131, 135)],
            ),
            (
                SpeedBlock::new(1008, 1200).unwrap(),
                vec![r(121, 124), r(u32::MAX, u32::MAX)],
            ),
        ];
        assert_eq!(
            merge_maximum_frame_bounds(&blocks).unwrap(),
            vec![r(130, 132), r(u32::MAX, u32::MAX)]
        );
        assert!(merge_maximum_frame_bounds(&blocks[..2]).is_err());
    }

    #[test]
    fn progress_transfer_contains_native_recurrence_across_speed_block() {
        use crate::hit_timing::green::motion::next_progress;
        let block = SpeedBlock::new(1000, 1031).unwrap();
        let arrival = arrival_seconds_bounds(block).unwrap();
        let delta = 1.0f32 / 120.0f32;
        let mut bounded = F32Range::new(0.0, 0.0).unwrap();
        let mut real = F32Range::new(0.0, 0.0).unwrap();
        let mut actual = (block.first()..=block.last())
            .map(|q| {
                (
                    arrival_seconds(f32::from(q) / 100.0).unwrap(),
                    0.0f32,
                    0.0f32,
                )
            })
            .collect::<Vec<_>>();
        for _ in 0..80 {
            real = real.add(delta).unwrap();
            let step = next_progress_bounds(bounded, arrival, delta, real).unwrap();
            bounded = step.progress;
            for (a, progress, elapsed) in &mut actual {
                *elapsed += delta;
                *progress = next_progress(*progress, *a, delta, *elapsed).unwrap();
                assert!(bounded.lower <= *progress && *progress <= bounded.upper);
            }
        }
    }

    #[test]
    fn interval_launcher_clock_and_gate_cover_native_frames_until_branch_split() {
        use crate::hit_timing::green::clock::MusicClock120;
        use crate::hit_timing::green::launch::launch_gate;
        let block = SpeedBlock::new(1000, 1031).unwrap();
        let mut launcher = initial_launcher_bar_bounds(block, 120.0).unwrap().unwrap();
        let mut music = BarProgressBounds {
            bar: 0,
            beat: F32Range::new(0.0, 0.0).unwrap(),
        };
        let mut actual = (block.first()..=block.last())
            .map(|q| MusicClock120::new(f32::from(q) / 100.0, 120.0).unwrap())
            .collect::<Vec<_>>();
        let position = NativePosition {
            bar: 1,
            numerator: 0,
            denominator: 1,
        };
        let mut certified_frames = 0;
        for _ in 0..150 {
            let Some(next_launcher) = launcher.advance(120.0).unwrap() else {
                break;
            };
            let Some(next_music) = music.advance(120.0).unwrap() else {
                break;
            };
            launcher = next_launcher;
            music = next_music;
            certified_frames += 1;
            let gate = launch_gate_bounds(position, music, launcher).unwrap();
            for clock in &mut actual {
                clock.update_120().unwrap();
                assert_eq!(clock.launcher.bar, launcher.bar);
                assert!(
                    launcher.beat.lower <= clock.launcher.beat
                        && clock.launcher.beat <= launcher.beat.upper
                );
                assert_eq!(clock.music.bar, music.bar);
                assert!(
                    music.beat.lower <= clock.music.beat && clock.music.beat <= music.beat.upper
                );
                if let Some(gate) = gate {
                    assert_eq!(launch_gate(position, *clock).unwrap(), gate);
                }
            }
        }
        assert!(certified_frames > 50);
    }
}
