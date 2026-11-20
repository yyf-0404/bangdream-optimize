//! Non-enumerating hidden-bridge transfer over certified frame endpoints.
use super::{CommonWindowBounds, FrameRange};
use crate::hit_timing::{
    green::{
        compact::{compile_chain, CompiledGreenChain, GreenChainNode},
        GreenRole,
    },
    ArrivalCurve, NoteWindow, ReplayError, MAX_SECONDS, REPLAY_FPS,
};

#[derive(Clone, Copy, Debug)]
pub struct HiddenGateFrameBounds {
    /// None means this node never obtains a successful Move callback.
    pub first_ready_frame: Option<FrameRange>,
    /// None means it never expires automatically.
    pub expires_after_frame: Option<FrameRange>,
}

/// The maximum, over all standard speeds, of one hidden bridge's earliest
/// successor callback at each predecessor input frame. Each interval encloses
/// that maximum, as produced by an exhaustive speed-block *partition*, not
/// by sampling or selecting endpoint speeds.
pub struct CommonBridgeMaxBounds {
    pub predecessor_note_index: usize,
    pub successor_note_index: usize,
    pub first_before_frame: u32,
    pub maximum_first_after: Vec<FrameRange>,
}

#[derive(Clone, Copy)]
enum Side {
    Safe,
    Possible,
}

fn domain(window: NoteWindow) -> (u32, u32) {
    let NoteWindow::NativeGreen {
        first_frames,
        last_frame,
    } = window
    else {
        unreachable!("bounded green windows have native domains")
    };
    (first_frames[0], last_frame)
}

fn clip(
    window: &mut NoteWindow,
    first: Option<u32>,
    last: Option<u32>,
    removed: &mut u64,
) -> Result<(), ReplayError> {
    let NoteWindow::NativeGreen {
        first_frames,
        last_frame,
    } = window
    else {
        unreachable!("bounded green windows have native domains")
    };
    let old_count = *last_frame - first_frames[0] + 1;
    let start = first_frames[0].max(first.unwrap_or(0));
    let end = (*last_frame).min(last.unwrap_or(u32::MAX));
    if start > end {
        return Err(ReplayError::Input("empty bounded green chain"));
    }
    *first_frames = first_frames.map(|f| f.max(start).min(end + 1));
    *last_frame = end;
    *removed += u64::from(old_count - (end - start + 1));
    Ok(())
}

