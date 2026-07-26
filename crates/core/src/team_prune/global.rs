use super::contribution::{
    full_medley_score_contribution_cover, same_character_score_contribution_cover,
    MedleyContributionDominance,
};
use super::hard::{
    full_medley_dominator_cover, full_medley_dominator_cover_ignoring_unification,
    rejection_counts, same_character_dominator_cover, MedleyCardPruneProfile, MedleyPruneContext,
};
use super::stats::MedleyCardPruneStats;
use crate::model::chart::Chart;
use crate::model::preparation::PreparedCard;
use std::collections::BTreeMap;

const MEDLEY_TEAM_COUNT: usize = 3;

pub(crate) fn global_prune_stats(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    current_best: i32,
) -> MedleyCardPruneStats {
    let context = MedleyPruneContext::new(cards, charts, profiles, current_best);
    let mut contribution_dominance =
        MedleyContributionDominance::new(cards, charts, profiles, current_best);
    let mut stats = MedleyCardPruneStats {
        raw_count: cards.len(),
        character_count: character_count(cards),
        characters_with_four_or_more_cards: characters_with_at_least_cards(cards, 4),
        unified_band_count: context.unified_bands.len(),
        unified_attribute_count: context.unified_attributes.len(),
        unified_band_attribute_count: context.unified_band_attributes.len(),
        obligation_count: context.obligations.iter().map(Vec::len).sum(),
        max_obligations_per_card: context
            .obligations
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or_default(),
        ..MedleyCardPruneStats::default()
    };

    for (idx, card) in cards.iter().enumerate() {
        if context.obligations[idx].is_empty() {
            stats.upper_bound_context_pruned += 1;
            continue;
        }

        let same_cover = same_character_dominator_cover(idx, card, cards, profiles, &context);
        stats.max_same_character_cover = stats.max_same_character_cover.max(same_cover);
        if same_cover >= MEDLEY_TEAM_COUNT {
            stats.same_character_pruned += 1;
            continue;
        }

        let cross_cover = full_medley_dominator_cover(idx, card, cards, profiles, &context);
        stats.max_cross_character_cover = stats.max_cross_character_cover.max(cross_cover);
        if cross_cover > 0 {
            stats.cross_character_pruned += 1;
            continue;
        }

        let contribution_same_cover = same_character_score_contribution_cover(
            idx,
            card,
            cards,
            &context,
            &mut contribution_dominance,
        );
        stats.max_score_contribution_same_cover = stats
            .max_score_contribution_same_cover
            .max(contribution_same_cover);
        if contribution_same_cover >= MEDLEY_TEAM_COUNT {
            stats.score_contribution_same_pruned += 1;
            continue;
        }

        let contribution_cross_cover = full_medley_score_contribution_cover(
            idx,
            card,
            cards,
            &context,
            &mut contribution_dominance,
        );
        stats.max_score_contribution_cross_cover = stats
            .max_score_contribution_cross_cover
            .max(contribution_cross_cover);
        if contribution_cross_cover > 0 {
            stats.score_contribution_cross_pruned += 1;
            continue;
        }

        let cross_cover_ignoring_unification =
            full_medley_dominator_cover_ignoring_unification(idx, card, cards, profiles);
        stats.max_cross_character_cover_ignoring_unification = stats
            .max_cross_character_cover_ignoring_unification
            .max(cross_cover_ignoring_unification);
        if cross_cover_ignoring_unification > 0 {
            stats.cross_prunable_ignoring_unification += 1;
        }

        stats.active_count += 1;
    }

    stats.rejection_counts = rejection_counts(cards, profiles, &context);
    stats
}

fn character_count(cards: &[PreparedCard]) -> usize {
    let mut characters = Vec::new();
    for card in cards {
        if !characters.contains(&card.character_id) {
            characters.push(card.character_id);
        }
    }

    characters.len()
}

fn characters_with_at_least_cards(cards: &[PreparedCard], threshold: usize) -> usize {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for card in cards {
        *counts.entry(card.character_id).or_default() += 1;
    }

    counts.values().filter(|&&count| count >= threshold).count()
}
