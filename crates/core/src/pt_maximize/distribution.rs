use std::collections::{BTreeMap, HashMap};

use crate::event_pt::{
    challenge_cp_gain, challenge_cp_points, cooperative_points, festival_multiplayer_points,
    solo_points, versus_multiplayer_points,
};
use crate::model::chart::{ExactScoreScratch, ExactSkillWindow, IndependentSkillScoreMatrix};
use crate::{Chart, DpChartModel, TeamCardSkill};

use super::model::{
    supports_live_variant, AveragePt, CaptainScoreDistribution, CooperativeLeaderSelection,
    CooperativePtScenario, FixedTeamPtEvaluation, FixedTeamPtScenario, LiveVariant,
    PtMaximizeError, ScoreHistogram, CHALLENGE_CP_COST, RANDOM_SKILL_ORDER_COUNT,
};

#[derive(Debug, Default)]
pub(crate) struct FullTeamScoreScratch {
    exact: ExactScoreScratch,
    subset_scores: [Vec<i32>; 32],
    score_entries: Vec<(i32, u64)>,
    skill_window_chart: usize,
    skill_windows: Vec<(SkillWindowKey, [ExactSkillWindow; 6])>,
    skill_meta_chart: usize,
    skill_metas: Vec<(SkillWindowKey, [f64; 6])>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CooperativeScoreKey {
    stat: i32,
    duration_bits: u64,
    score_up_bits: u64,
    rateup: bool,
}

#[derive(Debug, Default)]
pub(crate) struct CooperativeScoreScratch {
    source_chart: usize,
    fever_chart: Option<Chart>,
    exact: ExactScoreScratch,
    score_histograms: HashMap<CooperativeScoreKey, BTreeMap<(i32, i64), u64>>,
    score_cache_hits: u64,
    score_cache_misses: u64,
}

impl CooperativeScoreScratch {
    pub(crate) fn score_cache_counts(&self) -> (u64, u64) {
        (self.score_cache_hits, self.score_cache_misses)
    }
}

#[derive(Debug, Default)]
pub(crate) struct SinglePtScoreScratch {
    pub(crate) full_team: FullTeamScoreScratch,
    pub(crate) cooperative: CooperativeScoreScratch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SkillWindowKey {
    duration_bits: u64,
    score_up_bits: u64,
    rateup: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FullTeamPtSummary {
    pub(crate) captain_index: usize,
    pub(crate) captain_card_id: u32,
    pub(crate) average_pt: AveragePt,
    event_type: crate::EventType,
    live_variant: LiveVariant,
    pub(crate) score_sum: i64,
    min_score: i32,
    max_score: i32,
    pub(crate) sample_count: u64,
    min_pt: u64,
    max_pt: u64,
    average_cp_gain: Option<AveragePt>,
    challenge_cp_cost: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FullTeamPtCutoff {
    pub(crate) average_pt: AveragePt,
    pub(crate) equal_can_win: bool,
}

#[derive(Debug)]
pub(crate) enum FullTeamPtSummaryOutcome {
    Summary(FullTeamPtSummary),
    Queued,
    PrunedByMetaUpperBound,
    PrunedByExactUpperBound,
}

pub fn evaluate_full_team(
    chart: &Chart,
    team: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
    scenario: FixedTeamPtScenario,
) -> Result<FixedTeamPtEvaluation, PtMaximizeError> {
    let mut scratch = FullTeamScoreScratch::default();
    evaluate_full_team_with_scratch(chart, team, stat, is_medley, scenario, &mut scratch)
}

pub(crate) fn evaluate_full_team_with_scratch(
    chart: &Chart,
    team: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
    scenario: FixedTeamPtScenario,
    scratch: &mut FullTeamScoreScratch,
) -> Result<FixedTeamPtEvaluation, PtMaximizeError> {
    let event_type = scenario.event_type();
    let live_variant = scenario.live_variant();
    if !supports_live_variant(event_type, live_variant) {
        return Err(PtMaximizeError::UnsupportedLiveVariant {
            event_type,
            live_variant,
        });
    }

    let fever_chart;
    let chart = if matches!(scenario, FixedTeamPtScenario::Festival { .. }) {
        fever_chart = {
            let mut chart = chart.clone();
            chart.init_with_fever(chart.combo, is_medley)?;
            chart
        };
        &fever_chart
    } else {
        chart
    };
    let distributions =
        full_team_score_distributions_with_scratch(chart, team, stat, is_medley, scratch)?;
    let mut best: Option<FixedTeamPtEvaluation> = None;
    for captain in distributions {
        let evaluation = evaluate_distribution(captain, scenario)?;
        if best
            .as_ref()
            .is_none_or(|current| evaluation.average_pt > current.average_pt)
        {
            best = Some(evaluation);
        }
    }
    best.ok_or(PtMaximizeError::EmptyDistribution)
}

pub(crate) fn evaluate_full_team_summary_with_scratch(
    chart: &Chart,
    team: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
    scenario: FixedTeamPtScenario,
    cutoff: Option<FullTeamPtCutoff>,
    scratch: &mut FullTeamScoreScratch,
) -> Result<FullTeamPtSummaryOutcome, PtMaximizeError> {
    let event_type = scenario.event_type();
    let live_variant = scenario.live_variant();
    if !supports_live_variant(event_type, live_variant) {
        return Err(PtMaximizeError::UnsupportedLiveVariant {
            event_type,
            live_variant,
        });
    }

    let fever_chart;
    let chart = if matches!(scenario, FixedTeamPtScenario::Festival { .. }) {
        fever_chart = {
            let mut chart = chart.clone();
            chart.init_with_fever(chart.combo, is_medley)?;
            chart
        };
        &fever_chart
    } else {
        chart
    };
    let skills_overlap = chart.team_skills_may_overlap(team)?;
    if skills_overlap {
        return Ok(FullTeamPtSummaryOutcome::Queued);
    }
    if let Some(cutoff) = cutoff {
        let meta_score_upper = meta_score_upper_bound_cached(chart, team, stat, scratch)?;
        let meta_pt_upper = points_for_scenario(meta_score_upper, scenario)?;
        let meta_average_upper = AveragePt::new(u128::from(meta_pt_upper), 1)?;
        if meta_average_upper < cutoff.average_pt
            || (meta_average_upper == cutoff.average_pt && !cutoff.equal_can_win)
        {
            return Ok(FullTeamPtSummaryOutcome::PrunedByMetaUpperBound);
        }
    }
    let Some(matrix) =
        independent_skill_score_matrix_cached(chart, team, stat, is_medley, scratch)?
    else {
        return Ok(FullTeamPtSummaryOutcome::Queued);
    };
    let captain_index = (0..5)
        .max_by_key(|&card_idx| (matrix.deltas[card_idx][5], std::cmp::Reverse(card_idx)))
        .expect("a team always has five cards");
    if let Some(cutoff) = cutoff {
        let max_score = independent_max_score(&matrix, captain_index);
        let max_pt = points_for_scenario(max_score, scenario)?;
        let max_average = AveragePt::new(u128::from(max_pt), 1)?;
        if max_average < cutoff.average_pt
            || (max_average == cutoff.average_pt && !cutoff.equal_can_win)
        {
            return Ok(FullTeamPtSummaryOutcome::PrunedByExactUpperBound);
        }
    }
    let score_summary = prepare_independent_score_histogram(&matrix, captain_index, scratch);
    let point_summary = summarize_points(&scratch.score_entries, scenario)?;
    Ok(FullTeamPtSummaryOutcome::Summary(FullTeamPtSummary {
        captain_index,
        captain_card_id: team[captain_index].card_id,
        average_pt: AveragePt::new(point_summary.pt_sum, score_summary.sample_count)?,
        event_type,
        live_variant,
        score_sum: score_summary.score_sum,
        min_score: score_summary.min_score,
        max_score: score_summary.max_score,
        sample_count: score_summary.sample_count,
        min_pt: point_summary.min_pt,
        max_pt: point_summary.max_pt,
        average_cp_gain: point_summary
            .cp_sum
            .map(|cp_sum| AveragePt::new(cp_sum, score_summary.sample_count))
            .transpose()?,
        challenge_cp_cost: (scenario == FixedTeamPtScenario::ChallengeCp)
            .then_some(CHALLENGE_CP_COST),
    }))
}

fn meta_score_upper_bound_cached(
    chart: &Chart,
    team: &[TeamCardSkill; 5],
    stat: i32,
    scratch: &mut FullTeamScoreScratch,
) -> Result<i32, PtMaximizeError> {
    let chart_address = chart as *const Chart as usize;
    if scratch.skill_meta_chart != chart_address {
        scratch.skill_meta_chart = chart_address;
        scratch.skill_metas.clear();
    }
    let model = DpChartModel::from_chart(chart);
    let mut meta = [[0.0; 6]; 5];
    for (card_index, skill) in team.iter().copied().enumerate() {
        let key = SkillWindowKey {
            duration_bits: skill.duration.to_bits(),
            score_up_bits: skill.score_up.to_bits(),
            rateup: skill.rateup,
        };
        meta[card_index] = if let Some((_, values)) = scratch
            .skill_metas
            .iter()
            .find(|(cached_key, _)| *cached_key == key)
        {
            *values
        } else {
            let mut values = [0.0; 6];
            for (activation, value) in values.iter_mut().enumerate() {
                *value = model.skill_term(chart, activation, skill)?.sb;
            }
            scratch.skill_metas.push((key, values));
            values
        };
    }

    let mut normal = [f64::NEG_INFINITY; 32];
    normal[0] = 0.0;
    for mask in 0usize..31 {
        let activation = mask.count_ones() as usize;
        for card_index in 0..5 {
            if mask & (1 << card_index) != 0 {
                continue;
            }
            let next_mask = mask | (1 << card_index);
            normal[next_mask] = normal[next_mask].max(normal[mask] + meta[card_index][activation]);
        }
    }
    let captain = meta.iter().map(|values| values[5]).fold(0.0_f64, f64::max);
    let continuous_upper = stat.max(0) as f64 * (chart.meta.no_skill + normal[31] + captain);
    Ok(continuous_upper
        .ceil()
        .clamp(i32::MIN as f64, i32::MAX as f64) as i32)
}

fn independent_max_score(matrix: &IndependentSkillScoreMatrix, captain_index: usize) -> i32 {
    let mut states = [i32::MIN; 32];
    states[0] = 0;
    for mask in 0usize..31 {
        let position = mask.count_ones() as usize;
        for card_index in 0..5 {
            if mask & (1 << card_index) != 0 {
                continue;
            }
            let next_mask = mask | (1 << card_index);
            states[next_mask] = states[next_mask]
                .max(states[mask].saturating_add(matrix.deltas[card_index][position]));
        }
    }
    matrix
        .base_score
        .saturating_add(matrix.deltas[captain_index][5])
        .saturating_add(states[31])
}

fn independent_skill_score_matrix_cached(
    chart: &Chart,
    team: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
    scratch: &mut FullTeamScoreScratch,
) -> Result<Option<IndependentSkillScoreMatrix>, PtMaximizeError> {
    if chart.team_skills_may_overlap(team)? {
        return Ok(None);
    }
    let chart_address = chart as *const Chart as usize;
    if scratch.skill_window_chart != chart_address {
        scratch.skill_window_chart = chart_address;
        scratch.skill_windows.clear();
    }
    let mut windows = [[ExactSkillWindow::default(); 6]; 5];
    for (card_index, skill) in team.iter().copied().enumerate() {
        let key = SkillWindowKey {
            duration_bits: skill.duration.to_bits(),
            score_up_bits: skill.score_up.to_bits(),
            rateup: skill.rateup,
        };
        windows[card_index] = if let Some((_, windows)) = scratch
            .skill_windows
            .iter()
            .find(|(cached_key, _)| *cached_key == key)
        {
            *windows
        } else {
            let windows = chart.compile_exact_skill_windows(skill)?;
            scratch.skill_windows.push((key, windows));
            windows
        };
    }
    Ok(Some(chart.independent_skill_score_matrix_from_windows(
        team,
        stat,
        is_medley,
        &windows,
        &mut scratch.exact,
    )?))
}

pub(crate) fn materialize_full_team_summary(
    summary: FullTeamPtSummary,
    scratch: &FullTeamScoreScratch,
) -> FixedTeamPtEvaluation {
    FixedTeamPtEvaluation {
        event_type: summary.event_type,
        live_variant: summary.live_variant,
        captain_index: summary.captain_index,
        captain_card_id: summary.captain_card_id,
        score_distribution: ScoreHistogram {
            entries: scratch.score_entries.clone(),
            score_sum: summary.score_sum,
            min_score: summary.min_score,
            max_score: summary.max_score,
            sample_count: summary.sample_count,
        },
        average_pt: summary.average_pt,
        min_pt: summary.min_pt,
        max_pt: summary.max_pt,
        average_cp_gain: summary.average_cp_gain,
        challenge_cp_cost: summary.challenge_cp_cost,
    }
}

pub fn evaluate_cooperative_team(
    chart: &Chart,
    team: &[TeamCardSkill; 5],
    stat: i32,
    scenario: CooperativePtScenario,
) -> Result<FixedTeamPtEvaluation, PtMaximizeError> {
    let mut scratch = CooperativeScoreScratch::default();
    evaluate_cooperative_team_with_scratch(chart, team, stat, scenario, &mut scratch)
}

pub(crate) fn evaluate_cooperative_team_with_scratch(
    chart: &Chart,
    team: &[TeamCardSkill; 5],
    stat: i32,
    scenario: CooperativePtScenario,
    scratch: &mut CooperativeScoreScratch,
) -> Result<FixedTeamPtEvaluation, PtMaximizeError> {
    let player_stats = prepare_cooperative_evaluation(chart, stat, scenario, scratch)?;
    let repeated_player_indices =
        cooperative_repeated_player_indices(scenario.leader_selection, player_stats)?;

    let mut best = None;
    for captain_index in 0..5 {
        let evaluation = evaluate_prepared_cooperative_captain(
            team[captain_index],
            captain_index,
            stat,
            scenario,
            player_stats,
            &repeated_player_indices,
            scratch,
        )?;
        if best.as_ref().is_none_or(|current: &FixedTeamPtEvaluation| {
            evaluation.average_pt > current.average_pt
        }) {
            best = Some(evaluation);
        }
    }
    best.ok_or(PtMaximizeError::EmptyDistribution)
}

pub(crate) fn evaluate_cooperative_captain_with_scratch(
    chart: &Chart,
    captain: TeamCardSkill,
    captain_index: usize,
    stat: i32,
    scenario: CooperativePtScenario,
    scratch: &mut CooperativeScoreScratch,
) -> Result<FixedTeamPtEvaluation, PtMaximizeError> {
    let player_stats = prepare_cooperative_evaluation(chart, stat, scenario, scratch)?;
    let repeated_player_indices =
        cooperative_repeated_player_indices(scenario.leader_selection, player_stats)?;
    evaluate_prepared_cooperative_captain(
        captain,
        captain_index,
        stat,
        scenario,
        player_stats,
        &repeated_player_indices,
        scratch,
    )
}

fn prepare_cooperative_evaluation(
    chart: &Chart,
    stat: i32,
    scenario: CooperativePtScenario,
    scratch: &mut CooperativeScoreScratch,
) -> Result<[i32; 5], PtMaximizeError> {
    if !supports_live_variant(scenario.event_type, LiveVariant::Cooperative) {
        return Err(PtMaximizeError::UnsupportedCooperativeEvent {
            event_type: scenario.event_type,
        });
    }

    let chart_address = chart as *const Chart as usize;
    if scratch.source_chart != chart_address {
        let mut fever_chart = chart.clone();
        fever_chart.init_with_fever(fever_chart.combo, false)?;
        scratch.source_chart = chart_address;
        scratch.fever_chart = Some(fever_chart);
        scratch.score_histograms.clear();
        scratch.exact = ExactScoreScratch::default();
    }
    Ok([
        stat,
        scenario.teammates[0].expected_stat,
        scenario.teammates[1].expected_stat,
        scenario.teammates[2].expected_stat,
        scenario.teammates[3].expected_stat,
    ])
}

#[allow(clippy::too_many_arguments)]
fn evaluate_prepared_cooperative_captain(
    captain: TeamCardSkill,
    captain_index: usize,
    stat: i32,
    scenario: CooperativePtScenario,
    player_stats: [i32; 5],
    repeated_player_indices: &[usize],
    scratch: &mut CooperativeScoreScratch,
) -> Result<FixedTeamPtEvaluation, PtMaximizeError> {
    let score_key = CooperativeScoreKey {
        stat,
        duration_bits: captain.duration.to_bits(),
        score_up_bits: captain.score_up.to_bits(),
        rateup: captain.rateup,
    };
    let player_skills = [
        captain,
        scenario.teammates[0].skill(1),
        scenario.teammates[1].skill(2),
        scenario.teammates[2].skill(3),
        scenario.teammates[3].skill(4),
    ];
    let chart = scratch
        .fever_chart
        .as_ref()
        .expect("cooperative scratch prepares a fever chart");
    if let Some(joint) = scratch.score_histograms.get(&score_key) {
        scratch.score_cache_hits += 1;
        return evaluate_cooperative_distribution(captain_index, captain.card_id, joint, scenario);
    } else {
        scratch.score_cache_misses += 1;
        let joint = cooperative_score_histogram(
            chart,
            &player_skills,
            player_stats,
            repeated_player_indices,
            &mut scratch.exact,
        )?;
        let evaluation =
            evaluate_cooperative_distribution(captain_index, captain.card_id, &joint, scenario)?;
        scratch.score_histograms.insert(score_key, joint);
        Ok(evaluation)
    }
}

fn cooperative_repeated_player_indices(
    selection: CooperativeLeaderSelection,
    player_stats: [i32; 5],
) -> Result<Vec<usize>, PtMaximizeError> {
    match selection {
        CooperativeLeaderSelection::MaxStat => {
            let max_stat = player_stats
                .iter()
                .copied()
                .max()
                .unwrap_or(player_stats[0]);
            Ok((0..5)
                .filter(|&player_idx| player_stats[player_idx] == max_stat)
                .collect())
        }
        CooperativeLeaderSelection::Specified { player_index } if player_index < 5 => {
            Ok(vec![usize::from(player_index)])
        }
        CooperativeLeaderSelection::Specified { player_index } => {
            Err(PtMaximizeError::InvalidCooperativeLeaderIndex {
                index: player_index,
            })
        }
        CooperativeLeaderSelection::Random => Ok((0..5).collect()),
    }
}

fn cooperative_score_histogram(
    chart: &Chart,
    skills: &[TeamCardSkill; 5],
    player_stats: [i32; 5],
    repeated_player_indices: &[usize],
    scratch: &mut ExactScoreScratch,
) -> Result<BTreeMap<(i32, i64), u64>, PtMaximizeError> {
    let mut matrices = Vec::with_capacity(5);
    for &player_stat in &player_stats {
        let Some(matrix) =
            chart.independent_skill_score_matrix(skills, player_stat, false, scratch)?
        else {
            return cooperative_score_histogram_queued(
                chart,
                skills,
                player_stats,
                repeated_player_indices,
            );
        };
        matrices.push(matrix);
    }

    let base_personal = matrices[0].base_score;
    let base_total = matrices
        .iter()
        .map(|matrix| i64::from(matrix.base_score))
        .sum::<i64>();
    let mut states: [BTreeMap<(i32, i64), u64>; 32] = std::array::from_fn(|_| BTreeMap::new());
    states[0].insert((0, 0), 1);
    for mask in 0usize..31 {
        let position = mask.count_ones() as usize;
        let partials = states[mask]
            .iter()
            .map(|(&score, &count)| (score, count))
            .collect::<Vec<_>>();
        for skill_idx in 0..5 {
            if mask & (1 << skill_idx) != 0 {
                continue;
            }
            let next_mask = mask | (1 << skill_idx);
            let personal_delta = matrices[0].deltas[skill_idx][position];
            let total_delta = matrices
                .iter()
                .map(|matrix| i64::from(matrix.deltas[skill_idx][position]))
                .sum::<i64>();
            for &((personal, total), count) in &partials {
                *states[next_mask]
                    .entry((personal + personal_delta, total + total_delta))
                    .or_insert(0) += count;
            }
        }
    }

    let mut result = BTreeMap::new();
    for &repeated_player_idx in repeated_player_indices {
        let personal_delta = matrices[0].deltas[repeated_player_idx][5];
        let total_delta = matrices
            .iter()
            .map(|matrix| i64::from(matrix.deltas[repeated_player_idx][5]))
            .sum::<i64>();
        for (&(personal, total), &count) in &states[31] {
            *result
                .entry((
                    base_personal + personal + personal_delta,
                    base_total + total + total_delta,
                ))
                .or_insert(0) += count;
        }
    }
    debug_assert_eq!(
        result.values().sum::<u64>(),
        RANDOM_SKILL_ORDER_COUNT * repeated_player_indices.len() as u64
    );
    Ok(result)
}

fn cooperative_score_histogram_queued(
    chart: &Chart,
    skills: &[TeamCardSkill; 5],
    player_stats: [i32; 5],
    repeated_player_indices: &[usize],
) -> Result<BTreeMap<(i32, i64), u64>, PtMaximizeError> {
    let mut result = BTreeMap::new();
    for_each_permutation([0, 1, 2, 3, 4], |order| {
        for &repeated_player_idx in repeated_player_indices {
            let skill_order = [
                skills[order[0]],
                skills[order[1]],
                skills[order[2]],
                skills[order[3]],
                skills[order[4]],
                skills[repeated_player_idx],
            ];
            let mut scores = [0i32; 5];
            for player_idx in 0..5 {
                scores[player_idx] = chart.get_score_for_six_skills(
                    &skill_order,
                    player_stats[player_idx],
                    false,
                )?;
            }
            *result
                .entry((
                    scores[0],
                    scores.iter().map(|&score| i64::from(score)).sum(),
                ))
                .or_insert(0) += 1;
        }
        Ok::<(), PtMaximizeError>(())
    })?;
    Ok(result)
}

fn evaluate_cooperative_distribution(
    captain_index: usize,
    captain_card_id: u32,
    joint: &BTreeMap<(i32, i64), u64>,
    scenario: CooperativePtScenario,
) -> Result<FixedTeamPtEvaluation, PtMaximizeError> {
    let mut personal_histogram = BTreeMap::new();
    let mut pt_sum = 0u128;
    let mut cp_sum = 0u128;
    let mut min_pt = u64::MAX;
    let mut max_pt = 0u64;
    for (&(personal_score, total_score), &count) in joint {
        *personal_histogram.entry(personal_score).or_insert(0) += count;
        let pt = cooperative_points(
            scenario.event_type,
            personal_score,
            total_score,
            scenario.point_bonus_basis_points,
            scenario.mission_support_pt_bonus,
        )?;
        pt_sum += u128::from(pt) * u128::from(count);
        if scenario.event_type == crate::EventType::Challenge {
            cp_sum += u128::from(challenge_cp_gain(pt)) * u128::from(count);
        }
        min_pt = min_pt.min(pt);
        max_pt = max_pt.max(pt);
    }
    let score_distribution = score_histogram(personal_histogram);
    let sample_count = score_distribution.sample_count;
    Ok(FixedTeamPtEvaluation {
        event_type: scenario.event_type,
        live_variant: LiveVariant::Cooperative,
        captain_index,
        captain_card_id,
        score_distribution,
        average_pt: AveragePt::new(pt_sum, sample_count)?,
        min_pt,
        max_pt,
        average_cp_gain: (scenario.event_type == crate::EventType::Challenge)
            .then(|| AveragePt::new(cp_sum, sample_count))
            .transpose()?,
        challenge_cp_cost: None,
    })
}

pub(crate) fn full_team_score_distributions(
    chart: &Chart,
    team: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
) -> Result<Vec<CaptainScoreDistribution>, PtMaximizeError> {
    let mut scratch = FullTeamScoreScratch::default();
    full_team_score_distributions_with_scratch(chart, team, stat, is_medley, &mut scratch)
}

fn full_team_score_distributions_with_scratch(
    chart: &Chart,
    team: &[TeamCardSkill; 5],
    stat: i32,
    is_medley: bool,
    scratch: &mut FullTeamScoreScratch,
) -> Result<Vec<CaptainScoreDistribution>, PtMaximizeError> {
    if let Some(matrix) =
        chart.independent_skill_score_matrix(team, stat, is_medley, &mut scratch.exact)?
    {
        let captain_index = (0..5)
            .max_by_key(|&card_idx| (matrix.deltas[card_idx][5], std::cmp::Reverse(card_idx)))
            .expect("a team always has five cards");
        return Ok(vec![CaptainScoreDistribution {
            captain_index,
            captain_card_id: team[captain_index].card_id,
            distribution: independent_score_histogram(&matrix, captain_index, scratch),
        }]);
    }

    let mut result = Vec::with_capacity(5);
    for captain_index in 0..5 {
        let mut histogram = BTreeMap::new();
        for_each_permutation([0, 1, 2, 3, 4], |order| {
            let skills = [
                team[order[0]],
                team[order[1]],
                team[order[2]],
                team[order[3]],
                team[order[4]],
                team[captain_index],
            ];
            let score = chart.get_score_for_six_skills(&skills, stat, is_medley)?;
            *histogram.entry(score).or_insert(0) += 1;
            Ok::<(), PtMaximizeError>(())
        })?;
        result.push(CaptainScoreDistribution {
            captain_index,
            captain_card_id: team[captain_index].card_id,
            distribution: score_histogram(histogram),
        });
    }
    Ok(result)
}

fn independent_score_histogram(
    matrix: &IndependentSkillScoreMatrix,
    captain_index: usize,
    scratch: &mut FullTeamScoreScratch,
) -> ScoreHistogram {
    let summary = prepare_independent_score_histogram(matrix, captain_index, scratch);
    ScoreHistogram {
        entries: scratch.score_entries.clone(),
        score_sum: summary.score_sum,
        min_score: summary.min_score,
        max_score: summary.max_score,
        sample_count: summary.sample_count,
    }
}

#[derive(Debug, Clone, Copy)]
struct ScoreSummary {
    score_sum: i64,
    min_score: i32,
    max_score: i32,
    sample_count: u64,
}

fn prepare_independent_score_histogram(
    matrix: &IndependentSkillScoreMatrix,
    captain_index: usize,
    scratch: &mut FullTeamScoreScratch,
) -> ScoreSummary {
    for state in &mut scratch.subset_scores {
        state.clear();
    }
    scratch.subset_scores[0].push(0);
    for mask in 0usize..31 {
        let position = mask.count_ones() as usize;
        for card_idx in 0..5 {
            if mask & (1 << card_idx) != 0 {
                continue;
            }
            let next_mask = mask | (1 << card_idx);
            let delta = matrix.deltas[card_idx][position];
            debug_assert!(next_mask > mask);
            let (completed, pending) = scratch.subset_scores.split_at_mut(next_mask);
            let partials = &completed[mask];
            let next = &mut pending[0];
            for &sum in partials {
                next.push(sum + delta);
            }
        }
    }

    let fixed_score = matrix.base_score + matrix.deltas[captain_index][5];
    let scores = &mut scratch.subset_scores[31];
    scores.sort_unstable();
    scratch.score_entries.clear();
    scratch.score_entries.reserve(scores.len());
    for &delta in scores.iter() {
        let score = fixed_score + delta;
        match scratch.score_entries.last_mut() {
            Some((last_score, count)) if *last_score == score => *count += 1,
            _ => scratch.score_entries.push((score, 1)),
        }
    }
    let sample_count = scores.len() as u64;
    let score_sum = scratch
        .score_entries
        .iter()
        .map(|&(score, count)| i64::from(score) * count as i64)
        .sum();
    let summary = ScoreSummary {
        min_score: scratch.score_entries.first().map_or(0, |&(score, _)| score),
        max_score: scratch.score_entries.last().map_or(0, |&(score, _)| score),
        score_sum,
        sample_count,
    };
    debug_assert_eq!(summary.sample_count, RANDOM_SKILL_ORDER_COUNT);
    summary
}

fn score_histogram(entries: BTreeMap<i32, u64>) -> ScoreHistogram {
    let sample_count = entries.values().sum();
    let score_sum = entries
        .iter()
        .map(|(&score, &count)| i64::from(score) * count as i64)
        .sum();
    let min_score = entries.first_key_value().map_or(0, |(&score, _)| score);
    let max_score = entries.last_key_value().map_or(0, |(&score, _)| score);
    ScoreHistogram {
        entries: entries.into_iter().collect(),
        score_sum,
        min_score,
        max_score,
        sample_count,
    }
}

fn evaluate_distribution(
    captain: CaptainScoreDistribution,
    scenario: FixedTeamPtScenario,
) -> Result<FixedTeamPtEvaluation, PtMaximizeError> {
    let point_summary = summarize_points(&captain.distribution.entries, scenario)?;
    let sample_count = captain.distribution.sample_count;
    let average_pt = AveragePt::new(point_summary.pt_sum, sample_count)?;
    let average_cp_gain = point_summary
        .cp_sum
        .map(|cp_sum| AveragePt::new(cp_sum, sample_count))
        .transpose()?;
    Ok(FixedTeamPtEvaluation {
        event_type: scenario.event_type(),
        live_variant: scenario.live_variant(),
        captain_index: captain.captain_index,
        captain_card_id: captain.captain_card_id,
        score_distribution: captain.distribution,
        average_pt,
        min_pt: point_summary.min_pt,
        max_pt: point_summary.max_pt,
        average_cp_gain,
        challenge_cp_cost: (scenario == FixedTeamPtScenario::ChallengeCp)
            .then_some(CHALLENGE_CP_COST),
    })
}

#[derive(Debug, Clone, Copy)]
struct PointSummary {
    pt_sum: u128,
    cp_sum: Option<u128>,
    min_pt: u64,
    max_pt: u64,
}

fn summarize_points(
    entries: &[(i32, u64)],
    scenario: FixedTeamPtScenario,
) -> Result<PointSummary, PtMaximizeError> {
    let awards_cp = matches!(
        scenario,
        FixedTeamPtScenario::Solo {
            event_type: crate::EventType::Challenge,
            ..
        }
    );
    let mut pt_sum = 0u128;
    let mut cp_sum = awards_cp.then_some(0u128);
    let mut min_pt = u64::MAX;
    let mut max_pt = 0u64;
    for &(score, count) in entries {
        let pt = points_for_scenario(score, scenario)?;
        pt_sum += u128::from(pt) * u128::from(count);
        if let Some(cp_sum) = &mut cp_sum {
            *cp_sum += u128::from(challenge_cp_gain(pt)) * u128::from(count);
        }
        min_pt = min_pt.min(pt);
        max_pt = max_pt.max(pt);
    }
    Ok(PointSummary {
        pt_sum,
        cp_sum,
        min_pt,
        max_pt,
    })
}

pub(crate) fn points_for_scenario(
    score: i32,
    scenario: FixedTeamPtScenario,
) -> Result<u64, PtMaximizeError> {
    Ok(match scenario {
        FixedTeamPtScenario::Solo {
            event_type,
            point_bonus_basis_points,
            mission_support_pt_bonus,
        } => solo_points(
            event_type,
            score,
            point_bonus_basis_points,
            mission_support_pt_bonus,
        )?,
        FixedTeamPtScenario::Versus { team_rank } => versus_multiplayer_points(score, team_rank)?,
        FixedTeamPtScenario::Festival {
            other_players_score,
            team_rank,
            won,
        } => festival_multiplayer_points(
            i64::from(score.max(0))
                .saturating_add(other_players_score.max(0))
                .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            team_rank,
            won,
        )?,
        FixedTeamPtScenario::ChallengeCp => challenge_cp_points(score),
    })
}

fn for_each_permutation<E>(
    mut values: [usize; 5],
    mut visit: impl FnMut([usize; 5]) -> Result<(), E>,
) -> Result<(), E> {
    fn recurse<E>(
        values: &mut [usize; 5],
        start: usize,
        visit: &mut impl FnMut([usize; 5]) -> Result<(), E>,
    ) -> Result<(), E> {
        if start == values.len() {
            return visit(*values);
        }
        for idx in start..values.len() {
            values.swap(start, idx);
            recurse(values, start + 1, visit)?;
            values.swap(start, idx);
        }
        Ok(())
    }
    recurse(&mut values, 0, &mut visit)
}

#[cfg(test)]
mod tests {
    use super::super::model::CooperativeTeammate;
    use super::*;
    use crate::{ChartNode, ChartNodeType, EventType};

    #[test]
    fn subset_dp_matches_all_one_hundred_twenty_permutations() {
        let matrix = IndependentSkillScoreMatrix {
            base_score: 1_000,
            deltas: [
                [11, 12, 13, 14, 15, 100],
                [21, 22, 23, 24, 25, 90],
                [31, 32, 33, 34, 35, 80],
                [41, 42, 43, 44, 45, 70],
                [51, 52, 53, 54, 55, 60],
            ],
        };
        let actual = independent_score_histogram(&matrix, 0, &mut FullTeamScoreScratch::default());
        let mut expected = BTreeMap::new();
        for_each_permutation([0, 1, 2, 3, 4], |order| {
            let score = matrix.base_score
                + matrix.deltas[0][5]
                + order
                    .iter()
                    .enumerate()
                    .map(|(position, &card)| matrix.deltas[card][position])
                    .sum::<i32>();
            *expected.entry(score).or_insert(0) += 1;
            Ok::<(), ()>(())
        })
        .unwrap();
        assert_eq!(actual.entries, expected.into_iter().collect::<Vec<_>>());
        assert_eq!(actual.sample_count, 120);
        assert_eq!(independent_max_score(&matrix, 0), actual.max_score);
    }

    #[test]
    fn direct_mean_formula_matches_histogram_sum() {
        let matrix = IndependentSkillScoreMatrix {
            base_score: 50_000,
            deltas: std::array::from_fn(|card| {
                std::array::from_fn(|position| (card * 100 + position * 7) as i32)
            }),
        };
        let captain = 4;
        let histogram =
            independent_score_histogram(&matrix, captain, &mut FullTeamScoreScratch::default());
        let direct = 120i64 * i64::from(matrix.base_score)
            + 24 * matrix
                .deltas
                .iter()
                .map(|row| row[..5].iter().map(|&value| i64::from(value)).sum::<i64>())
                .sum::<i64>()
            + 120 * i64::from(matrix.deltas[captain][5]);
        assert_eq!(histogram.score_sum, direct);
    }

    #[test]
    fn real_nonqueued_chart_matrix_matches_strict_scorer() {
        let mut nodes = Vec::new();
        for activation in 0..6 {
            let start = activation as f64 * 10.0;
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: start,
            });
            for offset in 1..=4 {
                nodes.push(ChartNode {
                    node_type: ChartNodeType::Node,
                    time: start + offset as f64 * 0.5,
                });
            }
        }
        let mut chart = Chart::new(25, nodes);
        chart.init(0, false).unwrap();
        let team = std::array::from_fn(|idx| TeamCardSkill {
            card_id: idx as u32 + 1,
            duration: 3.0,
            score_up: 0.5 + idx as f64 * 0.1,
            rateup: false,
        });
        let distributions = full_team_score_distributions(&chart, &team, 250_000, false).unwrap();
        assert_eq!(distributions.len(), 1);
        let actual = &distributions[0];

        let mut expected = BTreeMap::new();
        for_each_permutation([0, 1, 2, 3, 4], |order| {
            let skills = [
                team[order[0]],
                team[order[1]],
                team[order[2]],
                team[order[3]],
                team[order[4]],
                team[actual.captain_index],
            ];
            let score = chart
                .get_score_for_six_skills(&skills, 250_000, false)
                .unwrap();
            *expected.entry(score).or_insert(0) += 1;
            Ok::<(), ()>(())
        })
        .unwrap();
        assert_eq!(
            actual.distribution.entries,
            expected.into_iter().collect::<Vec<_>>()
        );
        let meta_upper = meta_score_upper_bound_cached(
            &chart,
            &team,
            250_000,
            &mut FullTeamScoreScratch::default(),
        )
        .unwrap();
        assert!(meta_upper >= actual.distribution.max_score);
    }

    #[test]
    fn averages_integer_points_instead_of_average_score() {
        let distribution = CaptainScoreDistribution {
            captain_index: 0,
            captain_card_id: 1,
            distribution: score_histogram(BTreeMap::from([(9_749, 1), (9_751, 1)])),
        };
        let result = evaluate_distribution(
            distribution,
            FixedTeamPtScenario::Solo {
                event_type: EventType::Versus,
                point_bonus_basis_points: 0,
                mission_support_pt_bonus: 0,
            },
        )
        .unwrap();
        assert_eq!(result.average_pt, AveragePt::new(201, 2).unwrap());
        assert_eq!(result.min_pt, 100);
        assert_eq!(result.max_pt, 101);
    }

    #[test]
    fn cooperative_distribution_expands_tied_sixth_skill_once_per_tied_player() {
        let mut nodes = Vec::new();
        for activation in 0..6 {
            let start = activation as f64 * 10.0;
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: start,
            });
            nodes.push(ChartNode {
                node_type: ChartNodeType::Node,
                time: start + 1.0,
            });
        }
        let mut chart = Chart::new_with_fever_section(25, nodes, Some(35.0), Some(55.0));
        chart.init(0, false).unwrap();
        let team = std::array::from_fn(|idx| TeamCardSkill {
            card_id: idx as u32 + 1,
            duration: 3.0,
            score_up: 0.6 + idx as f64 * 0.1,
            rateup: false,
        });
        let scenario = CooperativePtScenario {
            event_type: EventType::Challenge,
            teammates: [
                CooperativeTeammate {
                    expected_stat: 300_000,
                    leader_score_up: 1.0,
                    leader_skill_duration: 3.0,
                },
                CooperativeTeammate {
                    expected_stat: 250_000,
                    leader_score_up: 0.9,
                    leader_skill_duration: 3.0,
                },
                CooperativeTeammate {
                    expected_stat: 240_000,
                    leader_score_up: 0.8,
                    leader_skill_duration: 3.0,
                },
                CooperativeTeammate {
                    expected_stat: 230_000,
                    leader_score_up: 0.7,
                    leader_skill_duration: 3.0,
                },
            ],
            leader_selection: CooperativeLeaderSelection::MaxStat,
            point_bonus_basis_points: 0,
            mission_support_pt_bonus: 0,
        };

