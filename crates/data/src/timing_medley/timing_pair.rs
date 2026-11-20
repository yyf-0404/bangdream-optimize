//! Certified card-price duals for the other two songs. Price optimization is
//! heuristic, but EVERY recorded certificate is an upper bound independently.
//! Each oracle keeps five distinct characters and one captain; the price dual
//! relaxes cross-team card sharing without charging it twice for the captain.
use super::timing_pool::Choice;
use bangdream_optimize_core::{
    hit_timing::bounds::PoolTimingUpper, scoring::MemberPower, Attribute, PreparedCard,
};
use std::collections::BTreeMap;
#[path = "timing_pair_blocking.rs"]
mod blocking;

#[derive(Clone, Copy)]
struct Candidate {
    card: usize,
    power: f64,
    weights: [[f64; 2]; 5],
}
struct Partition {
    groups: Vec<Vec<Candidate>>,
    error: f64,
    power_range: Option<[f64; 2]>,
}
pub struct Oracle {
    partitions: Vec<Partition>,
}
#[derive(Clone, Copy)]
struct State {
    value: f64,
    cards: [usize; 5],
    /// Only steers slope trials; score certificates do not depend on this
    /// diagnostic accumulation being the native scoring-power path.
    power: f64,
}
const EMPTY: State = State {
    value: f64::NEG_INFINITY,
    cards: [usize::MAX; 5],
    power: 0.,
};
impl Oracle {
    /// An upper for each forced member, across every allowed partition and
    /// trigger/captain assignment. Removing its character from the DP keeps
    /// the other four members distinct. No candidate roster is materialized.
    pub(super) fn forced(&self, prices: &[f64]) -> Vec<f64> {
        let mut result = vec![f64::NEG_INFINITY; prices.len()];
        for p in &self.partitions {
            assert!(
                p.power_range.is_none(),
                "forced-member shortcut requires an unrestricted power oracle"
            );
            let rows: Vec<Matrix> = p
                .groups
                .iter()
                .map(|group| {
                    std::array::from_fn(|slot| {
                        std::array::from_fn(|captain| {
                            group
                                .iter()
                                .map(|c| (c.weights[slot][captain] - prices[c.card]).next_up())
                                .fold(f64::NEG_INFINITY, f64::max)
                        })
                    })
                })
                .collect();
            for (omitted, group) in p.groups.iter().enumerate() {
                let mut dp = [f64::NEG_INFINITY; 64];
                dp[0] = 0.;
                for (i, row) in rows.iter().enumerate() {
                    if i == omitted {
                        continue;
                    }
                    let old = dp;
                    for state in 0..64 {
                        if !old[state].is_finite() || (state & 31).count_ones() >= 4 {
                            continue;
                        }
                        let used = state >> 5;
                        for slot in 0..5 {
                            if state & (1 << slot) != 0 {
                                continue;
                            }
                            for captain in 0..=(1 - used) {
                                let next = ((state & 31) | (1 << slot)) | ((used + captain) << 5);
                                dp[next] =
                                    dp[next].max((old[state] + row[slot][captain]).next_up());
                            }
                        }
                    }
                }
                for c in group {
                    for slot in 0..5 {
                        for captain in 0..2 {
                            let complement = (31 ^ (1 << slot)) | ((1 - captain) << 5);
                            let weight = (c.weights[slot][captain] - prices[c.card]).next_up();
                            let bound = ((dp[complement] + weight).next_up() + p.error).next_up();
                            result[c.card] = result[c.card].max(bound);
                        }
                    }
                }
            }
        }
        result
    }
    /// One declared signature, preserving distinct characters and the captain.
    /// Exact signature breaks may be relaxed, which only enlarges this upper.
    pub(super) fn fixed_context(
        model: &PoolTimingUpper,
        choices: &[[Choice; 4]],
        cards: &[PreparedCard],
        retained: &[usize],
        powers: &BTreeMap<u32, MemberPower>,
        song: usize,
        context: usize,
    ) -> Self {
        let mut grouped: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for &i in retained {
            grouped.entry(cards[i].character_id).or_default().push(i);
        }
        if grouped.len() < 5 {
            return Self { partitions: vec![] };
        }
        let mut peaks: Vec<_> = grouped
            .values()
            .map(|g| {
                g.iter()
                    .map(|&i| powers[&cards[i].card_id].linear_total())
                    .fold(0_f64, f64::max)
            })
            .collect();
        peaks.sort_by(|a, b| b.total_cmp(a));
        let upper = peaks[..5].iter().fold(0_f64, |a, b| (a + b).next_up());
        let linear = model.linearize(upper).expect("pool power cap");
        Self {
            partitions: vec![Partition {
                error: linear.error(),
                power_range: None,
                groups: grouped
                    .values()
                    .map(|g| {
                        g.iter()
                            .map(|&i| Candidate {
                                card: i,
                                power: powers[&cards[i].card_id].linear_total(),
                                weights: linear.weights(choices[i][context][song]),
                            })
                            .collect()
                    })
                    .collect(),
            }],
        }
    }
    pub fn new(
        model: &PoolTimingUpper,
        choices: &[[Choice; 4]],
        cards: &[PreparedCard],
        retained: &[usize],
        powers: &BTreeMap<u32, MemberPower>,
        song: usize,
    ) -> Self {
        Self::with_power_bins(model, choices, cards, retained, powers, song, 0)
    }
    /// Each range covers only L <= sum(member powers) <= U. Linearizing at
    /// U is safe there; a bounded slope search penalizes powers outside it.
    /// Ranges overlap outward at endpoints and their union covers every team.
    pub fn with_power_bins(
        model: &PoolTimingUpper,
        choices: &[[Choice; 4]],
        cards: &[PreparedCard],
        retained: &[usize],
        powers: &BTreeMap<u32, MemberPower>,
        song: usize,
        bins: usize,
    ) -> Self {
        Self::conditional(model, choices, cards, retained, powers, song, bins, None)
    }
    /// Deleted condition memberships must not re-enter a mixed partition just
    /// because the same card survives under another band/attribute condition.
    pub(super) fn conditional(
        model: &PoolTimingUpper,
        choices: &[[Choice; 4]],
        cards: &[PreparedCard],
        retained: &[usize],
        powers: &BTreeMap<u32, MemberPower>,
        song: usize,
        bins: usize,
        allowed: Option<&[[bool; 4]]>,
    ) -> Self {
        let bands: Vec<_> = cards
            .iter()
            .map(|c| c.band_id)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(Some)
            .chain(std::iter::once(None))
            .collect();
        let mut partitions = vec![];
        for attr in [
            None,
            Some(Attribute::Cool),
            Some(Attribute::Happy),
            Some(Attribute::Pure),
            Some(Attribute::Powerful),
        ] {
            for &band in &bands {
                let context = usize::from(band.is_some()) + 2 * usize::from(attr.is_some());
                let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
                for &i in retained {
                    let c = &cards[i];
                    if attr.is_none_or(|a| a == c.attribute)
                        && band.is_none_or(|b| b == c.band_id)
                        && allowed.is_none_or(|a| a[i][context])
                    {
                        groups.entry(c.character_id).or_default().push(i);
                    }
                }
                if groups.len() < 5 {
                    continue;
                }
                let mut peaks: Vec<_> = groups
                    .values()
                    .map(|g| {
                        g.iter()
                            .map(|&i| powers[&cards[i].card_id].linear_total())
                            .fold(0., f64::max)
                    })
                    .collect();
                peaks.sort_by(|a, b| b.total_cmp(a));
                let sum = peaks[..5].iter().fold(0_f64, |a, &b| (a + b).next_up());
                let mut lows: Vec<_> = groups
                    .values()
                    .map(|g| {
                        g.iter()
                            .map(|&i| powers[&cards[i].card_id].linear_total())
                            .fold(f64::INFINITY, f64::min)
                    })
                    .collect();
                lows.sort_by(f64::total_cmp);
                let lower = lows[..5]
                    .iter()
                    .fold(0_f64, |v, &p| (v + p).next_down().max(0.));
                for bin in 0..bins.max(1) {
                    let lo = (lower + (sum - lower) * (bin as f64) / (bins.max(1) as f64))
                        .next_down()
                        .max(0.);
                    let hi = if bin + 1 == bins.max(1) {
                        sum
                    } else {
                        (lower + (sum - lower) * ((bin + 1) as f64) / (bins as f64)).next_up()
                    };
                    let linear = model.linearize(hi).expect("pool cap covers power interval");
                    partitions.push(Partition {
                        error: linear.error(),
                        power_range: (bins > 0).then_some([lo, hi]),
                        groups: groups
                            .values()
                            .map(|g| {
                                g.iter()
                                    .map(|&i| Candidate {
                                        card: i,
                                        power: powers[&cards[i].card_id].linear_total(),
                                        weights: linear.weights(choices[i][context][song]),
                                    })
                                    .collect()
                            })
                            .collect(),
                    });
                }
            }
        }
        Self { partitions }
    }
    pub(super) fn unpriced_upper(&self, count: usize) -> u64 {
        self.best(&vec![0.; count]).value.max(0.).floor() as u64
    }
    /// Item-domain certificate. The incumbent may come from OTHER equipment,
    /// so it is a cutoff, never assumed achievable within these three oracles.
    /// Each dual independently covers all card-disjoint triples in this domain.
    pub(super) fn joint_item_upper(
        oracles: &[Self],
        count: usize,
        cutoff: u64,
        iterations: usize,
    ) -> (u64, usize) {
        assert_eq!(oracles.len(), 3);
        let mut prices = vec![0.; count];
        let mut best = u64::MAX;
        let mut rate = 0.;
        for step in 0..iterations.max(1) {
            let states: [State; 3] = std::array::from_fn(|s| oracles[s].best(&prices));
            let price_sum = prices.iter().fold(0_f64, |v, &p| (v + p).next_up());
            let total = states
                .iter()
                .fold(price_sum, |v, s| (v + s.value).next_up());
            assert!(total.is_finite() && total >= 0.);
            best = best.min(total.floor() as u64);
            if best < cutoff {
                return (best, step + 1);
            }
            if step == 0 {
                rate = total / 300.;
            }
            let mut uses = vec![0u8; count];
            for state in states {
                for i in state.cards {
                    uses[i] += 1;
                }
            }
            if uses.iter().all(|&n| n <= 1) {
                return (best, step + 1);
            }
            let step_size = rate * 0.5_f64.powi((step / 16) as i32);
            for (p, n) in prices.iter_mut().zip(uses) {
                *p = (*p + step_size * (f64::from(n) - 1.)).max(0.);
            }
        }
        (best, iterations.max(1))
    }
    fn best(&self, prices: &[f64]) -> State {
        let mut best = EMPTY;
        for p in &self.partitions {
            let result = p.best_in_range(prices);
            if result.value > best.value {
                best = result;
            }
        }
        best
    }
}
impl Partition {
    fn best_in_range(&self, prices: &[f64]) -> State {
        let mut best = self.best_at_slope(prices, 0.);
        let Some([lo, hi]) = self.power_range else {
            return best;
        };
        let direction = if best.power < lo {
            1.
        } else if best.power > hi {
            -1.
        } else {
            return best;
        };
        let mut left = 0.;
        let mut right = (best.value.abs() / hi.max(1.)).max(1.) * direction;
        // Every query is a certificate, even if this bounded search never
        // brackets the optimum or returns an infeasible maximizing roster.
        for _ in 0..6 {
            let value = self.best_at_slope(prices, right);
            if value.value < best.value {
                best = value;
            }
            if (direction > 0. && value.power >= lo) || (direction < 0. && value.power <= hi) {
                break;
            }
            left = right;
            right *= 2.;
        }
        for _ in 0..7 {
            let mid = (left + right) * 0.5;
            let value = self.best_at_slope(prices, mid);
            if value.value < best.value {
                best = value;
            }
            if (direction > 0. && value.power < lo) || (direction < 0. && value.power > hi) {
                left = mid;
            } else {
                right = mid;
            }
        }
        best
    }
    fn best_at_slope(&self, prices: &[f64], slope: f64) -> State {
        let p = self;
        let mut dp = [EMPTY; 64];
        dp[0].value = 0.;
        for group in &p.groups {
            let mut row = [[(f64::NEG_INFINITY, usize::MAX, 0.); 2]; 5];
            for c in group {
                for slot in 0..5 {
                    for captain in 0..2 {
                        let mut value = (c.weights[slot][captain] - prices[c.card]).next_up();
                        if slope != 0. {
                            value = (value + (slope * c.power).next_up()).next_up();
                        }
                        if value > row[slot][captain].0 {
                            row[slot][captain] = (value, c.card, c.power);
                        }
                    }
                }
            }
            // Read old states only: at most one member per character.
            let old = dp;
            for state in 0..64 {
                if !old[state].value.is_finite() {
                    continue;
                }
                let mask = state & 31;
                let used = state >> 5;
                for slot in 0..5 {
                    if mask & (1 << slot) != 0 {
                        continue;
                    }
                    for captain in 0..=(1 - used) {
                        let next = (mask | (1 << slot)) | ((used + captain) << 5);
                        let value = (old[state].value + row[slot][captain].0).next_up();
                        if value > dp[next].value {
                            dp[next] = old[state];
                            dp[next].value = value;
                            dp[next].cards[slot] = row[slot][captain].1;
                            dp[next].power += row[slot][captain].2;
                        }
                    }
                }
            }
        }
        let mut result = dp[63];
        result.value = (result.value + p.error).next_up();
        if let Some([lo, hi]) = p.power_range {
            let edge = if slope >= 0. { lo } else { hi };
            result.value = (result.value + (-slope * edge).next_up()).next_up();
        }
        result
    }
}

