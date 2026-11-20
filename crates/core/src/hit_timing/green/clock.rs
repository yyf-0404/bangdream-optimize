//! CN 9.4.4 music/launcher clocks, at the requested 120 FPS and adjust B=0.
//! These are preparation states, never extra team-search dimensions. Callers
//! retain native event order: input -> both clocks -> active BPM commands ->
//! existing notes -> new batches (which may change launcher BPM).
use super::{lifetime::NativeClock, motion::arrival_seconds};
use crate::hit_timing::ReplayError;
use serde::{Deserialize, Serialize};

/// NoteManager::.cctor 0x18a9e44/54. This is not a configurable chart resolution.
pub const MUSIC_BAR_DIVISION_COUNT: i32 = 192;
pub const FRAME_SECONDS: f32 = 1.0f32 / 120.0;

fn positive(value: f32) -> Result<f32, ReplayError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ReplayError::Input("invalid native music clock rate"));
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BarProgress {
    pub bar: i32,
    pub beat: f32,
}
impl BarProgress {
    fn validate(self) -> Result<(), ReplayError> {
        if self.bar < 0 || !self.beat.is_finite() || self.beat < 0.0 {
            return Err(ReplayError::Input("invalid native bar progress"));
        }
        Ok(())
    }
    /// Native getter multiplies integers BEFORE converting to f32.
    pub fn absolute_position(self) -> Result<f32, ReplayError> {
        self.validate()?;
        let base = self
            .bar
            .checked_mul(MUSIC_BAR_DIVISION_COUNT)
            .ok_or(ReplayError::Input("native bar position overflow"))?;
        let pos = self.beat + base as f32;
        if !pos.is_finite() {
            return Err(ReplayError::Input("native bar position overflow"));
        }
        Ok(pos)
    }
    /// SetupFirstGameProgress uses repeated f32 subtraction, NOT floor/fmod.
    pub fn from_lead(mut beat: f32) -> Result<Self, ReplayError> {
        let mut bar = 0i32;
        if !beat.is_finite() || beat < 0.0 {
            return Err(ReplayError::Input("invalid native launcher lead"));
        }
        while beat >= MUSIC_BAR_DIVISION_COUNT as f32 {
            let next = beat - MUSIC_BAR_DIVISION_COUNT as f32;
            if next >= beat {
                return Err(ReplayError::Input(
                    "native launcher lead stopped progressing",
                ));
            }
            beat = next;
            bar = bar
                .checked_add(1)
                .ok_or(ReplayError::Input("native bar position overflow"))?;
        }
        let result = Self { bar, beat };
        result.absolute_position()?;
        Ok(result)
    }
    /// Advance only this music clock, without launcher geometry or note speed.
    pub fn update_120(&mut self, bpm: f32) -> Result<(), ReplayError> {
        self.validate()?;
        positive(bpm)?;
        // GetBarSeconds -> divide by ticks/bar -> delta / seconds/tick.
        // Do not replace two divisions by delta*bpm*192/240.
        let step = positive(FRAME_SECONDS / ((240.0f32 / bpm) / MUSIC_BAR_DIVISION_COUNT as f32))?;
        let mut beat = self.beat + step;
        if !beat.is_finite() || beat <= self.beat {
            return Err(ReplayError::Input("native music clock stopped progressing"));
        }
        let mut bar = self.bar;
        // UpdateMusicScoreProgress wraps ONCE, unlike SetupFirstGameProgress.
        if beat >= MUSIC_BAR_DIVISION_COUNT as f32 {
            beat -= MUSIC_BAR_DIVISION_COUNT as f32;
            bar = bar
                .checked_add(1)
                .ok_or(ReplayError::Input("native bar position overflow"))?;
        }
        let next = Self { bar, beat };
        next.absolute_position()?;
        *self = next;
        Ok(())
    }
    /// NoteBpmChange.ExecUpdate compares bar and integer sub-bar position
    /// separately. Converting a chart beat/time to an absolute float is not equal.
    pub fn reached(self, bar: i32, numerator: i32, denominator: i32) -> Result<bool, ReplayError> {
        self.validate()?;
        if bar < 0 || numerator < 0 || denominator <= 0 || numerator >= denominator {
            return Err(ReplayError::Input("invalid native BPM position"));
        }
        let offset = MUSIC_BAR_DIVISION_COUNT
            .checked_mul(numerator)
            .ok_or(ReplayError::Input("native BPM position overflow"))?
            / denominator;
        Ok((bar as f32) < self.bar as f32
            || ((bar as f32) == self.bar as f32 && self.beat >= offset as f32))
    }
}

