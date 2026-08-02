use crate::medley::team;
use crate::model::dp::SongMode;
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::SelectedAreaItems;
use crate::{team_prune as prune, Chart, TeamCardSkill};
use std::collections::BTreeMap;

pub(super) fn contribution_pruned_card_indices(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    replacement_values: Option<&[u64]>,
    point_bonus_fixed_score_equivalent: Option<f64>,
) -> Result<Vec<usize>, team::TeamBuildError> {
    contribution_pruned_card_indices_impl(
        cards,
        chart,
        area_item_percent,
        selected_items,
        mode,
        None,
        replacement_values,
        point_bonus_fixed_score_equivalent,
    )
    .map(|(active, _)| active)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn contribution_pruned_captain_indices(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    teammate_skills: &[TeamCardSkill; 4],
    teammate_effective_stat: f64,
    replacement_values: Option<&[u64]>,
    point_bonus_fixed_score_equivalent: Option<f64>,
) -> Result<(Vec<usize>, prune::MedleyPruneTrace), team::TeamBuildError> {
    contribution_pruned_card_indices_impl(
        cards,
        chart,
        area_item_percent,
        selected_items,
        mode,
        Some((teammate_skills, teammate_effective_stat)),
        replacement_values,
        point_bonus_fixed_score_equivalent,
    )
}

#[allow(clippy::too_many_arguments)]
fn contribution_pruned_card_indices_impl(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    fixed_teammate_context: Option<(&[TeamCardSkill; 4], f64)>,
    replacement_values: Option<&[u64]>,
    point_bonus_fixed_score_equivalent: Option<f64>,
) -> Result<(Vec<usize>, prune::MedleyPruneTrace), team::TeamBuildError> {
    if !chart.warning.is_empty() {
        return Ok((
            cards
                .iter()
                .enumerate()
                .filter_map(|(index, card)| mode.allows(card).then_some(index))
                .collect(),
            prune::MedleyPruneTrace::default(),
        ));
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
    let bonus_bounds_start = crate::timing::Timer::start();
    let teammate_bonus_bounds = replacement_values
        .zip(point_bonus_fixed_score_equivalent)
        .and_then(|(values, _)| four_card_bonus_bounds(cards, mode, values));
    let point_bonus_bounds_ms = bonus_bounds_start.elapsed_ms();
    let adjusted_stats_start = crate::timing::Timer::start();
    let all_card_stats = team::adjusted_card_stats(cards, area_item_percent, selected_items);
    let adjusted_stats_ms = adjusted_stats_start.elapsed_ms();
    let same_shape_start = crate::timing::Timer::start();
    let hard_prefiltered_indices =
        same_shape_prefilter(cards, &all_card_stats, mode, replacement_values);
    let same_shape_prefilter_ms = same_shape_start.elapsed_ms();
    let profile_start = crate::timing::Timer::start();
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
    let profile_ms = profile_start.elapsed_ms();
    let (active, mut trace) =
        if let Some((teammate_skills, teammate_effective_stat)) = fixed_teammate_context {
            prune::single_team_active_card_indices_with_fixed_teammate_skills_and_trace(
                &hard_prefiltered_cards,
                chart,
                &hard_prefiltered_profiles,
                signature,
                teammate_skills,
                teammate_effective_stat,
                teammate_bonus_bounds,
                point_bonus_fixed_score_equivalent.unwrap_or_default(),
                hard_prefiltered_replacement_values.as_deref(),
            )
        } else if let (Some(card_bonus_micros), Some(teammate_bonus_bounds), Some(fixed)) = (
            hard_prefiltered_replacement_values.as_deref(),
            teammate_bonus_bounds,
            point_bonus_fixed_score_equivalent,
        ) {
            (
                prune::single_team_active_card_indices_with_joint_point_bonus(
                    &hard_prefiltered_cards,
                    chart,
                    &hard_prefiltered_profiles,
                    signature,
                    card_bonus_micros,
                    teammate_bonus_bounds,
                    fixed,
                ),
                prune::MedleyPruneTrace::default(),
            )
        } else {
            (
                prune::single_team_active_card_indices(
                    &hard_prefiltered_cards,
                    chart,
                    &hard_prefiltered_profiles,
                    signature,
                    hard_prefiltered_replacement_values.as_deref(),
                ),
                prune::MedleyPruneTrace::default(),
            )
        };
    let output_mapping_start = crate::timing::Timer::start();
    let active = active
        .into_iter()
        .map(|index| hard_prefiltered_indices[index])
        .collect();
    trace.point_bonus_bounds_ms += point_bonus_bounds_ms;
    trace.adjusted_stats_ms += adjusted_stats_ms;
    trace.same_shape_prefilter_ms += same_shape_prefilter_ms;
    trace.profile_ms += profile_ms;
    trace.output_mapping_ms += output_mapping_start.elapsed_ms();
    Ok((active, trace))
}

fn four_card_bonus_bounds(
    cards: &[PreparedCard],
    mode: SongMode,
    values: &[u64],
) -> Option<[u64; 2]> {
    if cards.len() != values.len() {
        return None;
    }
    let mut by_character = BTreeMap::<u32, (u64, u64)>::new();
    for (index, card) in cards.iter().enumerate() {
        if mode.allows(card) {
            let bonus = values[index];
            debug_assert_eq!(bonus % 100_000, 0);
            by_character
                .entry(card.character_id)
                .and_modify(|bounds| {
                    bounds.0 = bounds.0.min(bonus);
                    bounds.1 = bounds.1.max(bonus);
                })
                .or_insert((bonus, bonus));
        }
    }
    if by_character.len() < 4 {
        return None;
    }
    let mut minimums = by_character
        .values()
        .map(|bounds| bounds.0)
        .collect::<Vec<_>>();
    let mut maximums = by_character
        .values()
        .map(|bounds| bounds.1)
        .collect::<Vec<_>>();
    minimums.sort_unstable();
    maximums.sort_unstable_by(|left, right| right.cmp(left));
    Some([
        minimums.into_iter().take(4).fold(0, u64::saturating_add),
        maximums.into_iter().take(4).fold(0, u64::saturating_add),
    ])
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medley::test_support::prepared_card;
    use crate::Attribute;

    #[test]
    fn four_card_bonus_bounds_choose_distinct_characters() {
        let cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool),
            prepared_card(2, 1, 1, Attribute::Cool),
            prepared_card(3, 2, 1, Attribute::Cool),
            prepared_card(4, 3, 1, Attribute::Cool),
            prepared_card(5, 4, 1, Attribute::Cool),
        ];
        let values = [100_000, 1_000_000, 200_000, 300_000, 400_000];

        assert_eq!(
            four_card_bonus_bounds(&cards, SongMode::Mixed, &values),
            Some([1_000_000, 1_900_000])
        );
    }
}
