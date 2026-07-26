use std::collections::{BTreeMap, BTreeSet, HashMap};

use bangdream_optimize_medley_solver::{
    enumerate_medley_band, enumerate_medley_band_wide, solve_medley_seed_with_rounds,
    solve_medley_wide_seed_with_rounds, MedleyBandInput, MedleyBandVisit, MedleySolverInput,
    WideMedleyBandInput, WideMedleySolverInput,
};

use crate::event_pt::{medley_three_song_points, MEDLEY_SCORE_DIVISOR};
use crate::maximize::prune_dominated_item_combinations;
use crate::medley::scoring::RawTeamCandidate;
use crate::medley::seed::seed_medley_raw_team_indices_for_items;
use crate::medley::team::{
    adjusted_card_stats, build_raw_team_candidates_with_fixed_score_floor,
    medley_same_team_item_score_upper_bound, TeamBuildError, TeamGenerationOptions,
};
use crate::model::chart::ExactScoreScratch;
use crate::timing::Timer;
use crate::{
    area_item_combinations, AreaItemPercent, Chart, PreparedCard, SelectedAreaItems, SongMode,
    TeamCardSkill,
};

use super::distribution::full_team_score_distributions;
use super::model::{compare_nonnegative_averages, RANDOM_SKILL_ORDER_COUNT};
use super::{
    AveragePt, CaptainScoreDistribution, PtMaximizeError, PtMaximizeMedleyMetrics,
    PtMaximizeMedleyResult, PtMaximizeMedleyTeamResult, ScoreHistogram,
};

const SONG_COUNT: usize = 3;
const TEAM_SIZE: usize = 5;
const MEAN_SEED_RANDOM_BUCKET_ROUNDS: usize = 512;

#[derive(Debug, Clone)]
struct MedleyCandidate {
    raw_indices: [usize; TEAM_SIZE],
    mask_words: Vec<u64>,
    card_ids: Vec<u32>,
    skills: [TeamCardSkill; TEAM_SIZE],
    stat: i32,
    mean_numerators: [i64; SONG_COUNT],
}

#[derive(Debug, Clone)]
struct EvaluatedPlan {
    result: PtMaximizeMedleyResult,
}

pub fn search_medley(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
) -> Result<PtMaximizeMedleyResult, PtMaximizeError> {
    search_medley_with_metrics(cards, charts, area_item_percent).map(|(result, _)| result)
}

pub fn search_medley_with_metrics(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
) -> Result<(PtMaximizeMedleyResult, PtMaximizeMedleyMetrics), PtMaximizeError> {
    let total_start = Timer::start();
    if charts.len() != SONG_COUNT {
        return Err(PtMaximizeError::InvalidMedleySongCount {
            count: charts.len(),
        });
    }

    let items = prune_dominated_item_combinations(
        area_item_combinations(area_item_percent),
        cards,
        area_item_percent,
    );
    let mut search_metrics = PtMaximizeMedleyMetrics {
        item_count: items.len(),
        ..PtMaximizeMedleyMetrics::default()
    };
    let bounds_start = Timer::start();
    let mut bounded_items = items
        .into_iter()
        .map(|items| {
            let upper_bound =
                medley_same_team_item_score_upper_bound(cards, charts, area_item_percent, &items)
                    .map_err(|error| PtMaximizeError::MedleyCandidate(error.to_string()))?;
            Ok((items, upper_bound))
        })
        .collect::<Result<Vec<_>, PtMaximizeError>>()?;
    bounded_items.sort_unstable_by_key(|(_, upper_bound)| std::cmp::Reverse(*upper_bound));
    search_metrics.item_upper_bound_ms = bounds_start.elapsed_ms();
    if trace_enabled() {
        eprintln!(
            "PT medley item bounds: count={} elapsed_ms={:.3}",
            bounded_items.len(),
            bounds_start.elapsed_ms(),
        );
    }
    let mut best = None;
    for (item_idx, (selected_items, upper_bound)) in bounded_items.into_iter().enumerate() {
        if best.as_ref().is_some_and(|current: &EvaluatedPlan| {
            AveragePt {
                pt_sum: u128::from(medley_three_song_points(i64::from(upper_bound))),
                sample_count: 1,
            } < current.result.average_pt
        }) {
            continue;
        }
        let candidate = search_medley_for_items(
            cards,
            charts,
            area_item_percent,
            &selected_items,
            best.as_ref().map(|current| &current.result),
            &mut search_metrics,
        )?;
        let Some(candidate) = candidate else {
            if trace_enabled() {
                eprintln!(
                    "PT medley item rejected by shared incumbent: index={} items={:?} upper_bound={} total_elapsed_ms={:.3}",
                    item_idx,
                    selected_items,
                    upper_bound,
                    total_start.elapsed_ms(),
                );
            }
            continue;
        };
        if trace_enabled() {
            eprintln!(
                "PT medley item complete: index={} items={:?} upper_bound={} average_pt={:.6} total_elapsed_ms={:.3}",
                item_idx,
                selected_items,
                upper_bound,
                candidate.result.average_pt.as_f64(),
                total_start.elapsed_ms(),
            );
        }
        if best
            .as_ref()
            .is_none_or(|current: &EvaluatedPlan| better_result(&candidate.result, &current.result))
        {
            best = Some(candidate);
        }
    }
    let result = best
        .map(|plan| plan.result)
        .ok_or(PtMaximizeError::NoResult)?;
    search_metrics.total_elapsed_ms = total_start.elapsed_ms();
    Ok((result, search_metrics))
}

