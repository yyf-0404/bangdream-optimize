//! Exact finite-domain reduction. Positions are examined once during preparation;
//! score/search paths receive integer frame cuts and explicit causal constraints.
//! This does NOT infer native launch state, hidden-node callbacks or lifetime.
use super::{GreenRole, MAX_SECONDS, REPLAY_FPS};
use crate::hit_timing::{NoteConstraint, NoteWindow, ReplayError, ReplayNote};
use std::collections::HashSet;

/// Six nested position gates + last active frame, independent of team/skills.
/// No progress inversion, powf, coordinates or per-frame array survives here.
#[derive(Clone, Copy, Debug)]
pub struct GreenFrameProfile {
    // Head/end B,Gd,G,P; held-middle P; held-arrow-end P.
    first: [u32; 6],
    last: u32,
}
impl GreenFrameProfile {
    /// Supplied local positions must cover every frame of a verified, contiguous
    /// active lifetime. Nonmonotone gate outcomes cannot use this representation.
    pub fn compile(
        table: &[f32; 16],
        first_frame: u32,
        positions: impl IntoIterator<Item = f32>,
    ) -> Result<Self, ReplayError> {
        if table.iter().any(|y| !y.is_finite()) || table.windows(2).any(|w| w[0] <= w[1]) {
            return Err(ReplayError::Input("invalid native green position table"));
        }
        let gates = [table[0], table[1], table[2], table[5], table[7], table[8]];
        let mut first = [u32::MAX; 6];
        let mut last = None;
        let mut previous = 0;
        for (offset, y) in positions.into_iter().enumerate() {
            let frame = u32::try_from(offset)
                .ok()
                .and_then(|n| first_frame.checked_add(n))
                .filter(|&f| f <= MAX_SECONDS * REPLAY_FPS)
                .ok_or(ReplayError::Input("invalid green lifetime"))?;
            if !y.is_finite() {
                return Err(ReplayError::Input("invalid native green position"));
            }
            // Every gate >= Y[8], so max(y,Y[8]) <= gate iff y <= gate.
            // Equality is retained even when distinct f32 progresses share a Y.
            let crossed = gates.partition_point(|&boundary| y <= boundary);
            if crossed < previous {
                return Err(ReplayError::Input("nonmonotone green gates"));
            }
            for cut in &mut first[previous..crossed] {
                *cut = frame;
            }
            previous = crossed;
            last = Some(frame);
        }
        let last = last.ok_or(ReplayError::Input("invalid green lifetime"))?;
        for cut in &mut first {
            if *cut == u32::MAX {
                *cut = last + 1;
            }
        }
        Ok(Self { first, last })
    }

    pub fn window(self, role: GreenRole) -> Result<NoteWindow, ReplayError> {
        let first_frames = match role {
            GreenRole::Head | GreenRole::PlainEnd => self.first[..4].try_into().unwrap(),
            GreenRole::HeldMiddle => [self.first[4]; 4],
            GreenRole::HeldArrowEnd => [self.first[5]; 4],
        };
        if first_frames[0] > self.last {
            return Err(ReplayError::Input("empty green judgement domain"));
        }
        Ok(NoteWindow::NativeGreen {
            first_frames,
            last_frame: self.last,
        })
    }
}

/// A scoring node in an ALREADY VERIFIED causal chain. Hidden nodes may only
/// have been removed after proving their native callback/skip semantics.
#[derive(Clone, Copy, Debug)]
pub struct GreenChainNode {
    pub note_index: usize,
    pub window: NoteWindow,
    /// The first node must use zero. Later nodes always require at least one
    /// frame, even if this supplied lower bound is zero. This is a user-selected
    /// restriction, not a proven native timing gate.
    pub minimum_gap_from_previous: u32,
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledGreenChain {
    pub windows: Vec<(usize, NoteWindow)>,
    pub constraints: Vec<NoteConstraint>,
    pub removed_frames: u64,
}

/// Linear propagation for ONE continuous touch, including its scoring head.
/// Consecutive scoring nodes occupy distinct frames under the declared model.
/// Hidden nodes do not consume a scoring frame; different chains stay independent.
/// This restriction is not a native-game conclusion.
/// Raw Bad starts are retained: an active correction skill may make them legal.
/// Bounds are only intersected, so scoring and skill queue histories are unchanged.
/// Constraints remain present after clipping; independent clipped windows alone
/// do not encode correlations or same-frame dispatch order.
pub fn compile_chain(nodes: &[GreenChainNode]) -> Result<CompiledGreenChain, ReplayError> {
    if nodes.is_empty() || nodes[0].minimum_gap_from_previous != 0 {
        return Err(ReplayError::Input("invalid green chain"));
    }
    let mut seen = HashSet::new();
    let mut starts = Vec::with_capacity(nodes.len());
    let mut ends = Vec::with_capacity(nodes.len());
    let mut grades = Vec::with_capacity(nodes.len());
    let mut original = 0u64;
    for node in nodes {
        if !seen.insert(node.note_index) {
            return Err(ReplayError::Input("green chain repeats a scoring node"));
        }
        let NoteWindow::NativeGreen {
            first_frames,
            last_frame,
        } = node.window
        else {
            return Err(ReplayError::Input(
                "green chain requires native green domains",
            ));
        };
        ReplayNote {
            time: 0.,
            trigger: None,
            window: node.window,
        }
        .validate_window()?;
        starts.push(first_frames[0]);
        ends.push(last_frame);
        grades.push(first_frames);
        original += u64::from(last_frame - first_frames[0] + 1);
    }
    for i in 1..nodes.len() {
        let required = starts[i - 1]
            .checked_add(nodes[i].minimum_gap_from_previous.max(1))
            .ok_or(ReplayError::Input("empty green chain domain"))?;
        starts[i] = starts[i].max(required);
    }
    for i in (0..nodes.len() - 1).rev() {
        let latest = ends[i + 1]
            .checked_sub(nodes[i + 1].minimum_gap_from_previous.max(1))
            .ok_or(ReplayError::Input("empty green chain domain"))?;
        ends[i] = ends[i].min(latest);
    }
    let mut windows = Vec::with_capacity(nodes.len());
    let mut remaining = 0u64;
    for (i, node) in nodes.iter().enumerate() {
        if starts[i] > ends[i] {
            return Err(ReplayError::Input("empty green chain domain"));
        }
        let first_frames = grades[i].map(|cut| cut.max(starts[i]).min(ends[i] + 1));
        remaining += u64::from(ends[i] - starts[i] + 1);
        windows.push((
            node.note_index,
            NoteWindow::NativeGreen {
                first_frames,
                last_frame: ends[i],
            },
        ));
    }
    let constraints = nodes
        .windows(2)
        .map(|pair| NoteConstraint {
            before: pair[0].note_index,
            after: pair[1].note_index,
            minimum_gap_frames: pair[1].minimum_gap_from_previous.max(1),
            require_dispatch_order: true,
            earliest_after: None,
        })
        .collect();
    Ok(CompiledGreenChain {
        windows,
        constraints,
        removed_frames: original - remaining,
    })
}

#[cfg(test)]
mod tests;
