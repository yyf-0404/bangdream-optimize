//! Position generation from the CN 9.4.4 NoteUtility / SlideNoteManager formulas.
//! Inputs are native movement seeds, not chart-time estimates. This module does
//! not infer launch phase, active lifetime, hidden-node state or touch routing.
use crate::hit_timing::ReplayError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GreenGeometry {
    pub specific_speed: f32,
    pub start_y: f32,
    /// CalcNotePosition reads Transform.position (world coordinates).
    pub button_world_y: f32,
    /// Table crossing and stop tests read Transform.localPosition separately.
    pub button_local_y: f32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GreenJudgeTable {
    pub positions: [f32; 16],
    pub progress_rates: [f32; 16],
    /// First sample at/below the real judge line, before adjust B.
    pub crossing_sample: usize,
    /// Centre after subtracting adjust B; positions[8] is the virtual line.
    pub adjusted_crossing_sample: usize,
}

/// NoteUtility.GetNoteArrivalSeconds, including the strict 11.01 threshold.
/// Keep each ARM vadd/vdiv/vmul result in f32; vmla here is not fused.
pub fn arrival_seconds(speed: f32) -> Result<f32, ReplayError> {
    if !speed.is_finite() || speed < 1.0 {
        return Err(ReplayError::Input("invalid native green speed"));
    }
    let seconds = if speed > f32::from_bits(0x413028f6) {
        (speed - 11.0) / -10.0 + 0.5
    } else {
        5.5 + (speed - 1.0) * -0.5
    };
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(ReplayError::Input("invalid native green arrival time"));
    }
    Ok(seconds)
}

/// NoteUtility.CalcProgressRate: a zero progress uses realMoveSecond/arrival;
/// subsequent updates ADD delta/arrival. Replacing this by t/arrival is not exact.
pub fn next_progress(
    progress: f32,
    arrival: f32,
    delta: f32,
    real_move_second: f32,
) -> Result<f32, ReplayError> {
    if [progress, arrival, delta, real_move_second]
        .iter()
        .any(|x| !x.is_finite())
        || progress < 0.0
        || arrival <= 0.0
        || delta <= 0.0
        || real_move_second < 0.0
    {
        return Err(ReplayError::Input("invalid native green movement seed"));
    }
    let next = if progress == 0.0 {
        real_move_second / arrival
    } else {
        progress + delta / arrival
    };
    if !next.is_finite() || (progress != 0.0 && next <= progress) {
        return Err(ReplayError::Input(
            "native green movement stopped progressing",
        ));
    }
    Ok(next)
}

/// The native CalcNotePosition Y arithmetic AFTER its external powf call.
/// Exposed separately so instruction tests do not silently certify host powf as
/// Android libm. X interpolation does not participate in this Y computation.
pub fn position_y_from_factor(start_y: f32, judge_y: f32, factor: f32) -> Result<f32, ReplayError> {
    if [start_y, judge_y, factor].iter().any(|x| !x.is_finite())
        || start_y <= judge_y
        || factor < 0.0
    {
        return Err(ReplayError::Input("invalid native green position geometry"));
    }
    let y = start_y - (factor * (start_y - judge_y)).abs();
    if !y.is_finite() {
        return Err(ReplayError::Input("native green position overflow"));
    }
    Ok(y)
}

impl GreenGeometry {
    /// Native formula: startY - abs(powf(1.1f, (progress-1)*50) * (startY-buttonWorldY)).
    /// Uses the host powf implementation. Bit-for-bit Android libm equivalence
    /// is a separate adapter certification obligation, not assumed by this API.
    pub fn position_y(self, progress: f32) -> Result<f32, ReplayError> {
        if !progress.is_finite() || progress < 0.0 {
            return Err(ReplayError::Input("invalid native green progress"));
        }
        let exponent = (progress - 1.0) * 50.0;
        let factor = f32::from_bits(0x3f8ccccd).powf(exponent);
        position_y_from_factor(self.start_y, self.button_world_y, factor)
    }

