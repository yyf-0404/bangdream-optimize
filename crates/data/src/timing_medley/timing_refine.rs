//! Rebuild bounds on the surviving conditional domain, after replacement.
//! Does not add a new replacement rule or change the independent-window model.
use super::{
    timing_pair::{Oracle, Pair},
    timing_pool::Choice,
};
use bangdream_optimize_core::{
    hit_timing::{
        bounds::{PoolTimingChoice, PoolTimingUpper},
        ReplayInput, ReplaySkill,
    },
    scoring::{MemberPower, ScoringPower},
    Attribute, PreparedCard,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},

};

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Stats {
    pub skills_before: usize,
    pub skills_after: usize,
    pub memberships: usize,
    pub model_seconds: f64,
    pub oracle_seconds: f64,
    pub single_before: [u64; 3],
    pub single_after: [u64; 3],
    pub pairs_before: Vec<u64>,
    pub pairs_after: Vec<u64>,
    pub price_sets: usize,
    pub cutoff_seconds: f64,
    pub cutoff_rounds: Vec<CutoffRound>,
    pub cutoff_stable: bool,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CutoffRound {
    pub before: usize,
    pub after: usize,
    /// Sequential proofs: [card ID, condition index], applied simultaneously
    /// within a round. Earlier replacement witnesses need survive their own
    /// proof stage, not a later score-cutoff stage.
    pub removed: Vec<[u32; 2]>,
    pub replacements: Vec<super::timing_replacement::SignatureStats>,
    pub replacement_seconds: f64,
    pub after_replacement: usize,
    pub forced_product: super::timing_product::Stats,
    pub song_eligibility: Option<super::timing_eligibility::Stats>,
}

pub(super) fn cutoffs(
    cards: &[PreparedCard],
    powers: &[BTreeMap<u32, MemberPower>],
    allowed: &mut [[bool; 4]],
    models: &[PoolTimingUpper],
    choices: &[[Choice; 4]],
    pairs: &[Pair],
    singles: [u64; 3],
    seed: u64,
) -> CutoffRound {
    let mut round = CutoffRound {
        before: allowed.iter().flatten().filter(|&&v| v).count(),
        ..CutoffRound::default()
    };
    let bands: Vec<_> = cards
        .iter()
        .map(|c| c.band_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(Some)
        .chain(std::iter::once(None))
        .collect();
    for attr in [
        None,
        Some(Attribute::Cool),
        Some(Attribute::Happy),
        Some(Attribute::Pure),
        Some(Attribute::Powerful),
    ] {
        for &band in &bands {
            let context = usize::from(band.is_some()) + 2 * usize::from(attr.is_some());
            let indices: Vec<_> = (0..cards.len())
                .filter(|&i| {
                    allowed[i][context]
                        && band.is_none_or(|b| cards[i].band_id == b)
                        && attr.is_none_or(|a| cards[i].attribute == a)
                })
                .collect();
            if indices.is_empty() {
                continue;
            }
            let oracles: Vec<_> = (0..3)
                .map(|s| {
                    Oracle::fixed_context(
                        &models[s], choices, cards, &indices, &powers[s], s, context,
                    )
                })
                .collect();
            let mut uppers = if pairs.is_empty() {
                vec![[u64::MAX; 3]; cards.len()]
            } else {
                Pair::card_uppers(pairs, &oracles, cards.len())
            };
            for s in 0..3 {
                let forced = oracles[s].forced(&vec![0.; cards.len()]);
                let other = singles[(s + 1) % 3] + singles[(s + 2) % 3];
                for &i in &indices {
                    let upper = (forced[i].max(0.).floor() as u64).saturating_add(other);
                    uppers[i][s] = uppers[i][s].min(upper);
                }
            }
            if super::timing_product::enabled() {
                let start = Instant::now();
                let was_viable: Vec<_> = uppers
                    .iter()
                    .map(|v| v.iter().any(|&u| u >= seed))
                    .collect();
                for s in 0..3 {
                    let product = super::timing_product::bounds(
                        &models[s], choices, cards, &indices, s, context,
                    );
                    let other = singles[(s + 1) % 3].saturating_add(singles[(s + 2) % 3]);
                    for &i in &indices {
                        // The forced card cannot also appear in either other
                        // song. Keep the complete unpriced pair certificate;
                        // never add its priced constant to this product.
                        let companion = pairs.get(s).map_or(other, |p| other.min(p.upper(&[i])));
                        let upper = product[i].saturating_add(companion);
                        round.forced_product.queries += 1;
                        round.forced_product.tighter += usize::from(upper < uppers[i][s]);
                        round.forced_product.new_song_rejections +=
                            usize::from(upper < seed && uppers[i][s] >= seed);
                        uppers[i][s] = uppers[i][s].min(upper);
                    }
                }
                round.forced_product.new_condition_rejections += indices
                    .iter()
                    .filter(|&&i| was_viable[i] && uppers[i].iter().all(|&u| u < seed))
                    .count();
                round.forced_product.seconds += start.elapsed().as_secs_f64();
            }
            for i in indices {
                if uppers[i].iter().all(|&u| u < seed) {
                    round.removed.push([cards[i].card_id, context as u32]);
                }
            }
        }
    }
    let by_id: BTreeMap<_, _> = cards
        .iter()
        .enumerate()
        .map(|(i, c)| (c.card_id, i))
        .collect();
    for &[id, context] in &round.removed {
        allowed[by_id[&id]][context as usize] = false;
    }
    round.after = allowed.iter().flatten().filter(|&&v| v).count();
    assert_eq!(round.before - round.after, round.removed.len());
    round
}

pub(super) fn rebuild(
    cards: &[PreparedCard],
    powers: &[BTreeMap<u32, MemberPower>],
    inputs: &[ReplayInput; 3],
    skills: &[ReplaySkill],
    cases: &[[usize; 4]],
    allowed: &[[bool; 4]],
    cap: ScoringPower,
    models: &mut Vec<PoolTimingUpper>,
    choices: &mut Vec<[Choice; 4]>,
    pairs: &mut [Pair],
    singles: &mut [u64; 3],
    bins: usize,
    net_bounds: bool,
    state_net_bounds: bool,
    frontier_net_bounds: bool,
    adjacent_net_bounds: bool,
    model_cache: &mut super::timing_model_cache::Models,
) -> Result<Stats, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let ids: BTreeSet<_> = cases
        .iter()
        .zip(allowed)
        .flat_map(|(row, allow)| (0..4).filter(|&c| allow[c]).map(|c| row[c]))
        .collect();
    let mut stats = Stats {
        skills_before: skills.len(),
        skills_after: ids.len(),
        memberships: allowed.iter().flatten().filter(|&&v| v).count(),
        single_before: *singles,
        pairs_before: pairs.iter().map(|p| p.upper(&[])).collect(),
        ..Stats::default()
    };
    if ids.is_empty() {
        // No surviving roster. Keep valid original structures for empty scans.
        stats.single_after = *singles;
        stats.pairs_after = stats.pairs_before.clone();
        return Ok(stats);
    }
    let catalogue: Vec<_> = ids.iter().map(|&i| skills[i].clone()).collect();
    let remap: BTreeMap<_, _> = ids
        .into_iter()
        .enumerate()
        .map(|(new, old)| (old, new))
        .collect();
    *models = inputs
        .iter()
        .enumerate()
        .map(|(song, i)| {
            if adjacent_net_bounds {
                model_cache.adjacent(song, &catalogue, cap)
            } else if frontier_net_bounds {
                PoolTimingUpper::new_frontier_net(
                    i,
                    &catalogue,
                    cap,
                    super::timing_pool::window_options(),
                )
            } else if state_net_bounds {
                PoolTimingUpper::new_stateful_net(i, &catalogue, cap)
            } else if net_bounds {
                PoolTimingUpper::new_net(i, &catalogue, cap)
            } else {
                PoolTimingUpper::new(i, &catalogue, cap)
            }
        })
        .collect::<Result<_, _>>()?;
    // Choices belong to their model. Never combine old and rebuilt gains, and
    // never manufacture an allowed row for a deleted condition membership.
    *choices = vec![[[PoolTimingChoice::default(); 3]; 4]; cards.len()];
    let mut retained = vec![];
    for (i, card) in cards.iter().enumerate() {
        if allowed[i].iter().any(|&a| a) {
            retained.push(i);
        }
        for c in 0..4 {
            if !allowed[i][c] {
                continue;
            }
            for s in 0..3 {
                choices[i][c][s] = models[s]
                    .choice(remap[&cases[i][c]], powers[s][&card.card_id].linear_total())?;
            }
        }
    }
    stats.model_seconds = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let oracles: Vec<_> = (0..3)
        .map(|s| {
            Oracle::conditional(
                &models[s],
                choices,
                cards,
                &retained,
                &powers[s],
                s,
                bins,
                Some(allowed),
            )
        })
        .collect();
    for s in 0..3 {
        singles[s] = singles[s].min(oracles[s].unpriced_upper(cards.len()));
    }
    // Re-evaluate saved prices, rather than rerun their optimization. Taking
    // minima keeps the old certificates whenever a new relaxation is looser.
    stats.price_sets = Pair::tighten_joint(pairs, &oracles);
    stats.oracle_seconds = start.elapsed().as_secs_f64();
    stats.single_after = *singles;
    stats.pairs_after = pairs.iter().map(|p| p.upper(&[])).collect();
    Ok(stats)
}

use bangdream_optimize_core::timing::Instant;
