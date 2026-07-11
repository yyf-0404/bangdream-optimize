use super::team::{group_items_by_mode, signature_modes};
use super::{
    fire_cost_for_multiplier, points_for_score_with_support,
    score_interval_for_points_with_support, total_fire_cost, ScoreRangeError, ScoreRangePlay,
    ScoreRangeRequest, ScoreRangeSong, ScoreRangeSongDuration, ScoreRangeTeam,
    ScoreRangeTeamDomain, SkillBucketKey, SongKey,
};
use crate::model::preparation::{ALL_ATTRIBUTE_KEY, ALL_BAND_KEY};
use crate::timing::{optional_elapsed_ms, Timer};
use crate::{
    AreaItemPercent, CompressedAutoScore, Magazine, PreparedCard, SelectedAreaItems, SongMode,
    TeamCardSkill,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
struct BucketCard<'a> {
    card: &'a PreparedCard,
    skill: TeamCardSkill,
    point_bonus_micros: u64,
}

#[derive(Clone, Copy)]
struct ResolvedCard<'a> {
    card: &'a PreparedCard,
    stat: f64,
    point_bonus_micros: u64,
}

#[derive(Clone, Copy)]
struct PairRecord {
    stat: f64,
    point_bonus_micros: u64,
    left: usize,
    right: usize,
}

#[derive(Default)]
struct PairIndex {
    by_bonus: BTreeMap<u64, Vec<PairRecord>>,
    record_count: usize,
}

type PairStatBounds = BTreeMap<u64, (f64, f64)>;

enum PairQuery<'a> {
    Full(&'a PairIndex),
    Incremental {
        base: &'a PairIndex,
        adjusted_touching: &'a PairIndex,
        affected_cards: &'a [bool],
    },
}

impl PairQuery<'_> {
    fn base(&self) -> &PairIndex {
        match self {
            Self::Full(base) | Self::Incremental { base, .. } => base,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemAxis {
    Band,
    Attribute,
}

#[derive(Debug, Default)]
struct ItemBatchMetrics {
    outer_band_batches: usize,
    outer_attribute_batches: usize,
    base_queries: usize,
    reused_contexts: usize,
    incremental_queries: usize,
    full_queries: usize,
    lower_bound_pruned_layers: usize,
    upper_bound_pruned_layers: usize,
    lower_bound_pruned_buckets: usize,
    upper_bound_pruned_buckets: usize,
    structural_pruned_contexts: usize,
    max_base_pairs: usize,
    max_adjusted_pairs: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct TeamBounds {
    min_stat: i32,
    max_stat: i32,
    min_bonus_basis_points: u32,
    max_bonus_basis_points: u32,
}

#[derive(Debug, Clone, Copy)]
struct BasePointBounds {
    min: u64,
    max: u64,
}

struct PreparedSkillBucket<'a> {
    skill_key: SkillBucketKey,
    skill: TeamCardSkill,
    cards: Vec<BucketCard<'a>>,
    team_bounds: TeamBounds,
    coarse_base_points: BasePointBounds,
    template_base_points: Option<BasePointBounds>,
    safe_base_points: Option<BasePointBounds>,
}

struct PreparedMode<'a> {
    mode: SongMode,
    item_groups: Vec<&'a SelectedAreaItems>,
    buckets: Vec<PreparedSkillBucket<'a>>,
    coarse_base_points: BasePointBounds,
}

#[derive(Clone)]
struct SongModel {
    key: SongKey,
    exact: CompressedAutoScore,
}

#[derive(Clone, Copy)]
struct NonRateupSongTemplate {
    song_index: usize,
    key: SongKey,
    exact: ScoreRangeSongDuration,
}

#[derive(Clone)]
struct SkillSearchContext {
    songs: Vec<SongModel>,
    intervals: BTreeMap<u32, ValidIntervalSet>,
    score_lower_bounds: BTreeMap<(usize, u64), Option<i32>>,
}

#[derive(Debug, Clone, Copy)]
struct ValidStatInterval {
    min_stat: i32,
    max_stat: i32,
    song_index: usize,
    base_points: u64,
}

#[derive(Debug, Clone, Copy)]
struct StatRange {
    min_stat: i32,
    max_stat: i32,
}

#[derive(Debug, Clone, Copy)]
struct ExactStatRange {
    min_stat: f64,
    max_stat_exclusive: f64,
}

#[derive(Debug, Clone)]
struct ValidIntervalSet {
    union: Vec<StatRange>,
    ranked: Vec<ValidStatInterval>,
}

#[derive(Debug, Clone)]
struct Candidate {
    team: ScoreRangeTeam,
    plan: Vec<ScoreRangePlay>,
}

type CandidateKey = (SkillBucketKey, u32, i32, SongKey);
type CandidateMap = BTreeMap<CandidateKey, Candidate>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PlanObjective {
    play_count: u64,
    total_fire_cost: u64,
}

#[derive(Debug, Default)]
struct LayerTiming {
    mode_preparation_ms: f64,
    skill_contexts_ms: f64,
    score_compression_ms: f64,
    exact_bounds_ms: f64,
    item_search_ms: f64,
}

fn optimistic_term_frontier(mut terms: Vec<(f64, usize)>) -> Vec<(f64, usize)> {
    terms.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
    });
    let mut frontier = Vec::new();
    let mut max_count = 0_usize;
    for term in terms {
        if term.1 <= max_count {
            continue;
        }
        max_count = term.1;
        frontier.push(term);
    }
    frontier
}

fn pessimistic_term_frontier(mut terms: Vec<(f64, usize)>) -> Vec<(f64, usize)> {
    terms.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    let mut frontier = Vec::new();
    let mut min_count = usize::MAX;
    for term in terms {
        if term.1 >= min_count {
            continue;
        }
        min_count = term.1;
        frontier.push(term);
    }
    frontier
}

