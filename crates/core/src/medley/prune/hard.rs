use super::signature::{
    signature_can_complete_with_card, signature_improves_any_skill, MedleyPruneSignature,
};
use super::stats::DominanceRejectionCounts;
use crate::medley::team::TeamBuildError;
use crate::model::chart::{Chart, TeamCardSkill};
use crate::model::preparation::{PreparedCard, ScoreUp};
use crate::model::schema::Attribute;
use bangdream_optimize_team_prune::{
    cross_group_dominator_cover, dominance_graph_for_index_subset,
    dominator_cover_after_worst_teammate_groups, same_group_dominator_cover, DominanceGraph,
};
use std::collections::BTreeMap;

const TEAM_SIZE: usize = 5;
const MEDLEY_TEAM_COUNT: usize = 3;

#[derive(Debug, Clone)]
pub(in crate::medley) struct MedleyCardPruneProfile {
    pub(in crate::medley) stat: f64,
    skill_meta_variants: Vec<SkillMetaVariant>,
    pub(in crate::medley) skill_meta_bounds: Vec<SkillMetaBounds>,
    pub(in crate::medley) best_skill_meta_by_chart: Vec<f64>,
}

impl MedleyCardPruneProfile {
    pub(in crate::medley) fn skill_meta_for_score_up(&self, score_up: f64) -> Option<&[f64]> {
        let score_up_bits = score_up.to_bits();
        self.skill_meta_variants
            .iter()
            .find(|variant| variant.score_up_bits == score_up_bits)
            .or_else(|| self.skill_meta_variants.first())
            .map(|variant| variant.values.as_slice())
    }
}

#[derive(Debug, Clone)]
struct SkillMetaVariant {
    score_up_bits: u64,
    values: Vec<f64>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::medley) struct SkillMetaBounds {
    pub(in crate::medley) low: f64,
    pub(in crate::medley) high: f64,
}

#[derive(Debug, Clone, Copy)]
struct SignatureHardDominanceModel<'a> {
    stat: f64,
    card_id: u32,
    meta: &'a [f64],
}

#[derive(Debug, Clone)]
pub(in crate::medley) struct MedleyPruneContext {
    pub(in crate::medley) unified_bands: Vec<u32>,
    pub(in crate::medley) unified_attributes: Vec<Attribute>,
    pub(in crate::medley) unified_band_attributes: Vec<(u32, Attribute)>,
    pub(in crate::medley) obligations: Vec<Vec<MedleyPruneSignature>>,
}

impl MedleyPruneContext {
    pub(in crate::medley) fn new(
        cards: &[PreparedCard],
        charts: &[Chart],
        profiles: &[MedleyCardPruneProfile],
        current_best: i32,
    ) -> Self {
        let mut band_characters: Vec<(u32, Vec<u32>)> = Vec::new();
        let mut attribute_characters: Vec<(Attribute, Vec<u32>)> = Vec::new();
        let mut band_attribute_characters: Vec<((u32, Attribute), Vec<u32>)> = Vec::new();

        for card in cards {
            push_distinct_character_by_key(&mut band_characters, card.band_id, card.character_id);
            push_distinct_character_by_key(
                &mut attribute_characters,
                card.attribute,
                card.character_id,
            );
            push_distinct_character_by_key(
                &mut band_attribute_characters,
                (card.band_id, card.attribute),
                card.character_id,
            );
        }

        let unified_bands = possible_unified_keys(band_characters);
        let unified_attributes = possible_unified_keys(attribute_characters);
        let unified_band_attributes = possible_unified_keys(band_attribute_characters);
        let mut upper_bounds = MedleyPruneUpperBounds::new(cards, charts, profiles);
        let obligations = cards
            .iter()
            .enumerate()
            .map(|(idx, card)| {
                card_prune_obligations(idx, card, cards, charts, current_best, &mut upper_bounds)
            })
            .collect();

        Self {
            unified_bands,
            unified_attributes,
            unified_band_attributes,
            obligations,
        }
    }
}

