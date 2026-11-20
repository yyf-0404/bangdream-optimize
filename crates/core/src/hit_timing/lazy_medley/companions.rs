//! Reuse the existing conservative companion/pair certificates on timing uppers.
use super::WideMedleySolverInput;

/// Original candidate indices that may participate in a triple at `floor`.
/// Scores must be conservative, nonnegative i32 uppers. Non-five-member masks
/// fall back to retaining everything; they remain valid generic solver inputs.
pub fn compatible_candidates(input: &WideMedleySolverInput, floor: i64) -> Vec<usize> {
    let all: Vec<_> = (0..input.scores.len()).collect();
    if input.team_masks.len() != input.scores.len() {
        return all;
    }
    let mut members = Vec::with_capacity(all.len());
    for mask in &input.team_masks {
        let mut cards = [0; 5];
        let mut count = 0;
        for (word, &bits) in mask.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                if count == 5 {
                    return all;
                }
                cards[count] = word * 64 + bits.trailing_zeros() as usize;
                count += 1;
                bits &= bits - 1;
            }
        }
        if count != 5 {
            return all;
        }
        members.push(cards);
    }
    // The timing entry point selects and applies its key projection separately;
    // do not silently add automatic keys or invalidate its ablation switch.
    crate::medley::disjoint_bound::candidate_indices_without_key_projection(
        all,
        |i| members[i],
        |i| input.scores[i].map(i64::from),
        floor,
        "timing-upper",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatible_filter_preserves_every_above_floor_triple() {
        let mut seed = 67494_u32;
        for sample in 0..60 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            };
            let mut masks = Vec::new();
            for i in 0..30 {
                let mut mask = vec![0_u64; 6];
                if i < 3 {
                    for bit in i * 5..i * 5 + 5 {
                        mask[5] |= 1 << bit;
                    }
                } else {
                    for word in 0..5 {
                        mask[word] |= 1 << (next() % 4);
                    }
                }
                masks.push(mask);
            }
            let scores: Vec<_> = (0..30)
                .map(|_| std::array::from_fn(|_| (next() % 1000) as i32))
                .collect();
            let floor = if sample == 0 {
                0
            } else {
                (next() % 3000) as i64
            };
            let input = WideMedleySolverInput {
                current_best: -1,
                team_masks: masks,
                scores,
            };
            let kept = compatible_candidates(&input, floor);
            let separate = |a: usize, b: usize| {
                input.team_masks[a]
                    .iter()
                    .zip(&input.team_masks[b])
                    .all(|(a, b)| a & b == 0)
            };
            for a in 0..30 {
                for b in 0..30 {
                    for c in 0..30 {
                        let total = i64::from(input.scores[a][0])
                            + i64::from(input.scores[b][1])
                            + i64::from(input.scores[c][2]);
                        if total >= floor && separate(a, b) && separate(a, c) && separate(b, c) {
                            assert!(kept.contains(&a) && kept.contains(&b) && kept.contains(&c));
                        }
                    }
                }
            }
        }
    }
}
