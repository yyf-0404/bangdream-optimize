use super::candidate::TeamCandidate;
use super::prune::{seed_signatures, MedleyPruneSignature};
use super::scoring::{build_candidate, MedleyCardInput, RawTeamCandidate, SkillMetaCache};
use super::team::{adjusted_card_stats, TeamBuildError, TeamGenerationOptions};
use crate::model::chart::{Chart, ExactScoreScratch, TeamCardSkill};
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::{
    BuildResult, EventType, SelectedAreaItems, SongBuildResult, SongSelection,
};

const TEAM_SIZE: usize = 5;
const MEDLEY_TEAM_COUNT: usize = 3;
const MEDLEY_TOTAL_CARD_COUNT: usize = TEAM_SIZE * MEDLEY_TEAM_COUNT;
const SEED_CANDIDATE_LIMIT: usize = 128;
const SEED_BANNED_CARD_VARIANTS: usize = 12;
const SEED_DISJOINT_GREEDY_ROUNDS: usize = 4;
const SEED_LOCAL_SEARCH_ROUNDS: usize = 16;

pub(crate) fn seed_medley_result_for_items(
    event_id: u32,
    song_list: &[SongSelection],
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    options: TeamGenerationOptions,
) -> Result<Option<BuildResult>, TeamBuildError> {
    if song_list.len() != MEDLEY_TEAM_COUNT {
        return Ok(None);
    }
    let Some(chosen) = seed_medley_candidates_for_items(
        cards,
        charts,
        area_item_percent,
        selected_items,
        options,
    )?
    else {
        return Ok(None);
    };
    let chosen_candidates = [
        &chosen[0].candidate,
        &chosen[1].candidate,
        &chosen[2].candidate,
    ];
    let total_score = chosen
        .iter()
        .enumerate()
        .map(|(song_idx, candidate)| candidate.candidate.scores[song_idx])
        .sum();
    let total_stat = chosen_candidates
        .iter()
        .map(|candidate| candidate.stat)
        .sum();
    let songs = chosen_candidates
        .iter()
        .enumerate()
        .map(|(song_idx, candidate)| seed_song_result(&song_list[song_idx], candidate, song_idx))
        .collect();

    Ok(Some(BuildResult {
        event_id,
        event_type: EventType::Medley,
        total_score,
        total_stat,
        songs,
        items: Some(selected_items.clone()),
        solver: Some("seed".to_owned()),
        metrics: None,
    }))
}

#[derive(Debug, Clone)]
struct SeedTeamCandidate {
    raw_indices: [usize; TEAM_SIZE],
    candidate: TeamCandidate,
}

#[derive(Debug, Clone, Copy)]
struct SeedScoredCard {
    idx: usize,
    value: f64,
}

fn seed_team_candidates_for_song(
    cards: &[PreparedCard],
    charts: &[Chart],
    card_stats: &[f64],
    options: TeamGenerationOptions,
    song_idx: usize,
    skill_meta_cache: &mut SkillMetaCache,
    exact_score_scratch: &mut ExactScoreScratch,
) -> Result<Vec<SeedTeamCandidate>, TeamBuildError> {
    let signatures = seed_signatures(cards);
    let mut result = Vec::new();

    for signature in signatures {
        let scored_cards = seed_scored_cards(cards, charts, card_stats, signature, song_idx)?;
        if scored_cards.len() < TEAM_SIZE {
            continue;
        }

        let mut round_banned = Vec::new();
        for _ in 0..SEED_DISJOINT_GREEDY_ROUNDS {
            let Some(indices) = greedy_seed_team(cards, &scored_cards, &round_banned) else {
                break;
            };
            push_seed_candidate(
                &mut result,
                cards,
                charts,
                card_stats,
                options,
                indices,
                skill_meta_cache,
                exact_score_scratch,
            )?;
            round_banned.extend(indices);
        }
        for banned in scored_cards
            .iter()
            .take(SEED_BANNED_CARD_VARIANTS)
            .map(|scored| scored.idx)
        {
            if let Some(indices) = greedy_seed_team(cards, &scored_cards, &[banned]) {
                push_seed_candidate(
                    &mut result,
                    cards,
                    charts,
                    card_stats,
                    options,
                    indices,
                    skill_meta_cache,
                    exact_score_scratch,
                )?;
            }
        }
    }

    result.sort_by(|left, right| {
        right.candidate.scores[song_idx].cmp(&left.candidate.scores[song_idx])
    });
    result.truncate(SEED_CANDIDATE_LIMIT);
    Ok(result)
}

