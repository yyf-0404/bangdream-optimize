//! Reuse per-character maxima before excluding the forced card's character.
use super::{signature_skill_meta_for_chart, MedleyCardPruneProfile, MedleyPruneSignature};
use crate::PreparedCard;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(super) struct TeammateUpperBounds {
    pub(super) signature: MedleyPruneSignature,
    stat: CharacterLeaders,
    skills_by_chart: Vec<CharacterLeaders>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TeammateChartBound {
    pub(super) stat: f64,
    pub(super) normal_skill: f64,
    pub(super) captain_skill: f64,
}

/// Four teammates excluding at most one character need only the top five
/// distinct-character values. Power and skill leaders remain independent,
/// exactly as in the original optimistic bound.
#[derive(Debug, Clone)]
struct CharacterLeaders(Vec<(u32, f64)>);

impl CharacterLeaders {
    fn new(values: impl Iterator<Item = (u32, f64)>) -> Self {
        let mut values: Vec<_> = values.filter(|&(_, value)| value > 0.0).collect();
        values.sort_unstable_by(|left, right| right.1.total_cmp(&left.1));
        values.truncate(5);
        Self(values)
    }

    fn sum_excluding<const N: usize>(&self, excluded: u32) -> f64 {
        debug_assert!(N < 5);
        // Keep the same descending addition order and zero padding as TopValues.
        let mut values = [0.0; N];
        for (slot, &(_, value)) in self
            .0
            .iter()
            .filter(|&&(character, _)| character != excluded)
            .take(N)
            .enumerate()
        {
            values[slot] = value;
        }
        values.iter().sum()
    }
}

impl TeammateUpperBounds {
    pub(super) fn new(
        cards: &[PreparedCard],
        profiles: &[MedleyCardPruneProfile],
        signature: MedleyPruneSignature,
        chart_count: usize,
    ) -> Self {
        let mut characters: BTreeMap<u32, (f64, Vec<f64>)> = BTreeMap::new();
        for (card, profile) in cards.iter().zip(profiles) {
            if !signature.allows(card) {
                continue;
            }
            let (stat, skills) = characters
                .entry(card.character_id)
                .or_insert_with(|| (0.0, vec![0.0; chart_count]));
            *stat = stat.max(profile.stat);
            for (chart, skill) in skills.iter_mut().enumerate() {
                *skill = skill.max(signature_skill_meta_for_chart(
                    card, profile, signature, chart,
                ));
            }
        }
        Self {
            signature,
            stat: CharacterLeaders::new(characters.iter().map(|(&id, (stat, _))| (id, *stat))),
            skills_by_chart: (0..chart_count)
                .map(|chart| {
                    CharacterLeaders::new(
                        characters
                            .iter()
                            .map(|(&id, (_, skills))| (id, skills[chart])),
                    )
                })
                .collect(),
        }
    }

    pub(super) fn for_chart(&self, excluded: u32, chart: usize) -> TeammateChartBound {
        TeammateChartBound {
            stat: self.stat.sum_excluding::<4>(excluded),
            normal_skill: self.skills_by_chart[chart].sum_excluding::<4>(excluded),
            captain_skill: self.skills_by_chart[chart].sum_excluding::<1>(excluded),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{medley_card_prune_profiles, top_character_values_sum};
    use super::*;
    use crate::medley::{
        team::adjusted_card_stats,
        test_support::{medley_charts, prepared_card, selected_cool_items},
    };
    use crate::{AreaItemPercent, Attribute};

    #[test]
    fn excluded_character_leaders_match_full_card_scan_bit_for_bit() {
        let charts = medley_charts();
        let mut cards: Vec<_> = (0..48_u32)
            .map(|i| {
                let mut card = prepared_card(
                    i + 1,
                    (i / 3) * 1009,
                    i % 3 + 1,
                    if i % 2 == 0 {
                        Attribute::Cool
                    } else {
                        Attribute::Pure
                    },
                );
                card.stat.performance += (i % 7) as f64 * 112.13;
                card.stat.technique += (i % 11) as f64 * 81.7;
                card.skill.score_up = 0.5 + (i % 4) as f64 * 0.25;
                card.score_up.default = card.skill.score_up;
                card.score_up.unification_activate_effect_value = Some(1.5);
                card.score_up.unification_activate_condition_band_id = Some(card.band_id);
                card
            })
            .collect();
        cards[47].character_id = u32::MAX;
        for len in [0, 1, 3, 9, 17, 48] {
            let cards = &cards[..len];
            let powers =
                adjusted_card_stats(cards, &AreaItemPercent::empty(), &selected_cool_items());
            let profiles = medley_card_prune_profiles(cards, &charts, &powers).unwrap();
            for signature in [
                MedleyPruneSignature::Mixed,
                MedleyPruneSignature::UnifiedBand(1),
                MedleyPruneSignature::UnifiedBand(99),
                MedleyPruneSignature::UnifiedAttribute(Attribute::Pure),
                MedleyPruneSignature::UnifiedBandAttribute(2, Attribute::Cool),
            ] {
                let cached = TeammateUpperBounds::new(cards, &profiles, signature, charts.len());
                for excluded in cards
                    .iter()
                    .map(|c| c.character_id)
                    .chain([0, 999, u32::MAX])
                {
                    for chart in 0..charts.len() {
                        let bound = cached.for_chart(excluded, chart);
                        let allows = |card: &PreparedCard| {
                            card.character_id != excluded && signature.allows(card)
                        };
                        let stat = top_character_values_sum::<4, _>(cards, |i, card| {
                            allows(card).then_some(profiles[i].stat)
                        });
                        let normal = top_character_values_sum::<4, _>(cards, |i, card| {
                            allows(card).then(|| {
                                signature_skill_meta_for_chart(card, &profiles[i], signature, chart)
                            })
                        });
                        let captain = top_character_values_sum::<1, _>(cards, |i, card| {
                            allows(card).then(|| {
                                signature_skill_meta_for_chart(card, &profiles[i], signature, chart)
                            })
                        });
                        assert_eq!(
                            [
                                bound.stat.to_bits(),
                                bound.normal_skill.to_bits(),
                                bound.captain_skill.to_bits()
                            ],
                            [stat.to_bits(), normal.to_bits(), captain.to_bits()],
                            "len={len} excluded={excluded} signature={signature:?} chart={chart}"
                        );
                    }
                }
            }
        }
    }
}
