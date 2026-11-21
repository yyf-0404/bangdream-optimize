use super::dominance;
use crate::medley::team;
use crate::model::dp::SongMode;
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::SelectedAreaItems;
use crate::scoring::MemberPower;
use crate::{Chart, DpModelError, TeamCardSkill};
use std::borrow::Cow;
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
    pub(crate) power: MemberPower,
    pub(crate) card_id: u32,
    pub(crate) character_id: u32,
    pub(crate) stat: f64,
    pub(crate) skill: TeamCardSkill,
}

impl ResolvedSingleCard {
    fn new(card: &PreparedCard, power: MemberPower, skill: TeamCardSkill) -> Self {
        Self {
            power,
            card_id: card.card_id,
            character_id: card.character_id,
            stat: power.linear_total(),
            skill,
        }
    }
}

pub(crate) fn resolve_card_indices(
    cards: &[PreparedCard],
    indices: &[usize],
    powers: &[MemberPower],
    mode: SongMode,
) -> Result<Vec<ResolvedSingleCard>, DpModelError> {
    indices
        .iter()
        .map(|&index| {
            let card = &cards[index];
            Ok(ResolvedSingleCard::new(
                card,
                powers[index],
                mode.resolve_skill(card)?,
            ))
        })
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
    let power = card.member_power(
        area_item_percent,
        &selected_items.band,
        &selected_items.attribute,
        selected_items.magazine.as_str(),
    );
    Ok(ResolvedSingleCard::new(card, power, skill))
}

pub(crate) fn pruned_card_indices(
    cards: &[PreparedCard],
    chart: &Chart,
    powers: &[MemberPower],
    mode: SongMode,
) -> Result<Vec<usize>, team::TeamBuildError> {
    dominance::contribution_pruned_card_indices(
        cards,
        chart,
        || Cow::Borrowed(powers),
        mode,
        None,
        None,
        false,
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
    point_bonus_fixed_score_equivalent: Option<f64>,
    preserve_stat: bool,
) -> Result<Vec<usize>, team::TeamBuildError> {
    // Filler deliberately bypasses every skill field. Cooperative captains use
    // `pruned_cooperative_captain_indices`, which supplies the four external skills.
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
        || Cow::Owned(team::adjusted_card_stats(cards, area_item_percent, selected_items)),
        mode,
        replacement_values,
        point_bonus_fixed_score_equivalent,
        preserve_stat,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn pruned_cooperative_captain_indices(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    teammate_skills: &[TeamCardSkill; 4],
    teammate_effective_stat: f64,
    replacement_values: Option<&[u64]>,
    point_bonus_fixed_score_equivalent: Option<f64>,
    preserve_stat: bool,
) -> Result<(Vec<usize>, crate::team_prune::MedleyPruneTrace), team::TeamBuildError> {
    dominance::contribution_pruned_captain_indices(
        cards,
        chart,
        area_item_percent,
        selected_items,
        mode,
        teammate_skills,
        teammate_effective_stat,
        replacement_values,
        point_bonus_fixed_score_equivalent,
        preserve_stat,
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
                card_stats[other_index].componentwise_dominates(card_stats[candidate_index])
                    && other_bonus >= candidate_bonus
                    && (card_stats[other_index].linear_total()
                        > card_stats[candidate_index].linear_total()
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