fn search_medley_for_items(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    global_incumbent: Option<&PtMaximizeMedleyResult>,
    metrics: &mut PtMaximizeMedleyMetrics,
) -> Result<Option<EvaluatedPlan>, PtMaximizeError> {
    if let Some(global_incumbent) = global_incumbent {
        search_medley_for_items_above(
            cards,
            charts,
            area_item_percent,
            selected_items,
            global_incumbent,
            metrics,
        )
    } else {
        search_medley_for_items_seeded(cards, charts, area_item_percent, selected_items, metrics)
            .map(Some)
    }
}

fn search_medley_for_items_seeded(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    metrics: &mut PtMaximizeMedleyMetrics,
) -> Result<EvaluatedPlan, PtMaximizeError> {
    let options = TeamGenerationOptions::default();
    let seed_raw_indices = seed_medley_raw_team_indices_for_items(
        cards,
        charts,
        area_item_percent,
        selected_items,
        options,
    )
    .map_err(|error| PtMaximizeError::MedleyCandidate(error.to_string()))?
    .ok_or(PtMaximizeError::NoResult)?;
    let card_stats = adjusted_card_stats(cards, area_item_percent, selected_items);
    let mut seed_scratch = ExactScoreScratch::default();
    let seed_candidates = seed_raw_indices
        .map(|indices| {
            let stat = crate::floor_team_stat(indices.iter().map(|&idx| card_stats[idx]));
            mean_candidate_from_indices(cards, charts, indices, stat, &mut seed_scratch)
        })
        .transpose_array()?;
    let initial_seed_mean = (0..SONG_COUNT)
        .map(|song| seed_candidates[song].mean_numerators[song])
        .sum::<i64>();
    let initial_mean_band_floor = initial_seed_mean
        .saturating_sub((MEDLEY_SCORE_DIVISOR as i64) * RANDOM_SKILL_ORDER_COUNT as i64);
    let fixed_score_floor = (initial_mean_band_floor
        .saturating_add(RANDOM_SKILL_ORDER_COUNT as i64 - 1)
        / RANDOM_SKILL_ORDER_COUNT as i64)
        .saturating_sub(1)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;

    let raw_start = Timer::start();
    let mut raw = match build_raw_team_candidates_with_fixed_score_floor(
        cards,
        charts,
        area_item_percent,
        selected_items,
        options,
        fixed_score_floor,
    ) {
        Ok(raw) => raw,
        Err(TeamBuildError::NotEnoughCards { count: 0 }) => Vec::new(),
        Err(error) => return Err(PtMaximizeError::MedleyCandidate(error.to_string())),
    };
    let raw_candidate_count = raw.len();
    retain_raw_score_band_candidates(&mut raw, initial_mean_band_floor);
    metrics.raw_candidate_count = metrics
        .raw_candidate_count
        .saturating_add(raw_candidate_count);
    if trace_enabled() {
        eprintln!(
            "PT medley raw score-band filter: before={} after={} floor={}",
            raw_candidate_count,
            raw.len(),
            initial_mean_band_floor,
        );
    }
    let raw_ms = raw_start.elapsed_ms();
    let mean_start = Timer::start();
    let mut candidates = build_mean_candidates(cards, charts, raw)?;
    let mean_ms = mean_start.elapsed_ms();
    metrics.candidate_build_ms += raw_ms + mean_ms;

    let fallback_seed_indices = std::array::from_fn(|song| {
        if let Some(index) = candidates
            .iter()
            .position(|candidate| candidate.card_ids == seed_candidates[song].card_ids)
        {
            index
        } else {
            candidates.push(seed_candidates[song].clone());
            candidates.len() - 1
        }
    });
    let fallback_seed_raw_indices =
        fallback_seed_indices.map(|index| candidates[index].raw_indices);
    let initial_candidate_count = candidates.len();
    retain_mean_band_candidates_with_required(
        &mut candidates,
        initial_mean_band_floor,
        &fallback_seed_raw_indices,
    );
    let fallback_seed_indices = remap_candidate_indices(&candidates, fallback_seed_raw_indices)?;
    if trace_enabled() {
        eprintln!(
            "PT medley initial mean-band filter: before={} after={} floor={}",
            initial_candidate_count,
            candidates.len(),
            initial_mean_band_floor,
        );
    }
    compact_candidate_masks(&mut candidates);
    let seed_start = Timer::start();
    let seed_indices = approximate_mean_seed(&candidates)
        .filter(|&indices| {
            plan_mean_numerator(&candidates, indices)
                > plan_mean_numerator(&candidates, fallback_seed_indices)
        })
        .unwrap_or(fallback_seed_indices);
    let seed_ms = seed_start.elapsed_ms();
    metrics.seed_ms += seed_ms;
    let seed_mean = plan_mean_numerator(&candidates, seed_indices);
    let mean_band_floor =
        seed_mean.saturating_sub((MEDLEY_SCORE_DIVISOR as i64) * RANDOM_SKILL_ORDER_COUNT as i64);
    let seed_raw_indices = seed_indices.map(|index| candidates[index].raw_indices);
    let seeded_candidate_count = candidates.len();
    retain_mean_band_candidates_with_required(&mut candidates, mean_band_floor, &seed_raw_indices);
    let seed_indices = remap_candidate_indices(&candidates, seed_raw_indices)?;
    if candidates.len() != seeded_candidate_count {
        compact_candidate_masks(&mut candidates);
    }
    metrics.retained_candidate_count = metrics
        .retained_candidate_count
        .saturating_add(candidates.len());
    if trace_enabled() {
        eprintln!(
            "PT medley seed mean-band filter: before={} after={} floor={}",
            seeded_candidate_count,
            candidates.len(),
            mean_band_floor,
        );
    }
    let mut distributions = HashMap::new();
    let mut best = evaluate_plan(
        &candidates,
        charts,
        selected_items,
        seed_indices,
        &mut distributions,
    )?;
    if trace_enabled() {
        eprintln!(
            "PT medley item stages: items={:?} candidates={} raw_ms={:.3} mean_ms={:.3} seed_ms={:.3}",
            selected_items,
            candidates.len(),
            raw_ms,
            mean_ms,
            seed_ms,
        );
    }

    let scan_start = Timer::start();
    let search_floor = mean_band_floor.max(mean_numerator_to_match(best.result.average_pt));
    let scores = candidates
        .iter()
        .map(|candidate| candidate.mean_numerators)
        .collect::<Vec<_>>();
    let mut evaluation_error = None;
    let mut visit = |indices, _mean_numerator| match evaluate_plan(
        &candidates,
        charts,
        selected_items,
        indices,
        &mut distributions,
    ) {
        Ok(evaluated) => {
            if better_result(&evaluated.result, &best.result) {
                best = evaluated;
            }
            MedleyBandVisit::Continue {
                floor: mean_band_floor.max(mean_numerator_to_match(best.result.average_pt)),
            }
        }
        Err(error) => {
            evaluation_error = Some(error);
            MedleyBandVisit::Break
        }
    };
    let solver_metrics = if candidates[0].mask_words.len() <= 1 {
        enumerate_medley_band(
            &MedleyBandInput {
                floor: search_floor,
                team_masks: candidates
                    .iter()
                    .map(|candidate| candidate.mask_words[0])
                    .collect(),
                scores,
            },
            &mut visit,
        )
    } else {
        enumerate_medley_band_wide(
            &WideMedleyBandInput {
                floor: search_floor,
                team_masks: candidates
                    .iter()
                    .map(|candidate| candidate.mask_words.clone())
                    .collect(),
                scores,
            },
            &mut visit,
        )
    }
    .map_err(|error| PtMaximizeError::MedleySolver(error.to_string()))?;
    drop(visit);
    if let Some(error) = evaluation_error {
        return Err(error);
    }
    if trace_enabled() {
        eprintln!(
            "PT medley near band: candidates={} pair_checks={} triples_in_band={} mutually_exclusive={} implementation={:?} cached_distributions={} scan_ms={:.3}",
            candidates.len(),
            solver_metrics.pair_checks,
            solver_metrics.third_checks,
            solver_metrics.compatible_triples,
            solver_metrics.implementation,
            distributions.len(),
            scan_start.elapsed_ms(),
        );
    }
    metrics.solve_ms += scan_start.elapsed_ms();
    metrics.pair_check_count = metrics
        .pair_check_count
        .saturating_add(solver_metrics.pair_checks as u64);
    metrics.third_check_count = metrics
        .third_check_count
        .saturating_add(solver_metrics.third_checks as u64);
    metrics.compatible_plan_count = metrics
        .compatible_plan_count
        .saturating_add(solver_metrics.compatible_triples as u64);
    metrics.exact_distribution_count = metrics
        .exact_distribution_count
        .saturating_add(distributions.len());
    Ok(best)
}

