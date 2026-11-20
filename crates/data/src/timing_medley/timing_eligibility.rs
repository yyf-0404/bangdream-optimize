//! Whole-domain song eligibility snapshots for replacement occupancy proofs.
//! The snapshot is immutable throughout a replacement pass. In particular,
//! later conditions cannot mistake a locally unusable card for globally absent.
use super::{
    timing_pair::{Oracle, Pair},
    timing_pool::Choice,
};
use bangdream_optimize_core::{
    hit_timing::bounds::PoolTimingUpper, scoring::MemberPower, PreparedCard,
};
use std::{collections::BTreeMap};

#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Stats {
    pub seconds: f64,
    pub by_mask: [usize; 8],
    /// [card ID, song mask, forced three-song upper for song 0, 1, 2].
    pub certificates: Vec<[u64; 5]>,
}

pub(super) fn build(
    cards: &[PreparedCard],
    retained: &[usize],
    powers: &[BTreeMap<u32, MemberPower>],
    choices: &[[Choice; 4]],
    models: &[PoolTimingUpper],
    pairs: &[Pair],
    other_upper: [u64; 3],
    seed: u64,
    allowed: Option<&[[bool; 4]]>,
) -> (Vec<u8>, Stats) {
    let start = Instant::now();
    // Oracle::conditional maximizes over ALL remaining condition partitions.
    // Do not replace this with fixed_context, even for a unified target pool.
    let oracles: Vec<_> = (0..3)
        .map(|s| {
            Oracle::conditional(
                &models[s], choices, cards, retained, &powers[s], s, 0, allowed,
            )
        })
        .collect();
    let (masks, uppers) = certify(&oracles, pairs, cards.len(), other_upper, seed);
    let mut stats = Stats::default();
    for &i in retained {
        stats.by_mask[masks[i] as usize] += 1;
        stats.certificates.push([
            u64::from(cards[i].card_id),
            u64::from(masks[i]),
            uppers[i][0],
            uppers[i][1],
            uppers[i][2],
        ]);
    }
    stats.seconds = start.elapsed().as_secs_f64();
    (masks, stats)
}

pub(super) fn certify(
    oracles: &[Oracle],
    pairs: &[Pair],
    count: usize,
    other_upper: [u64; 3],
    seed: u64,
) -> (Vec<u8>, Vec<[u64; 3]>) {
    let mut uppers = if pairs.is_empty() {
        vec![[u64::MAX; 3]; count]
    } else {
        Pair::card_uppers(pairs, oracles, count)
    };
    let mut masks = vec![0; count];
    for s in 0..3 {
        let forced = oracles[s].forced(&vec![0.; count]);
        for i in 0..count {
            if !forced[i].is_finite() {
                uppers[i][s] = 0;
                continue;
            }
            let combined = (forced[i].max(0.).floor() as u64).saturating_add(other_upper[s]);
            uppers[i][s] = uppers[i][s].min(combined);
            // Equality remains eligible: this search retains seed ties.
            if uppers[i][s] >= seed {
                masks[i] |= 1 << s;
            }
        }
    }
    (masks, uppers)
}

use bangdream_optimize_core::timing::Instant;
