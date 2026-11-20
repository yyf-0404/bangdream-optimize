//! Adapted from core/team_prune/hard/cover.rs. Exact only for this relaxed
//! blocker model: other-song scores, full teams and non-witness cards are not
//! enumerated. Their omitted constraints can only enlarge blocked counts.
use super::Group;

#[derive(Clone)]
struct Row {
    blocked: i32,
    breaks: u8,
    occupancy: u8,
}

/// `eligibility[g]` contains one global three-song mask per DISTINCT witness
/// in groups[g]. It must include every allowed band/attribute condition, not
/// just the target's current condition. Groups have distinct characters.
/// A proof is needed for every song in target_songs. Missing feasible teammate
/// groups returns None, never a deletion certificate.
pub fn precise_free_replacements(
    groups: &[Group],
    eligibility: &[Vec<u8>],
    target: u32,
    required: u8,
    target_songs: u8,
) -> Option<usize> {
    assert!(required < 4 && target_songs < 8);
    assert_eq!(groups.len(), eligibility.len());
    for (g, masks) in groups.iter().zip(eligibility) {
        assert_eq!(g.witnesses, masks.len());
        assert!(masks.iter().all(|&m| m < 8));
    }
    if target_songs == 0 {
        return None;
    }
    let total: usize = groups.iter().map(|g| g.witnesses).sum();
    if total > i32::MAX as usize {
        return Some(0);
    }
    let mut minimum = usize::MAX;
    for song in 0..3 {
        if target_songs & (1 << song) == 0 {
            continue;
        }
        let first = 1 << ((song + 1) % 3);
        let second = 1 << ((song + 2) % 3);
        let rows: Vec<_> = groups
            .iter()
            .zip(eligibility)
            .map(|(g, masks)| {
                let a = masks.iter().any(|m| m & first != 0);
                let b = masks.iter().any(|m| m & second != 0);
                let two = masks.iter().filter(|&&m| m & (first | second) != 0).count() >= 2;
                let mut breaks = 0;
                if g.character != target {
                    for &flags in &g.breaks {
                        let projected = if required == 2 {
                            (flags & required) >> 1
                        } else {
                            flags & required
                        };
                        breaks |= 1 << projected;
                    }
                }
                Row {
                    blocked: g.witnesses as i32,
                    breaks,
                    occupancy: u8::from(a) | (u8::from(b) << 1) | (u8::from(a && b && two) << 2),
                }
            })
            .collect();
        let blocked = match required {
            0 => run::<1, 180>(&rows),
            1 | 2 => run::<2, 360>(&rows),
            _ => run::<4, 720>(&rows),
        }?;
        minimum = minimum.min(total.saturating_sub(blocked));
        if minimum == 0 {
            break;
        }
    }
    Some(minimum)
}

