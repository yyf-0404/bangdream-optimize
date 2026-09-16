//! Fixed five-card assignment with exactly the predecessor order of the legacy DP.
//! For each target subset, earlier source subsets remove the highest card bit first.

pub(crate) fn enabled() -> bool {
    #[cfg(feature = "experimental-fixed-assignment")]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var("BANGDREAM_OPTIMIZE_FIXED_ASSIGNMENT").as_deref() != Ok("off")
        })
    }
    #[cfg(not(feature = "experimental-fixed-assignment"))]
    true
}

macro_rules! define_assignment {
    ($name:ident, $score:ty, $minimum:expr, $empty_choice:expr) => {
        pub(crate) fn $name(deltas: &[[$score; 6]; 5]) -> ($score, [usize; 5], usize) {
            if deltas[1..].iter().all(|row| row == &deltas[0]) {
                return (
                    deltas[0][..5].iter().sum::<$score>() + deltas[0][5],
                    [0, 1, 2, 3, 4],
                    0,
                );
            }
            let mut dp = [$minimum; 32];
            let mut chosen = [$empty_choice; 32];
            dp[0] = 0 as $score;
            // A fixed target mask receives legacy forward-DP updates in ascending
            // predecessor order, i.e. removing card bits from highest to lowest.
            // Keep strict `>` so equal values choose the same predecessor.
            macro_rules! state {
                ($mask:expr) => {{
                    const SLOT: usize = ($mask as u32).count_ones() as usize - 1;
                    let mut best = $minimum;
                    let mut choice = $empty_choice;
                    macro_rules! consider {
                        ($card:literal) => {
                            if $mask & (1 << $card) != 0 {
                                let value = dp[$mask ^ (1 << $card)] + deltas[$card][SLOT];
                                if value > best {
                                    best = value;
                                    choice = $card;
                                }
                            }
                        };
                    }
                    consider!(4);
                    consider!(3);
                    consider!(2);
                    consider!(1);
                    consider!(0);
                    dp[$mask] = best;
                    chosen[$mask] = choice;
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

            let mut captain = 0;
            let mut captain_delta = $minimum;
            for card in 0..5 {
                if deltas[card][5] > captain_delta {
                    captain_delta = deltas[card][5];
                    captain = card;
                }
            }
            let mut order = [0; 5];
            let mut mask = 31;
            for slot in (0..5).rev() {
                let card = chosen[mask];
                order[slot] = card;
                mask ^= 1 << card;
            }
            (dp[31] + deltas[captain][5], order, captain)
        }
    };
}

define_assignment!(maximize_i32, i32, i32::MIN, usize::MAX);
define_assignment!(maximize_f64, f64, f64::NEG_INFINITY, 0);

#[cfg(test)]
mod tests {
    use super::*;
    const TEAM_SIZE: usize = 5;
    fn reference_i32(deltas: &[[i32; 6]; 5]) -> (i32, [usize; 5], usize) {
        if deltas[1..].iter().all(|row| row == &deltas[0]) {
            return (
                deltas[0][..5].iter().sum::<i32>() + deltas[0][5],
                [0, 1, 2, 3, 4],
                0,
            );
        }

        let mut dp = [i32::MIN; 1 << 5];
        let mut chosen = [usize::MAX; 1 << 5];
        dp[0] = 0;
        for mask in 0usize..(1 << 5) - 1 {
            let activation = FIVE_CARD_MASK_POPCOUNT[mask] as usize;
            let mut available = (!mask) & ((1 << 5) - 1);
            while available != 0 {
                let card_idx = available.trailing_zeros() as usize;
                available &= available - 1;
                let next = mask | (1 << card_idx);
                let value = dp[mask] + deltas[card_idx][activation];
                if value > dp[next] {
                    dp[next] = value;
                    chosen[next] = card_idx;
                }
            }
        }

        let mut captain_index = 0usize;
        for card_idx in 1..5 {
            if deltas[card_idx][5] > deltas[captain_index][5] {
                captain_index = card_idx;
            }
        }

        let mut order_indices = [0usize; 5];
        let mut mask = (1 << 5) - 1;
        for activation in (0..5).rev() {
            let card_idx = chosen[mask];
            order_indices[activation] = card_idx;
            mask ^= 1 << card_idx;
        }
        (
            dp[(1 << 5) - 1] + deltas[captain_index][5],
            order_indices,
            captain_index,
        )
    }

    const FIVE_CARD_MASK_POPCOUNT: [u8; 32] = [
        0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4, 1, 2, 2, 3, 2, 3, 3, 4, 2, 3, 3, 4, 3, 4,
        4, 5,
    ];
    fn reference_meta(skill_meta: &[[f64; TEAM_SIZE + 1]; TEAM_SIZE]) -> ([usize; 5], usize) {
        if skill_meta[1..].iter().all(|row| row == &skill_meta[0]) {
            return ([0, 1, 2, 3, 4], 0);
        }

        let mut dp = [f64::NEG_INFINITY; 1 << TEAM_SIZE];
        let mut choose = [0usize; 1 << TEAM_SIZE];
        dp[0] = 0.0;

        for mask in 0..(1usize << TEAM_SIZE) - 1 {
            let activation = FIVE_CARD_MASK_POPCOUNT[mask] as usize;
            let mut available = (!mask) & ((1 << TEAM_SIZE) - 1);
            while available != 0 {
                let card_idx = available.trailing_zeros() as usize;
                available &= available - 1;
                let card_meta = &skill_meta[card_idx];
                let value = dp[mask] + card_meta[activation];
                let next_mask = mask | (1 << card_idx);
                if value > dp[next_mask] {
                    dp[next_mask] = value;
                    choose[next_mask] = card_idx;
                }
            }
        }

        let mut captain_index = 0;
        let mut captain_meta = f64::NEG_INFINITY;
        for (card_idx, card_meta) in skill_meta.iter().enumerate() {
            let value = card_meta[TEAM_SIZE];
            if value > captain_meta {
                captain_meta = value;
                captain_index = card_idx;
            }
        }

        let mut order_indices = [0usize; TEAM_SIZE];
        let mut mask = (1usize << TEAM_SIZE) - 1;
        for slot in (0..TEAM_SIZE).rev() {
            let card_idx = choose[mask];
            order_indices[slot] = card_idx;
            mask ^= 1 << card_idx;
        }

        (order_indices, captain_index)
    }

    fn next(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    #[test]
    fn exact_integer_scores_and_ties_match_legacy_forward_dp() {
        let mut seed = 0x42881u64;
        for index in 0..12000 {
            let mut values = [[0; 6]; 5];
            for row in &mut values {
                for v in row {
                    *v = (next(&mut seed) % if index % 3 == 0 { 3 } else { 1_000_000 }) as i32 - 5;
                }
            }
            if index % 4 == 0 {
                values[1] = values[0];
                values[3] = values[2];
            }
            if index % 9 == 0 {
                values = [values[0]; 5];
            }
            assert_eq!(
                maximize_i32(&values),
                reference_i32(&values),
                "matrix {index}"
            );
        }
    }

    #[test]
    fn floating_point_seed_order_matches_legacy_without_changing_sum_order() {
        let mut seed = 0x431171u64;
        for index in 0..12000 {
            let mut values = [[0.0; 6]; 5];
            for row in &mut values {
                for v in row {
                    let bits = next(&mut seed);
                    *v = match index % 4 {
                        0 => (bits % 3) as f64,
                        1 => f64::from_bits(1.0f64.to_bits() + bits % 10),
                        2 => (bits % 1_000_000) as f64 * 0.000003 - 1.0,
                        _ => f64::from_bits(0x3e00_0000_0000_0000 + bits % 0x0500_0000_0000_0000),
                    };
                }
            }
            if index % 4 == 0 {
                values[1] = values[0];
                values[3] = values[2];
            }
            if index % 9 == 0 {
                values = [values[0]; 5];
            }
            let result = maximize_f64(&values);
            assert_eq!(
                (result.1, result.2),
                reference_meta(&values),
                "matrix {index}"
            );
        }
    }

    #[test]
    fn fixed_assignment_reaches_exhaustive_maximum() {
        fn search(matrix: &[[i32; 6]; 5], slot: usize, used: u8, total: i32) -> i32 {
            if slot == 5 {
                return total;
            }
            (0..5)
                .filter(|&card| used & (1 << card) == 0)
                .map(|card| {
                    search(
                        matrix,
                        slot + 1,
                        used | (1 << card),
                        total + matrix[card][slot],
                    )
                })
                .max()
                .unwrap()
        }
        let mut seed = 0x530121u64;
        for _ in 0..1000 {
            let values =
                std::array::from_fn(|_| std::array::from_fn(|_| (next(&mut seed) % 1000) as i32));
            let expected =
                search(&values, 0, 0, 0) + values.iter().map(|row| row[5]).max().unwrap();
            assert_eq!(maximize_i32(&values).0, expected);
        }
    }
}
