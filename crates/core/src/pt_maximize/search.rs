use std::collections::BTreeMap;

use crate::single::candidate::{self, SingleCardRole};
use crate::single::profile::{skill_meta_profile, SkillMetaProfile};
use crate::timing::Timer;
use crate::{
    area_item_combinations, cooperative_points, floor_team_stat, mode_candidates, AreaItemPercent,
    Chart, DpChartModel, EventType, PreparedCard, SelectedAreaItems, SongMode, TeamCardSkill,
};

use super::distribution::{
    evaluate_cooperative_captain_with_scratch, evaluate_cooperative_team_with_scratch,
    evaluate_full_team_summary_with_scratch, evaluate_full_team_with_scratch,
    materialize_full_team_summary, points_for_scenario, FullTeamPtCutoff, FullTeamPtSummary,
    FullTeamPtSummaryOutcome, SinglePtScoreScratch,
};
use super::{
    model::compare_nonnegative_averages, AveragePt, CooperativePtScenario, EventBonusApplication,
    LiveVariant, PtMaximizeError, PtMaximizeSearchScenario, PtMaximizeSingleMetrics,
    PtMaximizeTeamResult,
};

pub fn event_bonus_application(
    event_type: EventType,
    live_variant: LiveVariant,
) -> EventBonusApplication {
    if event_type == EventType::Medley
        || live_variant == LiveVariant::ChallengeCp
        || event_type == EventType::Versus
        || live_variant == LiveVariant::Festival
    {
        EventBonusApplication::TeamStat
    } else {
        EventBonusApplication::PointMultiplier
    }
}

#[derive(Debug, Clone, Copy)]
struct SearchCard {
    card_id: u32,
    character_id: u32,
    stat: f64,
    point_bonus_micros: u64,
    skill: TeamCardSkill,
    best_normal_meta: f64,
    captain_meta: f64,
}

#[derive(Debug)]
struct RoleCardGroups {
    groups: Vec<Vec<SearchCard>>,
    canonical_count: usize,
    retained_count: usize,
}

#[derive(Debug, Clone)]
struct SuffixBound {
    remaining_groups: usize,
    stat_by_slots: [f64; 6],
    point_bonus_by_slots: [u64; 6],
    normal_meta_by_slots: [f64; 6],
    captain_meta: f64,
    min_card_ids_by_slots: [[u32; 5]; 6],
}

#[derive(Debug, Clone, Copy)]
struct CharacterModeUpper {
    stat: f64,
    point_bonus_micros: u64,
    normal_meta: f64,
    captain_meta: f64,
    min_card_id: u32,
}

#[derive(Debug, Clone, Copy)]
struct CooperativeBranchUpper {
    score_factor: f64,
    teammate_score_upper: i64,
}

#[derive(Debug)]
struct SingleSearchTrace {
    enabled: bool,
    log_enabled: bool,
    total_start: Timer,
    current_item: usize,
    current_mode: usize,
    mode_searches: u64,
    allowed_cards: u64,
    canonical_cards: u64,
    prepruned_cards: u64,
    retained_cards: u64,
    recursive_nodes: u64,
    leaves: u64,
    minimum_stat_rejects: u64,
    exact_evaluations: u64,
    branch_meta_upper_bound_prunes: u64,
    meta_upper_bound_prunes: u64,
    exact_upper_bound_prunes: u64,
    mode_meta_upper_bound_prunes: u64,
    planned_leaves: u128,
    prepare_ms: f64,
    mode_meta_precheck_ms: f64,
    preprune_ms: f64,
    resolve_ms: f64,
    canonical_prune_ms: f64,
    suffix_ms: f64,
    enumerate_ms: f64,
    exact_evaluation_ms: f64,
}

impl SingleSearchTrace {
    fn new(enabled: bool, log_enabled: bool) -> Self {
        Self {
            enabled,
            log_enabled,
            total_start: Timer::start(),
            current_item: 0,
            current_mode: 0,
            mode_searches: 0,
            allowed_cards: 0,
            canonical_cards: 0,
            prepruned_cards: 0,
            retained_cards: 0,
            recursive_nodes: 0,
            leaves: 0,
            minimum_stat_rejects: 0,
            exact_evaluations: 0,
            branch_meta_upper_bound_prunes: 0,
            meta_upper_bound_prunes: 0,
            exact_upper_bound_prunes: 0,
            mode_meta_upper_bound_prunes: 0,
            planned_leaves: 0,
            prepare_ms: 0.0,
            mode_meta_precheck_ms: 0.0,
            preprune_ms: 0.0,
            resolve_ms: 0.0,
            canonical_prune_ms: 0.0,
            suffix_ms: 0.0,
            enumerate_ms: 0.0,
            exact_evaluation_ms: 0.0,
        }
    }

    fn trace_progress(&self) {
        if self.log_enabled && self.exact_evaluations % 1_000_000 == 0 {
            eprintln!(
                "PT single progress: item={} mode={} exact_evaluations={} leaves={} recursive_nodes={} elapsed_ms={:.3} exact_evaluation_ms={:.3}",
                self.current_item,
                self.current_mode,
                self.exact_evaluations,
                self.leaves,
                self.recursive_nodes,
                self.total_start.elapsed_ms(),
                self.exact_evaluation_ms,
            );
        }
    }

    fn metrics(&self, item_count: usize, mode_count: usize) -> PtMaximizeSingleMetrics {
        PtMaximizeSingleMetrics {
            item_count,
            mode_count,
            mode_search_count: self.mode_searches,
            retained_card_count: self.retained_cards,
            planned_team_count: self.planned_leaves,
            explored_team_count: self.leaves,
            exact_evaluation_count: self.exact_evaluations,
            candidate_build_ms: self.prepare_ms,
            solve_ms: self.enumerate_ms,
            exact_evaluation_ms: self.exact_evaluation_ms,
            total_elapsed_ms: self.total_start.elapsed_ms(),
        }
    }
}

pub fn search_single_song(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    point_bonus_micros: &BTreeMap<u32, u64>,
    minimum_stat: Option<i32>,
    scenario: PtMaximizeSearchScenario,
) -> Result<PtMaximizeTeamResult, PtMaximizeError> {
    search_single_song_with_metrics(
        cards,
        chart,
        area_item_percent,
        point_bonus_micros,
        minimum_stat,
        scenario,
    )
    .map(|(result, _)| result)
}

