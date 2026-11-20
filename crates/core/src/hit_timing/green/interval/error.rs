//! Uniform IEEE binary32 error budget for fixed-BPM music and launcher clocks.
//!
//! The affine reference uses the *native f32 step* as an exact real number.
//! It is not a replacement for the native recurrence. All bounds include
//! every standard speed in the block without evaluating a speed variant.
use super::{
    activation::activation_substep_count_bounds, launch::constant_bpm_launcher_position_at_frames,
};
use super::{arrival_seconds_bounds, initial_launcher_lead_bounds, FrameRange, SpeedBlock};
use crate::hit_timing::{
    green::clock::{FRAME_SECONDS, MUSIC_BAR_DIVISION_COUNT},
    ReplayError, MAX_SECONDS, REPLAY_FPS,
};

const UNIT_ROUNDOFF: f64 = 1.0 / 16_777_216.0; // 2^-24
const ADDITION_ABSOLUTE_ERROR: f64 = 1.0 / 131_072.0; // 2^-17
const INITIAL_WRAP_ERROR: f64 = 6.0 / 32_768.0; // six * 2^-15
const FRAME_ROUND_PAD: f64 = 1e-6;

/// For 0 <= n <= max_frames and A = native arrival seconds at any speed in
/// `block`, the native absolute ticks satisfy:
///
/// ```text
/// |music(n) - n * step| <= clock_ticks
/// |launcher(n) - (lead(A) + n * step)| <= clock_ticks
/// |lead(A) - 120 * A * step| <= lead_affine_ticks
/// ```
///
/// Multiplication in these statements is exact real arithmetic on the stored
/// f32 values. The output is an outward budget, never an estimated RMS error.
#[derive(Clone, Copy, Debug)]
pub struct ConstantBpmClockError {
    pub block: SpeedBlock,
    pub basic_bpm: f32,
    pub step_ticks: f32,
    pub max_frames: u32,
    pub clock_ticks: f64,
    pub lead_affine_ticks: f64,
}

/// Conditional envelope for an active note with note BPM equal to the fixed
/// music BPM. Let `c` be its activation input frame and `h=c+k`, with
/// `k<=max_regular_updates`. `absolute_tick` is an exactly representable
/// native integer position. If the exact pre-rounded launcher overshoot
/// `(launcher(c)-T)/lead` is at most `max_target_ratio` and native progress
/// stays at most `max_progress`, then:
///
/// ```text
/// A*progress(h) - (A + h/120 - T/(120*step))
///     in [-early_seconds, late_seconds].
/// ```
///
/// These two premises must be certified by the caller's chart-state analysis;
/// this function does not inspect trajectories or silently assume them.
#[derive(Clone, Copy, Debug)]
pub struct ConstantBpmMotionTimeError {
    pub block: SpeedBlock,
    pub step_ticks: f32,
    pub early_seconds: f64,
    pub late_seconds: f64,
    pub max_regular_updates: u32,
    pub max_progress: f64,
    pub max_target_ratio: f64,
}

