use super::enumeration::{
    compact_raw_candidate_masks, enumerate_signature_pool, raw_candidate_solver_input_for_indices,
    raw_candidate_used_card_count, CandidateIncumbentFilter, RawCandidateSolverInput,
    SignatureEnumerationStats,
};
use super::prune::{
    global_prune_stats, medley_card_prune_profiles, seed_signatures, signature_candidate_pools,
    signature_label, trace_medley_prune_stats, trace_score_contribution_cover_diagnostics,
    trace_signature_pool_stats, MedleyPruneSignature, MedleyPruneTrace,
};
use super::scoring::{RawTeamCandidate, ResolvedCandidateBuildProfile, SkillMetaCache};
use crate::medley::candidate::TeamCandidate;
use crate::model::chart::{Chart, ChartError};
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::SelectedAreaItems;
use crate::timing::{optional_elapsed_ms, Timer};
use bangdream_optimize_medley_solver::{
    solve_medley_wide_with, solve_medley_with, MedleySolverError, MedleySolverPreference,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

const TEAM_SIZE: usize = 5;
const MEDLEY_TEAM_COUNT: usize = 3;
const INCUMBENT_REFRESH_MIN_CANDIDATES: usize = 32_768;
const INCUMBENT_REFRESH_MAX_SOLVER_CANDIDATES: usize = 4_096;

#[derive(Debug, Default, Clone, Copy)]
struct EnumerationTrace {
    branch_upper_bound_pruned: usize,
    leaf_checks: usize,
    final_layer_card_checks: usize,
    final_layer_signature_passes: usize,
    final_layer_upper_bound_rejects: usize,
    signature_rejects: usize,
    candidate_builds: usize,
    candidate_filter_rejects: usize,
    build_candidate_ms: f64,
    candidate_filter_ms: f64,
    build_profile: ResolvedCandidateBuildProfile,
}

impl EnumerationTrace {
    fn add(&mut self, stats: &SignatureEnumerationStats) {
        self.branch_upper_bound_pruned += stats.branch_upper_bound_pruned;
        self.leaf_checks += stats.leaf_checks;
        self.final_layer_card_checks += stats.final_layer_card_checks;
        self.final_layer_signature_passes += stats.final_layer_signature_passes;
        self.final_layer_upper_bound_rejects += stats.final_layer_upper_bound_rejects;
        self.signature_rejects += stats.signature_rejects;
        self.candidate_builds += stats.candidate_builds;
        self.candidate_filter_rejects += stats.candidate_filter_rejects;
        self.build_candidate_ms += stats.build_candidate_ms;
        self.candidate_filter_ms += stats.candidate_filter_ms;
        self.build_profile.add(&stats.build_profile);
    }
}

#[derive(Debug, Error)]
pub enum TeamBuildError {
    #[error("at least five cards are required to build a team, got {count}")]
    NotEnoughCards { count: usize },

    #[error("candidate count exceeds limit {limit}")]
    TooManyCandidates { limit: usize },

    #[error("chart error: {0}")]
    Chart(#[from] ChartError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamGenerationOptions {
    pub score_as_medley: bool,
    pub max_candidates: usize,
}

impl Default for TeamGenerationOptions {
    fn default() -> Self {
        Self {
            score_as_medley: true,
            max_candidates: usize::MAX,
        }
    }
}

pub fn build_team_candidates(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    options: TeamGenerationOptions,
) -> Result<Vec<TeamCandidate>, TeamBuildError> {
    build_team_candidates_with_current_best(
        cards,
        charts,
        area_item_percent,
        selected_items,
        options,
        0,
    )
}

pub(crate) fn build_team_candidates_with_current_best(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    options: TeamGenerationOptions,
    current_best: i32,
) -> Result<Vec<TeamCandidate>, TeamBuildError> {
    let raw_candidates = build_raw_team_candidates_with_current_best(
        cards,
        charts,
        area_item_percent,
        selected_items,
        options,
        current_best,
    )?;

    compact_raw_candidate_masks(raw_candidates, cards, charts.len())
}

pub(crate) fn build_raw_team_candidates_with_current_best(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    options: TeamGenerationOptions,
    current_best: i32,
) -> Result<Vec<RawTeamCandidate>, TeamBuildError> {
    let trace = trace_enabled();
    let trace_detail = trace_detail_enabled();
    let build_start = trace.then(Timer::start);
    if cards.len() < TEAM_SIZE {
        return Err(TeamBuildError::NotEnoughCards { count: cards.len() });
    }

    let stat_start = trace.then(Timer::start);
    let card_stats = adjusted_card_stats(cards, area_item_percent, selected_items);
    let stat_ms = elapsed_ms(stat_start);
    let prune_start = trace.then(Timer::start);
    let profile_start = trace.then(Timer::start);
    let profiles = medley_card_prune_profiles(cards, charts, &card_stats)?;
    let profile_ms = elapsed_ms(profile_start);
    let mut current_best = current_best;
    let signature_pool_start = trace.then(Timer::start);
    let (signature_pools, signature_stats, prune_trace) =
        signature_candidate_pools(cards, charts, &profiles, current_best);
    let signature_pool_ms = elapsed_ms(signature_pool_start);
    let prune_ms = elapsed_ms(prune_start);
    if signature_pools.is_empty() {
        return Err(TeamBuildError::NotEnoughCards { count: 0 });
    }
    let mut raw_candidates = Vec::new();
    let mut skill_meta_cache = SkillMetaCache::new(charts.len());
    let mut next_incumbent_refresh_at = INCUMBENT_REFRESH_MIN_CANDIDATES;
    let mut enumeration_trace = EnumerationTrace::default();

    let enumerate_start = trace.then(Timer::start);
    for pool in &signature_pools {
        let candidate_filter =
            CandidateIncumbentFilter::new(cards, charts, &profiles, current_best);
        match enumerate_signature_pool(
            cards,
            &card_stats,
            charts,
            &profiles,
            options,
            pool.signature,
            &pool.active_card_indices,
            &mut raw_candidates,
            &mut skill_meta_cache,
            candidate_filter.as_ref(),
            trace,
        ) {
            Ok(stats) => {
                enumeration_trace.add(&stats);
                if trace_detail {
                    eprintln!(
                        "medley signature enumerate: signature={} active_cards={} groups={} estimated_candidates={} candidates_before={} candidates_after={} branch_bound_prunes={} leaf_checks={} final_layer_card_checks={} final_layer_signature_passes={} final_layer_upper_bound_rejects={} signature_rejects={} candidate_builds={} candidate_filter_rejects={} build_candidate_ms={:.3} candidate_filter_ms={:.3}",
                        signature_label(pool.signature),
                        stats.active_card_count,
                        stats.group_count,
                        pool.estimated_candidates,
                        stats.candidates_before,
                        stats.candidates_after,
                        stats.branch_upper_bound_pruned,
                        stats.leaf_checks,
                        stats.final_layer_card_checks,
                        stats.final_layer_signature_passes,
                        stats.final_layer_upper_bound_rejects,
                        stats.signature_rejects,
                        stats.candidate_builds,
                        stats.candidate_filter_rejects,
                        stats.build_candidate_ms,
                        stats.candidate_filter_ms,
                    );
                }
            }
            Err(error) => {
                let stats = error.stats;
                enumeration_trace.add(&stats);
                if trace {
                    eprintln!(
                        "medley signature enumerate error: signature={} active_cards={} groups={} estimated_candidates={} candidates_before={} candidates_after={} branch_bound_prunes={} leaf_checks={} final_layer_card_checks={} final_layer_signature_passes={} final_layer_upper_bound_rejects={} signature_rejects={} candidate_builds={} candidate_filter_rejects={} build_candidate_ms={:.3} candidate_filter_ms={:.3} error={}",
                        signature_label(pool.signature),
                        stats.active_card_count,
                        stats.group_count,
                        pool.estimated_candidates,
                        stats.candidates_before,
                        stats.candidates_after,
                        stats.branch_upper_bound_pruned,
                        stats.leaf_checks,
                        stats.final_layer_card_checks,
                        stats.final_layer_signature_passes,
                        stats.final_layer_upper_bound_rejects,
                        stats.signature_rejects,
                        stats.candidate_builds,
                        stats.candidate_filter_rejects,
                        stats.build_candidate_ms,
                        stats.candidate_filter_ms,
                        error.source,
                    );
                    if trace_detail {
                        for stats in signature_stats
                            .iter()
                            .filter(|stats| stats.signature == Some(pool.signature))
                        {
                            trace_signature_pool_stats(stats);
                        }
                        trace_score_contribution_cover_diagnostics(
                            cards,
                            charts,
                            &profiles,
                            current_best,
                            pool.signature,
                            &pool.active_card_indices,
                        );
                    }
                }
                return Err(error.source);
            }
        }
        if should_refresh_candidate_incumbent(
            current_best,
            charts.len(),
            raw_candidates.len(),
            next_incumbent_refresh_at,
        ) {
            current_best = refresh_candidate_incumbent(&raw_candidates, current_best, trace);
            next_incumbent_refresh_at = next_incumbent_refresh_count(raw_candidates.len());
        }
    }
    let enumerate_ms = elapsed_ms(enumerate_start);
    if raw_candidates.is_empty() {
        return Err(TeamBuildError::NotEnoughCards { count: 0 });
    }

    if trace {
        if trace_detail {
            let global_prune_stats = global_prune_stats(cards, charts, &profiles, current_best);
            trace_medley_prune_stats("medley team build global", &global_prune_stats);
            for stats in &signature_stats {
                trace_signature_pool_stats(stats);
            }
        }
        eprintln!(
            "medley team build detail: signature_pools={} raw_candidates={} candidates={} used_cards={} skill_meta_cache_entries={} stat_ms={stat_ms:.3} prune_ms={prune_ms:.3} profile_ms={profile_ms:.3} signature_pool_ms={signature_pool_ms:.3} enumerate_ms={enumerate_ms:.3} leaf_checks={} final_layer_card_checks={} final_layer_signature_passes={} final_layer_upper_bound_rejects={} signature_rejects={} candidate_builds={} candidate_filter_rejects={} branch_bound_prunes={} build_candidate_ms={:.3} candidate_filter_ms={:.3} candidate_profile_samples={} candidate_profile_total_ms={:.3} candidate_profile_stat_ms={:.3} candidate_profile_prepare_ms={:.3} candidate_profile_seed_ms={:.3} candidate_profile_order_ms={:.3} candidate_profile_result_ms={:.3} candidate_profile_finalize_ms={:.3} candidate_profile_order_chart0_ms={:.3} candidate_profile_order_chart1_ms={:.3} candidate_profile_order_chart2_ms={:.3} candidate_profile_order_nonoverlap_calls={} candidate_profile_order_overlap_calls={} candidate_profile_order_exact_delta_calls={} candidate_profile_order_overlap_check_ms={:.3} candidate_profile_order_base_score_ms={:.3} candidate_profile_order_assignment_ms={:.3} candidate_profile_order_exact_skill_ms={:.3} total_ms={:.3}",
            signature_pools.len(),
            raw_candidates.len(),
            raw_candidates.len(),
            raw_candidate_used_card_count(&raw_candidates),
            skill_meta_cache.entry_count(),
            enumeration_trace.leaf_checks,
            enumeration_trace.final_layer_card_checks,
            enumeration_trace.final_layer_signature_passes,
            enumeration_trace.final_layer_upper_bound_rejects,
            enumeration_trace.signature_rejects,
            enumeration_trace.candidate_builds,
            enumeration_trace.candidate_filter_rejects,
            enumeration_trace.branch_upper_bound_pruned,
            enumeration_trace.build_candidate_ms,
            enumeration_trace.candidate_filter_ms,
            enumeration_trace.build_profile.samples,
            enumeration_trace.build_profile.total_ms,
            enumeration_trace.build_profile.stat_ms,
            enumeration_trace.build_profile.prepare_ms,
            enumeration_trace.build_profile.seed_ms,
            enumeration_trace.build_profile.order_ms,
            enumeration_trace.build_profile.result_ms,
            enumeration_trace.build_profile.finalize_ms,
            enumeration_trace.build_profile.order_by_chart_ms[0],
            enumeration_trace.build_profile.order_by_chart_ms[1],
            enumeration_trace.build_profile.order_by_chart_ms[2],
            enumeration_trace
                .build_profile
                .order_detail
                .non_overlapping_calls,
            enumeration_trace
                .build_profile
                .order_detail
                .overlapping_calls,
            enumeration_trace
                .build_profile
                .order_detail
                .exact_skill_delta_calls,
            enumeration_trace
                .build_profile
                .order_detail
                .overlap_check_ms,
            enumeration_trace
                .build_profile
                .order_detail
                .base_score_ms,
            enumeration_trace
                .build_profile
                .order_detail
                .assignment_ms,
            enumeration_trace
                .build_profile
                .order_detail
                .exact_skill_ms,
            elapsed_ms(build_start),
        );
        trace_prune_timing(&prune_trace);
    }

    Ok(raw_candidates)
}

fn refresh_candidate_incumbent(
    raw_candidates: &[RawTeamCandidate],
    current_best: i32,
    trace: bool,
) -> i32 {
    if raw_candidates.len() < 3 {
        return current_best;
    }

    let filter_start = trace.then(Timer::start);
    let candidate_indices = refresh_candidate_indices(raw_candidates, current_best);
    let filter_ms = elapsed_ms(filter_start);
    if candidate_indices.len() < 3 {
        if trace {
            eprintln!(
                "medley candidate incumbent skipped: candidates={} solver_candidates={} current_best={} reason=no candidates after filter filter_ms={filter_ms:.3}",
                raw_candidates.len(),
                candidate_indices.len(),
                current_best,
            );
        }
        return current_best;
    }
    if !incumbent_refresh_within_solver_budget(candidate_indices.len()) {
        if trace {
            eprintln!(
                "medley candidate incumbent skipped: candidates={} solver_candidates={} current_best={} reason=solver candidate budget {} filter_ms={filter_ms:.3}",
                raw_candidates.len(),
                candidate_indices.len(),
                current_best,
                INCUMBENT_REFRESH_MAX_SOLVER_CANDIDATES,
            );
        }
        return current_best;
    }

    let solve_start = trace.then(Timer::start);
    let (plan, used_card_count) = match raw_candidate_solver_input_for_indices(
        raw_candidates,
        current_best,
        &candidate_indices,
    ) {
        RawCandidateSolverInput::Narrow {
            input,
            used_card_count,
        } => match solve_medley_with(&input, MedleySolverPreference::Auto) {
            Ok(plan) => (plan, used_card_count),
            Err(MedleySolverError::NoValidPlan) => return current_best,
            Err(error) => {
                if trace {
                    eprintln!(
                        "medley candidate incumbent skipped: candidates={} solver_candidates={} used_cards={} current_best={} reason={} filter_ms={filter_ms:.3} solve_ms={:.3}",
                        raw_candidates.len(),
                        candidate_indices.len(),
                        used_card_count,
                        current_best,
                        error,
                        elapsed_ms(solve_start),
                    );
                }
                return current_best;
            }
        },
        RawCandidateSolverInput::Wide {
            input,
            used_card_count,
        } => match solve_medley_wide_with(&input, MedleySolverPreference::Auto) {
            Ok(plan) => (plan, used_card_count),
            Err(MedleySolverError::NoValidPlan) => return current_best,
            Err(error) => {
                if trace {
                    eprintln!(
                        "medley candidate incumbent skipped: candidates={} solver_candidates={} used_cards={} current_best={} reason={} filter_ms={filter_ms:.3} solve_ms={:.3}",
                        raw_candidates.len(),
                        candidate_indices.len(),
                        used_card_count,
                        current_best,
                        error,
                        elapsed_ms(solve_start),
                    );
                }
                return current_best;
            }
        },
    };

    if trace {
        eprintln!(
            "medley candidate incumbent refresh: candidates={} solver_candidates={} used_cards={} old_best={} score={} implementation={:?} quality={:?} exact_work={} auto_route={:?} filter_ms={filter_ms:.3} solve_ms={:.3}",
            raw_candidates.len(),
            candidate_indices.len(),
            used_card_count,
            current_best,
            plan.score,
            plan.implementation,
            plan.quality,
            plan.exact_work,
            plan.auto_route,
            elapsed_ms(solve_start),
        );
    }

    current_best.max(plan.score)
}

fn incumbent_refresh_within_solver_budget(candidate_count: usize) -> bool {
    candidate_count <= INCUMBENT_REFRESH_MAX_SOLVER_CANDIDATES
}

fn should_refresh_candidate_incumbent(
    current_best: i32,
    chart_count: usize,
    candidate_count: usize,
    next_refresh_at: usize,
) -> bool {
    current_best > 0 && chart_count == MEDLEY_TEAM_COUNT && candidate_count >= next_refresh_at
}

fn next_incumbent_refresh_count(candidate_count: usize) -> usize {
    candidate_count
        .saturating_mul(2)
        .max(candidate_count.saturating_add(INCUMBENT_REFRESH_MIN_CANDIDATES))
}

fn refresh_candidate_indices(candidates: &[RawTeamCandidate], current_best: i32) -> Vec<usize> {
    let max_scores: [i32; MEDLEY_TEAM_COUNT] = std::array::from_fn(|song_idx| {
        candidates
            .iter()
            .map(|candidate| candidate.scores[song_idx])
            .max()
            .unwrap_or_default()
    });

    candidates
        .iter()
        .enumerate()
        .filter_map(|(idx, candidate)| {
            (0..MEDLEY_TEAM_COUNT)
                .any(|song_idx| {
                    let upper_bound = candidate.scores[song_idx] as i64
                        + max_scores[(song_idx + 1) % MEDLEY_TEAM_COUNT] as i64
                        + max_scores[(song_idx + 2) % MEDLEY_TEAM_COUNT] as i64;
                    upper_bound > current_best as i64
                })
                .then_some(idx)
        })
        .collect()
}

fn trace_prune_timing(trace: &MedleyPruneTrace) {
    eprintln!(
        "medley prune timing: signatures={} contribution_graphs={} context_ms={:.3} upper_bounds_init_ms={:.3} signatures_ms={:.3} active_indices_ms={:.3} hard_graph_ms={:.3} hard_cover_ms={:.3} contribution_context_ms={:.3} contribution_graph_ms={:.3} contribution_cover_ms={:.3} upper_bound_ms={:.3} completion_ms={:.3} capacity_ms={:.3}",
        trace.signature_count,
        trace.contribution_graph_count,
        trace.context_ms,
        trace.upper_bounds_init_ms,
        trace.signatures_ms,
        trace.active_indices_ms,
        trace.hard_graph_ms,
        trace.hard_cover_ms,
        trace.contribution_context_ms,
        trace.contribution_graph_ms,
        trace.contribution_cover_ms,
        trace.upper_bound_ms,
        trace.completion_ms,
        trace.capacity_ms,
    );
}

pub(in crate::medley) fn adjusted_card_stats(
    cards: &[PreparedCard],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
) -> Vec<f64> {
    cards
        .iter()
        .map(|card| {
            card.add_up_stat(
                area_item_percent,
                &selected_items.band,
                &selected_items.attribute,
                selected_items.magazine.as_str(),
            )
        })
        .collect()
}

pub(crate) fn medley_same_team_item_score_upper_bound(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
) -> Result<i32, TeamBuildError> {
    if cards.len() < TEAM_SIZE {
        return Err(TeamBuildError::NotEnoughCards { count: cards.len() });
    }
    let card_stats = adjusted_card_stats(cards, area_item_percent, selected_items);
    let profiles = medley_card_prune_profiles(cards, charts, &card_stats)?;
    let signatures = seed_signatures(cards);
    let mut total = 0;

    for (chart_idx, chart) in charts.iter().enumerate() {
        total +=
            medley_chart_item_score_upper_bound(cards, &profiles, chart, chart_idx, &signatures)
                as i32;
    }

    Ok(total)
}

pub(in crate::medley) fn medley_chart_item_score_upper_bound(
    cards: &[PreparedCard],
    profiles: &[super::prune::MedleyCardPruneProfile],
    chart: &Chart,
    chart_idx: usize,
    signatures: &[MedleyPruneSignature],
) -> f64 {
    signatures
        .iter()
        .filter_map(|&signature| {
            same_team_chart_signature_upper_bound(cards, profiles, chart, chart_idx, signature)
        })
        .fold(0.0_f64, f64::max)
        .ceil()
}

fn same_team_chart_signature_upper_bound(
    cards: &[PreparedCard],
    profiles: &[super::prune::MedleyCardPruneProfile],
    chart: &Chart,
    chart_idx: usize,
    signature: MedleyPruneSignature,
) -> Option<f64> {
    let groups = upper_bound_card_groups(cards, profiles, chart_idx, signature)?;
    if groups.len() < TEAM_SIZE {
        return None;
    }

    let mut dp: [Vec<UpperBoundState>; TEAM_SIZE + 1] = std::array::from_fn(|_| Vec::new());
    dp[0].push(UpperBoundState::default());

    for group in groups {
        for count in (1..=TEAM_SIZE).rev() {
            if dp[count - 1].is_empty() {
                continue;
            }

            let previous = dp[count - 1].clone();
            dp[count].reserve(previous.len().saturating_mul(group.len()));
            for state in previous {
                for card in &group {
                    dp[count].push(state.with_card(*card));
                }
            }
            prune_dominated_upper_bound_states(&mut dp[count]);
        }
    }

    dp[TEAM_SIZE]
        .iter()
        .map(|state| state.score_upper_bound(chart))
        .max_by(f64::total_cmp)
}

fn upper_bound_card_groups(
    cards: &[PreparedCard],
    profiles: &[super::prune::MedleyCardPruneProfile],
    chart_idx: usize,
    signature: MedleyPruneSignature,
) -> Option<Vec<Vec<UpperBoundCard>>> {
    let mut groups: BTreeMap<u32, Vec<UpperBoundCard>> = BTreeMap::new();

    for (idx, card) in cards.iter().enumerate() {
        if !signature.allows(card) {
            continue;
        }

        let profile = profiles.get(idx)?;
        let score_up = card
            .score_up
            .resolve(signature.team_band_id(), signature.team_attribute());
        let values = profile.skill_meta_for_score_up(score_up)?;
        let start = chart_idx * (TEAM_SIZE + 1);
        let normal = values
            .get(start..start + TEAM_SIZE)?
            .iter()
            .copied()
            .fold(0.0, f64::max);
        let captain = *values.get(start + TEAM_SIZE)?;

        groups
            .entry(card.character_id)
            .or_default()
            .push(UpperBoundCard {
                stat: profile.stat,
                normal,
                captain,
            });
    }

    let mut result = Vec::with_capacity(groups.len());
    for mut group in groups.into_values() {
        prune_dominated_upper_bound_cards(&mut group);
        result.push(group);
    }

    Some(result)
}

#[derive(Debug, Clone, Copy, Default)]
struct UpperBoundCard {
    stat: f64,
    normal: f64,
    captain: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct UpperBoundState {
    stat: f64,
    normal: f64,
    captain: f64,
}

impl UpperBoundState {
    fn with_card(self, card: UpperBoundCard) -> Self {
        Self {
            stat: self.stat + card.stat,
            normal: self.normal + card.normal,
            captain: self.captain.max(card.captain),
        }
    }

    fn score_upper_bound(self, chart: &Chart) -> f64 {
        self.stat * (chart.meta.no_skill + self.normal + self.captain)
    }
}

fn prune_dominated_upper_bound_cards(cards: &mut Vec<UpperBoundCard>) {
    cards.sort_by(|left, right| {
        right
            .stat
            .total_cmp(&left.stat)
            .then_with(|| right.normal.total_cmp(&left.normal))
            .then_with(|| right.captain.total_cmp(&left.captain))
    });
    let original = std::mem::take(cards);
    for (idx, card) in original.iter().copied().enumerate() {
        let dominated = original.iter().enumerate().any(|(other_idx, other)| {
            other_idx != idx
                && upper_bound_card_dominates(*other, card)
                && (upper_bound_card_strictly_dominates(*other, card) || other_idx < idx)
        });
        if !dominated {
            cards.push(card);
        }
    }
}

fn prune_dominated_upper_bound_states(states: &mut Vec<UpperBoundState>) {
    states.sort_by(|left, right| {
        right
            .stat
            .total_cmp(&left.stat)
            .then_with(|| right.normal.total_cmp(&left.normal))
            .then_with(|| right.captain.total_cmp(&left.captain))
    });
    let original = std::mem::take(states);
    for (idx, state) in original.iter().copied().enumerate() {
        let dominated = original.iter().enumerate().any(|(other_idx, other)| {
            other_idx != idx
                && upper_bound_state_dominates(*other, state)
                && (upper_bound_state_strictly_dominates(*other, state) || other_idx < idx)
        });
        if !dominated {
            states.push(state);
        }
    }
}

fn upper_bound_card_dominates(left: UpperBoundCard, right: UpperBoundCard) -> bool {
    left.stat >= right.stat && left.normal >= right.normal && left.captain >= right.captain
}

fn upper_bound_card_strictly_dominates(left: UpperBoundCard, right: UpperBoundCard) -> bool {
    left.stat > right.stat || left.normal > right.normal || left.captain > right.captain
}

fn upper_bound_state_dominates(left: UpperBoundState, right: UpperBoundState) -> bool {
    left.stat >= right.stat && left.normal >= right.normal && left.captain >= right.captain
}

fn upper_bound_state_strictly_dominates(left: UpperBoundState, right: UpperBoundState) -> bool {
    left.stat > right.stat || left.normal > right.normal || left.captain > right.captain
}

fn trace_enabled() -> bool {
    std::env::var_os("BANGDREAM_OPTIMIZE_DP_TRACE").is_some()
}

fn trace_detail_enabled() -> bool {
    std::env::var_os("BANGDREAM_OPTIMIZE_DP_TRACE_DETAIL").is_some()
}

fn elapsed_ms(start: Option<Timer>) -> f64 {
    optional_elapsed_ms(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medley::scoring::{build_candidate, MedleyCardInput};
    use crate::medley::test_support::{chart, prepared_card, selected_cool_items};
    use crate::model::chart::{ChartNode, ChartNodeType};
    use crate::model::preparation::{ScoreUp, StatRate, StatValue};
    use crate::model::schema::Attribute;
    use std::collections::BTreeMap;

    #[test]
    fn incumbent_refresh_respects_solver_candidate_budget() {
        assert!(incumbent_refresh_within_solver_budget(
            INCUMBENT_REFRESH_MAX_SOLVER_CANDIDATES
        ));
        assert!(!incumbent_refresh_within_solver_budget(
            INCUMBENT_REFRESH_MAX_SOLVER_CANDIDATES + 1
        ));
    }

    #[test]
    fn builds_single_candidate_from_five_distinct_characters() {
        let cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool),
            prepared_card(2, 2, 1, Attribute::Cool),
            prepared_card(3, 3, 1, Attribute::Cool),
            prepared_card(4, 4, 1, Attribute::Cool),
            prepared_card(5, 5, 1, Attribute::Cool),
        ];
        let selected_items = selected_cool_items();

        let candidates = build_team_candidates(
            &cards,
            &[chart()],
            &AreaItemPercent::empty(),
            &selected_items,
            TeamGenerationOptions::default(),
        )
        .unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].mask, 0b1_1111);
        assert_eq!(candidates[0].stat, 15_000);
        assert_eq!(candidates[0].captain_card_ids.len(), 1);
        assert_eq!(candidates[0].scores.len(), 1);
        assert!(candidates[0].scores[0] > 0);
    }

    #[test]
    fn skips_teams_with_duplicate_characters() {
        let cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool),
            prepared_card(2, 1, 1, Attribute::Happy),
            prepared_card(3, 2, 1, Attribute::Cool),
            prepared_card(4, 3, 1, Attribute::Cool),
            prepared_card(5, 4, 1, Attribute::Cool),
            prepared_card(6, 5, 1, Attribute::Cool),
        ];
        let selected_items = selected_cool_items();

        let candidates = build_team_candidates(
            &cards,
            &[chart()],
            &AreaItemPercent::empty(),
            &selected_items,
            TeamGenerationOptions::default(),
        )
        .unwrap();

        assert_eq!(candidates.len(), 2);
        assert!(candidates
            .iter()
            .all(|candidate| candidate.team_card_ids.len() == 5));
    }

    #[test]
    fn applies_area_items_and_unified_score_up() {
        let mut cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool),
            prepared_card(2, 2, 1, Attribute::Cool),
            prepared_card(3, 3, 1, Attribute::Cool),
            prepared_card(4, 4, 1, Attribute::Cool),
            prepared_card(5, 5, 1, Attribute::Cool),
        ];
        cards[0].score_up = ScoreUp {
            default: 1.0,
            unification_activate_effect_value: Some(1.5),
            unification_activate_condition_band_id: Some(1),
            unification_activate_condition_type: Some(Attribute::Cool),
        };
        let area = AreaItemPercent {
            band: BTreeMap::from([("1".to_owned(), StatRate::all(0.1))]),
            attribute: BTreeMap::new(),
            magazine: BTreeMap::new(),
        };
        let selected_items = selected_cool_items();

        let candidates = build_team_candidates(
            &cards,
            &[chart()],
            &area,
            &selected_items,
            TeamGenerationOptions::default(),
        )
        .unwrap();

        assert_eq!(candidates[0].stat, 16_500);
        assert!(candidates[0].scores[0] > 0);
    }

    #[test]
    fn keeps_three_dominated_cards_per_character_before_mask_capacity_check() {
        let mut cards = Vec::new();
        for character_id in 1..=5 {
            let dominant_id = character_id * 1000;
            cards.push(prepared_card(dominant_id, character_id, 1, Attribute::Cool));
            for duplicate_idx in 1..=13 {
                let mut card = prepared_card(
                    dominant_id + duplicate_idx,
                    character_id,
                    1,
                    Attribute::Cool,
                );
                card.stat = StatValue {
                    performance: 900.0,
                    technique: 900.0,
                    visual: 900.0,
                };
                cards.push(card);
            }
        }
        assert!(cards.len() > u64::BITS as usize);
        let selected_items = selected_cool_items();

        let candidates = build_team_candidates(
            &cards,
            &[chart()],
            &AreaItemPercent::empty(),
            &selected_items,
            TeamGenerationOptions::default(),
        )
        .unwrap();

        assert_eq!(candidates.len(), 243);
        assert!(candidates.iter().all(|candidate| {
            candidate.team_card_ids.iter().all(|card_id| {
                let suffix = card_id % 1000;
                suffix <= 2
            })
        }));
    }

    #[test]
    fn production_pruning_preserves_exhaustive_three_team_optimum() {
        validate_exhaustive_three_team_case(0);
    }

    #[test]
    #[ignore = "explicit randomized pruning stress test"]
    fn randomized_production_pruning_matches_exhaustive_three_team_optimum() {
        for seed in 1..=6 {
            validate_exhaustive_three_team_case(seed);
        }
    }

    fn validate_exhaustive_three_team_case(seed: u32) {
        let mut cards = Vec::new();
        for character_id in 1..=15u32 {
            let band_id = 1 + (character_id - 1) / 5;
            let attribute = if character_id <= 5 {
                Attribute::Cool
            } else {
                Attribute::Happy
            };
            let mut card = prepared_card(character_id, character_id, band_id, attribute);
            let stat = 900.0 + ((character_id * 37 + seed * 53) % 400) as f64;
            card.stat = StatValue {
                performance: stat,
                technique: stat + ((character_id + seed) % 3) as f64 * 40.0,
                visual: stat + ((character_id + seed) % 5) as f64 * 25.0,
            };
            card.skill.duration = [5.0, 6.0, 7.0][(character_id + seed) as usize % 3];
            card.score_up.default = 0.8 + ((character_id + seed) % 6) as f64 * 0.12;
            if character_id == 1 {
                card.stat = StatValue {
                    performance: 800.0,
                    technique: 820.0,
                    visual: 840.0,
                };
                card.skill.duration = 6.0;
                card.score_up.default = 0.7;
            }
            if matches!(character_id, 2 | 6 | 11) {
                card.score_up = ScoreUp {
                    default: card.score_up.default,
                    unification_activate_effect_value: Some(card.score_up.default + 0.65),
                    unification_activate_condition_band_id: Some(band_id),
                    unification_activate_condition_type: Some(attribute),
                };
            }
            cards.push(card);
        }
        for alternative_idx in 0..3u32 {
            let mut card = prepared_card(101 + alternative_idx, 1, 1, Attribute::Cool);
            let stat = 1_300.0 + alternative_idx as f64 * 20.0 + (seed % 7) as f64;
            card.stat = StatValue {
                performance: stat,
                technique: stat + 40.0,
                visual: stat + 80.0,
            };
            card.skill.duration = 6.0;
            card.score_up.default = 1.5 + alternative_idx as f64 * 0.02;
            cards.push(card);
        }
        for (alternative_idx, character_id) in [6u32].into_iter().enumerate() {
            let mut card = prepared_card(
                100 + character_id,
                character_id,
                4,
                if character_id == 6 {
                    Attribute::Cool
                } else {
                    Attribute::Happy
                },
            );
            let stat = 1_050.0 + alternative_idx as f64 * 90.0;
            card.stat = StatValue {
                performance: stat,
                technique: stat + 30.0,
                visual: stat + 60.0,
            };
            card.skill.duration = 6.0;
            card.skill.rateup = alternative_idx == 0;
            card.score_up.default = if card.skill.rateup {
                1.0
            } else {
                1.15 + alternative_idx as f64 * 0.1
            };
            cards.push(card);
        }

        let charts = exhaustive_validation_charts(seed);
        let area = AreaItemPercent {
            band: BTreeMap::from([("1".to_owned(), StatRate::all(0.08 + seed as f64 * 0.001))]),
            attribute: BTreeMap::from([(
                "cool".to_owned(),
                StatRate::all(0.06 + seed as f64 * 0.001),
            )]),
            magazine: BTreeMap::from([(
                "performance".to_owned(),
                StatRate {
                    performance: 0.05,
                    technique: 0.0,
                    visual: 0.0,
                },
            )]),
        };
        let items = selected_cool_items();
        let options = TeamGenerationOptions {
            score_as_medley: false,
            max_candidates: usize::MAX,
        };
        let card_stats = adjusted_card_stats(&cards, &area, &items);
        let inputs = cards
            .iter()
            .enumerate()
            .map(|(raw_index, card)| MedleyCardInput {
                card,
                raw_index,
                stat: card_stats[raw_index],
            })
            .collect::<Vec<_>>();

        let mut exhaustive = BTreeMap::new();
        let mut skill_meta_cache = SkillMetaCache::new(charts.len());
        let mut exact_score_scratch = crate::model::chart::ExactScoreScratch::default();
        for mask in 0u64..(1u64 << cards.len()) {
            if mask.count_ones() != TEAM_SIZE as u32 {
                continue;
            }
            let indices = (0..cards.len())
                .filter(|idx| mask & (1 << idx) != 0)
                .collect::<Vec<_>>();
            let mut characters = indices
                .iter()
                .map(|&idx| cards[idx].character_id)
                .collect::<Vec<_>>();
            characters.sort_unstable();
            characters.dedup();
            if characters.len() != TEAM_SIZE {
                continue;
            }
            let candidate = build_candidate(
                &inputs,
                &charts,
                options,
                &indices,
                &mut skill_meta_cache,
                &mut exact_score_scratch,
            )
            .unwrap();
            exhaustive.insert(mask, candidate.scores);
        }

        let production =
            build_raw_team_candidates_with_current_best(&cards, &charts, &area, &items, options, 0)
                .unwrap();
        let production = production
            .iter()
            .map(|candidate| {
                let mask = candidate
                    .raw_indices
                    .iter()
                    .fold(0u64, |mask, &idx| mask | (1 << idx));
                (mask, candidate.scores)
            })
            .collect::<BTreeMap<_, _>>();

        assert!(
            production.len() < exhaustive.len(),
            "seed={seed} validation pool must exercise pruning: production={} exhaustive={}",
            production.len(),
            exhaustive.len()
        );
        let item_upper =
            medley_same_team_item_score_upper_bound(&cards, &charts, &area, &items).unwrap();
        let independent_song_max_sum: i32 = (0..MEDLEY_TEAM_COUNT)
            .map(|song_idx| {
                exhaustive
                    .values()
                    .map(|scores| scores[song_idx])
                    .max()
                    .unwrap()
            })
            .sum();
        assert!(
            item_upper >= independent_song_max_sum,
            "seed={seed} item_upper={item_upper} exact={independent_song_max_sum}"
        );
        let exhaustive_best = exhaustive_three_team_score(&exhaustive, &cards);
        let production_best = exhaustive_three_team_score(&production, &cards);
        assert_eq!(production_best, exhaustive_best, "seed={seed}");

        let near_incumbent = exhaustive_best - 1;
        let incumbent_filtered = build_raw_team_candidates_with_current_best(
            &cards,
            &charts,
            &area,
            &items,
            options,
            near_incumbent,
        )
        .unwrap();
        let incumbent_filtered = incumbent_filtered
            .iter()
            .map(|candidate| {
                let mask = candidate
                    .raw_indices
                    .iter()
                    .fold(0u64, |mask, &idx| mask | (1 << idx));
                (mask, candidate.scores)
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            exhaustive_three_team_score(&incumbent_filtered, &cards),
            exhaustive_best,
            "seed={seed}"
        );
    }

    fn exhaustive_validation_charts(seed: u32) -> Vec<Chart> {
        (0..3)
            .map(|chart_idx| {
                let mut nodes = Vec::new();
                for activation in 0..6 {
                    let start = activation as f64 * 10.0;
                    nodes.push(ChartNode {
                        node_type: ChartNodeType::Skill,
                        time: start,
                    });
                    for note in 0..(3 + chart_idx * 2) {
                        nodes.push(ChartNode {
                            node_type: ChartNodeType::Node,
                            time: start
                                + 0.35
                                + seed as f64 * 0.005
                                + note as f64 * (7.4 / (3 + chart_idx * 2) as f64),
                        });
                    }
                }
                let mut chart = Chart::new(15 + chart_idx as i32 * 3, nodes);
                chart.init(0, false).unwrap();
                chart
            })
            .collect()
    }

    fn exhaustive_three_team_score(
        candidates: &BTreeMap<u64, [i32; MEDLEY_TEAM_COUNT]>,
        cards: &[PreparedCard],
    ) -> i32 {
        let mut selection_masks = vec![0u64];
        for character_id in [1u32, 6, 11] {
            let choices = cards
                .iter()
                .enumerate()
                .filter_map(|(idx, card)| {
                    (card.character_id == character_id).then_some(1u64 << idx)
                })
                .collect::<Vec<_>>();
            selection_masks = selection_masks
                .into_iter()
                .flat_map(|mask| choices.iter().map(move |&choice| mask | choice))
                .collect();
        }
        let fixed_mask = cards
            .iter()
            .enumerate()
            .filter(|(_, card)| !matches!(card.character_id, 1 | 6 | 11))
            .fold(0u64, |mask, (idx, _)| mask | (1 << idx));

        let mut best = i32::MIN;
        for selected in selection_masks {
            let universe = selected | fixed_mask;
            for song0 in five_card_subsets(universe) {
                let remaining = universe ^ song0;
                for song1 in five_card_subsets(remaining) {
                    let song2 = remaining ^ song1;
                    let (Some(score0), Some(score1), Some(score2)) = (
                        candidates.get(&song0),
                        candidates.get(&song1),
                        candidates.get(&song2),
                    ) else {
                        continue;
                    };
                    best = best.max(score0[0] + score1[1] + score2[2]);
                }
            }
        }
        best
    }

    fn five_card_subsets(mask: u64) -> Vec<u64> {
        let mut result = Vec::new();
        let mut subset = mask;
        while subset != 0 {
            if subset.count_ones() == TEAM_SIZE as u32 {
                result.push(subset);
            }
            subset = (subset - 1) & mask;
        }
        result
    }
}