        let result = evaluate_cooperative_team(&chart, &team, 300_000, scenario).unwrap();
        assert_eq!(result.live_variant, LiveVariant::Cooperative);
        assert_eq!(result.score_distribution.sample_count, 240);
        assert!(result.average_cp_gain.is_some());
        assert_eq!(result.challenge_cp_cost, None);
    }

    #[test]
    fn cooperative_distribution_supports_specified_and_random_sixth_skill() {
        let nodes = (0..6)
            .flat_map(|activation| {
                let start = activation as f64 * 10.0;
                [
                    ChartNode {
                        node_type: ChartNodeType::Skill,
                        time: start,
                    },
                    ChartNode {
                        node_type: ChartNodeType::Node,
                        time: start + 1.0,
                    },
                ]
            })
            .collect();
        let mut chart = Chart::new_with_fever_section(25, nodes, Some(35.0), Some(55.0));
        chart.init(0, false).unwrap();
        let team = std::array::from_fn(|idx| TeamCardSkill {
            card_id: idx as u32 + 1,
            duration: 3.0,
            score_up: 0.6 + idx as f64 * 0.1,
            rateup: false,
        });
        let base_scenario = CooperativePtScenario {
            event_type: EventType::Challenge,
            teammates: std::array::from_fn(|index| CooperativeTeammate {
                expected_stat: 250_000 - index as i32 * 10_000,
                leader_score_up: 1.0 - index as f64 * 0.1,
                leader_skill_duration: 3.0,
            }),
            leader_selection: CooperativeLeaderSelection::Specified { player_index: 3 },
            point_bonus_basis_points: 0,
            mission_support_pt_bonus: 0,
        };

        let specified = evaluate_cooperative_team(&chart, &team, 300_000, base_scenario).unwrap();
        assert_eq!(specified.score_distribution.sample_count, 120);

        let random = evaluate_cooperative_team(
            &chart,
            &team,
            300_000,
            CooperativePtScenario {
                leader_selection: CooperativeLeaderSelection::Random,
                ..base_scenario
            },
        )
        .unwrap();
        assert_eq!(random.score_distribution.sample_count, 600);
    }

