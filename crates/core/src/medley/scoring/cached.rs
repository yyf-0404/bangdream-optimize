//! Reuse stat-independent meta-order seeds within an immutable signature context.
//! Every team's exact integer scores and optimal orders are still calculated.
use super::*;

const CACHE_SLOTS: usize = 32_768;
const EMPTY_KEY: u128 = u128::MAX;

#[derive(Clone, Copy)]
struct Entry {
    key: u128,
    orders: [[u8; TEAM_SIZE]; MEDLEY_TEAM_COUNT],
    captains: [u8; MEDLEY_TEAM_COUNT],
}

impl Entry {
    const EMPTY: Self = Self {
        key: EMPTY_KEY,
        orders: [[0; 5]; 3],
        captains: [0; 3],
    };

    fn from_seeds(key: u128, seeds: [MedleySkillOrder; MEDLEY_TEAM_COUNT]) -> Self {
        Self {
            key,
            orders: seeds.map(|seed| seed.order_indices.map(|index| index as u8)),
            captains: seeds.map(|seed| seed.captain_index as u8),
        }
    }

    fn restore(self) -> [MedleySkillOrder; MEDLEY_TEAM_COUNT] {
        std::array::from_fn(|chart| MedleySkillOrder {
            order_indices: self.orders[chart].map(usize::from),
            captain_index: usize::from(self.captains[chart]),
        })
    }
}

pub(in crate::medley) struct CandidateScorer<'a> {
    cards: &'a [ResolvedMedleyCardInput],
    charts: &'a [Chart],
    options: TeamGenerationOptions,
    scratch: ExactScoreScratch,
    classes: Vec<u16>,
    entries: Vec<Entry>,
    pub(in crate::medley) hits: u64,
    pub(in crate::medley) misses: u64,
    pub(in crate::medley) collisions: u64,
}

fn enabled() -> bool {
    #[cfg(feature = "experimental-candidate-seed-cache")]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var("BANGDREAM_OPTIMIZE_CANDIDATE_SEED_CACHE").as_deref() != Ok("off")
        })
    }
    #[cfg(not(feature = "experimental-candidate-seed-cache"))]
    true
}

impl<'a> CandidateScorer<'a> {
    pub(in crate::medley) fn new(
        cards: &'a [ResolvedMedleyCardInput],
        charts: &'a [Chart],
        options: TeamGenerationOptions,
    ) -> Self {
        Self::with_slots(
            cards,
            charts,
            options,
            if enabled() { CACHE_SLOTS } else { 0 },
        )
    }

    fn with_slots(
        cards: &'a [ResolvedMedleyCardInput],
        charts: &'a [Chart],
        options: TeamGenerationOptions,
        slots: usize,
    ) -> Self {
        let mut class_ids = HashMap::new();
        let classes = if slots == 0 {
            Vec::new()
        } else {
            cards
                .iter()
                .map(|card| {
                    // The seed depends only on these exact meta bits, in card order.
                    // Stat, identity and skill windows still enter exact scoring.
                    let mut key = [0u64; MEDLEY_TEAM_COUNT * (TEAM_SIZE + 1)];
                    for (out, meta) in key
                        .iter_mut()
                        .zip(card.skill_meta_by_chart.iter().flatten())
                    {
                        *out = meta.to_bits();
                    }
                    let next_id = class_ids.len();
                    let id = *class_ids.entry(key).or_insert(next_id);
                    u16::try_from(id).ok()
                })
                .collect::<Option<Vec<_>>>()
                .unwrap_or_default()
        };
        let slots = if classes.len() == cards.len() {
            slots
        } else {
            0
        };
        debug_assert!(slots == 0 || slots.is_power_of_two());
        Self {
            cards,
            charts,
            options,
            scratch: ExactScoreScratch::compressed(),
            classes,
            entries: vec![Entry::EMPTY; slots],
            hits: 0,
            misses: 0,
            collisions: 0,
        }
    }