pub(in crate::medley) struct MedleyPruneUpperBounds<'a> {
    cards: &'a [PreparedCard],
    charts: &'a [Chart],
    profiles: &'a [MedleyCardPruneProfile],
    best_any_team_scores: Vec<f64>,
    teammate_cache: Vec<TeammateUpperBoundCacheEntry>,
}

#[derive(Debug, Clone)]
struct TeammateUpperBoundCacheEntry {
    signature: MedleyPruneSignature,
    excluded_character_id: u32,
    stat: f64,
    normal_skill_meta_by_chart: Vec<f64>,
    captain_skill_meta_by_chart: Vec<f64>,
}

impl<'a> MedleyPruneUpperBounds<'a> {
    pub(in crate::medley) fn new(
        cards: &'a [PreparedCard],
        charts: &'a [Chart],
        profiles: &'a [MedleyCardPruneProfile],
    ) -> Self {
        let best_any_team_scores = (0..charts.len())
            .map(|chart_idx| best_any_team_score_upper_bound(cards, charts, profiles, chart_idx))
            .collect();

        Self {
            cards,
            charts,
            profiles,
            best_any_team_scores,
            teammate_cache: Vec::new(),
        }
    }

    pub(in crate::medley) fn signature_can_beat_incumbent(
        &mut self,
        card_idx: usize,
        signature: MedleyPruneSignature,
        current_best: i32,
    ) -> bool {
        for song_idx in 0..self.charts.len() {
            let forced = self.forced_team_score_upper_bound(card_idx, signature, song_idx);
            let other_songs = self
                .best_any_team_scores
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != song_idx)
                .map(|(_, &score)| score)
                .sum::<f64>();
            if forced + other_songs > current_best as f64 {
                return true;
            }
        }

        false
    }

    fn forced_team_score_upper_bound(
        &mut self,
        card_idx: usize,
        signature: MedleyPruneSignature,
        chart_idx: usize,
    ) -> f64 {
        let card = &self.cards[card_idx];
        let profile = &self.profiles[card_idx];
        let teammate = self.teammate_upper_bound(signature, card.character_id);
        let score_up = card
            .score_up
            .resolve(signature.team_band_id(), signature.team_attribute());
        let card_meta = profile
            .skill_meta_for_score_up(score_up)
            .map(|values| {
                let start = chart_idx * (TEAM_SIZE + 1);
                values[start..start + TEAM_SIZE + 1]
                    .iter()
                    .copied()
                    .fold(0.0, f64::max)
            })
            .unwrap_or_default();

        let stat = profile.stat + teammate.stat;
        let skill_meta = card_meta
            + teammate.normal_skill_meta_by_chart[chart_idx]
            + card_meta.max(teammate.captain_skill_meta_by_chart[chart_idx]);
        (stat * (self.charts[chart_idx].meta.no_skill + skill_meta)).floor()
    }

    fn teammate_upper_bound(
        &mut self,
        signature: MedleyPruneSignature,
        excluded_character_id: u32,
    ) -> TeammateUpperBoundCacheEntry {
        if let Some(entry) = self.teammate_cache.iter().find(|entry| {
            entry.signature == signature && entry.excluded_character_id == excluded_character_id
        }) {
            return entry.clone();
        }

        let stat = top_character_values_sum::<4, _>(self.cards, |idx, card| {
            (card.character_id != excluded_character_id && signature.allows(card))
                .then_some(self.profiles[idx].stat)
        });
        let normal_skill_meta_by_chart = (0..self.charts.len())
            .map(|chart_idx| {
                top_character_values_sum::<4, _>(self.cards, |idx, card| {
                    (card.character_id != excluded_character_id && signature.allows(card)).then(
                        || {
                            signature_skill_meta_for_chart(
                                card,
                                &self.profiles[idx],
                                signature,
                                chart_idx,
                            )
                        },
                    )
                })
            })
            .collect();
        let captain_skill_meta_by_chart = (0..self.charts.len())
            .map(|chart_idx| {
                top_character_values_sum::<1, _>(self.cards, |idx, card| {
                    (card.character_id != excluded_character_id && signature.allows(card)).then(
                        || {
                            signature_skill_meta_for_chart(
                                card,
                                &self.profiles[idx],
                                signature,
                                chart_idx,
                            )
                        },
                    )
                })
            })
            .collect();

        let entry = TeammateUpperBoundCacheEntry {
            signature,
            excluded_character_id,
            stat,
            normal_skill_meta_by_chart,
            captain_skill_meta_by_chart,
        };
        self.teammate_cache.push(entry.clone());
        entry
    }
}

