use std::collections::BTreeMap;
use thiserror::Error;

mod prune;
mod timing;
mod trace;

use prune::{prune_single_cards_with_stats, trace_single_prune_stats};
use timing::{optional_elapsed_ms, Timer};
use trace::{trace_single_group_stats, FrontierInsertResult, SingleGroupTraceStats};

pub const TEAM_SIZE: usize = 5;
const CAPTAIN_CHOICES: usize = 2;
pub(crate) const SCORE_EPSILON: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SingleDpTerm {
    pub sb: f64,
    pub c: f64,
}

impl SingleDpTerm {
    pub const ZERO: Self = Self { sb: 0.0, c: 0.0 };
}

#[derive(Debug, Clone, PartialEq)]
pub struct SingleDpCard {
    pub card_id: u32,
    pub group_id: u32,
    pub stat: i32,
    pub normal_terms: [SingleDpTerm; TEAM_SIZE],
    pub captain_term: SingleDpTerm,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SingleDpInput {
    pub d: f64,
    pub c: f64,
    pub cards: Vec<SingleDpCard>,
}

impl SingleDpInput {
    pub fn score_raw(&self, sa: i32, sb: f64, c: f64) -> f64 {
        sa as f64 * (self.d + sb) + self.c + c
    }

    pub fn score(&self, sa: i32, sb: f64, c: f64) -> i32 {
        floor_score(self.score_raw(sa, sb, c))
    }
}

#[derive(Debug, Error)]
pub enum SingleDpError {
    #[error("at least five cards are required to build a team, got {count}")]
    NotEnoughCards { count: usize },

