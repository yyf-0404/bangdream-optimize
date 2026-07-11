use super::{NumericCardSource, SingleSongError, SingleSongResult};
use crate::{
    floor_team_stat, model::chart::CompiledSixSkillScore, Chart, DpChartModel, TeamCardSkill,
};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

const TEAM_SIZE: usize = 5;
const SKILL_COUNT: usize = TEAM_SIZE + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SkillShape {
    duration_bits: u64,
    rateup: bool,
}

#[derive(Debug, Clone, Copy)]
struct ExactCard {
    card_id: u32,
    stat: f64,
    skill_id: u16,
    shape: SkillShape,
    score_up: f64,
    normal_meta: [f64; TEAM_SIZE],
    captain_meta: f64,
    position_order: [u8; TEAM_SIZE],
}

impl ExactCard {
    fn order_value(self) -> f64 {
        self.stat * (self.normal_meta.into_iter().fold(0.0_f64, f64::max) + self.captain_meta)
    }
}

#[derive(Debug, Clone, Copy)]
struct SearchState {
    stat: f64,
    normal_meta: f64,
    selected_captain_meta: f64,
    captain_meta: [f64; TEAM_SIZE],
    skill_ids: [u16; TEAM_SIZE],
    card_ids: [u32; TEAM_SIZE],
}

impl SearchState {
    fn empty() -> Self {
        Self {
            stat: 0.0,
            normal_meta: 0.0,
            selected_captain_meta: 0.0,
            captain_meta: [0.0; TEAM_SIZE],
            skill_ids: [0; TEAM_SIZE],
            card_ids: [0; TEAM_SIZE],
        }
    }
}

#[derive(Debug, Clone)]
struct SuffixBound {
    remaining_groups: usize,
    stat_by_slots: [f64; TEAM_SIZE + 1],
    normal_meta_by_mask: [f64; 1 << TEAM_SIZE],
    captain_meta: f64,
}

#[derive(Debug, Clone, Default)]
struct ExactProfile {
    raw_cards: usize,
    active_cards: usize,
    groups: usize,
    nodes: usize,
    feasibility_prunes: usize,
    upper_bound_prunes: usize,
    leaves: usize,
    exact_score_calls: usize,
    leaf_captain_prunes: usize,
    score_cache_hits: usize,
    compiled_timelines: usize,
    incumbent_updates: usize,
    prepare_ms: f64,
    search_ms: f64,
    score_ms: f64,
}

struct SearchContext<'a> {
    chart: &'a Chart,
    no_skill_meta: f64,
    skills: &'a [TeamCardSkill],
    groups: &'a [Vec<ExactCard>],
    suffix: &'a [SuffixBound],
    best: Option<SingleSongResult>,
    score_cache: HashMap<(i32, [u16; SKILL_COUNT]), i32>,
    timeline_cache: HashMap<[u16; SKILL_COUNT], CompiledSixSkillScore>,
    profile: ExactProfile,
}

pub(super) fn solve(
    cards: &[NumericCardSource],
    chart: &Chart,
) -> Result<SingleSongResult, SingleSongError> {
    let prepare_start = Instant::now();
    let (skills, groups, raw_cards, active_cards) = prepare_cards(cards, chart)?;
    if groups.len() < TEAM_SIZE {
        return Err(SingleSongError::NotEnoughCards {
            count: groups.len(),
        });
    }
    let suffix = suffix_bounds(&groups);
    let mut context = SearchContext {
        chart,
        no_skill_meta: chart.meta.no_skill,
        skills: &skills,
        groups: &groups,
        suffix: &suffix,
        best: None,
        score_cache: HashMap::new(),
        timeline_cache: HashMap::new(),
        profile: ExactProfile {
            raw_cards,
            active_cards,
            groups: groups.len(),
            prepare_ms: prepare_start.elapsed().as_secs_f64() * 1000.0,
            ..ExactProfile::default()
        },
    };

    let search_start = Instant::now();
    search(&mut context, 0, 0, &SearchState::empty())?;
    context.profile.search_ms = search_start.elapsed().as_secs_f64() * 1000.0;

    if std::env::var_os("BANGDREAM_OPTIMIZE_EXACT_PROFILE").is_some()
        && context.profile.search_ms >= 10.0
    {
        eprintln!(
            "single exact profile: raw_cards={} active_cards={} groups={} nodes={} feasibility_prunes={} upper_bound_prunes={} leaves={} leaf_captain_prunes={} exact_score_calls={} score_cache_hits={} compiled_timelines={} incumbent_updates={} prepare_ms={:.3} search_ms={:.3} score_ms={:.3}",
            context.profile.raw_cards,
            context.profile.active_cards,
            context.profile.groups,
            context.profile.nodes,
            context.profile.feasibility_prunes,
            context.profile.upper_bound_prunes,
            context.profile.leaves,
            context.profile.leaf_captain_prunes,
            context.profile.exact_score_calls,
            context.profile.score_cache_hits,
            context.profile.compiled_timelines,
            context.profile.incumbent_updates,
            context.profile.prepare_ms,
            context.profile.search_ms,
            context.profile.score_ms,
        );
    }

    context.best.ok_or(SingleSongError::NoResult)
}

