use super::*;

const FIRST: u8 = 1;
const SECOND: u8 = 2;
const BOTH: u8 = 4;

#[derive(Clone, Copy)]
struct Group {
    blocked: i16,
    teammate_options: u8,
    // One set of occupancy choices for each possible target song.
    occupancy: [u8; 3],
}

pub(super) fn minimum_cover(
    groups: &[PreciseCoverCharacterGroup],
    target: &PreparedCard,
    signature: MedleyPruneSignature,
    dominator_count: usize,
    team_count: usize,
    eligibility: &[u8],
    chart_count: usize,
) -> usize {
    let required = signature_required_break_mask(signature);
    let groups = groups
        .iter()
        .map(|group| {
            let mut teammate_options = 0;
            if group.character_id != target.character_id {
                for mask in 0..4 {
                    if group.teammate_break_options & (1 << mask) != 0 {
                        // Only the distinctions required by this signature matter.
                        // A one-bit requirement always uses compact bit zero.
                        let projected = if required == 2 {
                            mask >> 1
                        } else {
                            mask & required
                        };
                        teammate_options |= 1 << projected;
                    }
                }
            }
            let mut occupancy = [0; 3];
            if team_count != 1 {
                for (target_chart, options) in occupancy.iter_mut().enumerate().take(chart_count) {
                    let (first, second) = match target_chart {
                        0 => (0b010, 0b100),
                        1 => (0b001, 0b100),
                        _ => (0b001, 0b010),
                    };
                    let mut first_count = 0;
                    let mut second_count = 0;
                    let mut union_count = 0;
                    for &index in &group.dominator_indices {
                        let mask = eligibility[index];
                        first_count += usize::from(mask & first != 0);
                        second_count += usize::from(mask & second != 0);
                        union_count += usize::from(mask & (first | second) != 0);
                    }
                    if first_count > 0 {
                        *options |= FIRST;
                    }
                    if second_count > 0 {
                        *options |= SECOND;
                    }
                    if first_count > 0 && second_count > 0 && union_count >= 2 {
                        *options |= BOTH;
                    }
                }
            }
            Group {
                blocked: group.dominator_indices.len() as i16,
                teammate_options,
                occupancy,
            }
        })
        .collect::<Vec<_>>();

    let mut minimum = usize::MAX;
    let mut results = [None; 3];
    for chart in 0..if team_count == 1 { 1 } else { chart_count } {
        // Equal occupancy choices give exactly the same DP. Interchanging the
        // two other songs is also equivalent: both have five available slots.
        let previous = (0..chart).find(|&previous| {
            groups
                .iter()
                .all(|group| group.occupancy[chart] == group.occupancy[previous])
                || groups
                    .iter()
                    .all(|group| group.occupancy[chart] == swap(group.occupancy[previous]))
        });
        let result = if let Some(previous) = previous {
            results[previous].unwrap()
        } else {
            match (team_count == 1, required) {
                (true, 0) => run::<1, 1, 5>(&groups, chart, dominator_count),
                (true, 1 | 2) => run::<2, 1, 10>(&groups, chart, dominator_count),
                (true, _) => run::<4, 1, 20>(&groups, chart, dominator_count),
                (false, 0) => run::<1, 6, 180>(&groups, chart, dominator_count),
                (false, 1 | 2) => run::<2, 6, 360>(&groups, chart, dominator_count),
                (false, _) => run::<4, 6, 720>(&groups, chart, dominator_count),
            }
        };
        results[chart] = Some(result);
        minimum = minimum.min(result);
        // Covers are nonnegative; another target song cannot lower zero.
        if minimum == 0 {
            break;
        }
    }
    minimum
}

fn swap(options: u8) -> u8 {
    (options & BOTH) | ((options & FIRST) << 1) | ((options & SECOND) >> 1)
}

