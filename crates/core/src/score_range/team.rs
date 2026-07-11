use super::{ScoreRangeTeam, ScoreRangeTeamDomain, SkillBucketKey, TeamRecoveryData};
use crate::model::preparation::{ALL_ATTRIBUTE_KEY, ALL_BAND_KEY};
use crate::{AreaItemPercent, Attribute, PreparedCard, SelectedAreaItems, SongMode};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

pub fn bucket_teams_by_skill(
    teams: impl IntoIterator<Item = ScoreRangeTeam>,
) -> BTreeMap<SkillBucketKey, Vec<ScoreRangeTeam>> {
    let mut buckets = BTreeMap::new();
    for team in teams {
        buckets
            .entry(SkillBucketKey::from_skill(team.skill))
            .or_insert_with(Vec::new)
            .push(team);
    }
    for teams in buckets.values_mut() {
        teams.sort_by_key(|team| (std::cmp::Reverse(team.stat), team.card_ids));
    }
    buckets
}

pub fn score_range_item_combinations(
    area_item_percent: &AreaItemPercent,
) -> Vec<SelectedAreaItems> {
    crate::area_item_combinations(area_item_percent)
}

pub fn prepare_score_range_team_domain(
    cards: &[PreparedCard],
    area_item_percent: &AreaItemPercent,
    items: &[SelectedAreaItems],
    point_bonus_micros: &BTreeMap<u32, u64>,
) -> ScoreRangeTeamDomain {
    ScoreRangeTeamDomain {
        teams: Vec::new(),
        recovery: TeamRecoveryData {
            cards: cards.to_vec(),
            area_item_percent: area_item_percent.clone(),
            point_bonus_micros: point_bonus_micros.clone(),
            items: items.to_vec(),
        },
    }
}

