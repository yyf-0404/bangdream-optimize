//! Position-judged green nodes: compile supplied native positions/lifetime into
//! movable domains. `motion` reconstructs native position formulas from explicit
//! movement seeds; neither API invents a launch phase/lifetime from chart time.
//! Continuous-contact paths only: no mid-chain release/recovery. Chain ordering,
//! native event order and gate facts remain adapter obligations. The projection
//! module reduces verified B=0 hidden runs to visible dependencies during preparation.
pub mod activation;
pub mod clock;
pub mod compact;
pub mod dispatch;
pub mod interval;
pub mod launch;
pub mod lifetime;
pub mod motion;
pub mod projection;
pub mod speed;
pub mod strict;
pub mod trajectory;

use super::{NoteWindow, ReplayError, MAX_SECONDS, REPLAY_FPS};
use crate::scoring::Judgement;

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GreenRole {
    Head,
    HeldMiddle,
    PlainEnd,
    HeldArrowEnd,
}

/// Native setter clamps VirtualPosY at Y[8]. Active lifetime must be supplied.
/// The timing-only optimization model assumes required gestures can be made;
/// touch position/distance/direction are not search inputs. Native dispatch
/// auditing may verify those conditions separately, outside this score model.
/// An unavailable node must not be repaired by judgement-correction skills.
pub fn position_grade(
    role: GreenRole,
    table: &[f32; 16],
    virtual_y: f32,
) -> Result<Judgement, ReplayError> {
    if !virtual_y.is_finite()
        || table.iter().any(|p| !p.is_finite())
        || table.windows(2).any(|p| p[0] <= p[1])
    {
        return Err(ReplayError::Input("invalid native green position table"));
    }
    let y = virtual_y.max(table[8]);
    Ok(match role {
        GreenRole::HeldMiddle => {
            if y <= table[7] {
                Judgement::Perfect
            } else {
                Judgement::Miss
            }
        }
        GreenRole::HeldArrowEnd => {
            if y <= table[8] {
                Judgement::Perfect
            } else {
                Judgement::Miss
            }
        }
        GreenRole::Head | GreenRole::PlainEnd => {
            if y > table[0] {
                Judgement::Miss
            } else if y > table[1] {
                Judgement::Bad
            } else if y > table[2] {
                Judgement::Good
            } else if y > table[5] {
                Judgement::Great
            } else {
                Judgement::Perfect
            }
        }
    })
}

/// Each sample is the native virtual Y for one consecutive 120 FPS frame.
/// `first_frame..first_frame+len` must already be intersected with the node's
/// native active lifetime (timeout, next-visible recall and state gates).
/// Reject nonmonotone samples rather than silently widening the legal domain.
/// Only a supplied native trajectory is supported; a +/- millisecond guess is
/// not a substitute for these inputs. Hidden control points are not scoring notes.
pub fn compile_window(
    role: GreenRole,
    table: &[f32; 16],
    first_frame: u32,
    positions: &[f32],
) -> Result<NoteWindow, ReplayError> {
    let last = first_frame
        .checked_add(
            u32::try_from(positions.len())
                .map_err(|_| ReplayError::Input("green frame count overflow"))?,
        )
        .and_then(|v| v.checked_sub(1))
        .filter(|&v| !positions.is_empty() && v <= MAX_SECONDS * REPLAY_FPS)
        .ok_or(ReplayError::Input("invalid green lifetime"))?;
    let mut first_frames = [last + 1; 4];
    let mut previous = Judgement::Miss;
    for (offset, &y) in positions.iter().enumerate() {
        let grade = position_grade(role, table, y)?;
        if grade < previous {
            return Err(ReplayError::Input("nonmonotone green domain"));
        }
        previous = grade;
        for (i, threshold) in [
            Judgement::Bad,
            Judgement::Good,
            Judgement::Great,
            Judgement::Perfect,
        ]
        .into_iter()
        .enumerate()
        {
            if grade >= threshold {
                first_frames[i] = first_frames[i].min(first_frame + offset as u32);
            }
        }
    }
    if first_frames[0] > last {
        return Err(ReplayError::Input("empty green judgement domain"));
    }
    Ok(NoteWindow::NativeGreen {
        first_frames,
        last_frame: last,
    })
}
