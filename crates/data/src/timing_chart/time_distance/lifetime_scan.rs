//! Shared scans over the already prepared binary32 music clock.
use bangdream_optimize_core::hit_timing::{
    green::lifetime::{NativeClock, MISS_SECOND_INTERVAL}, ReplayError,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct TimeoutNode {
    pub first_clock: u32,
    pub absolute_pos: i32,
}

/// Nodes have nondecreasing positions AND first-handler frames (chart order).
/// At a fixed frame late(A) is nonincreasing in A, including f32 rounding.
/// Unexpired eligible nodes therefore form a queue. A BPM change can make a
/// node's predicate fall again, but cannot undo an already removed node.
pub(super) fn timeout_last_frames(clocks: &[NativeClock], nodes: &[TimeoutNode]) -> Result<Vec<u32>, ReplayError> {
    timeout_last_frames_counted(clocks, nodes, || {})
}

fn timeout_last_frames_counted(clocks: &[NativeClock], nodes: &[TimeoutNode], mut checked: impl FnMut()) -> Result<Vec<u32>, ReplayError> {
    debug_assert!(!clocks.is_empty());
    debug_assert!(nodes.windows(2).all(|w| w[0].first_clock <= w[1].first_clock && w[0].absolute_pos <= w[1].absolute_pos));
    let horizon = clocks.len() as u32 - 1;
    let mut last = vec![horizon; nodes.len()];
    let (mut head, mut ready) = (0, 0);
    let mut frame = nodes.first().map_or(horizon + 1, |node| node.first_clock);
    while frame <= horizon {
        while ready < nodes.len() && nodes[ready].first_clock <= frame { ready += 1; }
        let clock = clocks[frame as usize];
        while head < ready {
            checked();
            if clock.late_seconds(nodes[head].absolute_pos)? <= MISS_SECOND_INTERVAL {
                // Skipping interior nodes must not hide negative overflow.
                // Finite arithmetic at both extremes bounds all intermediate
                // positions. The head already checked the upper extreme.
                if head + 1 < ready {
                    checked();
                    clock.late_seconds(nodes[ready - 1].absolute_pos)?;
                }
                break;
            }
            last[head] = frame.checked_sub(1).ok_or(ReplayError::Input("empty slide lifetime"))?;
            head += 1;
        }
        frame += 1;
        if head == ready {
            // No active checks between this frame and the next admission.
            frame = frame.max(nodes.get(ready).map_or(horizon + 1, |node| node.first_clock));
        }
    }
    Ok(last)
}

/// Music progress is nondecreasing over the supported 72,000-update horizon.
/// Search the monotone crossing first; a middle node additionally requires
/// C < next. If the first crossing skips that interval, no later sample can
/// enter it. Keep the two original f32 subtractions, not (current+next)/2.
pub(super) fn midpoint_expiry(clocks: &[NativeClock], first: u32, last: u32,
    current: f32, next: f32, head: bool) -> Option<u32> {
    midpoint_expiry_counted(clocks, first, last, current, next, head, || {})
}

fn midpoint_expiry_counted(clocks: &[NativeClock], first: u32, last: u32,
    current: f32, next: f32, head: bool, mut checked: impl FnMut()) -> Option<u32> {
    if first > last { return None; }
    let range = &clocks[first as usize..=last as usize];
    // Dense chains often cross the midpoint before the first Stop handler.
    // Resolve that common case with one check instead of a full binary search.
    checked();
    let until = next - range[0].adjusted_music_pos;
    let since = range[0].adjusted_music_pos - current;
    if !head && until <= 0. { return None; }
    if (head && until <= 0.) || since > until { return Some(first); }
    let offset = 1 + range[1..].partition_point(|clock| {
        checked();
        let until = next - clock.adjusted_music_pos;
        let since = clock.adjusted_music_pos - current;
        !((head && until <= 0.) || since > until)
    });
    let clock = range.get(offset)?;
    if !head && next - clock.adjusted_music_pos <= 0. { return None; }
    Some(first + offset as u32)
}

#[cfg(test)]
mod tests;
