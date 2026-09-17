//! Exact fixed-size subset/last-card DP for predecessor-dependent skill windows.
use super::ExactSkillOrder;

pub(super) fn maximum(deltas: &[[[i32; 5]; 5]; 6], base: i32) -> ExactSkillOrder {
    // Reserve six 3-bit digits for the five card indices and captain. Maximizing
    // score * SCALE - path preserves both integer score and lexicographic ties,
    // even for negative deltas. i64 leaves ample space for every i32 score.
    const SCALE: i64 = 1 << 18;
    let mut dp = [[0i64; 5]; 32];
    // Literal masks make all membership tests and predecessor offsets constant.
    // Each reachable state is written once, after all its predecessors.
    macro_rules! state {
        ($mask:literal) => {{
            const POSITION: usize = ($mask as u32).count_ones() as usize - 1;
            macro_rules! last {
                ($last:literal) => {
                    if $mask & (1 << $last) != 0 {
                        const PREVIOUS: usize = $mask ^ (1 << $last);
                        let value = if PREVIOUS == 0 {
                            i64::from(deltas[0][0][$last]) * SCALE
                        } else {
                            let mut best = i64::MIN;
                            macro_rules! consider {
                                ($previous:literal) => {
                                    if PREVIOUS & (1 << $previous) != 0 {
                                        best = best.max(
                                            dp[PREVIOUS][$previous]
                                                + i64::from(deltas[POSITION][$previous][$last])
                                                    * SCALE,
                                        );
                                    }
                                };
                            }
                            consider!(0);
                            consider!(1);
                            consider!(2);
                            consider!(3);
                            consider!(4);
                            best
                        };
                        dp[$mask][$last] = value - (($last as i64) << (3 * (5 - POSITION)));
                    }
                };
            }
            last!(0);
            last!(1);
            last!(2);
            last!(3);
            last!(4);
        }};
    }
    state!(1);
    state!(2);
    state!(3);
    state!(4);
    state!(5);
    state!(6);
    state!(7);
    state!(8);
    state!(9);
    state!(10);
    state!(11);
    state!(12);
    state!(13);
    state!(14);
    state!(15);
    state!(16);
    state!(17);
    state!(18);
    state!(19);
    state!(20);
    state!(21);
    state!(22);
    state!(23);
    state!(24);
    state!(25);
    state!(26);
    state!(27);
    state!(28);
    state!(29);
    state!(30);
    state!(31);
    let mut best = i64::MIN;
    for last in 0..5 {
        for captain in 0..5 {
            best = best
                .max(dp[31][last] + i64::from(deltas[5][last][captain]) * SCALE - captain as i64);
        }
    }
    let score = (best + SCALE - 1) >> 18;
    let path = (score * SCALE - best) as usize;
    ExactSkillOrder {
        score: base + score as i32,
        order_indices: std::array::from_fn(|position| (path >> (3 * (5 - position))) & 7),
        captain_index: path & 7,
    }
}
