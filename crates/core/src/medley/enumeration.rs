use super::candidate::TeamCandidate;
use super::prune::{best_any_team_score_upper_bound, MedleyCardPruneProfile, MedleyPruneSignature};
use super::scoring::{
    build_resolved_candidate, resolve_medley_cards_for_signature, selected_resolved_team_signature,
    MedleyCardInput, RawTeamCandidate, ResolvedMedleyCardInput, SkillMetaCache,
};
use super::team::{TeamBuildError, TeamGenerationOptions};
use crate::model::chart::Chart;
use crate::model::preparation::PreparedCard;
use crate::timing::Timer;
use bangdream_optimize_medley_solver::{MedleySolverInput, TeamMask, WideMedleySolverInput};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

const TEAM_SIZE: usize = 5;
const MEDLEY_TEAM_COUNT: usize = 3;

#[derive(Debug, Clone, Copy)]
pub(in crate::medley) struct SignatureEnumerationStats {
    pub(in crate::medley) active_card_count: usize,
    pub(in crate::medley) group_count: usize,
    pub(in crate::medley) candidates_before: usize,
    pub(in crate::medley) candidates_after: usize,
    pub(in crate::medley) branch_upper_bound_pruned: usize,
    pub(in crate::medley) leaf_checks: usize,
    pub(in crate::medley) final_layer_card_checks: usize,
    pub(in crate::medley) final_layer_signature_passes: usize,
    pub(in crate::medley) final_layer_upper_bound_rejects: usize,
    pub(in crate::medley) signature_rejects: usize,
    pub(in crate::medley) candidate_builds: usize,
    pub(in crate::medley) candidate_filter_rejects: usize,
    pub(in crate::medley) build_candidate_ms: f64,
    pub(in crate::medley) candidate_filter_ms: f64,
}

#[derive(Debug)]
pub(in crate::medley) struct SignatureEnumerationError {
    pub(in crate::medley) stats: SignatureEnumerationStats,
    pub(in crate::medley) source: TeamBuildError,
}

#[derive(Debug, Clone)]
pub(in crate::medley) struct CandidateIncumbentFilter {
    current_best: i32,
    other_song_upper_bounds: [f64; MEDLEY_TEAM_COUNT],
}

impl CandidateIncumbentFilter {
    pub(in crate::medley) fn new(
        cards: &[PreparedCard],
        charts: &[Chart],
        profiles: &[MedleyCardPruneProfile],
        current_best: i32,
    ) -> Option<Self> {
        if current_best <= 0 || charts.len() != MEDLEY_TEAM_COUNT {
            return None;
        }

        let best_any_team_scores: [f64; MEDLEY_TEAM_COUNT] = std::array::from_fn(|chart_idx| {
            best_any_team_score_upper_bound(cards, charts, profiles, chart_idx)
        });
        Some(Self {
            current_best,
            other_song_upper_bounds: [
                best_any_team_scores[1] + best_any_team_scores[2],
                best_any_team_scores[0] + best_any_team_scores[2],
                best_any_team_scores[0] + best_any_team_scores[1],
            ],
        })
    }