pub(crate) fn search_raw_domain(
    request: &ScoreRangeRequest,
    domain: &ScoreRangeTeamDomain,
    songs: &[ScoreRangeSong],
    target_delta: u64,
) -> Result<Vec<(ScoreRangeTeam, Vec<ScoreRangePlay>)>, ScoreRangeError> {
    if target_delta == 0 {
        return Ok(Vec::new());
    }

    let raw = &domain.recovery;
    let trace = std::env::var_os("BANGDREAM_OPTIMIZE_SCORE_RANGE_TRACE").is_some();
    let trace_song_skips =
        std::env::var_os("BANGDREAM_OPTIMIZE_SCORE_RANGE_TRACE_SONG_SKIPS").is_some();
    let mut modes = signature_modes(&raw.cards);
    modes.sort_by_key(|mode| match mode {
        SongMode::UnifiedBandAttribute(_, _) => 0,
        SongMode::UnifiedBand(_) | SongMode::UnifiedAttribute(_) => 1,
        SongMode::Mixed => 2,
    });
    let global_items = raw.items.iter().collect::<Vec<_>>();
    let global_team_bounds = team_bounds(
        raw.cards.iter().map(|card| {
            (
                card,
                raw.point_bonus_micros
                    .get(&card.card_id)
                    .copied()
                    .unwrap_or_default(),
            )
        }),
        &global_items,
        &raw.area_item_percent,
    )
    .unwrap_or_default();
    let optimistic_global_stat = global_team_bounds.max_stat;
    let optimistic_global_bonus = global_team_bounds.max_bonus_basis_points;
    let optimistic_skill_multiplier = optimistic_global_skill_multiplier(&raw.cards);
    let raw_song_upper_terms = songs
        .iter()
        .map(ScoreRangeSong::optimistic_auto_score_terms)
        .collect::<Result<Vec<_>, _>>()?;
    let raw_song_lower_terms = songs
        .iter()
        .map(ScoreRangeSong::pessimistic_auto_score_terms)
        .collect::<Result<Vec<_>, _>>()?;
    let song_upper_terms = optimistic_term_frontier(raw_song_upper_terms);
    let song_lower_terms = pessimistic_term_frontier(raw_song_lower_terms);
    if trace {
        eprintln!(
            "score-range song bound frontiers: songs={} lower={} upper={}",
            songs.len(),
            song_lower_terms.len(),
            song_upper_terms.len(),
        );
    }
    let optimistic_global_score = song_upper_terms
        .iter()
        .map(|&terms| {
            crate::Chart::optimistic_auto_score_from_terms(
                terms,
                optimistic_global_stat,
                optimistic_skill_multiplier,
            )
        })
        .max()
        .unwrap_or_default();
    let optimistic_global_base_points = points_for_score_with_support(
        request.event_type,
        optimistic_global_score,
        optimistic_global_bonus,
        1,
        request.mission_support_pt_bonus.unwrap_or_default(),
    )?;
    let theoretical_min_base_points = points_for_score_with_support(
        request.event_type,
        0,
        0,
        1,
        request.mission_support_pt_bonus.unwrap_or_default(),
    )?;
    let pessimistic_global_multiplier = pessimistic_global_skill_multiplier(&raw.cards);
    let pessimistic_global_score = song_lower_terms
        .iter()
        .map(|&terms| {
            crate::Chart::pessimistic_auto_score_from_terms(
                terms,
                global_team_bounds.min_stat,
                pessimistic_global_multiplier,
            )
        })
        .min()
        .unwrap_or_default();
    let pessimistic_global_base_points = points_for_score_with_support(
        request.event_type,
        pessimistic_global_score,
        global_team_bounds.min_bonus_basis_points,
        1,
        request.mission_support_pt_bonus.unwrap_or_default(),
    )?;
    let global_min_base_points = theoretical_min_base_points.max(pessimistic_global_base_points);
    let base_point_layers = ranked_base_point_layers(target_delta);
    let mut candidates = CandidateMap::new();
    let mut selected_objective = None;
    let mut skill_contexts = BTreeMap::<SkillBucketKey, SkillSearchContext>::new();
    let mut non_rateup_bound_templates = BTreeMap::<i32, Vec<NonRateupSongTemplate>>::new();
    let mut non_rateup_song_templates = BTreeMap::<i32, Vec<NonRateupSongTemplate>>::new();
    let mut item_metrics = ItemBatchMetrics::default();
    let mut prepared_modes = (0..modes.len())
        .map(|_| None)
        .collect::<Vec<Option<PreparedMode<'_>>>>();

    for (objective, base_points) in base_point_layers {
        let layer_started = trace.then(Timer::start);
        let mut layer_timing = LayerTiming::default();
        candidates.clear();
        for context in skill_contexts.values_mut() {
            context.intervals.clear();
        }
        if trace {
            eprintln!(
                "score-range objective layer: plays={} total_fire_cost={} base_points={base_points}",
                objective.play_count, objective.total_fire_cost,
            );
        }
        if optimistic_global_base_points < base_points {
            item_metrics.upper_bound_pruned_layers += 1;
            if trace {
                eprintln!(
                    "score-range objective layer pruned: base_points={base_points} optimistic_global_base_points={optimistic_global_base_points} optimistic_global_score={optimistic_global_score} optimistic_global_stat={optimistic_global_stat} optimistic_global_bonus={optimistic_global_bonus} optimistic_skill_multiplier={optimistic_skill_multiplier}",
                );
            }
            continue;
        }
        if base_points < global_min_base_points {
            item_metrics.lower_bound_pruned_layers += 1;
            if trace {
                eprintln!(
                    "score-range objective layer pruned: base_points={base_points} global_min_base_points={global_min_base_points}",
                );
            }
            continue;
        }

        'modes: for (mode_index, &mode) in modes.iter().enumerate() {
            let mode_started = trace.then(Timer::start);
            if prepared_modes[mode_index].is_none() {
                let phase_started = trace.then(Timer::start);
                prepared_modes[mode_index] = Some(prepare_mode(
                    mode,
                    &raw.cards,
                    &raw.point_bonus_micros,
                    &raw.items,
                    &raw.area_item_percent,
                    &song_lower_terms,
                    &song_upper_terms,
                    request,
                )?);
                layer_timing.mode_preparation_ms += optional_elapsed_ms(phase_started);
            }
            let prepared_mode = prepared_modes[mode_index]
                .as_mut()
                .expect("mode was prepared above");
            debug_assert_eq!(prepared_mode.mode, mode);
            if base_points < prepared_mode.coarse_base_points.min {
                item_metrics.lower_bound_pruned_buckets += prepared_mode.buckets.len();
                if trace {
                    eprintln!(
                        "score-range MITM mode coarsely lower-pruned: mode={mode:?} buckets={} target_base_points={base_points} pessimistic_base_points={}",
                        prepared_mode.buckets.len(),
                        prepared_mode.coarse_base_points.min,
                    );
                }
                continue;
            }
            if prepared_mode.coarse_base_points.max < base_points {
                item_metrics.upper_bound_pruned_buckets += prepared_mode.buckets.len();
                if trace {
                    eprintln!(
                        "score-range MITM mode coarsely upper-pruned: mode={mode:?} buckets={} target_base_points={base_points} optimistic_base_points={}",
                        prepared_mode.buckets.len(),
                        prepared_mode.coarse_base_points.max,
                    );
                }
                continue;
            }
            for bucket in &mut prepared_mode.buckets {
                let bucket_started = trace.then(Timer::start);
                let skill_key = bucket.skill_key;
                let skill = bucket.skill;
                let cards = &bucket.cards;
                let team_bounds = bucket.team_bounds;
                if base_points < bucket.coarse_base_points.min {
                    item_metrics.lower_bound_pruned_buckets += 1;
                    if trace {
                        eprintln!(
                            "score-range MITM bucket coarsely lower-pruned: mode={mode:?} skill={skill_key:?} cards={} target_base_points={base_points} pessimistic_base_points={} pessimistic_stat={} pessimistic_bonus={}",
                            cards.len(),
                            bucket.coarse_base_points.min,
                            team_bounds.min_stat,
                            team_bounds.min_bonus_basis_points,
                        );
                    }
                    continue;
                }
                if bucket.coarse_base_points.max < base_points {
                    item_metrics.upper_bound_pruned_buckets += 1;
                    if trace {
                        eprintln!(
                            "score-range MITM bucket coarsely upper-pruned: mode={mode:?} skill={skill_key:?} cards={} target_base_points={base_points} optimistic_base_points={} optimistic_stat={} optimistic_bonus={}",
                            cards.len(),
                            bucket.coarse_base_points.max,
                            team_bounds.max_stat,
                            team_bounds.max_bonus_basis_points,
                        );
                    }
                    continue;
                }
                if bucket.template_base_points.is_none() {
                    ensure_non_rateup_bound_templates(
                        songs,
                        skill_key,
                        skill,
                        &mut non_rateup_bound_templates,
                        trace_song_skips,
                        trace,
                        &mut layer_timing,
                    )?;
                    let phase_started = trace.then(Timer::start);
                    let templates = &non_rateup_bound_templates[&skill_key.duration_millis];
                    let pessimistic_score = templates
                        .iter()
                        .map(|song| song.exact.score(team_bounds.min_stat, skill.score_up))
                        .min()
                        .unwrap_or_default();
                    let optimistic_score_up = if skill.rateup {
                        optimistic_multiplier_for_skill(skill) - 1.0
                    } else {
                        skill.score_up
                    };
                    let optimistic_score = templates
                        .iter()
                        .map(|song| song.exact.score(team_bounds.max_stat, optimistic_score_up))
                        .max()
                        .unwrap_or_default();
                    bucket.template_base_points = Some(BasePointBounds {
                        min: points_for_score_with_support(
                            request.event_type,
                            pessimistic_score,
                            team_bounds.min_bonus_basis_points,
                            1,
                            request.mission_support_pt_bonus.unwrap_or_default(),
                        )?,
                        max: points_for_score_with_support(
                            request.event_type,
                            optimistic_score,
                            team_bounds.max_bonus_basis_points,
                            1,
                            request.mission_support_pt_bonus.unwrap_or_default(),
                        )?,
                    });
                    layer_timing.exact_bounds_ms += optional_elapsed_ms(phase_started);
                }
                if let Some(template_base_points) = bucket.template_base_points {
                    if base_points < template_base_points.min {
                        item_metrics.lower_bound_pruned_buckets += 1;
                        if trace {
                            eprintln!(
                                "score-range MITM bucket lower-pruned before materialization: mode={mode:?} skill={skill_key:?} cards={} target_base_points={base_points} pessimistic_base_points={}",
                                cards.len(), template_base_points.min,
                            );
                        }
                        continue;
                    }
                    if template_base_points.max < base_points {
                        item_metrics.upper_bound_pruned_buckets += 1;
                        if trace {
                            eprintln!(
                                "score-range MITM bucket upper-pruned before materialization: mode={mode:?} skill={skill_key:?} cards={} target_base_points={base_points} optimistic_base_points={}",
                                cards.len(), template_base_points.max,
                            );
                        }
                        continue;
                    }
                }
                if !skill.rateup {
                    ensure_safe_non_rateup_song_templates(
                        skill_key,
                        &non_rateup_bound_templates,
                        &mut non_rateup_song_templates,
                        trace_song_skips,
                    );
                }
                let phase_started = trace.then(Timer::start);
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    skill_contexts.entry(skill_key)
                {
                    let song_models = if skill.rateup {
                        let mut song_models = Vec::new();
                        for template in &non_rateup_bound_templates[&skill_key.duration_millis] {
                            if template.exact.has_skill_tail_risk() {
                                if trace_song_skips {
                                    eprintln!(
                                        "score-range song skipped: song={:?} reason=skill_tail_risk skill_nodes=6 duration_millis={}",
                                        template.key, skill_key.duration_millis,
                                    );
                                }
                                continue;
                            }
                            let song = &songs[template.song_index];
                            let compression_started = trace.then(Timer::start);
                            let exact = song.compressed_score(skill)?;
                            layer_timing.score_compression_ms +=
                                optional_elapsed_ms(compression_started);
                            song_models.push(SongModel {
                                key: song.key(),
                                exact,
                            });
                        }
                        song_models
                    } else {
                        debug_assert!(
                            non_rateup_song_templates.contains_key(&skill_key.duration_millis)
                        );
                        let compression_started = trace.then(Timer::start);
                        let song_models = non_rateup_song_templates[&skill_key.duration_millis]
                            .iter()
                            .map(|song| SongModel {
                                key: song.key,
                                exact: song.exact.compressed_score(skill.score_up, false),
                            })
                            .collect();
                        layer_timing.score_compression_ms +=
                            optional_elapsed_ms(compression_started);
                        song_models
                    };
                    entry.insert(SkillSearchContext {
                        songs: song_models,
                        intervals: BTreeMap::new(),
                        score_lower_bounds: BTreeMap::new(),
                    });
                }
                layer_timing.skill_contexts_ms += optional_elapsed_ms(phase_started);
                let context = skill_contexts
                    .get_mut(&skill_key)
                    .expect("skill context was inserted above");
                if context.songs.is_empty() {
                    continue;
                }
                if bucket.safe_base_points.is_none() {
                    let phase_started = trace.then(Timer::start);
                    let pessimistic_score = context
                        .songs
                        .iter()
                        .map(|song| song.exact.score(team_bounds.min_stat))
                        .min()
                        .unwrap_or_default();
                    let optimistic_score = context
                        .songs
                        .iter()
                        .map(|song| song.exact.score(team_bounds.max_stat))
                        .max()
                        .unwrap_or_default();
                    bucket.safe_base_points = Some(BasePointBounds {
                        min: points_for_score_with_support(
                            request.event_type,
                            pessimistic_score,
                            team_bounds.min_bonus_basis_points,
                            1,
                            request.mission_support_pt_bonus.unwrap_or_default(),
                        )?,
                        max: points_for_score_with_support(
                            request.event_type,
                            optimistic_score,
                            team_bounds.max_bonus_basis_points,
                            1,
                            request.mission_support_pt_bonus.unwrap_or_default(),
                        )?,
                    });
                    layer_timing.exact_bounds_ms += optional_elapsed_ms(phase_started);
                }
                let safe_base_points = bucket
                    .safe_base_points
                    .expect("safe bounds were initialized above");
                if base_points < safe_base_points.min {
                    item_metrics.lower_bound_pruned_buckets += 1;
                    if trace {
                        eprintln!(
                            "score-range MITM bucket lower-pruned: mode={mode:?} skill={skill_key:?} cards={} target_base_points={base_points} pessimistic_base_points={} pessimistic_stat={} pessimistic_bonus={}",
                            cards.len(),
                            safe_base_points.min,
                            team_bounds.min_stat,
                            team_bounds.min_bonus_basis_points,
                        );
                    }
                    continue;
                }
                if safe_base_points.max < base_points {
                    item_metrics.upper_bound_pruned_buckets += 1;
                    if trace {
                        eprintln!(
                            "score-range MITM bucket upper-pruned: mode={mode:?} skill={skill_key:?} cards={} target_base_points={base_points} optimistic_base_points={} optimistic_stat={} optimistic_bonus={}",
                            cards.len(),
                            safe_base_points.max,
                            team_bounds.max_stat,
                            team_bounds.max_bonus_basis_points,
                        );
                    }
                    continue;
                }
                let SkillSearchContext {
                    songs: song_models,
                    intervals: interval_cache,
                    score_lower_bounds,
                } = context;
                if trace {
                    eprintln!(
                        "score-range MITM bucket: mode={mode:?} skill={skill_key:?} cards={} songs={} items={}",
                        cards.len(),
                        song_models.len(),
                        prepared_mode.item_groups.len(),
                    );
                }

                let phase_started = trace.then(Timer::start);
                enumerate_item_groups(
                    request,
                    target_delta,
                    &[base_points],
                    mode,
                    skill_key,
                    skill,
                    &prepared_mode.item_groups,
                    cards,
                    &raw.area_item_percent,
                    song_models,
                    interval_cache,
                    score_lower_bounds,
                    &mut candidates,
                    &mut item_metrics,
                )?;
                layer_timing.item_search_ms += optional_elapsed_ms(phase_started);
                if candidates.len() >= request.max_results.max(1) {
                    break 'modes;
                }
                if trace {
                    eprintln!(
                        "score-range MITM bucket complete: mode={mode:?} skill={skill_key:?} cards={} candidates={} elapsed_ms={:.3}",
                        cards.len(),
                        candidates.len(),
                        optional_elapsed_ms(bucket_started),
                    );
                }
            }
            if trace {
                eprintln!(
                    "score-range MITM mode complete: mode={mode:?} elapsed_ms={:.3}",
                    optional_elapsed_ms(mode_started),
                );
            }
        }
        if trace {
            eprintln!(
                "score-range objective layer complete: plays={} total_fire_cost={} candidates={} elapsed_ms={:.3} timing={layer_timing:?}",
                objective.play_count,
                objective.total_fire_cost,
                candidates.len(),
                optional_elapsed_ms(layer_started),
            );
        }
        if !candidates.is_empty() {
            selected_objective = Some(objective);
            break;
        }
    }

    if trace {
        let interval_cache_entries = skill_contexts
            .values()
            .map(|context| context.intervals.len())
            .sum::<usize>();
        let interval_cache_ranges = skill_contexts
            .values()
            .flat_map(|context| context.intervals.values())
            .map(|intervals| intervals.union.len())
            .sum::<usize>();
        let score_boundary_cache_entries = skill_contexts
            .values()
            .map(|context| context.score_lower_bounds.len())
            .sum::<usize>();
        let pair_index_peak_bytes = item_metrics
            .max_base_pairs
            .saturating_add(item_metrics.max_adjusted_pairs)
            .saturating_mul(std::mem::size_of::<PairRecord>());
        eprintln!(
            "score-range item batch metrics: {item_metrics:?} skill_contexts={} interval_cache_entries={} interval_cache_ranges={} score_boundary_cache_entries={} pair_index_peak_bytes={pair_index_peak_bytes}",
            skill_contexts.len(),
            interval_cache_entries,
            interval_cache_ranges,
            score_boundary_cache_entries,
        );
        eprintln!("score-range selected objective: {selected_objective:?}");
    }

    let mut result = candidates
        .into_values()
        .map(|candidate| (candidate.team, candidate.plan))
        .collect::<Vec<_>>();
    result.sort_by_key(|(team, plan)| {
        (
            plan_rank(plan),
            team.stat,
            team.card_ids,
            team.items.band.clone(),
            team.items.attribute.clone(),
            team.items.magazine.as_str(),
        )
    });
    result.truncate(request.max_results.max(1));
    Ok(result)
}