    #[test]
    fn cooperative_score_cache_is_shared_across_point_bonuses() {
        let nodes = (0..6)
            .flat_map(|activation| {
                let start = activation as f64 * 10.0;
                [
                    ChartNode {
                        node_type: ChartNodeType::Skill,
                        time: start,
                    },
                    ChartNode {
                        node_type: ChartNodeType::Node,
                        time: start + 1.0,
                    },
                ]
            })
            .collect();
        let mut chart = Chart::new_with_fever_section(25, nodes, Some(35.0), Some(55.0));
        chart.init(0, false).unwrap();
        let captain = TeamCardSkill {
            card_id: 1,
            duration: 3.0,
            score_up: 1.3,
            rateup: false,
        };
        let base_scenario = CooperativePtScenario {
            event_type: EventType::MissionLive,
            teammates: std::array::from_fn(|_| CooperativeTeammate {
                expected_stat: 290_000,
                leader_score_up: 1.3,
                leader_skill_duration: 3.0,
            }),
            leader_selection: CooperativeLeaderSelection::MaxStat,
            point_bonus_basis_points: 0,
            mission_support_pt_bonus: 100,
        };
        let mut scratch = CooperativeScoreScratch::default();

        let without_bonus = evaluate_cooperative_captain_with_scratch(
            &chart,
            captain,
            0,
            300_000,
            base_scenario,
            &mut scratch,
        )
        .unwrap();
        let with_bonus = evaluate_cooperative_captain_with_scratch(
            &chart,
            captain,
            0,
            300_000,
            CooperativePtScenario {
                point_bonus_basis_points: 1_000,
                ..base_scenario
            },
            &mut scratch,
        )
        .unwrap();

        assert_eq!(scratch.score_cache_counts(), (1, 1));
        assert_eq!(
            without_bonus.score_distribution,
            with_bonus.score_distribution
        );
        assert!(with_bonus.average_pt > without_bonus.average_pt);
    }
}