fn search_medley_for_items_above(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    global_incumbent: &PtMaximizeMedleyResult,
    metrics: &mut PtMaximizeMedleyMetrics,
) -> Result<Option<EvaluatedPlan>, PtMaximizeError> {
    let options = TeamGenerationOptions::default();
    let mean_floor = mean_numerator_to_match(global_incumbent.average_pt);
    let fixed_score_floor = (mean_floor.saturating_add(RANDOM_SKILL_ORDER_COUNT as i64 - 1)
        / RANDOM_SKILL_ORDER_COUNT as i64)
        .saturating_sub(1)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;

    let raw_start = Timer::start();
    let mut raw = match build_raw_team_candidates_with_fixed_score_floor(
        cards,
        charts,
        area_item_percent,
        selected_items,
        options,
        fixed_score_floor,
    ) {
        Ok(raw) => raw,
        Err(TeamBuildError::NotEnoughCards { count: 0 }) => Vec::new(),
        Err(error) => return Err(PtMaximizeError::MedleyCandidate(error.to_string())),
    };
    let raw_candidate_count = raw.len();
    retain_raw_score_band_candidates(&mut raw, mean_floor);
    metrics.raw_candidate_count = metrics
        .raw_candidate_count
        .saturating_add(raw_candidate_count);
    if trace_enabled() {
        eprintln!(
            "PT medley raw score-band filter: before={} after={} floor={}",
            raw_candidate_count,
            raw.len(),
            mean_floor,
        );
    }
    let raw_ms = raw_start.elapsed_ms();
    if raw.is_empty() {
        metrics.candidate_build_ms += raw_ms;
        if trace_enabled() {
            eprintln!(
                "PT medley shared incumbent stages: items={:?} candidates=0 raw_ms={:.3} mean_ms=0.000 seed_ms=0.000",
                selected_items,
                raw_ms,
            );
        }
        return Ok(None);
    }

    let mean_start = Timer::start();
    let mut candidates = build_mean_candidates(cards, charts, raw)?;
    let mean_ms = mean_start.elapsed_ms();
    metrics.candidate_build_ms += raw_ms + mean_ms;
    let candidate_count = candidates.len();
    retain_mean_band_candidates_with_required(&mut candidates, mean_floor, &[]);
    if candidates.is_empty() {
        if trace_enabled() {
            eprintln!(
                "PT medley shared incumbent stages: items={:?} candidates=0 mean_candidates={} raw_ms={:.3} mean_ms={:.3} seed_ms=0.000",
                selected_items,
                candidate_count,
                raw_ms,
                mean_ms,
            );
        }
        return Ok(None);
    }
    metrics.retained_candidate_count = metrics
        .retained_candidate_count
        .saturating_add(candidates.len());
    compact_candidate_masks(&mut candidates);
    if trace_enabled() {
        eprintln!(
            "PT medley shared incumbent stages: items={:?} candidates={} mean_candidates={} raw_ms={:.3} mean_ms={:.3} seed_ms=0.000",
            selected_items,
            candidates.len(),
            candidate_count,
            raw_ms,
            mean_ms,
        );
    }

    let scan_start = Timer::start();
    let scores = candidates
        .iter()
        .map(|candidate| candidate.mean_numerators)
        .collect::<Vec<_>>();
    let mut distributions = HashMap::new();
    let mut best: Option<EvaluatedPlan> = None;
    let mut evaluation_error = None;
    let mut visit = |indices, _mean_numerator| match evaluate_plan(
        &candidates,
        charts,
        selected_items,
        indices,
        &mut distributions,
    ) {
        Ok(evaluated) => {
            if better_result(&evaluated.result, global_incumbent)
                && best
                    .as_ref()
                    .is_none_or(|current| better_result(&evaluated.result, &current.result))
            {
                best = Some(evaluated);
            }
            MedleyBandVisit::Continue {
                floor: best
                    .as_ref()
                    .map(|current| mean_numerator_to_match(current.result.average_pt))
                    .unwrap_or(mean_floor)
                    .max(mean_floor),
            }
        }
        Err(error) => {
            evaluation_error = Some(error);
            MedleyBandVisit::Break
        }
    };
    let solver_metrics = if candidates[0].mask_words.len() <= 1 {
        enumerate_medley_band(
            &MedleyBandInput {
                floor: mean_floor,
                team_masks: candidates
                    .iter()
                    .map(|candidate| candidate.mask_words[0])
                    .collect(),
                scores,
            },
            &mut visit,
        )
    } else {
        enumerate_medley_band_wide(
            &WideMedleyBandInput {
                floor: mean_floor,
                team_masks: candidates
                    .iter()
                    .map(|candidate| candidate.mask_words.clone())
                    .collect(),
                scores,
            },
            &mut visit,
        )
    }
    .map_err(|error| PtMaximizeError::MedleySolver(error.to_string()))?;
    drop(visit);
    if let Some(error) = evaluation_error {
        return Err(error);
    }
    if trace_enabled() {
        eprintln!(
            "PT medley shared incumbent band: candidates={} pair_checks={} triples_in_band={} mutually_exclusive={} implementation={:?} cached_distributions={} scan_ms={:.3}",
            candidates.len(),
            solver_metrics.pair_checks,
            solver_metrics.third_checks,
            solver_metrics.compatible_triples,
            solver_metrics.implementation,
            distributions.len(),
            scan_start.elapsed_ms(),
        );
    }
    metrics.solve_ms += scan_start.elapsed_ms();
    metrics.pair_check_count = metrics
        .pair_check_count
        .saturating_add(solver_metrics.pair_checks as u64);
    metrics.third_check_count = metrics
        .third_check_count
        .saturating_add(solver_metrics.third_checks as u64);
    metrics.compatible_plan_count = metrics
        .compatible_plan_count
        .saturating_add(solver_metrics.compatible_triples as u64);
    metrics.exact_distribution_count = metrics
        .exact_distribution_count
        .saturating_add(distributions.len());
    Ok(best)
}