    /// Invert this same host `position_y` calculation on positive f32 values.
    /// Positive f32 bit patterns are ordered by value. The native Y formula is
    /// decreasing in progress, so the first bit pattern at/below a gate is an
    /// exact crossing threshold for the subsequent frame walk. The caller's
    /// upper bracket comes from a finite native judge-table sample, avoiding
    /// an arbitrary progress value that could overflow a valid geometry.
    pub(crate) fn first_progress_at_or_below(
        self,
        gate: f32,
        upper_progress: f32,
    ) -> Result<f32, ReplayError> {
        if !gate.is_finite() || !upper_progress.is_finite() || upper_progress <= 0. {
            return Err(ReplayError::Input("invalid native green progress gate"));
        }
        let mut low = 0.0f32.to_bits();
        let mut high = upper_progress.to_bits();
        if self.position_y(f32::from_bits(low))? <= gate {
            return Ok(0.);
        }
        if self.position_y(f32::from_bits(high))? > gate {
            return Err(ReplayError::Input("native green progress gate not crossed"));
        }
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            if self.position_y(f32::from_bits(middle))? <= gate {
                high = middle;
            } else {
                low = middle;
            }
        }
        Ok(f32::from_bits(high))
    }
}

/// Reconstruct the entire 60 Hz table sampling loop even for 120 FPS gameplay.
/// The B offset selects a sample; it is not a millisecond window expansion.
pub fn judge_table(geometry: GreenGeometry, adjust_b: i32) -> Result<GreenJudgeTable, ReplayError> {
    let arrival = arrival_seconds(geometry.specific_speed)?;
    let stop_y = geometry.button_local_y - 100.0;
    if !stop_y.is_finite() || stop_y >= 999.0 {
        return Err(ReplayError::Input("invalid native green table stop line"));
    }
    let mut progress = 0.0;
    let mut elapsed = 0.0;
    let delta = f32::from_bits(0x3c888889);
    let mut last_y = 999.0;
    let mut ys = Vec::new();
    let mut rates = Vec::new();
    while last_y > stop_y {
        progress = next_progress(progress, arrival, delta, elapsed)?;
        last_y = geometry.position_y(progress)?;
        if ys.last().is_some_and(|&y| last_y >= y) {
            return Err(ReplayError::Input("nondecreasing native green table"));
        }
        ys.push(last_y);
        rates.push(progress);
        elapsed += delta;
    }
    let crossing = ys
        .iter()
        .position(|&y| y <= geometry.button_local_y)
        .ok_or(ReplayError::Input(
            "native green table never reaches judge line",
        ))?;
    let centre = (crossing as i64)
        .checked_sub(i64::from(adjust_b))
        .filter(|&i| i >= 8 && i + 7 < ys.len() as i64)
        .ok_or(ReplayError::Input("native green adjust B exceeds table"))?
        as usize;
    Ok(GreenJudgeTable {
        positions: std::array::from_fn(|i| ys[centre - 8 + i]),
        progress_rates: std::array::from_fn(|i| rates[centre - 8 + i]),
        crossing_sample: crossing,
        adjusted_crossing_sample: centre,
    })
}