/// Compatibility wrapper retained for callers of the old eager API. The redesigned domain keeps
/// raw cards and item contexts; five-card states are produced lazily by MITM during search.
pub fn enumerate_score_range_teams(
    cards: &[PreparedCard],
    area_item_percent: &AreaItemPercent,
    items: &[SelectedAreaItems],
    point_bonus_micros: &BTreeMap<u32, u64>,
) -> ScoreRangeTeamDomain {
    prepare_score_range_team_domain(cards, area_item_percent, items, point_bonus_micros)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ModeItemGroupKey {
    Mixed(usize),
    UnifiedBand {
        band_target: TargetGroup,
        attribute: String,
        magazine: &'static str,
    },
    UnifiedAttribute {
        band: String,
        attribute_target: TargetGroup,
        magazine: &'static str,
    },
    UnifiedBandAttribute {
        band_target: TargetGroup,
        attribute_target: TargetGroup,
        magazine: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TargetGroup {
    Miss,
    SingleHit,
    All,
    Multi(String),
}

pub(crate) fn group_items_by_mode<'a>(
    mode: SongMode,
    items: &'a [SelectedAreaItems],
) -> Vec<&'a SelectedAreaItems> {
    let mut result = BTreeMap::<ModeItemGroupKey, &'a SelectedAreaItems>::new();
    for (index, selected_items) in items.iter().enumerate() {
        let key = match mode {
            SongMode::Mixed => ModeItemGroupKey::Mixed(index),
            SongMode::UnifiedBand(band_id) => ModeItemGroupKey::UnifiedBand {
                band_target: band_target_group(&selected_items.band, band_id),
                attribute: selected_items.attribute.clone(),
                magazine: selected_items.magazine.as_str(),
            },
            SongMode::UnifiedAttribute(attribute) => ModeItemGroupKey::UnifiedAttribute {
                band: selected_items.band.clone(),
                attribute_target: attribute_target_group(&selected_items.attribute, attribute),
                magazine: selected_items.magazine.as_str(),
            },
            SongMode::UnifiedBandAttribute(band_id, attribute) => {
                ModeItemGroupKey::UnifiedBandAttribute {
                    band_target: band_target_group(&selected_items.band, band_id),
                    attribute_target: attribute_target_group(&selected_items.attribute, attribute),
                    magazine: selected_items.magazine.as_str(),
                }
            }
        };
        result
            .entry(key)
            .and_modify(|existing| {
                if compare_items(selected_items, existing).is_lt() {
                    *existing = selected_items;
                }
            })
            .or_insert(selected_items);
    }
    result.into_values().collect()
}

fn band_target_group(key: &str, band_id: u32) -> TargetGroup {
    if key == ALL_BAND_KEY {
        return TargetGroup::All;
    }
    if key.contains(',') {
        return TargetGroup::Multi(key.to_owned());
    }
    if key.parse::<u32>().ok() == Some(band_id) {
        TargetGroup::SingleHit
    } else {
        TargetGroup::Miss
    }
}

fn attribute_target_group(key: &str, attribute: Attribute) -> TargetGroup {
    if key == ALL_ATTRIBUTE_KEY {
        return TargetGroup::All;
    }
    if key.contains(',') {
        return TargetGroup::Multi(key.to_owned());
    }
    if key == attribute.as_str() {
        TargetGroup::SingleHit
    } else {
        TargetGroup::Miss
    }
}

fn compare_items(left: &SelectedAreaItems, right: &SelectedAreaItems) -> Ordering {
    left.band
        .cmp(&right.band)
        .then_with(|| left.attribute.cmp(&right.attribute))
        .then_with(|| left.magazine.as_str().cmp(right.magazine.as_str()))
}

pub(crate) fn signature_modes(cards: &[PreparedCard]) -> Vec<SongMode> {
    let mut result = vec![SongMode::Mixed];
    let bands = cards
        .iter()
        .map(|card| card.band_id)
        .collect::<BTreeSet<_>>();
    let mut attributes = Vec::new();
    for attribute in cards.iter().map(|card| card.attribute) {
        if !attributes.contains(&attribute) {
            attributes.push(attribute);
        }
    }
    result.extend(bands.iter().copied().map(SongMode::UnifiedBand));
    result.extend(attributes.iter().copied().map(SongMode::UnifiedAttribute));
    for band in bands {
        for attribute in &attributes {
            result.push(SongMode::UnifiedBandAttribute(band, *attribute));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::preparation::{StatRate, PERFORMANCE_KEY, TECHNIQUE_KEY, VISUAL_KEY};
    use crate::Magazine;

    #[test]
    fn score_range_uses_all_shared_item_combinations() {
        let area = AreaItemPercent {
            band: (1..=9)
                .map(|band| (band.to_string(), StatRate::zero()))
                .collect(),
            attribute: BTreeMap::from([
                ("cool".to_owned(), StatRate::zero()),
                ("happy".to_owned(), StatRate::zero()),
                ("pure".to_owned(), StatRate::zero()),
                ("powerful".to_owned(), StatRate::zero()),
                ("cool,happy,powerful,pure".to_owned(), StatRate::zero()),
            ]),
            magazine: BTreeMap::from([
                (PERFORMANCE_KEY.to_owned(), StatRate::zero()),
                (TECHNIQUE_KEY.to_owned(), StatRate::zero()),
                (VISUAL_KEY.to_owned(), StatRate::zero()),
            ]),
        };

        let combinations = score_range_item_combinations(&area);

        assert_eq!(combinations, crate::area_item_combinations(&area));
        assert_eq!(combinations.len(), 135);
    }

    #[test]
    fn mode_item_groups_merge_only_unified_dimensions() {
        let items = vec![
            SelectedAreaItems {
                band: "2".to_owned(),
                attribute: "cool".to_owned(),
                magazine: Magazine::Performance,
            },
            SelectedAreaItems {
                band: "1".to_owned(),
                attribute: "cool".to_owned(),
                magazine: Magazine::Performance,
            },
            SelectedAreaItems {
                band: "3".to_owned(),
                attribute: "cool".to_owned(),
                magazine: Magazine::Performance,
            },
            SelectedAreaItems {
                band: ALL_BAND_KEY.to_owned(),
                attribute: "cool".to_owned(),
                magazine: Magazine::Performance,
            },
        ];

        assert_eq!(group_items_by_mode(SongMode::Mixed, &items).len(), 4);
        assert_eq!(
            group_items_by_mode(SongMode::UnifiedBand(3), &items).len(),
            3,
        );
        assert_eq!(
            group_items_by_mode(SongMode::UnifiedBandAttribute(3, Attribute::Cool), &items,).len(),
            3,
        );
    }
}