pub(in crate::medley) fn rejection_counts(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    context: &MedleyPruneContext,
) -> DominanceRejectionCounts {
    let mut counts = DominanceRejectionCounts::default();
    for (right_idx, right) in cards.iter().enumerate() {
        for (left_idx, left) in cards.iter().enumerate() {
            if left_idx == right_idx {
                continue;
            }
            for &signature in &context.obligations[right_idx] {
                match dominance_rejection_for_signature(
                    left,
                    &profiles[left_idx],
                    right,
                    &profiles[right_idx],
                    signature,
                ) {
                    DominanceCheck::Dominates => {}
                    DominanceCheck::RejectStat => counts.stat += 1,
                    DominanceCheck::RejectUnification => counts.unification += 1,
                    DominanceCheck::RejectMeta => counts.meta += 1,
                    DominanceCheck::RejectTie => counts.tie += 1,
                }
            }
        }
    }

    counts
}

pub(in crate::medley) fn best_any_team_score_upper_bound(
    cards: &[PreparedCard],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    chart_idx: usize,
) -> f64 {
    let stat = top_character_values_sum::<5, _>(cards, |idx, _| Some(profiles[idx].stat));
    let normal_skill_meta = top_character_values_sum::<5, _>(cards, |idx, _| {
        Some(profiles[idx].best_skill_meta_by_chart[chart_idx])
    });
    let captain_skill_meta = top_character_values_sum::<1, _>(cards, |idx, _| {
        Some(profiles[idx].best_skill_meta_by_chart[chart_idx])
    });
    let skill_meta = normal_skill_meta + captain_skill_meta;
    (stat * (charts[chart_idx].meta.no_skill + skill_meta)).floor()
}

pub(in crate::medley) fn signature_skill_meta_for_chart(
    card: &PreparedCard,
    profile: &MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
    chart_idx: usize,
) -> f64 {
    let score_up = card
        .score_up
        .resolve(signature.team_band_id(), signature.team_attribute());
    profile
        .skill_meta_for_score_up(score_up)
        .map(|values| {
            let start = chart_idx * (TEAM_SIZE + 1);
            values[start..start + TEAM_SIZE + 1]
                .iter()
                .copied()
                .fold(0.0, f64::max)
        })
        .unwrap_or_default()
}

pub(in crate::medley) fn top_character_values_sum<const N: usize, F>(
    cards: &[PreparedCard],
    mut value: F,
) -> f64
where
    F: FnMut(usize, &PreparedCard) -> Option<f64>,
{
    let mut character_values: Vec<(u32, f64)> = Vec::new();
    for (idx, card) in cards.iter().enumerate() {
        let Some(value) = value(idx, card) else {
            continue;
        };
        if let Some((_, existing)) = character_values
            .iter_mut()
            .find(|(character_id, _)| *character_id == card.character_id)
        {
            *existing = existing.max(value);
        } else {
            character_values.push((card.character_id, value));
        }
    }

    let mut top = TopValues::<N>::default();
    for (_, value) in character_values {
        top.push(value);
    }
    top.sum()
}

pub(in crate::medley) fn medley_card_prune_profiles(
    cards: &[PreparedCard],
    charts: &[Chart],
    card_stats: &[f64],
) -> Result<Vec<MedleyCardPruneProfile>, TeamBuildError> {
    cards
        .iter()
        .zip(card_stats)
        .map(|(card, &stat)| {
            let skill_meta_variants = medley_skill_meta_variants(card, charts)?;
            let skill_meta_bounds = skill_meta_bounds(&skill_meta_variants);
            let best_skill_meta_by_chart =
                best_skill_meta_by_chart(&skill_meta_bounds, charts.len());
            Ok(MedleyCardPruneProfile {
                stat,
                skill_meta_variants,
                skill_meta_bounds,
                best_skill_meta_by_chart,
            })
        })
        .collect()
}

