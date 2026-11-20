//! Stable chart-relative coordinates, independent of the winning team and hits.
use super::{ReplayError, ReplayInput, REPLAY_FPS};
use crate::scoring::Judgement;

#[cfg(test)]
mod tests;

impl ReplayInput {
    /// Closest locally valid Perfect frame to the chart time. Half-frame ties
    /// retain the later frame. Candidate lifetime is part of local validity;
    /// same-lane selection and dispatch dependencies still require a replay.
    pub fn chart_reference_frame(&self, index: usize) -> Result<u32, ReplayError> {
        let note = self.notes.get(index).ok_or(ReplayError::Input("invalid reference note index"))?;
        let target = self.native_time_routing.as_ref()
            .and_then(|routing| routing.targets.get(index)).and_then(Option::as_ref);
        let ideal = note.time * f64::from(REPLAY_FPS);
        note.frame_domain(Judgement::Perfect)?.into_iter()
            .filter(|&frame| target.is_none_or(|t| t.active_from_frame <= frame && frame <= t.active_until_frame))
            .min_by(|a,b| (f64::from(*a)-ideal).abs().total_cmp(&(f64::from(*b)-ideal).abs())
                .then_with(|| b.cmp(a)))
            .ok_or(ReplayError::Input("note has no live Perfect reference frame"))
    }

    pub fn chart_reference_frames(&self) -> Result<Vec<u32>, ReplayError> {
        (0..self.notes.len()).map(|index| self.chart_reference_frame(index)).collect()
    }
}
