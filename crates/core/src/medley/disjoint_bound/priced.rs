//! Integer Lagrangian bound for the no-reused-card constraint across three teams.
//! For nonnegative card prices p, let b_s=max_T(score(T,s)-sum_{c in T}p_c).
//! Every disjoint plan is <= sum_c p_c + sum_s b_s. Fixing a team subtracts its
//! reduced-cost gap. Prices need not converge: every iteration is a valid bound.
use super::{Candidate, Timer};

const PRICE_SCALE: i128 = 1024;

pub(super) fn retained_roles(candidates: &[Candidate], floor: i64) -> Vec<u8> {
    let mut roles = vec![7; candidates.len()];
    if candidates.len() < 3 || floor <= 0 {
        return roles;
    }
    let start = Timer::start();
    let card_count = candidates
        .iter()
        .flat_map(|c| c.cards)
        .max()
        .map_or(0, |c| c + 1);
    let limit = i128::from(
        candidates
            .iter()
            .flat_map(|c| c.scores)
            .max()
            .unwrap_or(0)
            .max(1),
    ) * PRICE_SCALE;
    let threshold = i128::from(floor) * PRICE_SCALE;
    // Explicit-candidate masks may have fewer than five bits. Their arrays are
    // padded with repeated card IDs: each resource must be charged only once.
    let resources: Vec<_> = candidates
        .iter()
        .map(|candidate| {
            let mut cards = candidate.cards;
            cards.sort_unstable();
            let mut len = 1;
            for i in 1..cards.len() {
                if cards[i] != cards[len - 1] {
                    cards[len] = cards[i];
                    len += 1;
                }
            }
            (cards, len)
        })
        .collect();
    let mut prices = vec![0i128; card_count];
    let mut costs = vec![0i128; candidates.len()];
    let mut best_upper = i128::MAX;
    let mut stale = 0;
    let mut scale = 16i128;
    let mut rounds = 0;
    for _ in 0..96 {
        rounds += 1;
        let total: i128 = prices.iter().sum();
        let mut maxima = [i128::MIN; 3];
        let mut winners = [usize::MAX; 3];
        for (i, candidate) in candidates.iter().enumerate() {
            if roles[i] == 0 {
                continue;
            }
            let (cards, len) = &resources[i];
            let cost = cards[..*len].iter().map(|&c| prices[c]).sum::<i128>();
            costs[i] = cost;
            for song in 0..3 {
                if roles[i] & (1 << song) == 0 {
                    continue;
                }
                let value = i128::from(candidate.scores[song]) * PRICE_SCALE - cost;
                if value > maxima[song] {
                    maxima[song] = value;
                    winners[song] = i;
                }
            }
        }
        if winners.contains(&usize::MAX) {
            roles.fill(0);
            break;
        }
        let upper = total + maxima.iter().sum::<i128>();
        if upper < best_upper {
            best_upper = upper;
            stale = 0;
        } else {
            stale += 1;
        }
        for (i, candidate) in candidates.iter().enumerate() {
            for song in 0..3 {
                let bound = upper - maxima[song] + i128::from(candidate.scores[song]) * PRICE_SCALE
                    - costs[i];
                if bound < threshold {
                    roles[i] &= !(1 << song);
                }
            }
        }
        if upper < threshold {
            roles.fill(0);
            break;
        }
        let mut usage = vec![0i64; card_count];
        for winner in winners {
            let (cards, len) = &resources[winner];
            for &card in &cards[..*len] {
                usage[card] += 1;
            }
        }
        // Projected subgradient: unused zero-price cards are already at optimum.
        let norm: i128 = usage
            .iter()
            .zip(&prices)
            .map(|(&n, &p)| {
                if p == 0 && n == 0 {
                    0
                } else {
                    i128::from((n - 1) * (n - 1))
                }
            })
            .sum();
        if norm == 0 {
            break;
        }
        if stale >= 8 {
            scale = (scale / 2).max(1);
            stale = 0;
        }
        let step = ((upper - threshold + 1) * scale / (10 * norm))
            .max(1)
            .min(limit);
        for (p, &n) in prices.iter_mut().zip(&usage) {
            *p = p
                .saturating_add(step.saturating_mul(i128::from(n - 1)))
                .clamp(0, limit);
        }
    }
    if std::env::var_os("BANGDREAM_OPTIMIZE_DP_TRACE").is_some() {
        eprintln!(
            "DUAL_PRUNE before={} after={} roles={} rounds={} upper={} floor={} elapsed_ms={:.3}",
            candidates.len(),
            roles.iter().filter(|&&r| r != 0).count(),
            roles.iter().map(|r| r.count_ones()).sum::<u32>(),
            rounds,
            best_upper.div_euclid(PRICE_SCALE)
                + i128::from(best_upper.rem_euclid(PRICE_SCALE) != 0),
            floor,
            start.elapsed_ms()
        );
    }
    roles
}