pub(in crate::medley) fn medley_card_dominates_for_signature(
    left: &PreparedCard,
    left_profile: &MedleyCardPruneProfile,
    right: &PreparedCard,
    right_profile: &MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
) -> bool {
    dominance_rejection_for_signature(left, left_profile, right, right_profile, signature)
        == DominanceCheck::Dominates
}

pub(in crate::medley) fn medley_card_dominates_ignoring_unification(
    left: &PreparedCard,
    left_profile: &MedleyCardPruneProfile,
    right: &PreparedCard,
    right_profile: &MedleyCardPruneProfile,
) -> bool {
    if left_profile.stat < right_profile.stat {
        return false;
    }
    if left_profile.skill_meta_bounds.len() != right_profile.skill_meta_bounds.len() {
        return false;
    }

    let mut strictly_better = left_profile.stat > right_profile.stat;
    for (left_meta, right_meta) in left_profile
        .skill_meta_bounds
        .iter()
        .zip(&right_profile.skill_meta_bounds)
    {
        if left_meta.low < right_meta.high {
            return false;
        }
        strictly_better |= left_meta.low > right_meta.high;
    }

    strictly_better || left.card_id < right.card_id
}

pub(in crate::medley) fn same_character_cover(
    graph: &DominanceGraph,
    target_idx: usize,
    cards: &[PreparedCard],
) -> usize {
    same_group_dominator_cover(
        graph,
        target_idx,
        cards,
        |card| card.character_id,
        MEDLEY_TEAM_COUNT,
    )
}

pub(in crate::medley) fn cross_cover(
    graph: &DominanceGraph,
    target_idx: usize,
    cards: &[PreparedCard],
) -> usize {
    cross_group_dominator_cover(
        graph,
        target_idx,
        cards,
        |card| card.character_id,
        TEAM_SIZE,
        MEDLEY_TEAM_COUNT,
    )
}

pub(in crate::medley) fn hard_dominance_graph_for_indices(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
    allowed_indices: &[usize],
) -> DominanceGraph {
    let mut models = vec![None; cards.len()];
    for &idx in allowed_indices {
        let score_up = cards[idx]
            .score_up
            .resolve(signature.team_band_id(), signature.team_attribute());
        if let Some(meta) = profiles[idx].skill_meta_for_score_up(score_up) {
            models[idx] = Some(SignatureHardDominanceModel {
                stat: profiles[idx].stat,
                card_id: cards[idx].card_id,
                meta,
            });
        }
    }

    dominance_graph_for_index_subset(cards.len(), allowed_indices, |dominator_idx, target_idx| {
        let Some(left) = models[dominator_idx] else {
            return false;
        };
        let Some(right) = models[target_idx] else {
            return false;
        };
        signature_hard_model_dominates(left, right)
    })
}

fn signature_hard_model_dominates(
    left: SignatureHardDominanceModel<'_>,
    right: SignatureHardDominanceModel<'_>,
) -> bool {
    if left.stat < right.stat || left.meta.len() != right.meta.len() {
        return false;
    }

    let mut strictly_better = left.stat > right.stat;
    for (&left_value, &right_value) in left.meta.iter().zip(right.meta) {
        if left_value < right_value {
            return false;
        }
        strictly_better |= left_value > right_value;
    }

    strictly_better || left.card_id < right.card_id
}

pub(in crate::medley) fn same_character_dominator_cover(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    context: &MedleyPruneContext,
) -> usize {
    context.obligations[idx]
        .iter()
        .map(|&signature| {
            same_character_dominator_cover_for_signature(idx, card, cards, profiles, signature)
        })
        .min()
        .unwrap_or_default()
}

pub(in crate::medley) fn full_medley_dominator_cover(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    context: &MedleyPruneContext,
) -> usize {
    context.obligations[idx]
        .iter()
        .map(|&signature| {
            full_medley_dominator_cover_for_signature(idx, card, cards, profiles, signature)
        })
        .min()
        .unwrap_or_default()
}

