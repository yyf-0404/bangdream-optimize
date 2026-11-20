//! Nominal chart time and continuous binary32 music progress are separate.
//! BPM commands are available in beat order, without a speed-dependent launcher.
//! Apply them after the clock update, including the active-list removal phase.
use super::{invalid, number, tick, DataError};
use bangdream_optimize_core::hit_timing::green::clock::BarProgress;
use bangdream_optimize_core::hit_timing::green::lifetime::NativeClock;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BpmSegment {
    pub beat: f64,
    pub bpm: f64,
    pub time: f64,
    /// Completed update after which this rate applies. None means the command
    /// was not reached within the supported preparation horizon.
    pub applied_frame: Option<u32>,
    #[serde(skip)]
    tick: i32,
}

pub(super) struct TempoMap(pub Vec<BpmSegment>);

impl TempoMap {
    pub fn new(events: &[Value]) -> Result<Self, DataError> {
        let mut segments = Vec::new();
        for row in events.iter().filter(|e| e["type"] == "BPM") {
            let beat = number(row, "beat")?;
            let bpm = number(row, "bpm")?;
            let rate = bpm as f32;
            if bpm <= 0. || !rate.is_finite() || rate <= 0. {
                return Err(invalid("bpm", "expected positive binary32 rate"));
            }
            segments.push(BpmSegment {
                beat,
                bpm,
                time: 0.,
                applied_frame: None,
                tick: tick(beat, "BPM change")?,
            });
        }
        segments.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        if segments.first().is_none_or(|s| s.beat != 0.) {
            return Err(invalid("BPM", "need an initial BPM at beat zero"));
        }
        segments[0].applied_frame = Some(0);
        for i in 1..segments.len() {
            let prev = &segments[i - 1];
            let next = &segments[i];
            if next.beat <= prev.beat || next.tick <= prev.tick {
                return Err(invalid("BPM", "duplicate BPM position"));
            }
            let time = prev.time + (next.beat - prev.beat) * 60. / prev.bpm;
            if !time.is_finite() || time > 600. || time <= prev.time {
                return Err(invalid("BPM", "invalid or out-of-range segment time"));
            }
            segments[i].time = time;
        }
        Ok(Self(segments))
    }

    pub fn seconds(&self, beat: f64) -> Result<f64, DataError> {
        if !beat.is_finite() || beat < 0. {
            return Err(invalid("beat", "expected finite nonnegative beat"));
        }
        let index = self.0.partition_point(|s| s.beat <= beat) - 1;
        let segment = &self.0[index];
        let time = segment.time + (beat - segment.beat) * 60. / segment.bpm;
        if !time.is_finite() || time > 600. {
            return Err(invalid(
                "beat",
                "note lies outside the 0..600 second replay range",
            ));
        }
        Ok(time)
    }

    #[cfg(test)]
    pub fn music_positions(&mut self, last_frame: u32) -> Result<Vec<f32>, DataError> {
        Ok(self.music_clocks(last_frame)?.into_iter().map(|c| c.adjusted_music_pos).collect())
    }

    pub fn music_clocks(&mut self, last_frame: u32) -> Result<Vec<NativeClock>, DataError> {
        self.music_clocks_counted(last_frame, || {})
    }

    fn music_clocks_counted(&mut self, last_frame: u32, mut checked: impl FnMut()) -> Result<Vec<NativeClock>, DataError> {
        let mut clock = BarProgress::default();
        let mut bpm = self.0[0].bpm as f32;
        // An index-linked list preserves Vec::remove's exact visit order
        // without moving the suffix. Slot zero is the sentinel before the
        // first command; len is the end marker.
        let mut next: Vec<_> = (1..=self.0.len()).collect();
        let mut positions = Vec::with_capacity(last_frame as usize + 1);
        positions.push(NativeClock { adjusted_music_pos: clock.absolute_position()?, current_bpm: bpm, resolution:192 });
        for frame in 1..=last_frame {
            clock.update_120(bpm)?;
            let mut previous = 0;
            while next[previous] < self.0.len() {
                let index = next[previous];
                let command = &mut self.0[index];
                checked();
                // Tick order is strict. At fixed progress, failure for this
                // command implies failure for the whole remaining suffix.
                if !clock.reached(command.tick / 192, command.tick % 192, 192)? { break; }
                bpm = command.bpm as f32;
                command.applied_frame = Some(frame);
                next[previous] = next[index];
                // The shifted successor is skipped this update even if due.
                // It stays linked and becomes the predecessor of the next
                // visited command. Simply draining a due prefix is incorrect.
                previous = next[previous];
                if previous == self.0.len() { break; }
            }
            positions.push(NativeClock { adjusted_music_pos: clock.absolute_position()?, current_bpm: bpm, resolution:192 });
        }
        Ok(positions)
    }
}

#[cfg(test)]
mod tests;
