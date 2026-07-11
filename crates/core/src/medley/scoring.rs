use super::prune::MedleyPruneSignature;
use super::team::{TeamBuildError, TeamGenerationOptions};
use crate::model::chart::{
    Chart, ExactScoreScratch, ExactSkillOrderProfile, ExactSkillWindow, TeamCardSkill,
};
use crate::model::preparation::PreparedCard;
use crate::model::schema::Attribute;
use crate::timing::Timer;
use std::collections::HashMap;

const TEAM_SIZE: usize = 5;
const MEDLEY_TEAM_COUNT: usize = 3;

#[derive(Debug, Clone, Copy)]
pub(in crate::medley) struct MedleyCardInput<'a> {
    pub(in crate::medley) card: &'a PreparedCard,
    pub(in crate::medley) raw_index: usize,
    pub(in crate::medley) stat: f64,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::medley) struct ResolvedMedleyCardInput {
    raw_index: usize,
    stat: f64,
    band_id: u32,
    attribute: Attribute,
    skill: TeamCardSkill,
    skill_meta_by_chart: [[f64; TEAM_SIZE + 1]; MEDLEY_TEAM_COUNT],
    skill_windows_by_chart: [[ExactSkillWindow; TEAM_SIZE + 1]; MEDLEY_TEAM_COUNT],
}

#[derive(Debug, Clone)]
pub(crate) struct RawTeamCandidate {
    pub(crate) raw_indices: [usize; TEAM_SIZE],
    pub(crate) ordered_raw_indices: [[usize; TEAM_SIZE]; MEDLEY_TEAM_COUNT],
    pub(crate) captain_raw_indices: [usize; MEDLEY_TEAM_COUNT],
    pub(crate) scores: [i32; MEDLEY_TEAM_COUNT],
    pub(crate) stat: i32,
}

#[derive(Debug, Default, Clone, Copy)]
pub(in crate::medley) struct ResolvedCandidateBuildProfile {
    pub(in crate::medley) samples: usize,
    pub(in crate::medley) total_ms: f64,
    pub(in crate::medley) stat_ms: f64,
    pub(in crate::medley) prepare_ms: f64,
    pub(in crate::medley) seed_ms: f64,
    pub(in crate::medley) order_ms: f64,
    pub(in crate::medley) result_ms: f64,
    pub(in crate::medley) finalize_ms: f64,
    pub(in crate::medley) order_by_chart_ms: [f64; MEDLEY_TEAM_COUNT],
    pub(in crate::medley) order_detail: ExactSkillOrderProfile,
}

impl ResolvedCandidateBuildProfile {
    pub(in crate::medley) fn add(&mut self, other: &Self) {
        self.samples += other.samples;
        self.total_ms += other.total_ms;
        self.stat_ms += other.stat_ms;
        self.prepare_ms += other.prepare_ms;
        self.seed_ms += other.seed_ms;
        self.order_ms += other.order_ms;
        self.result_ms += other.result_ms;
        self.finalize_ms += other.finalize_ms;
        for chart_idx in 0..MEDLEY_TEAM_COUNT {
            self.order_by_chart_ms[chart_idx] += other.order_by_chart_ms[chart_idx];
        }
        self.order_detail.add(&other.order_detail);
    }
}

pub(in crate::medley) fn selected_resolved_team_signature(
    cards: &[ResolvedMedleyCardInput],
    selected_indices: &[usize],
) -> MedleyPruneSignature {
    let [idx0, idx1, idx2, idx3, idx4]: [usize; TEAM_SIZE] = selected_indices
        .try_into()
        .expect("team signature is called only for full teams");

    let first_band = cards[idx0].band_id;
    let same_band = cards[idx1].band_id == first_band
        && cards[idx2].band_id == first_band
        && cards[idx3].band_id == first_band
        && cards[idx4].band_id == first_band;
    let first_attribute = cards[idx0].attribute;
    let same_attribute = cards[idx1].attribute == first_attribute
        && cards[idx2].attribute == first_attribute
        && cards[idx3].attribute == first_attribute
        && cards[idx4].attribute == first_attribute;

    match (same_band, same_attribute) {
        (true, true) => MedleyPruneSignature::UnifiedBandAttribute(first_band, first_attribute),
        (true, false) => MedleyPruneSignature::UnifiedBand(first_band),
        (false, true) => MedleyPruneSignature::UnifiedAttribute(first_attribute),
        (false, false) => MedleyPruneSignature::Mixed,
    }
}

