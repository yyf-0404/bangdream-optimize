//! CN 9.4.4 temporal expiry of an UNSETTLED green node. Gesture geometry is not
//! a search state. Native clock/state facts are supplied by the chart adapter;
//! chart seconds must not be substituted for adjusted absolute music positions.
//! Source: NoteSlide.WaitState 0x1e8cbdc; NoteSlideAfter.StopState 0x214ad0c;
//! executeStopStateByExistAfterNote 0x214b294; NoteUtility.GetSecWithDistance.
use super::{compact::GreenFrameProfile, GreenRole};
use crate::hit_timing::{NoteWindow, ReplayError, MAX_SECONDS, REPLAY_FPS};
use serde::{Deserialize, Serialize};

pub const MISS_SECOND_INTERVAL: f32 = f32::from_bits(0x3e5dddde);

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeClock {
    pub adjusted_music_pos: f32,
    pub current_bpm: f32,
    pub resolution: i32,
}
impl NativeClock {
    pub fn late_seconds(self, absolute_pos: i32) -> Result<f32, ReplayError> {
        if !self.adjusted_music_pos.is_finite()
            || !self.current_bpm.is_finite()
            || self.current_bpm <= 0.
            || self.resolution <= 0
            || absolute_pos < 0
        {
            return Err(ReplayError::Input("invalid native green clock"));
        }
        // vcvt -> vsub -> 240/bpm -> vmul -> vdiv. No reassociation or f64.
        let distance = self.adjusted_music_pos - absolute_pos as f32;
        let seconds = ((240.0f32 / self.current_bpm) * distance) / self.resolution as f32;
        if !seconds.is_finite() {
            return Err(ReplayError::Input("native green clock overflow"));
        }
        Ok(seconds)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VisibleState {
    Move,
    Wait,
    Stop,
    Deactive,
    WaitingDeactive,
}
impl VisibleState {
    fn recalls_previous(self) -> bool {
        matches!(self, Self::Stop | Self::Deactive | Self::WaitingDeactive)
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisibleAfter {
    pub absolute_pos: i32,
    /// State observed at THIS node's update; do not replace by the successor's
    /// state after its later update in the same frame.
    pub state: VisibleState,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LifetimeUpdate {
    /// Movement itself does not run WaitState/StopState expiry on this update.
    Move,
    HeadWait {
        clock: NativeClock,
        next_visible_pos: i32,
    },
    AfterStop {
        clock: NativeClock,
        has_direct_after: bool,
        next_visible: Option<VisibleAfter>,
        /// Native head AfterNoteType == 8 AND this node has no direct successor.
        arrow_terminal: bool,
        /// Native controller ExecuteFrame; NOT a count of rendered frames.
        execute_frame: f32,
        adjust_b: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Expiry {
    Timeout,
    HeadNextVisible,
    NextVisibleStopped,
    MissingVisibleAfter,
    AfterMidpoint,
    ArrowWait,
}

/// Counters start at activation and advance only when their native branch runs.
/// This state is a preparation/replay helper, not an added per-team search key.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnsettledLifetime {
    stop_adjust_counter: u32,
    arrow_wait: f32,
    expired: Option<Expiry>,
}
impl UnsettledLifetime {
    pub fn expired(&self) -> Option<Expiry> {
        self.expired
    }

    pub fn update(
        &mut self,
        absolute_pos: i32,
        update: LifetimeUpdate,
    ) -> Result<Option<Expiry>, ReplayError> {
        if absolute_pos < 0 {
            return Err(ReplayError::Input("invalid green absolute position"));
        }
        if let Some(reason) = self.expired {
            return Ok(Some(reason));
        }
        let reason = match update {
            LifetimeUpdate::Move => None,
            LifetimeUpdate::HeadWait {
                clock,
                next_visible_pos,
            } => {
                if next_visible_pos <= absolute_pos {
                    return Err(ReplayError::Input("invalid green next visible position"));
                }
                let late = clock.late_seconds(absolute_pos)?;
                let since = clock.adjusted_music_pos - absolute_pos as f32;
                let until = next_visible_pos as f32 - clock.adjusted_music_pos;
                if late > MISS_SECOND_INTERVAL {
                    Some(Expiry::Timeout)
                } else if until <= 0. || since > until {
                    Some(Expiry::HeadNextVisible)
                } else {
                    None
                }
            }
            LifetimeUpdate::AfterStop {
                clock,
                has_direct_after,
                next_visible,
                arrow_terminal,
                execute_frame,
                adjust_b,
            } => {
                if (!has_direct_after && next_visible.is_some())
                    || (arrow_terminal && has_direct_after)
                    || !execute_frame.is_finite()
                    || execute_frame < 0.
                    || next_visible.is_some_and(|n| n.absolute_pos <= absolute_pos)
                {
                    return Err(ReplayError::Input("invalid green after-node lifecycle"));
                }
                let late = clock.late_seconds(absolute_pos)?;
                // The helper checks recall BEFORE StopState increments this counter.
                let delayed = has_direct_after
                    && next_visible.is_some()
                    && adjust_b < 0
                    && i64::from(self.stop_adjust_counter) < 6 - i64::from(adjust_b);
                if let Some(next) = next_visible.filter(|_| has_direct_after) {
                    if next.state.recalls_previous() && !delayed {
                        self.expired = Some(Expiry::NextVisibleStopped);
                        return Ok(self.expired);
                    }
                }
                if delayed {
                    self.stop_adjust_counter = self
                        .stop_adjust_counter
                        .checked_add(1)
                        .ok_or(ReplayError::Input("green stop adjustment overflow"))?;
                    None
                } else if arrow_terminal {
                    self.arrow_wait += execute_frame;
                    if self.arrow_wait >= 7. {
                        Some(Expiry::ArrowWait)
                    } else {
                        None
                    }
                } else if late > MISS_SECOND_INTERVAL {
                    Some(Expiry::Timeout)
                } else if has_direct_after {
                    match next_visible {
                        None => Some(Expiry::MissingVisibleAfter),
                        Some(next) => {
                            let since = clock.adjusted_music_pos - absolute_pos as f32;
                            let until = next.absolute_pos as f32 - clock.adjusted_music_pos;
                            // Unlike the head branch, until <= 0 does not expire here.
                            if until > 0. && since > until {
                                Some(Expiry::AfterMidpoint)
                            } else {
                                None
                            }
                        }
                    }
                } else {
                    None
                }
            }
        };
        self.expired = reason;
        Ok(reason)
    }
}

/// Local virtual position available to input, then the following native update.
/// The caller supplies actual state/clock traces, including any launch catch-up.
/// Traces depending on earlier chosen hits cannot be reused as static windows
/// until the adapter proves that dependence irrelevant or retains it separately.
#[derive(Clone, Copy, Debug)]
pub struct LifetimeSample {
    pub local_y: f32,
    pub following_update: LifetimeUpdate,
}
#[derive(Clone, Copy, Debug)]
pub struct LiveWindow {
    pub window: NoteWindow,
    /// Input in this frame remains valid; the following update expires the note.
    pub expires_after_frame: u32,
    pub reason: Expiry,
}

/// Feed the native temporal reducer directly into the existing compact domain.
/// Requires an expiry witness; a short sample stream must not silently cut off
/// legal late inputs. Does not certify the caller's chart-to-clock/position map.
pub fn compile_live_window(
    role: GreenRole,
    table: &[f32; 16],
    absolute_pos: i32,
    first_frame: u32,
    samples: impl IntoIterator<Item = LifetimeSample>,
) -> Result<LiveWindow, ReplayError> {
    let mut life = UnsettledLifetime::default();
    let mut positions = Vec::new();
    for sample in samples {
        let frame = u32::try_from(positions.len())
            .ok()
            .and_then(|n| first_frame.checked_add(n))
            .filter(|&n| n <= MAX_SECONDS * REPLAY_FPS)
            .ok_or(ReplayError::Input("invalid green lifetime"))?;
        if !sample.local_y.is_finite() {
            return Err(ReplayError::Input("invalid native green position"));
        }
        match (role, sample.following_update) {
            (GreenRole::Head, LifetimeUpdate::AfterStop { .. })
            | (
                GreenRole::HeldMiddle | GreenRole::PlainEnd | GreenRole::HeldArrowEnd,
                LifetimeUpdate::HeadWait { .. },
            ) => return Err(ReplayError::Input("green role and lifecycle disagree")),
            _ => {}
        }
        positions.push(sample.local_y);
        if let Some(reason) = life.update(absolute_pos, sample.following_update)? {
            return Ok(LiveWindow {
                window: GreenFrameProfile::compile(table, first_frame, positions)?.window(role)?,
                expires_after_frame: frame,
                reason,
            });
        }
    }
    Err(ReplayError::Input("unterminated native green lifetime"))
}

/// One verified speed variant of the SAME node, on the SAME global frame clock.
/// Generated tables are preparation inputs only; no position data enters search.
pub struct LifetimeVariant<I> {
    pub table: [f32; 16],
    pub first_frame: u32,
    pub samples: I,
}

/// Compile each variant's actual expiry before intersecting the raw grades.
/// Failed or incomplete variants propagate an error instead of being dropped.
/// Coverage of the supplied speed set is an adapter obligation, not inferred here.
pub fn compile_strict_live_window<I: IntoIterator<Item = LifetimeSample>>(
    role: GreenRole,
    absolute_pos: i32,
    variants: impl IntoIterator<Item = LifetimeVariant<I>>,
) -> Result<NoteWindow, ReplayError> {
    super::strict::intersect_windows(variants.into_iter().map(|variant| {
        compile_live_window(
            role,
            &variant.table,
            absolute_pos,
            variant.first_frame,
            variant.samples,
        )
        .map(|live| live.window)
    }))
}

#[cfg(test)]
mod tests;
