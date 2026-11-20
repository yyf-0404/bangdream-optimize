//! Unpriced, card-forced product bound. Do not mix it into a priced oracle.
use super::timing_pool::Choice;
use bangdream_optimize_core::{hit_timing::bounds::PoolTimingUpper, PreparedCard};
use std::collections::BTreeMap;

pub(super) fn enabled() -> bool {
    mode() != 0
}

pub(super) fn mode() -> u8 {
    match std::env::var("BANDORI_TIMING_FORCED_PRODUCT").as_deref() {
        Ok("1") => 1,
        Ok("2") => 2,
        _ => 0,
    }
}

#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Stats {
    pub queries: usize,
    pub tighter: usize,
    pub new_song_rejections: usize,
    pub new_condition_rejections: usize,
    pub seconds: f64,
}

pub(super) fn bounds(
    model: &PoolTimingUpper,
    choices: &[[Choice; 4]],
    cards: &[PreparedCard],
    indices: &[usize],
    song: usize,
    context: usize,
) -> Vec<u64> {
    let mut by_character: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for &i in indices {
        by_character
            .entry(cards[i].character_id)
            .or_default()
            .push(i);
    }
    let groups: Vec<_> = by_character
        .values()
        .map(|ids| ids.iter().map(|&i| choices[i][context][song]).collect())
        .collect();
    let bounds = if mode() == 2 {
        model.forced_assignment_product_uppers(&groups)
    } else {
        model.forced_product_uppers(&groups)
    }
    .expect("conditional choices must belong to this model and fit its power cap");
    let mut result = vec![u64::MAX; cards.len()];
    for (ids, values) in by_character.values().zip(bounds) {
        for (&i, value) in ids.iter().zip(values) {
            result[i] = value;
        }
    }
    result
}
