use super::dominance;
use crate::medley::team;
use crate::model::dp::SongMode;
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::SelectedAreaItems;
use crate::{Chart, DpModelError, TeamCardSkill};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SingleCardRole {
    /// Every card skill participates in the five normal activations and the
    /// selected captain can participate in the sixth activation.
    FullSkill,
    /// Only this card's leader skill participates in cooperative scoring.
    Captain,
    /// The card contributes only stat and event point bonus.
    Filler,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedSingleCard {
    pub(crate) card_id: u32,
    pub(crate) character_id: u32,
    pub(crate) stat: f64,
    pub(crate) skill: TeamCardSkill,
}

pub(crate) fn resolve_card_indices(
    cards: &[PreparedCard],
    indices: &[usize],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    role: SingleCardRole,
) -> Result<Vec<ResolvedSingleCard>, DpModelError> {
    indices
        .iter()
        .map(|&index| resolve_card(&cards[index], area_item_percent, selected_items, mode, role))
        .collect()
}

pub(crate) fn resolve_card(
    card: &PreparedCard,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    role: SingleCardRole,
) -> Result<ResolvedSingleCard, DpModelError> {
    let skill = if role == SingleCardRole::Filler {
        card.skill
    } else {
        mode.resolve_skill(card)?
    };
    Ok(ResolvedSingleCard {
        card_id: card.card_id,
        character_id: card.character_id,
        stat: card.add_up_stat(
            area_item_percent,
            &selected_items.band,
            &selected_items.attribute,
            selected_items.magazine.as_str(),
        ),
        skill,
    })
}

pub(crate) fn pruned_card_indices(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
) -> Result<Vec<usize>, team::TeamBuildError> {
    pruned_card_indices_for_role(
        cards,
        chart,
        area_item_percent,
        selected_items,
        mode,
        SingleCardRole::FullSkill,
        None,
    )
}

pub(crate) fn pruned_card_indices_for_role(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    role: SingleCardRole,
    replacement_values: Option<&[u64]>,
) -> Result<Vec<usize>, team::TeamBuildError> {
    // FullSkill and Captain share the existing contribution proof: it proves
    // replacement for every normal/captain physical role, so it is valid for
    // either subset of roles. Filler deliberately bypasses every skill field.
    if role == SingleCardRole::Filler {
        return Ok(filler_pruned_card_indices(
            cards,
            area_item_percent,
            selected_items,
            mode,
            replacement_values,
        ));
    }
    dominance::contribution_pruned_card_indices(
        cards,
        chart,
        area_item_percent,
        selected_items,
        mode,
        replacement_values,
    )
}

fn filler_pruned_card_indices(
    cards: &[PreparedCard],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    replacement_values: Option<&[u64]>,
) -> Vec<usize> {
    let card_stats = team::adjusted_card_stats(cards, area_item_percent, selected_items);
    let mut by_character = BTreeMap::<u32, Vec<usize>>::new();
    for (index, card) in cards.iter().enumerate() {
        if mode.allows(card) {
            by_character
                .entry(card.character_id)
                .or_default()
                .push(index);
        }
    }

    let mut result = Vec::new();
    for indices in by_character.values() {
        for &candidate_index in indices {
            let candidate_bonus = replacement_values
                .map(|values| values[candidate_index])
                .unwrap_or_default();
            let dominated = indices.iter().copied().any(|other_index| {
                if other_index == candidate_index {
                    return false;
                }
                let other_bonus = replacement_values
                    .map(|values| values[other_index])
                    .unwrap_or_default();
                card_stats[other_index] >= card_stats[candidate_index]
                    && other_bonus >= candidate_bonus
                    && (card_stats[other_index] > card_stats[candidate_index]
                        || other_bonus > candidate_bonus
                        || cards[other_index].card_id < cards[candidate_index].card_id)
            });
            if !dominated {
                result.push(candidate_index);
            }
        }
    }
    result
}
