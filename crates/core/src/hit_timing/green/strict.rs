//! User-selected conservative policy: common timing domain, worst raw grade.
//! Merge VERIFIED speed variants during preparation, never per team or order.
//! This proves intersection only for the supplied variants, not their coverage
//! of all game settings. Adjust B and the chart clock are held at declared values.
//! A synthetic speed sweep must not be labelled a certified native chart table.
use super::compact::{compile_chain, CompiledGreenChain, GreenChainNode};
use crate::hit_timing::{NoteWindow, ReplayError, ReplayNote};

fn cuts(window: NoteWindow) -> Result<([u32; 4], u32), ReplayError> {
    let NoteWindow::NativeGreen {
        first_frames,
        last_frame,
    } = window
    else {
        return Err(ReplayError::Input(
            "strict green policy requires native green domains",
        ));
    };
    ReplayNote {
        time: 0.,
        trigger: None,
        window,
    }
    .validate_window()?;
    Ok((first_frames, last_frame))
}

/// For grade g: intersection_s [first_s(g), last_s] equals
/// [max_s first_s(g), min_s last_s]. Hence each frame has the minimum raw grade
/// over variants. Apply active correction skills AFTER this merge; never turn an
/// unavailable/Miss frame into a valid note. Empty overlap is an explicit error.
/// Error-bearing inputs let native lifetime compilation failures propagate.
pub fn intersect_windows(
    windows: impl IntoIterator<Item = Result<NoteWindow, ReplayError>>,
) -> Result<NoteWindow, ReplayError> {
    let mut windows = windows.into_iter();
    let first = windows
        .next()
        .ok_or(ReplayError::Input("no strict green variants"))??;
    let (mut starts, mut last) = cuts(first)?;
    for window in windows {
        let (other_starts, other_last) = cuts(window?)?;
        last = last.min(other_last);
        for (start, other) in starts.iter_mut().zip(other_starts) {
            *start = (*start).max(other);
        }
    }
    if starts[0] > last {
        return Err(ReplayError::Input("empty strict green judgement domain"));
    }
    // An unavailable higher grade uses last+1, not a frame outside the lifetime.
    let first_frames = starts.map(|frame| frame.min(last + 1));
    Ok(NoteWindow::NativeGreen {
        first_frames,
        last_frame: last,
    })
}

/// Merge the SAME ordered scoring chain across speed variants. Native lifecycle
/// and hidden-node projection must already have supplied valid per-variant
/// domains/edges. For each edge take the largest required gap, then propagate
/// the common domains again: individually feasible chains may have no common plan.
/// Returns the existing chain representation, with no speed in any search state.
/// `removed_frames` counts the final propagation, not discarded variant frames.
pub fn compile_strict_chain<V: AsRef<[GreenChainNode]>>(
    variants: impl IntoIterator<Item = V>,
) -> Result<CompiledGreenChain, ReplayError> {
    let mut variants = variants.into_iter();
    let first = variants
        .next()
        .ok_or(ReplayError::Input("no strict green variants"))?;
    let mut common = compile_chain(first.as_ref())?;
    for variant in variants {
        let other = compile_chain(variant.as_ref())?;
        if common.windows.len() != other.windows.len()
            || common
                .windows
                .iter()
                .zip(&other.windows)
                .any(|(a, b)| a.0 != b.0)
        {
            return Err(ReplayError::Input(
                "strict green variants describe different chains",
            ));
        }
        for (window, other) in common.windows.iter_mut().zip(other.windows) {
            window.1 = intersect_windows([Ok(window.1), Ok(other.1)])?;
        }
        for (edge, other) in common.constraints.iter_mut().zip(other.constraints) {
            edge.minimum_gap_frames = edge.minimum_gap_frames.max(other.minimum_gap_frames);
        }
    }
    let nodes: Vec<_> = common
        .windows
        .into_iter()
        .enumerate()
        .map(|(i, (note_index, window))| GreenChainNode {
            note_index,
            window,
            minimum_gap_from_previous: if i == 0 {
                0
            } else {
                common.constraints[i - 1].minimum_gap_frames
            },
        })
        .collect();
    compile_chain(&nodes)
}

#[cfg(test)]
mod tests;