fn retain_mean_band_candidates_with_required(
    candidates: &mut Vec<MedleyCandidate>,
    floor: i64,
    required_raw_indices: &[[usize; TEAM_SIZE]],
) {
    let means = candidates
        .iter()
        .map(|candidate| candidate.mean_numerators)
        .collect::<Vec<_>>();
    let active_indices = mean_band_candidate_indices(&means, floor);
    if active_indices.len() == candidates.len() {
        return;
    }
    let mut active = vec![false; candidates.len()];
    for index in active_indices {
        active[index] = true;
    }
    for (index, candidate) in candidates.iter().enumerate() {
        if required_raw_indices.contains(&candidate.raw_indices) {
            active[index] = true;
        }
    }
    let mut index = 0;
    candidates.retain(|_| {
        let keep = active[index];
        index += 1;
        keep
    });
}

fn mean_band_candidate_indices(means: &[[i64; SONG_COUNT]], floor: i64) -> Vec<usize> {
    let maxima: [i64; SONG_COUNT] = std::array::from_fn(|song| {
        means
            .iter()
            .map(|values| values[song])
            .max()
            .unwrap_or(i64::MIN)
    });
    means
        .iter()
        .enumerate()
        .filter_map(|(index, values)| {
            (0..SONG_COUNT)
                .any(|song| {
                    values[song]
                        .saturating_add(maxima[(song + 1) % SONG_COUNT])
                        .saturating_add(maxima[(song + 2) % SONG_COUNT])
                        >= floor
                })
                .then_some(index)
        })
        .collect()
}