fn run<const BREAKS: usize, const SLOTS: usize, const SIZE: usize>(
    groups: &[Group],
    chart: usize,
    dominator_count: usize,
) -> usize {
    const UNREACHABLE: i16 = -1;
    let layer = SLOTS * SLOTS * BREAKS;
    let mut current = [UNREACHABLE; SIZE];
    current[0] = 0;
    let mut max_teammates = 0;
    let mut max_first = 0;
    let mut max_second = 0;

    for group in groups {
        let options = group.occupancy[chart];
        if group.teammate_options == 0 && options == 0 {
            continue;
        }
        // Leaving the character unused is already represented by this copy.
        let mut next = current;
        for teammates in 0..=max_teammates {
            for first in 0..=max_first {
                for second in 0..=max_second {
                    let index = teammates * layer + (first * SLOTS + second) * BREAKS;
                    for breaks in 0..BREAKS {
                        let index = index + breaks;
                        let value = current[index];
                        if value == UNREACHABLE {
                            continue;
                        }
                        if options & FIRST != 0 && first + 1 < SLOTS {
                            let dest = index + SLOTS * BREAKS;
                            next[dest] = next[dest].max(value + 1);
                        }
                        if options & SECOND != 0 && second + 1 < SLOTS {
                            let dest = index + BREAKS;
                            next[dest] = next[dest].max(value + 1);
                        }
                        if options & BOTH != 0 && first + 1 < SLOTS && second + 1 < SLOTS {
                            let dest = index + (SLOTS + 1) * BREAKS;
                            next[dest] = next[dest].max(value + 2);
                        }
                        if teammates + 1 < TEAM_SIZE {
                            let mut choices = group.teammate_options;
                            while choices != 0 {
                                let mask = choices.trailing_zeros() as usize;
                                choices &= choices - 1;
                                let dest = index - breaks + layer + (breaks | mask);
                                next[dest] = next[dest].max(value + group.blocked);
                            }
                        }
                    }
                }
            }
        }
        current = next;
        max_teammates =
            (max_teammates + usize::from(group.teammate_options != 0)).min(TEAM_SIZE - 1);
        max_first = (max_first + usize::from(options & FIRST != 0)).min(SLOTS - 1);
        max_second = (max_second + usize::from(options & SECOND != 0)).min(SLOTS - 1);
    }

    let mut unavailable = UNREACHABLE;
    for first in 0..SLOTS {
        for second in 0..SLOTS {
            let index = (TEAM_SIZE - 1) * layer + (first * SLOTS + second) * BREAKS + BREAKS - 1;
            unavailable = unavailable.max(current[index]);
        }
    }
    if unavailable == UNREACHABLE {
        0
    } else {
        dominator_count.saturating_sub(unavailable as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medley::test_support::prepared_card;

    fn next(seed: &mut u32) -> usize {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        *seed as usize
    }

    #[test]
    fn compact_cover_matches_reference_for_all_signatures_and_eligibility_masks() {
        let target = prepared_card(1, 0, 1, Attribute::Cool);
        let signatures = [
            MedleyPruneSignature::Mixed,
            MedleyPruneSignature::UnifiedBand(1),
            MedleyPruneSignature::UnifiedAttribute(Attribute::Cool),
            MedleyPruneSignature::UnifiedBandAttribute(1, Attribute::Cool),
        ];
        let mut seed = 0x6217_ea49;
        for case in 0..256 {
            let mut eligibility = Vec::new();
            let groups = (0..next(&mut seed) % 41)
                .map(|character| {
                    let count = next(&mut seed) % 9;
                    let dominators = (0..count)
                        .map(|_| {
                            let index = eligibility.len();
                            eligibility.push(if case % 4 == 0 {
                                0b111
                            } else {
                                (next(&mut seed) % 8) as u8
                            });
                            index
                        })
                        .collect();
                    PreciseCoverCharacterGroup {
                        character_id: character as u32,
                        teammate_break_options: (next(&mut seed) % 16) as u8,
                        dominator_indices: dominators,
                    }
                })
                .collect::<Vec<_>>();
            for signature in signatures {
                for team_count in [1, 3] {
                    let expected = reference_minimum_cover(
                        &groups,
                        &target,
                        signature,
                        eligibility.len(),
                        team_count,
                        &eligibility,
                        team_count,
                    );
                    let actual = minimum_cover(
                        &groups,
                        &target,
                        signature,
                        eligibility.len(),
                        team_count,
                        &eligibility,
                        team_count,
                    );
                    assert_eq!(
                        actual, expected,
                        "case={case}, signature={signature:?}, teams={team_count}"
                    );
                }
            }
        }
    }

    #[test]
    fn occupancy_requires_distinct_cards_and_keeps_infeasible_breaks() {
        let target = prepared_card(1, 0, 1, Attribute::Cool);
        for target_count in 0..4 {
            for masks in 0..512usize {
                let eligibility = (0..target_count)
                    .map(|index| ((masks >> (3 * index)) & 7) as u8)
                    .collect::<Vec<_>>();
                let mut groups = vec![PreciseCoverCharacterGroup {
                    character_id: 0,
                    teammate_break_options: 1,
                    dominator_indices: (0..target_count).collect(),
                }];
                groups.extend((1..5).map(|character_id| PreciseCoverCharacterGroup {
                    character_id,
                    teammate_break_options: 1,
                    dominator_indices: Vec::new(),
                }));
                for signature in [
                    MedleyPruneSignature::Mixed,
                    MedleyPruneSignature::UnifiedBandAttribute(1, Attribute::Cool),
                ] {
                    let expected = reference_minimum_cover(
                        &groups,
                        &target,
                        signature,
                        target_count,
                        3,
                        &eligibility,
                        3,
                    );
                    let actual = minimum_cover(
                        &groups,
                        &target,
                        signature,
                        target_count,
                        3,
                        &eligibility,
                        3,
                    );
                    assert_eq!(
                        actual, expected,
                        "count={target_count} masks={masks} signature={signature:?}"
                    );
                }
            }
        }
    }
}
