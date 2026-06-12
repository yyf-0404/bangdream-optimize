use super::prune::MedleyPruneSignature;
use super::team::{TeamBuildError, TeamGenerationOptions};
use crate::model::chart::{Chart, TeamCardSkill};
use crate::model::preparation::PreparedCard;
use crate::model::schema::Attribute;
use std::collections::HashMap;

const TEAM_SIZE: usize = 5;
const MEDLEY_TEAM_COUNT: usize = 3;

#[derive(Debug, Clone, Copy)]
pub(in crate::medley) struct MedleyCardInput<'a> {
    pub(in crate::medley) card: &'a PreparedCard,
    pub(in crate::medley) raw_index: usize,
    pub(in crate::medley) stat: f64,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::medley) struct ResolvedMedleyCardInput {
    raw_index: usize,
    stat: f64,
    band_id: u32,
    attribute: Attribute,
    skill: TeamCardSkill,
    skill_meta_by_chart: [[f64; TEAM_SIZE + 1]; MEDLEY_TEAM_COUNT],
}

#[derive(Debug, Clone)]
pub(crate) struct RawTeamCandidate {
    pub(crate) raw_indices: [usize; TEAM_SIZE],
    pub(crate) ordered_raw_indices: [[usize; TEAM_SIZE]; MEDLEY_TEAM_COUNT],
    pub(crate) captain_raw_indices: [usize; MEDLEY_TEAM_COUNT],
    pub(crate) scores: [i32; MEDLEY_TEAM_COUNT],
    pub(crate) stat: i32,
}

pub(in crate::medley) fn selected_resolved_team_signature(
    cards: &[ResolvedMedleyCardInput],
    selected_indices: &[usize],
) -> MedleyPruneSignature {
    let [idx0, idx1, idx2, idx3, idx4]: [usize; TEAM_SIZE] = selected_indices
        .try_into()
        .expect("team signature is called only for full teams");

    let first_band = cards[idx0].band_id;
    let same_band = cards[idx1].band_id == first_band
        && cards[idx2].band_id == first_band
        && cards[idx3].band_id == first_band
        && cards[idx4].band_id == first_band;
    let first_attribute = cards[idx0].attribute;
    let same_attribute = cards[idx1].attribute == first_attribute
        && cards[idx2].attribute == first_attribute
        && cards[idx3].attribute == first_attribute
        && cards[idx4].attribute == first_attribute;

    match (same_band, same_attribute) {
        (true, true) => MedleyPruneSignature::UnifiedBandAttribute(first_band, first_attribute),
        (true, false) => MedleyPruneSignature::UnifiedBand(first_band),
        (false, true) => MedleyPruneSignature::UnifiedAttribute(first_attribute),
        (false, false) => MedleyPruneSignature::Mixed,
    }
}