fn retain_raw_score_band_candidates(candidates: &mut Vec<RawTeamCandidate>, floor: i64) {
    let scores = candidates
        .iter()
        .map(|candidate| candidate.scores)
        .collect::<Vec<_>>();
    let active_indices = raw_score_band_candidate_indices(&scores, floor);
    if active_indices.len() == candidates.len() {
        return;
    }
    let mut active = vec![false; candidates.len()];
    for index in active_indices {
        active[index] = true;
    }
    let mut index = 0;
    candidates.retain(|_| {
        let keep = active[index];
        index += 1;
        keep
    });
}

fn raw_score_band_candidate_indices(scores: &[[i32; SONG_COUNT]], floor: i64) -> Vec<usize> {
    let maxima: [i64; SONG_COUNT] = std::array::from_fn(|song| {
        scores
            .iter()
            .map(|values| i64::from(values[song]).saturating_mul(RANDOM_SKILL_ORDER_COUNT as i64))
            .max()
            .unwrap_or(i64::MIN)
    });
    scores
        .iter()
        .enumerate()
        .filter_map(|(index, values)| {
            (0..SONG_COUNT)
                .any(|song| {
                    let other_max = (0..SONG_COUNT)
                        .filter(|&other| other != song)
                        .fold(0i64, |sum, other| sum.saturating_add(maxima[other]));
                    i64::from(values[song])
                        .saturating_mul(RANDOM_SKILL_ORDER_COUNT as i64)
                        .saturating_add(other_max)
                        >= floor
                })
                .then_some(index)
        })
        .collect()
}

fn remap_candidate_indices(
    candidates: &[MedleyCandidate],
    raw_indices: [[usize; TEAM_SIZE]; SONG_COUNT],
) -> Result<[usize; SONG_COUNT], PtMaximizeError> {
    raw_indices
        .map(|target| {
            candidates
                .iter()
                .position(|candidate| candidate.raw_indices == target)
                .ok_or(PtMaximizeError::NoResult)
        })
        .transpose_array()
}

fn trace_enabled() -> bool {
    std::env::var_os("BANGDREAM_OPTIMIZE_PT_TRACE").is_some()
}

fn build_mean_candidates(
    cards: &[PreparedCard],
    charts: &[Chart],
    raw_candidates: Vec<RawTeamCandidate>,
) -> Result<Vec<MedleyCandidate>, PtMaximizeError> {
    let mut scratch = ExactScoreScratch::default();
    raw_candidates
        .into_iter()
        .map(|raw| {
            mean_candidate_from_indices(cards, charts, raw.raw_indices, raw.stat, &mut scratch)
        })
        .collect()
}