/// Consecutive 120 FPS WORLD positions for an uninterrupted sequence of movement
/// calls. These are NOT directly VirtualPosY: the adapter must apply the actual
/// Transform local-position projection before compiling a judgement window.
/// Wait/Stop transitions and skipped movement calls belong to the state adapter.
/// For VirtualMove, real_move_second must already include the native adjustment.
/// The caller must provide the actual active lifetime; no timeout is guessed.
pub fn positions_120(
    geometry: GreenGeometry,
    mut progress: f32,
    mut real_move_second: f32,
    frame_count: usize,
) -> Result<Vec<f32>, ReplayError> {
    if frame_count == 0 || frame_count > (super::MAX_SECONDS * super::REPLAY_FPS + 1) as usize {
        return Err(ReplayError::Input("invalid native green trajectory length"));
    }
    let arrival = arrival_seconds(geometry.specific_speed)?;
    let delta = 1.0f32 / 120.0;
    let mut result = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        progress = next_progress(progress, arrival, delta, real_move_second)?;
        result.push(geometry.position_y(progress)?);
        real_move_second += delta;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::super::{compile_window, GreenRole};
    use super::*;
    use crate::hit_timing::NoteWindow;
    // This fixture has identity parents: generated world Y equals local Y.
    // Production adapters must supply the actual Transform projection instead.
    fn compile_motion_window(
        role: GreenRole,
        geometry: GreenGeometry,
        adjust: i32,
        first: u32,
        progress: f32,
        elapsed: f32,
        count: usize,
    ) -> Result<NoteWindow, ReplayError> {
        assert_eq!(geometry.button_world_y, geometry.button_local_y);
        let table = judge_table(geometry, adjust)?;
        let ys = positions_120(geometry, progress, elapsed, count)?;
        compile_window(role, &table.positions, first, &ys)
    }
    use crate::hit_timing::ReplayNote;
    use crate::scoring::Judgement;
    fn geometry(speed: f32) -> GreenGeometry {
        GreenGeometry {
            specific_speed: speed,
            start_y: 10.0,
            button_world_y: -5.0,
            button_local_y: -5.0,
        }
    }
    #[test]
    fn native_green_arrival_preserves_advanced_threshold() {
        assert_eq!(arrival_seconds(1.0).unwrap(), 5.5);
        assert_eq!(arrival_seconds(10.0).unwrap(), 1.0);
        assert_eq!(arrival_seconds(11.0).unwrap(), 0.5);
        let threshold = f32::from_bits(0x413028f6);
        assert_eq!(
            arrival_seconds(threshold).unwrap(),
            5.5 + (threshold - 1.) * -0.5
        );
        let above = f32::from_bits(threshold.to_bits() + 1);
        assert_eq!(arrival_seconds(above).unwrap(), (above - 11.) / -10. + 0.5);
        for bad in [0.0, 16.0, f32::INFINITY, f32::NAN] {
            assert!(arrival_seconds(bad).is_err());
        }
    }
    #[test]
    fn native_green_progress_uses_seed_then_accumulation() {
        assert_eq!(next_progress(0., 2., 0.125, 1.).unwrap(), 0.5);
        assert_eq!(next_progress(0.5, 2., 0.125, 999.).unwrap(), 0.5625);
        assert!(next_progress(f32::MAX, 2., 0.125, 1.).is_err());
        let mut accumulated = 0.;
        for i in 0..120 {
            accumulated = next_progress(accumulated, 1., 1. / 120., i as f32 / 120.).unwrap();
        }
        assert_ne!(accumulated.to_bits(), (119.0f32 / 120.0).to_bits());
    }
    #[test]
    fn inverted_gate_uses_the_first_crossing_f32_progress() {
        for speed in [1., 4.82, 10., 11.01, 11.66, 12.] {
            let geometry = geometry(speed);
            let table = judge_table(geometry, 0).unwrap();
            for gate in [
                table.positions[0],
                table.positions[1],
                table.positions[2],
                table.positions[5],
                table.positions[7],
                table.positions[8],
                geometry.button_local_y,
            ] {
                let crossing = geometry
                    .first_progress_at_or_below(gate, table.progress_rates[15])
                    .unwrap();
                assert!(geometry.position_y(crossing).unwrap() <= gate);
                assert!(
                    geometry
                        .position_y(f32::from_bits(crossing.to_bits() - 1))
                        .unwrap()
                        > gate
                );
            }
        }
    }
    #[test]
    fn native_green_table_adjusts_index_without_changing_sampling_frequency() {
        for speed in [1., 5., 10., 11., 11.5, 12.] {
            let middle = judge_table(geometry(speed), 0).unwrap();
            assert!(middle.positions[7] > -5. && middle.positions[8] <= -5.);
            assert!(middle.positions.windows(2).all(|w| w[0] > w[1]));
            for adjust in [-2, -1, 1, 2] {
                let table = judge_table(geometry(speed), adjust).unwrap();
                assert_eq!(table.crossing_sample, middle.crossing_sample);
                assert_eq!(
                    table.adjusted_crossing_sample as i64,
                    middle.crossing_sample as i64 - i64::from(adjust)
                );
                assert_eq!(table.positions[8], middle.positions[(8 - adjust) as usize]);
            }
        }
        assert!(judge_table(geometry(10.), i32::MAX).is_err());
        assert!(judge_table(geometry(10.), i32::MIN).is_err());
    }
    #[test]
    fn native_green_table_keeps_world_and_local_button_positions_distinct() {
        let base = geometry(10.);
        let shifted = GreenGeometry {
            button_local_y: -8.0,
            ..base
        };
        assert_eq!(
            base.position_y(1.).unwrap(),
            shifted.position_y(1.).unwrap()
        );
        let a = judge_table(base, 0).unwrap();
        let b = judge_table(shifted, 0).unwrap();
        assert!(b.crossing_sample > a.crossing_sample);
        assert!(b.positions[8] <= -8.0);
    }
    #[test]
    fn native_green_position_is_nonlinear_and_uses_supplied_geometry() {
        let g = geometry(10.);
        assert_eq!(g.position_y(1.).unwrap(), g.button_world_y);
        assert!(
            g.position_y(0.98).unwrap() - g.position_y(1.).unwrap()
                < g.position_y(1.).unwrap() - g.position_y(1.02).unwrap()
        );
        assert!(GreenGeometry { start_y: -6., ..g }.position_y(1.).is_err());
        assert!(position_y_from_factor(10., -5., f32::INFINITY).is_err());
    }
    #[test]
    fn native_green_motion_compiles_movable_windows_for_all_four_roles() {
        let g = geometry(10.);
        let table = judge_table(g, 0).unwrap();
        let ys = positions_120(g, 0., 0.83, 42).unwrap();
        for role in [
            GreenRole::Head,
            GreenRole::HeldMiddle,
            GreenRole::PlainEnd,
            GreenRole::HeldArrowEnd,
        ] {
            let w = compile_motion_window(role, g, 0, 100, 0., 0.83, 42).unwrap();
            assert_eq!(
                serde_json::to_value(&w).unwrap(),
                serde_json::to_value(compile_window(role, &table.positions, 100, &ys).unwrap())
                    .unwrap()
            );
            let note = ReplayNote {
                time: 1.0,
                trigger: None,
                window: w,
            };
            for (i, y) in ys.iter().enumerate() {
                assert_eq!(
                    note.raw_judgement(100 + i as u32).unwrap(),
                    super::super::position_grade(role, &table.positions, *y).unwrap()
                );
            }
            assert_eq!(note.raw_judgement(142).unwrap(), Judgement::Miss);
            assert!(note.frame_domain(Judgement::Perfect).unwrap().len() > 1);
        }
    }
    #[test]
    fn native_green_motion_phase_and_lifetime_are_explicit_inputs() {
        let g = geometry(10.);
        let first = compile_motion_window(GreenRole::Head, g, 0, 100, 0., 0.83, 42).unwrap();
        let shifted = compile_motion_window(GreenRole::Head, g, 0, 100, 0., 0.85, 42).unwrap();
        assert_ne!(
            serde_json::to_value(first).unwrap(),
            serde_json::to_value(shifted).unwrap()
        );
        assert!(positions_120(g, 0., 0., 0).is_err());
        assert!(positions_120(g, -1., 0., 10).is_err());
        assert!(compile_motion_window(GreenRole::Head, g, 0, 72000, 0., 0.9, 2).is_err());
    }
}
