//! Native activation catch-up and movement counters (CN 9.4.4).
//! Launch comes from the launcher clock, not chart_seconds - arrival_seconds.
//! Position transforms, stop/expiry and chain dispatch remain separate reducers.
use super::{
    clock::{MusicClock120, FRAME_SECONDS, MUSIC_BAR_DIVISION_COUNT},
    motion::{arrival_seconds, next_progress},
};
use crate::hit_timing::ReplayError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MovementState {
    pub progress: f32,
    pub move_seconds: f32,
    pub real_move_seconds: f32,
    pub virtual_move_seconds: f32,
}
impl MovementState {
    fn validate(self) -> Result<(), ReplayError> {
        if [
            self.progress,
            self.move_seconds,
            self.real_move_seconds,
            self.virtual_move_seconds,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(ReplayError::Input("invalid native activation state"));
        }
        Ok(())
    }
    fn move_by(&mut self, arrival: f32, delta: f32) -> Result<(), ReplayError> {
        self.move_seconds += delta;
        self.real_move_seconds += delta;
        self.progress = next_progress(self.progress, arrival, delta, self.real_move_seconds)?;
        self.validate()
    }
    /// Only an uninterrupted Move-state step. Native increments counters BEFORE
    /// CalcProgressRate. Wait/Stop must not accidentally call this method.
    pub fn move_120(&mut self, speed: f32) -> Result<(), ReplayError> {
        self.validate()?;
        let mut next = *self;
        next.move_by(arrival_seconds(speed)?, FRAME_SECONDS)?;
        *self = next;
        Ok(())
    }
    /// ExecuteUpdate in Stop increments the virtual counter, not the real one.
    /// NoteSlideAfter.StopState invokes VirtualMove only with a visible successor.
    pub fn virtual_move_120(&mut self, speed: f32) -> Result<(), ReplayError> {
        self.validate()?;
        let mut next = *self;
        next.move_seconds += FRAME_SECONDS;
        next.virtual_move_seconds += FRAME_SECONDS;
        next.progress = next_progress(
            next.progress,
            arrival_seconds(speed)?,
            FRAME_SECONDS,
            next.real_move_seconds + next.virtual_move_seconds,
        )?;
        next.validate()?;
        *self = next;
        Ok(())
    }
    /// NoteBase.activateAdjust 0x1d71408..0x1d71550. Substeps may overshoot the
    /// target. Setting progress directly to target or floor(elapsed*120) differs.
    pub fn activate(
        &mut self,
        launcher_pos: f32,
        absolute_pos: i32,
        note_bpm: f32,
        speed: f32,
    ) -> Result<u64, ReplayError> {
        self.validate()?;
        if !launcher_pos.is_finite() || absolute_pos < 0 || !note_bpm.is_finite() || note_bpm <= 0.
        {
            return Err(ReplayError::Input("invalid native activation clock"));
        }
        let arrival = arrival_seconds(speed)?;
        if launcher_pos <= absolute_pos as f32 {
            return Ok(0);
        }
        let denominator = ((arrival * note_bpm) / 240.0f32) * MUSIC_BAR_DIVISION_COUNT as f32;
        let target = (launcher_pos - absolute_pos as f32) / denominator;
        if !denominator.is_finite() || denominator <= 0. || !target.is_finite() {
            return Err(ReplayError::Input("native activation target overflow"));
        }
        if target <= self.progress {
            return Ok(0);
        }
        let x = (note_bpm * MUSIC_BAR_DIVISION_COUNT as f32) / 14400.0f32;
        // ARM vmla is two roundings, not f32::mul_add.
        let delta = 1.0f32 / (120.0f32 + x * 60.0f32);
        let mut next = *self;
        let mut count = 0u64;
        while next.progress < target {
            next.move_by(arrival, delta)?;
            count = count
                .checked_add(1)
                .ok_or(ReplayError::Input("native activation step overflow"))?;
        }
        *self = next;
        Ok(count)
    }
    /// One NoteAfterBase.WaitState update. This method is only for a node still
    /// waiting to launch. Returns true when the caller must switch to Move.
    /// Do NOT also run move_120 on the same update: only activateAdjust runs.
    pub fn wait_120(
        &mut self,
        clock: MusicClock120,
        absolute_pos: i32,
        note_bpm: f32,
        speed: f32,
    ) -> Result<bool, ReplayError> {
        self.validate()?;
        if absolute_pos < 0 {
            return Err(ReplayError::Input("invalid green absolute position"));
        }
        let launcher = clock.launcher.absolute_position()?;
        let mut next = *self;
        next.move_seconds += FRAME_SECONDS;
        next.validate()?;
        let launched = launcher >= absolute_pos as f32;
        if launched {
            next.activate(launcher, absolute_pos, note_bpm, speed)?;
        }
        *self = next;
        Ok(launched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_has_native_substep_overshoot() {
        let mut s = MovementState::default();
        let n = s.activate(96.5, 96, 120., 10.).unwrap();
        let step = 1.0f32 / (120. + ((120. * 192.) / 14400.) * 60.);
        assert_eq!(n, 2);
        assert_eq!(s.progress.to_bits(), (step + step).to_bits());
        assert!(s.progress > 0.5 / 96.);
        assert_eq!(s.activate(96.5, 96, 120., 10.).unwrap(), 0);
    }
    #[test]
    fn exact_launch_waits_for_next_regular_move() {
        let c = MusicClock120::new(10., 120.).unwrap();
        let mut s = MovementState::default();
        assert!(s.wait_120(c, 96, 120., 10.).unwrap());
        assert_eq!(s.progress, 0.);
        assert_eq!(s.real_move_seconds, 0.);
        assert_eq!(s.move_seconds, FRAME_SECONDS);
        s.move_120(10.).unwrap();
        assert_eq!(s.progress, FRAME_SECONDS);
        assert_eq!(s.move_seconds, FRAME_SECONDS + FRAME_SECONDS);
    }
    #[test]
    fn wait_does_not_accumulate_real_movement() {
        let c = MusicClock120::new(10., 120.).unwrap();
        let mut s = MovementState::default();
        for _ in 0..200 {
            assert!(!s.wait_120(c, 192, 120., 10.).unwrap());
        }
        assert_eq!(s.real_move_seconds, 0.);
        assert_eq!(s.progress, 0.);
    }
    #[test]
    fn updated_clock_feeds_launch_and_existing_lifetime() {
        let mut c = MusicClock120::new(10., 137.).unwrap();
        let mut s = MovementState::default();
        while !s.wait_120(c, 192, 137., 10.).unwrap() {
            c.update_120().unwrap();
        }
        assert!(s.progress > 0.);
        for _ in 0..120 {
            c.update_120().unwrap();
            s.move_120(10.).unwrap();
        }
        assert!(c.judgement_clock_b0().unwrap().late_seconds(192).unwrap() > 0.);
        assert!(s.progress > 1.);
    }
}