pub(in crate::medley) fn resolve_medley_cards_for_signature(
    cards: &[MedleyCardInput<'_>],
    charts: &[Chart],
    signature: MedleyPruneSignature,
    skill_meta_cache: &mut SkillMetaCache,
) -> Result<Vec<ResolvedMedleyCardInput>, TeamBuildError> {
    cards
        .iter()
        .map(|card| {
            let skill = TeamCardSkill {
                card_id: card.card.card_id,
                duration: card.card.skill.duration,
                score_up: card
                    .card
                    .score_up
                    .resolve(signature.team_band_id(), signature.team_attribute()),
                rateup: card.card.skill.rateup,
            };
            let mut skill_meta_by_chart = [[0.0; TEAM_SIZE + 1]; MEDLEY_TEAM_COUNT];
            let mut skill_windows_by_chart =
                [[ExactSkillWindow::default(); TEAM_SIZE + 1]; MEDLEY_TEAM_COUNT];
            for (chart_idx, chart) in charts.iter().enumerate() {
                skill_meta_by_chart[chart_idx] =
                    skill_meta_cache.values(chart_idx, chart, skill)?;
                for activation in 0..=TEAM_SIZE {
                    skill_windows_by_chart[chart_idx][activation] =
                        chart.compile_exact_skill_window(activation, skill)?;
                }
            }

            Ok(ResolvedMedleyCardInput {
                raw_index: card.raw_index,
                stat: card.stat,
                band_id: card.card.band_id,
                attribute: card.card.attribute,
                skill,
                skill_meta_by_chart,
                skill_windows_by_chart,
            })
        })
        .collect()
}

pub(in crate::medley) fn build_resolved_candidate(
    cards: &[ResolvedMedleyCardInput],
    charts: &[Chart],
    options: TeamGenerationOptions,
    selected_indices: &[usize; TEAM_SIZE],
    scratch: &mut ExactScoreScratch,
) -> Result<RawTeamCandidate, TeamBuildError> {
    build_resolved_candidate_internal::<false>(
        cards,
        charts,
        options,
        selected_indices,
        scratch,
        None,
    )
}

pub(in crate::medley) fn build_resolved_candidate_profiled(
    cards: &[ResolvedMedleyCardInput],
    charts: &[Chart],
    options: TeamGenerationOptions,
    selected_indices: &[usize; TEAM_SIZE],
    scratch: &mut ExactScoreScratch,
    profile: &mut ResolvedCandidateBuildProfile,
) -> Result<RawTeamCandidate, TeamBuildError> {
    build_resolved_candidate_internal::<true>(
        cards,
        charts,
        options,
        selected_indices,
        scratch,
        Some(profile),
    )
}

fn build_resolved_candidate_internal<const PROFILE: bool>(
    cards: &[ResolvedMedleyCardInput],
    charts: &[Chart],
    options: TeamGenerationOptions,
    selected_indices: &[usize; TEAM_SIZE],
    scratch: &mut ExactScoreScratch,
    mut profile: Option<&mut ResolvedCandidateBuildProfile>,
) -> Result<RawTeamCandidate, TeamBuildError> {
    let total_start = PROFILE.then(Timer::start);
    let stage_start = PROFILE.then(Timer::start);
    let selected_cards = selected_indices.map(|index| &cards[index]);
    let stat_floor = crate::floor_team_stat(selected_cards.iter().map(|card| card.stat));
    let team = selected_cards.map(|card| card.skill);
    if let (Some(profile), Some(start)) = (profile.as_deref_mut(), stage_start) {
        profile.stat_ms += start.elapsed_ms();
    }

    let mut scores = [0; MEDLEY_TEAM_COUNT];
    let mut captain_raw_indices = [0; MEDLEY_TEAM_COUNT];
    let mut ordered_raw_indices = [[0; TEAM_SIZE]; MEDLEY_TEAM_COUNT];

    for (chart_idx, chart) in charts.iter().enumerate() {
        let stage_start = PROFILE.then(Timer::start);
        let skill_meta = selected_cards.map(|card| card.skill_meta_by_chart[chart_idx]);
        let skill_windows = selected_cards.map(|card| card.skill_windows_by_chart[chart_idx]);
        if let (Some(profile), Some(start)) = (profile.as_deref_mut(), stage_start) {
            profile.prepare_ms += start.elapsed_ms();
        }

        let stage_start = PROFILE.then(Timer::start);
        let seed = max_meta_order_for_team(&skill_meta);
        if let (Some(profile), Some(start)) = (profile.as_deref_mut(), stage_start) {
            profile.seed_ms += start.elapsed_ms();
        }
        let stage_start = PROFILE.then(Timer::start);
        let order = if PROFILE {
            let profile = profile
                .as_deref_mut()
                .expect("profile exists in the profiled candidate builder");
            chart.get_independent_medley_score_order_from_exact_windows_profiled(
                &team,
                stat_floor,
                options.score_as_medley,
                seed.order_indices,
                seed.captain_index,
                &skill_windows,
                scratch,
                &mut profile.order_detail,
            )?
        } else {
            chart.get_independent_medley_score_order_from_exact_windows(
                &team,
                stat_floor,
                options.score_as_medley,
                seed.order_indices,
                seed.captain_index,
                &skill_windows,
                scratch,
            )?
        };
        if let (Some(profile), Some(start)) = (profile.as_deref_mut(), stage_start) {
            let elapsed = start.elapsed_ms();
            profile.order_ms += elapsed;
            profile.order_by_chart_ms[chart_idx] += elapsed;
        }

        let stage_start = PROFILE.then(Timer::start);
        scores[chart_idx] = order.score;
        captain_raw_indices[chart_idx] = selected_cards[order.captain_index].raw_index;
        ordered_raw_indices[chart_idx] =
            order.order_indices.map(|idx| selected_cards[idx].raw_index);
        if let (Some(profile), Some(start)) = (profile.as_deref_mut(), stage_start) {
            profile.result_ms += start.elapsed_ms();
        }
    }

    let stage_start = PROFILE.then(Timer::start);
    let candidate = RawTeamCandidate {
        raw_indices: selected_cards.map(|card| card.raw_index),
        ordered_raw_indices,
        captain_raw_indices,
        scores,
        stat: stat_floor,
    };
    if let (Some(profile), Some(start)) = (profile.as_deref_mut(), stage_start) {
        profile.finalize_ms += start.elapsed_ms();
    }
    if let (Some(profile), Some(start)) = (profile.as_deref_mut(), total_start) {
        profile.samples += 1;
        profile.total_ms += start.elapsed_ms();
    }
    Ok(candidate)
}

pub(in crate::medley) fn build_candidate(
    cards: &[MedleyCardInput<'_>],
    charts: &[Chart],
    options: TeamGenerationOptions,
    selected_indices: &[usize],
    skill_meta_cache: &mut SkillMetaCache,
    exact_score_scratch: &mut ExactScoreScratch,
) -> Result<RawTeamCandidate, TeamBuildError> {
    let selected_indices: [usize; TEAM_SIZE] = selected_indices
        .try_into()
        .expect("candidate build is called only for full teams");
    let team_band_id = unified_band_id(cards, &selected_indices);
    let team_attribute = unified_attribute(cards, &selected_indices);
    let resolved_skills =
        resolve_team_skills(cards, &selected_indices, team_band_id, team_attribute);
    let stat_floor =
        crate::floor_team_stat(selected_indices.iter().map(|&index| cards[index].stat));

    let mut scores = [0; MEDLEY_TEAM_COUNT];
    let mut captain_raw_indices = [0; MEDLEY_TEAM_COUNT];
    let mut ordered_raw_indices = [[0; TEAM_SIZE]; MEDLEY_TEAM_COUNT];

    for (chart_idx, chart) in charts.iter().enumerate() {
        let skill_meta = selected_skill_meta(
            chart_idx,
            chart,
            cards,
            &selected_indices,
            &resolved_skills,
            skill_meta_cache,
        )?;
        let seed = max_meta_order_for_team(&skill_meta);
        let mut skill_windows = [[ExactSkillWindow::default(); TEAM_SIZE + 1]; TEAM_SIZE];
        for card_idx in 0..TEAM_SIZE {
            for activation in 0..=TEAM_SIZE {
                skill_windows[card_idx][activation] =
                    chart.compile_exact_skill_window(activation, resolved_skills[card_idx])?;
            }
        }
        let order = chart.get_independent_medley_score_order_from_exact_windows(
            &resolved_skills,
            stat_floor,
            options.score_as_medley,
            seed.order_indices,
            seed.captain_index,
            &skill_windows,
            exact_score_scratch,
        )?;

        scores[chart_idx] = order.score;
        captain_raw_indices[chart_idx] = cards[selected_indices[order.captain_index]].raw_index;
        ordered_raw_indices[chart_idx] = order
            .order_indices
            .map(|idx| cards[selected_indices[idx]].raw_index);
    }

    Ok(RawTeamCandidate {
        raw_indices: selected_indices.map(|index| cards[index].raw_index),
        ordered_raw_indices,
        captain_raw_indices,
        scores,
        stat: stat_floor,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MedleySkillOrder {
    order_indices: [usize; TEAM_SIZE],
    captain_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SkillMetaCacheKey {
    card_id: u32,
    score_up_bits: u64,
}

pub(in crate::medley) struct SkillMetaCache {
    by_chart: Vec<HashMap<SkillMetaCacheKey, [f64; TEAM_SIZE + 1]>>,
}

impl SkillMetaCache {
    pub(in crate::medley) fn new(chart_count: usize) -> Self {
        Self {
            by_chart: (0..chart_count).map(|_| HashMap::new()).collect(),
        }
    }

    pub(in crate::medley) fn entry_count(&self) -> usize {
        self.by_chart.iter().map(HashMap::len).sum()
    }

    pub(in crate::medley) fn values(
        &mut self,
        chart_idx: usize,
        chart: &Chart,
        skill: TeamCardSkill,
    ) -> Result<[f64; TEAM_SIZE + 1], TeamBuildError> {
        let key = SkillMetaCacheKey {
            card_id: skill.card_id,
            score_up_bits: skill.score_up.to_bits(),
        };
        if let Some(values) = self.by_chart[chart_idx].get(&key) {
            return Ok(*values);
        }

        let mut values = [0.0; TEAM_SIZE + 1];
        for (activation, value) in values.iter_mut().enumerate() {
            *value = chart.skill_meta_value(activation, skill)?;
        }
        self.by_chart[chart_idx].insert(key, values);
        Ok(values)
    }
}

fn selected_skill_meta(
    chart_idx: usize,
    chart: &Chart,
    cards: &[MedleyCardInput<'_>],
    selected_indices: &[usize; TEAM_SIZE],
    team: &[TeamCardSkill; TEAM_SIZE],
    skill_meta_cache: &mut SkillMetaCache,
) -> Result<[[f64; TEAM_SIZE + 1]; TEAM_SIZE], TeamBuildError> {
    let mut skill_meta = [[0.0; TEAM_SIZE + 1]; TEAM_SIZE];
    for card_idx in 0..TEAM_SIZE {
        let card_id = cards[selected_indices[card_idx]].card.card_id;
        let skill = TeamCardSkill {
            card_id,
            ..team[card_idx]
        };
        skill_meta[card_idx] = skill_meta_cache.values(chart_idx, chart, skill)?;
    }

    Ok(skill_meta)
}

fn max_meta_order_for_team(skill_meta: &[[f64; TEAM_SIZE + 1]; TEAM_SIZE]) -> MedleySkillOrder {
    if skill_meta[1..].iter().all(|row| row == &skill_meta[0]) {
        return MedleySkillOrder {
            order_indices: [0, 1, 2, 3, 4],
            captain_index: 0,
        };
    }

    let mut dp = [f64::NEG_INFINITY; 1 << TEAM_SIZE];
    let mut choose = [0usize; 1 << TEAM_SIZE];
    dp[0] = 0.0;

    for mask in 0..(1usize << TEAM_SIZE) - 1 {
        let activation = FIVE_CARD_MASK_POPCOUNT[mask] as usize;
        let mut available = (!mask) & ((1 << TEAM_SIZE) - 1);
        while available != 0 {
            let card_idx = available.trailing_zeros() as usize;
            available &= available - 1;
            let card_meta = &skill_meta[card_idx];
            let value = dp[mask] + card_meta[activation];
            let next_mask = mask | (1 << card_idx);
            if value > dp[next_mask] {
                dp[next_mask] = value;
                choose[next_mask] = card_idx;
            }
        }
    }

    let mut captain_index = 0;
    let mut captain_meta = f64::NEG_INFINITY;
    for (card_idx, card_meta) in skill_meta.iter().enumerate() {
        let value = card_meta[TEAM_SIZE];
        if value > captain_meta {
            captain_meta = value;
            captain_index = card_idx;
        }
    }

    let mut order_indices = [0usize; TEAM_SIZE];
    let mut mask = (1usize << TEAM_SIZE) - 1;
    for slot in (0..TEAM_SIZE).rev() {
        let card_idx = choose[mask];
        order_indices[slot] = card_idx;
        mask ^= 1 << card_idx;
    }

    MedleySkillOrder {
        order_indices,
        captain_index,
    }
}

const FIVE_CARD_MASK_POPCOUNT: [u8; 32] = [
    0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4, 1, 2, 2, 3, 2, 3, 3, 4, 2, 3, 3, 4, 3, 4, 4, 5,
];

fn resolve_team_skills(
    cards: &[MedleyCardInput<'_>],
    selected_indices: &[usize; TEAM_SIZE],
    team_band_id: Option<u32>,
    team_attribute: Option<Attribute>,
) -> [TeamCardSkill; TEAM_SIZE] {
    std::array::from_fn(|idx| {
        let card = cards[selected_indices[idx]].card;
        TeamCardSkill {
            card_id: card.card_id,
            duration: card.skill.duration,
            score_up: card.score_up.resolve(team_band_id, team_attribute),
            rateup: card.skill.rateup,
        }
    })
}

fn unified_band_id(
    cards: &[MedleyCardInput<'_>],
    selected_indices: &[usize; TEAM_SIZE],
) -> Option<u32> {
    let first = cards[selected_indices[0]].card.band_id;
    selected_indices
        .iter()
        .all(|&index| cards[index].card.band_id == first)
        .then_some(first)
}

fn unified_attribute(
    cards: &[MedleyCardInput<'_>],
    selected_indices: &[usize; TEAM_SIZE],
) -> Option<Attribute> {
    let first = cards[selected_indices[0]].card.attribute;
    selected_indices
        .iter()
        .all(|&index| cards[index].card.attribute == first)
        .then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medley::test_support::{prepared_card, selected_cool_items};
    use crate::model::chart::{ChartNode, ChartNodeType};
    use crate::model::preparation::{AreaItemPercent, ScoreUp};

    #[derive(Clone, Copy)]
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0
        }

        fn usize(&mut self, upper: usize) -> usize {
            (self.next() as usize) % upper
        }

        fn f64(&mut self, low: f64, high: f64) -> f64 {
            let unit = (self.next() >> 11) as f64 / ((1_u64 << 53) as f64);
            low + unit * (high - low)
        }
    }

    #[test]
    fn exact_order_fixes_overlapping_meta_order_regression() {
        let mut rng = Rng(0x99aa_77cc_55ee_3311);
        let mut chart = overlapping_chart(&mut rng);
        chart.init(0, true).unwrap();
        let (team, stat) = random_team(&mut rng);

        let legacy_score = meta_order_score(&chart, &team, stat);
        let skill_meta = std::array::from_fn(|card_idx| {
            std::array::from_fn(|activation| {
                chart.skill_meta_value(activation, team[card_idx]).unwrap()
            })
        });
        let skill_windows = std::array::from_fn(|card_idx| {
            std::array::from_fn(|activation| {
                chart
                    .compile_exact_skill_window(activation, team[card_idx])
                    .unwrap()
            })
        });
        let seed = max_meta_order_for_team(&skill_meta);
        let fast = chart
            .get_independent_medley_score_order_from_exact_windows(
                &team,
                stat,
                true,
                seed.order_indices,
                seed.captain_index,
                &skill_windows,
                &mut ExactScoreScratch::default(),
            )
            .unwrap();
        let exact = chart.get_max_score_order(&team, stat, true).unwrap();
        let brute = brute_force_order_score(&chart, &team, stat);
        let independent = brute_force_independent_order_score(&chart, &team, stat);

        assert_ne!(legacy_score, brute);
        assert_eq!(fast.score, independent);
        assert_eq!(exact.score, brute);
    }

    #[test]
    fn exact_order_fixes_nonoverlapping_integer_floor_regression() {
        let mut rng = Rng(0x99aa_77cc_55ee_3311);
        let mut regression = None;
        for _ in 0..5_000 {
            let chart = nonoverlapping_chart(&mut rng);
            let team = random_team(&mut rng);
            let mut initialized = chart;
            initialized.init(0, true).unwrap();
            let legacy_score = meta_order_score(&initialized, &team.0, team.1);
            let brute = brute_force_order_score(&initialized, &team.0, team.1);
            if legacy_score != brute {
                regression = Some((initialized, team, legacy_score, brute));
                break;
            }
        }
        let (chart, (team, stat), legacy_score, brute) =
            regression.expect("expected a non-overlapping meta floor regression");
        let exact = chart.get_max_score_order(&team, stat, true).unwrap();

        assert_ne!(legacy_score, brute);
        assert_eq!(exact.score, brute);
    }

    #[test]
    fn public_full_matrix_order_matches_brute_force() {
        let mut rng = Rng(0x4f6b_9d21_37aa_c805);
        for _ in 0..100 {
            let mut chart = nonoverlapping_chart(&mut rng);
            chart.init(0, true).unwrap();
            let (mut team, stat) = random_team(&mut rng);
            team[0].duration = 5.0;
            team[0].score_up = 1.0;
            team[0].rateup = true;

            let exact = chart.get_max_score_order(&team, stat, true).unwrap();
            let brute = brute_force_order_score(&chart, &team, stat);
            assert_eq!(exact.score, brute);
        }
    }

    #[test]
    fn full_exact_matrix_matches_brute_force_with_rateup() {
        let mut rng = Rng(0x7281_9ac4_53de_b60f);
        let mut scratch = ExactScoreScratch::default();
        for _ in 0..100 {
            let mut chart = nonoverlapping_chart(&mut rng);
            chart.init(0, true).unwrap();
            let (mut team, stat) = random_team(&mut rng);
            team[0].duration = 5.0;
            team[0].score_up = 1.0;
            team[0].rateup = true;
            let skill_meta = std::array::from_fn(|card_idx| {
                std::array::from_fn(|activation| {
                    chart.skill_meta_value(activation, team[card_idx]).unwrap()
                })
            });
            let skill_windows = std::array::from_fn(|card_idx| {
                std::array::from_fn(|activation| {
                    chart
                        .compile_exact_skill_window(activation, team[card_idx])
                        .unwrap()
                })
            });
            let seed = max_meta_order_for_team(&skill_meta);

            let exact = chart
                .get_max_score_order_from_exact_windows(
                    &team,
                    stat,
                    true,
                    seed.order_indices,
                    seed.captain_index,
                    &skill_windows,
                    &mut scratch,
                )
                .unwrap();
            let brute = brute_force_order_score(&chart, &team, stat);

            assert_eq!(exact.score, brute);
        }
    }

    #[test]
    fn exact_order_handles_rateup_skill_and_captain_jointly() {
        let mut chart = overlapping_chart(&mut Rng(0x1234_5678_90ab_cdef));
        chart.init(0, true).unwrap();
        let team = [
            TeamCardSkill {
                card_id: 1,
                duration: 5.0,
                score_up: 1.0,
                rateup: true,
            },
            TeamCardSkill {
                card_id: 2,
                duration: 7.0,
                score_up: 1.65,
                rateup: false,
            },
            TeamCardSkill {
                card_id: 3,
                duration: 4.5,
                score_up: 1.45,
                rateup: false,
            },
            TeamCardSkill {
                card_id: 4,
                duration: 6.5,
                score_up: 1.0,
                rateup: true,
            },
            TeamCardSkill {
                card_id: 5,
                duration: 8.0,
                score_up: 1.2,
                rateup: false,
            },
        ];
        let stat = 280_000;

        let exact = chart.get_max_score_order(&team, stat, true).unwrap();
        let brute = brute_force_order_score(&chart, &team, stat);

        assert_eq!(exact.score, brute);
    }

    #[test]
    fn overlapping_order_uses_independent_matrix_with_resolved_unification_skill() {
        let mut chart = overlapping_chart(&mut Rng(0x0fed_cba9_8765_4321));
        chart.init(0, true).unwrap();
        let mut cards = Vec::from(std::array::from_fn::<_, 5, _>(|idx| {
            prepared_card(idx as u32 + 1, idx as u32 + 1, 1, Attribute::Cool)
        }));
        for (idx, card) in cards.iter_mut().enumerate() {
            card.skill.duration = [4.0, 5.0, 6.0, 7.0, 8.0][idx];
            card.skill.score_up = 0.7 + idx as f64 * 0.15;
            card.score_up.default = card.skill.score_up;
        }
        cards[0].score_up = ScoreUp {
            default: 0.7,
            unification_activate_effect_value: Some(1.9),
            unification_activate_condition_band_id: Some(1),
            unification_activate_condition_type: Some(Attribute::Cool),
        };
        let mut rateup_alternative = prepared_card(6, 1, 2, Attribute::Cool);
        rateup_alternative.skill.duration = 6.0;
        rateup_alternative.skill.score_up = 1.0;
        rateup_alternative.skill.rateup = true;
        rateup_alternative.score_up.default = 1.0;
        cards.push(rateup_alternative);

        let candidates = crate::medley::team::build_team_candidates(
            &cards,
            std::slice::from_ref(&chart),
            &AreaItemPercent::empty(),
            &selected_cool_items(),
            TeamGenerationOptions {
                score_as_medley: true,
                max_candidates: usize::MAX,
            },
        )
        .unwrap();
        assert_eq!(candidates.len(), 2);
        for candidate in &candidates {
            let selected = candidate
                .team_card_ids
                .iter()
                .map(|card_id| cards.iter().find(|card| card.card_id == *card_id).unwrap())
                .collect::<Vec<_>>();
            let team_band = selected
                .iter()
                .all(|card| card.band_id == selected[0].band_id)
                .then_some(selected[0].band_id);
            let team_attribute = Some(Attribute::Cool);
            let resolved_team: [TeamCardSkill; 5] = selected
                .iter()
                .map(|card| TeamCardSkill {
                    score_up: card.score_up.resolve(team_band, team_attribute),
                    ..card.skill
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();
            let exact = brute_force_independent_order_score(&chart, &resolved_team, candidate.stat);

            if candidate.team_card_ids.contains(&1) {
                assert_eq!(
                    resolved_team
                        .iter()
                        .find(|skill| skill.card_id == 1)
                        .unwrap()
                        .score_up,
                    1.9
                );
            }
            assert_eq!(candidate.scores[0], exact);
        }
    }

    fn meta_order_score(chart: &Chart, team: &[TeamCardSkill; 5], stat: i32) -> i32 {
        let order = chart.get_max_meta_order(team).unwrap();
        let skills: [TeamCardSkill; 6] = std::array::from_fn(|activation| {
            if activation == TEAM_SIZE {
                team[order.captain_index]
            } else {
                team[order.order_indices[activation]]
            }
        });
        chart.get_score_for_six_skills(&skills, stat, true).unwrap()
    }

    fn brute_force_order_score(chart: &Chart, team: &[TeamCardSkill; 5], stat: i32) -> i32 {
        let mut best = i32::MIN;
        let mut order = [0, 1, 2, 3, 4];
        loop {
            for captain_index in 0..TEAM_SIZE {
                let skills: [TeamCardSkill; 6] = std::array::from_fn(|activation| {
                    if activation == TEAM_SIZE {
                        team[captain_index]
                    } else {
                        team[order[activation]]
                    }
                });
                best = best.max(chart.get_score_for_six_skills(&skills, stat, true).unwrap());
            }
            if !next_test_permutation(&mut order) {
                break;
            }
        }
        best
    }

    fn brute_force_independent_order_score(
        chart: &Chart,
        team: &[TeamCardSkill; 5],
        stat: i32,
    ) -> i32 {
        let base = chart.no_skill_score_at_stat(stat).unwrap() as i32;
        let mut best = i32::MIN;
        let mut order = [0, 1, 2, 3, 4];
        loop {
            for captain_index in 0..TEAM_SIZE {
                let normal = order
                    .iter()
                    .enumerate()
                    .map(|(activation, &card_idx)| {
                        chart
                            .skill_delta_at_stat(activation, team[card_idx], stat)
                            .unwrap() as i32
                    })
                    .sum::<i32>();
                let captain = chart
                    .skill_delta_at_stat(TEAM_SIZE, team[captain_index], stat)
                    .unwrap() as i32;
                best = best.max(base + normal + captain);
            }
            if !next_test_permutation(&mut order) {
                break;
            }
        }
        best
    }

    fn overlapping_chart(rng: &mut Rng) -> Chart {
        let mut nodes = Vec::new();
        let gap = rng.f64(2.5, 7.5);
        for activation in 0..6 {
            let start = activation as f64 * gap;
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: start,
            });
            let notes = 2 + rng.usize(8);
            for note in 0..notes {
                nodes.push(ChartNode {
                    node_type: ChartNodeType::Node,
                    time: start
                        + 0.1
                        + (note as f64 + rng.f64(0.0, 0.8)) * gap * 0.9 / notes as f64,
                });
            }
        }
        let last = 6.0 * gap;
        for note in 0..10 {
            nodes.push(ChartNode {
                node_type: ChartNodeType::Node,
                time: last + note as f64 * 1.1,
            });
        }
        Chart::new(20, nodes)
    }

    fn nonoverlapping_chart(rng: &mut Rng) -> Chart {
        let mut nodes = Vec::new();
        for activation in 0..6 {
            let start = activation as f64 * 10.0;
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: start,
            });
            let notes = 2 + rng.usize(10);
            for note in 0..notes {
                nodes.push(ChartNode {
                    node_type: ChartNodeType::Node,
                    time: start + 0.1 + (note as f64 + rng.f64(0.0, 0.8)) * 8.5 / notes as f64,
                });
            }
        }
        Chart::new(20, nodes)
    }

    fn random_team(rng: &mut Rng) -> ([TeamCardSkill; 5], i32) {
        const NORMAL_DURATIONS: [f64; 9] = [3.0, 4.0, 4.5, 5.0, 5.5, 6.0, 6.5, 7.0, 8.0];
        const RATE_DURATIONS: [f64; 5] = [5.0, 5.5, 6.0, 6.5, 7.0];
        let mut stats = [0.0; 5];
        let team = std::array::from_fn(|idx| {
            let rateup = rng.usize(4) == 0;
            let duration = if rateup {
                RATE_DURATIONS[rng.usize(RATE_DURATIONS.len())]
            } else {
                NORMAL_DURATIONS[rng.usize(NORMAL_DURATIONS.len())]
            };
            let score_up = if rateup { 1.0 } else { rng.f64(0.2, 1.8) };
            stats[idx] = rng.f64(500.0, 2_000.0);
            TeamCardSkill {
                card_id: idx as u32 + 1,
                duration,
                score_up,
                rateup,
            }
        });
        let stat = crate::floor_team_stat(stats.into_iter().map(|value| value * 3.0));
        (team, stat)
    }

    fn next_test_permutation(values: &mut [usize]) -> bool {
        let Some(pivot) = (0..values.len().saturating_sub(1))
            .rev()
            .find(|&idx| values[idx] < values[idx + 1])
        else {
            return false;
        };
        let successor = (pivot + 1..values.len())
            .rev()
            .find(|&idx| values[pivot] < values[idx])
            .unwrap();
        values.swap(pivot, successor);
        values[pivot + 1..].reverse();
        true
    }
}
