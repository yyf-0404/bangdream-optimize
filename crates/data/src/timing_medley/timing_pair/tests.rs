use super::*;
use bangdream_optimize_core::hit_timing::layouts::physical_layouts;

#[test]
fn song_eligibility_unions_conditions_and_preserves_every_competitive_triple() {
    let make = |song: usize, strong: bool| {
        let mut oracle = example(0);
        for group in &mut oracle.partitions[0].groups {
            for c in group {
                let value = if strong && c.card % 3 == song {
                    900.
                } else {
                    10.
                };
                c.weights = [[value; 2]; 5];
            }
        }
        oracle
    };
    let oracles: Vec<_> = (0..3)
        .map(|s| {
            let mut local = make(s, false);
            local.partitions.extend(make(s, true).partitions);
            local
        })
        .collect();
    let pairs: Vec<_> = (0..3)
        .map(|s| Pair::new(&oracles[(s + 1) % 3], &oracles[(s + 2) % 3], 15, 32))
        .collect();
    let seed = 13500;
    let (masks, _) = super::super::timing_eligibility::certify(&oracles, &pairs, 15, [9000; 3], seed);
    for (i, &mask) in masks.iter().enumerate() {
        assert_eq!(mask, 1 << (i % 3));
    }
    // The target's weaker local condition alone would wrongly declare every
    // card absent from every song. The whole-domain snapshot retains them.
    let local: Vec<_> = (0..3).map(|s| make(s, false)).collect();
    let (local_masks, _) = super::super::timing_eligibility::certify(&local, &pairs, 15, [9000; 3], seed);
    assert!(local_masks.iter().all(|&m| m == 0));
    let all: Vec<_> = (0..3).map(|s| teams(&make(s, true))).collect();
    let mut checked = 0;
    for (a, sa) in &all[0] {
        for (b, sb) in &all[1] {
            if a.iter().any(|i| b.contains(i)) {
                continue;
            }
            let remaining: [usize; 5] = std::array::from_fn(|g| {
                (g * 3..g * 3 + 3)
                    .find(|i| !a.contains(i) && !b.contains(i))
                    .unwrap()
            });
            let sc = all[2].iter().find(|(c, _)| *c == remaining).unwrap().1;
            if sa + sb + sc < seed as f64 {
                continue;
            }
            for (song, ids) in [a, b, &remaining].iter().enumerate() {
                assert!(ids.iter().all(|&i| masks[i] & (1 << song) != 0));
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "exercise the equality-to-seed boundary");
}
fn example(seed: usize) -> Oracle {
    Oracle {
        partitions: vec![Partition {
            error: 0.,
            power_range: None,
            groups: (0..5)
                .map(|c| {
                    (0..3)
                        .map(|i| {
                            let card = c * 3 + i;
                            Candidate {
                                card,
                                power: (card * 11 + 100) as f64,
                                weights: std::array::from_fn(|s| {
                                    let normal = ((card * 7 + s * 13 + seed * 19) % 29) as f64
                                        + if i == 0 { 90. } else { 10. };
                                    [normal, normal + ((card * 11 + s * 3 + seed) % 17) as f64]
                                }),
                            }
                        })
                        .collect()
                })
                .collect(),
        }],
    }
}
fn teams(oracle: &Oracle) -> Vec<([usize; 5], f64)> {
    let mut out = vec![];
    for number in 0..243 {
        let mut n = number;
        let ids = std::array::from_fn(|c| {
            let id = c * 3 + n % 3;
            n /= 3;
            id
        });
        let mut best = f64::NEG_INFINITY;
        for order in physical_layouts() {
            for captain in 0..5 {
                let sum = (0..5)
                    .map(|slot| {
                        let id = ids[order[slot]];
                        oracle.partitions[0].groups[id / 3][id % 3].weights[slot]
                            [usize::from(captain == slot)]
                    })
                    .sum::<f64>();
                best = best.max(sum);
            }
        }
        out.push((ids, best));
    }
    out
}
#[test]
fn item_dual_covers_exhaustive_triples_even_when_cutoff_is_from_other_equipment() {
    let oracles = vec![example(1), example(7), example(13)];
    let all: Vec<_> = oracles.iter().map(teams).collect();
    let lookup: BTreeMap<_, _> = all[2].iter().copied().collect();
    let mut exact = 0_f64;
    for (a, sa) in &all[0] {
        for (b, sb) in &all[1] {
            if a.iter().any(|i| b.contains(i)) {
                continue;
            }
            let c = std::array::from_fn(|g| {
                (g * 3..g * 3 + 3)
                    .find(|i| !a.contains(i) && !b.contains(i))
                    .unwrap()
            });
            exact = exact.max(sa + sb + lookup[&c]);
        }
    }
    let loose: u64 = oracles.iter().map(|o| o.unpriced_upper(15)).sum();
    for cutoff in [0, exact as u64, loose + 1] {
        let mut last = u64::MAX;
        for budget in [1, 2, 16, 32] {
            let (upper, iterations) = Oracle::joint_item_upper(&oracles, 15, cutoff, budget);
            assert!(upper >= exact as u64);
            assert!(upper <= last);
            assert!((1..=budget).contains(&iterations));
            if cutoff > loose {
                assert_eq!(iterations, 1);
            }
            last = upper;
        }
    }
    assert!(Oracle::joint_item_upper(&oracles, 15, 0, 32).0 < loose);
}
#[test]
fn every_price_certificate_covers_all_disjoint_pairs_and_blocked_complements() {
    let a = example(1);
    let b = example(7);
    let ta = teams(&a);
    let tb = teams(&b);
    let pair = Pair::new(&a, &b, 15, 64);
    assert!(pair.stats.root_upper < pair.stats.single_relaxed.iter().sum::<f64>().floor() as u64);
    for (ids, x) in &ta {
        for (other, y) in &tb {
            if ids.iter().any(|i| other.contains(i)) {
                continue;
            }
            let blocked: Vec<_> = (0..15)
                .filter(|i| !ids.contains(i) && !other.contains(i))
                .collect();
            assert_eq!(blocked.len(), 5);
            assert!(pair.upper(&blocked) >= (*x + *y) as u64);
            for certificate in &pair.certificates {
                assert!(certificate.after_blocking(&blocked) >= x + y);
            }
        }
    }
    // An independently enumerated max for arbitrary prices, including negative
    // adjusted row weights, validates the five-character/captain DP itself.
    let prices: Vec<_> = (0..15).map(|i| ((i * 71) % 197) as f64).collect();
    let expected = ta
        .iter()
        .map(|(ids, v)| v - ids.iter().map(|&i| prices[i]).sum::<f64>())
        .fold(f64::NEG_INFINITY, f64::max);
    let actual = a.best(&prices);
    assert!(actual.value >= expected && actual.value - expected < 1e-8);
    let forced = a.forced(&prices);
    for i in 0..15 {
        let expected = ta
            .iter()
            .filter(|(ids, _)| ids.contains(&i))
            .map(|(ids, v)| v - ids.iter().map(|&j| prices[j]).sum::<f64>())
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(forced[i] >= expected && forced[i] - expected < 1e-8);
    }
    assert_eq!(
        actual
            .cards
            .iter()
            .map(|i| i / 3)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        5
    );
}

#[test]
fn blocking_certificates_cover_all_pairs_and_every_blocked_prefix() {
    let oracles = vec![example(1), example(7), example(13)];
    let all: Vec<_> = oracles.iter().map(teams).collect();
    let mut pairs: Vec<_> = (0..3)
        .map(|s| {
            let a = &oracles[(s + 1) % 3];
            let b = &oracles[(s + 2) % 3];
            let mut pair = Pair::new(a, b, 15, 64);
            let prices = vec![0.; 15];
            let singles = [a.best(&prices).value, b.best(&prices).value];
            pair.certificates.push(Certificate {
                upper: (singles[0] + singles[1]).next_up(),
                singles,
                prices,
                blocking: None,
            });
            pair
        })
        .collect();
    let old: Vec<_> = pairs.iter().map(|p| p.certificates.clone()).collect();
    Pair::prepare_blocking(
        &mut pairs,
        &oracles,
        &(0..15).map(|i| i / 3).collect::<Vec<_>>(),
        12,
    );
    let mut tightened = false;
    for s in 0..3 {
        for (ids, a) in &all[(s + 1) % 3] {
            for (other, b) in &all[(s + 2) % 3] {
                if ids.iter().any(|i| other.contains(i)) {
                    continue;
                }
                let blocked: Vec<_> = (0..15)
                    .filter(|i| !ids.contains(i) && !other.contains(i))
                    .collect();
                for length in 0..=5 {
                    let prefix = &blocked[..length];
                    assert!(pairs[s].upper(prefix) >= (a + b) as u64);
                    for (c, before) in pairs[s].certificates.iter().zip(&old[s]) {
                        let upper = c.after_blocking(prefix);
                        assert!(upper >= a + b);
                        assert!(upper <= before.after_blocking(prefix));
                        tightened |= upper < before.after_blocking(prefix) - 1.;
                    }
                }
            }
        }
    }
    assert!(tightened);
}

#[test]
fn rebuilt_certificates_cover_surviving_pairs_without_loosening_old_bounds() {
    let mut oracles = vec![example(1), example(7), example(13)];
    let all: Vec<_> = oracles.iter().map(teams).collect();
    let mut pairs: Vec<_> = (0..3)
        .map(|s| Pair::new(&oracles[(s + 1) % 3], &oracles[(s + 2) % 3], 15, 64))
        .collect();
    let before: Vec<_> = pairs.iter().map(|p| p.certificates.clone()).collect();
    // Remove high-scoring alternatives. A card may remain in one song's
    // allowed domain while disappearing from another, as with condition pools.
    let allowed = |s: usize, id: usize| id != 3 * s;
    for (s, oracle) in oracles.iter_mut().enumerate() {
        for group in &mut oracle.partitions[0].groups {
            group.retain(|c| allowed(s, c.card));
        }
    }
    Pair::tighten_joint(&mut pairs, &oracles);
    let mut strict = 0;
    for s in 0..3 {
        for (new, old) in pairs[s].certificates.iter().zip(&before[s]) {
            assert_eq!(new.prices, old.prices);
            assert!(new.upper <= old.upper);
            strict += usize::from(new.upper < old.upper - 1.);
        }
        let a = (s + 1) % 3;
        let b = (s + 2) % 3;
        for (ids, x) in &all[a] {
            if ids.iter().any(|&i| !allowed(a, i)) {
                continue;
            }
            pairs[s].audit_team(ids, *x as u64, 0);
            for (other, y) in &all[b] {
                if other.iter().any(|&i| !allowed(b, i)) || ids.iter().any(|i| other.contains(i)) {
                    continue;
                }
                let blocked: Vec<_> = (0..15)
                    .filter(|i| !ids.contains(i) && !other.contains(i))
                    .collect();
                assert!(pairs[s].upper(&blocked) >= (*x + *y) as u64);
            }
        }
    }
    assert!(strict > 0);
}

#[test]
fn partial_member_prices_preserve_every_feasible_triple_and_census_is_conservative() {
    let oracles = vec![example(1), example(7), example(13)];
    let all: Vec<_> = oracles.iter().map(teams).collect();
    let mut pairs: Vec<_> = (0..3)
        .map(|s| Pair::new(&oracles[(s + 1) % 3], &oracles[(s + 2) % 3], 15, 64))
        .collect();
    let seed = all
        .iter()
        .enumerate()
        .map(|(s, rows)| {
            rows.iter()
                .find(|(ids, _)| ids.iter().all(|i| i % 3 == s))
                .unwrap()
                .1
        })
        .sum::<f64>() as u64;
    Pair::add_triple_prices(&mut pairs, &oracles, 15, seed, true);
    let mut fixed: Vec<_> = (0..3)
        .map(|s| Pair::new(&oracles[(s + 1) % 3], &oracles[(s + 2) % 3], 15, 64))
        .collect();
    Pair::add_triple_prices(&mut fixed, &oracles, 15, seed, false);
    for (old, new) in fixed.iter().zip(&pairs) {
        for c in &old.certificates {
            assert!(new
                .certificates
                .iter()
                .any(|n| n.prices == c.prices && n.upper == c.upper && n.singles == c.singles));
        }
    }
    let card_uppers = Pair::card_uppers(&pairs, &oracles, 15);
    let raw = std::array::from_fn(|s| {
        (0..15)
            .map(|i| peak(oracles[s].partitions[0].groups[i / 3][i % 3].weights))
            .collect::<Vec<_>>()
    });
    let groups: Vec<Vec<usize>> = (0..5).map(|g| (g * 3..g * 3 + 3).collect()).collect();
    let lists: Vec<_> = groups.iter().map(Vec::as_slice).collect();
    let entries = std::array::from_fn(|s| {
        pairs[s]
            .certificates
            .iter()
            .enumerate()
            .map(|(index, c)| PrefixEntry::new(&raw[s], &lists, c, index, 0.))
            .collect()
    });
    let prefix = PricedPrefix { raw, entries };
    let by_ids: BTreeMap<_, _> = all[2].iter().copied().collect();
    let mut n = 0;
    for (a, x) in &all[0] {
        for (b, y) in &all[1] {
            if a.iter().any(|i| b.contains(i)) {
                continue;
            }
            let third = std::array::from_fn(|g| {
                (g * 3..g * 3 + 3)
                    .find(|i| !a.contains(i) && !b.contains(i))
                    .unwrap()
            });
            let score = (x + y + by_ids[&third]) as u64;
            for (s, t) in [*a, *b, third].iter().enumerate() {
                for &i in t {
                    assert!(card_uppers[i][s] >= score);
                }
            }
            for t in [*a, *b, third] {
                assert!(prefix.competitive(&pairs, &t, score));
                assert!(prefix.competitive(&pairs, &t[..n % 6], score));
            }
            n += 1;
        }
    }
    assert_eq!(n, 7776);
    let mut prefixes = std::collections::BTreeSet::new();
    for (team, _) in &all[0] {
        for d in 0..=5 {
            prefixes.insert(team[..d].to_vec());
        }
    }
    for chosen in prefixes {
        for seed in [1200, 1450, 1650, 1800, 2000] {
            if prefix.census_competitive(&pairs, &chosen, seed) {
                for (team, _) in &all[0] {
                    if team.starts_with(&chosen) {
                        assert!(prefix.competitive(&pairs, team, seed));
                    }
                }
            }
        }
    }
}

#[test]
fn power_intervals_cover_joint_products_with_prices_and_both_slope_directions() {
    let power = |i: usize| [100., 80., 40.][i % 3] + (i / 3) as f64;
    let gain = |i: usize, slot: usize, captain: usize| {
        [0.1, 0.8, 2.0][i % 3] * ([0.5, 0.9, 1.2, 0.7, 1.1][slot] + captain as f64 * 0.85)
    };
    let mut real = vec![];
    for code in 0..243 {
        let mut n = code;
        let ids: [usize; 5] = std::array::from_fn(|g| {
            let id = g * 3 + n % 3;
            n /= 3;
            id
        });
        let p = ids.iter().map(|&i| power(i)).sum::<f64>();
        let mut best = 0_f64;
        for order in physical_layouts() {
            for captain in 0..5 {
                let g = (0..5)
                    .map(|slot| gain(ids[order[slot]], slot, usize::from(slot == captain)))
                    .sum::<f64>();
                best = best.max(p * (1. + g));
            }
        }
        real.push((ids, p, best));
    }
    for prices in [
        vec![0.; 15],
        (0..15).map(|i| ((i * 71) % 197) as f64).collect(),
    ] {
        let expected = real
            .iter()
            .map(|(ids, _, v)| v - ids.iter().map(|&i| prices[i]).sum::<f64>())
            .fold(f64::NEG_INFINITY, f64::max);
        let mut first = 0.;
        for bins in [1, 3, 8, 16] {
            let partitions = (0..bins)
                .map(|b| {
                    let lo = (210. + 300. * b as f64 / bins as f64).next_down();
                    let hi = (210. + 300. * (b + 1) as f64 / bins as f64).next_up();
                    Partition {
                        power_range: Some([lo, hi]),
                        error: 0.,
                        groups: (0..5)
                            .map(|g| {
                                (0..3)
                                    .map(|m| {
                                        let card = g * 3 + m;
                                        Candidate {
                                            card,
                                            power: power(card),
                                            weights: std::array::from_fn(|slot| {
                                                std::array::from_fn(|captain| {
                                                    (power(card)
                                                        + (hi * gain(card, slot, captain))
                                                            .next_up())
                                                    .next_up()
                                                })
                                            }),
                                        }
                                    })
                                    .collect()
                            })
                            .collect(),
                    }
                })
                .collect();
            let oracle = Oracle { partitions };
            let bound = oracle.best(&prices).value;
            assert!(
                bound >= expected,
                "{bins} ranges underestimated: {bound} < {expected}"
            );
            if bins == 1 {
                first = bound;
            }
            if bins == 16 {
                assert!(bound < first * 0.85, "joint bound did not tighten product");
            }
            for p in &oracle.partitions {
                let [lo, hi] = p.power_range.unwrap();
                for (ids, power, value) in &real {
                    if *power < lo || *power > hi {
                        continue;
                    }
                    let value = value - ids.iter().map(|&i| prices[i]).sum::<f64>();
                    for slope in [-50., -3.5, 0., 1.75, 50.] {
                        assert!(p.best_at_slope(&prices, slope).value >= value);
                    }
                }
            }
        }
    }
}