    pub(in crate::medley) fn score(
        &mut self,
        selected: &[usize; TEAM_SIZE],
        profile: Option<&mut ResolvedCandidateBuildProfile>,
    ) -> Result<RawTeamCandidate, TeamBuildError> {
        let seeds = if self.entries.is_empty() {
            self.misses += 1;
            return if let Some(profile) = profile {
                build_resolved_candidate_profiled(
                    self.cards,
                    self.charts,
                    self.options,
                    selected,
                    &mut self.scratch,
                    profile,
                )
            } else {
                build_resolved_candidate(
                    self.cards,
                    self.charts,
                    self.options,
                    selected,
                    &mut self.scratch,
                )
            };
        } else {
            self.seeds(selected)
        };
        if let Some(profile) = profile {
            build_resolved_candidate_internal::<true>(
                self.cards,
                self.charts,
                self.options,
                selected,
                &mut self.scratch,
                Some(profile),
                Some(&seeds),
            )
        } else {
            build_resolved_candidate_internal::<false>(
                self.cards,
                self.charts,
                self.options,
                selected,
                &mut self.scratch,
                None,
                Some(&seeds),
            )
        }
    }

    fn seeds(&mut self, selected: &[usize; TEAM_SIZE]) -> [MedleySkillOrder; MEDLEY_TEAM_COUNT] {
        // Five 16-bit class ids use 80 bits, so no valid key equals the empty marker.
        // Keep positions in the key to preserve the original DP's tie behavior.
        let mut key = 0u128;
        for (slot, &index) in selected.iter().enumerate() {
            key |= u128::from(self.classes[index]) << (slot * 16);
        }
        let index = slot_index(key, self.entries.len());
        let entry = self.entries[index];
        if entry.key == key {
            self.hits += 1;
            return entry.restore();
        }
        self.misses += 1;
        self.collisions += u64::from(entry.key != EMPTY_KEY);
        let seeds = std::array::from_fn(|chart| {
            max_meta_order_for_team(
                &selected.map(|index| self.cards[index].skill_meta_by_chart[chart]),
            )
        });
        self.entries[index] = Entry::from_seeds(key, seeds);
        seeds
    }
}