pub fn search_single_song_with_metrics(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    point_bonus_micros: &BTreeMap<u32, u64>,
    minimum_stat: Option<i32>,
    scenario: PtMaximizeSearchScenario,
) -> Result<(PtMaximizeTeamResult, PtMaximizeSingleMetrics), PtMaximizeError> {
    let raw_items = area_item_combinations(area_item_percent);
    let items =
        crate::maximize::prune_dominated_item_combinations(raw_items, cards, area_item_percent);
    let modes = mode_candidates(cards);
    let mut best = None;
    let mut trace = SingleSearchTrace::new(true, trace_enabled());
    let mut exact_scratch = SinglePtScoreScratch::default();
    let mut saw_searchable_mode = false;
    let mut saw_not_enough_distinct_characters = false;
    for (item_index, selected_items) in items.iter().enumerate() {
        for (mode_index, &mode) in modes.iter().enumerate() {
            trace.current_item = item_index;
            trace.current_mode = mode_index;
            trace.mode_searches += 1;
            let mode_start = trace.enabled.then(Timer::start);
            let before_exact = trace.exact_evaluations;
            match search_team_for_mode_traced(
                cards,
                chart,
                area_item_percent,
                selected_items,
                mode,
                point_bonus_micros,
                minimum_stat,
                scenario,
                &mut best,
                &mut trace,
                &mut exact_scratch,
            ) {
                Ok(()) => saw_searchable_mode = true,
                Err(PtMaximizeError::NotEnoughDistinctCharacters) => {
                    saw_not_enough_distinct_characters = true;
                    continue;
                }
                Err(error) => return Err(error),
            }
            if trace.log_enabled && trace.exact_evaluations - before_exact >= 1_000 {
                eprintln!(
                    "PT single mode complete: item={} mode={} exact_evaluations={} mode_exact_evaluations={} mode_elapsed_ms={:.3}",
                    item_index,
                    mode_index,
                    trace.exact_evaluations,
                    trace.exact_evaluations - before_exact,
                    mode_start.map(|timer| timer.elapsed_ms()).unwrap_or_default(),
                );
            }
        }
    }
    if trace.log_enabled {
        eprintln!(
            "PT single stages: items={} modes={} mode_searches={} allowed_cards={} prepruned_cards={} canonical_cards={} retained_cards={} planned_leaves={} recursive_nodes={} leaves={} minimum_stat_rejects={} exact_evaluations={} mode_meta_upper_bound_prunes={} branch_meta_upper_bound_prunes={} meta_upper_bound_prunes={} exact_upper_bound_prunes={} prepare_ms={:.3} mode_meta_precheck_ms={:.3} preprune_ms={:.3} resolve_ms={:.3} canonical_prune_ms={:.3} suffix_ms={:.3} enumerate_ms={:.3} exact_evaluation_ms={:.3} total_ms={:.3}",
            items.len(),
            modes.len(),
            trace.mode_searches,
            trace.allowed_cards,
            trace.prepruned_cards,
            trace.canonical_cards,
            trace.retained_cards,
            trace.planned_leaves,
            trace.recursive_nodes,
            trace.leaves,
            trace.minimum_stat_rejects,
            trace.exact_evaluations,
            trace.mode_meta_upper_bound_prunes,
            trace.branch_meta_upper_bound_prunes,
            trace.meta_upper_bound_prunes,
            trace.exact_upper_bound_prunes,
            trace.prepare_ms,
            trace.mode_meta_precheck_ms,
            trace.preprune_ms,
            trace.resolve_ms,
            trace.canonical_prune_ms,
            trace.suffix_ms,
            trace.enumerate_ms,
            trace.exact_evaluation_ms,
            trace.total_start.elapsed_ms(),
        );
    }
    let result = match best {
        Some(result) => result,
        None if saw_not_enough_distinct_characters && !saw_searchable_mode => {
            return Err(PtMaximizeError::NotEnoughDistinctCharacters);
        }
        None => return Err(PtMaximizeError::NoResult),
    };
    let metrics = trace.metrics(items.len(), modes.len());
    Ok((result, metrics))
}

pub fn search_team_for_mode(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    point_bonus_micros: &BTreeMap<u32, u64>,
    minimum_stat: Option<i32>,
    scenario: PtMaximizeSearchScenario,
) -> Result<PtMaximizeTeamResult, PtMaximizeError> {
    let mut trace = SingleSearchTrace::new(false, false);
    let mut exact_scratch = SinglePtScoreScratch::default();
    let mut best = None;
    search_team_for_mode_traced(
        cards,
        chart,
        area_item_percent,
        selected_items,
        mode,
        point_bonus_micros,
        minimum_stat,
        scenario,
        &mut best,
        &mut trace,
        &mut exact_scratch,
    )?;
    best.ok_or(PtMaximizeError::NoResult)
}

