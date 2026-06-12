mod medley;
mod single;

use crate::medley::error::BuildError;
use crate::medley::seed::seed_medley_result_for_items;
use crate::medley::team::{TeamBuildError, TeamGenerationOptions};
use crate::model::chart::Chart;
use crate::model::dp::SongMode;
use crate::model::preparation::{AreaItemPercent, PreparedCard};
use crate::model::schema::{
    Attribute, BuildResult, EventType, Magazine, SelectedAreaItems, SongSelection,
};
use crate::single::SingleSongDpError;
use crate::timing::Timer;
use bangdream_optimize_medley_solver::{MedleySolverError, MedleySolverPreference};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CalculationError {
    #[error("no valid area item combinations are available")]
    NoAreaItemCombinations,

    #[error("no build result found")]
    NoBuildResult,

    #[error("team build error: {0}")]
    Team(#[from] TeamBuildError),

    #[error("single-song DP error: {0}")]
    SingleDp(#[from] SingleSongDpError),

    #[error("candidate calculation error: {0}")]
    Candidate(#[from] BuildError),

    #[error(
        "calculation requires matching songs and charts, got {songs} songs and {charts} charts"
    )]
    SongChartCountMismatch { songs: usize, charts: usize },

    #[error("{event_type:?} calculation requires {expected} songs, got {actual}")]
    InvalidSongCount {
        event_type: EventType,
        expected: usize,
        actual: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferredItemTarget {
    pub band: String,
    pub attribute: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSearchOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred: Option<PreferredItemTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solver_preference: Option<MedleySolverPreference>,
    #[serde(default)]
    pub team_generation: TeamGenerationOptions,
}

impl Default for ItemSearchOptions {
    fn default() -> Self {
        Self {
            preferred: None,
            solver_preference: None,
            team_generation: TeamGenerationOptions::default(),
        }
    }
}

pub fn calculate_best_result_for_items(
    event_id: u32,
    event_type: EventType,
    song_list: Vec<SongSelection>,
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    options: ItemSearchOptions,
) -> Result<BuildResult, CalculationError> {
    let calculation_start = Timer::start();
    validate_song_inputs(event_type, &song_list, charts)?;
    let item_combinations =
        item_combinations(area_item_percent, event_type, options.preferred.clone())?;
    let item_combination_count = item_combinations.len();
    let item_combinations =
        prune_dominated_item_combinations(item_combinations, cards, area_item_percent);
    let item_combinations_after = item_combinations.len();
    if trace_enabled() {
        eprintln!(
            "item combinations: {item_combination_count} -> {}",
            item_combinations_after
        );
    }
    let mut best: Option<BuildResult> = None;
    let mut medley_search_metrics =
        (event_type == EventType::Medley).then(medley::MedleySearchMetrics::default);

    for selected_items in item_combinations {
        let current_best = best_score(&best);
        if event_type == EventType::Medley {
            let upper_bound_start = Timer::start();
            let can_beat = medley::medley_item_can_beat_incumbent(
                cards,
                charts,
                area_item_percent,
                &selected_items,
                current_best,
            )?;
            if let Some(metrics) = medley_search_metrics.as_mut() {
                metrics.add_item_upper_bound_ms(upper_bound_start.elapsed_ms());
            }
            if !can_beat {
                continue;
            }
        }

        if event_type == EventType::Medley && cards.len() > 128 {
            let seed_start = Timer::start();
            let seed = seed_medley_result_for_items(
                event_id,
                &song_list,
                cards,
                charts,
                area_item_percent,
                &selected_items,
                options.team_generation,
            )?;
            if let Some(metrics) = medley_search_metrics.as_mut() {
                metrics.add_seed_ms(seed_start.elapsed_ms());
            }
            if let Some(seed) = seed {
                if best_score(&best) < seed.total_score {
                    if trace_enabled() {
                        eprintln!("medley seed incumbent: total_score={}", seed.total_score);
                    }
                    best = Some(seed);
                }
            }
        }

        let current_best = best_score(&best);
        let item_result = if event_type == EventType::Medley {
            medley::calculate_medley_result_for_items(
                event_id,
                &song_list,
                cards,
                charts,
                area_item_percent,
                &selected_items,
                &options,
                current_best,
                medley_search_metrics.as_mut(),
            )
        } else {
            calculate_result_for_items(
                event_id,
                event_type,
                &song_list,
                cards,
                charts,
                area_item_percent,
                &selected_items,
                &options,
                current_best,
            )
        };

        match item_result {
            Ok(result) if best_score(&best) <= result.total_score => {
                if let Some(metrics) = medley_search_metrics.as_mut() {
                    metrics.record_best_result(&result);
                }
                best = Some(result);
            }
            Ok(_) => {}
            Err(CalculationError::SingleDp(SingleSongDpError::NotEnoughCards { .. }))
            | Err(CalculationError::SingleDp(SingleSongDpError::NoResult))
            | Err(CalculationError::Team(TeamBuildError::NotEnoughCards { .. }))
            | Err(CalculationError::Candidate(BuildError::EmptyCandidates))
            | Err(CalculationError::Candidate(BuildError::MedleySolver(
                MedleySolverError::NoValidPlan,
            ))) => {}
            Err(error) => return Err(error),
        }
    }

    let mut result = best.ok_or(CalculationError::NoBuildResult)?;
    let metrics = result.metrics.get_or_insert_with(Default::default);
    metrics.card_count = cards.len();
    metrics.song_count = song_list.len();
    metrics.item_combinations_before = item_combination_count;
    metrics.item_combinations_after = item_combinations_after;
    metrics.total_elapsed_ms = calculation_start.elapsed_ms();
    if let Some(medley_search_metrics) = medley_search_metrics {
        metrics.medley = Some(medley_search_metrics.into_metrics());
    }
    Ok(result)
}

fn validate_song_inputs(
    event_type: EventType,
    song_list: &[SongSelection],
    charts: &[Chart],
) -> Result<(), CalculationError> {
    if song_list.len() != charts.len() {
        return Err(CalculationError::SongChartCountMismatch {
            songs: song_list.len(),
            charts: charts.len(),
        });
    }

    let expected = match event_type {
        EventType::Medley => 3,
        EventType::Versus | EventType::Challenge => 1,
    };
    if song_list.len() != expected {
        return Err(CalculationError::InvalidSongCount {
            event_type,
            expected,
            actual: song_list.len(),
        });
    }

    Ok(())
}

fn calculate_result_for_items(
    event_id: u32,
    event_type: EventType,
    song_list: &[SongSelection],
    cards: &[PreparedCard],
    charts: &[Chart],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    options: &ItemSearchOptions,
    current_best: i32,
) -> Result<BuildResult, CalculationError> {
    match event_type {
        EventType::Medley => medley::calculate_medley_result_for_items(
            event_id,
            song_list,
            cards,
            charts,
            area_item_percent,
            selected_items,
            options,
            current_best,
            None,
        ),
        EventType::Versus | EventType::Challenge => single::calculate_single_result_for_items(
            event_id,
            event_type,
            &song_list[0],
            cards,
            &charts[0],
            area_item_percent,
            selected_items,
        ),
    }
}

fn trace_enabled() -> bool {
    std::env::var_os("BANGDREAM_OPTIMIZE_DP_TRACE").is_some()
}

fn mode_candidates(cards: &[PreparedCard]) -> Vec<SongMode> {
    let mut modes = vec![SongMode::Mixed];
    let bands = card_bands(cards);
    let attributes = card_attributes(cards);

    for &band_id in &bands {
        push_useful_mode(cards, &mut modes, SongMode::UnifiedBand(band_id));
    }
    for &attribute in &attributes {
        push_useful_mode(cards, &mut modes, SongMode::UnifiedAttribute(attribute));
    }
    for &band_id in &bands {
        for &attribute in &attributes {
            push_useful_mode(
                cards,
                &mut modes,
                SongMode::UnifiedBandAttribute(band_id, attribute),
            );
        }
    }

    modes
}

fn push_useful_mode(cards: &[PreparedCard], modes: &mut Vec<SongMode>, mode: SongMode) {
    if !mode_can_build_team(cards, mode) || !mode_improves_any_skill(cards, mode) {
        return;
    }
    if modes
        .iter()
        .any(|&existing| mode_dominates(cards, existing, mode))
    {
        return;
    }

    modes.retain(|&existing| !mode_dominates(cards, mode, existing));
    modes.push(mode);
}

fn mode_can_build_team(cards: &[PreparedCard], mode: SongMode) -> bool {
    let mut characters = Vec::new();
    for card in cards.iter().filter(|card| mode.allows(card)) {
        if !characters.contains(&card.character_id) {
            characters.push(card.character_id);
            if characters.len() >= 5 {
                return true;
            }
        }
    }

    false
}

fn mode_improves_any_skill(cards: &[PreparedCard], mode: SongMode) -> bool {
    cards
        .iter()
        .filter(|card| mode.allows(card))
        .any(|card| mode_score_up(card, mode) > card.score_up.default)
}

fn mode_score_up(card: &PreparedCard, mode: SongMode) -> f64 {
    match mode {
        SongMode::Mixed => card.score_up.default,
        SongMode::UnifiedBand(band_id) => card.score_up.resolve(Some(band_id), None),
        SongMode::UnifiedAttribute(attribute) => card.score_up.resolve(None, Some(attribute)),
        SongMode::UnifiedBandAttribute(band_id, attribute) => {
            card.score_up.resolve(Some(band_id), Some(attribute))
        }
    }
}

fn mode_dominates(cards: &[PreparedCard], stronger: SongMode, weaker: SongMode) -> bool {
    cards.iter().all(|card| {
        if !weaker.allows(card) {
            return true;
        }
        if !stronger.allows(card) {
            return false;
        }
        mode_effective_score_up(card, stronger) >= mode_effective_score_up(card, weaker)
    })
}

fn mode_effective_score_up(card: &PreparedCard, mode: SongMode) -> u64 {
    if card.skill.rateup {
        return 0;
    }

    mode_score_up(card, mode).to_bits()
}

fn card_bands(cards: &[PreparedCard]) -> Vec<u32> {
    let mut bands = Vec::new();
    for card in cards {
        if !bands.contains(&card.band_id) {
            bands.push(card.band_id);
        }
    }
    bands.sort_unstable();
    bands
}

fn card_attributes(cards: &[PreparedCard]) -> Vec<Attribute> {
    const ORDER: [Attribute; 5] = [
        Attribute::Cool,
        Attribute::Happy,
        Attribute::Pure,
        Attribute::Powerful,
        Attribute::All,
    ];

    ORDER
        .into_iter()
        .filter(|attribute| cards.iter().any(|card| card.attribute == *attribute))
        .collect()
}

fn best_score(best: &Option<BuildResult>) -> i32 {
    best.as_ref().map(|result| result.total_score).unwrap_or(0)
}

fn item_combinations(
    area_item_percent: &AreaItemPercent,
    event_type: EventType,
    preferred: Option<PreferredItemTarget>,
) -> Result<Vec<SelectedAreaItems>, CalculationError> {
    let magazines: Vec<Magazine> = area_item_percent
        .magazine
        .keys()
        .filter_map(|key| Magazine::from_key(key))
        .collect();
    let bands: Vec<String> = area_item_percent.band.keys().cloned().collect();
    let attributes: Vec<String> = area_item_percent.attribute.keys().cloned().collect();

    if magazines.is_empty() || bands.is_empty() || attributes.is_empty() {
        return Err(CalculationError::NoAreaItemCombinations);
    }

    let mut combinations = Vec::new();
    for magazine in magazines {
        if event_type == EventType::Medley {
            if let Some(preferred) = &preferred {
                combinations.push(SelectedAreaItems {
                    band: preferred.band.clone(),
                    attribute: preferred.attribute.clone(),
                    magazine,
                });
            }
        }

        for band in &bands {
            for attribute in &attributes {
                if event_type == EventType::Medley {
                    if let Some(preferred) = &preferred {
                        if &preferred.band == band && &preferred.attribute == attribute {
                            continue;
                        }
                    }
                }

                combinations.push(SelectedAreaItems {
                    band: band.clone(),
                    attribute: attribute.clone(),
                    magazine,
                });
            }
        }
    }

    if combinations.is_empty() {
        return Err(CalculationError::NoAreaItemCombinations);
    }

    Ok(combinations)
}

fn prune_dominated_item_combinations(
    combinations: Vec<SelectedAreaItems>,
    cards: &[PreparedCard],
    area_item_percent: &AreaItemPercent,
) -> Vec<SelectedAreaItems> {
    let stats = combinations
        .iter()
        .map(|items| {
            cards
                .iter()
                .map(|card| card_stat_for_items(card, area_item_percent, items))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    combinations
        .into_iter()
        .enumerate()
        .filter_map(|(idx, items)| {
            let dominated = stats.iter().enumerate().any(|(other_idx, other)| {
                other_idx != idx
                    && item_stats_dominate(other, &stats[idx])
                    && (item_stats_strictly_better(other, &stats[idx]) || other_idx < idx)
            });
            (!dominated).then_some(items)
        })
        .collect()
}

fn card_stat_for_items(
    card: &PreparedCard,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
) -> i32 {
    card.add_up_stat(
        area_item_percent,
        &selected_items.band,
        &selected_items.attribute,
        selected_items.magazine.as_str(),
    )
    .floor() as i32
}

fn item_stats_dominate(left: &[i32], right: &[i32]) -> bool {
    left.iter().zip(right).all(|(left, right)| left >= right)
}

fn item_stats_strictly_better(left: &[i32], right: &[i32]) -> bool {
    left.iter().zip(right).any(|(left, right)| left > right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::chart::{ChartNode, ChartNodeType, TeamCardSkill};
    use crate::model::preparation::{ScoreUp, StatRate, StatValue, PERFORMANCE_KEY};
    use crate::model::schema::{Attribute, Magazine};
    use std::collections::BTreeMap;

    fn chart(song_idx: u32) -> Chart {
        let mut nodes = Vec::new();
        for idx in 0..6 {
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: idx as f64 * 10.0 + song_idx as f64,
            });
            nodes.push(ChartNode {
                node_type: ChartNodeType::Node,
                time: idx as f64 * 10.0 + song_idx as f64 + 1.0,
            });
        }
        let mut chart = Chart::new(5, nodes);
        chart.init(0, false).unwrap();
        chart
    }

    fn prepared_card(card_id: u32, character_id: u32, band_id: u32) -> PreparedCard {
        PreparedCard {
            card_id,
            character_id,
            band_id,
            rarity: 4,
            attribute: Attribute::Cool,
            level: 60,
            training: true,
            illust_training_status: true,
            episodes: [true, true],
            limit_break_rank: 0,
            skill_level: 5,
            stat: StatValue {
                performance: 1000.0,
                technique: 1000.0,
                visual: 1000.0,
            },
            event_add_stat: StatValue::zero(),
            skill: TeamCardSkill {
                card_id,
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
        }
    }

    fn area_item_percent() -> AreaItemPercent {
        AreaItemPercent {
            band: BTreeMap::from([
                ("1".to_owned(), StatRate::all(0.10)),
                ("2".to_owned(), StatRate::all(0.30)),
            ]),
            attribute: BTreeMap::from([("cool".to_owned(), StatRate::all(0.10))]),
            magazine: BTreeMap::from([(
                PERFORMANCE_KEY.to_owned(),
                StatRate {
                    performance: 0.20,
                    technique: 0.0,
                    visual: 0.0,
                },
            )]),
        }
    }

    #[test]
    fn medley_item_combinations_include_everyone_as_actual_target_set() {
        let area_item_percent = AreaItemPercent {
            band: BTreeMap::from([
                ("1".to_owned(), StatRate::all(0.10)),
                ("2".to_owned(), StatRate::all(0.10)),
                ("3".to_owned(), StatRate::all(0.10)),
                ("4".to_owned(), StatRate::all(0.10)),
                ("5".to_owned(), StatRate::all(0.10)),
                ("18".to_owned(), StatRate::all(0.10)),
                ("21".to_owned(), StatRate::all(0.10)),
                ("45".to_owned(), StatRate::all(0.10)),
                ("1,2,3,4,5,18,21,45".to_owned(), StatRate::all(0.10)),
            ]),
            attribute: BTreeMap::from([
                ("cool".to_owned(), StatRate::all(0.10)),
                ("happy".to_owned(), StatRate::all(0.10)),
                ("pure".to_owned(), StatRate::all(0.10)),
                ("powerful".to_owned(), StatRate::all(0.10)),
                ("cool,happy,powerful,pure".to_owned(), StatRate::all(0.10)),
            ]),
            magazine: BTreeMap::from([
                (PERFORMANCE_KEY.to_owned(), StatRate::all(0.10)),
                ("technique".to_owned(), StatRate::all(0.10)),
                ("visual".to_owned(), StatRate::all(0.10)),
            ]),
        };

        let combinations = item_combinations(
            &area_item_percent,
            EventType::Medley,
            Some(PreferredItemTarget {
                band: "4".to_owned(),
                attribute: "cool".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(combinations.len(), 135);
        assert!(combinations
            .iter()
            .any(|items| items.band == "1,2,3,4,5,18,21,45"));
        assert!(combinations
            .iter()
            .any(|items| items.attribute == "cool,happy,powerful,pure"));
    }

    #[test]
    fn prunes_zero_level_all_attribute_item_choice() {
        let cards = (1..=5)
            .map(|idx| prepared_card(idx, idx, 4))
            .collect::<Vec<_>>();
        let area_item_percent = AreaItemPercent {
            band: BTreeMap::from([("4".to_owned(), StatRate::all(0.10))]),
            attribute: BTreeMap::from([
                ("cool".to_owned(), StatRate::all(0.10)),
                ("cool,happy,powerful,pure".to_owned(), StatRate::zero()),
            ]),
            magazine: BTreeMap::from([(PERFORMANCE_KEY.to_owned(), StatRate::zero())]),
        };
        let combinations = item_combinations(&area_item_percent, EventType::Medley, None).unwrap();

        assert_eq!(combinations.len(), 2);
        let pruned = prune_dominated_item_combinations(combinations, &cards, &area_item_percent);

        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].attribute, "cool");
    }

    #[test]
    fn selects_best_item_combination_for_single_song_event() {
        let cards = vec![
            prepared_card(1, 1, 2),
            prepared_card(2, 2, 2),
            prepared_card(3, 3, 2),
            prepared_card(4, 4, 2),
            prepared_card(5, 5, 2),
        ];

        let result = calculate_best_result_for_items(
            200,
            EventType::Challenge,
            vec![SongSelection {
                song_id: 1,
                difficulty: 3,
            }],
            &cards,
            &[chart(0)],
            &area_item_percent(),
            ItemSearchOptions::default(),
        )
        .unwrap();

        assert_eq!(result.items.as_ref().unwrap().band, "2");
        assert_eq!(
            result.items.as_ref().unwrap().magazine,
            Magazine::Performance
        );
        assert_eq!(result.solver.as_deref(), Some("dp"));
        assert_eq!(result.songs.len(), 1);
        assert!(result.total_score > 0);
    }

    #[test]
    fn medley_uses_candidate_solver_at_optimization_entry() {
        let cards = (0..15)
            .map(|idx| prepared_card(idx + 1, idx + 1, 1))
            .collect::<Vec<_>>();

        let result = calculate_best_result_for_items(
            201,
            EventType::Medley,
            vec![
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
            &cards,
            &[chart(0), chart(1), chart(2)],
            &area_item_percent(),
            ItemSearchOptions {
                preferred: Some(PreferredItemTarget {
                    band: "1".to_owned(),
                    attribute: "cool".to_owned(),
                }),
                solver_preference: Some(MedleySolverPreference::Scalar),
                team_generation: TeamGenerationOptions::default(),
            },
        );

        let result = result.unwrap();
        assert_eq!(result.event_type, EventType::Medley);
        assert_eq!(result.solver.as_deref(), Some("scalar"));
        assert_eq!(result.songs.len(), 3);
        assert!(result.total_score > 0);
    }

    #[test]
    fn medley_large_signature_pool_can_pass_contribution_preprune() {
        let mut cards = (0..=u64::BITS)
            .map(|idx| prepared_card(idx + 1, idx + 1, 1))
            .collect::<Vec<_>>();
        for (idx, card) in cards.iter_mut().enumerate() {
            let stat = 1000.0 + idx as f64;
            card.stat = StatValue {
                performance: stat,
                technique: stat,
                visual: stat,
            };
            card.score_up.default = 2.0 - idx as f64 * 0.001;
        }

        let result = calculate_best_result_for_items(
            203,
            EventType::Medley,
            vec![
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
            &cards,
            &[chart(0), chart(1), chart(2)],
            &area_item_percent(),
            ItemSearchOptions {
                preferred: Some(PreferredItemTarget {
                    band: "1".to_owned(),
                    attribute: "cool".to_owned(),
                }),
                solver_preference: Some(MedleySolverPreference::Scalar),
                team_generation: TeamGenerationOptions::default(),
            },
        )
        .unwrap();

        assert_eq!(result.event_type, EventType::Medley);
        assert_eq!(result.solver.as_deref(), Some("scalar"));
        assert_eq!(result.songs.len(), 3);
        assert!(result.total_score > 0);
    }

    #[test]
    fn medley_allows_large_final_layer_candidate_generation() {
        let cards = (0..15)
            .map(|idx| prepared_card(idx + 1, idx + 1, 1))
            .collect::<Vec<_>>();

        let result = calculate_best_result_for_items(
            204,
            EventType::Medley,
            vec![
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
            &cards,
            &[chart(0), chart(1), chart(2)],
            &area_item_percent(),
            ItemSearchOptions {
                preferred: Some(PreferredItemTarget {
                    band: "1".to_owned(),
                    attribute: "cool".to_owned(),
                }),
                solver_preference: Some(MedleySolverPreference::Scalar),
                team_generation: TeamGenerationOptions {
                    max_candidates: 1,
                    ..TeamGenerationOptions::default()
                },
            },
        )
        .unwrap();

        assert_eq!(result.event_type, EventType::Medley);
        assert_eq!(result.solver.as_deref(), Some("scalar"));
        assert!(result.total_score > 0);
    }

    #[test]
    fn prunes_dominated_item_combinations() {
        let cards = (1..=5)
            .map(|idx| prepared_card(idx, idx, 2))
            .collect::<Vec<_>>();
        let combinations = vec![
            SelectedAreaItems {
                band: "1".to_owned(),
                attribute: "cool".to_owned(),
                magazine: Magazine::Performance,
            },
            SelectedAreaItems {
                band: "2".to_owned(),
                attribute: "cool".to_owned(),
                magazine: Magazine::Performance,
            },
        ];

        let pruned = prune_dominated_item_combinations(combinations, &cards, &area_item_percent());

        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].band, "2");
    }

    #[test]
    fn mode_candidates_remove_dominated_card_pool_effects() {
        let mut cards = (1..=5)
            .map(|idx| prepared_card(idx, idx, 1))
            .collect::<Vec<_>>();
        for card in &mut cards {
            card.score_up = ScoreUp {
                default: 0.50,
                unification_activate_effect_value: Some(1.00),
                unification_activate_condition_band_id: None,
                unification_activate_condition_type: Some(Attribute::Cool),
            };
        }

        let modes = mode_candidates(&cards);

        assert_eq!(modes, vec![SongMode::UnifiedAttribute(Attribute::Cool)]);
    }

    #[test]
    fn errors_when_no_item_combinations_exist() {
        let error = calculate_best_result_for_items(
            202,
            EventType::Challenge,
            vec![SongSelection {
                song_id: 1,
                difficulty: 3,
            }],
            &[],
            &[chart(0)],
            &AreaItemPercent::empty(),
            ItemSearchOptions::default(),
        )
        .unwrap_err();

        assert!(matches!(error, CalculationError::NoAreaItemCombinations));
    }
}