fn seed_scored_cards(
    cards: &[PreparedCard],
    charts: &[Chart],
    card_stats: &[f64],
    signature: MedleyPruneSignature,
    song_idx: usize,
) -> Result<Vec<SeedScoredCard>, TeamBuildError> {
    let chart = &charts[song_idx];
    let mut scored = Vec::new();
    for (idx, card) in cards.iter().enumerate() {
        if !signature.allows(card) {
            continue;
        }
        let score_up = card
            .score_up
            .resolve(signature.team_band_id(), signature.team_attribute());
        let skill = TeamCardSkill {
            card_id: card.card_id,
            duration: card.skill.duration,
            score_up,
            rateup: card.skill.rateup,
        };
        let mut best_meta: f64 = 0.0;
        for activation in 0..=TEAM_SIZE {
            best_meta = best_meta.max(chart.skill_meta_value(activation, skill)?);
        }
        let stat = card_stats[idx];
        scored.push(SeedScoredCard {
            idx,
            value: stat * (chart.meta.no_skill + best_meta),
        });
    }

    scored.sort_by(|left, right| {
        right
            .value
            .total_cmp(&left.value)
            .then_with(|| cards[left.idx].card_id.cmp(&cards[right.idx].card_id))
    });
    Ok(scored)
}

fn greedy_seed_team(
    cards: &[PreparedCard],
    scored_cards: &[SeedScoredCard],
    banned_indices: &[usize],
) -> Option<[usize; TEAM_SIZE]> {
    let mut selected = Vec::with_capacity(TEAM_SIZE);
    let mut characters = Vec::with_capacity(TEAM_SIZE);
    for scored in scored_cards {
        if banned_indices.contains(&scored.idx) {
            continue;
        }
        let character_id = cards[scored.idx].character_id;
        if characters.contains(&character_id) {
            continue;
        }
        characters.push(character_id);
        selected.push(scored.idx);
        if selected.len() == TEAM_SIZE {
            return selected.try_into().ok();
        }
    }

    None
}

fn push_seed_candidate(
    candidates: &mut Vec<SeedTeamCandidate>,
    cards: &[PreparedCard],
    charts: &[Chart],
    card_stats: &[f64],
    options: TeamGenerationOptions,
    raw_indices: [usize; TEAM_SIZE],
    skill_meta_cache: &mut SkillMetaCache,
    exact_score_scratch: &mut ExactScoreScratch,
) -> Result<(), TeamBuildError> {
    if candidates
        .iter()
        .any(|candidate| same_seed_team(candidate.raw_indices, raw_indices))
    {
        return Ok(());
    }

    let candidate = seed_candidate_from_raw_indices(
        cards,
        charts,
        card_stats,
        options,
        raw_indices,
        skill_meta_cache,
        exact_score_scratch,
    )?;

    candidates.push(SeedTeamCandidate {
        raw_indices,
        candidate,
    });
    Ok(())
}

fn seed_candidate_from_raw_indices(
    cards: &[PreparedCard],
    charts: &[Chart],
    card_stats: &[f64],
    options: TeamGenerationOptions,
    raw_indices: [usize; TEAM_SIZE],
    skill_meta_cache: &mut SkillMetaCache,
    exact_score_scratch: &mut ExactScoreScratch,
) -> Result<TeamCandidate, TeamBuildError> {
    let seed_cards: Vec<_> = raw_indices
        .iter()
        .copied()
        .map(|raw_idx| MedleyCardInput {
            card: &cards[raw_idx],
            raw_index: raw_idx,
            stat: card_stats[raw_idx],
        })
        .collect();
    let selected_indices = [0, 1, 2, 3, 4];
    let raw_candidate = build_candidate(
        &seed_cards,
        charts,
        options,
        &selected_indices,
        skill_meta_cache,
        exact_score_scratch,
    )?;

    Ok(seed_team_candidate_from_raw(
        raw_candidate,
        cards,
        charts.len(),
    ))
}