fn mean_candidate_from_indices(
    cards: &[PreparedCard],
    charts: &[Chart],
    raw_indices: [usize; TEAM_SIZE],
    stat: i32,
    scratch: &mut ExactScoreScratch,
) -> Result<MedleyCandidate, PtMaximizeError> {
    let selected = raw_indices.map(|idx| &cards[idx]);
    let mode = selected_team_mode(&selected);
    let skills = selected
        .map(|card| mode.resolve_skill(card))
        .transpose_array()?;
    let mut mean_numerators = [0; SONG_COUNT];
    for song in 0..SONG_COUNT {
        mean_numerators[song] = best_mean_numerator(&charts[song], &skills, stat, scratch)?;
    }
    let mut card_ids = selected.map(|card| card.card_id).to_vec();
    card_ids.sort_unstable();
    Ok(MedleyCandidate {
        raw_indices,
        mask_words: Vec::new(),
        card_ids,
        skills,
        stat,
        mean_numerators,
    })
}

fn compact_candidate_masks(candidates: &mut [MedleyCandidate]) {
    let used = candidates
        .iter()
        .flat_map(|candidate| candidate.raw_indices)
        .collect::<BTreeSet<_>>();
    let positions = used
        .into_iter()
        .enumerate()
        .map(|(position, raw_idx)| (raw_idx, position))
        .collect::<HashMap<_, _>>();
    let word_count = positions.len().div_ceil(64).max(1);
    for candidate in candidates {
        candidate.mask_words = vec![0; word_count];
        for raw_idx in candidate.raw_indices {
            let position = positions[&raw_idx];
            candidate.mask_words[position / 64] |= 1u64 << (position % 64);
        }
    }
}

trait TransposeArray<T, E, const N: usize> {
    fn transpose_array(self) -> Result<[T; N], E>;
}

impl<T, E, const N: usize> TransposeArray<T, E, N> for [Result<T, E>; N] {
    fn transpose_array(self) -> Result<[T; N], E> {
        self.into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map(|values| {
                values
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("array length is preserved"))
            })
    }
}

fn selected_team_mode(cards: &[&PreparedCard; TEAM_SIZE]) -> SongMode {
    let band_id = cards[0].band_id;
    let attribute = cards[0].attribute;
    let unified_band = cards.iter().all(|card| card.band_id == band_id);
    let unified_attribute = cards.iter().all(|card| card.attribute == attribute);
    match (unified_band, unified_attribute) {
        (true, true) => SongMode::UnifiedBandAttribute(band_id, attribute),
        (true, false) => SongMode::UnifiedBand(band_id),
        (false, true) => SongMode::UnifiedAttribute(attribute),
        (false, false) => SongMode::Mixed,
    }
}

fn best_mean_numerator(
    chart: &Chart,
    skills: &[TeamCardSkill; TEAM_SIZE],
    stat: i32,
    scratch: &mut ExactScoreScratch,
) -> Result<i64, PtMaximizeError> {
    if let Some(matrix) = chart.independent_skill_score_matrix(skills, stat, true, scratch)? {
        let captain = (0..TEAM_SIZE)
            .max_by_key(|&idx| {
                (
                    matrix.deltas[idx][5],
                    std::cmp::Reverse(skills[idx].card_id),
                )
            })
            .expect("a team has five cards");
        return Ok(
            RANDOM_SKILL_ORDER_COUNT as i64 * i64::from(matrix.base_score)
                + (RANDOM_SKILL_ORDER_COUNT as i64 / TEAM_SIZE as i64)
                    * matrix
                        .deltas
                        .iter()
                        .flat_map(|row| row[..TEAM_SIZE].iter())
                        .map(|&delta| i64::from(delta))
                        .sum::<i64>()
                + RANDOM_SKILL_ORDER_COUNT as i64 * i64::from(matrix.deltas[captain][5]),
        );
    }
    Ok(full_team_score_distributions(chart, skills, stat, true)?
        .into_iter()
        .map(|distribution| distribution.distribution.score_sum)
        .max()
        .ok_or(PtMaximizeError::EmptyDistribution)?)
}

fn plan_mean_numerator(candidates: &[MedleyCandidate], indices: [usize; SONG_COUNT]) -> i64 {
    (0..SONG_COUNT)
        .map(|song| candidates[indices[song]].mean_numerators[song])
        .sum()
}

fn mean_numerator_to_match(average_pt: AveragePt) -> i64 {
    let samples = u128::from(average_pt.sample_count);
    let variable_pt_sum = average_pt
        .pt_sum
        .saturating_sub(100u128.saturating_mul(samples));
    let numerator = u128::from(MEDLEY_SCORE_DIVISOR)
        .saturating_mul(u128::from(RANDOM_SKILL_ORDER_COUNT))
        .saturating_mul(variable_pt_sum);
    numerator.div_ceil(samples).min(i64::MAX as u128) as i64
}

