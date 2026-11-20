//! One interpretation of prepared causal constraints for replay and search.
use super::{ArrivalCurve, ArrivalRun, NoteConstraint, ReplayError, MAX_SECONDS, REPLAY_FPS};

impl NoteConstraint {
    pub fn required_after(&self, before_frame: u32) -> Option<u32> {
        required_after(
            self.minimum_gap_frames,
            self.earliest_after.as_ref(),
            before_frame,
        )
    }

    pub fn allows_frames(&self, before_frame: u32, after_frame: u32) -> bool {
        self.required_after(before_frame)
            .is_some_and(|f| after_frame >= f)
    }
}

pub(super) fn required_after(gap: u32, curve: Option<&ArrivalCurve>, before: u32) -> Option<u32> {
    let minimum = before.checked_add(gap)?;
    Some(match curve {
        Some(curve) => minimum.max(curve.earliest(before)?),
        None => minimum,
    })
}

impl ArrivalCurve {
    /// Lossless compression; None means no successor is reachable. Neither
    /// holes nor values outside the table are filled by interpolation.
    pub fn from_samples(first: u32, samples: &[Option<u32>]) -> Result<Self, ReplayError> {
        let limit = MAX_SECONDS * REPLAY_FPS;
        if samples.is_empty()
            || first > limit
            || samples.len() as u64 > u64::from(limit - first) + 1
            || samples.iter().flatten().any(|&v| v > limit)
        {
            return Err(ReplayError::Input("invalid arrival samples"));
        }
        let mut runs = Vec::new();
        let mut start = 0;
        while start < samples.len() {
            let follows_before = samples
                .get(start + 1)
                .is_some_and(|&next| samples[start].is_some_and(|a| a.checked_add(1) == next));
            let mut end = start;
            while end + 1 < samples.len() {
                let expected = if follows_before {
                    samples[start].and_then(|a| a.checked_add((end + 1 - start) as u32))
                } else {
                    samples[start]
                };
                if samples[end + 1] != expected {
                    break;
                }
                end += 1;
            }
            runs.push(ArrivalRun {
                through_frame: first + end as u32,
                first_after_frame: samples[start],
                follows_before,
            });
            start = end + 1;
        }
        let curve = Self {
            first_before_frame: first,
            runs,
        };
        curve.validate()?;
        Ok(curve)
    }

    pub fn earliest(&self, before_frame: u32) -> Option<u32> {
        if before_frame < self.first_before_frame {
            return None;
        }
        let i = self
            .runs
            .partition_point(|r| r.through_frame < before_frame);
        let run = self.runs.get(i)?;
        let first = if i == 0 {
            self.first_before_frame
        } else {
            self.runs[i - 1].through_frame + 1
        };
        let value = run.first_after_frame?;
        if run.follows_before {
            value.checked_add(before_frame - first)
        } else {
            Some(value)
        }
    }

    pub(crate) fn validate(&self) -> Result<(), ReplayError> {
        let limit = MAX_SECONDS * REPLAY_FPS;
        if self.runs.is_empty() || self.first_before_frame > limit {
            return Err(ReplayError::Input("invalid arrival curve domain"));
        }
        let mut first = self.first_before_frame;
        for run in &self.runs {
            if run.through_frame < first
                || run.through_frame > limit
                || (run.follows_before && run.first_after_frame.is_none())
                || run.first_after_frame.is_some_and(|v| {
                    v > limit
                        || (run.follows_before
                            && v.checked_add(run.through_frame - first)
                                .is_none_or(|v| v > limit))
                })
            {
                return Err(ReplayError::Input("invalid arrival curve run"));
            }
            first = run.through_frame + 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