fn seed_team_candidate_from_raw(
    candidate: RawTeamCandidate,
    cards: &[PreparedCard],
    chart_count: usize,
) -> TeamCandidate {
    TeamCandidate {
        mask: (0..TEAM_SIZE).fold(0u64, |mask, idx| mask | (1u64 << idx)),
        mask_words: Vec::new(),
        team_card_ids: candidate
            .raw_indices
            .iter()
            .map(|&raw_idx| cards[raw_idx].card_id)
            .collect(),
        ordered_team_card_ids: Some(
            (0..chart_count)
                .map(|chart_idx| {
                    candidate.ordered_raw_indices[chart_idx]
                        .iter()
                        .map(|&raw_idx| cards[raw_idx].card_id)
                        .collect()
                })
                .collect(),
        ),
        captain_card_ids: (0..chart_count)
            .map(|chart_idx| cards[candidate.captain_raw_indices[chart_idx]].card_id)
            .collect(),
        scores: candidate.scores[..chart_count].to_vec(),
        stat: candidate.stat,
    }
}

fn improve_seed_partition(
    teams: &mut [SeedTeamCandidate; MEDLEY_TEAM_COUNT],
    cards: &[PreparedCard],
    charts: &[Chart],
    card_stats: &[f64],
    options: TeamGenerationOptions,
    skill_meta_cache: &mut SkillMetaCache,
    exact_score_scratch: &mut ExactScoreScratch,
) -> Result<(), TeamBuildError> {
    for _ in 0..SEED_LOCAL_SEARCH_ROUNDS {
        let current_score = seed_partition_score(teams);
        let mut best_move: Option<SeedReplacement> = None;

        for song_idx in 0..MEDLEY_TEAM_COUNT {
            for slot in 0..TEAM_SIZE {
                for raw_idx in 0..cards.len() {
                    if seed_partition_uses_card(teams, raw_idx)
                        && raw_idx != teams[song_idx].raw_indices[slot]
                    {
                        continue;
                    }
                    if raw_idx == teams[song_idx].raw_indices[slot] {
                        continue;
                    }
                    if !seed_replacement_keeps_team_characters(
                        cards,
                        teams[song_idx].raw_indices,
                        slot,
                        raw_idx,
                    ) {
                        continue;
                    }

                    let mut raw_indices = teams[song_idx].raw_indices;
                    raw_indices[slot] = raw_idx;
                    let candidate = seed_candidate_from_raw_indices(
                        cards,
                        charts,
                        card_stats,
                        options,
                        raw_indices,
                        skill_meta_cache,
                        exact_score_scratch,
                    )?;
                    let score = current_score - teams[song_idx].candidate.scores[song_idx]
                        + candidate.scores[song_idx];
                    if score > best_move.as_ref().map_or(current_score, |best| best.score) {
                        best_move = Some(SeedReplacement {
                            song_idx,
                            score,
                            team: SeedTeamCandidate {
                                raw_indices,
                                candidate,
                            },
                        });
                    }
                }
            }
        }

        let Some(best_move) = best_move else {
            break;
        };
        teams[best_move.song_idx] = best_move.team;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct SeedReplacement {
    song_idx: usize,
    score: i32,
    team: SeedTeamCandidate,
}

fn seed_partition_score(teams: &[SeedTeamCandidate; MEDLEY_TEAM_COUNT]) -> i32 {
    teams
        .iter()
        .enumerate()
        .map(|(song_idx, team)| team.candidate.scores[song_idx])
        .sum()
}

fn seed_partition_uses_card(
    teams: &[SeedTeamCandidate; MEDLEY_TEAM_COUNT],
    raw_idx: usize,
) -> bool {
    teams.iter().any(|team| team.raw_indices.contains(&raw_idx))
}

fn seed_replacement_keeps_team_characters(
    cards: &[PreparedCard],
    raw_indices: [usize; TEAM_SIZE],
    replacement_slot: usize,
    replacement_idx: usize,
) -> bool {
    let replacement_character = cards[replacement_idx].character_id;
    raw_indices.iter().enumerate().all(|(slot, &raw_idx)| {
        slot == replacement_slot || cards[raw_idx].character_id != replacement_character
    })
}

fn same_seed_team(mut left: [usize; TEAM_SIZE], mut right: [usize; TEAM_SIZE]) -> bool {
    left.sort_unstable();
    right.sort_unstable();
    left == right
}

fn best_seed_candidate_indices(song_candidates: &[Vec<SeedTeamCandidate>]) -> Option<[usize; 3]> {
    let mut best_score = i32::MIN;
    let mut best_indices = None;

    for (first_idx, first) in song_candidates[0].iter().enumerate() {
        for (second_idx, second) in song_candidates[1].iter().enumerate() {
            if !seed_teams_disjoint(first.raw_indices, second.raw_indices) {
                continue;
            }
            for (third_idx, third) in song_candidates[2].iter().enumerate() {
                if !seed_teams_disjoint(first.raw_indices, third.raw_indices)
                    || !seed_teams_disjoint(second.raw_indices, third.raw_indices)
                {
                    continue;
                }
                let score = first.candidate.scores[0]
                    + second.candidate.scores[1]
                    + third.candidate.scores[2];
                if score > best_score {
                    best_score = score;
                    best_indices = Some([first_idx, second_idx, third_idx]);
                }
            }
        }
    }

    best_indices
}

fn seed_teams_disjoint(left: [usize; TEAM_SIZE], right: [usize; TEAM_SIZE]) -> bool {
    left.iter().all(|left_idx| !right.contains(left_idx))
}

fn seed_song_result(
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
        skill_queue_risk: false,
    }
}

pub(crate) fn seed_medley_raw_team_indices_for_items(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    options: TeamGenerationOptions,
) -> Result<Option<[[usize; TEAM_SIZE]; MEDLEY_TEAM_COUNT]>, TeamBuildError> {
    Ok(
        seed_medley_candidates_for_items(
            cards,
            charts,
            area_item_percent,
            selected_items,
            options,
        )?
        .map(|chosen| chosen.map(|candidate| candidate.raw_indices)),
    )
}

fn seed_medley_candidates_for_items(
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    options: TeamGenerationOptions,
) -> Result<Option<[SeedTeamCandidate; MEDLEY_TEAM_COUNT]>, TeamBuildError> {
    if charts.len() != MEDLEY_TEAM_COUNT || cards.len() < MEDLEY_TOTAL_CARD_COUNT {
        return Ok(None);
    }
    let card_stats = adjusted_card_stats(cards, area_item_percent, selected_items);
    let mut skill_meta_cache = SkillMetaCache::new(charts.len());
    let mut exact_score_scratch = ExactScoreScratch::default();
    let mut song_candidates = Vec::with_capacity(MEDLEY_TEAM_COUNT);
    for song_idx in 0..MEDLEY_TEAM_COUNT {
        let candidates = seed_team_candidates_for_song(
            cards,
            charts,
            &card_stats,
            options,
            song_idx,
            &mut skill_meta_cache,
            &mut exact_score_scratch,
        )?;
        if candidates.is_empty() {
            return Ok(None);
        }
        song_candidates.push(candidates);
    }
    let Some(indices) = best_seed_candidate_indices(&song_candidates) else {
        return Ok(None);
    };
    let mut chosen = [
        song_candidates[0][indices[0]].clone(),
        song_candidates[1][indices[1]].clone(),
        song_candidates[2][indices[2]].clone(),
    ];
    improve_seed_partition(
        &mut chosen,
        cards,
        charts,
        &card_stats,
        options,
        &mut skill_meta_cache,
        &mut exact_score_scratch,
    )?;
    Ok(Some(chosen))
}