fn approximate_mean_seed(candidates: &[MedleyCandidate]) -> Option<[usize; SONG_COUNT]> {
    let scores = candidates
        .iter()
        .map(|candidate| {
            candidate.mean_numerators.map(|value| {
                (value / RANDOM_SKILL_ORDER_COUNT as i64)
                    .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
            })
        })
        .collect::<Vec<_>>();
    if candidates.first()?.mask_words.len() <= 1 {
        solve_medley_seed_with_rounds(
            &MedleySolverInput {
                current_best: 0,
                team_masks: candidates
                    .iter()
                    .map(|candidate| candidate.mask_words[0])
                    .collect(),
                scores,
            },
            MEAN_SEED_RANDOM_BUCKET_ROUNDS,
        )
        .ok()
        .map(|plan| plan.indices)
    } else {
        solve_medley_wide_seed_with_rounds(
            &WideMedleySolverInput {
                current_best: 0,
                team_masks: candidates
                    .iter()
                    .map(|candidate| candidate.mask_words.clone())
                    .collect(),
                scores,
            },
            MEAN_SEED_RANDOM_BUCKET_ROUNDS,
        )
        .ok()
        .map(|plan| plan.indices)
    }
}

fn candidate_distributions(
    candidates: &[MedleyCandidate],
    charts: &[Chart],
    candidate_idx: usize,
    song_idx: usize,
    cache: &mut HashMap<(usize, usize), Vec<CaptainScoreDistribution>>,
) -> Result<Vec<CaptainScoreDistribution>, PtMaximizeError> {
    if let Some(cached) = cache.get(&(candidate_idx, song_idx)) {
        return Ok(cached.clone());
    }
    let candidate = &candidates[candidate_idx];
    let value =
        full_team_score_distributions(&charts[song_idx], &candidate.skills, candidate.stat, true)?;
    cache.insert((candidate_idx, song_idx), value.clone());
    Ok(value)
}

fn evaluate_plan(
    candidates: &[MedleyCandidate],
    charts: &[Chart],
    items: &SelectedAreaItems,
    candidate_indices: [usize; SONG_COUNT],
    cache: &mut HashMap<(usize, usize), Vec<CaptainScoreDistribution>>,
) -> Result<EvaluatedPlan, PtMaximizeError> {
    let mut distribution_values = Vec::with_capacity(SONG_COUNT);
    for song in 0..SONG_COUNT {
        distribution_values.push(candidate_distributions(
            candidates,
            charts,
            candidate_indices[song],
            song,
            cache,
        )?);
    }
    let distributions: [Vec<CaptainScoreDistribution>; SONG_COUNT] = distribution_values
        .try_into()
        .unwrap_or_else(|_| unreachable!("three songs were collected"));
    let mut best: Option<PtMaximizeMedleyResult> = None;
    for first in &distributions[0] {
        for second in &distributions[1] {
            for third in &distributions[2] {
                let selected = [first, second, third];
                let result = medley_result_for_distributions(
                    candidates,
                    candidate_indices,
                    items,
                    selected,
                )?;
                if best
                    .as_ref()
                    .is_none_or(|current| better_result(&result, current))
                {
                    best = Some(result);
                }
            }
        }
    }
    Ok(EvaluatedPlan {
        result: best.ok_or(PtMaximizeError::EmptyDistribution)?,
    })
}

fn medley_result_for_distributions(
    candidates: &[MedleyCandidate],
    candidate_indices: [usize; SONG_COUNT],
    items: &SelectedAreaItems,
    distributions: [&CaptainScoreDistribution; SONG_COUNT],
) -> Result<PtMaximizeMedleyResult, PtMaximizeError> {
    let histograms = distributions.map(|value| &value.distribution);
    let (pt_sum, sample_count) = medley_pt_sum(histograms)?;
    let min_score = histograms
        .iter()
        .map(|histogram| i64::from(histogram.min_score))
        .sum();
    let max_score = histograms
        .iter()
        .map(|histogram| i64::from(histogram.max_score))
        .sum();
    let total_score_sum = histograms
        .iter()
        .map(|histogram| {
            i128::from(histogram.score_sum) * i128::from(sample_count / histogram.sample_count)
        })
        .sum();
    let teams = (0..SONG_COUNT)
        .map(|song| {
            let candidate = &candidates[candidate_indices[song]];
            PtMaximizeMedleyTeamResult {
                team_card_ids: candidate.card_ids.clone(),
                captain_card_id: distributions[song].captain_card_id,
                total_stat: candidate.stat,
                items: items.clone(),
                score_distribution: distributions[song].distribution.clone(),
            }
        })
        .collect();
    Ok(PtMaximizeMedleyResult {
        teams,
        average_pt: AveragePt::new(pt_sum, sample_count)?,
        min_pt: medley_three_song_points(min_score),
        max_pt: medley_three_song_points(max_score),
        total_score_sum,
        sample_count,
    })
}