    fn candidate_can_beat_incumbent(&self, candidate: &RawTeamCandidate) -> bool {
        let current_best = self.current_best as f64;
        candidate.scores[0] as f64 + self.other_song_upper_bounds[0] > current_best
            || candidate.scores[1] as f64 + self.other_song_upper_bounds[1] > current_best
            || candidate.scores[2] as f64 + self.other_song_upper_bounds[2] > current_best
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::medley) fn enumerate_signature_pool(
    cards: &[PreparedCard],
    card_stats: &[f64],
    charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    options: TeamGenerationOptions,
    signature: MedleyPruneSignature,
    active_card_indices: &[usize],
    candidates: &mut Vec<RawTeamCandidate>,
    skill_meta_cache: &mut SkillMetaCache,
    candidate_filter: Option<&CandidateIncumbentFilter>,
) -> Result<SignatureEnumerationStats, SignatureEnumerationError> {
    let active_cards = active_medley_cards(cards, card_stats, active_card_indices);
    let groups = character_groups(&active_cards);
    let mut selected_indices = [0; TEAM_SIZE];
    let prefix_filter = candidate_filter.and_then(|filter| {
        PrefixUpperBoundFilter::new(&active_cards, charts, profiles, signature, &groups, filter)
    });
    let mut stats = SignatureEnumerationStats {
        active_card_count: active_card_indices.len(),
        group_count: groups.len(),
        candidates_before: candidates.len(),
        candidates_after: candidates.len(),
        branch_upper_bound_pruned: 0,
        leaf_checks: 0,
        final_layer_card_checks: 0,
        final_layer_signature_passes: 0,
        final_layer_upper_bound_rejects: 0,
        signature_rejects: 0,
        candidate_builds: 0,
        candidate_filter_rejects: 0,
        build_candidate_ms: 0.0,
        candidate_filter_ms: 0.0,
    };
    let resolved_cards = match resolve_medley_cards_for_signature(
        &active_cards,
        charts,
        signature,
        skill_meta_cache,
    ) {
        Ok(cards) => cards,
        Err(source) => {
            return Err(SignatureEnumerationError { stats, source });
        }
    };

    match enumerate_signature_teams(
        &resolved_cards,
        charts,
        &groups,
        options,
        signature,
        0,
        0,
        &mut selected_indices,
        candidates,
        candidate_filter,
        prefix_filter.as_ref(),
        PrefixUpperBoundState::default(),
        PrefixSignatureState::default(),
        &mut stats,
    ) {
        Ok(()) => {
            stats.candidates_after = candidates.len();
            Ok(stats)
        }
        Err(source) => {
            stats.candidates_after = candidates.len();
            Err(SignatureEnumerationError { stats, source })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn enumerate_signature_teams(
    resolved_cards: &[ResolvedMedleyCardInput],
    charts: &[Chart],
    groups: &FlatCharacterGroups,
    options: TeamGenerationOptions,
    signature: MedleyPruneSignature,
    group_start: usize,
    selected_count: usize,
    selected_indices: &mut [usize; TEAM_SIZE],
    candidates: &mut Vec<RawTeamCandidate>,
    candidate_filter: Option<&CandidateIncumbentFilter>,
    prefix_filter: Option<&PrefixUpperBoundFilter>,
    prefix_state: PrefixUpperBoundState,
    signature_state: PrefixSignatureState,
    stats: &mut SignatureEnumerationStats,
) -> Result<(), TeamBuildError> {
    if selected_count == TEAM_SIZE {
        return process_complete_signature_team(
            resolved_cards,
            charts,
            options,
            signature,
            group_start,
            selected_indices,
            candidates,
            candidate_filter,
            prefix_filter,
            prefix_state,
            stats,
        );
    }

    if prefix_filter.is_some_and(|filter| {
        !filter.partial_team_can_beat_incumbent(selected_count, prefix_state, group_start)
    }) {
        stats.branch_upper_bound_pruned += 1;
        return Ok(());
    }

    let remaining_slots = TEAM_SIZE - selected_count;
    if remaining_slots == 1 {
        let flat_start = groups.offset(group_start);
        let flat_end = groups.card_indices.len();
        let mut flat_pos = flat_start;
        let use_avx2 = avx2_available();
        while use_avx2 && flat_pos + 4 <= flat_end {
            let can_beat_mask = prefix_filter
                .map(|filter| {
                    final_layer_can_beat_mask_avx2(filter, groups, flat_pos, prefix_state)
                })
                .unwrap_or(0b1111);
            let signature_mask =
                final_layer_signature_mask_avx2(groups, flat_pos, signature_state, signature);
            for lane in 0..4 {
                let lane_mask = 1 << lane;
                if can_beat_mask & lane_mask == 0 {
                    reject_final_layer_by_upper_bound(stats);
                    continue;
                }
                if signature_mask & lane_mask == 0 {
                    reject_final_layer_by_signature(stats);
                    continue;
                }
                process_final_layer_flat_card(
                    resolved_cards,
                    charts,
                    groups,
                    options,
                    flat_pos + lane,
                    selected_indices,
                    candidates,
                    candidate_filter,
                    stats,
                )?;
            }
            flat_pos += 4;
        }

        for flat_pos in flat_pos..flat_end {
            let card_idx = groups.card_indices[flat_pos];
            if let Some(filter) = prefix_filter {
                let group_start = groups.group_indices[flat_pos] + 1;
                if !filter.final_layer_card_can_beat_incumbent(prefix_state, group_start, card_idx)
                {
                    reject_final_layer_by_upper_bound(stats);
                    continue;
                }
            }
            if !signature_state.matches_with_card(groups, card_idx, signature) {
                reject_final_layer_by_signature(stats);
                continue;
            }
            process_final_layer_flat_card(
                resolved_cards,
                charts,
                groups,
                options,
                flat_pos,
                selected_indices,
                candidates,
                candidate_filter,
                stats,
            )?;
        }

        return Ok(());
    }

    let end = groups.len().saturating_sub(remaining_slots) + 1;
    let flat_start = groups.offset(group_start);
    let flat_end = groups.offset(end);
    for flat_pos in flat_start..flat_end {
        let card_idx = groups.card_indices[flat_pos];
        let group_idx = groups.group_indices[flat_pos];
        let next_prefix_state = prefix_filter
            .map(|filter| prefix_state.with_card(&filter.card_bounds[card_idx]))
            .unwrap_or(prefix_state);
        let next_signature_state = signature_state.with_card(groups, card_idx);
        selected_indices[selected_count] = card_idx;
        enumerate_signature_teams(
            resolved_cards,
            charts,
            groups,
            options,
            signature,
            group_idx + 1,
            selected_count + 1,
            selected_indices,
            candidates,
            candidate_filter,
            prefix_filter,
            next_prefix_state,
            next_signature_state,
            stats,
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn process_final_layer_flat_card(
    resolved_cards: &[ResolvedMedleyCardInput],
    charts: &[Chart],
    groups: &FlatCharacterGroups,
    options: TeamGenerationOptions,
    flat_pos: usize,
    selected_indices: &[usize; TEAM_SIZE],
    candidates: &mut Vec<RawTeamCandidate>,
    candidate_filter: Option<&CandidateIncumbentFilter>,
    stats: &mut SignatureEnumerationStats,
) -> Result<(), TeamBuildError> {
    stats.final_layer_card_checks += 1;
    stats.leaf_checks += 1;
    let card_idx = groups.card_indices[flat_pos];
    stats.final_layer_signature_passes += 1;

    let team_indices = [
        selected_indices[0],
        selected_indices[1],
        selected_indices[2],
        selected_indices[3],
        card_idx,
    ];
    let build_start = Timer::start();
    let candidate = build_resolved_candidate(resolved_cards, charts, options, &team_indices)?;
    stats.build_candidate_ms += build_start.elapsed_ms();
    stats.candidate_builds += 1;

    let filter_start = Timer::start();
    let rejected =
        candidate_filter.is_some_and(|filter| !filter.candidate_can_beat_incumbent(&candidate));
    stats.candidate_filter_ms += filter_start.elapsed_ms();
    if rejected {
        stats.candidate_filter_rejects += 1;
        return Ok(());
    }
    candidates.push(candidate);
    Ok(())
}

#[inline]
fn reject_final_layer_by_upper_bound(stats: &mut SignatureEnumerationStats) {
    stats.final_layer_card_checks += 1;
    stats.leaf_checks += 1;
    stats.branch_upper_bound_pruned += 1;
    stats.final_layer_upper_bound_rejects += 1;
}

#[inline]
fn reject_final_layer_by_signature(stats: &mut SignatureEnumerationStats) {
    stats.final_layer_card_checks += 1;
    stats.leaf_checks += 1;
    stats.signature_rejects += 1;
}

#[inline]
fn final_layer_can_beat_mask_avx2(
    filter: &PrefixUpperBoundFilter,
    groups: &FlatCharacterGroups,
    flat_pos: usize,
    state: PrefixUpperBoundState,
) -> u8 {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if avx2_available() {
            // SAFETY: runtime AVX2 support is checked above. The caller only
            // requests a four-lane mask when four flat positions are available.
            return unsafe { final_layer_can_beat_mask_avx2_x86(filter, groups, flat_pos, state) };
        }
    }

    final_layer_can_beat_mask_scalar(filter, groups, flat_pos, state)
}

#[inline]
fn final_layer_can_beat_mask_scalar(
    filter: &PrefixUpperBoundFilter,
    groups: &FlatCharacterGroups,
    flat_pos: usize,
    state: PrefixUpperBoundState,
) -> u8 {
    let mut mask = 0u8;
    for lane in 0..4 {
        let pos = flat_pos + lane;
        let card_idx = groups.card_indices[pos];
        let group_start = groups.group_indices[pos] + 1;
        let next_state = state.with_card(&filter.card_bounds[card_idx]);
        if filter.partial_team_can_beat_incumbent(TEAM_SIZE, next_state, group_start) {
            mask |= 1 << lane;
        }
    }
    mask
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn final_layer_can_beat_mask_avx2_x86(
    filter: &PrefixUpperBoundFilter,
    groups: &FlatCharacterGroups,
    flat_pos: usize,
    state: PrefixUpperBoundState,
) -> u8 {
    let card_indices = _mm_set_epi32(
        groups.card_indices[flat_pos + 3] as i32,
        groups.card_indices[flat_pos + 2] as i32,
        groups.card_indices[flat_pos + 1] as i32,
        groups.card_indices[flat_pos] as i32,
    );
    let suffix_indices = _mm_set_epi32(
        groups.group_indices[flat_pos + 3] as i32 + 1,
        groups.group_indices[flat_pos + 2] as i32 + 1,
        groups.group_indices[flat_pos + 1] as i32 + 1,
        groups.group_indices[flat_pos] as i32 + 1,
    );

    let stat = _mm256_add_pd(
        _mm256_i32gather_pd(filter.card_stat_bounds.as_ptr(), card_indices, 8),
        _mm256_set1_pd(state.stat),
    );
    let current_best = _mm256_set1_pd(filter.current_best as f64);
    let mut can_beat = _mm256_setzero_pd();

    for chart_idx in 0..MEDLEY_TEAM_COUNT {
        let normal_meta = _mm256_add_pd(
            _mm256_i32gather_pd(
                filter.card_normal_meta_by_chart[chart_idx].as_ptr(),
                card_indices,
                8,
            ),
            _mm256_set1_pd(state.normal_meta_by_chart[chart_idx]),
        );
        let card_captain = _mm256_i32gather_pd(
            filter.card_captain_meta_by_chart[chart_idx].as_ptr(),
            card_indices,
            8,
        );
        let suffix_captain = _mm256_i32gather_pd(
            filter.suffix_captain_meta_by_chart[chart_idx].as_ptr(),
            suffix_indices,
            8,
        );
        let captain_meta = _mm256_max_pd(
            _mm256_max_pd(
                card_captain,
                _mm256_set1_pd(state.captain_meta_by_chart[chart_idx]),
            ),
            suffix_captain,
        );
        let meta = _mm256_add_pd(
            _mm256_set1_pd(filter.no_skill_by_chart[chart_idx]),
            _mm256_add_pd(normal_meta, captain_meta),
        );
        let score = _mm256_add_pd(
            _mm256_floor_pd(_mm256_mul_pd(stat, meta)),
            _mm256_set1_pd(filter.other_song_upper_bounds[chart_idx]),
        );
        can_beat = _mm256_or_pd(can_beat, _mm256_cmp_pd(score, current_best, _CMP_GT_OQ));
    }

    _mm256_movemask_pd(can_beat) as u8
}

#[inline]
fn final_layer_signature_mask_avx2(
    groups: &FlatCharacterGroups,
    flat_pos: usize,
    state: PrefixSignatureState,
    signature: MedleyPruneSignature,
) -> u8 {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if avx2_available() {
            // SAFETY: runtime AVX2 support is checked above. The caller only
            // requests a four-lane mask when four flat positions are available.
            return unsafe {
                final_layer_signature_mask_avx2_x86(groups, flat_pos, state, signature)
            };
        }
    }

    final_layer_signature_mask_scalar(groups, flat_pos, state, signature)
}

#[inline]
fn final_layer_signature_mask_scalar(
    groups: &FlatCharacterGroups,
    flat_pos: usize,
    state: PrefixSignatureState,
    signature: MedleyPruneSignature,
) -> u8 {
    let mut mask = 0u8;
    for lane in 0..4 {
        let card_idx = groups.card_indices[flat_pos + lane];
        if state.matches_with_card(groups, card_idx, signature) {
            mask |= 1 << lane;
        }
    }
    mask
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn final_layer_signature_mask_avx2_x86(
    groups: &FlatCharacterGroups,
    flat_pos: usize,
    state: PrefixSignatureState,
    signature: MedleyPruneSignature,
) -> u8 {
    let card_indices = _mm_set_epi32(
        groups.card_indices[flat_pos + 3] as i32,
        groups.card_indices[flat_pos + 2] as i32,
        groups.card_indices[flat_pos + 1] as i32,
        groups.card_indices[flat_pos] as i32,
    );
    let band_ids = _mm_i32gather_epi32(groups.card_band_id_codes.as_ptr(), card_indices, 4);
    let attributes = _mm_i32gather_epi32(groups.card_attribute_codes.as_ptr(), card_indices, 4);

    let all_true = _mm_cmpeq_epi32(_mm_setzero_si128(), _mm_setzero_si128());
    let all_false = _mm_setzero_si128();
    let same_band = if state.same_band {
        _mm_cmpeq_epi32(band_ids, _mm_set1_epi32(state.first_band as i32))
    } else {
        all_false
    };
    let first_attribute = state.first_attribute.map(attribute_code).unwrap_or(-1);
    let same_attribute = if state.same_attribute {
        _mm_cmpeq_epi32(attributes, _mm_set1_epi32(first_attribute))
    } else {
        all_false
    };
    let not_same_band = _mm_andnot_si128(same_band, all_true);
    let not_same_attribute = _mm_andnot_si128(same_attribute, all_true);

    let matches = match signature {
        MedleyPruneSignature::Mixed => _mm_and_si128(not_same_band, not_same_attribute),
        MedleyPruneSignature::UnifiedBand(band_id) => {
            if state.first_band == band_id {
                _mm_and_si128(same_band, not_same_attribute)
            } else {
                all_false
            }
        }
        MedleyPruneSignature::UnifiedAttribute(attribute) => {
            if first_attribute == attribute_code(attribute) {
                _mm_and_si128(same_attribute, not_same_band)
            } else {
                all_false
            }
        }
        MedleyPruneSignature::UnifiedBandAttribute(band_id, attribute) => {
            if state.first_band == band_id && first_attribute == attribute_code(attribute) {
                _mm_and_si128(same_band, same_attribute)
            } else {
                all_false
            }
        }
    };

    _mm_movemask_ps(_mm_castsi128_ps(matches)) as u8
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn process_complete_signature_team(
    resolved_cards: &[ResolvedMedleyCardInput],
    charts: &[Chart],
    options: TeamGenerationOptions,
    signature: MedleyPruneSignature,
    group_start: usize,
    selected_indices: &[usize; TEAM_SIZE],
    candidates: &mut Vec<RawTeamCandidate>,
    candidate_filter: Option<&CandidateIncumbentFilter>,
    prefix_filter: Option<&PrefixUpperBoundFilter>,
    prefix_state: PrefixUpperBoundState,
    stats: &mut SignatureEnumerationStats,
) -> Result<(), TeamBuildError> {
    stats.leaf_checks += 1;
    if selected_resolved_team_signature(resolved_cards, selected_indices) != signature {
        stats.signature_rejects += 1;
        return Ok(());
    }
    stats.final_layer_signature_passes += 1;
    if prefix_filter.is_some_and(|filter| {
        !filter.partial_team_can_beat_incumbent(selected_indices.len(), prefix_state, group_start)
    }) {
        stats.branch_upper_bound_pruned += 1;
        stats.final_layer_upper_bound_rejects += 1;
        return Ok(());
    }
    if candidates.len() >= options.max_candidates {
        return Err(TeamBuildError::TooManyCandidates {
            limit: options.max_candidates,
        });
    }
    let build_start = Timer::start();
    let candidate = build_resolved_candidate(resolved_cards, charts, options, selected_indices)?;
    stats.build_candidate_ms += build_start.elapsed_ms();
    stats.candidate_builds += 1;

    let filter_start = Timer::start();
    let rejected =
        candidate_filter.is_some_and(|filter| !filter.candidate_can_beat_incumbent(&candidate));
    stats.candidate_filter_ms += filter_start.elapsed_ms();
    if rejected {
        stats.candidate_filter_rejects += 1;
        return Ok(());
    }
    candidates.push(candidate);
    Ok(())
}

#[derive(Debug, Clone)]
struct PrefixUpperBoundFilter {
    current_best: i32,
    no_skill_by_chart: [f64; MEDLEY_TEAM_COUNT],
    other_song_upper_bounds: [f64; MEDLEY_TEAM_COUNT],
    card_bounds: Vec<CardUpperBound>,
    card_stat_bounds: Vec<f64>,
    card_normal_meta_by_chart: [Vec<f64>; MEDLEY_TEAM_COUNT],
    card_captain_meta_by_chart: [Vec<f64>; MEDLEY_TEAM_COUNT],
    suffix_bounds: Vec<SuffixUpperBound>,
    suffix_captain_meta_by_chart: [Vec<f64>; MEDLEY_TEAM_COUNT],
}

#[derive(Debug, Clone)]
struct CardUpperBound {
    stat: f64,
    normal_meta_by_chart: [f64; MEDLEY_TEAM_COUNT],
    captain_meta_by_chart: [f64; MEDLEY_TEAM_COUNT],
}

#[derive(Debug, Clone)]
struct SuffixUpperBound {
    remaining_groups: usize,
    stat_by_slots: [f64; TEAM_SIZE + 1],
    normal_meta_by_chart_and_slots: [[f64; TEAM_SIZE + 1]; MEDLEY_TEAM_COUNT],
    captain_meta_by_chart: [f64; MEDLEY_TEAM_COUNT],
}

#[derive(Debug, Clone, Copy, Default)]
struct PrefixUpperBoundState {
    stat: f64,
    normal_meta_by_chart: [f64; MEDLEY_TEAM_COUNT],
    captain_meta_by_chart: [f64; MEDLEY_TEAM_COUNT],
}

impl PrefixUpperBoundState {
    fn with_card(self, bound: &CardUpperBound) -> Self {
        let mut next = self;
        next.stat += bound.stat;
        next.normal_meta_by_chart[0] += bound.normal_meta_by_chart[0];
        next.normal_meta_by_chart[1] += bound.normal_meta_by_chart[1];
        next.normal_meta_by_chart[2] += bound.normal_meta_by_chart[2];
        next.captain_meta_by_chart[0] =
            next.captain_meta_by_chart[0].max(bound.captain_meta_by_chart[0]);
        next.captain_meta_by_chart[1] =
            next.captain_meta_by_chart[1].max(bound.captain_meta_by_chart[1]);
        next.captain_meta_by_chart[2] =
            next.captain_meta_by_chart[2].max(bound.captain_meta_by_chart[2]);
        next
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PrefixSignatureState {
    first_band: u32,
    first_attribute: Option<crate::model::schema::Attribute>,
    same_band: bool,
    same_attribute: bool,
    selected_count: usize,
}

impl PrefixSignatureState {
    fn with_card(self, groups: &FlatCharacterGroups, card_idx: usize) -> Self {
        let band = groups.card_band_ids[card_idx];
        let attribute = groups.card_attributes[card_idx];
        if self.selected_count == 0 {
            return Self {
                first_band: band,
                first_attribute: Some(attribute),
                same_band: true,
                same_attribute: true,
                selected_count: 1,
            };
        }

        Self {
            first_band: self.first_band,
            first_attribute: self.first_attribute,
            same_band: self.same_band && band == self.first_band,
            same_attribute: self.same_attribute && self.first_attribute == Some(attribute),
            selected_count: self.selected_count + 1,
        }
    }

    fn matches_with_card(
        self,
        groups: &FlatCharacterGroups,
        card_idx: usize,
        signature: MedleyPruneSignature,
    ) -> bool {
        let band = groups.card_band_ids[card_idx];
        let attribute = groups.card_attributes[card_idx];
        let same_band = self.same_band && band == self.first_band;
        let same_attribute = self.same_attribute && self.first_attribute == Some(attribute);
        match signature {
            MedleyPruneSignature::Mixed => !same_band && !same_attribute,
            MedleyPruneSignature::UnifiedBand(band_id) => {
                same_band && self.first_band == band_id && !same_attribute
            }
            MedleyPruneSignature::UnifiedAttribute(target_attribute) => {
                same_attribute && self.first_attribute == Some(target_attribute) && !same_band
            }
            MedleyPruneSignature::UnifiedBandAttribute(band_id, target_attribute) => {
                same_band
                    && same_attribute
                    && self.first_band == band_id
                    && self.first_attribute == Some(target_attribute)
            }
        }
    }
}

impl PrefixUpperBoundFilter {
    fn new(
        cards: &[MedleyCardInput<'_>],
        charts: &[Chart],
        profiles: &[MedleyCardPruneProfile],
        signature: MedleyPruneSignature,
        groups: &FlatCharacterGroups,
        candidate_filter: &CandidateIncumbentFilter,
    ) -> Option<Self> {
        if charts.len() != MEDLEY_TEAM_COUNT {
            return None;
        }

        let card_bounds = cards
            .iter()
            .map(|card| card_upper_bound(card, charts, profiles, signature))
            .collect::<Option<Vec<_>>>()?;
        let suffix_bounds =
            suffix_upper_bounds(cards, charts, groups, &card_bounds).collect::<Vec<_>>();
        let card_stat_bounds = card_bounds.iter().map(|bound| bound.stat).collect();
        let card_normal_meta_by_chart = std::array::from_fn(|chart_idx| {
            card_bounds
                .iter()
                .map(|bound| bound.normal_meta_by_chart[chart_idx])
                .collect()
        });
        let card_captain_meta_by_chart = std::array::from_fn(|chart_idx| {
            card_bounds
                .iter()
                .map(|bound| bound.captain_meta_by_chart[chart_idx])
                .collect()
        });
        let suffix_captain_meta_by_chart = std::array::from_fn(|chart_idx| {
            suffix_bounds
                .iter()
                .map(|bound| bound.captain_meta_by_chart[chart_idx])
                .collect()
        });
        Some(Self {
            current_best: candidate_filter.current_best,
            no_skill_by_chart: std::array::from_fn(|idx| charts[idx].meta.no_skill),
            other_song_upper_bounds: candidate_filter.other_song_upper_bounds,
            card_bounds,
            card_stat_bounds,
            card_normal_meta_by_chart,
            card_captain_meta_by_chart,
            suffix_bounds,
            suffix_captain_meta_by_chart,
        })
    }

    fn partial_team_can_beat_incumbent(
        &self,
        selected_count: usize,
        state: PrefixUpperBoundState,
        group_start: usize,
    ) -> bool {
        let remaining_slots = TEAM_SIZE.saturating_sub(selected_count);
        let Some(suffix) = self.suffix_bounds.get(group_start) else {
            return true;
        };
        if remaining_slots > suffix.remaining_groups {
            return false;
        }

        let stat = state.stat + suffix.stat_by_slots[remaining_slots];

        let current_best = self.current_best as f64;
        self.chart_can_beat_incumbent(0, remaining_slots, stat, state, suffix, current_best)
            || self.chart_can_beat_incumbent(1, remaining_slots, stat, state, suffix, current_best)
            || self.chart_can_beat_incumbent(2, remaining_slots, stat, state, suffix, current_best)
    }

    fn final_layer_card_can_beat_incumbent(
        &self,
        state: PrefixUpperBoundState,
        group_start: usize,
        card_idx: usize,
    ) -> bool {
        let next_state = state.with_card(&self.card_bounds[card_idx]);
        self.partial_team_can_beat_incumbent(TEAM_SIZE, next_state, group_start)
    }

    fn chart_can_beat_incumbent(
        &self,
        chart_idx: usize,
        remaining_slots: usize,
        stat: f64,
        state: PrefixUpperBoundState,
        suffix: &SuffixUpperBound,
        current_best: f64,
    ) -> bool {
        let normal_meta = state.normal_meta_by_chart[chart_idx]
            + suffix.normal_meta_by_chart_and_slots[chart_idx][remaining_slots];
        let captain_meta =
            state.captain_meta_by_chart[chart_idx].max(suffix.captain_meta_by_chart[chart_idx]);
        let score_upper_bound =
            (stat * (self.no_skill_by_chart[chart_idx] + normal_meta + captain_meta)).floor();

        score_upper_bound + self.other_song_upper_bounds[chart_idx] > current_best
    }
}

fn card_upper_bound(
    card: &MedleyCardInput<'_>,
    _charts: &[Chart],
    profiles: &[MedleyCardPruneProfile],
    signature: MedleyPruneSignature,
) -> Option<CardUpperBound> {
    let profile = profiles.get(card.raw_index)?;
    let score_up = card
        .card
        .score_up
        .resolve(signature.team_band_id(), signature.team_attribute());
    let values = profile.skill_meta_for_score_up(score_up)?;
    let mut normal_meta_by_chart = [0.0; MEDLEY_TEAM_COUNT];
    let mut captain_meta_by_chart = [0.0; MEDLEY_TEAM_COUNT];

    for chart_idx in 0..MEDLEY_TEAM_COUNT {
        let start = chart_idx * (TEAM_SIZE + 1);
        let end = start + TEAM_SIZE;
        normal_meta_by_chart[chart_idx] =
            values.get(start..end)?.iter().copied().fold(0.0, f64::max);
        captain_meta_by_chart[chart_idx] = *values.get(end)?;
    }

    Some(CardUpperBound {
        stat: card.stat,
        normal_meta_by_chart,
        captain_meta_by_chart,
    })
}

fn suffix_upper_bounds<'a>(
    cards: &'a [MedleyCardInput<'_>],
    _charts: &'a [Chart],
    groups: &'a FlatCharacterGroups,
    card_bounds: &'a [CardUpperBound],
) -> impl Iterator<Item = SuffixUpperBound> + 'a {
    (0..=groups.len()).map(move |start| {
        let mut stat_top = TopValues::default();
        let mut normal_top_by_chart = [TopValues::default(); MEDLEY_TEAM_COUNT];
        let mut captain_meta_by_chart = [0.0_f64; MEDLEY_TEAM_COUNT];

        for group_idx in start..groups.len() {
            let mut group_stat = 0.0_f64;
            let mut group_normal_by_chart = [0.0_f64; MEDLEY_TEAM_COUNT];

            for flat_pos in groups.group_flat_range(group_idx) {
                let card_idx = groups.card_indices[flat_pos];
                let card = &cards[card_idx];
                let bound = &card_bounds[card_idx];
                group_stat = group_stat.max(bound.stat.max(card.stat));
                for chart_idx in 0..MEDLEY_TEAM_COUNT {
                    group_normal_by_chart[chart_idx] =
                        group_normal_by_chart[chart_idx].max(bound.normal_meta_by_chart[chart_idx]);
                    captain_meta_by_chart[chart_idx] = captain_meta_by_chart[chart_idx]
                        .max(bound.captain_meta_by_chart[chart_idx]);
                }
            }

            stat_top.push(group_stat);
            for chart_idx in 0..MEDLEY_TEAM_COUNT {
                normal_top_by_chart[chart_idx].push(group_normal_by_chart[chart_idx]);
            }
        }

        SuffixUpperBound {
            remaining_groups: groups.len() - start,
            stat_by_slots: stat_top.prefix_sums(),
            normal_meta_by_chart_and_slots: normal_top_by_chart.map(|top| top.prefix_sums()),
            captain_meta_by_chart,
        }
    })
}

#[derive(Debug, Clone, Copy)]
struct TopValues {
    values: [f64; TEAM_SIZE],
}

impl Default for TopValues {
    fn default() -> Self {
        Self {
            values: [0.0; TEAM_SIZE],
        }
    }
}

impl TopValues {
    fn push(&mut self, value: f64) {
        if value <= self.values[TEAM_SIZE - 1] {
            return;
        }

        self.values[TEAM_SIZE - 1] = value;
        let mut idx = TEAM_SIZE - 1;
        while idx > 0 && self.values[idx] > self.values[idx - 1] {
            self.values.swap(idx, idx - 1);
            idx -= 1;
        }
    }

    fn prefix_sums(self) -> [f64; TEAM_SIZE + 1] {
        let mut sums = [0.0; TEAM_SIZE + 1];
        for idx in 0..TEAM_SIZE {
            sums[idx + 1] = sums[idx] + self.values[idx];
        }
        sums
    }
}

pub(in crate::medley) fn compact_raw_candidate_masks(
    raw_candidates: Vec<RawTeamCandidate>,
    cards: &[PreparedCard],
    chart_count: usize,
) -> Result<Vec<TeamCandidate>, TeamBuildError> {
    let mut used_cards = BTreeSet::new();
    for candidate in &raw_candidates {
        for raw_idx in candidate.raw_indices {
            used_cards.insert(raw_idx);
        }
    }
    let mut masks_by_raw_index = BTreeMap::new();
    let word_count = used_cards.len().div_ceil(u64::BITS as usize).max(1);
    for (bit_idx, raw_idx) in used_cards.into_iter().enumerate() {
        masks_by_raw_index.insert(raw_idx, bit_idx);
    }

    let mut result = Vec::with_capacity(raw_candidates.len());
    for raw_candidate in raw_candidates {
        let mut mask_words = vec![0u64; word_count];
        for raw_idx in raw_candidate.raw_indices {
            let bit_idx = masks_by_raw_index[&raw_idx];
            mask_words[bit_idx / u64::BITS as usize] |= 1u64 << (bit_idx % u64::BITS as usize);
        }
        let (mask, mask_words) = if word_count == 1 {
            (mask_words[0], Vec::new())
        } else {
            (0, mask_words)
        };
        let ordered_team_card_ids = (0..chart_count)
            .map(|chart_idx| {
                raw_candidate.ordered_raw_indices[chart_idx]
                    .iter()
                    .map(|&raw_idx| cards[raw_idx].card_id)
                    .collect()
            })
            .collect();
        let captain_card_ids = (0..chart_count)
            .map(|chart_idx| cards[raw_candidate.captain_raw_indices[chart_idx]].card_id)
            .collect();
        let scores = raw_candidate.scores[..chart_count].to_vec();
        let team_card_ids = raw_candidate
            .raw_indices
            .iter()
            .map(|&raw_idx| cards[raw_idx].card_id)
            .collect();

        result.push(TeamCandidate {
            mask,
            mask_words,
            team_card_ids,
            ordered_team_card_ids: Some(ordered_team_card_ids),
            captain_card_ids,
            scores,
            stat: raw_candidate.stat,
        });
    }

    Ok(result)
}

pub(in crate::medley) enum RawCandidateSolverInput {
    Narrow {
        input: MedleySolverInput,
        used_card_count: usize,
    },
    Wide {
        input: WideMedleySolverInput,
        used_card_count: usize,
    },
}

pub(in crate::medley) fn raw_candidate_solver_input_for_indices(
    raw_candidates: &[RawTeamCandidate],
    current_best: i32,
    candidate_indices: &[usize],
) -> RawCandidateSolverInput {
    let mut used_cards = BTreeSet::new();
    for &candidate_idx in candidate_indices {
        for raw_idx in raw_candidates[candidate_idx].raw_indices {
            used_cards.insert(raw_idx);
        }
    }
    let used_card_count = used_cards.len();

    let mut masks_by_raw_index = BTreeMap::new();
    for (bit_idx, raw_idx) in used_cards.into_iter().enumerate() {
        masks_by_raw_index.insert(raw_idx, bit_idx);
    }

    if used_card_count <= u64::BITS as usize {
        let mut team_masks = Vec::with_capacity(candidate_indices.len());
        let mut scores = Vec::with_capacity(candidate_indices.len());
        for &candidate_idx in candidate_indices {
            let raw_candidate = &raw_candidates[candidate_idx];
            let mut mask = 0u64;
            for raw_idx in raw_candidate.raw_indices {
                let bit_idx = masks_by_raw_index[&raw_idx];
                mask |= 1u64 << bit_idx;
            }
            team_masks.push(mask as TeamMask);
            scores.push(raw_candidate.scores);
        }

        return RawCandidateSolverInput::Narrow {
            input: MedleySolverInput {
                current_best,
                team_masks,
                scores,
            },
            used_card_count,
        };
    }

    let word_count = used_card_count.div_ceil(u64::BITS as usize).max(1);
    let mut team_masks = Vec::with_capacity(candidate_indices.len());
    let mut scores = Vec::with_capacity(candidate_indices.len());
    for &candidate_idx in candidate_indices {
        let raw_candidate = &raw_candidates[candidate_idx];
        let mut mask_words = vec![0u64; word_count];
        for raw_idx in raw_candidate.raw_indices {
            let bit_idx = masks_by_raw_index[&raw_idx];
            mask_words[bit_idx / u64::BITS as usize] |= 1u64 << (bit_idx % u64::BITS as usize);
        }
        team_masks.push(mask_words);
        scores.push(raw_candidate.scores);
    }

    RawCandidateSolverInput::Wide {
        input: WideMedleySolverInput {
            current_best,
            team_masks,
            scores,
        },
        used_card_count,
    }
}

pub(in crate::medley) fn raw_candidate_used_card_count(
    raw_candidates: &[RawTeamCandidate],
) -> usize {
    let mut used_cards = BTreeSet::new();
    for candidate in raw_candidates {
        for raw_idx in candidate.raw_indices {
            used_cards.insert(raw_idx);
        }
    }
    used_cards.len()
}

fn active_medley_cards<'a>(
    cards: &'a [PreparedCard],
    card_stats: &[f64],
    active_card_indices: &[usize],
) -> Vec<MedleyCardInput<'a>> {
    active_card_indices
        .iter()
        .copied()
        .map(|card_idx| MedleyCardInput {
            card: &cards[card_idx],
            raw_index: card_idx,
            stat: card_stats[card_idx],
        })
        .collect()
}

fn avx2_available() -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        std::is_x86_feature_detected!("avx2")
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        false
    }
}

#[derive(Debug, Clone)]
struct FlatCharacterGroups {
    card_indices: Vec<usize>,
    group_indices: Vec<usize>,
    group_offsets: Vec<usize>,
    card_band_ids: Vec<u32>,
    card_attributes: Vec<crate::model::schema::Attribute>,
    card_band_id_codes: Vec<i32>,
    card_attribute_codes: Vec<i32>,
}

impl FlatCharacterGroups {
    fn len(&self) -> usize {
        self.group_offsets.len().saturating_sub(1)
    }

    fn offset(&self, group_idx: usize) -> usize {
        self.group_offsets
            .get(group_idx)
            .copied()
            .unwrap_or(self.card_indices.len())
    }

    fn group_flat_range(&self, group_idx: usize) -> std::ops::Range<usize> {
        self.group_offsets[group_idx]..self.group_offsets[group_idx + 1]
    }
}

fn character_groups(cards: &[MedleyCardInput<'_>]) -> FlatCharacterGroups {
    let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (idx, card) in cards.iter().enumerate() {
        groups.entry(card.card.character_id).or_default().push(idx);
    }

    let groups = groups.into_values().collect::<Vec<_>>();
    let mut card_indices = Vec::with_capacity(cards.len());
    let mut group_indices = Vec::with_capacity(cards.len());
    let mut group_offsets = Vec::with_capacity(groups.len() + 1);
    group_offsets.push(0);
    for (group_idx, group) in groups.into_iter().enumerate() {
        for card_idx in group {
            card_indices.push(card_idx);
            group_indices.push(group_idx);
        }
        group_offsets.push(card_indices.len());
    }

    FlatCharacterGroups {
        card_indices,
        group_indices,
        group_offsets,
        card_band_ids: cards.iter().map(|card| card.card.band_id).collect(),
        card_attributes: cards.iter().map(|card| card.card.attribute).collect(),
        card_band_id_codes: cards.iter().map(|card| card.card.band_id as i32).collect(),
        card_attribute_codes: cards
            .iter()
            .map(|card| attribute_code(card.card.attribute))
            .collect(),
    }
}

fn attribute_code(attribute: crate::model::schema::Attribute) -> i32 {
    match attribute {
        crate::model::schema::Attribute::Cool => 0,
        crate::model::schema::Attribute::Happy => 1,
        crate::model::schema::Attribute::Pure => 2,
        crate::model::schema::Attribute::Powerful => 3,
        crate::model::schema::Attribute::All => 4,
    }
}