#[derive(Clone)]
pub struct Certificate {
    pub upper: f64,
    prices: Vec<f64>,
    singles: [f64; 2],
    blocking: Option<blocking::Bounds>,
}
impl Certificate {
    pub fn after_blocking(&self, blocked: &[usize]) -> f64 {
        let paid = blocked
            .iter()
            .fold(0_f64, |v, &i| (v + self.prices[i]).next_down().max(0.));
        let original = (self.upper - paid).next_up();
        self.blocking.as_ref().map_or(original, |b| {
            original.min(b.upper(blocked, paid, self.singles))
        })
    }
}
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub single_relaxed: [f64; 2],
    pub root_upper: u64,
    pub iterations: usize,
    pub certificates: usize,
    pub priced_cards: usize,
    pub blocking_cards: usize,
    pub blocking_max_drop: f64,
    pub blocking_key_cards: Vec<usize>,
    pub blocking_key_subsets: usize,
}
pub struct Pair {
    pub certificates: Vec<Certificate>,
    pub stats: Stats,
}
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TripleStats {
    pub upper: u64,
    pub iterations: usize,
}
impl Pair {
    pub fn key_cards(pairs: &[Self], count: usize, limit: usize) -> Vec<usize> {
        let mut ranked: Vec<_> = (0..count)
            .map(|i| {
                (
                    i,
                    pairs
                        .iter()
                        .flat_map(|p| &p.certificates)
                        .map(|c| c.prices[i])
                        .fold(0_f64, f64::max),
                )
            })
            .filter(|v| v.1 > 0.)
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked.truncate(limit);
        ranked.into_iter().map(|v| v.0).collect()
    }
    /// Only refines saved certificates, not every price-optimization step.
    /// A range oracle is an upper over ALL covered ranges; its maximizing
    /// roster is not an achievable lower score and is never used as one.
    pub fn tighten_joint(pairs: &mut [Self], oracles: &[Oracle]) -> usize {
        let mut cache: BTreeMap<Vec<u64>, [f64; 3]> = BTreeMap::new();
        for (s, pair) in pairs.iter_mut().enumerate() {
            for c in &mut pair.certificates {
                let key = c.prices.iter().map(|v| v.to_bits()).collect();
                let values = cache
                    .entry(key)
                    .or_insert_with(|| std::array::from_fn(|s| oracles[s].best(&c.prices).value));
                for other in 0..2 {
                    c.singles[other] = c.singles[other].min(values[(s + other + 1) % 3]);
                }
                let sum = c.prices.iter().fold(0_f64, |v, &p| (v + p).next_up());
                c.upper = c
                    .upper
                    .min(((c.singles[0] + c.singles[1]).next_up() + sum).next_up());
            }
            pair.stats.root_upper = pair
                .certificates
                .iter()
                .map(|c| c.upper.floor() as u64)
                .min()
                .unwrap();
        }
        cache.len()
    }
    /// Force one card into each possible song and certify the best possible
    /// three-song sum. Uses original whole-pool oracles; pruning several cards
    /// together cannot invalidate the retained upper bounds.
    pub fn card_uppers(pairs: &[Self], oracles: &[Oracle], count: usize) -> Vec<[u64; 3]> {
        let mut result = vec![[u64::MAX; 3]; count];
        for s in 0..3 {
            for c in &pairs[s].certificates {
                let forced = oracles[s].forced(&c.prices);
                for i in 0..count {
                    let bound = (forced[i] + c.upper).next_up().max(0.).floor() as u64;
                    result[i][s] = result[i][s].min(bound);
                }
            }
        }
        result
    }
    /// Three-song prices target the resource scarcity of a complete plan.
    /// Extracting either pair still yields a valid certificate for that pair.
    pub fn add_triple_prices(
        pairs: &mut [Self],
        oracles: &[Oracle],
        count: usize,
        seed: u64,
        adaptive_prices: bool,
    ) -> TripleStats {
        let mut stats = TripleStats::default();
        let mut kept = vec![];
        // A globally smaller dual may be looser after blocking a particular
        // team, so keep the old certificates as well as the adaptive ones.
        for adaptive in [false, true].into_iter().filter(|v| !*v || adaptive_prices) {
            let mut prices = vec![0.; count];
            let mut saved = vec![];
            let mut rate = 0.;
            for step in 0..if adaptive { 512 } else { 96 } {
                let states: [State; 3] = std::array::from_fn(|s| oracles[s].best(&prices));
                let price_sum = prices.iter().fold(0_f64, |v, &p| (v + p).next_up());
                let total = states
                    .iter()
                    .fold(price_sum, |v, s| (v + s.value).next_up());
                if step == 0 {
                    rate = total / 300.;
                }
                assert!(
                    total.floor() >= seed as f64,
                    "three-song upper fell below validated seed"
                );
                stats.iterations += 1;
                saved.push((total, prices.clone(), states));
                let mut uses = vec![0u8; count];
                for s in states {
                    for i in s.cards {
                        uses[i] += 1;
                    }
                }
                let norm = uses
                    .iter()
                    .zip(&prices)
                    .filter(|(n, p)| **n > 1 || **p > 0.)
                    .map(|(&n, _)| (f64::from(n) - 1.).powi(2))
                    .sum::<f64>();
                // Complementarity matters too: an unused card with a positive
                // price still makes the certificate unnecessarily loose.
                if (adaptive && norm == 0.) || (!adaptive && uses.iter().all(|&n| n <= 1)) {
                    break;
                }
                // Polyak-style projected steps use a VALIDATED achievable score.
                // They tune quality only; each saved dual remains independently
                // valid if the relaxed optimum is above that target or we stop.
                let step_size = if !adaptive {
                    rate * 0.5_f64.powi((step / 16) as i32)
                } else if seed > 0 {
                    (total - seed as f64).max(0.) / norm * 0.75_f64.powi((step / 64) as i32)
                } else {
                    rate * 0.75_f64.powi((step / 64) as i32)
                };
                for (p, n) in prices.iter_mut().zip(uses) {
                    *p = (*p + step_size * (f64::from(n) - 1.)).max(0.);
                }
            }
            saved.sort_by(|a, b| a.0.total_cmp(&b.0));
            saved.truncate(8);
            kept.extend(saved);
        }
        stats.upper = kept.iter().map(|r| r.0.floor() as u64).min().unwrap();
        for (_, prices, states) in kept {
            for excluded in 0..3 {
                let singles = [
                    states[(excluded + 1) % 3].value,
                    states[(excluded + 2) % 3].value,
                ];
                let sum = prices.iter().fold(0_f64, |v, &p| (v + p).next_up());
                pairs[excluded].certificates.push(Certificate {
                    upper: ((singles[0] + singles[1]).next_up() + sum).next_up(),
                    prices: prices.clone(),
                    singles,
                    blocking: None,
                });
            }
        }
        for p in pairs {
            p.stats.root_upper = p
                .certificates
                .iter()
                .map(|c| c.upper.floor() as u64)
                .min()
                .unwrap();
            p.stats.certificates = p.certificates.len();
            p.stats.priced_cards = (0..count)
                .filter(|&i| p.certificates.iter().any(|c| c.prices[i] > 0.))
                .count();
        }
        stats
    }
    pub fn audit_team(&self, cards: &[usize; 5], coarse: u64, song: usize) {
        for c in &self.certificates {
            let priced = coarse as f64 - cards.iter().map(|&i| c.prices[i]).sum::<f64>();
            assert!(
                priced <= c.singles[song],
                "priced oracle underestimated a real roster"
            );
        }
    }
    pub fn new(a: &Oracle, b: &Oracle, count: usize, iterations: usize) -> Self {
        let mut prices = vec![0.; count];
        let mut certificates = vec![];
        let mut stats = Stats::default();
        let mut start_step = 0.;
        for step in 0..iterations {
            let x = a.best(&prices);
            let y = b.best(&prices);
            assert!(x.value.is_finite() && y.value.is_finite());
            if step == 0 {
                stats.single_relaxed = [x.value, y.value];
                start_step = (x.value + y.value) / 200.;
            }
            let sum = prices.iter().fold(0_f64, |a, &b| (a + b).next_up());
            let upper = ((x.value + y.value).next_up() + sum).next_up();
            certificates.push(Certificate {
                upper,
                prices: prices.clone(),
                singles: [x.value, y.value],
                blocking: None,
            });
            stats.iterations += 1;
            let mut uses = vec![0u8; count];
            for i in x.cards.into_iter().chain(y.cards) {
                uses[i] += 1;
            }
            if uses.iter().all(|&n| n <= 1) {
                break;
            }
            // Only affects certificate quality; no convergence claim or lower
            // score is inferred from the independent oracle winners.
            let rate = start_step * 0.5_f64.powi((step / 12) as i32);
            for (p, n) in prices.iter_mut().zip(uses) {
                *p = (*p + rate * (f64::from(n) - 1.)).max(0.);
            }
        }
        certificates.sort_by(|a, b| a.upper.total_cmp(&b.upper));
        // Every certificate individually covers all teams. Keeping only a
        // bounded best subset weakens pruning; it cannot exclude a valid team.
        certificates.truncate(8);
        stats.certificates = certificates.len();
        stats.root_upper = certificates[0].upper.floor() as u64;
        stats.priced_cards = (0..count)
            .filter(|&i| certificates.iter().any(|c| c.prices[i] > 0.))
            .count();
        Self {
            certificates,
            stats,
        }
    }
    pub fn upper(&self, blocked: &[usize]) -> u64 {
        self.certificates
            .iter()
            .map(|c| c.after_blocking(blocked))
            .fold(f64::INFINITY, f64::min)
            .max(0.)
            .floor() as u64
    }
    /// Lowest prefilter pair bound across all remaining choices, for census
    /// acceptance only. Each future group can pay at most its largest price.
    pub fn worst_blocking_upper(&self, blocked: &[usize], remaining: &[&[usize]]) -> u64 {
        self.certificates
            .iter()
            .map(|c| {
                let mut value = c.after_blocking(blocked);
                for group in remaining {
                    let price = group.iter().map(|&i| c.prices[i]).fold(0., f64::max);
                    value = (value - price).next_down();
                }
                value
            })
            .fold(f64::INFINITY, f64::min)
            .max(0.)
            .floor() as u64
    }
}

