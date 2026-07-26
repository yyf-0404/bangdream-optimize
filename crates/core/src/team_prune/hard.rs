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
pub(crate) struct MedleyCardPruneProfile {
    pub(crate) stat: f64,
    skill_meta_variants: Vec<SkillMetaVariant>,
    pub(crate) skill_meta_bounds: Vec<SkillMetaBounds>,
    pub(crate) best_skill_meta_by_chart: Vec<f64>,
}

impl MedleyCardPruneProfile {
    pub(crate) fn skill_meta_for_score_up(&self, score_up: f64) -> Option<&[f64]> {
        let score_up_bits = score_up.to_bits();
        self.skill_meta_variants
            .iter()
            .find(|variant| variant.score_up_bits == score_up_bits)
            .map(|variant| variant.values.as_slice())
    }
}

#[derive(Debug, Clone)]
struct SkillMetaVariant {
    score_up_bits: u64,
    values: Vec<f64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SkillMetaBounds {
    pub(crate) low: f64,
    pub(crate) high: f64,
}

#[derive(Debug, Clone, Copy)]
struct SignatureHardDominanceModel<'a> {
    stat: f64,
    card_id: u32,
    meta: &'a [f64],
}

#[derive(Debug, Clone)]
pub(crate) struct MedleyPruneContext {
    pub(crate) unified_bands: Vec<u32>,
    pub(crate) unified_attributes: Vec<Attribute>,
    pub(crate) unified_band_attributes: Vec<(u32, Attribute)>,
    pub(crate) obligations: Vec<Vec<MedleyPruneSignature>>,
}

