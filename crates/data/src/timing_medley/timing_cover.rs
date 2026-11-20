//! Replacement availability for five distinct characters and three teams.
//! Four teammates block entire character buckets. The other two teams can
//! consume at most ten cards and at most two cards per remaining character.
pub struct Group {
    pub character: u32,
    pub breaks: Vec<u8>,
    pub witnesses: usize,
}

#[path = "timing_cover/precise.rs"]
mod precise;
pub use precise::precise_free_replacements;

/// 0: current coarse cover; 1: exact occupancy with all songs allowed;
/// 2: global other-song eligibility; 3: also narrow target song obligations;
/// 4: additionally allow different replacement witnesses for each target song.
pub(super) fn mode() -> u8 {
    match std::env::var("BANDORI_TIMING_PRECISE_COVER").as_deref() {
        Ok("1") => 1,
        Ok("2") => 2,
        Ok("3") => 3,
        Ok("4") => 4,
        _ => 0,
    }
}

/// 0: historical research refinement; 1: repeat condition-preserving cross
/// cover; 2: also refresh the cross-stage context after same-role deletions.
/// Kept separate from precise occupancy so each contribution can be measured.
pub(super) fn repeat_mode() -> u8 {
    match std::env::var("BANDORI_TIMING_REPEAT_COVER").as_deref() {
        Ok("1") => 1,
        Ok("2") => 2,
        _ => 0,
    }
}

/// 1: relax initial cross-category replacements; 2: also allow refinement
/// transitions certified to stay inside the previous membership snapshot.
pub(super) fn unification_mode() -> u8 {
    match std::env::var("BANDORI_TIMING_UNIFICATION_COVER").as_deref() {
        Ok("1") => 1,
        Ok("2") => 2,
        _ => 0,
    }
}

/// A lower bound on free replacements for EVERY legal set of four teammates.
/// Band/attribute break masks enforce the exact mixed/unified signature.
pub fn free_replacements(groups: &[Group], target: u32, required: u8) -> Option<usize> {
    let total: usize = groups.iter().map(|g| g.witnesses).sum();
    let excess: usize = groups.iter().map(|g| g.witnesses.saturating_sub(2)).sum();
    // Independent maxima can come from different blockers; combining their
    // lower bounds is conservative. No Pareto frontier or roster enumeration.
    let mut dp = [[None::<[usize; 2]>; 4]; 5];
    dp[0][0] = Some([0, 0]);
    for group in groups {
        if group.character == target {
            continue;
        }
        for count in (0..4).rev() {
            for mask in 0..4 {
                let Some(previous) = dp[count][mask] else {
                    continue;
                };
                for &flags in &group.breaks {
                    let next = mask | usize::from(flags & required);
                    let candidate = [
                        previous[0] + group.witnesses,
                        previous[1] + group.witnesses.saturating_sub(2),
                    ];
                    let best = dp[count + 1][next].get_or_insert(candidate);
                    best[0] = best[0].max(candidate[0]);
                    best[1] = best[1].max(candidate[1]);
                }
            }
        }
    }
    let blocked = dp[4][usize::from(required)]?;
    Some(
        total
            .saturating_sub(blocked[0] + 10)
            .max(excess.saturating_sub(blocked[1])),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(groups: &[Group], target: u32, required: u8) -> Option<usize> {
        let available: Vec<_> = (0..groups.len())
            .filter(|&i| groups[i].character != target)
            .collect();
        let mut minimum = None;
        for a in 0..available.len() {
            for b in a + 1..available.len() {
                for c in b + 1..available.len() {
                    for d in c + 1..available.len() {
                        let blocked = [available[a], available[b], available[c], available[d]];
                        let mut masks = vec![0];
                        for &i in &blocked {
                            masks = masks
                                .iter()
                                .flat_map(|&m| {
                                    groups[i].breaks.iter().map(move |&f| m | (f & required))
                                })
                                .collect();
                        }
                        if !masks.contains(&required) {
                            continue;
                        }
                        let remaining: Vec<_> = groups
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| !blocked.contains(i))
                            .map(|(_, g)| g.witnesses)
                            .collect();
                        let free = remaining.iter().sum::<usize>()
                            - remaining.iter().map(|n| (*n).min(2)).sum::<usize>().min(10);
                        minimum = Some(minimum.map_or(free, |old: usize| old.min(free)));
                    }
                }
            }
        }
        minimum
    }

    #[test]
    fn cover_keeps_role_conflicts_and_signature_breaks() {
        let mut groups: Vec<_> = (0..6)
            .map(|i| Group {
                character: i,
                breaks: vec![u8::from(i == 5)],
                witnesses: if (1..=4).contains(&i) { 3 } else { 0 },
            })
            .collect();
        assert_eq!(free_replacements(&groups, 0, 0), Some(0));
        assert_eq!(free_replacements(&groups, 0, 1), Some(1));
        for g in &mut groups {
            g.witnesses = 0;
        }
        groups[0].witnesses = 2;
        assert_eq!(free_replacements(&groups, 0, 0), Some(0));
        groups[0].witnesses = 3;
        assert_eq!(free_replacements(&groups, 0, 0), Some(1));
    }

    #[test]
    fn dp_lower_bound_matches_or_underestimates_exhaustive_blockers() {
        for seed in 0..128_usize {
            let groups: Vec<_> = (0..8)
                .map(|i| Group {
                    character: i as u32,
                    breaks: if (seed + i) % 3 == 0 {
                        vec![0, 2]
                    } else {
                        vec![((seed / 7 + i) % 4) as u8]
                    },
                    witnesses: (seed * 17 + i * 11) % 8,
                })
                .collect();
            for target in [0, 3] {
                for required in 0..4 {
                    let actual = free_replacements(&groups, target, required);
                    let exact = reference(&groups, target, required);
                    assert_eq!(actual.is_some(), exact.is_some());
                    if let (Some(a), Some(b)) = (actual, exact) {
                        assert!(a <= b);
                    }
                }
            }
        }
    }
}
