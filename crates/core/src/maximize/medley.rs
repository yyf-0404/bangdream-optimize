use super::{trace_enabled, CalculationError, ItemSearchOptions};
use crate::medley::candidate::{calculate_medley_from_raw_candidates, RawCandidateBuildRequest};
use crate::medley::team::{
    build_raw_team_candidates_with_current_best, medley_same_team_item_score_upper_bound,
};
use crate::model::chart::Chart;
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::{
    BuildResult, MedleyCalculationMetrics, SelectedAreaItems, SongSelection,
};
use crate::timing::{optional_elapsed_ms, Timer};

#[derive(Debug, Default, Clone)]
pub(super) struct MedleySearchMetrics {
    candidate_count: usize,
    solver_candidate_count: usize,
    solver_filter_ms: f64,
    solver_ms: f64,
    seed_ms: f64,
    item_upper_bound_ms: f64,
    candidate_build_ms: f64,
    candidate_build_count: usize,
    solver_count: usize,
    seed_count: usize,
    item_upper_bound_count: usize,
    best_used_card_count: Option<usize>,
}

impl MedleySearchMetrics {
    pub(super) fn add_seed_ms(&mut self, elapsed_ms: f64) {
        self.seed_count += 1;
        self.seed_ms += elapsed_ms;
    }

    pub(super) fn add_item_upper_bound_ms(&mut self, elapsed_ms: f64) {
        self.item_upper_bound_count += 1;
        self.item_upper_bound_ms += elapsed_ms;
    }

    pub(super) fn add_candidate_build(&mut self, candidate_count: usize, elapsed_ms: f64) {
        self.candidate_count += candidate_count;
        self.candidate_build_ms += elapsed_ms;
        self.candidate_build_count += 1;
    }

    pub(super) fn add_solver_result(&mut self, result: &BuildResult) {
        let Some(metrics) = result
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.medley.as_ref())
        else {
            return;
        };
        self.solver_candidate_count += metrics.solver_candidate_count;
        self.solver_filter_ms += metrics.solver_filter_ms;
        self.solver_ms += metrics.solver_ms;
        self.solver_count += 1;
    }

    pub(super) fn add_solver_error_ms(&mut self, elapsed_ms: f64) {
        self.solver_ms += elapsed_ms;
        self.solver_count += 1;
    }

    pub(super) fn record_best_result(&mut self, result: &BuildResult) {
        self.best_used_card_count = result
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.medley.as_ref())
            .and_then(|metrics| metrics.used_card_count);
    }

    pub(super) fn into_metrics(self) -> MedleyCalculationMetrics {
        MedleyCalculationMetrics {
            candidate_count: self.candidate_count,
            solver_candidate_count: self.solver_candidate_count,
            solver_filter_ms: self.solver_filter_ms,
            solver_ms: self.solver_ms,
            seed_ms: self.seed_ms,
            item_upper_bound_ms: self.item_upper_bound_ms,
            candidate_build_count: self.candidate_build_count,
            solver_count: self.solver_count,
            seed_count: self.seed_count,
            item_upper_bound_count: self.item_upper_bound_count,
            candidate_build_ms: Some(self.candidate_build_ms),
            used_card_count: self.best_used_card_count,
        }
    }
}

pub(super) fn calculate_medley_result_for_items(
    event_id: u32,
    song_list: &[SongSelection],
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    options: &ItemSearchOptions,
    current_best: i32,
    search_metrics: Option<&mut MedleySearchMetrics>,
) -> Result<BuildResult, CalculationError> {
    let trace = trace_enabled();
    let mut search_metrics = search_metrics;
    let build_start = Timer::start();
    let candidates = build_raw_team_candidates_with_current_best(
        cards,
        charts,
        area_item_percent,
        selected_items,
        options.team_generation,
        current_best,
    )?;
    let build_ms = build_start.elapsed_ms();
    if let Some(metrics) = search_metrics.as_mut() {
        metrics.add_candidate_build(candidates.len(), build_ms);
    }

    if trace {
        eprintln!(
            "medley candidates: count={} build_ms={build_ms:.3}",
            candidates.len(),
        );
    }

    let solver_start = Timer::start();
    let result = calculate_medley_from_raw_candidates(RawCandidateBuildRequest {
        event_id,
        song_list,
        candidates: &candidates,
        cards,
        current_best: current_best.saturating_sub(1),
        solver_preference: options.solver_preference,
        items: Some(selected_items.clone()),
    });
    let solver_elapsed_ms = solver_start.elapsed_ms();
    let result = match result {
        Ok(result) => {
            if let Some(metrics) = search_metrics.as_mut() {
                metrics.add_solver_result(&result);
            }
            result
        }
        Err(error) => {
            if let Some(metrics) = search_metrics.as_mut() {
                metrics.add_solver_error_ms(solver_elapsed_ms);
            }
            return Err(error.into());
        }
    };
    let mut result = result;
    if let Some(metrics) = result
        .metrics
        .as_mut()
        .and_then(|metrics| metrics.medley.as_mut())
    {
        metrics.candidate_build_ms = Some(build_ms);
    }

    if trace {
        eprintln!(
            "medley result: solver={} total_score={} solver_ms={:.3}",
            result.solver.as_deref().unwrap_or("unknown"),
            result.total_score,
            solver_elapsed_ms,
        );
    }

    Ok(result)
}

pub(super) fn medley_item_can_beat_incumbent(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    current_best: i32,
) -> Result<bool, CalculationError> {
    if current_best <= 0 {
        return Ok(true);
    }

    let trace = trace_enabled();
    let upper_bound_start = trace.then(Timer::start);
    let upper_bound =
        medley_same_team_item_score_upper_bound(cards, charts, area_item_percent, selected_items)?;
    let upper_bound_ms = elapsed_ms(upper_bound_start);
    let can_beat = upper_bound >= current_best;

    if trace {
        eprintln!(
            "medley same-team item upper bound: items={:?} upper_bound={} current_best={} can_beat={} bound_ms={upper_bound_ms:.3}",
            selected_items, upper_bound, current_best, can_beat,
        );
    }

    Ok(can_beat)
}

fn elapsed_ms(start: Option<Timer>) -> f64 {
    optional_elapsed_ms(start)
}