impl MedleyPruneContext {
    pub(crate) fn new(
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

pub(crate) struct MedleyPruneUpperBounds<'a> {
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
    pub(crate) fn new(
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

    pub(crate) fn with_best_any_team_scores(
        cards: &'a [PreparedCard],
        charts: &'a [Chart],
        profiles: &'a [MedleyCardPruneProfile],
        best_any_team_scores: &[f64],
    ) -> Self {
        debug_assert_eq!(charts.len(), best_any_team_scores.len());
        Self {
            cards,
            charts,
            profiles,
            best_any_team_scores: best_any_team_scores.to_vec(),
            teammate_cache: Vec::new(),
        }
    }

    pub(crate) fn signature_can_beat_incumbent(
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

    pub(crate) fn card_chart_eligibility_masks(
        &mut self,
        signatures: &[MedleyPruneSignature],
        current_best: i32,
    ) -> Vec<u8> {
        debug_assert!(self.charts.len() <= u8::BITS as usize);
        let all_charts_mask = if self.charts.len() == u8::BITS as usize {
            u8::MAX
        } else {
            (1_u8 << self.charts.len()) - 1
        };
        if current_best <= 0 || self.charts.len() != MEDLEY_TEAM_COUNT {
            return vec![all_charts_mask; self.cards.len()];
        }

        let mut masks = vec![0_u8; self.cards.len()];
        for card_idx in 0..self.cards.len() {
            for chart_idx in 0..self.charts.len() {
                let other_songs = self
                    .best_any_team_scores
                    .iter()
                    .enumerate()
                    .filter(|(idx, _)| *idx != chart_idx)
                    .map(|(_, &score)| score)
                    .sum::<f64>();
                let eligible = other_songs > current_best as f64
                    || signatures.iter().copied().any(|signature| {
                        signature.allows(&self.cards[card_idx])
                            && self.forced_team_score_upper_bound(card_idx, signature, chart_idx)
                                + other_songs
                                > current_best as f64
                    });
                if eligible {
                    masks[card_idx] |= 1_u8 << chart_idx;
                }
            }
        }
        masks
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
        (stat * (self.charts[chart_idx].meta.no_skill + skill_meta)).ceil()
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

pub(crate) fn rejection_counts(
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

pub(crate) fn best_any_team_score_upper_bound(
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
    (stat * (charts[chart_idx].meta.no_skill + skill_meta)).ceil()
}

pub(crate) fn signature_skill_meta_for_chart(
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

pub(crate) fn top_character_values_sum<const N: usize, F>(
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

pub(crate) fn medley_card_prune_profiles(
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

pub(crate) fn medley_card_dominates_for_signature(
    left: &PreparedCard,
    left_profile: &MedleyCardPruneProfile,
    right: &PreparedCard,
    right_profile: &MedleyCardPruneProfile,
    signature: MedleyPruneSignature,
) -> bool {
    dominance_rejection_for_signature(left, left_profile, right, right_profile, signature)
        == DominanceCheck::Dominates
}

pub(crate) fn medley_card_dominates_ignoring_unification(
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

pub(crate) fn same_character_cover(
    graph: &DominanceGraph,
    target_idx: usize,
    cards: &[PreparedCard],
    required_cover: usize,
) -> usize {
    same_group_dominator_cover(
        graph,
        target_idx,
        cards,
        |card| card.character_id,
        required_cover,
    )
}

pub(crate) fn cross_cover(
    graph: &DominanceGraph,
    target_idx: usize,
    cards: &[PreparedCard],
    signature: MedleyPruneSignature,
    team_count: usize,
    chart_eligibility_masks: &[u8],
    chart_count: usize,
) -> usize {
    let coarse_cover = cross_group_dominator_cover(
        graph,
        target_idx,
        cards,
        |card| card.character_id,
        TEAM_SIZE,
        team_count,
    );
    if coarse_cover > 0 {
        return coarse_cover;
    }

    precise_cross_cover(
        graph,
        target_idx,
        cards,
        signature,
        team_count,
        chart_eligibility_masks,
        chart_count,
    )
}

#[derive(Debug)]
struct PreciseCoverCharacterGroup {
    character_id: u32,
    teammate_break_options: u8,
    dominator_indices: Vec<usize>,
}

fn precise_cross_cover(
    graph: &DominanceGraph,
    target_idx: usize,
    cards: &[PreparedCard],
    signature: MedleyPruneSignature,
    team_count: usize,
    chart_eligibility_masks: &[u8],
    chart_count: usize,
) -> usize {
    if target_idx >= cards.len() || chart_eligibility_masks.len() != cards.len() {
        return 0;
    }
    if !matches!(team_count, 1 | MEDLEY_TEAM_COUNT)
        || (team_count == MEDLEY_TEAM_COUNT && chart_count != MEDLEY_TEAM_COUNT)
    {
        return 0;
    }

    let target = &cards[target_idx];
    let incoming = graph.incoming(target_idx);
    if incoming.is_empty() {
        return 0;
    }

    let mut groups_by_character: BTreeMap<u32, PreciseCoverCharacterGroup> = BTreeMap::new();
    for (card_idx, card) in cards.iter().enumerate() {
        if !signature.allows(card) {
            continue;
        }
        let break_mask = card_break_mask(target, card);
        let group = groups_by_character
            .entry(card.character_id)
            .or_insert_with(|| PreciseCoverCharacterGroup {
                character_id: card.character_id,
                teammate_break_options: 0,
                dominator_indices: Vec::new(),
            });
        group.teammate_break_options |= 1_u8 << break_mask;
        if incoming.contains(&card_idx) {
            group.dominator_indices.push(card_idx);
        }
    }
    let groups = groups_by_character.into_values().collect::<Vec<_>>();
    let dominator_count = groups
        .iter()
        .map(|group| group.dominator_indices.len())
        .sum::<usize>();
    if dominator_count == 0 {
        return 0;
    }

    let target_chart_indices = if team_count == 1 {
        vec![0]
    } else {
        (0..chart_count).collect::<Vec<_>>()
    };
    target_chart_indices
        .into_iter()
        .map(|target_chart_idx| {
            precise_cross_cover_for_target_chart(
                &groups,
                target,
                signature,
                dominator_count,
                team_count,
                chart_eligibility_masks,
                target_chart_idx,
                chart_count,
            )
        })
        .min()
        .unwrap_or_default()
}

fn precise_cross_cover_for_target_chart(
    groups: &[PreciseCoverCharacterGroup],
    target: &PreparedCard,
    signature: MedleyPruneSignature,
    dominator_count: usize,
    team_count: usize,
    chart_eligibility_masks: &[u8],
    target_chart_idx: usize,
    chart_count: usize,
) -> usize {
    const BREAK_STATE_COUNT: usize = 4;
    const OTHER_SLOT_COUNT: usize = TEAM_SIZE + 1;
    const DP_SIZE: usize = TEAM_SIZE * OTHER_SLOT_COUNT * OTHER_SLOT_COUNT * BREAK_STATE_COUNT;
    const UNREACHABLE: i16 = -1;

    let other_charts = if team_count == 1 {
        Vec::new()
    } else {
        (0..chart_count)
            .filter(|&chart_idx| chart_idx != target_chart_idx)
            .collect::<Vec<_>>()
    };
    debug_assert!(other_charts.len() <= 2);

    let required_break_mask = signature_required_break_mask(signature);
    let state_index = |teammates: usize, first_slots: usize, second_slots: usize, breaks: usize| {
        (((teammates * OTHER_SLOT_COUNT + first_slots) * OTHER_SLOT_COUNT + second_slots)
            * BREAK_STATE_COUNT)
            + breaks
    };
    let mut current = [UNREACHABLE; DP_SIZE];
    current[state_index(0, 0, 0, 0)] = 0;

    for group in groups {
        let mut next = current;
        let other_options = other_team_occupancy_options(
            &group.dominator_indices,
            chart_eligibility_masks,
            &other_charts,
        );
        for teammates in 0..TEAM_SIZE {
            for first_slots in 0..=TEAM_SIZE {
                for second_slots in 0..=TEAM_SIZE {
                    for breaks in 0..BREAK_STATE_COUNT {
                        let value =
                            current[state_index(teammates, first_slots, second_slots, breaks)];
                        if value == UNREACHABLE {
                            continue;
                        }

                        for &(add_first, add_second) in &other_options {
                            let next_first = first_slots + add_first;
                            let next_second = second_slots + add_second;
                            if next_first > TEAM_SIZE || next_second > TEAM_SIZE {
                                continue;
                            }
                            let next_idx = state_index(teammates, next_first, next_second, breaks);
                            next[next_idx] =
                                next[next_idx].max(value + (add_first + add_second) as i16);
                        }

                        if group.character_id == target.character_id || teammates + 1 >= TEAM_SIZE {
                            continue;
                        }
                        for break_mask in 0..BREAK_STATE_COUNT {
                            if group.teammate_break_options & (1_u8 << break_mask) == 0 {
                                continue;
                            }
                            let next_breaks = breaks | break_mask;
                            let next_idx =
                                state_index(teammates + 1, first_slots, second_slots, next_breaks);
                            next[next_idx] =
                                next[next_idx].max(value + group.dominator_indices.len() as i16);
                        }
                    }
                }
            }
        }
        current = next;
    }

    let mut max_unavailable = UNREACHABLE;
    for first_slots in 0..=TEAM_SIZE {
        for second_slots in 0..=TEAM_SIZE {
            for breaks in 0..BREAK_STATE_COUNT {
                if breaks & required_break_mask != required_break_mask {
                    continue;
                }
                max_unavailable = max_unavailable
                    .max(current[state_index(TEAM_SIZE - 1, first_slots, second_slots, breaks)]);
            }
        }
    }

    if max_unavailable == UNREACHABLE {
        return 0;
    }
    dominator_count.saturating_sub(max_unavailable as usize)
}

fn card_break_mask(target: &PreparedCard, card: &PreparedCard) -> usize {
    usize::from(card.band_id != target.band_id)
        | (usize::from(card.attribute != target.attribute) << 1)
}

fn signature_required_break_mask(signature: MedleyPruneSignature) -> usize {
    match signature {
        MedleyPruneSignature::Mixed => 0b11,
        MedleyPruneSignature::UnifiedBand(_) => 0b10,
        MedleyPruneSignature::UnifiedAttribute(_) => 0b01,
        MedleyPruneSignature::UnifiedBandAttribute(_, _) => 0,
    }
}

fn other_team_occupancy_options(
    dominator_indices: &[usize],
    chart_eligibility_masks: &[u8],
    other_charts: &[usize],
) -> Vec<(usize, usize)> {
    let mut options = vec![(0, 0)];
    let Some(&first_chart) = other_charts.first() else {
        return options;
    };
    let first_bit = 1_u8 << first_chart;
    if dominator_indices
        .iter()
        .any(|&idx| chart_eligibility_masks[idx] & first_bit != 0)
    {
        options.push((1, 0));
    }

    let Some(&second_chart) = other_charts.get(1) else {
        return options;
    };
    let second_bit = 1_u8 << second_chart;
    if dominator_indices
        .iter()
        .any(|&idx| chart_eligibility_masks[idx] & second_bit != 0)
    {
        options.push((0, 1));
    }
    if dominator_indices.iter().copied().any(|first_idx| {
        chart_eligibility_masks[first_idx] & first_bit != 0
            && dominator_indices.iter().copied().any(|second_idx| {
                first_idx != second_idx && chart_eligibility_masks[second_idx] & second_bit != 0
            })
    }) {
        options.push((1, 1));
    }
    options
}

pub(crate) fn hard_dominance_graph_for_indices(
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

pub(crate) fn same_character_dominator_cover(
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

pub(crate) fn full_medley_dominator_cover(
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

pub(crate) fn full_medley_dominator_cover_ignoring_unification(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medley::team::adjusted_card_stats;
    use crate::medley::test_support::{medley_charts, prepared_card, selected_cool_items};
    use crate::model::preparation::AreaItemPercent;

    #[test]
    fn missing_score_up_variant_is_not_silently_replaced() {
        let profile = MedleyCardPruneProfile {
            stat: 1.0,
            skill_meta_variants: vec![SkillMetaVariant {
                score_up_bits: 1.0_f64.to_bits(),
                values: vec![10.0],
            }],
            skill_meta_bounds: vec![SkillMetaBounds {
                low: 10.0,
                high: 10.0,
            }],
            best_skill_meta_by_chart: vec![10.0],
        };

        assert_eq!(profile.skill_meta_for_score_up(1.0), Some(&[10.0][..]));
        assert_eq!(profile.skill_meta_for_score_up(1.5), None);
    }

    #[test]
    fn forced_team_upper_bound_covers_every_exact_team_containing_card() {
        let cards = (1..=7)
            .map(|character_id| prepared_card(character_id, character_id, 1, Attribute::Cool))
            .collect::<Vec<_>>();
        let charts = medley_charts();
        let card_stats =
            adjusted_card_stats(&cards, &AreaItemPercent::empty(), &selected_cool_items());
        let profiles = medley_card_prune_profiles(&cards, &charts, &card_stats).unwrap();
        let signature = MedleyPruneSignature::UnifiedBandAttribute(1, Attribute::Cool);
        let mut bounds = MedleyPruneUpperBounds::new(&cards, &charts, &profiles);

        for forced_idx in 0..cards.len() {
            for chart_idx in 0..charts.len() {
                let upper = bounds.forced_team_score_upper_bound(forced_idx, signature, chart_idx);
                let mut exact_max = i32::MIN;
                for mask in 0usize..(1 << cards.len()) {
                    if mask.count_ones() != TEAM_SIZE as u32 || mask & (1 << forced_idx) == 0 {
                        continue;
                    }
                    let indices = (0..cards.len())
                        .filter(|idx| mask & (1 << idx) != 0)
                        .collect::<Vec<_>>();
                    let stat =
                        crate::floor_team_stat(indices.iter().map(|&idx| profiles[idx].stat));
                    let team = indices
                        .iter()
                        .map(|&idx| TeamCardSkill {
                            score_up: cards[idx]
                                .score_up
                                .resolve(signature.team_band_id(), signature.team_attribute()),
                            ..cards[idx].skill
                        })
                        .collect::<Vec<_>>();
                    exact_max = exact_max.max(
                        charts[chart_idx]
                            .get_max_score_order(&team, stat, false)
                            .unwrap()
                            .score,
                    );
                }
                assert!(
                    exact_max as f64 <= upper,
                    "forced={forced_idx} chart={chart_idx} exact={exact_max} upper={upper}"
                );
            }
        }
    }

    #[test]
    fn precise_cover_rejects_infeasible_mixed_teammate_blockers() {
        let mut cards = vec![prepared_card(1, 99, 1, Attribute::Cool)];
        let mut graph = DominanceGraph::new(10);
        for character_id in 1..=4 {
            for copy_idx in 0..2 {
                let card_idx = cards.len();
                cards.push(prepared_card(
                    100 * character_id + copy_idx,
                    character_id,
                    1,
                    Attribute::Cool,
                ));
                graph.add_edge(card_idx, 0);
            }
        }
        cards.push(prepared_card(999, 5, 2, Attribute::Happy));
        let eligibility = vec![1_u8; cards.len()];

        let coarse =
            cross_group_dominator_cover(&graph, 0, &cards, |card| card.character_id, TEAM_SIZE, 1);
        let precise = precise_cross_cover(
            &graph,
            0,
            &cards,
            MedleyPruneSignature::Mixed,
            1,
            &eligibility,
            1,
        );

        assert_eq!(coarse, 0);
        assert_eq!(precise, 2);
    }

    #[test]
    fn precise_cover_respects_chart_specific_other_team_eligibility() {
        let mut cards = vec![prepared_card(1, 99, 1, Attribute::Cool)];
        let mut graph = DominanceGraph::new(7);
        for copy_idx in 0..2 {
            let card_idx = cards.len();
            cards.push(prepared_card(100 + copy_idx, 99, 1, Attribute::Cool));
            graph.add_edge(card_idx, 0);
        }
        for character_id in 1..=4 {
            cards.push(prepared_card(
                200 + character_id,
                character_id,
                1,
                Attribute::Cool,
            ));
        }
        let mut eligibility = vec![0b111_u8; cards.len()];
        eligibility[1] = 0b010;
        eligibility[2] = 0b010;

        let coarse = cross_group_dominator_cover(
            &graph,
            0,
            &cards,
            |card| card.character_id,
            TEAM_SIZE,
            MEDLEY_TEAM_COUNT,
        );
        let precise = precise_cross_cover(
            &graph,
            0,
            &cards,
            MedleyPruneSignature::UnifiedBandAttribute(1, Attribute::Cool),
            MEDLEY_TEAM_COUNT,
            &eligibility,
            MEDLEY_TEAM_COUNT,
        );

        assert_eq!(coarse, 0);
        assert_eq!(precise, 1);
    }
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