pub(in crate::medley) fn full_medley_dominator_cover_ignoring_unification(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
) -> usize {
    let mut counts_by_character: BTreeMap<u32, usize> = BTreeMap::new();

    for (other_idx, other) in cards.iter().enumerate() {
        if other_idx == idx
            || !medley_card_dominates_ignoring_unification(
                other,
                &profiles[other_idx],
                card,
                &profiles[idx],
            )
        {
            continue;
        }

        *counts_by_character.entry(other.character_id).or_default() += 1;
    }

    dominator_cover_after_worst_teammate_characters(counts_by_character, card.character_id)
}

fn same_character_dominator_cover_for_signature(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
) -> usize {
    cards
        .iter()
        .enumerate()
        .filter(|&(other_idx, other)| {
            other_idx != idx
                && other.character_id == card.character_id
                && medley_card_dominates_for_signature(
                    other,
                    &profiles[other_idx],
                    card,
                    &profiles[idx],
                    signature,
                )
        })
        .take(MEDLEY_TEAM_COUNT)
        .count()
}

fn full_medley_dominator_cover_for_signature(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
) -> usize {
    let mut counts_by_character: BTreeMap<u32, usize> = BTreeMap::new();

    for (other_idx, other) in cards.iter().enumerate() {
        if other_idx == idx
            || !medley_card_dominates_for_signature(
                other,
                &profiles[other_idx],
                card,
                &profiles[idx],
                signature,
            )
        {
            continue;
        }

        *counts_by_character.entry(other.character_id).or_default() += 1;
    }

    dominator_cover_after_worst_teammate_characters(counts_by_character, card.character_id)
}

fn dominator_cover_after_worst_teammate_characters(
    counts_by_character: BTreeMap<u32, usize>,
    target_character_id: u32,
) -> usize {
    dominator_cover_after_worst_teammate_groups(
        counts_by_character,
        target_character_id,
        TEAM_SIZE,
        MEDLEY_TEAM_COUNT,
    )
}