pub fn constant_bpm_motion_time_error(
    block: SpeedBlock,
    basic_bpm: f32,
    absolute_tick: i32,
    clock: ConstantBpmClockError,
    max_regular_updates: u32,
    max_progress: f64,
    max_target_ratio: f64,
) -> Result<ConstantBpmMotionTimeError, ReplayError> {
    if !(192..16_777_216).contains(&absolute_tick)
        || !max_progress.is_finite()
        || max_progress <= 0.0
        || !max_target_ratio.is_finite()
        || max_target_ratio < 0.0
        || max_regular_updates > clock.max_frames
    {
        return Err(ReplayError::Input("invalid native motion error premises"));
    }
    let expected = FRAME_SECONDS / ((240.0f32 / basic_bpm) / MUSIC_BAR_DIVISION_COUNT as f32);
    if clock.block != block
        || clock.basic_bpm.to_bits() != basic_bpm.to_bits()
        || clock.step_ticks.to_bits() != expected.to_bits()
    {
        return Err(ReplayError::Input("motion and clock BPM disagree"));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let lead = initial_launcher_lead_bounds(block, basic_bpm)?;
    let a_min = f64::from(arrival.lower);
    let a_max = f64::from(arrival.upper);
    let l_min = f64::from(lead.lower);
    let rate = 120.0 * f64::from(clock.step_ticks);
    let safe_rate = rate - clock.lead_affine_ticks / a_min;
    if safe_rate <= 0.0 || l_min <= clock.clock_ticks {
        return Err(ReplayError::Input("native motion clock error is too wide"));
    }
    // Write L = 120*A*step + lambda and P = L + c*step + epsilon.
    // |lambda| and |epsilon| are bounded by clock. For D=P-T>=0 and
    // D/L<=Q, |c*step-T|<= (Q+1)*L+|epsilon|. This eliminates c and A
    // from the large terms before applying any independent interval hull.
    let activation_clock =
        (max_target_ratio + 1.0 + clock.clock_ticks / l_min) * clock.lead_affine_ticks / rate
            + clock.clock_ticks / safe_rate;
    // O=fl(P-T), target=fl(O/L): two normal f32 roundings. 4u*Q is
    // an outward relative bound, including the exact-zero case.
    let target_round = 4.0 * UNIT_ROUNDOFF * max_target_ratio * a_max;
    let base = activation_clock + target_round + UNIT_ROUNDOFF;

    let x = (basic_bpm * MUSIC_BAR_DIVISION_COUNT as f32) / 14_400.0f32;
    let substep = 1.0f32 / (120.0f32 + x * 60.0f32);
    if !substep.is_finite() || substep <= 0.0 {
        return Err(ReplayError::Input("invalid native activation substep"));
    }
    // The activation loop stops on its first progress>=target. Its excess
    // cannot exceed one rounded substep plus one progress addition rounding.
    let target_upper = max_target_ratio * (1.0 + 4.0 * UNIT_ROUNDOFF);
    let substep_seconds = f64::from(substep) * (1.0 + UNIT_ROUNDOFF);
    let activation_excess =
        substep_seconds + UNIT_ROUNDOFF * (a_max * target_upper + substep_seconds) + UNIT_ROUNDOFF;

    // Each ordinary move adds fl(FRAME_SECONDS/A) to native progress.
    // Multiplying by A differs from 1/120 by <3u*FRAME_SECONDS. The
    // progress addition contributes <2u*Amax*(Pmax+dmax) seconds.
    let max_progress_step = f64::from(FRAME_SECONDS) / a_min * (1.0 + UNIT_ROUNDOFF);
    let per_regular_step = 3.0 * UNIT_ROUNDOFF * f64::from(FRAME_SECONDS)
        + 2.0 * UNIT_ROUNDOFF * a_max * (max_progress + max_progress_step);
    let motion_round = f64::from(max_regular_updates) * per_regular_step;
    Ok(ConstantBpmMotionTimeError {
        block,
        step_ticks: clock.step_ticks,
        early_seconds: base + motion_round,
        late_seconds: base + activation_excess + motion_round,
        max_regular_updates,
        max_progress,
        max_target_ratio,
    })
}

/// Conditional bound for the 60 Hz judge-table progress recurrence. For every
/// sample index `i<=max_samples` whose native progress is `<=max_progress`,
/// `|A*progress_i - i/60| <= seconds`. The first sample has zero progress;
/// later samples repeatedly add `fl(delta_60/A)`. Geometry and powf only
/// decide which sample is the judge-line crossing, not this time bound.
#[derive(Clone, Copy, Debug)]
pub struct JudgeTableProgressTimeError {
    pub block: SpeedBlock,
    pub seconds: f64,
    pub max_samples: u32,
    pub max_progress: f64,
}

pub fn judge_table_progress_time_error(
    block: SpeedBlock,
    max_samples: u32,
    max_progress: f64,
) -> Result<JudgeTableProgressTimeError, ReplayError> {
    if max_samples > MAX_SECONDS * 60 || !max_progress.is_finite() || max_progress <= 0.0 {
        return Err(ReplayError::Input("invalid native table error premises"));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let a_min = f64::from(arrival.lower);
    let a_max = f64::from(arrival.upper);
    let delta = f64::from(f32::from_bits(0x3c888889));
    let max_step = delta / a_min * (1.0 + UNIT_ROUNDOFF);
    let per_sample =
        3.0 * UNIT_ROUNDOFF * delta + 2.0 * UNIT_ROUNDOFF * a_max * (max_progress + max_step);
    Ok(JudgeTableProgressTimeError {
        block,
        seconds: f64::from(max_samples) * per_sample + UNIT_ROUNDOFF,
        max_samples,
        max_progress,
    })
}

/// Close the 600-sample / progress<=2 premises of the fixed-geometry 60 Hz
/// table without running any speed. This is conditional on the one-ULP powf
/// premise used by `conditional_one_ulp_position_margins`.
pub fn conditional_one_ulp_judge_table_error(
    block: SpeedBlock,
    start_y: f32,
    button_world_y: f32,
    button_local_y: f32,
) -> Result<(JudgeTableProgressTimeError, PositionComparisonMargins), ReplayError> {
    let margins = conditional_one_ulp_position_margins(start_y, button_world_y, button_local_y)?;
    let table = judge_table_progress_time_error(block, 600, 2.0)?;
    let arrival = arrival_seconds_bounds(block)?;
    let a_min = f64::from(arrival.lower);
    let a_max = f64::from(arrival.upper);
    let delta = f64::from(f32::from_bits(0x3c888889));
    let progress_step = delta / a_min * (1.0 + UNIT_ROUNDOFF);
    let first_stop_progress = 1.6 + progress_step + UNIT_ROUNDOFF * (1.6 + progress_step);
    // For p<1.6 the table has not necessarily stopped, but every update
    // increases p by at least delta/Amax minus one addition rounding, which
    // exceeds the certified 10^-4 comparison margin. At the first p>=1.6,
    // native exponent is >29, so factor>15*(1-2u) and Y is below
    // button_local_y-100 in this geometry. The first overshoot stays <2.
    let minimum_step =
        delta / a_max * (1.0 - UNIT_ROUNDOFF) - UNIT_ROUNDOFF * (1.6 + progress_step);
    // If sample 600 were reached while p<1.6, the table-time envelope would
    // require A*p>=10-table.seconds, contradicting A*p<1.6*Amax.
    let horizon_closed = 10.0 - table.seconds > 1.6 * a_max;
    // At p<=1.5, the factor is <12 and Y remains above the stop line. The
    // crossing sample C has p near 1. Seven further samples remain below
    // 1.5 by the first-crossing phase and 60 Hz time-error inequalities.
    let post_center_upper =
        1.0 + margins.judge_line_progress + (8.0 / 60.0 + 2.0 * table.seconds) / a_min;
    if first_stop_progress >= 2.0
        || minimum_step <= margins.sample_order_progress
        || !horizon_closed
        || post_center_upper >= 1.5
    {
        return Err(ReplayError::Input(
            "native judge-table horizon is not certified",
        ));
    }
    judge_crossing_sample_bounds(block, table, margins.judge_line_progress)?;
    Ok((table, margins))
}

/// These margins are obligations on the selected Android `powf` binary and
/// the final geometry, not facts inferred from a host `powf` experiment.
/// `judge_line_progress` means Y(p)>button for p<1-margin and Y(p)<=button
/// for p>1+margin. `sample_order_progress` means Y(p)>Y(t) for p<t-margin
/// and Y(p)<=Y(t) for p>t+margin over the relevant native progress domain.
#[derive(Clone, Copy, Debug)]
pub struct PositionComparisonMargins {
    pub judge_line_progress: f64,
    pub sample_order_progress: f64,
}

/// Certify the final 184 SPECIAL geometry's progress-comparison margins,
/// *conditional on* the selected Android ARM32 powf returning within one ULP
/// of 1.1f32^e for every native exponent generated by progress in [0, 2].
/// This does not establish that premise for any device binary.
///
/// For two stored f32 progresses separated by 10^-4, their native exponents
/// differ by at least 50*10^-4 - 104u. The exact base's logarithm exceeds
/// 0.095, so their exact factors differ by at least a 0.00047 ratio. Over the
/// exponent domain [-51,51], factors lie in [1/200,200]. This certifies the
/// table/sample order margin independently of the judge-line margin.
///
/// For the judge line, progress 1 +/- 2e-7 is within Sterbenz's range, so
/// p-1 is exact. The following multiplication by 50 has relative error <=u.
/// Thus the magnitude of its exponent is >=50*2e-7*(1-u), and the exact
/// factor is <=1/(1+x) or >=1+x, where x=0.095*50*2e-7*(1-u).
/// One-ULP powf, the precomputed distance and its product have combined
/// relative error <6u. The final subtraction has absolute error bounded by
/// u*(5+8.5*(1+6u)*factor). The inequalities below check both sides of the
/// button comparison at the nearest boundary; farther progress only widens
/// the gap because its factor is monotone and the remaining coefficient is
/// positive. This is still conditional on the selected Android powf binary.
pub fn conditional_one_ulp_position_margins(
    start_y: f32,
    button_world_y: f32,
    button_local_y: f32,
) -> Result<PositionComparisonMargins, ReplayError> {
    let distance = start_y - button_world_y;
    if !start_y.is_finite()
        || !button_world_y.is_finite()
        || button_world_y.to_bits() != button_local_y.to_bits()
        || !(4.9..=5.0).contains(&start_y)
        || !(-3.5..=-3.4).contains(&button_world_y)
        || !(8.4..=8.5).contains(&distance)
    {
        return Err(ReplayError::Input(
            "geometry is outside conditional one-ULP proof",
        ));
    }
    const SAMPLE_ORDER_MARGIN: f64 = 0.0001;
    const JUDGE_LINE_MARGIN: f64 = 0.0000002;
    let native_exponent_gap = 50.0 * SAMPLE_ORDER_MARGIN - 104.0 * UNIT_ROUNDOFF;
    let factor_gain = 0.095 * native_exponent_gap;
    let relative_arithmetic = 6.0 * UNIT_ROUNDOFF;
    // If F_high >= (1+gain)*F_low, each powf+multiply result differs from
    // its exact product by <6u relative. Two final Y subtractions contribute
    // <=2u*start_y + u*(product_low+product_high). Divide by D*F_low and
    // use D>=8.4, F_low>=1/200 and start_y<=5.
    let retained_factor_gap =
        (1.0 + factor_gain) * (1.0 - relative_arithmetic) - (1.0 + relative_arithmetic);
    let y_round_ratio = 2.0 * UNIT_ROUNDOFF * 5.0 / (8.4 * 0.005)
        + UNIT_ROUNDOFF * (2.0 + factor_gain) * (1.0 + relative_arithmetic);
    let line_gain = 0.095 * 50.0 * JUDGE_LINE_MARGIN * (1.0 - UNIT_ROUNDOFF);
    let below_factor_max = 1.0 / (1.0 + line_gain);
    let above_factor_min = 1.0 + line_gain;
    let below_button_gap = 8.4 * (1.0 - (1.0 + relative_arithmetic) * below_factor_max)
        - UNIT_ROUNDOFF * (5.0 + 8.5 * (1.0 + relative_arithmetic) * below_factor_max);
    let above_button_gap = 8.4 * ((1.0 - relative_arithmetic) * above_factor_min - 1.0)
        - UNIT_ROUNDOFF * (5.0 + 8.5 * (1.0 + relative_arithmetic) * above_factor_min);
    if retained_factor_gap <= y_round_ratio || below_button_gap <= 0.0 || above_button_gap <= 0.0 {
        return Err(ReplayError::Input(
            "one-ULP geometry margin inequality did not close",
        ));
    }
    Ok(PositionComparisonMargins {
        judge_line_progress: JUDGE_LINE_MARGIN,
        sample_order_progress: SAMPLE_ORDER_MARGIN,
    })
}

/// Conditional integer range for the 60 Hz judge-line crossing sample. The
/// supplied powf/geometry margin and table-progress premises remain external
/// proof obligations. This range can guide adaptive speed-block refinement.
pub fn judge_crossing_sample_bounds(
    block: SpeedBlock,
    table: JudgeTableProgressTimeError,
    judge_line_margin: f64,
) -> Result<(u32, u32), ReplayError> {
    if table.block != block
        || !judge_line_margin.is_finite()
        || !(0.0..1.0).contains(&judge_line_margin)
    {
        return Err(ReplayError::Input("invalid judge-line phase premises"));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let a_min = f64::from(arrival.lower);
    let a_max = f64::from(arrival.upper);
    let c_min = (60.0 * (a_min * (1.0 - judge_line_margin) - table.seconds) - FRAME_ROUND_PAD)
        .ceil() as i32;
    let c_max = (60.0 * (a_max * (1.0 + judge_line_margin) + table.seconds) + 1.0 + FRAME_ROUND_PAD)
        .floor() as i32;
    if c_min < 8 || c_max < c_min || c_max + 7 > table.max_samples as i32 {
        return Err(ReplayError::Input(
            "judge-line sample index exceeds proof premises",
        ));
    }
    // C is the first Y<=button sample. Its p>=1-margin, while the prior
    // sample has p<=1+margin. The time error gives a coarse integer bracket.
    // The table recurrence is monotone in arrival A: sample 0 is zero,
    // sample 1 is fl(delta/A), and all later samples add that same rounded
    // positive increment. Propagating only Amax and Amin therefore gives
    // exact binary32 endpoint bounds for *every* speed in the block. This is
    // endpoint interval transfer, not a speed-variant walk.
    let delta = f32::from_bits(0x3c888889);
    let increment_lower = delta / arrival.upper;
    let increment_upper = delta / arrival.lower;
    let mut progress_lower = 0.0f32;
    let mut progress_upper = 0.0f32;
    let mut refined_min = c_min as u32;
    let mut refined_max = c_max as u32;
    for sample in 0..=c_max as u32 {
        if sample == 1 {
            progress_lower = increment_lower;
            progress_upper = increment_upper;
        } else if sample > 1 {
            progress_lower += increment_lower;
            progress_upper += increment_upper;
        }
        if sample < c_min as u32 {
            continue;
        }
        if f64::from(progress_upper) < 1.0 - judge_line_margin {
            refined_min = refined_min.max(sample + 1);
        }
        if f64::from(progress_lower) <= table.max_progress
            && f64::from(progress_lower) > 1.0 + judge_line_margin
        {
            refined_max = refined_max.min(sample);
        }
    }
    if refined_min > refined_max {
        return Err(ReplayError::Input(
            "empty native judge-line sample interval",
        ));
    }
    Ok((refined_min, refined_max))
}

fn judge_crossing_phase_bounds(
    block: SpeedBlock,
    table: JudgeTableProgressTimeError,
    judge_line_margin: f64,
) -> Result<(f64, f64), ReplayError> {
    let (c_min, c_max) = judge_crossing_sample_bounds(block, table, judge_line_margin)?;
    let arrival = arrival_seconds_bounds(block)?;
    let a_min = f64::from(arrival.lower);
    let a_max = f64::from(arrival.upper);
    // Intersect the index-derived phase with the generic one-sample phase.
    let generic_lower = -a_max * judge_line_margin - table.seconds;
    let generic_upper = 1.0 / 60.0 + a_max * judge_line_margin + table.seconds;
    let lower = generic_lower.max(f64::from(c_min) / 60.0 - a_max);
    let upper = generic_upper.min(f64::from(c_max) / 60.0 - a_min);
    if lower > upper {
        return Err(ReplayError::Input("empty judge-line phase interval"));
    }
    Ok((lower, upper))
}

/// Native binary32 progress at the first judge-line table sample, enclosed
/// over the crossing-index interval. This keeps the exact 60 Hz recurrence
/// when a block has a narrow or singleton crossing sample.
fn judge_center_progress_bounds(
    block: SpeedBlock,
    table: JudgeTableProgressTimeError,
    judge_line_margin: f64,
) -> Result<(f32, f32), ReplayError> {
    let (first, last) = judge_crossing_sample_bounds(block, table, judge_line_margin)?;
    let arrival = arrival_seconds_bounds(block)?;
    let delta = f32::from_bits(0x3c888889);
    let increment_lower = delta / arrival.upper;
    let increment_upper = delta / arrival.lower;
    let mut lower = 0.0f32;
    let mut upper = 0.0f32;
    let mut at_first = None;
    for sample in 1..=last {
        lower += increment_lower;
        upper += increment_upper;
        if sample == first {
            at_first = Some(lower);
        }
    }
    Ok((
        at_first.ok_or(ReplayError::Input("missing judge centre progress"))?,
        upper,
    ))
}

/// Prove that one native grade gate has not been reached at an input frame.
/// This uses the same binary32 activation and ordinary-move recurrences as
/// the native path, with interval endpoints over an entire speed block. It
/// can discharge a score-relevant frame left uncertain by the uniform time
/// error budget. A positive result proves exclusion; zero or a negative
/// result is inconclusive, never evidence of an early hit.
///
/// The launcher-position and activation-frame enclosures must describe this
/// node's actual first activation. The caller must separately establish the
/// one-ULP position-comparison premise represented by `margins`.
pub fn conditional_gate_progress_separation_at_frame(
    block: SpeedBlock,
    basic_bpm: f32,
    absolute_tick: i32,
    activation_frames: FrameRange,
    input_frame: u32,
    sample_offset: i32,
    table: JudgeTableProgressTimeError,
    margins: PositionComparisonMargins,
) -> Result<f64, ReplayError> {
    if table.block != block
        || !(192..16_777_216).contains(&absolute_tick)
        || !(-8..=7).contains(&sample_offset)
        || activation_frames.lower > activation_frames.upper
        || input_frame > MAX_SECONDS * REPLAY_FPS
        || !margins.sample_order_progress.is_finite()
        || margins.sample_order_progress < 0.0
    {
        return Err(ReplayError::Input(
            "invalid fixed-frame green gate premises",
        ));
    }
    if input_frame < activation_frames.lower {
        return Ok(f64::INFINITY);
    }
    let (center_first, center_last) =
        judge_crossing_sample_bounds(block, table, margins.judge_line_progress)?;
    let sample_first = center_first as i32 + sample_offset;
    let sample_last = center_last as i32 + sample_offset;
    if sample_first < 1 || sample_last > table.max_samples as i32 {
        return Err(ReplayError::Input("fixed-frame judge sample is absent"));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let table_increment_lower = f32::from_bits(0x3c888889) / arrival.upper;
    let mut table_lower = 0.0f32;
    for _ in 1..=sample_first {
        table_lower += table_increment_lower;
    }
    let threshold_lower = table_lower;
    let ordinary_upper = FRAME_SECONDS / arrival.lower;
    let mut latest_progress = 0.0f32;
    for activation in activation_frames.lower..=activation_frames.upper.min(input_frame) {
        let position = constant_bpm_launcher_position_at_frames(
            block,
            basic_bpm,
            FrameRange::new(activation, activation)?,
        )?;
        let seed = activation_substep_count_bounds(block, position, absolute_tick, basic_bpm)?;
        if seed.lower == 0 {
            // A zero seed uses native CalcProgressRate's initial branch on
            // its first ordinary move; that branch needs a separate proof.
            return Ok(f64::NEG_INFINITY);
        }
        let mut progress = seed.seed_progress.upper;
        for _ in activation..input_frame {
            progress += ordinary_upper;
        }
        latest_progress = latest_progress.max(progress);
    }
    let separation =
        f64::from(threshold_lower) - f64::from(latest_progress) - margins.sample_order_progress;
    Ok(separation)
}

/// Conditional first frame at/below table sample C+offset, where C is the
/// table's first judge-line crossing. Requires the comparison margins above,
/// the table/motion premises, 16 retained samples around C, and the note to
/// stay active in Move until this gate is crossed. It does not certify any of
/// those conditions; the returned bracket is a theorem *under* them.
pub fn conditional_grade_gate_frames(
    block: SpeedBlock,
    absolute_tick: i32,
    sample_offset: i32,
    activation_frames: FrameRange,
    motion: ConstantBpmMotionTimeError,
    table: JudgeTableProgressTimeError,
    margins: PositionComparisonMargins,
    step_ticks: f32,
) -> Result<FrameRange, ReplayError> {
    if motion.block != block
        || table.block != block
        || motion.step_ticks.to_bits() != step_ticks.to_bits()
        || !(192..16_777_216).contains(&absolute_tick)
        || !(-8..=7).contains(&sample_offset)
        || activation_frames.lower > activation_frames.upper
        || !step_ticks.is_finite()
        || step_ticks <= 0.0
        || !margins.judge_line_progress.is_finite()
        || margins.judge_line_progress < 0.0
        || !margins.sample_order_progress.is_finite()
        || margins.sample_order_progress < 0.0
    {
        return Err(ReplayError::Input("invalid native grade-frame premises"));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let a_max = f64::from(arrival.upper);
    let (phase_lower, phase_upper) =
        judge_crossing_phase_bounds(block, table, margins.judge_line_progress)?;
    // One table recurrence error for sample C+offset, plus the separate
    // position-comparison margin. The crossing phase retains speed-block
    // information whenever the possible integer C range narrows.
    let sample_uncertainty = a_max * margins.sample_order_progress + table.seconds;
    let chart_seconds = f64::from(absolute_tick) / (120.0 * f64::from(step_ticks));
    let possible_first = 120.0
        * (chart_seconds + phase_lower + f64::from(sample_offset) / 60.0
            - sample_uncertainty
            - motion.late_seconds);
    let guaranteed_by = 120.0
        * (chart_seconds
            + phase_upper
            + f64::from(sample_offset) / 60.0
            + sample_uncertainty
            + motion.early_seconds);
    if !possible_first.is_finite() || !guaranteed_by.is_finite() {
        return Err(ReplayError::Input("native grade-frame bound overflow"));
    }
    let max_frame = f64::from(MAX_SECONDS * REPLAY_FPS);
    if possible_first.abs() > max_frame || guaranteed_by.abs() > max_frame {
        return Err(ReplayError::Input(
            "native grade frame exceeds proof horizon",
        ));
    }
    // Every input integer/f32 is exact in f64. The bounded 72,000-frame
    // arithmetic uses fewer than 32 operations, so 10^-6 of one frame is a
    // conservative outward allowance for host binary64 rounding. The upper
    // comparison is strict, hence floor(bound)+1 even at integer equality.
    let lower = (possible_first - FRAME_ROUND_PAD).ceil().max(0.0) as u32;
    let upper = (guaranteed_by + FRAME_ROUND_PAD).floor().max(0.0) as u32 + 1;
    FrameRange::new(
        lower.max(activation_frames.lower),
        upper.max(activation_frames.upper),
    )
}

#[derive(Clone, Copy, Debug)]
pub enum StopPositionGate {
    /// Head and terminal after-node stop at the centre table sample.
    TableSample8,
    /// Internal after-node stop at the fixed local button line.
    ButtonLine,
}

/// Conditional first stop frame for the conjunction `progress>1` and the
/// native position gate. Requires the same unproved powf/domain premises as
/// conditional_grade_gate_frames. The caller can then check that this upper
/// frame lies within the motion budget's step horizon from every activation.
pub fn conditional_stop_frame_bounds(
    block: SpeedBlock,
    absolute_tick: i32,
    activation_frames: FrameRange,
    motion: ConstantBpmMotionTimeError,
    table: JudgeTableProgressTimeError,
    margins: PositionComparisonMargins,
    gate: StopPositionGate,
    step_ticks: f32,
) -> Result<FrameRange, ReplayError> {
    if motion.block != block
        || table.block != block
        || motion.step_ticks.to_bits() != step_ticks.to_bits()
        || !(192..16_777_216).contains(&absolute_tick)
        || !step_ticks.is_finite()
        || step_ticks <= 0.0
        || !margins.judge_line_progress.is_finite()
        || margins.judge_line_progress < 0.0
    {
        return Err(ReplayError::Input("invalid native stop-frame premises"));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let a_max = f64::from(arrival.upper);
    let chart_seconds = f64::from(absolute_tick) / (120.0 * f64::from(step_ticks));
    if !chart_seconds.is_finite() || chart_seconds > f64::from(MAX_SECONDS) {
        return Err(ReplayError::Input(
            "native stop frame exceeds proof horizon",
        ));
    }
    let strict_progress = FrameRange::new(
        (120.0 * (chart_seconds - motion.late_seconds) - FRAME_ROUND_PAD)
            .floor()
            .max(0.0) as u32
            + 1,
        (120.0 * (chart_seconds + motion.early_seconds) + FRAME_ROUND_PAD)
            .floor()
            .max(0.0) as u32
            + 1,
    )?;
    let position = match gate {
        StopPositionGate::TableSample8 => conditional_grade_gate_frames(
            block,
            absolute_tick,
            0,
            activation_frames,
            motion,
            table,
            margins,
            step_ticks,
        )?,
        StopPositionGate::ButtonLine => {
            let possible =
                120.0 * (chart_seconds - a_max * margins.judge_line_progress - motion.late_seconds);
            let guaranteed = 120.0
                * (chart_seconds + a_max * margins.judge_line_progress + motion.early_seconds);
            FrameRange::new(
                (possible - FRAME_ROUND_PAD).ceil().max(0.0) as u32,
                (guaranteed + FRAME_ROUND_PAD).floor().max(0.0) as u32 + 1,
            )?
        }
    };
    FrameRange::new(
        position
            .lower
            .max(strict_progress.lower)
            .max(activation_frames.lower),
        position
            .upper
            .max(strict_progress.upper)
            .max(activation_frames.upper),
    )
}

/// A sufficient, conditional proof that an internal after-node cannot stop
/// before its root. Both nodes share the same arrival, lead, launcher clock,
/// BPM and activation substep. Comparing their *difference* cancels the large
/// absolute launcher-clock error; only the few updates between activations
/// contribute clock rounding. `root_stop` and `after_stop` must already be
/// certified conditional brackets, and the same progress/powf premises as the
/// grade-gate proof apply. The caller must supply a native chain in which the
/// after-node activates strictly after the root at each individual speed, as
/// `constant_bpm_activation_ratio_bound` establishes. `None` means the bound
/// is inconclusive.
pub fn conditional_root_before_after_stop(
    block: SpeedBlock,
    root_tick: i32,
    after_tick: i32,
    root_activation: FrameRange,
    after_activation: FrameRange,
    root_stop: FrameRange,
    after_stop: FrameRange,
    motion: ConstantBpmMotionTimeError,
    table: JudgeTableProgressTimeError,
    margins: PositionComparisonMargins,
    basic_bpm: f32,
    clock: ConstantBpmClockError,
) -> Result<Option<f64>, ReplayError> {
    if after_tick <= root_tick
        || root_tick < 192
        || after_tick >= 16_777_216
        || motion.block != block
        || table.block != block
        || clock.block != block
        || motion.step_ticks.to_bits() != clock.step_ticks.to_bits()
        || clock.basic_bpm.to_bits() != basic_bpm.to_bits()
        || after_activation.upper < root_activation.lower
        || root_stop.upper < root_activation.lower
        || after_stop.upper < after_activation.lower
        || !margins.sample_order_progress.is_finite()
        || margins.sample_order_progress < 0.0
    {
        return Err(ReplayError::Input("invalid paired stop-order premises"));
    }
    if after_stop.lower >= root_stop.upper {
        return Ok(Some(f64::INFINITY));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let lead = initial_launcher_lead_bounds(block, basic_bpm)?;
    let a_min = f64::from(arrival.lower);
    let a_max = f64::from(arrival.upper);
    let lead_min = f64::from(lead.lower);
    let lead_max = f64::from(lead.upper);
    let step = f64::from(clock.step_ticks);
    let rate = 120.0 * step;
    let frame_seconds = f64::from(FRAME_SECONDS);
    let delta_ticks = f64::from(after_tick - root_tick);
    let between = f64::from(after_activation.upper - root_activation.lower);
    let root_updates = f64::from(root_stop.upper - root_activation.lower);
    let after_updates = f64::from(after_stop.upper - after_activation.lower);
    if root_updates > f64::from(motion.max_regular_updates)
        || after_updates > f64::from(motion.max_regular_updates)
        || lead_min <= 0.0
    {
        return Err(ReplayError::Input("paired stop exceeds motion horizon"));
    }
    // P is the f32 AbsolutePosition used by activateAdjust. The underlying
    // beat advances by `step` with <=2^-17 tick error per update; subtracting
    // 192 on wrap is exact. Each final absolute-position addition costs at
    // most u*(T_after + lead_max*Q + 1) ticks. This bound concerns the local
    // difference P_after-P_root, never the full accumulated clock drift.
    let max_abs = f64::from(after_tick) + lead_max * motion.max_target_ratio + 1.0;
    let launcher_difference_error =
        between * ADDITION_ABSOLUTE_ERROR + 2.0 * UNIT_ROUNDOFF * max_abs;
    // A/L differs from 1/(120*step) by |L-120*A*step|/(rate*L).
    // Apply it to the *difference* of the two activation numerators.
    let lead_ratio_error = clock.lead_affine_ticks
        * (delta_ticks + between * step + launcher_difference_error)
        / (rate * lead_min);
    // Each target is fl(fl(P-T)/L); normal binary32 rounding costs <3u
    // relative to its nonnegative exact numerator. The after-node may
    // overshoot its target by at most one rounded activation substep.
    let target_round = 6.0 * UNIT_ROUNDOFF * a_max * motion.max_target_ratio;
    let x = (basic_bpm * MUSIC_BAR_DIVISION_COUNT as f32) / 14_400.0f32;
    let substep = 1.0f32 / (120.0f32 + x * 60.0f32);
    if !substep.is_finite() || substep <= 0.0 {
        return Err(ReplayError::Input("invalid paired activation substep"));
    }
    let substep = f64::from(substep);
    let max_substep_progress = substep / a_min * (1.0 + UNIT_ROUNDOFF);
    let activation_overshoot = substep
        + 2.0 * UNIT_ROUNDOFF * substep
        + 2.0 * UNIT_ROUNDOFF * a_max * (motion.max_target_ratio + max_substep_progress);
    let (_, phase_upper) = judge_crossing_phase_bounds(block, table, margins.judge_line_progress)?;
    // For one ordinary update, A*fl(p+fl(d/A))-A*p differs from d by
    // <=u*d + u*A*(p+fl(d/A)); use 2u on both terms as an outward allowance.
    let max_regular_progress = frame_seconds / a_min * (1.0 + UNIT_ROUNDOFF);
    // The general motion certificate permits progress up to 2. For this
    // paired proof only the first stop matters. The centre-table progress is
    // <=1+(phase_upper+table_error)/Amin. Root and after stop are guaranteed
    // by that centre (or the fixed button line), with at most one ordinary
    // increment beyond the threshold. Activation can also start beyond it;
    // use the larger activation ceiling. A loose increment rounding bound
    // based on the already-closed progress<=2 premise gives a noncircular
    // first-stop ceiling below 1.1 for this chart.
    let activation_progress = motion.max_target_ratio * (1.0 + 3.0 * UNIT_ROUNDOFF)
        + max_substep_progress * (1.0 + 2.0 * UNIT_ROUNDOFF);
    let table_center_progress =
        1.0 + (phase_upper + table.seconds) / a_min + margins.sample_order_progress;
    let rounded_regular_step =
        max_regular_progress + UNIT_ROUNDOFF * (motion.max_progress + max_regular_progress);
    let first_stop_progress = activation_progress
        .max(table_center_progress)
        .max(1.0 + margins.judge_line_progress)
        + rounded_regular_step;
    let paired_progress = if first_stop_progress < 1.1 {
        1.1
    } else {
        motion.max_progress
    };
    let per_regular = 2.0 * UNIT_ROUNDOFF * frame_seconds
        + 2.0 * UNIT_ROUNDOFF * a_max * (paired_progress + max_regular_progress);
    let regular_round = (root_updates + after_updates) * per_regular;
    let frame_step_round = between * (frame_seconds - 1.0 / 120.0).abs();
    let difference_lower = delta_ticks / rate
        - launcher_difference_error / rate
        - lead_ratio_error
        - target_round
        - activation_overshoot
        - regular_round
        - frame_step_round;
    // The root table centre is the first judge-line sample C. Its progress
    // time is <=C/60+table.seconds. If root progress exceeds that sample by
    // the geometry margin when the after-node reaches progress>1, root has
    // already met both native stop predicates on that frame.
    let required = phase_upper + table.seconds + a_max * margins.sample_order_progress;
    let slack = difference_lower - required;
    Ok((difference_lower > 0.0 && slack > FRAME_ROUND_PAD / 120.0).then_some(slack))
}

/// A second sufficient stop-order proof that preserves the shared launcher
/// frame gap and the two integer activateAdjust counts. The caller must have
/// certified that `branches` covers every same-speed activation pair, that
/// both nodes keep moving until their natural stop, and that the motion/table
/// progress and powf comparison premises hold. `None` is inconclusive.
pub fn conditional_discrete_root_before_after_stop(
    block: SpeedBlock,
    branches: &[super::activation::PairedActivationBranch],
    after_activation: FrameRange,
    after_stop: FrameRange,
    motion: ConstantBpmMotionTimeError,
    table: JudgeTableProgressTimeError,
    margins: PositionComparisonMargins,
) -> Result<Option<f64>, ReplayError> {
    if branches.is_empty()
        || motion.block != block
        || table.block != block
        || after_stop.upper < after_activation.lower
        || after_stop.upper - after_activation.lower > motion.max_regular_updates
        || !motion.max_progress.is_finite()
        || motion.max_progress <= 0.0
    {
        return Err(ReplayError::Input("invalid discrete paired stop premises"));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let regular_lower = FRAME_SECONDS / arrival.upper;
    let regular_upper = FRAME_SECONDS / arrival.lower;
    let (_, centre_upper) =
        judge_center_progress_bounds(block, table, margins.judge_line_progress)?;
    let required_progress = f64::from(centre_upper) - 1.0 + margins.sample_order_progress;
    let shared_updates = f64::from(after_stop.upper - after_activation.lower);
    // Before either first stop, root progress cannot exceed its centre table
    // sample plus the certified comparison margin, while an internal after
    // node cannot exceed the button line plus its margin. The first stopping
    // update adds at most one ordinary increment and one rounding. Activation
    // seeds are included for the case where they already cross a stop gate.
    let largest_seed = branches.iter().fold(0.0f64, |value, branch| {
        value
            .max(f64::from(branch.root_seed_progress.upper))
            .max(f64::from(branch.after_seed_progress.upper))
    });
    let before_stop_cap = largest_seed
        .max(f64::from(centre_upper) + margins.sample_order_progress)
        .max(1.0 + margins.judge_line_progress);
    let first_stop_cap = before_stop_cap
        + f64::from(regular_upper)
        + UNIT_ROUNDOFF * (before_stop_cap + f64::from(regular_upper))
        + 1e-8;
    if first_stop_cap > motion.max_progress {
        return Err(ReplayError::Input(
            "discrete paired progress cap exceeds motion premise",
        ));
    }
    // If the root has not stopped before the after-node, both take the same
    // binary32 ordinary increment at each subsequent frame. Each rounded
    // addition changes their progress difference by at most 2u(P+increment).
    let shared_rounding =
        shared_updates * 2.0 * UNIT_ROUNDOFF * (first_stop_cap + f64::from(regular_upper));
    let mut minimum_slack = f64::INFINITY;
    for branch in branches {
        if branch.frame_gap == 0 {
            return Err(ReplayError::Input("paired activation frame gap is zero"));
        }
        let mut root_at_after = branch.root_seed_progress.lower;
        for _ in 0..branch.frame_gap {
            root_at_after += regular_lower;
        }
        let difference_lower = f64::from(root_at_after)
            - f64::from(branch.after_seed_progress.upper)
            - shared_rounding;
        minimum_slack = minimum_slack.min(difference_lower - required_progress);
    }
    Ok((minimum_slack > 1e-10).then_some(minimum_slack))
}

/// Prove that the root has naturally stopped no later than the lower bound on
/// the after-node's stop frame. This endpoint transfer is useful when the
/// paired progress-difference bound is inconclusive at an integer frame.
/// It advances only the lower binary32 progress envelope from a certified
/// activation seed; it does not evaluate a speed variant or call powf.
pub fn conditional_root_stops_by_after_lower_frame(
    block: SpeedBlock,
    root_activation: FrameRange,
    root_seed_progress: super::F32Range,
    after_stop: FrameRange,
    motion: ConstantBpmMotionTimeError,
    table: JudgeTableProgressTimeError,
    margins: PositionComparisonMargins,
) -> Result<Option<u32>, ReplayError> {
    if motion.block != block
        || table.block != block
        || root_activation.lower > root_activation.upper
        || after_stop.lower > after_stop.upper
        || super::F32Range::new(root_seed_progress.lower, root_seed_progress.upper).is_err()
    {
        return Err(ReplayError::Input("invalid root frame-stop premises"));
    }
    if after_stop.lower <= root_activation.upper {
        return Ok(None);
    }
    let updates = after_stop.lower - root_activation.upper;
    if updates > motion.max_regular_updates {
        return Err(ReplayError::Input("root frame-stop exceeds motion premise"));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let ordinary_lower = FRAME_SECONDS / arrival.upper;
    let (_, centre_upper) =
        judge_center_progress_bounds(block, table, margins.judge_line_progress)?;
    let required = (f64::from(centre_upper) + margins.sample_order_progress).max(1.0);
    let mut progress_lower = root_seed_progress.lower;
    for offset in 1..=updates {
        progress_lower += ordinary_lower;
        if !progress_lower.is_finite() {
            return Err(ReplayError::Input("root progress envelope overflow"));
        }
        if f64::from(progress_lower) > required {
            return Ok(Some(root_activation.upper + offset));
        }
    }
    Ok(None)
}

pub fn constant_bpm_clock_error(
    block: SpeedBlock,
    basic_bpm: f32,
    max_frames: u32,
) -> Result<ConstantBpmClockError, ReplayError> {
    if !basic_bpm.is_finite() || basic_bpm < 1.0 || max_frames > MAX_SECONDS * REPLAY_FPS {
        return Err(ReplayError::Input(
            "clock error budget is outside proven range",
        ));
    }
    let arrival = arrival_seconds_bounds(block)?;
    let lead = initial_launcher_lead_bounds(block, basic_bpm)?;
    if lead.upper > 1024.0 {
        return Err(ReplayError::Input(
            "launcher lead exceeds proven wrap range",
        ));
    }
    let step = FRAME_SECONDS / ((240.0f32 / basic_bpm) / MUSIC_BAR_DIVISION_COUNT as f32);
    if !step.is_finite() || step <= 0.0 || step >= 64.0 {
        return Err(ReplayError::Input(
            "clock step exceeds proven rounding range",
        ));
    }
    // Beat is always in [0,192), and beat+step is below 256. Each f32
    // addition therefore differs from the exact sum by at most 2^-17 ticks.
    // A wrap subtracts 192 from a value in [192,256]; Sterbenz's lemma makes
    // that subtraction exact. Initial lead<=1024 needs at most five repeated
    // subtractions, each with <=2^-15 absolute error. We use six.
    let recurrent = f64::from(max_frames) * ADDITION_ABSOLUTE_ERROR;
    let max_raw = f64::from(lead.upper)
        + f64::from(max_frames) * f64::from(step)
        + INITIAL_WRAP_ERROR
        + recurrent;
    if max_raw >= 16_777_216.0 {
        return Err(ReplayError::Input(
            "clock absolute position exceeds exact integer range",
        ));
    }
    // The integer bar*192 converts exactly below 2^24. AbsolutePosition's
    // final addition has relative error <=u. A one-tick allowance protects
    // this f64 bound computation; the extra u covers any final sum rounding.
    let absolute = UNIT_ROUNDOFF * (max_raw + 1.0);
    let clock_ticks = INITIAL_WRAP_ERROR + recurrent + absolute + UNIT_ROUNDOFF;
    // lead = fl(fl(fl(A*B)/240)*192). Relative error is <4u.
    // step = fl(fl(1/120) / fl(fl(240/B)/192)); 120*step differs from
    // B*192/240 by <5u relative. Since 192/240<1, 10u*Amax*B is a strict
    // common bound for |lead - 120*A*step|. All intermediates are normal in
    // the accepted BPM and standard-speed domain.
    let lead_affine_ticks =
        10.0 * UNIT_ROUNDOFF * f64::from(arrival.upper) * f64::from(basic_bpm) + UNIT_ROUNDOFF;
    Ok(ConstantBpmClockError {
        block,
        basic_bpm,
        step_ticks: step,
        max_frames,
        clock_ticks,
        lead_affine_ticks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::green::activation::MovementState;
    use crate::hit_timing::green::clock::{initial_launcher_lead, MusicClock120};

    #[test]
    fn one_ulp_margin_certificate_is_geometry_specific() {
        let margin = conditional_one_ulp_position_margins(4.9765005, -3.45, -3.45).unwrap();
        assert_eq!(margin.judge_line_progress, 0.0000002);
        assert_eq!(margin.sample_order_progress, 0.0001);
        let (table, paired) =
            conditional_one_ulp_judge_table_error(SpeedBlock::all(), 4.9765005, -3.45, -3.45)
                .unwrap();
        assert_eq!(table.max_samples, 600);
        assert_eq!(table.max_progress, 2.0);
        assert_eq!(paired.judge_line_progress, margin.judge_line_progress);
        let q100 = SpeedBlock::new(100, 100).unwrap();
        let (table_q100, margin_q100) =
            conditional_one_ulp_judge_table_error(q100, 4.9765005, -3.45, -3.45).unwrap();
        assert_eq!(
            judge_crossing_sample_bounds(q100, table_q100, margin_q100.judge_line_progress)
                .unwrap(),
            (331, 331)
        );
        assert!(conditional_one_ulp_position_margins(4.9765005, -3.45, -3.44).is_err());
        assert!(conditional_one_ulp_position_margins(1000.0, 900.0, 900.0).is_err());
    }

    #[test]
    fn root_frame_transfer_closes_the_q100_three_tick_case() {
        let block = SpeedBlock::new(100, 100).unwrap();
        let clock_budget = constant_bpm_clock_error(block, 185.0, 13_000).unwrap();
        let motion =
            constant_bpm_motion_time_error(block, 185.0, 3168, clock_budget, 700, 2.0, 1.004)
                .unwrap();
        let (table, margins) =
            conditional_one_ulp_judge_table_error(block, 4.9765005, -3.45, -3.45).unwrap();
        let mut clock = MusicClock120::new(1.0, 185.0).unwrap();
        for _ in 0..1909 {
            clock.update_120().unwrap();
        }
        let mut root = MovementState::default();
        assert_eq!(
            root.activate(
                clock.launcher.absolute_position().unwrap(),
                3168,
                185.0,
                1.0
            )
            .unwrap(),
            1
        );
        let proven = conditional_root_stops_by_after_lower_frame(
            block,
            FrameRange::new(1909, 1909).unwrap(),
            super::super::F32Range::new(root.progress, root.progress).unwrap(),
            FrameRange::new(2571, 2572).unwrap(),
            motion,
            table,
            margins,
        )
        .unwrap();
        assert!(proven.is_some_and(|frame| frame <= 2571));
    }

    #[test]
    fn uniform_clock_budget_contains_experimental_native_traces() {
        let block = SpeedBlock::all();
        let budget = constant_bpm_clock_error(block, 185.0, 13_000).unwrap();
        assert!(budget.clock_ticks < 0.102);
        assert!(budget.lead_affine_ticks < 0.001);
        for q in [100, 347, 1000, 1101, 1102, 1200] {
            let speed = q as f32 / 100.0;
            let arrival = crate::hit_timing::green::motion::arrival_seconds(speed).unwrap();
            let lead = initial_launcher_lead(speed, 185.0).unwrap();
            let mut clock = MusicClock120::new(speed, 185.0).unwrap();
            for n in 0..=budget.max_frames {
                let reference = f64::from(n) * f64::from(budget.step_ticks);
                let music = f64::from(clock.music.absolute_position().unwrap());
                let launcher = f64::from(clock.launcher.absolute_position().unwrap());
                assert!(
                    (music - reference).abs() <= budget.clock_ticks,
                    "q={q} n={n}"
                );
                assert!(
                    (launcher - (f64::from(lead) + reference)).abs() <= budget.clock_ticks,
                    "q={q} n={n}"
                );
                if n < budget.max_frames {
                    clock.update_120().unwrap();
                }
            }
            assert!(
                (f64::from(lead) - 120.0 * f64::from(arrival) * f64::from(budget.step_ticks)).abs()
                    <= budget.lead_affine_ticks
            );
        }
    }

    #[test]
    fn activation_and_motion_budget_contains_experimental_traces() {
        let block = SpeedBlock::all();
        let clock_budget = constant_bpm_clock_error(block, 185.0, 13_000).unwrap();
        let motion_budget =
            constant_bpm_motion_time_error(block, 185.0, 384, clock_budget, 700, 2.0, 2.0).unwrap();
        let tick = 384i32;
        let rate = 120.0 * f64::from(clock_budget.step_ticks);
        for q in 100..=1200 {
            let speed = q as f32 / 100.0;
            let arrival = crate::hit_timing::green::motion::arrival_seconds(speed).unwrap();
            let lead = initial_launcher_lead(speed, 185.0).unwrap();
            let mut clock = MusicClock120::new(speed, 185.0).unwrap();
            let mut first = 0u32;
            loop {
                clock.update_120().unwrap();
                first += 1;
                if clock.launcher.absolute_position().unwrap() >= tick as f32 {
                    break;
                }
            }
            let launcher = clock.launcher.absolute_position().unwrap();
            let ratio = (f64::from(launcher) - f64::from(tick)) / f64::from(lead);
            assert!(ratio <= motion_budget.max_target_ratio);
            let mut movement = MovementState::default();
            movement.activate(launcher, tick, 185.0, speed).unwrap();
            for k in 0..=motion_budget.max_regular_updates {
                if movement.progress > motion_budget.max_progress as f32 {
                    break;
                }
                let h = first + k;
                let native_time = f64::from(arrival) * f64::from(movement.progress);
                let reference = f64::from(arrival) + f64::from(h) / 120.0 - f64::from(tick) / rate;
                let error = native_time - reference;
                assert!(
                    error >= -motion_budget.early_seconds,
                    "q={q} h={h} error={error}"
                );
                assert!(
                    error <= motion_budget.late_seconds,
                    "q={q} h={h} error={error}"
                );
                if k < motion_budget.max_regular_updates {
                    movement.move_120(speed).unwrap();
                }
            }
        }
    }

    #[test]
    fn judge_table_progress_budget_contains_experimental_recurrence() {
        let budget = judge_table_progress_time_error(SpeedBlock::all(), 600, 2.0).unwrap();
        for q in 100..=1200 {
            let speed = q as f32 / 100.0;
            let arrival = crate::hit_timing::green::motion::arrival_seconds(speed).unwrap();
            let mut progress = 0.0f32;
            let mut elapsed = 0.0f32;
            for i in 0..=budget.max_samples {
                progress = crate::hit_timing::green::motion::next_progress(
                    progress,
                    arrival,
                    f32::from_bits(0x3c888889),
                    elapsed,
                )
                .unwrap();
                if f64::from(progress) > budget.max_progress {
                    break;
                }
                let error = f64::from(arrival) * f64::from(progress) - f64::from(i) / 60.0;
                assert!(error.abs() <= budget.seconds, "q={q} i={i} error={error}");
                elapsed += f32::from_bits(0x3c888889);
            }
        }
    }

    #[test]
    fn conditional_grade_frame_bracket_contains_host_diagnostic_cuts() {
        use crate::hit_timing::green::{
            launch::{NativeBatch, NativePosition},
            motion::{judge_table, GreenGeometry},
        };
        let block = SpeedBlock::all();
        let clock_budget = constant_bpm_clock_error(block, 185.0, 13_000).unwrap();
        let motion_budget =
            constant_bpm_motion_time_error(block, 185.0, 384, clock_budget, 700, 2.0, 2.0).unwrap();
        let table_budget = judge_table_progress_time_error(block, 600, 2.0).unwrap();
        let batch = NativeBatch {
            position: NativePosition {
                bar: 2,
                numerator: 0,
                denominator: 1,
            },
            empty: false,
            bpm: None,
        };
        let launch =
            super::super::launch::constant_bpm_batch_launch_bounds(block, 185.0, &[batch]).unwrap();
        let first = launch[0].update_frame;
        let activation = FrameRange::new(first.lower + 1, first.upper + 1).unwrap();
        let bracket = conditional_grade_gate_frames(
            block,
            384,
            -8,
            activation,
            motion_budget,
            table_budget,
            PositionComparisonMargins {
                judge_line_progress: 0.0001,
                sample_order_progress: 0.0001,
            },
            clock_budget.step_ticks,
        )
        .unwrap();
        let stop_bracket = conditional_stop_frame_bounds(
            block,
            384,
            activation,
            motion_budget,
            table_budget,
            PositionComparisonMargins {
                judge_line_progress: 0.0001,
                sample_order_progress: 0.0001,
            },
            StopPositionGate::TableSample8,
            clock_budget.step_ticks,
        )
        .unwrap();
        for q in [100, 347, 1000, 1101, 1102, 1200] {
            let speed = q as f32 / 100.0;
            let geometry = GreenGeometry {
                specific_speed: speed,
                start_y: 4.9765005,
                button_world_y: -3.45,
                button_local_y: -3.45,
            };
            let table = judge_table(geometry, 0).unwrap();
            let mut clock = MusicClock120::new(speed, 185.0).unwrap();
            let mut frame = 0u32;
            loop {
                clock.update_120().unwrap();
                frame += 1;
                if clock.launcher.absolute_position().unwrap() >= 384.0 {
                    break;
                }
            }
            let mut movement = MovementState::default();
            movement
                .activate(
                    clock.launcher.absolute_position().unwrap(),
                    384,
                    185.0,
                    speed,
                )
                .unwrap();
            let cut = loop {
                if movement.progress > 0.0
                    && geometry.position_y(movement.progress).unwrap() <= table.positions[0]
                {
                    break frame;
                }
                movement.move_120(speed).unwrap();
                frame += 1;
                assert!(frame < 1000);
            };
            assert!(
                bracket.lower <= cut && cut <= bracket.upper,
                "q={q} cut={cut} bracket={bracket:?}"
            );
            let stop = loop {
                if movement.progress > 1.0
                    && geometry.position_y(movement.progress).unwrap() <= table.positions[8]
                {
                    break frame;
                }
                movement.move_120(speed).unwrap();
                frame += 1;
                assert!(frame < 1000);
            };
            assert!(
                stop_bracket.lower <= stop && stop <= stop_bracket.upper,
                "q={q} stop={stop} bracket={stop_bracket:?}"
            );
        }
    }
}