fn ensure_non_rateup_bound_templates(
    songs: &[ScoreRangeSong],
    skill_key: SkillBucketKey,
    _skill: TeamCardSkill,
    templates_by_duration: &mut BTreeMap<i32, Vec<NonRateupSongTemplate>>,
    trace_song_skips: bool,
    trace: bool,
    timing: &mut LayerTiming,
) -> Result<(), ScoreRangeError> {
    if templates_by_duration.contains_key(&skill_key.duration_millis) {
        return Ok(());
    }
    let mut templates = Vec::new();
    for (song_index, song) in songs.iter().enumerate() {
        let compression_started = trace.then(Timer::start);
        let Ok(exact) = song.duration_model(skill_key.duration_millis) else {
            if trace_song_skips {
                eprintln!(
                    "score-range song skipped: song={:?} reason=missing_duration_template duration_millis={}",
                    song.key(), skill_key.duration_millis,
                );
            }
            continue;
        };
        timing.score_compression_ms += optional_elapsed_ms(compression_started);
        templates.push(NonRateupSongTemplate {
            song_index,
            key: song.key(),
            exact,
        });
    }
    templates_by_duration.insert(skill_key.duration_millis, templates);
    Ok(())
}

fn ensure_safe_non_rateup_song_templates(
    skill_key: SkillBucketKey,
    bound_templates: &BTreeMap<i32, Vec<NonRateupSongTemplate>>,
    safe_templates: &mut BTreeMap<i32, Vec<NonRateupSongTemplate>>,
    trace_song_skips: bool,
) {
    if safe_templates.contains_key(&skill_key.duration_millis) {
        return;
    }
    let mut templates = Vec::new();
    for &template in &bound_templates[&skill_key.duration_millis] {
        if template.exact.has_skill_tail_risk() {
            if trace_song_skips {
                eprintln!(
                    "score-range song skipped: song={:?} reason=skill_tail_risk skill_nodes=6 duration_millis={}",
                    template.key, skill_key.duration_millis,
                );
            }
            continue;
        }
        templates.push(template);
    }
    safe_templates.insert(skill_key.duration_millis, templates);
}

fn skill_buckets_for_mode<'a>(
    cards: &'a [PreparedCard],
    point_bonus_micros: &BTreeMap<u32, u64>,
    mode: SongMode,
) -> BTreeMap<SkillBucketKey, Vec<BucketCard<'a>>> {
    let mut result = BTreeMap::<SkillBucketKey, Vec<BucketCard<'a>>>::new();
    for card in cards.iter().filter(|card| mode.allows(card)) {
        let Ok(skill) = mode.resolve_skill(card) else {
            continue;
        };
        let point_bonus_micros = point_bonus_micros
            .get(&card.card_id)
            .copied()
            .unwrap_or_default();
        result
            .entry(SkillBucketKey::from_skill(skill))
            .or_default()
            .push(BucketCard {
                card,
                skill,
                point_bonus_micros,
            });
    }
    result
}

#[derive(Clone, Copy)]
struct CharacterTeamBounds {
    min_stat: f64,
    max_stat: f64,
    min_bonus_micros: u64,
    max_bonus_micros: u64,
}

fn team_bounds<'a>(
    cards: impl IntoIterator<Item = (&'a PreparedCard, u64)>,
    items: &[&SelectedAreaItems],
    area_item_percent: &AreaItemPercent,
) -> Option<TeamBounds> {
    if items.is_empty() {
        return None;
    }
    let mut by_character = BTreeMap::<u32, CharacterTeamBounds>::new();
    for (card, point_bonus_micros) in cards {
        let mut stats = items.iter().map(|items| {
            card.add_up_stat(
                area_item_percent,
                &items.band,
                &items.attribute,
                items.magazine.as_str(),
            )
        });
        let first_stat = stats.next()?;
        let (min_stat, max_stat) = stats.fold((first_stat, first_stat), |bounds, stat| {
            (bounds.0.min(stat), bounds.1.max(stat))
        });
        by_character
            .entry(card.character_id)
            .and_modify(|bounds| {
                bounds.min_stat = bounds.min_stat.min(min_stat);
                bounds.max_stat = bounds.max_stat.max(max_stat);
                bounds.min_bonus_micros = bounds.min_bonus_micros.min(point_bonus_micros);
                bounds.max_bonus_micros = bounds.max_bonus_micros.max(point_bonus_micros);
            })
            .or_insert(CharacterTeamBounds {
                min_stat,
                max_stat,
                min_bonus_micros: point_bonus_micros,
                max_bonus_micros: point_bonus_micros,
            });
    }
    if by_character.len() < 5 {
        return None;
    }

    let mut min_stats = by_character
        .values()
        .map(|bounds| bounds.min_stat)
        .collect::<Vec<_>>();
    let mut max_stats = by_character
        .values()
        .map(|bounds| bounds.max_stat)
        .collect::<Vec<_>>();
    let mut min_bonuses = by_character
        .values()
        .map(|bounds| bounds.min_bonus_micros)
        .collect::<Vec<_>>();
    let mut max_bonuses = by_character
        .values()
        .map(|bounds| bounds.max_bonus_micros)
        .collect::<Vec<_>>();
    min_stats.sort_unstable_by(f64::total_cmp);
    max_stats.sort_unstable_by(|left, right| right.total_cmp(left));
    min_bonuses.sort_unstable();
    max_bonuses.sort_unstable_by(|left, right| right.cmp(left));
    let min_bonus_micros = min_bonuses
        .into_iter()
        .take(5)
        .fold(0_u64, u64::saturating_add);
    let max_bonus_micros = max_bonuses
        .into_iter()
        .take(5)
        .fold(0_u64, u64::saturating_add);
    Some(TeamBounds {
        min_stat: min_stats.into_iter().take(5).sum::<f64>().floor() as i32,
        max_stat: max_stats.into_iter().take(5).sum::<f64>().floor() as i32,
        min_bonus_basis_points: bonus_micros_to_basis_points(min_bonus_micros),
        max_bonus_basis_points: bonus_micros_to_basis_points(max_bonus_micros),
    })
}

fn bonus_micros_to_basis_points(total_micros: u64) -> u32 {
    ((total_micros.saturating_add(5_000)) / 10_000).min(u32::MAX as u64) as u32
}

fn pessimistic_multiplier_for_skill(skill: TeamCardSkill) -> f64 {
    (1.0 + skill.score_up).clamp(0.0, 1.0)
}

fn pessimistic_global_skill_multiplier(cards: &[PreparedCard]) -> f64 {
    cards
        .iter()
        .flat_map(|card| {
            [
                Some(card.score_up.default),
                card.score_up.unification_activate_effect_value,
            ]
        })
        .flatten()
        .map(|score_up| (1.0 + score_up).clamp(0.0, 1.0))
        .reduce(f64::min)
        .unwrap_or(0.0)
}

#[allow(clippy::too_many_arguments)]
fn prepare_mode<'a>(
    mode: SongMode,
    cards: &'a [PreparedCard],
    point_bonus_micros: &BTreeMap<u32, u64>,
    items: &'a [SelectedAreaItems],
    area_item_percent: &AreaItemPercent,
    song_lower_terms: &[(f64, usize)],
    song_upper_terms: &[(f64, usize)],
    request: &ScoreRangeRequest,
) -> Result<PreparedMode<'a>, ScoreRangeError> {
    let item_groups = group_items_by_mode(mode, items);
    let mut buckets = skill_buckets_for_mode(cards, point_bonus_micros, mode)
        .into_iter()
        .collect::<Vec<_>>();
    buckets.sort_by_key(|(skill, _)| {
        (
            std::cmp::Reverse(skill.score_up_millionths),
            std::cmp::Reverse(skill.duration_millis),
            std::cmp::Reverse(skill.rateup),
        )
    });
    let mut prepared_buckets = Vec::new();
    for (skill_key, cards) in buckets {
        if cards.len() < 5 {
            continue;
        }
        let Some(team_bounds) = team_bounds(
            cards
                .iter()
                .map(|entry| (entry.card, entry.point_bonus_micros)),
            &item_groups,
            area_item_percent,
        ) else {
            continue;
        };
        let skill = cards[0].skill;
        let min_multiplier = pessimistic_multiplier_for_skill(skill);
        let max_multiplier = optimistic_multiplier_for_skill(skill);
        let min_score = song_lower_terms
            .iter()
            .map(|&terms| {
                crate::Chart::pessimistic_auto_score_from_terms(
                    terms,
                    team_bounds.min_stat,
                    min_multiplier,
                )
            })
            .min()
            .unwrap_or_default();
        let max_score = song_upper_terms
            .iter()
            .map(|&terms| {
                crate::Chart::optimistic_auto_score_from_terms(
                    terms,
                    team_bounds.max_stat,
                    max_multiplier,
                )
            })
            .max()
            .unwrap_or_default();
        let min_base_points = points_for_score_with_support(
            request.event_type,
            min_score,
            team_bounds.min_bonus_basis_points,
            1,
            request.mission_support_pt_bonus.unwrap_or_default(),
        )?;
        let max_base_points = points_for_score_with_support(
            request.event_type,
            max_score,
            team_bounds.max_bonus_basis_points,
            1,
            request.mission_support_pt_bonus.unwrap_or_default(),
        )?;
        prepared_buckets.push(PreparedSkillBucket {
            skill_key,
            skill,
            cards,
            team_bounds,
            coarse_base_points: BasePointBounds {
                min: min_base_points,
                max: max_base_points,
            },
            template_base_points: None,
            safe_base_points: None,
        });
    }
    let coarse_base_points = BasePointBounds {
        min: prepared_buckets
            .iter()
            .map(|bucket| bucket.coarse_base_points.min)
            .min()
            .unwrap_or_default(),
        max: prepared_buckets
            .iter()
            .map(|bucket| bucket.coarse_base_points.max)
            .max()
            .unwrap_or_default(),
    };
    Ok(PreparedMode {
        mode,
        item_groups,
        buckets: prepared_buckets,
        coarse_base_points,
    })
}

fn optimistic_global_skill_multiplier(cards: &[PreparedCard]) -> f64 {
    let max_score_up = cards.iter().fold(0.0_f64, |current, card| {
        let unified = card
            .score_up
            .unification_activate_effect_value
            .unwrap_or(card.score_up.default);
        current.max(card.score_up.default).max(unified)
    });
    // Rate-up advances in 0.005 steps and can cross its nominal 2.5 cap once.
    (1.0 + max_score_up).max(2.51)
}

fn optimistic_multiplier_for_skill(skill: TeamCardSkill) -> f64 {
    let base = (1.0 + skill.score_up).max(1.0);
    if skill.rateup {
        base.max(2.51)
    } else {
        base
    }
}