fn compile_side(
    windows: &[CommonWindowBounds],
    bridges: &[CommonBridgeMaxBounds],
    side: Side,
) -> Result<CompiledGreenChain, ReplayError> {
    let nodes = windows
        .iter()
        .enumerate()
        .map(|(index, bound)| {
            let window = match side {
                Side::Safe => bound.safe_window(),
                Side::Possible => bound.possible_window(),
            }
            .ok_or(ReplayError::Input("empty bounded green window"))?;
            Ok(GreenChainNode {
                note_index: bound.note_index,
                window,
                minimum_gap_from_previous: u32::from(index != 0),
            })
        })
        .collect::<Result<Vec<_>, ReplayError>>()?;
    let mut compiled = compile_chain(&nodes)?;
    for (edge_index, edge) in compiled.constraints.iter_mut().enumerate() {
        let bound = &bridges[edge_index];
        if bound.predecessor_note_index != edge.before
            || bound.successor_note_index != edge.after
            || bound.maximum_first_after.is_empty()
        {
            return Err(ReplayError::Input("bounded green bridge topology differs"));
        }
        let (first, last) = domain(compiled.windows[edge_index].1);
        let (after_first, after_last) = domain(compiled.windows[edge_index + 1].1);
        let mut samples = Vec::with_capacity((last - first + 1) as usize);
        let mut redundant = true;
        for before in first..=last {
            let offset = before
                .checked_sub(bound.first_before_frame)
                .and_then(|index| bound.maximum_first_after.get(index as usize))
                .ok_or(ReplayError::Input(
                    "bounded bridge misses predecessor frame",
                ))?;
            let extremum = match side {
                Side::Safe => offset.upper,
                Side::Possible => offset.lower,
            };
            let base = before
                .checked_add(edge.minimum_gap_frames)
                .map(|f| f.max(after_first))
                .filter(|&f| f <= after_last);
            let arrival = if extremum == u32::MAX {
                None
            } else {
                base.map(|f| f.max(extremum)).filter(|&f| f <= after_last)
            };
            redundant &= arrival == base;
            samples.push(arrival);
        }
        if samples.iter().all(Option::is_none) {
            return Err(ReplayError::Input("empty bounded green dependency"));
        }
        if !redundant {
            edge.earliest_after = Some(ArrivalCurve::from_samples(first, &samples)?);
        }
    }
    for index in 0..compiled.constraints.len() {
        let (first, _) = domain(compiled.windows[index].1);
        let required = compiled.constraints[index]
            .required_after(first)
            .ok_or(ReplayError::Input("empty bounded green chain"))?;
        clip(
            &mut compiled.windows[index + 1].1,
            Some(required),
            None,
            &mut compiled.removed_frames,
        )?;
    }
    for index in (0..compiled.constraints.len()).rev() {
        let (first, last) = domain(compiled.windows[index].1);
        let (_, after_last) = domain(compiled.windows[index + 1].1);
        let latest = (first..=last)
            .rev()
            .find(|&frame| compiled.constraints[index].allows_frames(frame, after_last))
            .ok_or(ReplayError::Input("empty bounded green chain"))?;
        clip(
            &mut compiled.windows[index].1,
            None,
            Some(latest),
            &mut compiled.removed_frames,
        )?;
    }
    // Forward/backward clipping can make a previously useful bridge curve
    // redundant with the successor's final window. Remove it so search does
    // not keep a needless per-frame constraint.
    for index in 0..compiled.constraints.len() {
        if compiled.constraints[index].earliest_after.is_none() {
            continue;
        }
        let (first, last) = domain(compiled.windows[index].1);
        let (after_first, after_last) = domain(compiled.windows[index + 1].1);
        let edge = &compiled.constraints[index];
        let redundant = (first..=last).all(|before| {
            let baseline = before
                .checked_add(edge.minimum_gap_frames)
                .map(|required| required.max(after_first))
                .filter(|&required| required <= after_last);
            let bounded = edge
                .required_after(before)
                .map(|required| required.max(after_first))
                .filter(|&required| required <= after_last);
            baseline == bounded
        });
        if redundant {
            compiled.constraints[index].earliest_after = None;
        }
    }
    Ok(compiled)
}

/// Compile two outer models from proved window and hidden-bridge maxima.
/// Every plan admitted by the safe model is valid for all speeds; every
/// all-speed plan is admitted by the possible model. Exact optimum scoring
/// requires the safe and possible optimum scores to agree.
pub fn compile_bounded_projected_chain(
    windows: &[CommonWindowBounds],
    bridges: &[CommonBridgeMaxBounds],
) -> Result<(CompiledGreenChain, CompiledGreenChain), ReplayError> {
    if windows.len() < 2 || bridges.len() + 1 != windows.len() {
        return Err(ReplayError::Input("invalid bounded green chain topology"));
    }
    Ok((
        compile_side(windows, bridges, Side::Safe)?,
        compile_side(windows, bridges, Side::Possible)?,
    ))
}