#[allow(clippy::too_many_arguments)]
fn search_team_for_mode_traced(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    point_bonus_micros: &BTreeMap<u32, u64>,
    minimum_stat: Option<i32>,
    scenario: PtMaximizeSearchScenario,
    best: &mut Option<PtMaximizeTeamResult>,
    trace: &mut SingleSearchTrace,
    exact_scratch: &mut SinglePtScoreScratch,
) -> Result<(), PtMaximizeError> {
    let prepare_start = trace.enabled.then(Timer::start);
    let bonus_application = event_bonus_application(scenario.event_type(), scenario.live_variant());
    let raw_allowed_count = cards.iter().filter(|card| mode.allows(card)).count();
    let mode_meta_precheck_start = trace.enabled.then(Timer::start);
    if !mode_meta_upper_can_beat(
        cards,
        chart,
        area_item_percent,
        selected_items,
        mode,
        point_bonus_micros,
        minimum_stat,
        scenario,
        best.as_ref(),
    )? {
        if trace.enabled {
            trace.allowed_cards += raw_allowed_count as u64;
            trace.mode_meta_upper_bound_prunes += 1;
            trace.mode_meta_precheck_ms += mode_meta_precheck_start
                .map(|timer| timer.elapsed_ms())
                .unwrap_or_default();
            trace.prepare_ms += prepare_start
                .map(|timer| timer.elapsed_ms())
                .unwrap_or_default();
        }
        return Ok(());
    }
    if trace.enabled {
        trace.mode_meta_precheck_ms += mode_meta_precheck_start
            .map(|timer| timer.elapsed_ms())
            .unwrap_or_default();
    }
    let replacement_values =
        (bonus_application == EventBonusApplication::PointMultiplier).then(|| {
            cards
                .iter()
                .map(|card| {
                    point_bonus_micros
                        .get(&card.card_id)
                        .copied()
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
        });
    let preprune_start = trace.enabled.then(Timer::start);
    let branch_meta_supported = matches!(
        scenario,
        PtMaximizeSearchScenario::FullTeam {
            scenario: fixed_scenario
        } if chart.warning.is_empty()
            && !matches!(fixed_scenario, super::FixedTeamPtScenario::Festival { .. })
    );
    let cooperative_branch_supported =
        matches!(scenario, PtMaximizeSearchScenario::Cooperative { .. });
    let full_skill_indices = if cooperative_branch_supported {
        Vec::new()
    } else if best.is_some() && matches!(mode, SongMode::Mixed) && branch_meta_supported {
        cards
            .iter()
            .enumerate()
            .filter_map(|(index, card)| mode.allows(card).then_some(index))
            .collect()
    } else {
        candidate::pruned_card_indices_for_role(
            cards,
            chart,
            area_item_percent,
            selected_items,
            mode,
            SingleCardRole::FullSkill,
            replacement_values.as_deref(),
        )
        .map_err(|error| PtMaximizeError::MedleyCandidate(error.to_string()))?
    };
    let (captain_indices, filler_indices) = if cooperative_branch_supported {
        (
            if best.is_none() {
                candidate::pruned_card_indices_for_role(
                    cards,
                    chart,
                    area_item_percent,
                    selected_items,
                    mode,
                    SingleCardRole::Captain,
                    replacement_values.as_deref(),
                )
                .map_err(|error| PtMaximizeError::MedleyCandidate(error.to_string()))?
            } else {
                // The original contribution graph is safe for a cooperative
                // captain, but rebuilding it for every item/mode costs more than
                // the strict incumbent bounds save. An unpruned captain pool is
                // also safe; role-aware canonical pruning below still removes
                // exact and same-skill duplicates.
                cards
                    .iter()
                    .enumerate()
                    .filter_map(|(index, card)| mode.allows(card).then_some(index))
                    .collect()
            },
            candidate::pruned_card_indices_for_role(
                cards,
                chart,
                area_item_percent,
                selected_items,
                mode,
                SingleCardRole::Filler,
                replacement_values.as_deref(),
            )
            .map_err(|error| PtMaximizeError::MedleyCandidate(error.to_string()))?,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    if trace.enabled {
        trace.preprune_ms += preprune_start
            .map(|timer| timer.elapsed_ms())
            .unwrap_or_default();
    }
    if trace.enabled {
        trace.allowed_cards += raw_allowed_count as u64;
        trace.prepruned_cards += if cooperative_branch_supported {
            captain_indices.len().saturating_add(filler_indices.len())
        } else {
            full_skill_indices.len()
        } as u64;
    }
    let meta_model = DpChartModel::from_chart(chart);
    let resolve_start = trace.enabled.then(Timer::start);
    let (full_skill_groups, captain_groups, filler_groups) = if cooperative_branch_supported {
        (
            None,
            Some(resolve_role_card_groups(
                cards,
                &captain_indices,
                chart,
                &meta_model,
                area_item_percent,
                selected_items,
                mode,
                point_bonus_micros,
                bonus_application,
                SingleCardRole::Captain,
            )?),
            Some(resolve_role_card_groups(
                cards,
                &filler_indices,
                chart,
                &meta_model,
                area_item_percent,
                selected_items,
                mode,
                point_bonus_micros,
                bonus_application,
                SingleCardRole::Filler,
            )?),
        )
    } else {
        (
            Some(resolve_role_card_groups(
                cards,
                &full_skill_indices,
                chart,
                &meta_model,
                area_item_percent,
                selected_items,
                mode,
                point_bonus_micros,
                bonus_application,
                SingleCardRole::FullSkill,
            )?),
            None,
            None,
        )
    };
    if trace.enabled {
        trace.resolve_ms += resolve_start
            .map(|timer| timer.elapsed_ms())
            .unwrap_or_default();
        let role_groups = full_skill_groups
            .iter()
            .chain(captain_groups.iter())
            .chain(filler_groups.iter());
        trace.canonical_cards += role_groups
            .clone()
            .map(|groups| groups.canonical_count)
            .sum::<usize>() as u64;
        trace.retained_cards += role_groups
            .map(|groups| groups.retained_count)
            .sum::<usize>() as u64;
        trace.prepare_ms += prepare_start
            .map(|timer| timer.elapsed_ms())
            .unwrap_or_default();
    }
    if let (Some(captain_groups), Some(filler_groups)) = (captain_groups, filler_groups) {
        if captain_groups.groups.is_empty() || filler_groups.groups.len() < 4 {
            return Err(PtMaximizeError::NotEnoughDistinctCharacters);
        }
        if trace.enabled {
            let planned_leaves =
                planned_cooperative_team_count(&captain_groups.groups, &filler_groups.groups);
            trace.planned_leaves = trace.planned_leaves.saturating_add(planned_leaves);
        }
        let enumerate_start = trace.enabled.then(Timer::start);
        let PtMaximizeSearchScenario::Cooperative {
            scenario: cooperative,
        } = scenario
        else {
            unreachable!("cooperative role pools require a cooperative scenario");
        };
        enumerate_cooperative_teams(
            &captain_groups.groups,
            &filler_groups.groups,
            chart,
            selected_items,
            minimum_stat,
            cooperative,
            best,
            trace,
            exact_scratch,
        )?;
        if trace.enabled {
            trace.enumerate_ms += enumerate_start
                .map(|timer| timer.elapsed_ms())
                .unwrap_or_default();
        }
        return Ok(());
    }

    let groups = full_skill_groups
        .expect("non-cooperative search must have full-skill groups")
        .groups;
    if groups.len() < 5 {
        return Err(PtMaximizeError::NotEnoughDistinctCharacters);
    }
    if trace.enabled {
        let planned_leaves = planned_team_count(&groups);
        trace.planned_leaves = trace.planned_leaves.saturating_add(planned_leaves);
        if trace.log_enabled && planned_leaves >= 1_000 {
            eprintln!(
                "PT single mode start: item={} mode={} mode_value={mode:?} groups={} retained_cards={} planned_leaves={} prepare_ms={:.3}",
                trace.current_item,
                trace.current_mode,
                groups.len(),
                groups.iter().map(Vec::len).sum::<usize>(),
                planned_leaves,
                prepare_start
                    .map(|timer| timer.elapsed_ms())
                    .unwrap_or_default(),
            );
        }
    }
    let mut selected = Vec::with_capacity(5);
    let suffix_start = trace.enabled.then(Timer::start);
    let suffix = suffix_bounds(&groups);
    if trace.enabled {
        trace.suffix_ms += suffix_start
            .map(|timer| timer.elapsed_ms())
            .unwrap_or_default();
    }
    let enumerate_start = trace.enabled.then(Timer::start);
    enumerate_teams(
        &groups,
        &suffix,
        0,
        &mut selected,
        chart,
        selected_items,
        minimum_stat,
        scenario,
        best,
        trace,
        exact_scratch,
    )?;
    if trace.enabled {
        trace.enumerate_ms += enumerate_start
            .map(|timer| timer.elapsed_ms())
            .unwrap_or_default();
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn resolve_role_card_groups(
    cards: &[PreparedCard],
    active_indices: &[usize],
    chart: &Chart,
    meta_model: &DpChartModel,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    point_bonus_micros: &BTreeMap<u32, u64>,
    bonus_application: EventBonusApplication,
    role: SingleCardRole,
) -> Result<RoleCardGroups, PtMaximizeError> {
    let point_bonus = |card_id| {
        if bonus_application == EventBonusApplication::PointMultiplier {
            point_bonus_micros
                .get(&card_id)
                .copied()
                .unwrap_or_default()
        } else {
            0
        }
    };
    let mut by_character = BTreeMap::<u32, Vec<SearchCard>>::new();
    let canonical_count = if role == SingleCardRole::Filler {
        let mut canonical = BTreeMap::new();
        for &card_index in active_indices {
            let card = candidate::resolve_card(
                &cards[card_index],
                area_item_percent,
                selected_items,
                mode,
                role,
            )?;
            let resolved = SearchCard {
                card_id: card.card_id,
                character_id: card.character_id,
                stat: card.stat,
                point_bonus_micros: point_bonus(card.card_id),
                // Filler skills never enter scoring or an upper bound. Keeping the
                // original value only makes the selected card self-contained.
                skill: card.skill,
                best_normal_meta: 0.0,
                captain_meta: 0.0,
            };
            let key = (
                resolved.character_id,
                resolved.stat.to_bits(),
                resolved.point_bonus_micros,
            );
            canonical
                .entry(key)
                .and_modify(|current: &mut SearchCard| {
                    if resolved.card_id < current.card_id {
                        *current = resolved;
                    }
                })
                .or_insert(resolved);
        }
        let canonical_count = canonical.len();
        for card in canonical.into_values() {
            by_character
                .entry(card.character_id)
                .or_default()
                .push(card);
        }
        canonical_count
    } else {
        let mut canonical = BTreeMap::new();
        for &card_index in active_indices {
            let card = candidate::resolve_card(
                &cards[card_index],
                area_item_percent,
                selected_items,
                mode,
                role,
            )?;
            let skill = card.skill;
            let meta = skill_meta_profile(chart, meta_model, skill)?;
            let resolved = SearchCard {
                card_id: card.card_id,
                character_id: card.character_id,
                stat: card.stat,
                point_bonus_micros: point_bonus(card.card_id),
                skill,
                best_normal_meta: meta.best_normal(),
                captain_meta: meta.captain,
            };
            let key = (
                resolved.character_id,
                resolved.stat.to_bits(),
                resolved.point_bonus_micros,
                resolved.skill.duration.to_bits(),
                resolved.skill.score_up.to_bits(),
                resolved.skill.rateup,
            );
            canonical
                .entry(key)
                .and_modify(|current: &mut SearchCard| {
                    if resolved.card_id < current.card_id {
                        *current = resolved;
                    }
                })
                .or_insert(resolved);
        }
        let canonical_count = canonical.len();
        for card in canonical.into_values() {
            by_character
                .entry(card.character_id)
                .or_default()
                .push(card);
        }
        canonical_count
    };

    for cards in by_character.values_mut() {
        cards.sort_by_key(|card| card.card_id);
        *cards = if role == SingleCardRole::Filler {
            prune_filler_dominated_cards(cards)
        } else {
            prune_same_skill_dominated_cards(cards)
        };
    }
    let retained_count = by_character.values().map(Vec::len).sum();
    Ok(RoleCardGroups {
        groups: by_character.into_values().collect(),
        canonical_count,
        retained_count,
    })
}

#[allow(clippy::too_many_arguments)]
fn mode_meta_upper_can_beat(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
    point_bonus_micros: &BTreeMap<u32, u64>,
    minimum_stat: Option<i32>,
    scenario: PtMaximizeSearchScenario,
    current: Option<&PtMaximizeTeamResult>,
) -> Result<bool, PtMaximizeError> {
    let PtMaximizeSearchScenario::FullTeam {
        scenario: fixed_scenario,
    } = scenario
    else {
        return Ok(true);
    };
    if !chart.warning.is_empty()
        || matches!(fixed_scenario, super::FixedTeamPtScenario::Festival { .. })
    {
        return Ok(true);
    }
    if current.is_none() && minimum_stat.is_none() {
        return Ok(true);
    }

    let bonus_application = event_bonus_application(scenario.event_type(), scenario.live_variant());
    let meta_model = DpChartModel::from_chart(chart);
    let mut skill_meta = BTreeMap::<(u64, u64, bool), SkillMetaProfile>::new();
    let mut by_character = BTreeMap::<u32, CharacterModeUpper>::new();
    for card in cards.iter().filter(|card| mode.allows(card)) {
        let skill = mode.resolve_skill(card)?;
        let meta_key = (
            skill.duration.to_bits(),
            skill.score_up.to_bits(),
            skill.rateup,
        );
        let meta = match skill_meta.get(&meta_key).copied() {
            Some(value) => value,
            None => {
                let value = skill_meta_profile(chart, &meta_model, skill)?;
                skill_meta.insert(meta_key, value);
                value
            }
        };
        let upper = CharacterModeUpper {
            stat: card.add_up_stat(
                area_item_percent,
                &selected_items.band,
                &selected_items.attribute,
                selected_items.magazine.as_str(),
            ),
            point_bonus_micros: if bonus_application == EventBonusApplication::PointMultiplier {
                point_bonus_micros
                    .get(&card.card_id)
                    .copied()
                    .unwrap_or_default()
            } else {
                0
            },
            normal_meta: meta.best_normal(),
            captain_meta: meta.captain,
            min_card_id: card.card_id,
        };
        by_character
            .entry(card.character_id)
            .and_modify(|current| {
                current.stat = current.stat.max(upper.stat);
                current.point_bonus_micros =
                    current.point_bonus_micros.max(upper.point_bonus_micros);
                current.normal_meta = current.normal_meta.max(upper.normal_meta);
                current.captain_meta = current.captain_meta.max(upper.captain_meta);
                current.min_card_id = current.min_card_id.min(upper.min_card_id);
            })
            .or_insert(upper);
    }
    if by_character.len() < 5 {
        return Ok(false);
    }

    let mut stat_upper = by_character
        .values()
        .map(|value| value.stat)
        .collect::<Vec<_>>();
    stat_upper.sort_unstable_by(|left, right| right.total_cmp(left));
    let stat = ceil_team_stat_upper(stat_upper.into_iter().take(5));
    if minimum_stat.is_some_and(|minimum| stat < minimum) {
        return Ok(false);
    }
    let Some(current) = current else {
        return Ok(true);
    };

    let mut point_bonus_upper = by_character
        .values()
        .map(|value| value.point_bonus_micros)
        .collect::<Vec<_>>();
    point_bonus_upper.sort_unstable_by(|left, right| right.cmp(left));
    let point_bonus_micros = point_bonus_upper
        .into_iter()
        .take(5)
        .fold(0_u64, u64::saturating_add);
    let mut normal_meta_upper = by_character
        .values()
        .map(|value| value.normal_meta)
        .collect::<Vec<_>>();
    normal_meta_upper.sort_unstable_by(|left, right| right.total_cmp(left));
    let normal_meta = normal_meta_upper.into_iter().take(5).sum::<f64>();
    let captain_meta = by_character
        .values()
        .map(|value| value.captain_meta)
        .fold(0.0_f64, f64::max);
    let score_upper = (stat.max(0) as f64 * (chart.meta.no_skill + normal_meta + captain_meta))
        .ceil()
        .clamp(i32::MIN as f64, i32::MAX as f64) as i32;
    let point_bonus_basis_points =
        ((point_bonus_micros.saturating_add(5_000)) / 10_000).min(u32::MAX as u64) as u32;
    let fixed_scenario = match scenario.with_point_bonus(point_bonus_basis_points) {
        PtMaximizeSearchScenario::FullTeam { scenario } => scenario,
        _ => unreachable!("the full-team branch was checked above"),
    };
    let average_upper = AveragePt::new(
        u128::from(points_for_scenario(score_upper, fixed_scenario)?),
        1,
    )?;
    if average_upper != current.evaluation.average_pt {
        return Ok(average_upper > current.evaluation.average_pt);
    }

    let mut min_team_ids = by_character
        .values()
        .map(|value| value.min_card_id)
        .collect::<Vec<_>>();
    min_team_ids.sort_unstable();
    min_team_ids.truncate(5);
    Ok(min_team_ids.as_slice() <= current.team_card_ids.as_slice())
}

fn planned_team_count(groups: &[Vec<SearchCard>]) -> u128 {
    planned_team_count_for_slots(groups, 5)
}

fn planned_team_count_for_slots(groups: &[Vec<SearchCard>], slots: usize) -> u128 {
    let mut counts = [0_u128; 6];
    counts[0] = 1;
    for group in groups {
        let choices = group.len() as u128;
        for selected in (1..=slots).rev() {
            counts[selected] =
                counts[selected].saturating_add(counts[selected - 1].saturating_mul(choices));
        }
    }
    counts[slots]
}

fn planned_cooperative_team_count(
    captain_groups: &[Vec<SearchCard>],
    filler_groups: &[Vec<SearchCard>],
) -> u128 {
    captain_groups.iter().fold(0_u128, |total, captains| {
        let Some(captain) = captains.first() else {
            return total;
        };
        let available_fillers = filler_groups
            .iter()
            .filter(|group| {
                group
                    .first()
                    .is_some_and(|filler| filler.character_id != captain.character_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        total.saturating_add(
            (captains.len() as u128)
                .saturating_mul(planned_team_count_for_slots(&available_fillers, 4)),
        )
    })
}

fn suffix_bounds(groups: &[Vec<SearchCard>]) -> Vec<SuffixBound> {
    let mut suffix = vec![empty_suffix_bound(); groups.len() + 1];
    for group_index in (0..groups.len()).rev() {
        let next = suffix[group_index + 1].clone();
        let group = &groups[group_index];
        let group_stat = group
            .iter()
            .map(|card| card.stat)
            .fold(f64::NEG_INFINITY, f64::max);
        let group_point_bonus = group
            .iter()
            .map(|card| card.point_bonus_micros)
            .max()
            .unwrap_or_default();
        let group_normal_meta = group
            .iter()
            .map(|card| card.best_normal_meta)
            .fold(f64::NEG_INFINITY, f64::max);
        let group_captain_meta = group
            .iter()
            .map(|card| card.captain_meta)
            .fold(0.0_f64, f64::max);
        let group_min_card_id = group
            .iter()
            .map(|card| card.card_id)
            .min()
            .unwrap_or(u32::MAX);
        let mut current = next.clone();
        current.remaining_groups += 1;
        current.captain_meta = current.captain_meta.max(group_captain_meta);
        for slots in 1..=5.min(current.remaining_groups) {
            if next.remaining_groups < slots - 1 {
                continue;
            }
            current.stat_by_slots[slots] =
                current.stat_by_slots[slots].max(group_stat + next.stat_by_slots[slots - 1]);
            current.point_bonus_by_slots[slots] = current.point_bonus_by_slots[slots]
                .max(group_point_bonus.saturating_add(next.point_bonus_by_slots[slots - 1]));
            current.normal_meta_by_slots[slots] = current.normal_meta_by_slots[slots]
                .max(group_normal_meta + next.normal_meta_by_slots[slots - 1]);

            let mut take_ids = next.min_card_ids_by_slots[slots - 1];
            take_ids[slots - 1] = group_min_card_id;
            take_ids.sort_unstable();
            if take_ids < current.min_card_ids_by_slots[slots] {
                current.min_card_ids_by_slots[slots] = take_ids;
            }
        }
        suffix[group_index] = current;
    }
    suffix
}

fn empty_suffix_bound() -> SuffixBound {
    let mut stat_by_slots = [f64::NEG_INFINITY; 6];
    stat_by_slots[0] = 0.0;
    let mut normal_meta_by_slots = [f64::NEG_INFINITY; 6];
    normal_meta_by_slots[0] = 0.0;
    let mut min_card_ids_by_slots = [[u32::MAX; 5]; 6];
    min_card_ids_by_slots[0] = [u32::MAX; 5];
    SuffixBound {
        remaining_groups: 0,
        stat_by_slots,
        point_bonus_by_slots: [0; 6],
        normal_meta_by_slots,
        captain_meta: 0.0,
        min_card_ids_by_slots,
    }
}

#[allow(clippy::too_many_arguments)]
fn branch_meta_can_beat(
    selected: &[SearchCard],
    suffix: &SuffixBound,
    needed: usize,
    chart: &Chart,
    minimum_stat: Option<i32>,
    scenario: PtMaximizeSearchScenario,
    current: Option<&PtMaximizeTeamResult>,
) -> Result<bool, PtMaximizeError> {
    let Some(current) = current else {
        return Ok(true);
    };
    let PtMaximizeSearchScenario::FullTeam {
        scenario: fixed_scenario,
    } = scenario
    else {
        return Ok(true);
    };
    if !chart.warning.is_empty()
        || matches!(fixed_scenario, super::FixedTeamPtScenario::Festival { .. })
    {
        return Ok(true);
    }

    let selected_stat = selected.iter().map(|card| card.stat).sum::<f64>();
    let stat = ceil_team_stat_upper([selected_stat, suffix.stat_by_slots[needed]]);
    if minimum_stat.is_some_and(|minimum| stat < minimum) {
        return Ok(false);
    }
    let selected_normal_meta = selected
        .iter()
        .map(|card| card.best_normal_meta)
        .sum::<f64>();
    let normal_meta = selected_normal_meta + suffix.normal_meta_by_slots[needed];
    let captain_meta = selected
        .iter()
        .map(|card| card.captain_meta)
        .fold(suffix.captain_meta, f64::max);
    let score_upper = (stat.max(0) as f64 * (chart.meta.no_skill + normal_meta + captain_meta))
        .ceil()
        .clamp(i32::MIN as f64, i32::MAX as f64) as i32;

    let point_bonus_micros = selected
        .iter()
        .map(|card| card.point_bonus_micros)
        .sum::<u64>()
        .saturating_add(suffix.point_bonus_by_slots[needed]);
    let point_bonus_basis_points =
        ((point_bonus_micros.saturating_add(5_000)) / 10_000).min(u32::MAX as u64) as u32;
    let fixed_scenario = match scenario.with_point_bonus(point_bonus_basis_points) {
        PtMaximizeSearchScenario::FullTeam { scenario } => scenario,
        _ => unreachable!("the full-team branch was checked above"),
    };
    let pt_upper = AveragePt::new(
        u128::from(points_for_scenario(score_upper, fixed_scenario)?),
        1,
    )?;
    if pt_upper != current.evaluation.average_pt {
        return Ok(pt_upper > current.evaluation.average_pt);
    }

    let mut min_team_ids = [u32::MAX; 5];
    for (index, card) in selected.iter().enumerate() {
        min_team_ids[index] = card.card_id;
    }
    min_team_ids[selected.len()..selected.len() + needed]
        .copy_from_slice(&suffix.min_card_ids_by_slots[needed][..needed]);
    min_team_ids.sort_unstable();
    Ok(min_team_ids.as_slice() <= current.team_card_ids.as_slice())
}

fn prune_same_skill_dominated_cards(cards: &[SearchCard]) -> Vec<SearchCard> {
    cards
        .iter()
        .copied()
        .filter(|candidate| {
            !cards.iter().any(|other| {
                same_skill(*candidate, *other)
                    && other.stat >= candidate.stat
                    && other.point_bonus_micros >= candidate.point_bonus_micros
                    && (other.stat > candidate.stat
                        || other.point_bonus_micros > candidate.point_bonus_micros
                        || other.card_id < candidate.card_id)
            })
        })
        .collect()
}

fn same_skill(left: SearchCard, right: SearchCard) -> bool {
    left.skill.duration.to_bits() == right.skill.duration.to_bits()
        && left.skill.score_up.to_bits() == right.skill.score_up.to_bits()
        && left.skill.rateup == right.skill.rateup
}

fn prune_filler_dominated_cards(cards: &[SearchCard]) -> Vec<SearchCard> {
    cards
        .iter()
        .copied()
        .filter(|candidate| {
            !cards.iter().any(|other| {
                other.card_id != candidate.card_id
                    && other.stat >= candidate.stat
                    && other.point_bonus_micros >= candidate.point_bonus_micros
                    && (other.stat > candidate.stat
                        || other.point_bonus_micros > candidate.point_bonus_micros
                        || other.card_id < candidate.card_id)
            })
        })
        .collect()
}

fn ceil_team_stat_upper(stats: impl IntoIterator<Item = f64>) -> i32 {
    stats
        .into_iter()
        .sum::<f64>()
        .ceil()
        .clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

fn cooperative_branch_upper(
    fever_chart: &Chart,
    meta_model: &DpChartModel,
    captain: TeamCardSkill,
    teammate_skills: [TeamCardSkill; 4],
    scenario: CooperativePtScenario,
) -> Result<CooperativeBranchUpper, PtMaximizeError> {
    let skills = [
        captain,
        teammate_skills[0],
        teammate_skills[1],
        teammate_skills[2],
        teammate_skills[3],
    ];
    let mut normal_meta = 0.0;
    let mut captain_meta = 0.0_f64;
    for skill in skills {
        let profile = skill_meta_profile(fever_chart, meta_model, skill)?;
        normal_meta += profile.best_normal();
        captain_meta = captain_meta.max(profile.captain);
    }
    let score_factor = fever_chart.meta.no_skill + normal_meta + captain_meta;
    let teammate_score_upper = scenario
        .teammates
        .iter()
        .map(|teammate| {
            (teammate.expected_stat.max(0) as f64 * score_factor)
                .ceil()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i64
        })
        .fold(0_i64, i64::saturating_add);
    Ok(CooperativeBranchUpper {
        score_factor,
        teammate_score_upper,
    })
}

#[allow(clippy::too_many_arguments)]
fn enumerate_cooperative_teams(
    captain_groups: &[Vec<SearchCard>],
    filler_groups: &[Vec<SearchCard>],
    chart: &Chart,
    selected_items: &SelectedAreaItems,
    minimum_stat: Option<i32>,
    scenario: CooperativePtScenario,
    best: &mut Option<PtMaximizeTeamResult>,
    trace: &mut SingleSearchTrace,
    exact_scratch: &mut SinglePtScoreScratch,
) -> Result<(), PtMaximizeError> {
    let mut fever_chart = chart.clone();
    fever_chart.init_with_fever(fever_chart.combo, false)?;
    let fever_meta_model = DpChartModel::from_chart(&fever_chart);
    let teammate_skills = [
        scenario.teammates[0].skill(1),
        scenario.teammates[1].skill(2),
        scenario.teammates[2].skill(3),
        scenario.teammates[3].skill(4),
    ];
    for captain_group in captain_groups {
        let Some(captain_character_id) = captain_group.first().map(|captain| captain.character_id)
        else {
            continue;
        };
        let available_fillers = filler_groups
            .iter()
            .filter_map(|group| {
                group
                    .first()
                    .is_some_and(|filler| filler.character_id != captain_character_id)
                    .then_some(group.clone())
            })
            .collect::<Vec<_>>();
        if available_fillers.len() < 4 {
            continue;
        }
        let suffix = suffix_bounds(&available_fillers);
        for &captain in captain_group {
            let upper = cooperative_branch_upper(
                &fever_chart,
                &fever_meta_model,
                captain.skill,
                teammate_skills,
                scenario,
            )?;
            let mut selected = Vec::with_capacity(5);
            selected.push(captain);
            enumerate_cooperative_fillers(
                &available_fillers,
                &suffix,
                0,
                &mut selected,
                chart,
                selected_items,
                minimum_stat,
                scenario,
                upper,
                best,
                trace,
                exact_scratch,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn enumerate_cooperative_fillers(
    groups: &[Vec<SearchCard>],
    suffix: &[SuffixBound],
    group_start: usize,
    selected: &mut Vec<SearchCard>,
    chart: &Chart,
    selected_items: &SelectedAreaItems,
    minimum_stat: Option<i32>,
    scenario: CooperativePtScenario,
    upper: CooperativeBranchUpper,
    best: &mut Option<PtMaximizeTeamResult>,
    trace: &mut SingleSearchTrace,
    exact_scratch: &mut SinglePtScoreScratch,
) -> Result<(), PtMaximizeError> {
    trace.recursive_nodes += u64::from(trace.enabled);
    if selected.len() == 5 {
        trace.leaves += u64::from(trace.enabled);
        return evaluate_cooperative_selection(
            selected,
            chart,
            selected_items,
            minimum_stat,
            scenario,
            upper,
            best,
            trace,
            exact_scratch,
        );
    }
    let needed = 5 - selected.len();
    if groups.len().saturating_sub(group_start) < needed {
        return Ok(());
    }
    let selected_stat = selected.iter().map(|card| card.stat).sum::<f64>();
    let stat_upper =
        ceil_team_stat_upper([selected_stat, suffix[group_start].stat_by_slots[needed]]);
    if minimum_stat.is_some_and(|minimum| stat_upper < minimum) {
        trace.branch_meta_upper_bound_prunes += u64::from(trace.enabled);
        return Ok(());
    }
    if let Some(current) = best.as_ref() {
        let point_bonus_micros = selected
            .iter()
            .map(|card| card.point_bonus_micros)
            .sum::<u64>()
            .saturating_add(suffix[group_start].point_bonus_by_slots[needed]);
        let point_bonus_basis_points =
            ((point_bonus_micros.saturating_add(5_000)) / 10_000).min(u32::MAX as u64) as u32;
        let personal_score_upper = (stat_upper.max(0) as f64 * upper.score_factor)
            .ceil()
            .clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        let total_score_upper =
            i64::from(personal_score_upper).saturating_add(upper.teammate_score_upper);
        let pt_upper = cooperative_points(
            scenario.event_type,
            personal_score_upper,
            total_score_upper,
            point_bonus_basis_points,
            0,
        )?;
        if AveragePt::new(u128::from(pt_upper), 1)? < current.evaluation.average_pt {
            trace.branch_meta_upper_bound_prunes += u64::from(trace.enabled);
            return Ok(());
        }
    }
    for group_index in group_start..=groups.len() - needed {
        for &card in &groups[group_index] {
            selected.push(card);
            enumerate_cooperative_fillers(
                groups,
                suffix,
                group_index + 1,
                selected,
                chart,
                selected_items,
                minimum_stat,
                scenario,
                upper,
                best,
                trace,
                exact_scratch,
            )?;
            selected.pop();
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn evaluate_cooperative_selection(
    selected: &[SearchCard],
    chart: &Chart,
    selected_items: &SelectedAreaItems,
    minimum_stat: Option<i32>,
    mut scenario: CooperativePtScenario,
    upper: CooperativeBranchUpper,
    best: &mut Option<PtMaximizeTeamResult>,
    trace: &mut SingleSearchTrace,
    exact_scratch: &mut SinglePtScoreScratch,
) -> Result<(), PtMaximizeError> {
    let stat = floor_team_stat(selected.iter().map(|card| card.stat));
    if minimum_stat.is_some_and(|minimum| stat < minimum) {
        trace.minimum_stat_rejects += u64::from(trace.enabled);
        return Ok(());
    }
    let point_bonus_micros = selected
        .iter()
        .map(|card| card.point_bonus_micros)
        .sum::<u64>();
    let point_bonus_basis_points =
        ((point_bonus_micros.saturating_add(5_000)) / 10_000).min(u32::MAX as u64) as u32;
    scenario.point_bonus_basis_points = point_bonus_basis_points;
    if let Some(current) = best.as_ref() {
        let personal_score_upper = (stat.max(0) as f64 * upper.score_factor)
            .ceil()
            .clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        let total_score_upper =
            i64::from(personal_score_upper).saturating_add(upper.teammate_score_upper);
        let pt_upper = cooperative_points(
            scenario.event_type,
            personal_score_upper,
            total_score_upper,
            point_bonus_basis_points,
            0,
        )?;
        if AveragePt::new(u128::from(pt_upper), 1)? < current.evaluation.average_pt {
            trace.meta_upper_bound_prunes += u64::from(trace.enabled);
            return Ok(());
        }
    }
    let evaluation_start = trace.enabled.then(Timer::start);
    let mut evaluation = evaluate_cooperative_captain_with_scratch(
        chart,
        selected[0].skill,
        0,
        stat,
        scenario,
        &mut exact_scratch.cooperative,
    )?;
    finish_exact_trace(trace, evaluation_start);

    let (team_card_ids, captain_index) = sorted_team_with_captain_index(
        selected.iter().map(|card| card.card_id).collect(),
        evaluation.captain_card_id,
    );
    evaluation.captain_index = captain_index;
    let candidate = PtMaximizeTeamResult {
        captain_card_id: evaluation.captain_card_id,
        team_card_ids,
        total_stat: stat,
        point_bonus_basis_points,
        items: selected_items.clone(),
        evaluation,
    };
    if best
        .as_ref()
        .is_none_or(|current| better(&candidate, current))
    {
        *best = Some(candidate);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn enumerate_teams(
    groups: &[Vec<SearchCard>],
    suffix: &[SuffixBound],
    group_start: usize,
    selected: &mut Vec<SearchCard>,
    chart: &Chart,
    selected_items: &SelectedAreaItems,
    minimum_stat: Option<i32>,
    scenario: PtMaximizeSearchScenario,
    best: &mut Option<PtMaximizeTeamResult>,
    trace: &mut SingleSearchTrace,
    exact_scratch: &mut SinglePtScoreScratch,
) -> Result<(), PtMaximizeError> {
    trace.recursive_nodes += u64::from(trace.enabled);
    if selected.len() == 5 {
        trace.leaves += u64::from(trace.enabled);
        evaluate_selected_team(
            selected,
            chart,
            selected_items,
            minimum_stat,
            scenario,
            best,
            trace,
            exact_scratch,
        )?;
        return Ok(());
    }
    let needed = 5 - selected.len();
    if groups.len().saturating_sub(group_start) < needed {
        return Ok(());
    }
    if !branch_meta_can_beat(
        selected,
        &suffix[group_start],
        needed,
        chart,
        minimum_stat,
        scenario,
        best.as_ref(),
    )? {
        trace.branch_meta_upper_bound_prunes += u64::from(trace.enabled);
        return Ok(());
    }
    for group_idx in group_start..=groups.len() - needed {
        for &card in &groups[group_idx] {
            selected.push(card);
            enumerate_teams(
                groups,
                suffix,
                group_idx + 1,
                selected,
                chart,
                selected_items,
                minimum_stat,
                scenario,
                best,
                trace,
                exact_scratch,
            )?;
            selected.pop();
        }
    }
    Ok(())
}

fn evaluate_selected_team(
    selected: &[SearchCard],
    chart: &Chart,
    selected_items: &SelectedAreaItems,
    minimum_stat: Option<i32>,
    scenario: PtMaximizeSearchScenario,
    best: &mut Option<PtMaximizeTeamResult>,
    trace: &mut SingleSearchTrace,
    exact_scratch: &mut SinglePtScoreScratch,
) -> Result<(), PtMaximizeError> {
    let stat = floor_team_stat(selected.iter().map(|card| card.stat));
    if minimum_stat.is_some_and(|minimum| stat < minimum) {
        trace.minimum_stat_rejects += u64::from(trace.enabled);
        return Ok(());
    }
    let point_bonus_micros = selected
        .iter()
        .map(|card| card.point_bonus_micros)
        .sum::<u64>();
    let point_bonus_basis_points =
        ((point_bonus_micros.saturating_add(5_000)) / 10_000).min(u32::MAX as u64) as u32;
    let scenario = scenario.with_point_bonus(point_bonus_basis_points);
    let team = std::array::from_fn(|index| selected[index].skill);
    let evaluation_start = trace.enabled.then(Timer::start);
    let mut trace_finished = false;
    let mut evaluation = match scenario {
        PtMaximizeSearchScenario::FullTeam { scenario } => {
            let cutoff = best.as_ref().map(|current| FullTeamPtCutoff {
                average_pt: current.evaluation.average_pt,
                // Equal PT can still improve the secondary objective (average
                // score), regardless of the canonical card-id tie-break.
                equal_can_win: true,
            });
            match evaluate_full_team_summary_with_scratch(
                chart,
                &team,
                stat,
                false,
                scenario,
                cutoff,
                &mut exact_scratch.full_team,
            )? {
                FullTeamPtSummaryOutcome::Summary(summary) => {
                    finish_exact_trace(trace, evaluation_start);
                    trace_finished = true;
                    if !summary_better(&summary, selected, best.as_ref()) {
                        return Ok(());
                    }
                    materialize_full_team_summary(summary, &mut exact_scratch.full_team)
                }
                FullTeamPtSummaryOutcome::Queued => evaluate_full_team_with_scratch(
                    chart,
                    &team,
                    stat,
                    false,
                    scenario,
                    &mut exact_scratch.full_team,
                )?,
                FullTeamPtSummaryOutcome::PrunedByMetaUpperBound => {
                    trace.meta_upper_bound_prunes += u64::from(trace.enabled);
                    finish_exact_trace(trace, evaluation_start);
                    return Ok(());
                }
                FullTeamPtSummaryOutcome::PrunedByExactUpperBound => {
                    trace.exact_upper_bound_prunes += u64::from(trace.enabled);
                    finish_exact_trace(trace, evaluation_start);
                    return Ok(());
                }
            }
        }
        PtMaximizeSearchScenario::Cooperative { scenario } => {
            evaluate_cooperative_team_with_scratch(
                chart,
                &team,
                stat,
                scenario,
                &mut exact_scratch.cooperative,
            )?
        }
        PtMaximizeSearchScenario::Medley => return Err(PtMaximizeError::NoResult),
    };
    if !trace_finished {
        finish_exact_trace(trace, evaluation_start);
    }
    let (team_card_ids, captain_index) = sorted_team_with_captain_index(
        selected.iter().map(|card| card.card_id).collect(),
        evaluation.captain_card_id,
    );
    evaluation.captain_index = captain_index;
    let candidate = PtMaximizeTeamResult {
        captain_card_id: evaluation.captain_card_id,
        team_card_ids,
        total_stat: stat,
        point_bonus_basis_points,
        items: selected_items.clone(),
        evaluation,
    };
    if best
        .as_ref()
        .is_none_or(|current| better(&candidate, current))
    {
        *best = Some(candidate);
    }
    Ok(())
}

fn finish_exact_trace(trace: &mut SingleSearchTrace, evaluation_start: Option<Timer>) {
    if trace.enabled {
        trace.exact_evaluations += 1;
        trace.exact_evaluation_ms += evaluation_start
            .map(|timer| timer.elapsed_ms())
            .unwrap_or_default();
        trace.trace_progress();
    }
}

fn summary_better(
    summary: &FullTeamPtSummary,
    selected: &[SearchCard],
    current: Option<&PtMaximizeTeamResult>,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    if summary.average_pt != current.evaluation.average_pt {
        return summary.average_pt > current.evaluation.average_pt;
    }
    let score_order = compare_nonnegative_averages(
        i128::from(summary.score_sum),
        summary.sample_count,
        i128::from(current.evaluation.score_distribution.score_sum),
        current.evaluation.score_distribution.sample_count,
    );
    if score_order != std::cmp::Ordering::Equal {
        return score_order == std::cmp::Ordering::Greater;
    }
    let mut team_card_ids: [u32; 5] = std::array::from_fn(|index| selected[index].card_id);
    team_card_ids.sort_unstable();
    (team_card_ids.as_slice(), summary.captain_card_id)
        < (current.team_card_ids.as_slice(), current.captain_card_id)
}

fn trace_enabled() -> bool {
    std::env::var_os("BANGDREAM_OPTIMIZE_PT_TRACE").is_some()
}

fn better(candidate: &PtMaximizeTeamResult, current: &PtMaximizeTeamResult) -> bool {
    let candidate_value: AveragePt = candidate.evaluation.average_pt;
    let current_value: AveragePt = current.evaluation.average_pt;
    if candidate_value != current_value {
        return candidate_value > current_value;
    }
    let score_order = compare_nonnegative_averages(
        i128::from(candidate.evaluation.score_distribution.score_sum),
        candidate.evaluation.score_distribution.sample_count,
        i128::from(current.evaluation.score_distribution.score_sum),
        current.evaluation.score_distribution.sample_count,
    );
    score_order == std::cmp::Ordering::Greater
        || (score_order == std::cmp::Ordering::Equal
            && (
                candidate.team_card_ids.as_slice(),
                candidate.captain_card_id,
            ) < (current.team_card_ids.as_slice(), current.captain_card_id))
}

fn sorted_team_with_captain_index(
    mut team_card_ids: Vec<u32>,
    captain_card_id: u32,
) -> (Vec<u32>, usize) {
    team_card_ids.sort_unstable();
    let captain_index = team_card_ids
        .binary_search(&captain_card_id)
        .expect("the captain must belong to the selected team");
    (team_card_ids, captain_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        evaluate_cooperative_team, Attribute, ChartNode, ChartNodeType, CooperativeLeaderSelection,
        CooperativeTeammate, FixedTeamPtEvaluation, FixedTeamPtScenario, Magazine, ScoreHistogram,
        ScoreUp, StatValue,
    };

    #[test]
    fn sorted_team_remaps_captain_index() {
        let (team, captain_index) =
            sorted_team_with_captain_index(vec![1958, 1768, 1988, 2178, 2276], 1958);
        assert_eq!(team, vec![1768, 1958, 1988, 2178, 2276]);
        assert_eq!(captain_index, 1);
    }

    fn card(card_id: u32, character_id: u32, stat: f64, score_up: f64) -> PreparedCard {
        PreparedCard {
            card_id,
            character_id,
            band_id: 1,
            rarity: 4,
            attribute: Attribute::Cool,
            level: 60,
            training: true,
            illust_training_status: true,
            episodes: [true, true],
            limit_break_rank: 0,
            skill_level: 5,
            stat: StatValue {
                performance: stat,
                technique: 0.0,
                visual: 0.0,
            },
            event_add_stat: StatValue::zero(),
            skill: TeamCardSkill {
                card_id,
                duration: 3.0,
                score_up,
                rateup: false,
            },
            score_up: ScoreUp {
                default: score_up,
                unification_activate_effect_value: None,
                unification_activate_condition_band_id: None,
                unification_activate_condition_type: None,
            },
        }
    }

    #[test]
    fn equal_average_pt_prefers_higher_average_score() {
        let result = |score_sum| PtMaximizeTeamResult {
            team_card_ids: vec![1, 2, 3, 4, 5],
            captain_card_id: 1,
            total_stat: 100,
            point_bonus_basis_points: 0,
            items: SelectedAreaItems {
                band: "1".to_owned(),
                attribute: "cool".to_owned(),
                magazine: Magazine::Performance,
            },
            evaluation: FixedTeamPtEvaluation {
                event_type: EventType::LiveTry,
                live_variant: LiveVariant::Solo,
                captain_index: 0,
                captain_card_id: 1,
                score_distribution: ScoreHistogram {
                    entries: vec![(score_sum, 1)],
                    score_sum: i64::from(score_sum),
                    min_score: score_sum,
                    max_score: score_sum,
                    sample_count: 1,
                },
                average_pt: AveragePt::new(500, 1).unwrap(),
                min_pt: 500,
                max_pt: 500,
                average_cp_gain: None,
                challenge_cp_cost: None,
            },
        };

        assert!(better(&result(1_001), &result(1_000)));
        assert!(!better(&result(1_000), &result(1_001)));
    }

    #[test]
    fn exhaustive_mode_search_can_prefer_point_bonus_over_raw_score() {
        let mut nodes = Vec::new();
        for activation in 0..6 {
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: activation as f64 * 10.0,
            });
            nodes.push(ChartNode {
                node_type: ChartNodeType::Node,
                time: activation as f64 * 10.0 + 1.0,
            });
        }
        let mut chart = Chart::new(25, nodes);
        chart.init(0, false).unwrap();
        let mut cards = (1..=5)
            .map(|id| card(id, id, 1_000.0, 0.5))
            .collect::<Vec<_>>();
        cards.push(card(6, 1, 1_400.0, 0.5));
        let bonuses = BTreeMap::from([(1, 100_000_000)]);
        let items = SelectedAreaItems {
            band: "1".to_owned(),
            attribute: "cool".to_owned(),
            magazine: Magazine::Performance,
        };
        let result = search_team_for_mode(
            &cards,
            &chart,
            &AreaItemPercent::empty(),
            &items,
            SongMode::Mixed,
            &bonuses,
            None,
            PtMaximizeSearchScenario::FullTeam {
                scenario: FixedTeamPtScenario::Solo {
                    event_type: EventType::Challenge,
                    point_bonus_basis_points: 0,
                    mission_support_pt_bonus: 0,
                },
            },
        )
        .unwrap();

        assert!(result.team_card_ids.contains(&1));
        assert!(!result.team_card_ids.contains(&6));
        assert_eq!(result.point_bonus_basis_points, 10_000);
    }

    #[test]
    fn same_skill_prune_keeps_stat_bonus_tradeoffs() {
        let skill = TeamCardSkill {
            card_id: 1,
            duration: 5.0,
            score_up: 1.0,
            rateup: false,
        };
        let make = |card_id, stat, point_bonus_micros| SearchCard {
            card_id,
            character_id: 1,
            stat,
            point_bonus_micros,
            skill: TeamCardSkill { card_id, ..skill },
            best_normal_meta: 1.0,
            captain_meta: 1.0,
        };
        let cards = [
            make(1, 100.0, 100),
            make(2, 110.0, 90),
            make(3, 110.0, 100),
            make(4, 90.0, 110),
        ];
        let kept = prune_same_skill_dominated_cards(&cards)
            .into_iter()
            .map(|card| card.card_id)
            .collect::<Vec<_>>();
        assert_eq!(kept, vec![3, 4]);
    }

    #[test]
    fn filler_prune_ignores_skill_but_keeps_stat_bonus_tradeoffs() {
        let make = |card_id, stat, point_bonus_micros, score_up| SearchCard {
            card_id,
            character_id: 1,
            stat,
            point_bonus_micros,
            skill: TeamCardSkill {
                card_id,
                duration: 5.0,
                score_up,
                rateup: false,
            },
            best_normal_meta: score_up,
            captain_meta: score_up,
        };
        let cards = [
            make(1, 100.0, 100, 2.0),
            make(2, 110.0, 100, 0.1),
            make(3, 90.0, 110, 0.1),
        ];
        let kept = prune_filler_dominated_cards(&cards)
            .into_iter()
            .map(|card| card.card_id)
            .collect::<Vec<_>>();
        assert_eq!(kept, vec![2, 3]);
    }

    #[test]
    fn cooperative_captain_fill_search_matches_exhaustive_teams() {
        let mut nodes = Vec::new();
        for activation in 0..6 {
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: activation as f64 * 10.0,
            });
            nodes.push(ChartNode {
                node_type: ChartNodeType::Node,
                time: activation as f64 * 10.0 + 1.0,
            });
        }
        let mut chart = Chart::new(25, nodes);
        chart.init(0, false).unwrap();
        let cards = vec![
            card(1, 1, 1_000.0, 0.60),
            card(2, 1, 1_100.0, 0.40),
            card(3, 2, 1_020.0, 0.50),
            card(4, 3, 1_030.0, 0.70),
            card(5, 4, 1_040.0, 0.80),
            card(6, 5, 1_050.0, 0.90),
            card(7, 6, 1_060.0, 1.00),
        ];
        let bonuses = BTreeMap::from([
            (1, 30_000_000),
            (2, 10_000_000),
            (3, 20_000_000),
            (4, 10_000_000),
            (5, 20_000_000),
            (6, 10_000_000),
            (7, 20_000_000),
        ]);
        let items = SelectedAreaItems {
            band: "1".to_owned(),
            attribute: "cool".to_owned(),
            magazine: Magazine::Performance,
        };
        let cooperative = CooperativePtScenario {
            event_type: EventType::LiveTry,
            teammates: [CooperativeTeammate {
                expected_stat: 5_000,
                leader_score_up: 1.30,
                leader_skill_duration: 7.0,
            }; 4],
            leader_selection: CooperativeLeaderSelection::MaxStat,
            point_bonus_basis_points: 0,
        };
        let result = search_team_for_mode(
            &cards,
            &chart,
            &AreaItemPercent::empty(),
            &items,
            SongMode::Mixed,
            &bonuses,
            None,
            PtMaximizeSearchScenario::Cooperative {
                scenario: cooperative,
            },
        )
        .unwrap();

        let mut brute = None;
        for a in 0..cards.len() {
            for b in a + 1..cards.len() {
                for c in b + 1..cards.len() {
                    for d in c + 1..cards.len() {
                        for e in d + 1..cards.len() {
                            let indices = [a, b, c, d, e];
                            let mut characters =
                                indices.map(|index| cards[index].character_id).to_vec();
                            characters.sort_unstable();
                            characters.dedup();
                            if characters.len() != 5 {
                                continue;
                            }
                            let stat = floor_team_stat(indices.iter().map(|&index| {
                                cards[index].add_up_stat(
                                    &AreaItemPercent::empty(),
                                    &items.band,
                                    &items.attribute,
                                    items.magazine.as_str(),
                                )
                            }));
                            let point_bonus_micros = indices
                                .iter()
                                .map(|&index| {
                                    bonuses.get(&cards[index].card_id).copied().unwrap_or(0)
                                })
                                .sum::<u64>();
                            let point_bonus_basis_points =
                                ((point_bonus_micros + 5_000) / 10_000) as u32;
                            let mut scenario = cooperative;
                            scenario.point_bonus_basis_points = point_bonus_basis_points;
                            let mut team_card_ids =
                                indices.map(|index| cards[index].card_id).to_vec();
                            team_card_ids.sort_unstable();
                            for captain_position in 0..5 {
                                let team = std::array::from_fn(|position| {
                                    let source_position = if position == 0 {
                                        captain_position
                                    } else if position <= captain_position {
                                        position - 1
                                    } else {
                                        position
                                    };
                                    cards[indices[source_position]].skill
                                });
                                let evaluation =
                                    evaluate_cooperative_team(&chart, &team, stat, scenario)
                                        .unwrap();
                                let candidate = PtMaximizeTeamResult {
                                    captain_card_id: evaluation.captain_card_id,
                                    team_card_ids: team_card_ids.clone(),
                                    total_stat: stat,
                                    point_bonus_basis_points,
                                    items: items.clone(),
                                    evaluation,
                                };
                                if brute
                                    .as_ref()
                                    .is_none_or(|current| better(&candidate, current))
                                {
                                    brute = Some(candidate);
                                }
                            }
                        }
                    }
                }
            }
        }
        let brute = brute.unwrap();
        assert_eq!(result.team_card_ids, brute.team_card_ids);
        assert_eq!(result.captain_card_id, brute.captain_card_id);
        assert_eq!(result.total_stat, brute.total_stat);
        assert_eq!(
            result.point_bonus_basis_points,
            brute.point_bonus_basis_points
        );
        assert_eq!(result.evaluation.average_pt, brute.evaluation.average_pt);
    }
}