fn prepare_cards(
    cards: &[NumericCardSource],
    chart: &Chart,
) -> Result<(Vec<TeamCardSkill>, Vec<Vec<ExactCard>>, usize, usize), SingleSongError> {
    let model = DpChartModel::from_chart(chart);
    let mut skill_ids = BTreeMap::new();
    let mut skills = vec![TeamCardSkill {
        card_id: 0,
        duration: 0.0,
        score_up: 0.0,
        rateup: false,
    }];
    let mut groups: BTreeMap<u32, Vec<ExactCard>> = BTreeMap::new();

    for card in cards {
        let skill_key = (
            card.skill.duration.to_bits(),
            card.skill.score_up.to_bits(),
            card.skill.rateup,
        );
        let skill_id = *skill_ids.entry(skill_key).or_insert_with(|| {
            let id = skills.len() as u16;
            skills.push(card.skill);
            id
        });
        let (normal_meta, captain_meta) = if chart.warning.is_empty() {
            let mut normal_meta = [0.0; TEAM_SIZE];
            for (position, value) in normal_meta.iter_mut().enumerate() {
                *value = model.skill_term(chart, position, card.skill)?.sb;
            }
            (
                normal_meta,
                model.skill_term(chart, TEAM_SIZE, card.skill)?.sb,
            )
        } else {
            let upper = chart
                .optimistic_skill_meta_any_window(card.skill)
                .map_err(crate::DpModelError::from)?;
            ([upper; TEAM_SIZE], upper)
        };
        groups
            .entry(card.character_id)
            .or_default()
            .push(ExactCard {
                card_id: card.card_id,
                stat: card.stat,
                skill_id,
                shape: SkillShape {
                    duration_bits: card.skill.duration.to_bits(),
                    rateup: card.skill.rateup,
                },
                score_up: card.skill.score_up,
                normal_meta,
                captain_meta,
                position_order: normal_position_order(normal_meta),
            });
    }

    let mut groups = groups
        .into_values()
        .map(prune_same_character_cards)
        .collect::<Vec<_>>();
    for group in &mut groups {
        group.sort_by(|left, right| {
            right
                .order_value()
                .total_cmp(&left.order_value())
                .then_with(|| left.card_id.cmp(&right.card_id))
        });
    }
    groups.sort_by(|left, right| {
        right[0]
            .order_value()
            .total_cmp(&left[0].order_value())
            .then_with(|| left.len().cmp(&right.len()))
    });
    let active_cards = groups.iter().map(Vec::len).sum();
    Ok((skills, groups, cards.len(), active_cards))
}

fn prune_same_character_cards(mut cards: Vec<ExactCard>) -> Vec<ExactCard> {
    cards.sort_by_key(|card| card.card_id);
    let mut result = Vec::new();
    for card in &cards {
        let dominated = cards.iter().any(|other| {
            other.card_id != card.card_id
                && other.shape == card.shape
                && other.stat >= card.stat
                && other.score_up >= card.score_up
                && (other.stat > card.stat
                    || other.score_up > card.score_up
                    || other.card_id < card.card_id)
        });
        if !dominated {
            result.push(*card);
        }
    }
    result
}

fn normal_position_order(normal_meta: [f64; TEAM_SIZE]) -> [u8; TEAM_SIZE] {
    let mut order = [0_u8, 1, 2, 3, 4];
    order.sort_by(|left, right| {
        normal_meta[*right as usize]
            .total_cmp(&normal_meta[*left as usize])
            .then_with(|| left.cmp(right))
    });
    order
}

fn suffix_bounds(groups: &[Vec<ExactCard>]) -> Vec<SuffixBound> {
    (0..=groups.len())
        .map(|start| {
            let suffix = &groups[start..];
            let mut group_stats = suffix
                .iter()
                .map(|group| group.iter().map(|card| card.stat).fold(0.0_f64, f64::max))
                .collect::<Vec<_>>();
            group_stats.sort_by(|left, right| right.total_cmp(left));
            let mut stat_by_slots = [0.0; TEAM_SIZE + 1];
            for slots in 1..=TEAM_SIZE {
                stat_by_slots[slots] =
                    stat_by_slots[slots - 1] + group_stats.get(slots - 1).copied().unwrap_or(0.0);
            }
            let mut normal_meta_by_mask = [f64::NEG_INFINITY; 1 << TEAM_SIZE];
            normal_meta_by_mask[0] = 0.0;
            let mut captain_meta = 0.0_f64;
            for group in suffix {
                let mut next = normal_meta_by_mask;
                for (mask, &value) in normal_meta_by_mask.iter().enumerate() {
                    if !value.is_finite() {
                        continue;
                    }
                    for card in group {
                        for position in empty_positions(mask) {
                            let next_mask = mask | (1 << position);
                            next[next_mask] =
                                next[next_mask].max(value + card.normal_meta[position]);
                        }
                    }
                }
                normal_meta_by_mask = next;
                for card in group {
                    captain_meta = captain_meta.max(card.captain_meta);
                }
            }
            SuffixBound {
                remaining_groups: suffix.len(),
                stat_by_slots,
                normal_meta_by_mask,
                captain_meta,
            }
        })
        .collect()
}