fn card_prune_obligations(
    idx: usize,
    card: &PreparedCard,
    cards: &[PreparedCard],
    charts: &[Chart],
    current_best: i32,
    upper_bounds: &mut MedleyPruneUpperBounds<'_>,
) -> Vec<MedleyPruneSignature> {
    let signatures = [
        MedleyPruneSignature::Mixed,
        MedleyPruneSignature::UnifiedBand(card.band_id),
        MedleyPruneSignature::UnifiedAttribute(card.attribute),
        MedleyPruneSignature::UnifiedBandAttribute(card.band_id, card.attribute),
    ];

    let obligations: Vec<_> = signatures
        .into_iter()
        .filter(|&signature| signature_can_complete_with_card(cards, idx, signature))
        .filter(|&signature| {
            signature == MedleyPruneSignature::Mixed
                || signature_improves_any_skill(cards, signature)
        })
        .filter(|&signature| {
            current_best <= 0
                || charts.len() != MEDLEY_TEAM_COUNT
                || upper_bounds.signature_can_beat_incumbent(idx, signature, current_best)
        })
        .collect();

    if obligations.is_empty() && current_best <= 0 {
        return signatures.to_vec();
    }

    obligations
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DominanceCheck {
    Dominates,
    RejectStat,
    RejectUnification,
    RejectMeta,
    RejectTie,
}

fn medley_skill_meta_variants(
    card: &PreparedCard,
    charts: &[Chart],
) -> Result<Vec<SkillMetaVariant>, TeamBuildError> {
    let score_ups = score_up_bounds(card.score_up);
    let mut result = Vec::with_capacity(score_ups.len());

    for score_up in score_ups.iter().copied() {
        let mut values = Vec::with_capacity(charts.len() * (TEAM_SIZE + 1));
        for chart in charts {
            for activation in 0..=TEAM_SIZE {
                let value = chart.skill_meta_value(
                    activation,
                    TeamCardSkill {
                        card_id: card.card_id,
                        duration: card.skill.duration,
                        score_up,
                        rateup: card.skill.rateup,
                    },
                )?;
                values.push(value);
            }
        }
        result.push(SkillMetaVariant {
            score_up_bits: score_up.to_bits(),
            values,
        });
    }

    Ok(result)
}

fn skill_meta_bounds(variants: &[SkillMetaVariant]) -> Vec<SkillMetaBounds> {
    let Some(first) = variants.first() else {
        return Vec::new();
    };
    let mut result = Vec::with_capacity(first.values.len());
    for idx in 0..first.values.len() {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for variant in variants {
            let value = variant.values[idx];
            low = low.min(value);
            high = high.max(value);
        }
        result.push(SkillMetaBounds { low, high });
    }
    result
}

fn best_skill_meta_by_chart(bounds: &[SkillMetaBounds], chart_count: usize) -> Vec<f64> {
    (0..chart_count)
        .map(|chart_idx| {
            let start = chart_idx * (TEAM_SIZE + 1);
            let end = start + TEAM_SIZE + 1;
            bounds[start..end]
                .iter()
                .map(|bound| bound.high)
                .fold(0.0, f64::max)
        })
        .collect()
}

fn score_up_bounds(score_up: ScoreUp) -> Vec<f64> {
    let mut values = vec![score_up.default];
    if let Some(unified_value) = score_up.unification_activate_effect_value {
        if unified_value != score_up.default {
            values.push(unified_value);
        }
    }
    values
}

fn dominance_rejection_for_signature(
    left: &PreparedCard,
    left_profile: &MedleyCardPruneProfile,
    right: &PreparedCard,
    right_profile: &MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
) -> DominanceCheck {
    if left_profile.stat < right_profile.stat {
        return DominanceCheck::RejectStat;
    }
    if !signature.allows(left) || !signature.allows(right) {
        return DominanceCheck::RejectUnification;
    }

    let left_score_up = left
        .score_up
        .resolve(signature.team_band_id(), signature.team_attribute());
    let right_score_up = right
        .score_up
        .resolve(signature.team_band_id(), signature.team_attribute());
    let Some(left_meta) = left_profile.skill_meta_for_score_up(left_score_up) else {
        return DominanceCheck::RejectMeta;
    };
    let Some(right_meta) = right_profile.skill_meta_for_score_up(right_score_up) else {
        return DominanceCheck::RejectMeta;
    };
    if left_meta.len() != right_meta.len() {
        return DominanceCheck::RejectMeta;
    }

    let mut strictly_better = left_profile.stat > right_profile.stat;
    for (&left_value, &right_value) in left_meta.iter().zip(right_meta) {
        if left_value < right_value {
            return DominanceCheck::RejectMeta;
        }
        strictly_better |= left_value > right_value;
    }

    if strictly_better || left.card_id < right.card_id {
        DominanceCheck::Dominates
    } else {
        DominanceCheck::RejectTie
    }
}

#[derive(Debug, Clone, Copy)]
struct TopValues<const N: usize> {
    values: [f64; N],
}

impl<const N: usize> Default for TopValues<N> {
    fn default() -> Self {
        Self { values: [0.0; N] }
    }
}

impl<const N: usize> TopValues<N> {
    fn push(&mut self, value: f64) {
        if value <= self.values[N - 1] {
            return;
        }

        self.values[N - 1] = value;
        let mut idx = N - 1;
        while idx > 0 && self.values[idx] > self.values[idx - 1] {
            self.values.swap(idx, idx - 1);
            idx -= 1;
        }
    }

    fn sum(&self) -> f64 {
        self.values.iter().sum()
    }
}

fn push_distinct_character_by_key<K: PartialEq>(
    entries: &mut Vec<(K, Vec<u32>)>,
    key: K,
    character_id: u32,
) {
    if let Some((_, characters)) = entries
        .iter_mut()
        .find(|(existing_key, _)| *existing_key == key)
    {
        if !characters.contains(&character_id) {
            characters.push(character_id);
        }
        return;
    }

    entries.push((key, vec![character_id]));
}

fn possible_unified_keys<K>(entries: Vec<(K, Vec<u32>)>) -> Vec<K> {
    entries
        .into_iter()
        .filter_map(|(key, characters)| (characters.len() >= TEAM_SIZE).then_some(key))
        .collect()
}