struct PrefixEntry {
    constant: f64,
    maximum: [[f64; 2]; 5],
    minimum: [[f64; 2]; 5],
    certificate: usize,
}
type Matrix = [[f64; 2]; 5];
fn peak(matrix: Matrix) -> [f64; 2] {
    std::array::from_fn(|k| {
        matrix
            .iter()
            .map(|r| r[k])
            .fold(f64::NEG_INFINITY, f64::max)
    })
}
impl PrefixEntry {
    fn new(
        raw: &[[f64; 2]],
        lists: &[&[usize]],
        c: &Certificate,
        certificate: usize,
        error: f64,
    ) -> Self {
        let maximum = std::array::from_fn(|g| {
            std::array::from_fn(|k| {
                lists[g]
                    .iter()
                    .map(|&i| (raw[i][k] - c.prices[i]).next_up())
                    .fold(f64::NEG_INFINITY, f64::max)
            })
        });
        let minimum = std::array::from_fn(|g| {
            std::array::from_fn(|k| {
                lists[g]
                    .iter()
                    .map(|&i| (raw[i][k] - c.prices[i]).next_down())
                    .fold(f64::INFINITY, f64::min)
            })
        });
        Self {
            constant: (error + c.upper).next_up(),
            maximum,
            minimum,
            certificate,
        }
    }
}
/// Fixes five character groups, not five cards. Upper rows independently
/// relax the first-five trigger assignment so future expensive cards also pay
/// their prices BEFORE those cards are enumerated.
pub struct PricedPrefix {
    raw: [Vec<[f64; 2]>; 3],
    entries: [Vec<PrefixEntry>; 3],
}
fn row_upper(rows: [[f64; 2]; 5], constant: f64) -> f64 {
    let sum = rows.iter().fold(0_f64, |v, r| (v + r[0]).next_up());
    let extra = rows
        .iter()
        .map(|r| (r[1] - r[0]).next_up())
        .fold(f64::NEG_INFINITY, f64::max);
    ((sum + extra).next_up() + constant).next_up()
}
fn row_lower(rows: [[f64; 2]; 5], constant: f64) -> f64 {
    (0..5)
        .map(|captain| {
            (rows.iter().enumerate().fold(0_f64, |v, (i, r)| {
                (v + r[usize::from(i == captain)]).next_down()
            }) + constant)
                .next_down()
        })
        .fold(f64::NEG_INFINITY, f64::max)
}
impl PricedPrefix {
    pub fn new(
        models: &[PoolTimingUpper],
        choices: &[[Choice; 4]],
        power: &[[f64; 3]],
        lists: &[&[usize]],
        context: usize,
        pairs: &[Pair],
    ) -> Self {
        let mut raw = std::array::from_fn(|_| vec![[f64::NAN; 2]; choices.len()]);
        let mut entries: [Vec<PrefixEntry>; 3] = std::array::from_fn(|_| vec![]);
        if pairs.is_empty() {
            return Self { raw, entries };
        }
        for s in 0..3 {
            let sum = lists.iter().fold(0_f64, |v, g| {
                (v + g.iter().map(|&i| power[i][s]).fold(0., f64::max)).next_up()
            });
            let linear = models[s]
                .linearize(sum)
                .expect("pool cap covers five character groups");
            for group in lists {
                for &i in *group {
                    raw[s][i] = peak(linear.weights(choices[i][context][s]));
                }
            }
            for (index, c) in pairs[s].certificates.iter().enumerate() {
                entries[s].push(PrefixEntry::new(&raw[s], lists, c, index, linear.error()));
            }
            entries[s].sort_by(|a, b| {
                row_upper(a.maximum, a.constant).total_cmp(&row_upper(b.maximum, b.constant))
            });
        }
        Self { raw, entries }
    }
    pub fn competitive(&self, pairs: &[Pair], chosen: &[usize], seed: u64) -> bool {
        (0..3).any(|s| {
            self.entries[s].iter().all(|e| {
                let c = &pairs[s].certificates[e.certificate];
                let matrices = std::array::from_fn(|g| {
                    if g < chosen.len() {
                        let i = chosen[g];
                        self.raw[s][i].map(|v| (v - c.prices[i]).next_up())
                    } else {
                        e.maximum[g]
                    }
                });
                row_upper(matrices, e.constant).floor() >= seed as f64
            })
        })
    }
    /// Lower bound on this prefilter's own values, never on game score.
    pub fn census_competitive(&self, pairs: &[Pair], chosen: &[usize], seed: u64) -> bool {
        (0..3).any(|s| {
            self.entries[s].iter().all(|e| {
                let c = &pairs[s].certificates[e.certificate];
                let rows = std::array::from_fn(|g| {
                    if g < chosen.len() {
                        let i = chosen[g];
                        self.raw[s][i].map(|v| (v - c.prices[i]).next_down())
                    } else {
                        e.minimum[g]
                    }
                });
                row_lower(rows, e.constant).floor() >= seed as f64
            })
        })
    }
}

#[cfg(test)]
#[path = "timing_pair/tests.rs"]
mod tests;