fn search(
    context: &mut SearchContext<'_>,
    group_idx: usize,
    mask: usize,
    state: &SearchState,
) -> Result<(), SingleSongError> {
    context.profile.nodes += 1;
    if mask == (1 << TEAM_SIZE) - 1 {
        return score_leaf(context, state);
    }
    let remaining_slots = TEAM_SIZE - mask.count_ones() as usize;
    let bound = &context.suffix[group_idx];
    if remaining_slots > bound.remaining_groups {
        context.profile.feasibility_prunes += 1;
        return Ok(());
    }
    if context.best.is_some()
        && score_upper_bound(context, state, mask, bound) <= context.best.as_ref().unwrap().score
    {
        context.profile.upper_bound_prunes += 1;
        return Ok(());
    }

    let group = &context.groups[group_idx];
    for card in group {
        for &position in &card.position_order {
            let position = position as usize;
            if mask & (1 << position) != 0 {
                continue;
            }
            let mut next = *state;
            next.stat += card.stat;
            next.normal_meta += card.normal_meta[position];
            next.selected_captain_meta = next.selected_captain_meta.max(card.captain_meta);
            next.captain_meta[position] = card.captain_meta;
            next.skill_ids[position] = card.skill_id;
            next.card_ids[position] = card.card_id;
            search(context, group_idx + 1, mask | (1 << position), &next)?;
        }
    }

    if bound.remaining_groups > remaining_slots {
        search(context, group_idx + 1, mask, state)?;
    }
    Ok(())
}

fn score_upper_bound(
    context: &SearchContext<'_>,
    state: &SearchState,
    mask: usize,
    bound: &SuffixBound,
) -> i32 {
    let remaining_slots = TEAM_SIZE - mask.count_ones() as usize;
    let stat = floor_team_stat([state.stat, bound.stat_by_slots[remaining_slots]]);
    let mut meta = context.no_skill_meta + state.normal_meta;
    let empty_mask = (!mask) & ((1 << TEAM_SIZE) - 1);
    meta += bound.normal_meta_by_mask[empty_mask];
    meta += state.selected_captain_meta.max(bound.captain_meta);
    integer_score_upper(stat as f64 * meta)
}

fn score_leaf(context: &mut SearchContext<'_>, state: &SearchState) -> Result<(), SingleSongError> {
    context.profile.leaves += 1;
    let stat = floor_team_stat([state.stat]);
    for captain_idx in 0..TEAM_SIZE {
        let score_upper = integer_score_upper(
            stat as f64
                * (context.no_skill_meta + state.normal_meta + state.captain_meta[captain_idx]),
        );
        if context
            .best
            .as_ref()
            .is_some_and(|best| score_upper <= best.score)
        {
            context.profile.leaf_captain_prunes += 1;
            continue;
        }
        let mut skill_ids = [0_u16; SKILL_COUNT];
        skill_ids[..TEAM_SIZE].copy_from_slice(&state.skill_ids);
        skill_ids[TEAM_SIZE] = state.skill_ids[captain_idx];
        let score_start = Instant::now();
        let score = if let Some(&score) = context.score_cache.get(&(stat, skill_ids)) {
            context.profile.score_cache_hits += 1;
            score
        } else {
            let skills = skill_ids.map(|skill_id| context.skills[skill_id as usize]);
            let compiled = match context.timeline_cache.entry(skill_ids) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    context.profile.compiled_timelines += 1;
                    let compiled = context
                        .chart
                        .compile_six_skill_score(&skills, false)
                        .map_err(crate::DpModelError::from)?;
                    entry.insert(compiled)
                }
            };
            let score = context.chart.score_compiled_six_skills(compiled, stat);
            context.score_cache.insert((stat, skill_ids), score);
            context.profile.exact_score_calls += 1;
            score
        };
        context.profile.score_ms += score_start.elapsed().as_secs_f64() * 1000.0;
        let candidate = SingleSongResult {
            score,
            stat,
            team_card_ids: state.card_ids.to_vec(),
            captain_card_id: state.card_ids[captain_idx],
        };
        if context.best.as_ref().is_none_or(|old| {
            candidate.score > old.score
                || (candidate.score == old.score && candidate.stat > old.stat)
                || (candidate.score == old.score
                    && candidate.stat == old.stat
                    && candidate.team_card_ids < old.team_card_ids)
        }) {
            context.profile.incumbent_updates += 1;
            context.best = Some(candidate);
        }
    }
    Ok(())
}

fn integer_score_upper(value: f64) -> i32 {
    value.ceil().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

fn empty_positions(mask: usize) -> impl Iterator<Item = usize> {
    (0..TEAM_SIZE).filter(move |position| mask & (1 << position) == 0)
}

#[cfg(test)]
mod tests {
    use super::integer_score_upper;

    #[test]
    fn integer_upper_rounds_outward_at_score_boundary() {
        assert_eq!(integer_score_upper(74_249.000_000_1), 74_250);
        assert_eq!(integer_score_upper(74_250.0), 74_250);
    }
}