fn run<const BREAKS: usize, const SIZE: usize>(rows: &[Row]) -> Option<usize> {
    const NONE: i32 = -1;
    let layer = 36 * BREAKS;
    let mut dp = [NONE; SIZE];
    dp[0] = 0;
    let (mut mt, mut ma, mut mb) = (0, 0, 0);
    for row in rows {
        // A character blocked by a target teammate already loses every
        // witness. Also charging other-song slots for it cannot improve the
        // blocking maximum; omitting those wasted slots is a safe relaxation.
        let mut next = dp;
        for t in 0..=mt {
            for a in 0..=ma {
                for b in 0..=mb {
                    for flags in 0..BREAKS {
                        let index = t * layer + (a * 6 + b) * BREAKS + flags;
                        let value = dp[index];
                        if value == NONE {
                            continue;
                        }
                        if a < 5 && row.occupancy & 1 != 0 {
                            let dest = index + 6 * BREAKS;
                            next[dest] = next[dest].max(value + 1);
                        }
                        if b < 5 && row.occupancy & 2 != 0 {
                            let dest = index + BREAKS;
                            next[dest] = next[dest].max(value + 1);
                        }
                        if a < 5 && b < 5 && row.occupancy & 4 != 0 {
                            let dest = index + 7 * BREAKS;
                            next[dest] = next[dest].max(value + 2);
                        }
                        if t < 4 {
                            for mask in 0..BREAKS {
                                if row.breaks & (1 << mask) == 0 {
                                    continue;
                                }
                                let dest = index - flags + layer + (flags | mask);
                                next[dest] = next[dest].max(value + row.blocked);
                            }
                        }
                    }
                }
            }
        }
        dp = next;
        mt = (mt + usize::from(row.breaks != 0)).min(4);
        ma = (ma + usize::from(row.occupancy & 1 != 0)).min(5);
        mb = (mb + usize::from(row.occupancy & 2 != 0)).min(5);
    }
    let maximum = (0..6)
        .flat_map(|a| (0..6).map(move |b| (a, b)))
        .map(|(a, b)| dp[4 * layer + (a * 6 + b) * BREAKS + BREAKS - 1])
        .max()
        .unwrap();
    (maximum >= 0).then_some(maximum as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(character: u32, breaks: Vec<u8>, witnesses: usize) -> Group {
        Group {
            character,
            breaks,
            witnesses,
        }
    }

    #[test]
    fn a_single_witness_cannot_be_consumed_by_both_other_songs() {
        let groups: Vec<_> = (0..5)
            .map(|c| group(c, vec![0], usize::from(c == 0)))
            .collect();
        let masks = vec![vec![7], vec![], vec![], vec![], vec![]];
        assert_eq!(precise_free_replacements(&groups, &masks, 0, 0, 1), Some(0));
        let mut groups = groups;
        groups[0].witnesses = 2;
        let masks = vec![vec![6, 1], vec![], vec![], vec![], vec![]];
        assert_eq!(precise_free_replacements(&groups, &masks, 0, 0, 1), Some(1));
        let masks = vec![vec![2, 2], vec![], vec![], vec![], vec![]];
        assert_eq!(super::super::free_replacements(&groups, 0, 0), Some(0));
        assert_eq!(precise_free_replacements(&groups, &masks, 0, 0, 1), Some(1));
        let masks = vec![vec![2, 4], vec![], vec![], vec![], vec![]];
        assert_eq!(precise_free_replacements(&groups, &masks, 0, 0, 1), Some(0));
        assert_eq!(precise_free_replacements(&groups, &masks, 0, 0, 2), Some(1));
        assert_eq!(precise_free_replacements(&groups, &masks, 0, 0, 3), Some(0));
        assert_eq!(precise_free_replacements(&groups, &masks, 0, 2, 1), None);
    }

    // Exhaustively choose the actual four blocker characters and their break
    // options. For the rest, enumerate distinct witness IDs used by each other
    // song, so this reference does not share the DP's occupancy compression.
    fn reference(
        groups: &[Group],
        masks: &[Vec<u8>],
        target: u32,
        required: u8,
        songs: u8,
    ) -> Option<usize> {
        fn occupancy(
            groups: &[Group],
            masks: &[Vec<u8>],
            blocked: usize,
            at: usize,
            a: usize,
            b: usize,
            song: usize,
        ) -> usize {
            if at == groups.len() {
                return 0;
            }
            let onward = |na, nb| occupancy(groups, masks, blocked, at + 1, na, nb, song);
            if blocked & (1 << at) != 0 {
                return groups[at].witnesses + onward(a, b);
            }
            let mut best = onward(a, b);
            for x in 0..masks[at].len() {
                if a < 5 && masks[at][x] & (1 << ((song + 1) % 3)) != 0 {
                    best = best.max(1 + onward(a + 1, b));
                    if b < 5 {
                        for y in 0..masks[at].len() {
                            if y != x && masks[at][y] & (1 << ((song + 2) % 3)) != 0 {
                                best = best.max(2 + onward(a + 1, b + 1));
                            }
                        }
                    }
                }
                if b < 5 && masks[at][x] & (1 << ((song + 2) % 3)) != 0 {
                    best = best.max(1 + onward(a, b + 1));
                }
            }
            best
        }
        let total: usize = groups.iter().map(|g| g.witnesses).sum();
        let mut best = None;
        for blocked in 0usize..1 << groups.len() {
            if blocked.count_ones() != 4 {
                continue;
            }
            let mut flags = vec![0];
            for (g, group) in groups.iter().enumerate() {
                if blocked & (1 << g) == 0 {
                    continue;
                }
                if group.character == target {
                    flags.clear();
                    break;
                }
                flags = flags
                    .iter()
                    .flat_map(|&f| group.breaks.iter().map(move |b| f | (b & required)))
                    .collect();
            }
            if !flags.contains(&required) {
                continue;
            }
            for song in 0..3 {
                if songs & (1 << song) == 0 {
                    continue;
                }
                let free = total - occupancy(groups, masks, blocked, 0, 0, 0, song);
                best = Some(best.map_or(free, |v: usize| v.min(free)));
            }
        }
        best
    }

    #[test]
    fn precise_cover_matches_exhaustive_role_card_and_song_assignments() {
        for seed in 0..64 {
            let groups: Vec<_> = (0..8)
                .map(|g| {
                    group(
                        g,
                        match (g as usize + seed) % 4 {
                            0 => vec![],
                            1 => vec![0, 2],
                            2 => vec![1, 3],
                            _ => vec![0],
                        },
                        (g as usize * 13 + seed * 7) % 3,
                    )
                })
                .collect();
            let masks: Vec<Vec<_>> = groups
                .iter()
                .enumerate()
                .map(|(g, row)| {
                    (0..row.witnesses)
                        .map(|i| ((g * 5 + i * 3 + seed) % 8) as u8)
                        .collect()
                })
                .collect();
            for required in 0..4 {
                for songs in [1, 2, 4, 7] {
                    let actual = precise_free_replacements(&groups, &masks, 0, required, songs);
                    assert_eq!(
                        actual,
                        reference(&groups, &masks, 0, required, songs),
                        "seed={seed} breaks={required} songs={songs}"
                    );
                    if let (Some(coarse), Some(fine)) = (
                        super::super::free_replacements(&groups, 0, required),
                        actual,
                    ) {
                        assert!(fine >= coarse);
                    }
                }
            }
        }
    }

    #[test]
    fn each_other_song_has_only_five_places() {
        let groups: Vec<_> = (0..11)
            .map(|g| {
                group(
                    g,
                    if (1..5).contains(&g) { vec![0] } else { vec![] },
                    usize::from(g == 0 || g >= 5),
                )
            })
            .collect();
        let masks: Vec<_> = groups.iter().map(|g| vec![2; g.witnesses]).collect();
        assert_eq!(precise_free_replacements(&groups, &masks, 0, 0, 1), Some(2));
    }
}
