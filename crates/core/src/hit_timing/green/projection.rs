//! Preparation-only elimination of ordered hidden runs, fixed B=0 cadence.
//! Inputs are verified fixed gate/lifetime facts, NOT inferred chart times.
//! A hidden expiry may advance a non-current node; ordered expiries reduce this
//! to prefix advancement. Reject inputs outside that proof rather than relax.
//! The resulting scorer sees only visible windows and F(predecessor_frame).
use super::{
    compact::{CompiledGreenChain, GreenChainNode},
    strict::compile_strict_chain,
    GreenRole,
};
use crate::hit_timing::{
    ArrivalCurve, NoteWindow, ReplayError, ReplayInput, MAX_SECONDS, REPLAY_FPS,
};

#[derive(Clone, Copy, Debug)]
pub struct HiddenGate {
    /// First input frame meeting the active Move gate; thereafter it stays true
    /// through expiry. None means no successful Move before automatic expiry.
    pub first_ready_frame: Option<u32>,
    /// First post-input update that expires this node if it remains unjudged.
    pub expires_after_frame: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct HiddenBridge {
    pub hidden: Vec<HiddenGate>,
    pub successor: GreenRole,
}

/// Preparation scratch data only. Each bridge joins two consecutive visible
/// scoring nodes. No hidden object is stored in ReplayInput.
pub struct GreenChainVariant<'a> {
    pub nodes: &'a [GreenChainNode],
    pub bridges: &'a [HiddenBridge],
    pub adjust_b: i32,
}

fn domain(window: NoteWindow) -> (u32, u32) {
    match window {
        NoteWindow::NativeGreen {
            first_frames,
            last_frame,
        } => (first_frames[0], last_frame),
        _ => unreachable!("compile_strict_chain validated native green domains"),
    }
}

impl HiddenBridge {
    fn validate(&self, predecessor_last: u32) -> Result<(), ReplayError> {
        let limit = MAX_SECONDS * REPLAY_FPS;
        if matches!(self.successor, GreenRole::Head) {
            return Err(ReplayError::Input("head cannot follow a hidden bridge"));
        }
        for h in &self.hidden {
            if h.first_ready_frame.is_some_and(|f| f > limit)
                || h.expires_after_frame.is_some_and(|f| f > limit)
            {
                return Err(ReplayError::Input("invalid hidden gate frame"));
            }
            // An unjudged future hidden expiry can jump over an earlier visible
            // note. This reduction cannot encode that as just a pairwise edge.
            // Equality is safe: predecessor input happens before that update.
            if h.expires_after_frame.is_some_and(|f| f < predecessor_last) {
                return Err(ReplayError::Input(
                    "hidden expiry can precede its visible predecessor",
                ));
            }
        }
        if self.hidden.windows(2).any(|p| {
            p[0].expires_after_frame.unwrap_or(u32::MAX)
                > p[1].expires_after_frame.unwrap_or(u32::MAX)
        }) {
            return Err(ReplayError::Input(
                "hidden expiry order needs a richer projection",
            ));
        }
        Ok(())
    }

    /// Earliest next scoring callback, excluding the successor's own window.
    /// Gates/expiry have already been validated. Input precedes update.
    pub(super) fn earliest(&self, before: u32) -> Option<u32> {
        let mut next = before.checked_add(1)?;
        // Native Ended can skip a terminal hidden run in that same callback.
        if matches!(
            self.successor,
            GreenRole::PlainEnd | GreenRole::HeldArrowEnd
        ) {
            return Some(next);
        }
        let mut cursor = 0;
        while cursor < self.hidden.len() {
            let expired = self
                .hidden
                .partition_point(|h| h.expires_after_frame.is_some_and(|f| f < next));
            cursor = cursor.max(expired);
            if cursor == self.hidden.len() {
                break;
            }
            let h = self.hidden[cursor];
            let moved = h.first_ready_frame.map(|f| f.max(next));
            if let Some(frame) = moved.filter(|&f| h.expires_after_frame.is_none_or(|e| f <= e)) {
                // B=0 consumes this frame's sole Move callback even without score.
                cursor += 1;
                next = frame.checked_add(1)?;
            } else if let Some(expiry) = h.expires_after_frame {
                // Automatic prefix advancement is usable starting next input.
                next = expiry.checked_add(1)?;
            } else {
                return None;
            }
        }
        Some(next)
    }
}

