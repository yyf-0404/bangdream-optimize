use super::{time_judgement, NoteWindow, ReplayError, ReplayNote, SlideTimeRole, MAX_SECONDS, REPLAY_FPS};
use crate::scoring::Judgement;

impl ReplayNote {
    /// Nominal-time projection of native Y[0,1,2,5,7,8] gates. The native
    /// position table samples at 1/60 s, i.e. two input frames. Here tau is
    /// chart time, explicitly excluding launch/position sampling phase.
    /// The current stage uses pure time/progress with uniform startup;
    /// native coordinate/powf equivalence is outside its acceptance scope.
    /// Native references: SlideNoteManager.Judge; NoteSlide.ExecTouchMoved;
    /// NoteSlideAfter.StopState.
    pub(crate) fn slide_time_profile(&self, role: SlideTimeRole, last_frame: Option<u32>) -> Result<([u32; 4], u32), ReplayError> {
        if !self.time.is_finite() || !(0. ..=f64::from(MAX_SECONDS)).contains(&self.time) {
            return Err(ReplayError::Input("note time outside replay range"));
        }
        // Keep the declared binary64 nominal-time projection. Do not round
        // chart seconds to f32 or replace it by the BPM timeout clock.
        let nominal_frame = self.time * f64::from(REPLAY_FPS);
        let centre = nominal_frame.ceil() as u32;
        let first = match role {
            SlideTimeRole::Head | SlideTimeRole::End => [16, 14, 12, 6].map(|early| centre.saturating_sub(early)),
            SlideTimeRole::Middle => [centre.saturating_sub(2); 4],
            SlideTimeRole::FlickEnd | SlideTimeRole::DirectionalEnd => [centre; 4],
        };
        let last = if let Some(last) = last_frame { last } else if role == SlideTimeRole::FlickEnd {
            // Input I_f sees the position produced by U_(f-1). In the pure
            // time projection, arrival is f >= nominal_frame, while Move
            // switches to Stop only when f > nominal_frame. The switch update
            // cannot also execute Stop. Its first counter update is U_s,
            // s = floor(nominal_frame)+1. Fourteen exact f32 increments of 0.5
            // expire after I_(s+13), including one extra input at exact arrival.
            (nominal_frame.floor() as u32).saturating_add(14)
        } else {
            // Native non-arrow timeout is strictly late > this f32 constant.
            (self.time * f64::from(REPLAY_FPS)
                + f64::from(super::green::lifetime::MISS_SECOND_INTERVAL) * f64::from(REPLAY_FPS)).floor() as u32
        };
        if last_frame.is_some_and(|f| f > MAX_SECONDS * REPLAY_FPS) || last < first[0] {
            return Err(ReplayError::Input("invalid slide time lifetime"));
        }
        let last = last.min(MAX_SECONDS * REPLAY_FPS);
        Ok((first, last))
    }

    pub(crate) fn validate_window(&self) -> Result<(), ReplayError> {
        if let NoteWindow::NativeGreen {
            first_frames,
            last_frame,
        } = self.window
        {
            if first_frames.windows(2).any(|w| w[0] > w[1])
                || first_frames[0] > last_frame
                || first_frames[3] > last_frame.saturating_add(1)
                || last_frame > MAX_SECONDS * REPLAY_FPS
            {
                return Err(ReplayError::Input("invalid native green frame domain"));
            }
        }
        Ok(())
    }

    pub(crate) fn raw_judgement(&self, frame: u32) -> Result<Judgement, ReplayError> {
        match self.window {
            NoteWindow::PinnedPerfect { frame: fixed } => Ok(if frame == fixed {
                Judgement::Perfect
            } else {
                Judgement::Miss
            }),
            NoteWindow::NativeGreen {
                first_frames,
                last_frame,
            } => {
                if frame > last_frame {
                    return Ok(Judgement::Miss);
                }
                for (start, grade) in first_frames
                    .into_iter()
                    .zip([
                        Judgement::Bad,
                        Judgement::Good,
                        Judgement::Great,
                        Judgement::Perfect,
                    ])
                    .rev()
                {
                    if frame >= start {
                        return Ok(grade);
                    }
                }
                Ok(Judgement::Miss)
            }
            NoteWindow::SlideTime { role, last_frame } => {
                let (first, last) = self.slide_time_profile(role, last_frame)?;
                if frame > last { return Ok(Judgement::Miss); }
                Ok(first.into_iter().zip([Judgement::Bad, Judgement::Good, Judgement::Great, Judgement::Perfect])
                    .rev().find_map(|(start, grade)| (frame >= start).then_some(grade)).unwrap_or(Judgement::Miss))
            }
            w => time_judgement(
                f64::from(frame) / f64::from(REPLAY_FPS) - self.time,
                matches!(w, NoteWindow::WidenedEndpoint),
            ),
        }
    }

    /// Coarse search bounds, before applying the exact judgment predicate.
    pub(crate) fn frame_search_bounds(&self) -> Result<(u32, u32), ReplayError> {
        if !self.time.is_finite() || self.time < 0. || self.time > f64::from(MAX_SECONDS) {
            return Err(ReplayError::Input("note time outside replay range"));
        }
        self.validate_window()?;
        Ok(match self.window {
            NoteWindow::PinnedPerfect { frame } => (frame, frame),
            NoteWindow::NativeGreen {
                first_frames,
                last_frame,
            } => (first_frames[0], last_frame),
            NoteWindow::SlideTime { role, last_frame } => {
                let (first, last) = self.slide_time_profile(role, last_frame)?;
                (first[0], last)
            }
            _ => (
                ((self.time - 0.15) * f64::from(REPLAY_FPS))
                    .floor()
                    .clamp(0., f64::from(MAX_SECONDS * REPLAY_FPS)) as u32,
                ((self.time + 0.15) * f64::from(REPLAY_FPS))
                    .ceil()
                    .clamp(0., f64::from(MAX_SECONDS * REPLAY_FPS)) as u32,
            ),
        })
    }

    /// Inclusive input frames accepted by this note's declared judgment window.
    /// Candidate lifetime and connection constraints are applied separately.
    pub fn frame_domain(&self, minimum: Judgement) -> Result<Vec<u32>, ReplayError> {
        let (first, last) = self.frame_search_bounds()?;
        if matches!(self.window, NoteWindow::PinnedPerfect { .. }) {
            return Ok(vec![first]);
        }
        (first..=last)
            .filter_map(|frame| match self.raw_judgement(frame) {
                Ok(g) if g >= minimum => Some(Ok(frame)),
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }
}