#[allow(clippy::too_many_arguments)]
fn enumerate_item_groups(
    request: &ScoreRangeRequest,
    target_delta: u64,
    divisors: &[u64],
    mode: SongMode,
    skill_key: SkillBucketKey,
    skill: TeamCardSkill,
    item_groups: &[&SelectedAreaItems],
    bucket_cards: &[BucketCard<'_>],
    area_item_percent: &AreaItemPercent,
    songs: &[SongModel],
    interval_cache: &mut BTreeMap<u32, ValidIntervalSet>,
    score_lower_bounds: &mut BTreeMap<(usize, u64), Option<i32>>,
    candidates: &mut CandidateMap,
    metrics: &mut ItemBatchMetrics,
) -> Result<(), ScoreRangeError> {
    let result_limit = request.max_results.max(1);
    for magazine in [Magazine::Performance, Magazine::Technique, Magazine::Visual] {
        let contexts = item_groups
            .iter()
            .copied()
            .filter(|items| items.magazine == magazine)
            .collect::<Vec<_>>();
        if contexts.is_empty() {
            continue;
        }

        let (local, full): (Vec<_>, Vec<_>) = contexts.into_iter().partition(|items| {
            is_single_target(ItemAxis::Band, &items.band)
                && is_single_target(ItemAxis::Attribute, &items.attribute)
        });

        for items in full {
            enumerate_full_item_context(
                request,
                target_delta,
                divisors,
                mode,
                skill_key,
                skill,
                items,
                bucket_cards,
                area_item_percent,
                songs,
                interval_cache,
                score_lower_bounds,
                candidates,
                metrics,
            )?;
            if candidates.len() >= result_limit {
                return Ok(());
            }
        }

        if !local.is_empty() {
            enumerate_local_item_batch(
                request,
                target_delta,
                divisors,
                mode,
                skill_key,
                skill,
                magazine,
                &local,
                bucket_cards,
                area_item_percent,
                songs,
                interval_cache,
                score_lower_bounds,
                candidates,
                metrics,
            )?;
            if candidates.len() >= result_limit {
                return Ok(());
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn enumerate_full_item_context(
    request: &ScoreRangeRequest,
    target_delta: u64,
    divisors: &[u64],
    mode: SongMode,
    skill_key: SkillBucketKey,
    skill: TeamCardSkill,
    items: &SelectedAreaItems,
    bucket_cards: &[BucketCard<'_>],
    area_item_percent: &AreaItemPercent,
    songs: &[SongModel],
    interval_cache: &mut BTreeMap<u32, ValidIntervalSet>,
    score_lower_bounds: &mut BTreeMap<(usize, u64), Option<i32>>,
    candidates: &mut CandidateMap,
    metrics: &mut ItemBatchMetrics,
) -> Result<(), ScoreRangeError> {
    let resolved = resolve_cards_for_items(bucket_cards, area_item_percent, items);
    if resolved.len() < 5 {
        return Ok(());
    }
    let stats = resolved.iter().map(|card| card.stat).collect::<Vec<_>>();
    if !item_context_may_match(
        request,
        target_delta,
        divisors,
        &resolved,
        &stats,
        songs,
        interval_cache,
        score_lower_bounds,
    )? {
        metrics.structural_pruned_contexts += 1;
        return Ok(());
    }
    let pairs = build_pair_index(&resolved, &stats, None);
    metrics.full_queries += 1;
    metrics.max_base_pairs = metrics.max_base_pairs.max(pairs.record_count);
    enumerate_item_context(
        request,
        target_delta,
        divisors,
        mode,
        skill_key,
        skill,
        items,
        area_item_percent,
        &resolved,
        &stats,
        PairQuery::Full(&pairs),
        None,
        songs,
        interval_cache,
        score_lower_bounds,
        candidates,
    )
}

#[allow(clippy::too_many_arguments)]
fn enumerate_local_item_batch(
    request: &ScoreRangeRequest,
    target_delta: u64,
    divisors: &[u64],
    mode: SongMode,
    skill_key: SkillBucketKey,
    skill: TeamCardSkill,
    magazine: Magazine,
    contexts: &[&SelectedAreaItems],
    bucket_cards: &[BucketCard<'_>],
    area_item_percent: &AreaItemPercent,
    songs: &[SongModel],
    interval_cache: &mut BTreeMap<u32, ValidIntervalSet>,
    score_lower_bounds: &mut BTreeMap<(usize, u64), Option<i32>>,
    candidates: &mut CandidateMap,
    metrics: &mut ItemBatchMetrics,
) -> Result<(), ScoreRangeError> {
    let result_limit = request.max_results.max(1);
    let band_count = contexts
        .iter()
        .map(|items| items.band.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let attribute_count = contexts
        .iter()
        .map(|items| items.attribute.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let outer_axis = if band_count <= attribute_count {
        metrics.outer_band_batches += 1;
        ItemAxis::Band
    } else {
        metrics.outer_attribute_batches += 1;
        ItemAxis::Attribute
    };
    let inner_axis = match outer_axis {
        ItemAxis::Band => ItemAxis::Attribute,
        ItemAxis::Attribute => ItemAxis::Band,
    };

    let mut by_outer = BTreeMap::<String, Vec<&SelectedAreaItems>>::new();
    for &items in contexts {
        by_outer
            .entry(item_key(items, outer_axis).to_owned())
            .or_default()
            .push(items);
    }

    for (outer_key, inner_contexts) in by_outer {
        let base_items = items_for_axes(magazine, outer_axis, &outer_key, "");
        let resolved = resolve_cards_for_batch(
            bucket_cards,
            area_item_percent,
            &base_items,
            &inner_contexts,
        );
        if resolved.len() < 5 {
            continue;
        }
        let base_stats = resolved.iter().map(|card| card.stat).collect::<Vec<_>>();
        let base_can_match = item_context_may_match(
            request,
            target_delta,
            divisors,
            &resolved,
            &base_stats,
            songs,
            interval_cache,
            score_lower_bounds,
        )?;
        let mut prefetched_inner = Vec::new();
        if !base_can_match {
            prefetched_inner.reserve(inner_contexts.len());
            for &items in &inner_contexts {
                let inner_key = item_key(items, inner_axis);
                let affected_cards = resolved
                    .iter()
                    .map(|entry| card_matches_target(entry.card, inner_axis, inner_key))
                    .collect::<Vec<_>>();
                let stats = resolve_card_stats(&resolved, area_item_percent, items);
                let can_match = item_context_may_match(
                    request,
                    target_delta,
                    divisors,
                    &resolved,
                    &stats,
                    songs,
                    interval_cache,
                    score_lower_bounds,
                )?;
                prefetched_inner.push(can_match.then_some((affected_cards, stats)));
            }
            if prefetched_inner.iter().all(Option::is_none) {
                metrics.structural_pruned_contexts += prefetched_inner.len() + 1;
                continue;
            }
        }
        let base_pairs = build_pair_index(&resolved, &base_stats, None);
        metrics.base_queries += 1;
        metrics.max_base_pairs = metrics.max_base_pairs.max(base_pairs.record_count);

        let mut base_candidates = CandidateMap::new();
        if base_can_match {
            enumerate_item_context(
                request,
                target_delta,
                divisors,
                mode,
                skill_key,
                skill,
                &base_items,
                area_item_percent,
                &resolved,
                &base_stats,
                PairQuery::Full(&base_pairs),
                None,
                songs,
                interval_cache,
                score_lower_bounds,
                &mut base_candidates,
            )?;
        } else {
            metrics.structural_pruned_contexts += 1;
        }
        let base_had_solution = !base_candidates.is_empty();

        for (inner_index, items) in inner_contexts.into_iter().enumerate() {
            let inner_key = item_key(items, inner_axis);
            if let Some((&key, candidate)) = base_candidates.iter().find(|(_, candidate)| {
                !team_touches_target(&candidate.team, &resolved, inner_axis, inner_key)
            }) {
                let mut reused = candidate.clone();
                reused.team.items = items.clone();
                insert_candidate(candidates, key, reused);
                metrics.reused_contexts += 1;
                if candidates.len() >= result_limit {
                    return Ok(());
                }
                continue;
            }

            let work = if base_can_match {
                let affected_cards = resolved
                    .iter()
                    .map(|entry| card_matches_target(entry.card, inner_axis, inner_key))
                    .collect::<Vec<_>>();
                let stats = resolve_card_stats(&resolved, area_item_percent, items);
                item_context_may_match(
                    request,
                    target_delta,
                    divisors,
                    &resolved,
                    &stats,
                    songs,
                    interval_cache,
                    score_lower_bounds,
                )?
                .then_some((affected_cards, stats))
            } else {
                prefetched_inner[inner_index].take()
            };
            let Some((affected_cards, stats)) = work else {
                metrics.structural_pruned_contexts += 1;
                continue;
            };
            let adjusted_pairs =
                build_pair_index(&resolved, &stats, Some(affected_cards.as_slice()));
            metrics.incremental_queries += 1;
            metrics.max_adjusted_pairs =
                metrics.max_adjusted_pairs.max(adjusted_pairs.record_count);
            enumerate_item_context(
                request,
                target_delta,
                divisors,
                mode,
                skill_key,
                skill,
                items,
                area_item_percent,
                &resolved,
                &stats,
                PairQuery::Incremental {
                    base: &base_pairs,
                    adjusted_touching: &adjusted_pairs,
                    affected_cards: &affected_cards,
                },
                (!base_had_solution).then_some(affected_cards.as_slice()),
                songs,
                interval_cache,
                score_lower_bounds,
                candidates,
            )?;
            if candidates.len() >= result_limit {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn is_single_target(axis: ItemAxis, key: &str) -> bool {
    if key.is_empty() || key.contains(',') {
        return false;
    }
    match axis {
        ItemAxis::Band => key != ALL_BAND_KEY && key.parse::<u32>().is_ok(),
        ItemAxis::Attribute => key != ALL_ATTRIBUTE_KEY,
    }
}

fn item_key(items: &SelectedAreaItems, axis: ItemAxis) -> &str {
    match axis {
        ItemAxis::Band => &items.band,
        ItemAxis::Attribute => &items.attribute,
    }
}

fn items_for_axes(
    magazine: Magazine,
    outer_axis: ItemAxis,
    outer_key: &str,
    inner_key: &str,
) -> SelectedAreaItems {
    match outer_axis {
        ItemAxis::Band => SelectedAreaItems {
            band: outer_key.to_owned(),
            attribute: inner_key.to_owned(),
            magazine,
        },
        ItemAxis::Attribute => SelectedAreaItems {
            band: inner_key.to_owned(),
            attribute: outer_key.to_owned(),
            magazine,
        },
    }
}

fn card_matches_target(card: &PreparedCard, axis: ItemAxis, key: &str) -> bool {
    match axis {
        ItemAxis::Band => key
            .split(',')
            .filter_map(|part| part.parse::<u32>().ok())
            .any(|band_id| band_id == card.band_id),
        ItemAxis::Attribute => key
            .split(',')
            .any(|attribute| attribute == card.attribute.as_str()),
    }
}

fn team_touches_target(
    team: &ScoreRangeTeam,
    cards: &[ResolvedCard<'_>],
    axis: ItemAxis,
    key: &str,
) -> bool {
    team.card_ids.iter().any(|card_id| {
        cards
            .iter()
            .find(|entry| entry.card.card_id == *card_id)
            .is_some_and(|entry| card_matches_target(entry.card, axis, key))
    })
}

fn resolve_card_stats(
    cards: &[ResolvedCard<'_>],
    area_item_percent: &AreaItemPercent,
    items: &SelectedAreaItems,
) -> Vec<f64> {
    cards
        .iter()
        .map(|entry| {
            entry.card.add_up_stat(
                area_item_percent,
                &items.band,
                &items.attribute,
                items.magazine.as_str(),
            )
        })
        .collect()
}

fn build_pair_index(
    cards: &[ResolvedCard<'_>],
    stats: &[f64],
    only_touching: Option<&[bool]>,
) -> PairIndex {
    let mut result = PairIndex::default();
    for left in 0..cards.len() {
        for right in (left + 1)..cards.len() {
            if cards[left].card.character_id == cards[right].card.character_id
                || only_touching.is_some_and(|affected| !affected[left] && !affected[right])
            {
                continue;
            }
            let pair = PairRecord {
                stat: stats[left] + stats[right],
                point_bonus_micros: cards[left]
                    .point_bonus_micros
                    .saturating_add(cards[right].point_bonus_micros),
                left,
                right,
            };
            result
                .by_bonus
                .entry(pair.point_bonus_micros)
                .or_default()
                .push(pair);
            result.record_count += 1;
        }
    }
    for records in result.by_bonus.values_mut() {
        records.sort_by(|left, right| {
            left.stat
                .total_cmp(&right.stat)
                .then_with(|| {
                    cards[left.left]
                        .card
                        .card_id
                        .cmp(&cards[right.left].card.card_id)
                })
                .then_with(|| {
                    cards[left.right]
                        .card
                        .card_id
                        .cmp(&cards[right.right].card.card_id)
                })
        });
    }
    result
}

fn insert_candidate(
    candidates: &mut CandidateMap,
    key: CandidateKey,
    candidate: Candidate,
) -> bool {
    let replace = candidates.get(&key).is_none_or(|current| {
        candidate_precedes(
            &candidate.team,
            &candidate.plan,
            &current.team,
            &current.plan,
        )
    });
    if replace {
        candidates.insert(key, candidate);
    }
    replace
}

fn resolve_cards_for_items<'a>(
    cards: &[BucketCard<'a>],
    area_item_percent: &AreaItemPercent,
    items: &SelectedAreaItems,
) -> Vec<ResolvedCard<'a>> {
    let mut canonical = BTreeMap::<(u32, u64, u64, u32, &'static str), ResolvedCard<'a>>::new();
    for entry in cards {
        let exact_stat = entry.card.add_up_stat(
            area_item_percent,
            &items.band,
            &items.attribute,
            items.magazine.as_str(),
        );
        let resolved = ResolvedCard {
            card: entry.card,
            stat: exact_stat,
            point_bonus_micros: entry.point_bonus_micros,
        };
        let key = (
            resolved.card.character_id,
            exact_stat.to_bits(),
            resolved.point_bonus_micros,
            resolved.card.band_id,
            resolved.card.attribute.as_str(),
        );
        canonical
            .entry(key)
            .and_modify(|current| {
                if resolved.card.card_id < current.card.card_id {
                    *current = resolved;
                }
            })
            .or_insert(resolved);
    }
    canonical.into_values().collect()
}

fn resolve_cards_for_batch<'a>(
    cards: &[BucketCard<'a>],
    area_item_percent: &AreaItemPercent,
    base_items: &SelectedAreaItems,
    inner_contexts: &[&SelectedAreaItems],
) -> Vec<ResolvedCard<'a>> {
    let mut canonical =
        BTreeMap::<(u32, Vec<u64>, u64, u32, &'static str), ResolvedCard<'a>>::new();
    for entry in cards {
        let stat_for = |items: &SelectedAreaItems| {
            entry.card.add_up_stat(
                area_item_percent,
                &items.band,
                &items.attribute,
                items.magazine.as_str(),
            )
        };
        let exact_base_stat = stat_for(base_items);
        let mut stat_signature = Vec::with_capacity(inner_contexts.len() + 1);
        stat_signature.push(exact_base_stat.to_bits());
        stat_signature.extend(inner_contexts.iter().map(|items| stat_for(items).to_bits()));
        let resolved = ResolvedCard {
            card: entry.card,
            stat: exact_base_stat,
            point_bonus_micros: entry.point_bonus_micros,
        };
        let key = (
            resolved.card.character_id,
            stat_signature,
            resolved.point_bonus_micros,
            resolved.card.band_id,
            resolved.card.attribute.as_str(),
        );
        canonical
            .entry(key)
            .and_modify(|current| {
                if resolved.card.card_id < current.card.card_id {
                    *current = resolved;
                }
            })
            .or_insert(resolved);
    }
    canonical.into_values().collect()
}

fn build_pair_stat_bounds_for_cards(cards: &[ResolvedCard<'_>], stats: &[f64]) -> PairStatBounds {
    let mut result = PairStatBounds::new();
    for left in 0..cards.len() {
        for right in (left + 1)..cards.len() {
            if cards[left].card.character_id == cards[right].card.character_id {
                continue;
            }
            let pair_bonus = cards[left]
                .point_bonus_micros
                .saturating_add(cards[right].point_bonus_micros);
            let pair_stat = stats[left] + stats[right];
            result
                .entry(pair_bonus)
                .and_modify(|bounds| {
                    bounds.0 = bounds.0.min(pair_stat);
                    bounds.1 = bounds.1.max(pair_stat);
                })
                .or_insert((pair_stat, pair_stat));
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn item_context_may_match(
    request: &ScoreRangeRequest,
    target_delta: u64,
    divisors: &[u64],
    cards: &[ResolvedCard<'_>],
    stats: &[f64],
    songs: &[SongModel],
    interval_cache: &mut BTreeMap<u32, ValidIntervalSet>,
    score_lower_bounds: &mut BTreeMap<(usize, u64), Option<i32>>,
) -> Result<bool, ScoreRangeError> {
    // This is a relaxed 2+3 feasibility check. It may admit overlapping characters across the
    // two halves, but a miss proves that the exact search cannot produce a five-card team.
    let pair_bounds = build_pair_stat_bounds_for_cards(cards, stats);
    if pair_bounds.is_empty() {
        return Ok(false);
    }
    let bonus_groups = cards_by_bonus_sorted(cards, stats);
    let mut triple_range_cache = Vec::<(u64, Vec<ExactStatRange>)>::new();
    for first_group in 0..bonus_groups.len() {
        for second_group in first_group..bonus_groups.len() {
            for third_group in second_group..bonus_groups.len() {
                let first_indices = &bonus_groups[first_group].1;
                let second_indices = &bonus_groups[second_group].1;
                let third_indices = &bonus_groups[third_group].1;
                let Some((group_min_stat, group_max_stat)) = triple_group_stat_bounds(
                    stats,
                    first_group == second_group,
                    second_group == third_group,
                    first_indices,
                    second_indices,
                    third_indices,
                ) else {
                    continue;
                };
                let triple_bonus = bonus_groups[first_group]
                    .0
                    .saturating_add(bonus_groups[second_group].0)
                    .saturating_add(bonus_groups[third_group].0);
                let range_index = if let Some(index) = triple_range_cache
                    .iter()
                    .position(|(bonus, _)| *bonus == triple_bonus)
                {
                    index
                } else {
                    let ranges = build_triple_stat_ranges(
                        request,
                        target_delta,
                        divisors,
                        triple_bonus,
                        &pair_bounds,
                        songs,
                        interval_cache,
                        score_lower_bounds,
                    )?;
                    triple_range_cache.push((triple_bonus, ranges));
                    triple_range_cache.len() - 1
                };
                if triple_range_cache[range_index].1.iter().any(|range| {
                    range.min_stat <= group_max_stat && group_min_stat < range.max_stat_exclusive
                }) {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn enumerate_item_context(
    request: &ScoreRangeRequest,
    target_delta: u64,
    divisors: &[u64],
    mode: SongMode,
    skill_key: SkillBucketKey,
    skill: TeamCardSkill,
    items: &SelectedAreaItems,
    area_item_percent: &AreaItemPercent,
    cards: &[ResolvedCard<'_>],
    stats: &[f64],
    pairs: PairQuery<'_>,
    triple_must_touch: Option<&[bool]>,
    songs: &[SongModel],
    interval_cache: &mut BTreeMap<u32, ValidIntervalSet>,
    score_lower_bounds: &mut BTreeMap<(usize, u64), Option<i32>>,
    candidates: &mut CandidateMap,
) -> Result<(), ScoreRangeError> {
    debug_assert_eq!(cards.len(), stats.len());
    debug_assert!(triple_must_touch.is_none_or(|affected| affected.len() == cards.len()));
    let witness_budget = request.max_results.max(1);
    if candidates.len() >= witness_budget {
        return Ok(());
    }

    // A fixed triple-bonus group has fixed target stat ranges. Sorting each group lets the third
    // card be selected by two binary searches instead of enumerating and rejecting every triple.
    let bonus_groups = cards_by_bonus_sorted(cards, stats);
    let pair_bounds = pair_query_stat_bounds(&pairs);
    let mut triple_range_cache = Vec::<(u64, Vec<ExactStatRange>)>::new();
    for first_group in 0..bonus_groups.len() {
        for second_group in first_group..bonus_groups.len() {
            for third_group in second_group..bonus_groups.len() {
                let triple_bonus = bonus_groups[first_group]
                    .0
                    .saturating_add(bonus_groups[second_group].0)
                    .saturating_add(bonus_groups[third_group].0);
                let range_index = if let Some(index) = triple_range_cache
                    .iter()
                    .position(|(bonus, _)| *bonus == triple_bonus)
                {
                    index
                } else {
                    let ranges = build_triple_stat_ranges(
                        request,
                        target_delta,
                        divisors,
                        triple_bonus,
                        &pair_bounds,
                        songs,
                        interval_cache,
                        score_lower_bounds,
                    )?;
                    triple_range_cache.push((triple_bonus, ranges));
                    triple_range_cache.len() - 1
                };
                let triple_ranges = &triple_range_cache[range_index].1;
                let first_indices = &bonus_groups[first_group].1;
                let second_indices = &bonus_groups[second_group].1;
                let third_indices = &bonus_groups[third_group].1;
                let Some((group_min_stat, group_max_stat)) = triple_group_stat_bounds(
                    stats,
                    first_group == second_group,
                    second_group == third_group,
                    first_indices,
                    second_indices,
                    third_indices,
                ) else {
                    continue;
                };
                if !triple_ranges.iter().any(|range| {
                    range.min_stat <= group_max_stat && group_min_stat < range.max_stat_exclusive
                }) {
                    continue;
                }

                for (first_position, &first) in first_indices.iter().enumerate() {
                    let second_start = if first_group == second_group {
                        first_position + 1
                    } else {
                        0
                    };
                    for (second_position, &second) in
                        second_indices.iter().enumerate().skip(second_start)
                    {
                        if cards[first].card.character_id == cards[second].card.character_id {
                            continue;
                        }
                        let partial_stat = stats[first] + stats[second];
                        let third_start = if second_group == third_group {
                            second_position + 1
                        } else {
                            0
                        };
                        for range in triple_ranges {
                            let start = third_indices
                                .partition_point(|&index| {
                                    partial_stat + stats[index] < range.min_stat
                                })
                                .max(third_start);
                            let end = third_indices.partition_point(|&index| {
                                partial_stat + stats[index] < range.max_stat_exclusive
                            });
                            for &third in &third_indices[start.min(end)..end] {
                                if triple_must_touch.is_some_and(|affected| {
                                    !affected[first] && !affected[second] && !affected[third]
                                }) {
                                    continue;
                                }
                                let third_character = cards[third].card.character_id;
                                if third_character == cards[first].card.character_id
                                    || third_character == cards[second].card.character_id
                                {
                                    continue;
                                }
                                if search_triple_candidate(
                                    request,
                                    target_delta,
                                    mode,
                                    skill_key,
                                    skill,
                                    items,
                                    area_item_percent,
                                    cards,
                                    stats,
                                    triple_bonus,
                                    [first, second, third],
                                    &pairs,
                                    songs,
                                    interval_cache,
                                    candidates,
                                    witness_budget,
                                )? {
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn cards_by_bonus_sorted(cards: &[ResolvedCard<'_>], stats: &[f64]) -> Vec<(u64, Vec<usize>)> {
    let mut by_bonus = BTreeMap::<u64, Vec<usize>>::new();
    for (index, card) in cards.iter().enumerate() {
        by_bonus
            .entry(card.point_bonus_micros)
            .or_default()
            .push(index);
    }
    let mut result = by_bonus.into_iter().collect::<Vec<_>>();
    for (_, indices) in &mut result {
        indices.sort_unstable_by(|&left, &right| {
            stats[left]
                .total_cmp(&stats[right])
                .then_with(|| left.cmp(&right))
        });
    }
    result
}

fn triple_group_stat_bounds(
    stats: &[f64],
    first_equals_second: bool,
    second_equals_third: bool,
    first: &[usize],
    second: &[usize],
    third: &[usize],
) -> Option<(f64, f64)> {
    let (minimum, maximum) = match (first_equals_second, second_equals_third) {
        (true, true) => {
            if first.len() < 3 {
                return None;
            }
            (&first[..3], &first[(first.len() - 3)..])
        }
        (true, false) => {
            if first.len() < 2 || third.is_empty() {
                return None;
            }
            return Some((
                sum_three_stats(stats, first[0], first[1], third[0]),
                sum_three_stats(
                    stats,
                    first[first.len() - 2],
                    first[first.len() - 1],
                    third[third.len() - 1],
                ),
            ));
        }
        (false, true) => {
            if first.is_empty() || second.len() < 2 {
                return None;
            }
            return Some((
                sum_three_stats(stats, first[0], second[0], second[1]),
                sum_three_stats(
                    stats,
                    first[first.len() - 1],
                    second[second.len() - 2],
                    second[second.len() - 1],
                ),
            ));
        }
        (false, false) => {
            if first.is_empty() || second.is_empty() || third.is_empty() {
                return None;
            }
            return Some((
                sum_three_stats(stats, first[0], second[0], third[0]),
                sum_three_stats(
                    stats,
                    first[first.len() - 1],
                    second[second.len() - 1],
                    third[third.len() - 1],
                ),
            ));
        }
    };
    Some((
        sum_three_stats(stats, minimum[0], minimum[1], minimum[2]),
        sum_three_stats(stats, maximum[0], maximum[1], maximum[2]),
    ))
}

fn sum_three_stats(stats: &[f64], first: usize, second: usize, third: usize) -> f64 {
    stats[first] + stats[second] + stats[third]
}

#[allow(clippy::too_many_arguments)]
fn search_triple_candidate(
    request: &ScoreRangeRequest,
    target_delta: u64,
    mode: SongMode,
    skill_key: SkillBucketKey,
    skill: TeamCardSkill,
    items: &SelectedAreaItems,
    area_item_percent: &AreaItemPercent,
    cards: &[ResolvedCard<'_>],
    stats: &[f64],
    triple_bonus: u64,
    triple: [usize; 3],
    pairs: &PairQuery<'_>,
    songs: &[SongModel],
    interval_cache: &BTreeMap<u32, ValidIntervalSet>,
    candidates: &mut CandidateMap,
    witness_budget: usize,
) -> Result<bool, ScoreRangeError> {
    let triple_stat = sum_three_stats(stats, triple[0], triple[1], triple[2]);
    for (&pair_bonus, base_pair_records) in &pairs.base().by_bonus {
        let total_bonus_micros = triple_bonus.saturating_add(pair_bonus);
        let point_bonus_basis_points =
            ((total_bonus_micros + 5_000) / 10_000).min(u32::MAX as u64) as u32;
        let interval_set = interval_cache
            .get(&point_bonus_basis_points)
            .expect("triple ranges prepare every pair bonus interval");
        let (min_pair_stat, max_pair_stat) = pair_stat_bounds(pairs, pair_bonus, base_pair_records);
        let min_total_stat = triple_stat + min_pair_stat;
        let max_total_stat = triple_stat + max_pair_stat;
        let first_range = interval_set
            .union
            .partition_point(|range| range.max_stat as f64 + 1.0 <= min_total_stat);
        for range in interval_set.union[first_range..]
            .iter()
            .take_while(|range| range.min_stat as f64 <= max_total_stat)
        {
            let min_pair = range.min_stat as f64 - triple_stat;
            let max_pair_exclusive = range.max_stat as f64 + 1.0 - triple_stat;
            let skip_touching = match pairs {
                PairQuery::Full(_) => None,
                PairQuery::Incremental { affected_cards, .. } => Some(*affected_cards),
            };
            let mut found = candidate_from_pair_records(
                request,
                target_delta,
                mode,
                skill_key,
                skill,
                items,
                area_item_percent,
                cards,
                songs,
                interval_set,
                point_bonus_basis_points,
                triple,
                min_pair,
                max_pair_exclusive,
                triple_stat,
                base_pair_records,
                skip_touching,
            )?;
            if found.is_none() {
                if let PairQuery::Incremental {
                    adjusted_touching, ..
                } = pairs
                {
                    if let Some(adjusted_records) = adjusted_touching.by_bonus.get(&pair_bonus) {
                        found = candidate_from_pair_records(
                            request,
                            target_delta,
                            mode,
                            skill_key,
                            skill,
                            items,
                            area_item_percent,
                            cards,
                            songs,
                            interval_set,
                            point_bonus_basis_points,
                            triple,
                            min_pair,
                            max_pair_exclusive,
                            triple_stat,
                            adjusted_records,
                            None,
                        )?;
                    }
                }
            }
            if let Some((key, candidate)) = found {
                insert_candidate(candidates, key, candidate);
                if candidates.len() >= witness_budget {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn build_triple_stat_ranges(
    request: &ScoreRangeRequest,
    target_delta: u64,
    divisors: &[u64],
    triple_bonus: u64,
    pair_bounds: &PairStatBounds,
    songs: &[SongModel],
    interval_cache: &mut BTreeMap<u32, ValidIntervalSet>,
    score_lower_bounds: &mut BTreeMap<(usize, u64), Option<i32>>,
) -> Result<Vec<ExactStatRange>, ScoreRangeError> {
    let mut ranges = Vec::new();
    for (&pair_bonus, &(min_pair_stat, max_pair_stat)) in pair_bounds {
        let total_bonus_micros = triple_bonus.saturating_add(pair_bonus);
        let point_bonus_basis_points =
            ((total_bonus_micros + 5_000) / 10_000).min(u32::MAX as u64) as u32;
        if let std::collections::btree_map::Entry::Vacant(entry) =
            interval_cache.entry(point_bonus_basis_points)
        {
            entry.insert(build_valid_intervals(
                request,
                target_delta,
                divisors,
                point_bonus_basis_points,
                songs,
                score_lower_bounds,
            )?);
        }
        for range in &interval_cache[&point_bonus_basis_points].union {
            let min_stat = range.min_stat as f64 - max_pair_stat;
            let max_stat_exclusive = range.max_stat as f64 + 1.0 - min_pair_stat;
            if min_stat < max_stat_exclusive {
                ranges.push(ExactStatRange {
                    min_stat,
                    max_stat_exclusive,
                });
            }
        }
    }
    Ok(merge_exact_stat_ranges(ranges))
}

fn pair_query_stat_bounds(pairs: &PairQuery<'_>) -> PairStatBounds {
    pairs
        .base()
        .by_bonus
        .iter()
        .map(|(&pair_bonus, records)| (pair_bonus, pair_stat_bounds(pairs, pair_bonus, records)))
        .collect()
}

fn pair_stat_bounds(
    pairs: &PairQuery<'_>,
    pair_bonus: u64,
    base_pair_records: &[PairRecord],
) -> (f64, f64) {
    let mut min_pair_stat = base_pair_records
        .first()
        .map(|pair| pair.stat)
        .unwrap_or(f64::INFINITY);
    let mut max_pair_stat = base_pair_records
        .last()
        .map(|pair| pair.stat)
        .unwrap_or(f64::NEG_INFINITY);
    if let PairQuery::Incremental {
        adjusted_touching, ..
    } = pairs
    {
        if let Some(adjusted_records) = adjusted_touching.by_bonus.get(&pair_bonus) {
            if let Some(first) = adjusted_records.first() {
                min_pair_stat = min_pair_stat.min(first.stat);
            }
            if let Some(last) = adjusted_records.last() {
                max_pair_stat = max_pair_stat.max(last.stat);
            }
        }
    }
    (min_pair_stat, max_pair_stat)
}

#[allow(clippy::too_many_arguments)]
fn candidate_from_pair_records(
    request: &ScoreRangeRequest,
    target_delta: u64,
    mode: SongMode,
    skill_key: SkillBucketKey,
    skill: TeamCardSkill,
    items: &SelectedAreaItems,
    _area_item_percent: &AreaItemPercent,
    cards: &[ResolvedCard<'_>],
    songs: &[SongModel],
    interval_set: &ValidIntervalSet,
    point_bonus_basis_points: u32,
    triple: [usize; 3],
    min_pair: f64,
    max_pair_exclusive: f64,
    triple_stat: f64,
    pair_records: &[PairRecord],
    skip_touching: Option<&[bool]>,
) -> Result<Option<(CandidateKey, Candidate)>, ScoreRangeError> {
    let start = pair_records.partition_point(|pair| pair.stat < min_pair);
    let end = pair_records.partition_point(|pair| pair.stat < max_pair_exclusive);
    for pair in pair_records[start..end].iter().rev() {
        if skip_touching.is_some_and(|affected| affected[pair.left] || affected[pair.right])
            || !pair_is_disjoint(pair, triple, cards)
        {
            continue;
        }
        let indices = [pair.left, pair.right, triple[0], triple[1], triple[2]];
        if !matches_mode(mode, indices, cards) {
            continue;
        }
        let total_stat = (pair.stat + triple_stat).floor() as i32;
        let Some(interval) = interval_set
            .ranked
            .iter()
            .find(|interval| interval.min_stat <= total_stat && total_stat <= interval.max_stat)
        else {
            continue;
        };
        let song = &songs[interval.song_index];
        let score = song.exact.score(total_stat);
        let base_points = points_for_score_with_support(
            request.event_type,
            score,
            point_bonus_basis_points,
            1,
            request.mission_support_pt_bonus.unwrap_or_default(),
        )?;
        if base_points != interval.base_points
            || base_points == 0
            || !target_delta.is_multiple_of(base_points)
        {
            continue;
        }
        let Some(plan) = one_song_plan(song.key, score, base_points, target_delta) else {
            continue;
        };
        let mut card_ids = indices.map(|index| cards[index].card.card_id);
        card_ids.sort_unstable();
        let mut resolved_skill = skill;
        resolved_skill.card_id = card_ids[0];
        let team = ScoreRangeTeam {
            card_ids,
            stat: total_stat,
            skill: resolved_skill,
            point_bonus_basis_points,
            items: items.clone(),
            recovery_mode: None,
        };
        let key = (skill_key, point_bonus_basis_points, total_stat, song.key);
        return Ok(Some((key, Candidate { team, plan })));
    }
    Ok(None)
}

fn build_valid_intervals(
    request: &ScoreRangeRequest,
    target_delta: u64,
    divisors: &[u64],
    point_bonus_basis_points: u32,
    songs: &[SongModel],
    score_lower_bounds: &mut BTreeMap<(usize, u64), Option<i32>>,
) -> Result<ValidIntervalSet, ScoreRangeError> {
    let trace = std::env::var_os("BANGDREAM_OPTIMIZE_SCORE_RANGE_TRACE").is_some();
    let started = trace.then(Timer::start);
    let mut result = Vec::new();
    for (song_index, song) in songs.iter().enumerate() {
        for &base_points in divisors {
            let Some(score_interval) = score_interval_for_points_with_support(
                request.event_type,
                base_points,
                point_bonus_basis_points,
                1,
                request.mission_support_pt_bonus.unwrap_or_default(),
            )?
            else {
                continue;
            };
            let Some(min_stat) = cached_lower_bound_score(
                score_lower_bounds,
                song_index,
                &song.exact,
                score_interval.min_score,
            ) else {
                continue;
            };
            let max_stat = match score_interval.max_score.checked_add(1) {
                Some(next_score) => cached_lower_bound_score(
                    score_lower_bounds,
                    song_index,
                    &song.exact,
                    next_score,
                )
                .map(|stat| stat.saturating_sub(1))
                .unwrap_or(i32::MAX),
                None => i32::MAX,
            };
            if min_stat <= max_stat {
                result.push(ValidStatInterval {
                    min_stat,
                    max_stat,
                    song_index,
                    base_points,
                });
            }
        }
    }
    result.sort_by_key(|interval| {
        (
            objective_for_base_points(target_delta, interval.base_points),
            songs[interval.song_index].key,
            interval.min_stat,
        )
    });
    result.dedup_by_key(|interval| {
        (
            interval.min_stat,
            interval.max_stat,
            interval.song_index,
            interval.base_points,
        )
    });
    let raw_count = result.len();
    let union = union_stat_ranges(&result);
    if trace {
        eprintln!(
            "score-range intervals: bonus={} songs={} divisors={} raw={} union={} elapsed_ms={:.3}",
            point_bonus_basis_points,
            songs.len(),
            divisors.len(),
            raw_count,
            union.len(),
            optional_elapsed_ms(started),
        );
    }
    Ok(ValidIntervalSet {
        union,
        ranked: result,
    })
}

fn cached_lower_bound_score(
    cache: &mut BTreeMap<(usize, u64), Option<i32>>,
    song_index: usize,
    model: &CompressedAutoScore,
    target: u64,
) -> Option<i32> {
    let key = (song_index, target);
    if let Some(&value) = cache.get(&key) {
        return value;
    }
    let value = lower_bound_score(model, target);
    cache.insert(key, value);
    value
}

fn union_stat_ranges(intervals: &[ValidStatInterval]) -> Vec<StatRange> {
    let ranges = intervals
        .iter()
        .map(|interval| StatRange {
            min_stat: interval.min_stat,
            max_stat: interval.max_stat,
        })
        .collect::<Vec<_>>();
    merge_stat_ranges(ranges)
}

fn merge_stat_ranges(mut ranges: Vec<StatRange>) -> Vec<StatRange> {
    ranges.sort_by_key(|range| (range.min_stat, range.max_stat));
    let mut result = Vec::<StatRange>::new();
    for range in ranges {
        if let Some(previous) = result.last_mut() {
            if range.min_stat <= previous.max_stat.saturating_add(1) {
                previous.max_stat = previous.max_stat.max(range.max_stat);
                continue;
            }
        }
        result.push(range);
    }
    result
}

fn merge_exact_stat_ranges(mut ranges: Vec<ExactStatRange>) -> Vec<ExactStatRange> {
    ranges.sort_by(|left, right| {
        left.min_stat
            .total_cmp(&right.min_stat)
            .then_with(|| left.max_stat_exclusive.total_cmp(&right.max_stat_exclusive))
    });
    let mut result = Vec::<ExactStatRange>::new();
    for range in ranges {
        if let Some(previous) = result.last_mut() {
            if range.min_stat <= previous.max_stat_exclusive {
                previous.max_stat_exclusive =
                    previous.max_stat_exclusive.max(range.max_stat_exclusive);
                continue;
            }
        }
        result.push(range);
    }
    result
}

fn lower_bound_score(model: &CompressedAutoScore, target: u64) -> Option<i32> {
    model.lower_bound_stat(target)
}

fn pair_is_disjoint(pair: &PairRecord, triple: [usize; 3], cards: &[ResolvedCard<'_>]) -> bool {
    let left_character = cards[pair.left].card.character_id;
    let right_character = cards[pair.right].card.character_id;
    triple.into_iter().all(|index| {
        let character = cards[index].card.character_id;
        character != left_character && character != right_character
    })
}

fn matches_mode(mode: SongMode, indices: [usize; 5], cards: &[ResolvedCard<'_>]) -> bool {
    let first_band = cards[indices[0]].card.band_id;
    let first_attribute = cards[indices[0]].card.attribute;
    let same_band = indices
        .iter()
        .all(|&index| cards[index].card.band_id == first_band);
    let same_attribute = indices
        .iter()
        .all(|&index| cards[index].card.attribute == first_attribute);
    match mode {
        SongMode::Mixed => !same_band && !same_attribute,
        SongMode::UnifiedBand(_) => !same_attribute,
        SongMode::UnifiedAttribute(_) => !same_band,
        SongMode::UnifiedBandAttribute(_, _) => true,
    }
}

fn one_song_plan(
    song: SongKey,
    score: i32,
    base_points: u64,
    target: u64,
) -> Option<Vec<ScoreRangePlay>> {
    if base_points == 0 || !target.is_multiple_of(base_points) {
        return None;
    }
    let mut units = target / base_points;
    let mut result = Vec::new();
    for fire_multiplier in [15_u32, 10, 5, 1] {
        let count = units / fire_multiplier as u64;
        if count == 0 {
            continue;
        }
        result.push(ScoreRangePlay {
            song_id: song.song_id,
            difficulty: song.difficulty,
            fire_multiplier,
            score,
            pt: base_points.saturating_mul(fire_multiplier as u64),
            count: u32::try_from(count).ok()?,
        });
        units %= fire_multiplier as u64;
    }
    (units == 0).then_some(result)
}

fn objective_for_units(mut units: u64) -> PlanObjective {
    let mut objective = PlanObjective {
        play_count: 0,
        total_fire_cost: 0,
    };
    for fire_multiplier in [15_u32, 10, 5, 1] {
        let count = units / u64::from(fire_multiplier);
        objective.play_count = objective.play_count.saturating_add(count);
        objective.total_fire_cost = objective
            .total_fire_cost
            .saturating_add(count.saturating_mul(fire_cost_for_multiplier(fire_multiplier)));
        units %= u64::from(fire_multiplier);
    }
    objective
}

fn objective_for_base_points(target: u64, base_points: u64) -> PlanObjective {
    debug_assert!(base_points > 0 && target.is_multiple_of(base_points));
    objective_for_units(target / base_points)
}

fn ranked_base_point_layers(target: u64) -> Vec<(PlanObjective, u64)> {
    let mut layers = divisors_descending(target)
        .into_iter()
        .map(|base_points| (objective_for_base_points(target, base_points), base_points))
        .collect::<Vec<_>>();
    layers.sort_unstable_by_key(|(objective, base_points)| {
        (*objective, std::cmp::Reverse(*base_points))
    });
    layers
}

fn divisors_descending(value: u64) -> Vec<u64> {
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    let mut divisor = 1_u64;
    while divisor <= value / divisor {
        if value.is_multiple_of(divisor) {
            lower.push(divisor);
            if divisor != value / divisor {
                upper.push(value / divisor);
            }
        }
        divisor += 1;
    }
    lower.extend(upper.into_iter().rev());
    lower.sort_unstable_by(|left, right| right.cmp(left));
    lower
}

fn candidate_precedes(
    team: &ScoreRangeTeam,
    plan: &[ScoreRangePlay],
    current_team: &ScoreRangeTeam,
    current_plan: &[ScoreRangePlay],
) -> bool {
    (
        plan_rank(plan),
        team.card_ids,
        &team.items.band,
        &team.items.attribute,
        team.items.magazine.as_str(),
    ) < (
        plan_rank(current_plan),
        current_team.card_ids,
        &current_team.items.band,
        &current_team.items.attribute,
        current_team.items.magazine.as_str(),
    )
}

fn plan_objective(plan: &[ScoreRangePlay]) -> PlanObjective {
    PlanObjective {
        play_count: plan.iter().map(|play| play.count as u64).sum(),
        total_fire_cost: total_fire_cost(plan),
    }
}

fn plan_rank(plan: &[ScoreRangePlay]) -> (PlanObjective, Vec<(std::cmp::Reverse<i32>, u32, u8)>) {
    let mut song_preference = plan
        .iter()
        .map(|play| (std::cmp::Reverse(play.score), play.song_id, play.difficulty))
        .collect::<Vec<_>>();
    song_preference.sort_unstable();
    song_preference.dedup();
    (plan_objective(plan), song_preference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::preparation::StatRate;
    use crate::{
        AreaItemPercent, Attribute, Chart, ChartNode, ChartNodeType, EventType, Magazine, ScoreUp,
        SongSelection, StatValue,
    };

    #[test]
    fn divisor_enumeration_is_complete_and_descending() {
        assert_eq!(
            divisors_descending(60),
            vec![60, 30, 20, 15, 12, 10, 6, 5, 4, 3, 2, 1]
        );
    }

    #[test]
    fn song_bound_frontiers_preserve_score_extrema() {
        let terms = vec![(0.8, 100), (0.8, 80), (0.6, 120), (0.5, 90)];
        let upper = optimistic_term_frontier(terms.clone());
        let lower = pessimistic_term_frontier(terms.clone());
        for stat in [1, 12_345, 543_210] {
            for multiplier in [1.0, 1.6, 2.51] {
                assert_eq!(
                    terms
                        .iter()
                        .map(|&term| {
                            crate::Chart::optimistic_auto_score_from_terms(term, stat, multiplier)
                        })
                        .max(),
                    upper
                        .iter()
                        .map(|&term| {
                            crate::Chart::optimistic_auto_score_from_terms(term, stat, multiplier)
                        })
                        .max(),
                );
                assert_eq!(
                    terms
                        .iter()
                        .map(|&term| {
                            crate::Chart::pessimistic_auto_score_from_terms(term, stat, multiplier)
                        })
                        .min(),
                    lower
                        .iter()
                        .map(|&term| {
                            crate::Chart::pessimistic_auto_score_from_terms(term, stat, multiplier)
                        })
                        .min(),
                );
            }
        }
    }

    #[test]
    fn base_point_layers_follow_play_count_before_fire_cost() {
        let layers = ranked_base_point_layers(210);
        let layer_for = |base_points| {
            layers
                .iter()
                .position(|(_, candidate)| *candidate == base_points)
                .unwrap()
        };

        assert_eq!(
            objective_for_base_points(210, 14),
            PlanObjective {
                play_count: 1,
                total_fire_cost: 3,
            }
        );
        assert_eq!(
            objective_for_base_points(210, 15),
            PlanObjective {
                play_count: 5,
                total_fire_cost: 2,
            }
        );
        assert!(layer_for(14) < layer_for(15));
    }

    #[test]
    fn plan_rank_uses_actual_fire_cost_after_play_count() {
        let play = |fire_multiplier, count| ScoreRangePlay {
            song_id: 1,
            difficulty: 3,
            fire_multiplier,
            score: 1_000_000,
            pt: 1_000,
            count,
        };
        let nine_fire = vec![play(5, 9)];
        let eight_fire = vec![play(15, 2), play(10, 1), play(1, 6)];

        assert_eq!(plan_objective(&nine_fire).play_count, 9);
        assert_eq!(plan_objective(&nine_fire).total_fire_cost, 9);
        assert_eq!(plan_objective(&eight_fire).play_count, 9);
        assert_eq!(plan_objective(&eight_fire).total_fire_cost, 8);
        assert!(plan_rank(&eight_fire) < plan_rank(&nine_fire));
    }

    #[test]
    fn exact_score_lower_bound_finds_first_stat() {
        let model = CompressedAutoScoreTest::linear();
        assert_eq!(lower_bound_score(&model, 18), Some(9));
    }

    #[test]
    fn raw_mitm_finds_five_distinct_characters_for_one_song() {
        let cards = (1..=5)
            .map(|id| PreparedCard {
                card_id: id,
                character_id: id,
                band_id: if id == 1 { 1 } else { 2 },
                rarity: 4,
                attribute: if id == 1 {
                    Attribute::Cool
                } else {
                    Attribute::Pure
                },
                level: 60,
                training: true,
                illust_training_status: true,
                episodes: [true, true],
                limit_break_rank: 0,
                skill_level: 1,
                stat: StatValue {
                    performance: 100.4,
                    technique: 100.0,
                    visual: 100.0,
                },
                event_add_stat: StatValue::zero(),
                skill: TeamCardSkill {
                    card_id: id,
                    duration: 5.0,
                    score_up: 1.0,
                    rateup: false,
                },
                score_up: ScoreUp {
                    default: 1.0,
                    unification_activate_effect_value: None,
                    unification_activate_condition_band_id: None,
                    unification_activate_condition_type: None,
                },
            })
            .collect::<Vec<_>>();
        let items = vec![SelectedAreaItems {
            band: String::new(),
            attribute: String::new(),
            magazine: Magazine::Performance,
        }];
        let domain = super::super::prepare_score_range_team_domain(
            &cards,
            &AreaItemPercent::empty(),
            &items,
            &BTreeMap::new(),
        );
        let chart = Chart::new(
            5,
            (0..6)
                .map(|index| ChartNode {
                    node_type: ChartNodeType::Skill,
                    time: index as f64 * 10.0,
                })
                .collect(),
        );
        let songs = vec![ScoreRangeSong::new(
            SongSelection {
                song_id: 1,
                difficulty: 3,
            },
            chart,
        )
        .unwrap()];
        let request = ScoreRangeRequest {
            event_type: EventType::Versus,
            current_pt: 0,
            target_total_pt: 100,
            auto_base_multiplier: None,
            mission_support_pt_bonus: None,
            max_results: 20,
        };

        let result = search_raw_domain(&request, &domain, &songs, 100).unwrap();

        assert!(!result.is_empty());
        assert_eq!(result[0].0.card_ids, [1, 2, 3, 4, 5]);
        assert_eq!(result[0].0.stat, 1_502);
        assert_eq!(result[0].1.iter().map(|play| play.count).sum::<u32>(), 1);
    }

    #[test]
    fn pair_scan_reaches_disjoint_candidate_after_thirty_two_conflicts() {
        let mut cards = (0..37)
            .map(|index| {
                fixture_card(
                    index as u32 + 1,
                    if index == 1 { 2 } else { 1 },
                    if index == 1 {
                        Attribute::Pure
                    } else {
                        Attribute::Cool
                    },
                )
            })
            .collect::<Vec<_>>();
        for card in &mut cards {
            card.stat = StatValue::zero();
        }
        let resolved = cards
            .iter()
            .map(|card| ResolvedCard {
                card,
                stat: 0.0,
                point_bonus_micros: 0,
            })
            .collect::<Vec<_>>();
        let mut pair_records = vec![PairRecord {
            stat: 0.0,
            point_bonus_micros: 0,
            left: 35,
            right: 36,
        }];
        pair_records.extend((0..32).map(|offset| PairRecord {
            stat: (offset + 1) as f64,
            point_bonus_micros: 0,
            left: 0,
            right: offset as usize + 3,
        }));
        let songs = vec![SongModel {
            key: SongKey {
                song_id: 1,
                difficulty: 3,
            },
            exact: CompressedAutoScoreTest::linear(),
        }];
        let intervals = ValidIntervalSet {
            union: vec![StatRange {
                min_stat: 0,
                max_stat: 32,
            }],
            ranked: vec![ValidStatInterval {
                min_stat: 0,
                max_stat: 32,
                song_index: 0,
                base_points: 100,
            }],
        };
        let request = ScoreRangeRequest {
            event_type: EventType::Versus,
            current_pt: 0,
            target_total_pt: 100,
            auto_base_multiplier: None,
            mission_support_pt_bonus: None,
            max_results: 1,
        };
        let skill = resolved[0].card.skill;
        let items = SelectedAreaItems {
            band: String::new(),
            attribute: String::new(),
            magazine: Magazine::Performance,
        };

        let (_, candidate) = candidate_from_pair_records(
            &request,
            100,
            SongMode::Mixed,
            SkillBucketKey::from_skill(skill),
            skill,
            &items,
            &AreaItemPercent::empty(),
            &resolved,
            &songs,
            &intervals,
            0,
            [0, 1, 2],
            0.0,
            33.0,
            0.0,
            &pair_records,
            None,
        )
        .unwrap()
        .expect("the disjoint pair after 32 conflicts must be found");

        assert_eq!(candidate.team.card_ids, [1, 2, 3, 36, 37]);
    }

    #[test]
    fn batched_mitm_matches_exhaustive_five_card_search() {
        let mut cards = (1..=7)
            .map(|id| {
                fixture_card(
                    id,
                    if id % 3 == 0 { 2 } else { 1 },
                    if id % 2 == 0 {
                        Attribute::Pure
                    } else {
                        Attribute::Cool
                    },
                )
            })
            .collect::<Vec<_>>();
        let mut same_character_variant = fixture_card(8, 1, Attribute::Cool);
        same_character_variant.character_id = cards[0].character_id;
        same_character_variant.stat = StatValue {
            performance: cards[0].stat.performance,
            technique: cards[0].stat.visual,
            visual: cards[0].stat.technique,
        };
        cards.push(same_character_variant);
        let area_item_percent = fixture_area_item_percent();
        let items = super::super::score_range_item_combinations(&area_item_percent);
        let point_bonus_micros = cards
            .iter()
            .map(|card| (card.card_id, (card.character_id % 4) as u64 * 10_000_000))
            .collect::<BTreeMap<_, _>>();
        let song = fixture_song();
        let songs = vec![song.clone()];
        let sample_cards = [&cards[0], &cards[1], &cards[2], &cards[3], &cards[4]];
        let sample_items = &items[0];
        let sample_stat = sample_cards
            .iter()
            .map(|card| {
                card.add_up_stat(
                    &area_item_percent,
                    &sample_items.band,
                    &sample_items.attribute,
                    sample_items.magazine.as_str(),
                )
            })
            .sum::<f64>()
            .floor() as i32;
        let sample_bonus = point_bonus_basis_points(
            sample_cards
                .iter()
                .map(|card| point_bonus_micros[&card.card_id]),
        );
        let sample_score = song
            .compressed_score(cards[0].skill)
            .unwrap()
            .score(sample_stat);
        let target =
            points_for_score_with_support(EventType::LiveTry, sample_score, sample_bonus, 1, 0)
                .unwrap();
        let request = ScoreRangeRequest {
            event_type: EventType::LiveTry,
            current_pt: 0,
            target_total_pt: target,
            auto_base_multiplier: None,
            mission_support_pt_bonus: None,
            max_results: 10_000,
        };
        let domain = super::super::prepare_score_range_team_domain(
            &cards,
            &area_item_percent,
            &items,
            &point_bonus_micros,
        );

        let actual = search_raw_domain(&request, &domain, &songs, target)
            .unwrap()
            .into_iter()
            .map(|(team, plan)| {
                let song_key = SongKey {
                    song_id: plan[0].song_id,
                    difficulty: plan[0].difficulty,
                };
                (
                    (
                        SkillBucketKey::from_skill(team.skill),
                        team.point_bonus_basis_points,
                        team.stat,
                        song_key,
                    ),
                    Candidate { team, plan },
                )
            })
            .collect::<CandidateMap>();
        let mut expected = brute_force_candidates(
            &request,
            target,
            &cards,
            &area_item_percent,
            &items,
            &point_bonus_micros,
            &song,
        );
        let best_objective = expected
            .values()
            .map(|candidate| plan_objective(&candidate.plan))
            .min()
            .unwrap();
        expected.retain(|_, candidate| plan_objective(&candidate.plan) == best_objective);

        assert!(!expected.is_empty());
        assert!(expected
            .values()
            .any(|candidate| candidate.team.card_ids.contains(&8)));
        assert_eq!(actual.len(), expected.len());
        for (key, expected_candidate) in expected {
            let actual_candidate = actual.get(&key).expect("MITM missed a brute-force state");
            assert_eq!(actual_candidate.team, expected_candidate.team);
            assert_eq!(actual_candidate.plan, expected_candidate.plan);
        }

        let impossible_request = ScoreRangeRequest {
            target_total_pt: 1,
            ..request.clone()
        };
        assert!(search_raw_domain(&impossible_request, &domain, &songs, 1)
            .unwrap()
            .is_empty());
        assert!(brute_force_candidates(
            &impossible_request,
            1,
            &cards,
            &area_item_percent,
            &items,
            &point_bonus_micros,
            &song,
        )
        .is_empty());
    }

    #[allow(clippy::too_many_arguments)]
    fn brute_force_candidates(
        request: &ScoreRangeRequest,
        target: u64,
        cards: &[PreparedCard],
        area_item_percent: &AreaItemPercent,
        items: &[SelectedAreaItems],
        point_bonus_micros: &BTreeMap<u32, u64>,
        song: &ScoreRangeSong,
    ) -> CandidateMap {
        let mut result = CandidateMap::new();
        for a in 0..cards.len() {
            for b in (a + 1)..cards.len() {
                for c in (b + 1)..cards.len() {
                    for d in (c + 1)..cards.len() {
                        for e in (d + 1)..cards.len() {
                            let indices = [a, b, c, d, e];
                            if indices
                                .iter()
                                .map(|&index| cards[index].character_id)
                                .collect::<BTreeSet<_>>()
                                .len()
                                != 5
                            {
                                continue;
                            }
                            let mode = mode_for_five_cards(indices, cards);
                            let skills =
                                indices.map(|index| mode.resolve_skill(&cards[index]).unwrap());
                            if skills[1..].iter().any(|skill| {
                                SkillBucketKey::from_skill(*skill)
                                    != SkillBucketKey::from_skill(skills[0])
                            }) {
                                continue;
                            }
                            let score_model = song.compressed_score(skills[0]).unwrap();
                            for selected_items in items {
                                let stat = indices
                                    .iter()
                                    .map(|&index| {
                                        cards[index].add_up_stat(
                                            area_item_percent,
                                            &selected_items.band,
                                            &selected_items.attribute,
                                            selected_items.magazine.as_str(),
                                        )
                                    })
                                    .sum::<f64>()
                                    .floor() as i32;
                                let bonus = point_bonus_basis_points(
                                    indices
                                        .iter()
                                        .map(|&index| point_bonus_micros[&cards[index].card_id]),
                                );
                                let score = score_model.score(stat);
                                let base_points = points_for_score_with_support(
                                    request.event_type,
                                    score,
                                    bonus,
                                    1,
                                    request.mission_support_pt_bonus.unwrap_or_default(),
                                )
                                .unwrap();
                                let Some(plan) =
                                    one_song_plan(song.key(), score, base_points, target)
                                else {
                                    continue;
                                };
                                let mut card_ids = indices.map(|index| cards[index].card_id);
                                card_ids.sort_unstable();
                                let mut skill = skills[0];
                                skill.card_id = card_ids[0];
                                let team = ScoreRangeTeam {
                                    card_ids,
                                    stat,
                                    skill,
                                    point_bonus_basis_points: bonus,
                                    items: selected_items.clone(),
                                    recovery_mode: None,
                                };
                                let key =
                                    (SkillBucketKey::from_skill(skill), bonus, stat, song.key());
                                insert_candidate(&mut result, key, Candidate { team, plan });
                            }
                        }
                    }
                }
            }
        }
        result
    }

    fn mode_for_five_cards(indices: [usize; 5], cards: &[PreparedCard]) -> SongMode {
        let band = cards[indices[0]].band_id;
        let attribute = cards[indices[0]].attribute;
        let same_band = indices.iter().all(|&index| cards[index].band_id == band);
        let same_attribute = indices
            .iter()
            .all(|&index| cards[index].attribute == attribute);
        match (same_band, same_attribute) {
            (true, true) => SongMode::UnifiedBandAttribute(band, attribute),
            (true, false) => SongMode::UnifiedBand(band),
            (false, true) => SongMode::UnifiedAttribute(attribute),
            (false, false) => SongMode::Mixed,
        }
    }

    fn point_bonus_basis_points(values: impl IntoIterator<Item = u64>) -> u32 {
        let micros = values.into_iter().sum::<u64>();
        ((micros + 5_000) / 10_000).min(u32::MAX as u64) as u32
    }

    fn fixture_card(card_id: u32, band_id: u32, attribute: Attribute) -> PreparedCard {
        PreparedCard {
            card_id,
            character_id: card_id,
            band_id,
            rarity: 4,
            attribute,
            level: 60,
            training: true,
            illust_training_status: true,
            episodes: [true, true],
            limit_break_rank: 0,
            skill_level: 1,
            stat: StatValue {
                performance: 900.0 + card_id as f64 * 73.0,
                technique: 800.0 + card_id as f64 * 47.0,
                visual: 700.0 + card_id as f64 * 31.0,
            },
            event_add_stat: StatValue::zero(),
            skill: TeamCardSkill {
                card_id,
                duration: 3.0,
                score_up: 0.5,
                rateup: false,
            },
            score_up: ScoreUp {
                default: 0.5,
                unification_activate_effect_value: None,
                unification_activate_condition_band_id: None,
                unification_activate_condition_type: None,
            },
        }
    }

    fn fixture_area_item_percent() -> AreaItemPercent {
        let uniform = |value| StatRate {
            performance: value,
            technique: value,
            visual: value,
        };
        AreaItemPercent {
            band: BTreeMap::from([
                ("1".to_owned(), uniform(0.10)),
                ("2".to_owned(), uniform(0.10)),
                (ALL_BAND_KEY.to_owned(), uniform(0.03)),
            ]),
            attribute: BTreeMap::from([
                (
                    "cool".to_owned(),
                    StatRate {
                        performance: 0.12,
                        technique: 0.04,
                        visual: 0.08,
                    },
                ),
                (
                    "pure".to_owned(),
                    StatRate {
                        performance: 0.12,
                        technique: 0.04,
                        visual: 0.08,
                    },
                ),
                (ALL_ATTRIBUTE_KEY.to_owned(), uniform(0.02)),
            ]),
            magazine: BTreeMap::from([(
                "performance".to_owned(),
                StatRate {
                    performance: 0.04,
                    technique: 0.0,
                    visual: 0.0,
                },
            )]),
        }
    }

    fn fixture_song() -> ScoreRangeSong {
        let mut nodes = Vec::new();
        for activation in 0..6 {
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: activation as f64 * 12.0,
            });
            for offset in [1.0, 2.0, 4.0] {
                nodes.push(ChartNode {
                    node_type: ChartNodeType::Node,
                    time: activation as f64 * 12.0 + offset,
                });
            }
        }
        ScoreRangeSong::new(
            SongSelection {
                song_id: 99,
                difficulty: 3,
            },
            Chart::new(20, nodes),
        )
        .unwrap()
    }

    struct CompressedAutoScoreTest;

    impl CompressedAutoScoreTest {
        fn linear() -> CompressedAutoScore {
            let mut chart = crate::Chart::new(
                5,
                (0..6)
                    .map(|index| crate::ChartNode {
                        node_type: crate::ChartNodeType::Skill,
                        time: index as f64 * 10.0,
                    })
                    .collect(),
            );
            chart.init_auto_with_base_multiplier(2.0 / 3.0).unwrap();
            chart
                .compressed_auto_score(TeamCardSkill {
                    card_id: 1,
                    duration: 0.0,
                    score_up: 0.0,
                    rateup: false,
                })
                .unwrap()
        }
    }
}