/// Compile all supplied speed variants into one common visible-node model.
/// A successor frame is legal iff b >= max_s F_s(a), with common raw windows.
/// Earliest advancement is dominating because both Move and ordered expiry are
/// monotone in the hidden cursor, and later scoring is possible by waiting.
/// This proof requires fixed paths, ordered expiries and no earlier visible skip;
/// the constructor checks the finite-data parts of those preconditions.
pub fn compile_projected_chain(
    variants: &[GreenChainVariant<'_>],
) -> Result<CompiledGreenChain, ReplayError> {
    let mut common = compile_strict_chain(variants.iter().map(|v| v.nodes))?;
    for variant in variants {
        if variant.adjust_b != 0 {
            return Err(ReplayError::Input(
                "hidden projection currently requires B=0",
            ));
        }
        if variant.bridges.len() + 1 != variant.nodes.len() {
            return Err(ReplayError::Input(
                "hidden bridges do not match visible chain",
            ));
        }
        for (i, bridge) in variant.bridges.iter().enumerate() {
            bridge.validate(domain(common.windows[i].1).1)?;
            let first = variants[0].bridges.get(i).ok_or(ReplayError::Input(
                "hidden bridges do not match visible chain",
            ))?;
            if std::mem::discriminant(&bridge.successor) != std::mem::discriminant(&first.successor)
            {
                return Err(ReplayError::Input("green variants disagree on node roles"));
            }
        }
    }
    for (i, edge) in common.constraints.iter_mut().enumerate() {
        let (first, last) = domain(common.windows[i].1);
        let (after_first, after_last) = domain(common.windows[i + 1].1);
        let mut samples = Vec::with_capacity((last - first + 1) as usize);
        let mut redundant = true;
        for before in first..=last {
            let base = before
                .checked_add(edge.minimum_gap_frames)
                .map(|f| f.max(after_first))
                .filter(|&f| f <= after_last);
            let arrival = base.and_then(|base| {
                variants.iter().try_fold(base, |f, v| {
                    v.bridges[i].earliest(before).map(|other| f.max(other))
                })
            });
            let arrival = arrival.filter(|&f| f <= after_last);
            redundant &= arrival == base;
            samples.push(arrival);
        }
        if samples.iter().all(Option::is_none) {
            return Err(ReplayError::Input("empty projected green dependency"));
        }
        if !redundant {
            edge.earliest_after = Some(ArrivalCurve::from_samples(first, &samples)?);
        }
    }
    // Exact chain support propagation for monotone upper-closed edges. This only
    // tightens windows; the correlations remain encoded in the edges themselves.
    for i in 0..common.constraints.len() {
        let (first, _) = domain(common.windows[i].1);
        let required = common.constraints[i]
            .required_after(first)
            .ok_or(ReplayError::Input("empty projected green chain"))?;
        clip(
            &mut common.windows[i + 1].1,
            Some(required),
            None,
            &mut common.removed_frames,
        )?;
    }
    for i in (0..common.constraints.len()).rev() {
        let (first, last) = domain(common.windows[i].1);
        let (_, after_last) = domain(common.windows[i + 1].1);
        let latest = (first..=last)
            .rev()
            .find(|&f| common.constraints[i].allows_frames(f, after_last))
            .ok_or(ReplayError::Input("empty projected green chain"))?;
        clip(
            &mut common.windows[i].1,
            None,
            Some(latest),
            &mut common.removed_frames,
        )?;
    }
    Ok(common)
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
        unreachable!()
    };
    let old_count = *last_frame - first_frames[0] + 1;
    let start = first_frames[0].max(first.unwrap_or(0));
    let end = (*last_frame).min(last.unwrap_or(u32::MAX));
    if start > end {
        return Err(ReplayError::Input("empty projected green chain"));
    }
    *first_frames = first_frames.map(|f| f.max(start).min(end + 1));
    *last_frame = end;
    *removed += u64::from(old_count - (end - start + 1));
    Ok(())
}

impl CompiledGreenChain {
    /// Atomic installation into the EXISTING node/constraint table. Preserve
    /// all other constraints/routing. A supplied baseline must still replay:
    /// never silently move a skill trigger or invent a replacement judgment.
    pub fn apply_to(&self, input: &mut ReplayInput) -> Result<(), ReplayError> {
        let mut trial = input.clone();
        for &(i, window) in &self.windows {
            let note = trial.notes.get_mut(i).ok_or(ReplayError::Input(
                "projected green node is absent from chart",
            ))?;
            note.window = window;
        }
        trial.constraints.extend(self.constraints.iter().cloned());
        crate::hit_timing::replay(&trial)?;
        *input = trial;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