fn medley_pt_sum(
    histograms: [&ScoreHistogram; SONG_COUNT],
) -> Result<(u128, u64), PtMaximizeError> {
    if histograms
        .iter()
        .any(|histogram| histogram.sample_count == 0)
    {
        return Err(PtMaximizeError::EmptyDistribution);
    }
    let sample_count = histograms
        .iter()
        .try_fold(1u64, |value, histogram| {
            value.checked_mul(histogram.sample_count)
        })
        .ok_or(PtMaximizeError::EmptyDistribution)?;
    let divisor = MEDLEY_SCORE_DIVISOR as i32;
    let mut quotient_sums = [0u128; SONG_COUNT];
    let remainders: [Vec<(i32, u64)>; SONG_COUNT] = std::array::from_fn(|song| {
        let mut values = BTreeMap::new();
        for &(score, count) in &histograms[song].entries {
            quotient_sums[song] += (score.max(0) / divisor) as u128 * u128::from(count);
            *values.entry(score.max(0) % divisor).or_insert(0) += count;
        }
        values.into_iter().collect()
    });
    let total_samples = u128::from(sample_count);
    let mut pt_sum = 100u128 * total_samples;
    for song in 0..SONG_COUNT {
        pt_sum += quotient_sums[song] * (total_samples / u128::from(histograms[song].sample_count));
    }

    let mut pair_remainders = BTreeMap::<i32, u64>::new();
    for &(left, left_count) in &remainders[0] {
        for &(right, right_count) in &remainders[1] {
            *pair_remainders.entry(left + right).or_insert(0) += left_count * right_count;
        }
    }
    for (pair, pair_count) in pair_remainders {
        for &(third, third_count) in &remainders[2] {
            pt_sum += ((pair + third) / divisor) as u128
                * u128::from(pair_count)
                * u128::from(third_count);
        }
    }
    Ok((pt_sum, sample_count))
}

fn better_result(candidate: &PtMaximizeMedleyResult, current: &PtMaximizeMedleyResult) -> bool {
    if candidate.average_pt != current.average_pt {
        return candidate.average_pt > current.average_pt;
    }
    let score_order = compare_nonnegative_averages(
        candidate.total_score_sum,
        candidate.sample_count,
        current.total_score_sum,
        current.sample_count,
    );
    score_order == std::cmp::Ordering::Greater
        || (score_order == std::cmp::Ordering::Equal
            && canonical_result_key(candidate) < canonical_result_key(current))
}

fn canonical_result_key(result: &PtMaximizeMedleyResult) -> Vec<(Vec<u32>, u32)> {
    result
        .teams
        .iter()
        .map(|team| (team.team_card_ids.clone(), team.captain_card_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remainder_convolution_matches_direct_three_way_sum() {
        let histogram = |entries: Vec<(i32, u64)>| ScoreHistogram {
            score_sum: entries
                .iter()
                .map(|&(score, count)| i64::from(score) * count as i64)
                .sum(),
            min_score: entries.first().unwrap().0,
            max_score: entries.last().unwrap().0,
            sample_count: entries.iter().map(|entry| entry.1).sum(),
            entries,
        };
        let values = [
            histogram(vec![(18_499, 1), (18_500, 2)]),
            histogram(vec![(1, 3), (9_250, 1)]),
            histogram(vec![(9_249, 1), (18_499, 2)]),
        ];
        let (actual, samples) = medley_pt_sum([&values[0], &values[1], &values[2]]).unwrap();
        let mut expected = 0u128;
        for &(a, ac) in &values[0].entries {
            for &(b, bc) in &values[1].entries {
                for &(c, cc) in &values[2].entries {
                    expected += u128::from(medley_three_song_points(i64::from(a + b + c)))
                        * u128::from(ac * bc * cc);
                }
            }
        }
        assert_eq!(actual, expected);
        assert_eq!(samples, 36);
    }

    #[test]
    fn equal_average_pt_prefers_higher_average_score() {
        let result = |score_sum| PtMaximizeMedleyResult {
            teams: Vec::new(),
            average_pt: AveragePt::new(1_000, 2).unwrap(),
            min_pt: 500,
            max_pt: 500,
            total_score_sum: score_sum,
            sample_count: 2,
        };

        assert!(better_result(&result(2_001), &result(2_000)));
        assert!(!better_result(&result(2_000), &result(2_001)));
    }

    #[test]
    fn mean_band_filter_keeps_candidate_that_can_reach_floor_in_any_song() {
        let means = vec![[100, 0, 0], [0, 100, 0], [0, 0, 100], [1, 1, 1]];

        assert_eq!(mean_band_candidate_indices(&means, 250), vec![0, 1, 2]);
    }

    #[test]
    fn raw_score_band_filter_treats_best_order_as_mean_upper_bound() {
        let scores = vec![[100, 0, 0], [0, 100, 0], [0, 0, 100], [1, 1, 1]];

        assert_eq!(
            raw_score_band_candidate_indices(&scores, 250 * RANDOM_SKILL_ORDER_COUNT as i64),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn mean_numerator_match_floor_uses_exact_ceiling() {
        assert_eq!(
            mean_numerator_to_match(AveragePt {
                pt_sum: 100,
                sample_count: 1,
            }),
            0
        );
        assert_eq!(
            mean_numerator_to_match(AveragePt {
                pt_sum: 101,
                sample_count: 1,
            }),
            MEDLEY_SCORE_DIVISOR as i64 * RANDOM_SKILL_ORDER_COUNT as i64
        );
        assert_eq!(
            mean_numerator_to_match(AveragePt {
                pt_sum: 701,
                sample_count: 7,
            }),
            317_143
        );
    }
}