/// NoteManager.ExecAwakeEnd uses speed's arrival time and BASIC BPM.
pub fn initial_launcher_lead(speed: f32, basic_bpm: f32) -> Result<f32, ReplayError> {
    positive(basic_bpm)?;
    positive(((arrival_seconds(speed)? * basic_bpm) / 240.0f32) * MUSIC_BAR_DIVISION_COUNT as f32)
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MusicClock120 {
    pub music: BarProgress,
    pub launcher: BarProgress,
    pub current_bpm: f32,
    pub launcher_bpm: f32,
}
impl MusicClock120 {
    /// Fresh controller at song initialization; loading/resume must retain their
    /// actual existing state instead of calling this constructor again.
    pub fn new(speed: f32, basic_bpm: f32) -> Result<Self, ReplayError> {
        Ok(Self {
            music: BarProgress::default(),
            launcher: BarProgress::from_lead(initial_launcher_lead(speed, basic_bpm)?)?,
            current_bpm: basic_bpm,
            launcher_bpm: basic_bpm,
        })
    }
    pub fn update_120(&mut self) -> Result<(), ReplayError> {
        let mut next = *self;
        next.music.update_120(next.current_bpm)?;
        next.launcher.update_120(next.launcher_bpm)?;
        *self = next;
        Ok(())
    }
    /// Called when a BPM command is launched, possibly before its scoring time.
    pub fn launch_bpm(&mut self, bpm: f32) -> Result<(), ReplayError> {
        self.launcher_bpm = positive(bpm)?;
        Ok(())
    }
    /// Called by an active BPM command AFTER this frame's clock update. No reset.
    pub fn apply_bpm(&mut self, bpm: f32) -> Result<(), ReplayError> {
        self.current_bpm = positive(bpm)?;
        Ok(())
    }
    /// Feeds the existing timeout/midpoint compiler. B=0 GetAdjustMusicPos returns
    /// MusicPos unchanged; nonzero B requires the native fast/slow conversion.
    pub fn judgement_clock_b0(self) -> Result<NativeClock, ReplayError> {
        positive(self.current_bpm)?;
        Ok(NativeClock {
            adjusted_music_pos: self.music.absolute_position()?,
            current_bpm: self.current_bpm,
            resolution: MUSIC_BAR_DIVISION_COUNT,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clocks_have_native_lead_and_independent_bpm() {
        let mut c = MusicClock120::new(10., 120.).unwrap();
        assert_eq!(c.launcher.absolute_position().unwrap(), 96.);
        assert_eq!(c.music.absolute_position().unwrap(), 0.);
        c.launch_bpm(240.).unwrap();
        c.update_120().unwrap();
        assert_eq!(
            c.music.beat.to_bits(),
            (FRAME_SECONDS / ((240. / 120.) / 192.)).to_bits()
        );
        assert_eq!(
            c.launcher.beat.to_bits(),
            (96. + FRAME_SECONDS / ((240. / 240.) / 192.)).to_bits()
        );
        let before = c.music;
        c.apply_bpm(240.).unwrap();
        assert_eq!(c.music.beat.to_bits(), before.beat.to_bits());
        assert_eq!(c.judgement_clock_b0().unwrap().resolution, 192);
    }
    #[test]
    fn initial_wrap_repeats_but_each_frame_wraps_once() {
        let p = BarProgress::from_lead(480.).unwrap();
        assert_eq!((p.bar, p.beat), (2, 96.));
        let mut p = BarProgress { bar: 0, beat: 500. };
        let delta = FRAME_SECONDS / ((240. / 120.) / 192.);
        p.update_120(120.).unwrap();
        assert_eq!((p.bar, p.beat), (1, (500. + delta) - 192.));
    }
    #[test]
    fn bpm_gate_keeps_integer_fraction_and_inclusive_equality() {
        let mut p = BarProgress { bar: 2, beat: 27. };
        assert!(p.reached(2, 1, 7).unwrap()); // 192/7 truncates to 27.
        p.beat = f32::from_bits(27.0f32.to_bits() - 1);
        assert!(!p.reached(2, 1, 7).unwrap());
        assert!(p.reached(1, 6, 7).unwrap());
        assert!(!p.reached(3, 0, 1).unwrap());
    }
    #[test]
    fn clock_recurrence_is_not_direct_seconds() {
        let mut c = MusicClock120::new(10., 173.).unwrap();
        for _ in 0..12000 {
            c.update_120().unwrap();
        }
        assert_ne!(
            c.music.absolute_position().unwrap().to_bits(),
            (100.0f32 * 173. / 240. * 192.).to_bits()
        );
        assert!(BarProgress {
            bar: i32::MAX,
            beat: 0.
        }
        .absolute_position()
        .is_err());
        assert!(MusicClock120::new(10., f32::NAN).is_err());
    }
}