fn slot_index(key: u128, slots: usize) -> usize {
    let mut value = key as u64 ^ ((key >> 64) as u64).rotate_left(23);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((value ^ (value >> 31)) as usize) & (slots - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::chart::{ChartNode, ChartNodeType};

    fn fixture(gap: f64, medley: bool) -> (Vec<Chart>, Vec<ResolvedMedleyCardInput>) {
        let charts = (0..3)
            .map(|song| {
                let mut nodes = Vec::new();
                for activation in 0..6 {
                    let time = activation as f64 * gap;
                    nodes.push(ChartNode {
                        node_type: ChartNodeType::Skill,
                        time,
                    });
                    for note in 0..17 {
                        nodes.push(ChartNode {
                            node_type: ChartNodeType::Node,
                            time: time + 0.1 + note as f64 * gap / 18.0,
                        });
                    }
                }
                let mut chart = Chart::new(25 + song, nodes);
                chart.init(song * 102, medley).unwrap();
                chart
            })
            .collect::<Vec<_>>();
        let cards = (0..20)
            .map(|index| {
                let class = index % 5;
                let group = index / 5;
                let skill = TeamCardSkill {
                    card_id: 100 + index as u32,
                    duration: 5.0 + class as f64 * 0.5,
                    score_up: 1.0 + class as f64 * 0.1 + if group == 3 { 0.01 } else { 0.0 },
                    rateup: class == 4,
                };
                ResolvedMedleyCardInput {
                    raw_index: index * 7 + 3,
                    stat: 40_000.0 + class as f64 * 110.4 + if group == 2 { 123.0 } else { 0.0 },
                    band_id: 1,
                    attribute: Attribute::Cool,
                    skill,
                    skill_meta_by_chart: std::array::from_fn(|song| {
                        std::array::from_fn(|activation| {
                            charts[song].skill_meta_value(activation, skill).unwrap()
                        })
                    }),
                    skill_windows_by_chart: std::array::from_fn(|song| {
                        std::array::from_fn(|activation| {
                            charts[song]
                                .compile_exact_skill_window(activation, skill)
                                .unwrap()
                        })
                    }),
                }
            })
            .collect();
        (charts, cards)
    }

    fn assert_same(a: &RawTeamCandidate, b: &RawTeamCandidate) {
        assert_eq!(a.raw_indices, b.raw_indices);
        assert_eq!(a.scores, b.scores);
        assert_eq!(a.stat, b.stat);
        assert_eq!(a.ordered_raw_indices, b.ordered_raw_indices);
        assert_eq!(a.captain_raw_indices, b.captain_raw_indices);
    }

    #[test]
    fn seed_hits_preserve_current_ids_positions_stats_and_skill_changes() {
        for gap in [3.0, 10.0] {
            for medley in [true, false] {
                let (charts, cards) = fixture(gap, medley);
                let options = TeamGenerationOptions {
                    score_as_medley: medley,
                    ..TeamGenerationOptions::default()
                };
                for slots in [1, 128] {
                    let mut scorer = CandidateScorer::with_slots(&cards, &charts, options, slots);
                    let mut reference = ExactScoreScratch::compressed();
                    for selected in [
                        [0, 1, 2, 3, 4],
                        [5, 6, 7, 8, 9],
                        [1, 0, 2, 3, 4],
                        [6, 5, 7, 8, 9],
                        [11, 10, 12, 13, 14],
                        [15, 16, 17, 18, 19],
                    ] {
                        let expected = build_resolved_candidate(
                            &cards,
                            &charts,
                            options,
                            &selected,
                            &mut reference,
                        )
                        .unwrap();
                        let actual = scorer.score(&selected, None).unwrap();
                        assert_same(&actual, &expected);
                    }
                    assert_eq!(scorer.hits, 3);
                    assert_eq!(scorer.misses, 3);
                    if slots == 1 {
                        assert_eq!(scorer.collisions, 2);
                    }
                }
            }
        }
    }

    #[test]
    fn identical_skills_keep_ties_and_meta_bits_are_part_of_the_class() {
        let (charts, mut cards) = fixture(10.0, true);
        for index in 1..10 {
            let (raw_index, card_id) = (cards[index].raw_index, cards[index].skill.card_id);
            cards[index] = cards[0];
            cards[index].raw_index = raw_index;
            cards[index].skill.card_id = card_id;
        }
        let options = TeamGenerationOptions::default();
        let mut scorer = CandidateScorer::with_slots(&cards, &charts, options, 128);
        scorer.score(&[0, 1, 2, 3, 4], None).unwrap();
        let hit = scorer.score(&[9, 8, 7, 6, 5], None).unwrap();
        let expected = build_resolved_candidate(
            &cards,
            &charts,
            options,
            &[9, 8, 7, 6, 5],
            &mut ExactScoreScratch::compressed(),
        )
        .unwrap();
        assert_same(&hit, &expected);
        assert_eq!(scorer.hits, 1);
        drop(scorer);
        cards[5].skill_meta_by_chart[0][0] += 1.0;
        let scorer = CandidateScorer::with_slots(&cards, &charts, options, 128);
        assert_ne!(scorer.classes[0], scorer.classes[5]);
    }

    #[test]
    fn randomized_teams_match_uncached_scores_even_with_constant_eviction() {
        let (charts, cards) = fixture(3.0, true);
        let options = TeamGenerationOptions::default();
        let mut scorer = CandidateScorer::with_slots(&cards, &charts, options, 4);
        let mut reference = ExactScoreScratch::compressed();
        let mut seed = 17u64;
        for _ in 0..1000 {
            let mut selected = [usize::MAX; 5];
            for slot in 0..5 {
                loop {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let card = seed as usize % cards.len();
                    if !selected.contains(&card) {
                        selected[slot] = card;
                        break;
                    }
                }
            }
            let expected =
                build_resolved_candidate(&cards, &charts, options, &selected, &mut reference)
                    .unwrap();
            assert_same(&scorer.score(&selected, None).unwrap(), &expected);
            // Every immediate repeat must hit, independent of earlier evictions.
            assert_same(&scorer.score(&selected, None).unwrap(), &expected);
        }
        assert!(scorer.hits >= 1000);
        assert!(scorer.collisions > 100);
        assert!(std::mem::size_of::<Entry>() * CACHE_SLOTS <= 2 * 1024 * 1024);
    }
}
