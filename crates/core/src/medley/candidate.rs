use crate::medley::enumeration::{raw_candidate_solver_input_for_indices, RawCandidateSolverInput};
use crate::medley::error::BuildError;
use crate::medley::scoring::RawTeamCandidate;
use crate::model::preparation::PreparedCard;
use crate::model::schema::{
    BuildResult, CalculationMetrics, EventType, MedleyCalculationMetrics, SelectedAreaItems,
    SongBuildResult, SongSelection,
};
use crate::timing::Timer;
use bangdream_optimize_medley_solver::{
    solve_medley_wide_with, solve_medley_with, MedleySolverImplementation, MedleySolverInput,
    MedleySolverPreference, WideMedleySolverInput,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateBuildRequest {
    pub event_id: u32,
    pub event_type: EventType,
    pub song_list: Vec<SongSelection>,
    pub candidates: Vec<TeamCandidate>,
    #[serde(default)]
    pub current_best: i32,
    #[serde(default)]
    pub solver_preference: Option<MedleySolverPreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<SelectedAreaItems>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamCandidate {
    pub mask: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mask_words: Vec<u64>,
    pub team_card_ids: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordered_team_card_ids: Option<Vec<Vec<u32>>>,
    pub captain_card_ids: Vec<u32>,
    pub scores: Vec<i32>,
    pub stat: i32,
}

pub(crate) struct RawCandidateBuildRequest<'a> {
    pub(crate) event_id: u32,
    pub(crate) song_list: &'a [SongSelection],
    pub(crate) candidates: &'a [RawTeamCandidate],
    pub(crate) cards: &'a [PreparedCard],
    pub(crate) current_best: i32,
    pub(crate) solver_preference: Option<MedleySolverPreference>,
    pub(crate) items: Option<SelectedAreaItems>,
}

pub fn calculate_from_candidates(
    request: CandidateBuildRequest,
) -> Result<BuildResult, BuildError> {
    if matches!(
        request.event_type,
        EventType::Festival | EventType::LiveTry | EventType::MissionLive
    ) {
        return Err(BuildError::UnsupportedEventType {
            event_type: request.event_type.as_str().to_owned(),
        });
    }
    validate_candidates(&request)?;

    match request.event_type {
        EventType::Medley => calculate_medley(request),
        EventType::Versus | EventType::Challenge => calculate_single_team(request),
        EventType::Festival | EventType::LiveTry | EventType::MissionLive => unreachable!(),
    }
}

pub(crate) fn calculate_medley_from_raw_candidates(
    request: RawCandidateBuildRequest<'_>,
) -> Result<BuildResult, BuildError> {
    if request.candidates.is_empty() {
        return Err(BuildError::EmptyCandidates);
    }
    if request.song_list.len() != 3 {
        return Err(BuildError::InvalidMedleySongCount {
            count: request.song_list.len(),
        });
    }

    let trace = trace_enabled();
    let filter_start = Timer::start();
    let candidate_indices =
        raw_medley_solver_candidate_indices(request.candidates, request.current_best);
    let filter_ms = filter_start.elapsed_ms();
    if trace {
        eprintln!(
            "medley solver filter: candidates={} solver_candidates={} current_best={} filter_ms={:.3}",
            request.candidates.len(),
            candidate_indices.len(),
            request.current_best,
            filter_ms,
        );
    }

    let solve_start = Timer::start();
    let preference = request
        .solver_preference
        .unwrap_or(MedleySolverPreference::Auto);
    let (plan, used_card_count) = match raw_candidate_solver_input_for_indices(
        request.candidates,
        request.current_best,
        &candidate_indices,
    ) {
        RawCandidateSolverInput::Narrow {
            input,
            used_card_count,
        } => (solve_medley_with(&input, preference)?, used_card_count),
        RawCandidateSolverInput::Wide {
            input,
            used_card_count,
        } => (solve_medley_wide_with(&input, preference)?, used_card_count),
    };
    let solver_ms = solve_start.elapsed_ms();
    if trace {
        eprintln!(
            "medley solver core: implementation={} score={} used_cards={} solve_ms={:.3}",
            format_medley_solver(plan.implementation),
            plan.score,
            used_card_count,
            solver_ms,
        );
    }

    let mut total_stat = 0;
    let mut songs = Vec::with_capacity(3);
    for (song_idx, &solver_candidate_idx) in plan.indices.iter().enumerate() {
        let candidate_idx = candidate_indices[solver_candidate_idx];
        let candidate = &request.candidates[candidate_idx];
        total_stat += candidate.stat;
        songs.push(raw_song_result(
            &request.song_list[song_idx],
            candidate,
            request.cards,
            song_idx,
        ));
    }

    Ok(BuildResult {
        event_id: request.event_id,
        event_type: EventType::Medley,
        total_score: plan.score,
        total_stat,
        songs,
        items: request.items,
        solver: Some(format_medley_solver(plan.implementation).to_owned()),
        metrics: Some(CalculationMetrics {
            medley: Some(MedleyCalculationMetrics {
                candidate_count: request.candidates.len(),
                solver_candidate_count: candidate_indices.len(),
                solver_filter_ms: filter_ms,
                solver_ms,
                candidate_build_ms: None,
                used_card_count: Some(used_card_count),
                ..Default::default()
            }),
            ..Default::default()
        }),
    })
}

fn validate_candidates(request: &CandidateBuildRequest) -> Result<(), BuildError> {
    if request.candidates.is_empty() {
        return Err(BuildError::EmptyCandidates);
    }

    let expected = request.song_list.len();
    for (idx, candidate) in request.candidates.iter().enumerate() {
        if candidate.scores.len() != expected {
            return Err(BuildError::CandidateSongCountMismatch {
                candidate_id: idx,
                expected,
                actual: candidate.scores.len(),
            });
        }
    }

    Ok(())
}

fn calculate_medley(request: CandidateBuildRequest) -> Result<BuildResult, BuildError> {
    if request.song_list.len() != 3 {
        return Err(BuildError::InvalidMedleySongCount {
            count: request.song_list.len(),
        });
    }

    let trace = trace_enabled();
    let filter_start = Timer::start();
    let candidate_indices =
        medley_solver_candidate_indices(&request.candidates, request.current_best);
    let filter_ms = filter_start.elapsed_ms();
    if trace {
        eprintln!(
            "medley solver filter: candidates={} solver_candidates={} current_best={} filter_ms={:.3}",
            request.candidates.len(),
            candidate_indices.len(),
            request.current_best,
            filter_ms,
        );
    }
    let solver_input = MedleySolverInput {
        current_best: request.current_best,
        team_masks: candidate_indices
            .iter()
            .map(|&idx| request.candidates[idx].mask)
            .collect(),
        scores: candidate_indices
            .iter()
            .map(|&idx| {
                let candidate = &request.candidates[idx];
                [
                    candidate.scores[0],
                    candidate.scores[1],
                    candidate.scores[2],
                ]
            })
            .collect(),
    };

    let solve_start = Timer::start();
    let preference = request
        .solver_preference
        .unwrap_or(MedleySolverPreference::Auto);
    let plan = if candidates_use_wide_masks(&request.candidates) {
        let word_count = candidate_wide_mask_word_count(&request.candidates);
        let wide_input = WideMedleySolverInput {
            current_best: request.current_best,
            team_masks: candidate_indices
                .iter()
                .map(|&idx| candidate_wide_mask(&request.candidates[idx], word_count))
                .collect(),
            scores: solver_input.scores.clone(),
        };
        solve_medley_wide_with(&wide_input, preference)?
    } else {
        solve_medley_with(&solver_input, preference)?
    };
    let solver_ms = solve_start.elapsed_ms();
    if trace {
        eprintln!(
            "medley solver core: implementation={} score={} solve_ms={:.3}",
            format_medley_solver(plan.implementation),
            plan.score,
            solver_ms,
        );
    }

    let mut total_stat = 0;
    let mut songs = Vec::with_capacity(3);
    for (song_idx, &solver_candidate_idx) in plan.indices.iter().enumerate() {
        let candidate_idx = candidate_indices[solver_candidate_idx];
        let candidate = &request.candidates[candidate_idx];
        total_stat += candidate.stat;
        songs.push(song_result(
            &request.song_list[song_idx],
            candidate,
            song_idx,
        ));
    }

    Ok(BuildResult {
        event_id: request.event_id,
        event_type: request.event_type,
        total_score: plan.score,
        total_stat,
        songs,
        items: request.items,
        solver: Some(format_medley_solver(plan.implementation).to_owned()),
        metrics: Some(CalculationMetrics {
            medley: Some(MedleyCalculationMetrics {
                candidate_count: request.candidates.len(),
                solver_candidate_count: candidate_indices.len(),
                solver_filter_ms: filter_ms,
                solver_ms,
                candidate_build_ms: None,
                used_card_count: None,
                ..Default::default()
            }),
            ..Default::default()
        }),
    })
}

fn medley_solver_candidate_indices(candidates: &[TeamCandidate], current_best: i32) -> Vec<usize> {
    let max_scores: [i32; 3] = std::array::from_fn(|song_idx| {
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
            (0..3)
                .any(|song_idx| {
                    let upper_bound = candidate.scores[song_idx] as i64
                        + max_scores[(song_idx + 1) % 3] as i64
                        + max_scores[(song_idx + 2) % 3] as i64;
                    upper_bound > current_best as i64
                })
                .then_some(idx)
        })
        .collect()
}

fn raw_medley_solver_candidate_indices(
    candidates: &[RawTeamCandidate],
    current_best: i32,
) -> Vec<usize> {
    let max_scores: [i32; 3] = std::array::from_fn(|song_idx| {
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
            (0..3)
                .any(|song_idx| {
                    let upper_bound = candidate.scores[song_idx] as i64
                        + max_scores[(song_idx + 1) % 3] as i64
                        + max_scores[(song_idx + 2) % 3] as i64;
                    upper_bound > current_best as i64
                })
                .then_some(idx)
        })
        .collect()
}

fn calculate_single_team(request: CandidateBuildRequest) -> Result<BuildResult, BuildError> {
    let (candidate_idx, candidate) = request
        .candidates
        .iter()
        .enumerate()
        .max_by_key(|(_, candidate)| candidate.scores[0])
        .ok_or(BuildError::EmptyCandidates)?;

    let score = candidate.scores[0];
    let song = song_result(&request.song_list[0], candidate, 0);

    Ok(BuildResult {
        event_id: request.event_id,
        event_type: request.event_type,
        total_score: score,
        total_stat: request.candidates[candidate_idx].stat,
        songs: vec![song],
        items: request.items,
        solver: None,
        metrics: None,
    })
}

fn song_result(
    song: &SongSelection,
    candidate: &TeamCandidate,
    song_idx: usize,
) -> SongBuildResult {
    SongBuildResult {
        song_id: song.song_id,
        difficulty: song.difficulty,
        score: candidate.scores[song_idx],
        stat: candidate.stat,
        team_card_ids: candidate
            .ordered_team_card_ids
            .as_ref()
            .and_then(|teams| teams.get(song_idx))
            .cloned()
            .unwrap_or_else(|| candidate.team_card_ids.clone()),
        captain_card_id: candidate
            .captain_card_ids
            .get(song_idx)
            .copied()
            .or_else(|| candidate.team_card_ids.first().copied())
            .unwrap_or_default(),
    }
}

fn raw_song_result(
    song: &SongSelection,
    candidate: &RawTeamCandidate,
    cards: &[PreparedCard],
    song_idx: usize,
) -> SongBuildResult {
    SongBuildResult {
        song_id: song.song_id,
        difficulty: song.difficulty,
        score: candidate.scores[song_idx],
        stat: candidate.stat,
        team_card_ids: candidate.ordered_raw_indices[song_idx]
            .iter()
            .map(|&raw_idx| cards[raw_idx].card_id)
            .collect(),
        captain_card_id: cards[candidate.captain_raw_indices[song_idx]].card_id,
    }
}

fn format_medley_solver(implementation: MedleySolverImplementation) -> &'static str {
    match implementation {
        MedleySolverImplementation::Scalar => "scalar",
        MedleySolverImplementation::ScalarWide => "scalarWide",
        MedleySolverImplementation::RandomBucket => "randomBucket",
        MedleySolverImplementation::RandomBucketAvx2 => "randomBucketAvx2",
        MedleySolverImplementation::Avx2 => "avx2",
        MedleySolverImplementation::Avx2Wide => "avx2Wide",
        MedleySolverImplementation::ScalarFallbackAvx2Unavailable => {
            "scalarFallbackAvx2Unavailable"
        }
        MedleySolverImplementation::ScalarFallbackAvx2Unsupported => {
            "scalarFallbackAvx2Unsupported"
        }
    }
}

