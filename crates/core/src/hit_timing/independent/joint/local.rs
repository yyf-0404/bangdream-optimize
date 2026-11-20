//! Conservative skill envelopes and the notes whose placement may affect them.
//! Endpoints use the same integer FIFO clock as JointCosts. Raw Bad trigger
//! domains over-cover every accepted request, including repaired requests.
use super::*;

mod bad_bounds;
pub(super) use bad_bounds::BadBounds;

pub(super) fn bad_scan_enabled() -> bool {
    std::env::var("BANDORI_TIMING_DISABLE_BAD_INTERVAL_SCAN").as_deref() != Ok("1")
}

pub(super) fn enabled() -> bool {
    std::env::var("BANDORI_TIMING_DISABLE_JOINT_LOCAL_GEOMETRY").as_deref() != Ok("1")
}

pub(super) fn regions(input: &ReplayInput) -> Result<Vec<(u32, u32)>, ReplayError> {
    let ranges = geometry::skill_ranges(input, |index| {
        let frames = input.notes[index].frame_domain(Judgement::Bad)?;
        Ok((frames.first().copied().ok_or(ReplayError::Input("empty joint trigger domain"))?,
            frames.last().copied().unwrap()))
    })?;
    Ok(merge_regions(&ranges))
}

fn merge_regions(ranges: &[(usize, u32, u32)]) -> Vec<(u32, u32)> {
    let mut regions: Vec<(u32, u32)> = vec![];
    for &(_, first, last) in ranges {
        let range = (first, last);
        if let Some(previous) = regions.last_mut().filter(|r| range.0 <= r.1 + 1) {
            previous.1 = previous.1.max(range.1);
        } else {
            regions.push(range);
        }
    }
    regions
}

pub(super) fn protected(input: &ReplayInput) -> Result<Vec<bool>, ReplayError> {
    if bad_scan_enabled() {
        let bad = BadBounds::new(&input.notes)?;
        let ranges = geometry::skill_ranges(input, |index| Ok(bad.bounds[index]))?;
        return Ok(bad.protected(&input.notes, &merge_regions(&ranges)));
    }
    protected_legacy(input)
}

fn protected_legacy(input: &ReplayInput) -> Result<Vec<bool>, ReplayError> {
    let ranges = regions(input)?;
    input.notes.iter().map(|note| {
        let frames = note.frame_domain(Judgement::Bad)?;
        Ok(note.trigger.is_some() || frames.iter()
            .any(|&frame| ranges.iter().any(|&(a, b)| a <= frame && frame <= b)))
    }).collect()
}

pub(super) fn geometry_key(
    input: &ReplayInput,
    enabled: bool,
) -> Result<Vec<(u8, u32, u32)>, ReplayError> {
    Ok(geometry(input, enabled)?.key)
}

pub(crate) struct JointGeometry {
    pub key: Vec<(u8, u32, u32)>,
    pub corrections: Vec<(Judgement, u32, u32)>,
    pub protected: Vec<bool>,
}

pub(super) fn geometry(input: &ReplayInput, enabled: bool) -> Result<JointGeometry, ReplayError> {
    crate::hit_timing::validate(input)?;
    if enabled && bad_scan_enabled() {
        return geometry_with_bounds(input, &BadBounds::new(&input.notes)?);
    }
    let corrections = geometry::correction_ranges(input)?;
    let protected = if enabled { protected_legacy(input)? } else { vec![] };
    Ok(layout(corrections, protected, enabled))
}

/// Input validation is performed by the caller before using fixed-chart bounds.
pub(super) fn geometry_with_bounds(input: &ReplayInput, bad: &BadBounds)
    -> Result<JointGeometry, ReplayError> {
    let ranges = geometry::skill_ranges(input, |index| Ok(bad.bounds[index]))?;
    let corrections = geometry::corrections_from_ranges(input, &ranges);
    let protected = bad.protected(&input.notes, &merge_regions(&ranges));
    Ok(layout(corrections, protected, true))
}

pub(super) fn audit_geometry(input: &ReplayInput, actual: &JointGeometry) -> Result<(), ReplayError> {
    let expected = layout(geometry::correction_ranges(input)?, protected_legacy(input)?, true);
    if actual.key != expected.key || actual.corrections != expected.corrections
        || actual.protected != expected.protected {
        return Err(ReplayError::Input("Bad interval geometry differs from exhaustive frame scan"));
    }
    Ok(())
}

fn layout(corrections: Vec<(Judgement, u32, u32)>, protected: Vec<bool>, enabled: bool)
    -> JointGeometry {
    let mut key: Vec<_> = corrections.iter().map(|&(g, a, b)| (g as u8, a, b)).collect();
    if enabled {
        // Raw correction domains still determine the graph's legal choices.
        // For compression, only the protected-note mask matters. Different
        // durations with the same mask safely share the same geometry.
        key.push((u8::MAX, 0, 0));
        key.extend(protected.iter().enumerate()
            .filter_map(|(i, &active)| active.then_some((u8::MAX, i as u32, 1))));
    }
    JointGeometry { key, corrections, protected }
}