/// Inclusive bound on `HiddenBridge::earliest(before)` for every speed in a
/// speed block. `u32::MAX` encodes None. The caller supplies bounds from the
/// native gate and lifetime proofs, in original hidden-node order.
///
/// For a finite expiry e and next input n, one hidden node maps n to
/// `max(n, min(max(n, ready), e)+1)`; absent ready is +infinity. If expiry is
/// absent, it maps to `max(n, ready)+1`, or +infinity if ready is absent too.
/// This transfer is monotone in n, ready and expiry, with absent values treated
/// as +infinity. Therefore lower and upper endpoint walks enclose every
/// speed's hidden bridge, without evaluating any speed variant.
pub fn hidden_bridge_earliest_bounds(
    successor: GreenRole,
    hidden: &[HiddenGateFrameBounds],
    predecessor_last: u32,
    before: u32,
) -> Result<FrameRange, ReplayError> {
    let limit = MAX_SECONDS * REPLAY_FPS;
    if matches!(successor, GreenRole::Head) || before > predecessor_last || predecessor_last > limit
    {
        return Err(ReplayError::Input("invalid hidden bridge frame premises"));
    }
    let mut previous_expiry: Option<Option<FrameRange>> = None;
    for gate in hidden {
        for range in [gate.first_ready_frame, gate.expires_after_frame]
            .into_iter()
            .flatten()
        {
            if range.lower > range.upper || range.upper > limit {
                return Err(ReplayError::Input("invalid hidden gate frame bound"));
            }
        }
        if let Some(expiry) = gate.expires_after_frame {
            if expiry.lower < predecessor_last {
                return Err(ReplayError::Input(
                    "hidden expiry can precede visible predecessor",
                ));
            }
        }
        if let Some(previous) = previous_expiry {
            let previous_upper = previous.map_or(u32::MAX, |range: FrameRange| range.upper);
            let current_lower = gate
                .expires_after_frame
                .map_or(u32::MAX, |range| range.lower);
            if previous_upper > current_lower {
                return Err(ReplayError::Input(
                    "hidden expiry order is not interval-certified",
                ));
            }
        }
        previous_expiry = Some(gate.expires_after_frame);
    }
    let first = before
        .checked_add(1)
        .ok_or(ReplayError::Input("hidden bridge frame overflow"))?;
    if matches!(successor, GreenRole::PlainEnd | GreenRole::HeldArrowEnd) {
        return FrameRange::new(first, first);
    }
    let transfer = |upper: bool| -> Result<u32, ReplayError> {
        let mut next = first;
        for gate in hidden {
            let ready = gate
                .first_ready_frame
                .map(|range| if upper { range.upper } else { range.lower });
            let expiry =
                gate.expires_after_frame
                    .map(|range| if upper { range.upper } else { range.lower });
            next = match (ready, expiry) {
                (_, Some(expiry)) => {
                    let callback = ready.unwrap_or(u32::MAX).max(next).min(expiry);
                    next.max(
                        callback
                            .checked_add(1)
                            .ok_or(ReplayError::Input("hidden expiry frame overflow"))?,
                    )
                }
                (Some(ready), None) => next
                    .max(ready)
                    .checked_add(1)
                    .ok_or(ReplayError::Input("hidden callback frame overflow"))?,
                (None, None) => u32::MAX,
            };
            if next == u32::MAX {
                break;
            }
        }
        Ok(next)
    };
    FrameRange::new(transfer(false)?, transfer(true)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::green::projection::{HiddenBridge, HiddenGate};

    #[test]
    fn endpoint_transfer_contains_every_concrete_hidden_bridge() {
        let hidden = [
            HiddenGateFrameBounds {
                first_ready_frame: Some(FrameRange::new(22, 24).unwrap()),
                expires_after_frame: Some(FrameRange::new(28, 30).unwrap()),
            },
            HiddenGateFrameBounds {
                first_ready_frame: Some(FrameRange::new(25, 28).unwrap()),
                expires_after_frame: Some(FrameRange::new(33, 35).unwrap()),
            },
            HiddenGateFrameBounds {
                first_ready_frame: None,
                expires_after_frame: Some(FrameRange::new(39, 41).unwrap()),
            },
        ];
        for before in 18..=20 {
            let bound =
                hidden_bridge_earliest_bounds(GreenRole::HeldMiddle, &hidden, 20, before).unwrap();
            for ready0 in 22..=24 {
                for expiry0 in 28..=30 {
                    for ready1 in 25..=28 {
                        for expiry1 in 33..=35 {
                            for expiry2 in 39..=41 {
                                let bridge = HiddenBridge {
                                    hidden: vec![
                                        HiddenGate {
                                            first_ready_frame: Some(ready0),
                                            expires_after_frame: Some(expiry0),
                                        },
                                        HiddenGate {
                                            first_ready_frame: Some(ready1),
                                            expires_after_frame: Some(expiry1),
                                        },
                                        HiddenGate {
                                            first_ready_frame: None,
                                            expires_after_frame: Some(expiry2),
                                        },
                                    ],
                                    successor: GreenRole::HeldMiddle,
                                };
                                let actual = bridge.earliest(before).unwrap();
                                assert!(bound.lower <= actual && actual <= bound.upper);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn terminal_successor_skips_hidden_run_on_next_callback() {
        let hidden = [HiddenGateFrameBounds {
            first_ready_frame: None,
            expires_after_frame: Some(FrameRange::new(30, 31).unwrap()),
        }];
        assert_eq!(
            hidden_bridge_earliest_bounds(GreenRole::PlainEnd, &hidden, 20, 20).unwrap(),
            FrameRange::new(21, 21).unwrap()
        );
    }

    #[test]
    fn bounded_models_keep_safe_constraints_inside_possible_constraints() {
        let windows = [
            CommonWindowBounds {
                note_index: 4,
                first_frames: [FrameRange::new(100, 102).unwrap(); 4],
                last_frame: FrameRange::new(130, 132).unwrap(),
            },
            CommonWindowBounds {
                note_index: 9,
                first_frames: [FrameRange::new(105, 107).unwrap(); 4],
                last_frame: FrameRange::new(160, 162).unwrap(),
            },
        ];
        let bridge = CommonBridgeMaxBounds {
            predecessor_note_index: 4,
            successor_note_index: 9,
            first_before_frame: 100,
            maximum_first_after: (100..=132)
                .map(|before| FrameRange::new(before + 3, before + 4).unwrap())
                .collect(),
        };
        let (safe, possible) = compile_bounded_projected_chain(&windows, &[bridge]).unwrap();
        for (safe_window, possible_window) in safe.windows.iter().zip(&possible.windows) {
            let (safe_first, safe_last) = domain(safe_window.1);
            let (possible_first, possible_last) = domain(possible_window.1);
            assert!(possible_first <= safe_first && safe_last <= possible_last);
        }
        for before in domain(safe.windows[0].1).0..=domain(safe.windows[0].1).1 {
            assert!(
                safe.constraints[0].required_after(before)
                    >= possible.constraints[0].required_after(before)
            );
        }
    }

    #[test]
    fn clipping_removes_bridge_curve_covered_by_successor_window() {
        let windows = [
            CommonWindowBounds {
                note_index: 1,
                first_frames: [FrameRange::new(100, 100).unwrap(); 4],
                last_frame: FrameRange::new(105, 105).unwrap(),
            },
            CommonWindowBounds {
                note_index: 2,
                first_frames: [FrameRange::new(105, 105).unwrap(); 4],
                last_frame: FrameRange::new(110, 110).unwrap(),
            },
        ];
        let bridge = CommonBridgeMaxBounds {
            predecessor_note_index: 1,
            successor_note_index: 2,
            first_before_frame: 100,
            maximum_first_after: vec![FrameRange::new(106, 106).unwrap(); 6],
        };
        let (safe, possible) = compile_bounded_projected_chain(&windows, &[bridge]).unwrap();
        for model in [safe, possible] {
            assert_eq!(domain(model.windows[1].1).0, 106);
            assert!(model.constraints[0].earliest_after.is_none());
        }
    }
}
