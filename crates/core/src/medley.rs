pub mod candidate;
mod enumeration;
pub mod error;
mod prune;
mod scoring;
pub(crate) mod seed;
pub mod team;

use crate::model::dp::SongMode;
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::SelectedAreaItems;
use crate::Chart;

pub(crate) fn single_team_pruned_card_indices(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
) -> Result<Vec<usize>, team::TeamBuildError> {
    if !chart.warning.is_empty() {
        return Ok(cards
            .iter()
            .enumerate()
            .filter_map(|(idx, card)| mode.allows(card).then_some(idx))
            .collect());
    }
    let signature = match mode {
        SongMode::Mixed => prune::MedleyPruneSignature::Mixed,
        SongMode::UnifiedBand(band_id) => prune::MedleyPruneSignature::UnifiedBand(band_id),
        SongMode::UnifiedAttribute(attribute) => {
            prune::MedleyPruneSignature::UnifiedAttribute(attribute)
        }
        SongMode::UnifiedBandAttribute(band_id, attribute) => {
            prune::MedleyPruneSignature::UnifiedBandAttribute(band_id, attribute)
        }
    };
    let all_card_stats = team::adjusted_card_stats(cards, area_item_percent, selected_items);
    let hard_prefiltered_indices = single_same_shape_prefilter(cards, &all_card_stats, mode);
    let hard_prefiltered_cards = hard_prefiltered_indices
        .iter()
        .map(|&idx| cards[idx].clone())
        .collect::<Vec<_>>();
    let card_stats = hard_prefiltered_indices
        .iter()
        .map(|&idx| all_card_stats[idx])
        .collect::<Vec<_>>();
    let hard_prefiltered_profiles = prune::medley_card_prune_profiles(
        &hard_prefiltered_cards,
        std::slice::from_ref(chart),
        &card_stats,
    )?;
    Ok(prune::single_team_active_card_indices(
        &hard_prefiltered_cards,
        chart,
        &hard_prefiltered_profiles,
        signature,
    )
    .into_iter()
    .map(|idx| hard_prefiltered_indices[idx])
    .collect())
}

fn single_same_shape_prefilter(
    cards: &[PreparedCard],
    card_stats: &[f64],
    mode: SongMode,
) -> Vec<usize> {
    let (band_id, attribute) = match mode {
        SongMode::Mixed => (None, None),
        SongMode::UnifiedBand(band_id) => (Some(band_id), None),
        SongMode::UnifiedAttribute(attribute) => (None, Some(attribute)),
        SongMode::UnifiedBandAttribute(band_id, attribute) => (Some(band_id), Some(attribute)),
    };
    let allowed = cards
        .iter()
        .enumerate()
        .filter_map(|(idx, card)| mode.allows(card).then_some(idx))
        .collect::<Vec<_>>();

    allowed
        .iter()
        .copied()
        .filter(|&target_idx| {
            let target = &cards[target_idx];
            let target_score_up = target.score_up.resolve(band_id, attribute);
            !allowed.iter().copied().any(|dominator_idx| {
                if dominator_idx == target_idx {
                    return false;
                }
                let dominator = &cards[dominator_idx];
                if dominator.character_id != target.character_id
                    || dominator.skill.duration.to_bits() != target.skill.duration.to_bits()
                    || dominator.skill.rateup != target.skill.rateup
                    || card_stats[dominator_idx] < card_stats[target_idx]
                {
                    return false;
                }
                let dominator_score_up = dominator.score_up.resolve(band_id, attribute);
                dominator_score_up >= target_score_up
                    && (card_stats[dominator_idx] > card_stats[target_idx]
                        || dominator_score_up > target_score_up
                        || dominator.card_id < target.card_id)
            })
        })
        .collect()
}

#[cfg(test)]
mod test_support;