fn candidates_use_wide_masks(candidates: &[TeamCandidate]) -> bool {
    candidates
        .iter()
        .any(|candidate| !candidate.mask_words.is_empty())
}

fn candidate_wide_mask_word_count(candidates: &[TeamCandidate]) -> usize {
    candidates
        .iter()
        .map(|candidate| {
            candidate
                .mask_words
                .len()
                .max(usize::from(candidate.mask != 0))
        })
        .max()
        .unwrap_or(1)
        .max(1)
}

fn candidate_wide_mask(candidate: &TeamCandidate, word_count: usize) -> Vec<u64> {
    if candidate.mask_words.is_empty() {
        let mut words = vec![0; word_count];
        words[0] = candidate.mask;
        return words;
    }

    let mut words = candidate.mask_words.clone();
    words.resize(word_count, 0);
    words
}

fn trace_enabled() -> bool {
    std::env::var_os("BANGDREAM_OPTIMIZE_DP_TRACE").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::schema::{EventType, SongSelection};

    #[test]
    fn calculates_medley_from_candidates() {
        let request = CandidateBuildRequest {
            event_id: 100,
            event_type: EventType::Medley,
            song_list: vec![
                SongSelection {
                    song_id: 1,
                    difficulty: 3,
                },
                SongSelection {
                    song_id: 2,
                    difficulty: 3,
                },
                SongSelection {
                    song_id: 3,
                    difficulty: 3,
                },
            ],
            candidates: vec![
                TeamCandidate {
                    mask: 0b00001,
                    mask_words: Vec::new(),
                    team_card_ids: vec![10, 11, 12, 13, 14],
                    ordered_team_card_ids: None,
                    captain_card_ids: vec![10, 10, 10],
                    scores: vec![100, 1, 1],
                    stat: 1000,
                },
                TeamCandidate {
                    mask: 0b00010,
                    mask_words: Vec::new(),
                    team_card_ids: vec![20, 21, 22, 23, 24],
                    ordered_team_card_ids: None,
                    captain_card_ids: vec![20, 20, 20],
                    scores: vec![1, 100, 1],
                    stat: 2000,
                },
                TeamCandidate {
                    mask: 0b00100,
                    mask_words: Vec::new(),
                    team_card_ids: vec![30, 31, 32, 33, 34],
                    ordered_team_card_ids: None,
                    captain_card_ids: vec![30, 30, 30],
                    scores: vec![1, 1, 100],
                    stat: 3000,
                },
            ],
            current_best: 0,
            solver_preference: Some(MedleySolverPreference::Scalar),
            items: None,
        };

        let result = calculate_from_candidates(request).unwrap();

        assert_eq!(result.total_score, 300);
        assert_eq!(result.total_stat, 6000);
        assert_eq!(result.songs.len(), 3);
    }

    #[test]
    fn rejects_festival_candidates_until_fever_is_implemented() {
        let error = calculate_from_candidates(CandidateBuildRequest {
            event_id: 100,
            event_type: EventType::Festival,
            song_list: Vec::new(),
            candidates: Vec::new(),
            current_best: 0,
            solver_preference: None,
            items: None,
        })
        .unwrap_err();

        assert!(matches!(error, BuildError::UnsupportedEventType { .. }));
    }
}
