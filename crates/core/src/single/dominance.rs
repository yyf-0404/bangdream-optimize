use crate::medley::team;
use crate::model::dp::SongMode;
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::SelectedAreaItems;
use crate::{team_prune as prune, Chart};

pub(super) fn contribution_pruned_card_indices(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    replacement_values: Option<&[u64]>,
) -> Result<Vec<usize>, team::TeamBuildError> {
    if !chart.warning.is_empty() {
        return Ok(cards
            .iter()
            .enumerate()
            .filter_map(|(index, card)| mode.allows(card).then_some(index))
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
    let hard_prefiltered_indices =
        same_shape_prefilter(cards, &all_card_stats, mode, replacement_values);
    let hard_prefiltered_cards = hard_prefiltered_indices
        .iter()
        .map(|&index| cards[index].clone())
        .collect::<Vec<_>>();
    let card_stats = hard_prefiltered_indices
        .iter()
        .map(|&index| all_card_stats[index])
        .collect::<Vec<_>>();
    let hard_prefiltered_replacement_values = replacement_values.map(|values| {
        hard_prefiltered_indices
            .iter()
            .map(|&index| values[index])
            .collect::<Vec<_>>()
    });
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
        hard_prefiltered_replacement_values.as_deref(),
    )
    .into_iter()
    .map(|index| hard_prefiltered_indices[index])
    .collect())
}

fn same_shape_prefilter(
    cards: &[PreparedCard],
    card_stats: &[f64],
    mode: SongMode,
    replacement_values: Option<&[u64]>,
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
        .filter_map(|(index, card)| mode.allows(card).then_some(index))
        .collect::<Vec<_>>();

    allowed
        .iter()
        .copied()
        .filter(|&target_index| {
            let target = &cards[target_index];
            let target_score_up = target.score_up.resolve(band_id, attribute);
            !allowed.iter().copied().any(|dominator_index| {
                if dominator_index == target_index {
                    return false;
                }
                if replacement_values
                    .is_some_and(|values| values[dominator_index] < values[target_index])
                {
                    return false;
                }
                let dominator = &cards[dominator_index];
                if dominator.character_id != target.character_id
                    || dominator.skill.duration.to_bits() != target.skill.duration.to_bits()
                    || dominator.skill.rateup != target.skill.rateup
                    || card_stats[dominator_index] < card_stats[target_index]
                {
                    return false;
                }
                let dominator_score_up = dominator.score_up.resolve(band_id, attribute);
                dominator_score_up >= target_score_up
                    && (card_stats[dominator_index] > card_stats[target_index]
                        || dominator_score_up > target_score_up
                        || dominator.card_id < target.card_id)
            })
        })
        .collect()
}
