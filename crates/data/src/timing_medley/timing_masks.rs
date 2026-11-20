//! Compact only the bit indices used by retained rosters, preserving conflicts.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskStats {
    pub pool_cards: usize,
    pub used_cards: usize,
    pub words: usize,
    pub compact: bool,
}

pub fn build(
    rosters: &[[usize; 5]],
    pool_cards: usize,
    compact: bool,
) -> (Vec<Vec<u64>>, MaskStats) {
    let mut active = vec![false; pool_cards];
    for roster in rosters {
        for &i in roster {
            active[i] = true;
        }
    }
    let used_cards = active.iter().filter(|&&v| v).count();
    let words = if compact { used_cards } else { pool_cards }.div_ceil(64);
    let mut mapping = vec![0; pool_cards];
    let mut next = 0;
    for (i, &used) in active.iter().enumerate() {
        if used {
            mapping[i] = if compact { next } else { i };
            next += 1;
        }
    }
    let masks = rosters
        .iter()
        .map(|r| {
            let mut mask = vec![0; words];
            for &card in r {
                let i = mapping[card];
                mask[i / 64] |= 1 << (i % 64);
            }
            mask
        })
        .collect();
    (
        masks,
        MaskStats {
            pool_cards,
            used_cards,
            words,
            compact,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_preserves_every_pair_and_crosses_word_boundaries() {
        let rosters: Vec<_> = (0..140)
            .map(|i| {
                [
                    i * 7,
                    ((i + 1) % 140) * 7,
                    1100 + i,
                    1300 + i % 9,
                    1401 + i % 7,
                ]
            })
            .collect();
        let (original, a) = build(&rosters, 1456, false);
        let (compact, b) = build(&rosters, 1456, true);
        assert_eq!(a.words, 23);
        assert_eq!(b.used_cards, 296);
        assert_eq!(b.words, 5);
        for (i, r) in rosters.iter().enumerate() {
            assert_eq!(compact[i].iter().map(|v| v.count_ones()).sum::<u32>(), 5);
            for (j, s) in rosters.iter().enumerate() {
                let shared = r.iter().any(|v| s.contains(v));
                assert_eq!(
                    original[i]
                        .iter()
                        .zip(&original[j])
                        .any(|(a, b)| a & b != 0),
                    shared
                );
                assert_eq!(
                    compact[i].iter().zip(&compact[j]).any(|(a, b)| a & b != 0),
                    shared
                );
            }
        }
        let disjoint = [[1, 5, 70, 990, 1400], [2, 6, 71, 991, 1401]];
        let (masks, stats) = build(&disjoint, 1456, true);
        assert_eq!(stats.words, 1);
        assert_eq!(masks[0][0] & masks[1][0], 0);
    }
}