    #[error("no valid single-song DP result found")]
    NoResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleDpResult {
    pub score: i32,
    pub stat: i32,
    pub team_card_ids: Vec<u32>,
    pub captain_card_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SingleState {
    pub(crate) sa: i32,
    pub(crate) sb: f64,
    pub(crate) c: f64,
    pub(crate) positions: [u32; TEAM_SIZE],
    pub(crate) captain_card_id: u32,
}

impl SingleState {
    fn empty() -> Self {
        Self {
            sa: 0,
            sb: 0.0,
            c: 0.0,
            positions: [0; TEAM_SIZE],
            captain_card_id: 0,
        }
    }

    fn dominates(&self, other: &Self) -> bool {
        self.sa >= other.sa && self.sb >= other.sb && self.c >= other.c
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Frontier {
    states: Vec<SingleState>,
}

impl Frontier {
    fn insert(&mut self, state: SingleState) -> FrontierInsertResult {
        if self.states.iter().any(|old| old.dominates(&state)) {
            return FrontierInsertResult {
                inserted: false,
                removed_count: 0,
            };
        }
        let before_len = self.states.len();
        self.states.retain(|old| !state.dominates(old));
        let removed_count = before_len - self.states.len();
        self.states.push(state);
        FrontierInsertResult {
            inserted: true,
            removed_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SingleSuffixBound {
    pub(crate) remaining_groups: usize,
    stat_by_slots: [i32; TEAM_SIZE + 1],
    normal_terms: [SingleDpTerm; TEAM_SIZE],
    captain_term: SingleDpTerm,
}

pub fn solve_single_dp(input: &SingleDpInput) -> Result<SingleDpResult, SingleDpError> {
    solve_single_dp_with_raw_score(input).map(|(result, _)| result)
}

pub fn solve_single_dp_with_raw_score(
    input: &SingleDpInput,
) -> Result<(SingleDpResult, f64), SingleDpError> {
    let (cards, prune_stats) = prune_single_cards_with_stats(input);
    if trace_enabled() {
        trace_single_prune_stats(&prune_stats);
    }
    if cards.len() < TEAM_SIZE {
        return Err(SingleDpError::NotEnoughCards { count: cards.len() });
    }

    let frontiers = single_song_frontiers(&cards, input);
    let result_frontier = &frontiers[frontier_index(single_full_mask(), true)];
    let best = result_frontier
        .states
        .iter()
        .max_by(|left, right| {
            input
                .score_raw(left.sa, left.sb, left.c)
                .total_cmp(&input.score_raw(right.sa, right.sb, right.c))
        })
        .ok_or(SingleDpError::NoResult)?;
    let score_raw = input.score_raw(best.sa, best.sb, best.c);

    Ok((
        SingleDpResult {
            score: input.score(best.sa, best.sb, best.c),
            stat: best.sa,
            team_card_ids: best.positions.to_vec(),
            captain_card_id: best.captain_card_id,
        },
        score_raw,
    ))
}

fn single_song_frontiers(cards: &[SingleDpCard], input: &SingleDpInput) -> Vec<Frontier> {
    let groups = character_groups(cards);
    let suffix_bounds = single_suffix_bounds(&groups, cards);
    let mut incumbent = seed_single_incumbent_state(cards, input)
        .map(|state| input.score_raw(state.sa, state.sb, state.c))
        .unwrap_or(f64::NEG_INFINITY);
    let mut frontiers = vec![Frontier::default(); (1 << TEAM_SIZE) * CAPTAIN_CHOICES];
    let _ = frontiers[frontier_index(0, false)].insert(SingleState::empty());
    let trace = trace_enabled();

    for (group_idx, group) in groups.iter().enumerate() {
        let group_start = trace.then(Timer::start);
        let old = std::mem::take(&mut frontiers);
        let mut next = vec![Frontier::default(); (1 << TEAM_SIZE) * CAPTAIN_CHOICES];
        let bound = &suffix_bounds[group_idx + 1];
        let mut trace_stats = SingleGroupTraceStats {
            group_idx,
            group_count: groups.len(),
            card_count: group.len(),
            old_states: trace
                .then(|| frontier_state_count(&old))
                .unwrap_or_default(),
            ..SingleGroupTraceStats::default()
        };

        for mask in 0..(1usize << TEAM_SIZE) {
            for captain_used in [false, true] {
                let key = frontier_index(mask, captain_used);
                for state in &old[key].states {
                    insert_single_state(
                        &mut next,
                        state.clone(),
                        mask,
                        captain_used,
                        bound,
                        input,
                        &mut incumbent,
                        &mut trace_stats,
                    );

                    for &card_idx in group {
                        let card = &cards[card_idx];
                        for position in empty_positions(mask) {
                            insert_transition(
                                &mut next,
                                state,
                                card,
                                mask,
                                captain_used,
                                position,
                                card.normal_terms[position],
                                None,
                                bound,
                                input,
                                &mut incumbent,
                                &mut trace_stats,
                            );

                            if !captain_used {
                                insert_transition(
                                    &mut next,
                                    state,
                                    card,
                                    mask,
                                    captain_used,
                                    position,
                                    card.normal_terms[position],
                                    Some(card.captain_term),
                                    bound,
                                    input,
                                    &mut incumbent,
                                    &mut trace_stats,
                                );
                            }
                        }
                    }
                }
            }
        }

        if trace {
            trace_stats.next_states = frontier_state_count(&next);
            trace_stats.group_ms = elapsed_ms(group_start);
            trace_single_group_stats(&trace_stats);
        }

        frontiers = next;
    }

    frontiers
}

#[allow(clippy::too_many_arguments)]
fn insert_transition(
    frontiers: &mut [Frontier],
    state: &SingleState,
    card: &SingleDpCard,
    mask: usize,
    captain_used: bool,
    position: usize,
    normal: SingleDpTerm,
    captain: Option<SingleDpTerm>,
    bound: &SingleSuffixBound,
    input: &SingleDpInput,
    incumbent: &mut f64,
    trace_stats: &mut SingleGroupTraceStats,
) {
    let mut next = state.clone();
    next.sa += card.stat;
    next.sb += normal.sb;
    next.c += normal.c;
    next.positions[position] = card.card_id;
    let next_captain_used = captain_used || captain.is_some();
    if let Some(captain) = captain {
        next.sb += captain.sb;
        next.c += captain.c;
        next.captain_card_id = card.card_id;
    }

    let next_mask = mask | (1usize << position);
    insert_single_state(
        frontiers,
        next,
        next_mask,
        next_captain_used,
        bound,
        input,
        incumbent,
        trace_stats,
    );
}

fn insert_single_state(
    frontiers: &mut [Frontier],
    state: SingleState,
    mask: usize,
    captain_used: bool,
    bound: &SingleSuffixBound,
    input: &SingleDpInput,
    incumbent: &mut f64,
    trace_stats: &mut SingleGroupTraceStats,
) {
    trace_stats.insert_attempts += 1;
    if !single_state_can_complete(mask, captain_used, bound) {
        trace_stats.feasibility_prunes += 1;
        return;
    }

    if mask == single_full_mask() {
        let score = input.score_raw(state.sa, state.sb, state.c);
        if score > *incumbent {
            *incumbent = score;
            trace_stats.incumbent_updates += 1;
        }
        trace_stats.completed_states += 1;
        let result = frontiers[frontier_index(mask, captain_used)].insert(state);
        trace_stats.record_frontier_insert(result);
        return;
    }

    if single_state_upper_bound(input, &state, mask, captain_used, bound) + SCORE_EPSILON
        < *incumbent
    {
        trace_stats.upper_bound_prunes += 1;
        return;
    }

    let result = frontiers[frontier_index(mask, captain_used)].insert(state);
    trace_stats.record_frontier_insert(result);
}

fn single_suffix_bounds(groups: &[Vec<usize>], cards: &[SingleDpCard]) -> Vec<SingleSuffixBound> {
    let mut result = Vec::with_capacity(groups.len() + 1);

    for start in 0..=groups.len() {
        result.push(single_bound_from_indices(
            cards,
            groups[start..].iter().flatten().copied(),
        ));
    }

    result
}

pub(crate) fn seed_single_incumbent_state(
    cards: &[SingleDpCard],
    input: &SingleDpInput,
) -> Option<SingleState> {
    let mut used_groups = Vec::with_capacity(TEAM_SIZE);
    let mut selected_card_indices = Vec::with_capacity(TEAM_SIZE);
    let mut state = SingleState::empty();
    let mut positions = Vec::with_capacity(TEAM_SIZE);

    for position in 0..TEAM_SIZE {
        let mut max_sb = 0.0;
        for card in cards {
            max_sb = f64::max(max_sb, card.normal_terms[position].sb);
        }
        positions.push((input.d + max_sb, position));
    }
    positions.sort_unstable_by(|left, right| right.0.total_cmp(&left.0));

    for &(_, position) in &positions {
        let mut best: Option<(usize, f64, SingleDpTerm)> = None;
        for (card_idx, card) in cards.iter().enumerate() {
            if used_groups.contains(&card.group_id) {
                continue;
            }
            let term = card.normal_terms[position];
            let value = card.stat.max(0) as f64 * (input.d + term.sb) + term.c;
            if best.is_none_or(|(_, best_value, _)| best_value < value) {
                best = Some((card_idx, value, term));
            }
        }

        let (card_idx, _, term) = best?;
        let card = &cards[card_idx];
        used_groups.push(card.group_id);
        selected_card_indices.push(card_idx);
        state.sa += card.stat;
        state.sb += term.sb;
        state.c += term.c;
        state.positions[position] = card.card_id;
    }

    let (captain_card_idx, captain) = selected_card_indices
        .iter()
        .map(|&card_idx| (card_idx, cards[card_idx].captain_term))
        .max_by(|left, right| {
            let left_score = state.sa as f64 * left.1.sb + left.1.c;
            let right_score = state.sa as f64 * right.1.sb + right.1.c;
            left_score.total_cmp(&right_score)
        })?;

    state.sb += captain.sb;
    state.c += captain.c;
    state.captain_card_id = cards[captain_card_idx].card_id;

    Some(state)
}

pub(crate) fn single_bound_from_indices(
    cards: &[SingleDpCard],
    indices: impl IntoIterator<Item = usize>,
) -> SingleSuffixBound {
    let mut bound = SingleSuffixBound {
        remaining_groups: 0,
        stat_by_slots: [0; TEAM_SIZE + 1],
        normal_terms: [SingleDpTerm::ZERO; TEAM_SIZE],
        captain_term: SingleDpTerm::ZERO,
    };
    let mut group_stats: BTreeMap<u32, i32> = BTreeMap::new();

    for card_idx in indices {
        let card = &cards[card_idx];
        group_stats
            .entry(card.group_id)
            .and_modify(|stat| *stat = (*stat).max(card.stat.max(0)))
            .or_insert_with(|| card.stat.max(0));
        for position in 0..TEAM_SIZE {
            maximize_model_term(
                &mut bound.normal_terms[position],
                card.normal_terms[position],
            );
        }
        maximize_model_term(&mut bound.captain_term, card.captain_term);
    }

    bound.remaining_groups = group_stats.len();
    let mut group_stats = group_stats.into_values().collect::<Vec<_>>();
    group_stats.sort_unstable_by(|left, right| right.cmp(left));
    let mut stat_sum = 0;
    for slots in 1..=TEAM_SIZE {
        if let Some(stat) = group_stats.get(slots - 1) {
            stat_sum += *stat;
        }
        bound.stat_by_slots[slots] = stat_sum;
    }

    bound
}

fn maximize_model_term(target: &mut SingleDpTerm, candidate: SingleDpTerm) {
    target.sb = target.sb.max(candidate.sb);
    target.c = target.c.max(candidate.c);
}

fn character_groups(cards: &[SingleDpCard]) -> Vec<Vec<usize>> {
    let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (idx, input) in cards.iter().enumerate() {
        groups.entry(input.group_id).or_default().push(idx);
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        left.len()
            .cmp(&right.len())
            .then_with(|| left.first().cmp(&right.first()))
    });
    groups
}

fn empty_positions(mask: usize) -> impl Iterator<Item = usize> {
    (0..TEAM_SIZE).filter(move |position| mask & (1usize << position) == 0)
}

fn single_full_mask() -> usize {
    (1usize << TEAM_SIZE) - 1
}

fn remaining_slots(mask: usize) -> usize {
    TEAM_SIZE - mask.count_ones() as usize
}

fn single_state_can_complete(mask: usize, captain_used: bool, bound: &SingleSuffixBound) -> bool {
    let remaining_slots = remaining_slots(mask);
    if remaining_slots > bound.remaining_groups {
        return false;
    }
    captain_used || remaining_slots > 0
}

pub(crate) fn single_state_upper_bound(
    input: &SingleDpInput,
    state: &SingleState,
    mask: usize,
    captain_used: bool,
    bound: &SingleSuffixBound,
) -> f64 {
    let remaining_slots = remaining_slots(mask);
    let mut extra_sb = 0.0;
    let mut extra_c = 0.0;
    for position in empty_positions(mask) {
        extra_sb += bound.normal_terms[position].sb;
        extra_c += bound.normal_terms[position].c;
    }
    if !captain_used && remaining_slots > 0 {
        extra_sb += bound.captain_term.sb;
        extra_c += bound.captain_term.c;
    }

    let weight = input.d + state.sb + extra_sb;
    input.c
        + state.sa as f64 * weight
        + state.c
        + extra_c
        + bound.stat_by_slots[remaining_slots] as f64 * weight.max(0.0)
}

fn frontier_index(mask: usize, captain_used: bool) -> usize {
    mask * CAPTAIN_CHOICES + usize::from(captain_used)
}

fn floor_score(score: f64) -> i32 {
    score.floor().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

fn trace_enabled() -> bool {
    std::env::var_os("BANGDREAM_OPTIMIZE_DP_TRACE").is_some()
}

fn frontier_state_count(frontiers: &[Frontier]) -> usize {
    frontiers.iter().map(|frontier| frontier.states.len()).sum()
}

fn elapsed_ms(start: Option<Timer>) -> f64 {
    optional_elapsed_ms(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_dp_matches_bruteforce_numeric_model() {
        let input = SingleDpInput {
            d: 10.0,
            c: 3.0,
            cards: (1..=6)
                .map(|card_id| card(card_id, card_id, 900 + card_id as i32 * 20, card_id as f64))
                .collect(),
        };

        let dp = solve_single_dp(&input).unwrap();
        let brute = brute_force_single_dp(&input);

        assert_eq!(dp.score, brute.score);
        assert_eq!(dp.stat, brute.stat);
        assert_eq!(dp.captain_card_id, brute.captain_card_id);
    }

    fn brute_force_single_dp(input: &SingleDpInput) -> SingleDpResult {
        let mut best: Option<SingleDpResult> = None;
        let mut selected = Vec::new();

        enumerate_card_sets(&input.cards, 0, &mut selected, &mut |team| {
            for order in permutations(team) {
                for captain_idx in 0..TEAM_SIZE {
                    let mut sa = 0;
                    let mut sb = 0.0;
                    let mut c = 0.0;
                    for (position, &card_idx) in order.iter().enumerate() {
                        let card = &input.cards[card_idx];
                        let term = card.normal_terms[position];
                        sa += card.stat;
                        sb += term.sb;
                        c += term.c;
                    }
                    let captain_card = &input.cards[order[captain_idx]];
                    sb += captain_card.captain_term.sb;
                    c += captain_card.captain_term.c;
                    let candidate = SingleDpResult {
                        score: input.score(sa, sb, c),
                        stat: sa,
                        team_card_ids: order.iter().map(|&idx| input.cards[idx].card_id).collect(),
                        captain_card_id: captain_card.card_id,
                    };
                    if best
                        .as_ref()
                        .is_none_or(|best| best.score < candidate.score)
                    {
                        best = Some(candidate);
                    }
                }
            }
        });

        best.expect("bruteforce should find a result")
    }

    fn enumerate_card_sets(
        cards: &[SingleDpCard],
        start: usize,
        selected: &mut Vec<usize>,
        callback: &mut impl FnMut(&[usize]),
    ) {
        if selected.len() == TEAM_SIZE {
            callback(selected);
            return;
        }
        if start >= cards.len() {
            return;
        }
        let remaining = TEAM_SIZE - selected.len();
        if cards.len() - start < remaining {
            return;
        }

        for idx in start..cards.len() {
            if selected
                .iter()
                .any(|&selected_idx| cards[selected_idx].group_id == cards[idx].group_id)
            {
                continue;
            }
            selected.push(idx);
            enumerate_card_sets(cards, idx + 1, selected, callback);
            selected.pop();
        }
    }

    fn permutations(values: &[usize]) -> Vec<Vec<usize>> {
        let mut result = Vec::new();
        let mut current = values.to_vec();
        permute(&mut current, 0, &mut result);
        result
    }

    fn permute(values: &mut [usize], start: usize, result: &mut Vec<Vec<usize>>) {
        if start == values.len() {
            result.push(values.to_vec());
            return;
        }
        for idx in start..values.len() {
            values.swap(start, idx);
            permute(values, start + 1, result);
            values.swap(start, idx);
        }
    }

    fn card(card_id: u32, group_id: u32, stat: i32, weight: f64) -> SingleDpCard {
        SingleDpCard {
            card_id,
            group_id,
            stat,
            normal_terms: std::array::from_fn(|position| SingleDpTerm {
                sb: weight * (position as f64 + 1.0) * 0.01,
                c: weight * position as f64,
            }),
            captain_term: SingleDpTerm {
                sb: weight * 0.03,
                c: weight,
            },
        }
    }
}