pub(in crate::medley) fn resolve_medley_cards_for_signature(
    cards: &[MedleyCardInput<'_>],
    charts: &[Chart],
    signature: MedleyPruneSignature,
    skill_meta_cache: &mut SkillMetaCache,
) -> Result<Vec<ResolvedMedleyCardInput>, TeamBuildError> {
    cards
        .iter()
        .map(|card| {
            let skill = TeamCardSkill {
                card_id: card.card.card_id,
                duration: card.card.skill.duration,
                score_up: card
                    .card
                    .score_up
                    .resolve(signature.team_band_id(), signature.team_attribute()),
                rateup: card.card.skill.rateup,
            };
            let mut skill_meta_by_chart = [[0.0; TEAM_SIZE + 1]; MEDLEY_TEAM_COUNT];
            for (chart_idx, chart) in charts.iter().enumerate() {
                skill_meta_by_chart[chart_idx] =
                    skill_meta_cache.values(chart_idx, chart, skill)?;
            }

            Ok(ResolvedMedleyCardInput {
                raw_index: card.raw_index,
                stat: card.stat,
                band_id: card.card.band_id,
                attribute: card.card.attribute,
                skill,
                skill_meta_by_chart,
            })
        })
        .collect()
}

pub(in crate::medley) fn build_resolved_candidate(
    cards: &[ResolvedMedleyCardInput],
    charts: &[Chart],
    options: TeamGenerationOptions,
    selected_indices: &[usize; TEAM_SIZE],
) -> Result<RawTeamCandidate, TeamBuildError> {
    let stat = selected_indices
        .iter()
        .map(|&index| cards[index].stat)
        .sum::<f64>();
    let stat_floor = stat.floor() as i32;

    let mut scores = [0; MEDLEY_TEAM_COUNT];
    let mut captain_raw_indices = [0; MEDLEY_TEAM_COUNT];
    let mut ordered_raw_indices = [[0; TEAM_SIZE]; MEDLEY_TEAM_COUNT];

    for (chart_idx, chart) in charts.iter().enumerate() {
        let skill_meta = selected_indices.map(|index| cards[index].skill_meta_by_chart[chart_idx]);
        let order = max_meta_order_for_team(&skill_meta);
        let skill_order: [TeamCardSkill; TEAM_SIZE + 1] = std::array::from_fn(|idx| match idx {
            TEAM_SIZE => cards[selected_indices[order.captain_index]].skill,
            _ => cards[selected_indices[order.order_indices[idx]]].skill,
        });

        scores[chart_idx] =
            chart.get_score_for_six_skills(&skill_order, stat_floor, options.score_as_medley)?;
        captain_raw_indices[chart_idx] = cards[selected_indices[order.captain_index]].raw_index;
        ordered_raw_indices[chart_idx] = order
            .order_indices
            .map(|idx| cards[selected_indices[idx]].raw_index);
    }

    Ok(RawTeamCandidate {
        raw_indices: selected_indices.map(|index| cards[index].raw_index),
        ordered_raw_indices,
        captain_raw_indices,
        scores,
        stat: stat_floor,
    })
}

pub(in crate::medley) fn build_candidate(
    cards: &[MedleyCardInput<'_>],
    charts: &[Chart],
    options: TeamGenerationOptions,
    selected_indices: &[usize],
    skill_meta_cache: &mut SkillMetaCache,
) -> Result<RawTeamCandidate, TeamBuildError> {
    let selected_indices: [usize; TEAM_SIZE] = selected_indices
        .try_into()
        .expect("candidate build is called only for full teams");
    let team_band_id = unified_band_id(cards, &selected_indices);
    let team_attribute = unified_attribute(cards, &selected_indices);
    let resolved_skills =
        resolve_team_skills(cards, &selected_indices, team_band_id, team_attribute);
    let stat = selected_indices
        .iter()
        .map(|&index| cards[index].stat)
        .sum::<f64>();
    let stat_floor = stat.floor() as i32;

    let mut scores = [0; MEDLEY_TEAM_COUNT];
    let mut captain_raw_indices = [0; MEDLEY_TEAM_COUNT];
    let mut ordered_raw_indices = [[0; TEAM_SIZE]; MEDLEY_TEAM_COUNT];

    for (chart_idx, chart) in charts.iter().enumerate() {
        let skill_meta = selected_skill_meta(
            chart_idx,
            chart,
            cards,
            &selected_indices,
            &resolved_skills,
            skill_meta_cache,
        )?;
        let order = max_meta_order_for_team(&skill_meta);
        let skill_order: [TeamCardSkill; TEAM_SIZE + 1] = std::array::from_fn(|idx| match idx {
            TEAM_SIZE => resolved_skills[order.captain_index],
            _ => resolved_skills[order.order_indices[idx]],
        });

        scores[chart_idx] =
            chart.get_score_for_six_skills(&skill_order, stat_floor, options.score_as_medley)?;
        captain_raw_indices[chart_idx] = cards[selected_indices[order.captain_index]].raw_index;
        ordered_raw_indices[chart_idx] = order
            .order_indices
            .map(|idx| cards[selected_indices[idx]].raw_index);
    }

    Ok(RawTeamCandidate {
        raw_indices: selected_indices.map(|index| cards[index].raw_index),
        ordered_raw_indices,
        captain_raw_indices,
        scores,
        stat: stat_floor,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MedleySkillOrder {
    order_indices: [usize; TEAM_SIZE],
    captain_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SkillMetaCacheKey {
    card_id: u32,
    score_up_bits: u64,
}

pub(in crate::medley) struct SkillMetaCache {
    by_chart: Vec<HashMap<SkillMetaCacheKey, [f64; TEAM_SIZE + 1]>>,
}

impl SkillMetaCache {
    pub(in crate::medley) fn new(chart_count: usize) -> Self {
        Self {
            by_chart: (0..chart_count).map(|_| HashMap::new()).collect(),
        }
    }

    pub(in crate::medley) fn entry_count(&self) -> usize {
        self.by_chart.iter().map(HashMap::len).sum()
    }

    pub(in crate::medley) fn values(
        &mut self,
        chart_idx: usize,
        chart: &Chart,
        skill: TeamCardSkill,
    ) -> Result<[f64; TEAM_SIZE + 1], TeamBuildError> {
        let key = SkillMetaCacheKey {
            card_id: skill.card_id,
            score_up_bits: skill.score_up.to_bits(),
        };
        if let Some(values) = self.by_chart[chart_idx].get(&key) {
            return Ok(*values);
        }

        let mut values = [0.0; TEAM_SIZE + 1];
        for (activation, value) in values.iter_mut().enumerate() {
            *value = chart.skill_meta_value(activation, skill)?;
        }
        self.by_chart[chart_idx].insert(key, values);
        Ok(values)
    }
}

fn selected_skill_meta(
    chart_idx: usize,
    chart: &Chart,
    cards: &[MedleyCardInput<'_>],
    selected_indices: &[usize; TEAM_SIZE],
    team: &[TeamCardSkill; TEAM_SIZE],
    skill_meta_cache: &mut SkillMetaCache,
) -> Result<[[f64; TEAM_SIZE + 1]; TEAM_SIZE], TeamBuildError> {
    let mut skill_meta = [[0.0; TEAM_SIZE + 1]; TEAM_SIZE];
    for card_idx in 0..TEAM_SIZE {
        let card_id = cards[selected_indices[card_idx]].card.card_id;
        let skill = TeamCardSkill {
            card_id,
            ..team[card_idx]
        };
        skill_meta[card_idx] = skill_meta_cache.values(chart_idx, chart, skill)?;
    }

    Ok(skill_meta)
}

fn max_meta_order_for_team(skill_meta: &[[f64; TEAM_SIZE + 1]; TEAM_SIZE]) -> MedleySkillOrder {
    let mut dp = [0.0; 1 << TEAM_SIZE];
    let mut choose = [0usize; 1 << TEAM_SIZE];

    for mask in 0..(1usize << TEAM_SIZE) {
        let activation = mask.count_ones() as usize;
        for (card_idx, card_meta) in skill_meta.iter().enumerate() {
            if mask >> card_idx & 1 == 1 {
                continue;
            }
            let value = dp[mask] + card_meta[activation];
            let next_mask = mask | (1 << card_idx);
            if value > dp[next_mask] {
                dp[next_mask] = value;
                choose[next_mask] = card_idx;
            }
        }
    }

    let mut captain_index = 0;
    let mut captain_meta = 0.0;
    for (card_idx, card_meta) in skill_meta.iter().enumerate() {
        let value = card_meta[TEAM_SIZE];
        if value > captain_meta {
            captain_meta = value;
            captain_index = card_idx;
        }
    }

    let mut order_indices = [0usize; TEAM_SIZE];
    let mut mask = (1usize << TEAM_SIZE) - 1;
    for slot in (0..TEAM_SIZE).rev() {
        let card_idx = choose[mask];
        order_indices[slot] = card_idx;
        mask ^= 1 << card_idx;
    }

    MedleySkillOrder {
        order_indices,
        captain_index,
    }
}

fn resolve_team_skills(
    cards: &[MedleyCardInput<'_>],
    selected_indices: &[usize; TEAM_SIZE],
    team_band_id: Option<u32>,
    team_attribute: Option<Attribute>,
) -> [TeamCardSkill; TEAM_SIZE] {
    std::array::from_fn(|idx| {
        let card = cards[selected_indices[idx]].card;
        TeamCardSkill {
            card_id: card.card_id,
            duration: card.skill.duration,
            score_up: card.score_up.resolve(team_band_id, team_attribute),
            rateup: card.skill.rateup,
        }
    })
}

fn unified_band_id(
    cards: &[MedleyCardInput<'_>],
    selected_indices: &[usize; TEAM_SIZE],
) -> Option<u32> {
    let first = cards[selected_indices[0]].card.band_id;
    selected_indices
        .iter()
        .all(|&index| cards[index].card.band_id == first)
        .then_some(first)
}

fn unified_attribute(
    cards: &[MedleyCardInput<'_>],
    selected_indices: &[usize; TEAM_SIZE],
) -> Option<Attribute> {
    let first = cards[selected_indices[0]].card.attribute;
    selected_indices
        .iter()
        .all(|&index| cards[index].card.attribute == first)
        .then_some(first)
}
