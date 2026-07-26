use crate::model::{
    chart::Chart,
    dp::{DpModelError, SongMode},
    preparation::{AreaItemPercent, PreparedCard},
    schema::SelectedAreaItems,
};
use thiserror::Error;

pub(crate) mod candidate;
mod dominance;
mod exact;
mod mode;
pub(crate) mod profile;

pub use mode::mode_candidates;

#[derive(Debug, Error)]
pub enum SingleSongError {
    #[error("at least five cards are required to build a team, got {count}")]
    NotEnoughCards { count: usize },

    #[error("no valid single-song result found")]
    NoResult,

    #[error("DP model error: {0}")]
    Model(#[from] DpModelError),

    #[error("team prune error: {0}")]
    Prune(#[from] crate::medley::team::TeamBuildError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleSongResult {
    pub score: i32,
    pub stat: i32,
    pub team_card_ids: Vec<u32>,
    pub captain_card_id: u32,
}

pub fn calculate_single_song(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
) -> Result<SingleSongResult, SingleSongError> {
    let active_indices =
        candidate::pruned_card_indices(cards, chart, area_item_percent, selected_items, mode)?;
    let cards = candidate::resolve_card_indices(
        cards,
        &active_indices,
        area_item_percent,
        selected_items,
        mode,
        candidate::SingleCardRole::FullSkill,
    )?;
    exact::solve(&cards, chart)
}

#[cfg(test)]
fn numeric_card_sources(
    cards: &[PreparedCard],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
) -> Result<Vec<candidate::ResolvedSingleCard>, SingleSongError> {
    let indices = cards
        .iter()
        .enumerate()
        .filter_map(|(index, card)| mode.allows(card).then_some(index))
        .collect::<Vec<_>>();
    Ok(candidate::resolve_card_indices(
        cards,
        &indices,
        area_item_percent,
        selected_items,
        mode,
        candidate::SingleCardRole::FullSkill,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chart::{ChartNode, ChartNodeType},
        preparation::{ScoreUp, StatValue},
        schema::{Attribute, Magazine},
        TeamCardSkill,
    };

    const TEAM_SIZE: usize = 5;

    #[test]
    fn single_song_exact_matches_bruteforce() {
        let mut chart = chart();
        chart.init(0, false).unwrap();
        let cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool, 1000, 0.60),
            prepared_card(2, 2, 1, Attribute::Cool, 1100, 0.70),
            prepared_card(3, 3, 1, Attribute::Cool, 1200, 0.80),
            prepared_card(4, 4, 1, Attribute::Cool, 1300, 0.90),
            prepared_card(5, 5, 1, Attribute::Cool, 1400, 1.00),
            prepared_card(6, 6, 1, Attribute::Cool, 1500, 0.50),
        ];
        let selected_items = selected_items();
        let area = AreaItemPercent::empty();

        let exact =
            calculate_single_song(&cards, &chart, &area, &selected_items, SongMode::Mixed).unwrap();
        let numeric =
            numeric_card_sources(&cards, &area, &selected_items, SongMode::Mixed).unwrap();
        let brute = brute_force_single_song(&numeric, &chart);

        assert_eq!(exact.score, brute.score);
        assert_eq!(exact.stat, brute.stat);
        assert_eq!(exact.captain_card_id, brute.captain_card_id);
    }

    #[test]
    fn single_song_mode_resolves_unification_skill() {
        let mut chart = chart();
        chart.init(0, false).unwrap();
        let mut cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool, 1000, 0.10),
            prepared_card(2, 2, 1, Attribute::Cool, 1000, 0.10),
            prepared_card(3, 3, 1, Attribute::Cool, 1000, 0.10),
            prepared_card(4, 4, 1, Attribute::Cool, 1000, 0.10),
            prepared_card(5, 5, 1, Attribute::Cool, 1000, 0.10),
        ];
        cards[0].score_up = ScoreUp {
            default: 0.10,
            unification_activate_effect_value: Some(1.00),
            unification_activate_condition_band_id: Some(1),
            unification_activate_condition_type: None,
        };
        let selected_items = selected_items();
        let area = AreaItemPercent::empty();

        let mixed =
            calculate_single_song(&cards, &chart, &area, &selected_items, SongMode::Mixed).unwrap();
        let unified = calculate_single_song(
            &cards,
            &chart,
            &area,
            &selected_items,
            SongMode::UnifiedBand(1),
        )
        .unwrap();

        assert!(unified.score > mixed.score);
    }

    #[test]
    fn single_song_exact_drops_dominated_same_character_card() {
        let mut chart = chart();
        chart.init(0, false).unwrap();
        let cards = vec![
            prepared_card(1, 1, 1, Attribute::Cool, 1000, 0.50),
            prepared_card(2, 1, 1, Attribute::Cool, 1200, 0.50),
            prepared_card(3, 2, 1, Attribute::Cool, 1000, 0.50),
            prepared_card(4, 3, 1, Attribute::Cool, 1000, 0.50),
            prepared_card(5, 4, 1, Attribute::Cool, 1000, 0.50),
            prepared_card(6, 5, 1, Attribute::Cool, 1000, 0.50),
        ];
        let selected_items = selected_items();
        let area = AreaItemPercent::empty();

        let result =
            calculate_single_song(&cards, &chart, &area, &selected_items, SongMode::Mixed).unwrap();

        assert!(result.team_card_ids.contains(&2));
        assert!(!result.team_card_ids.contains(&1));
    }

    #[test]
    fn single_song_exact_floors_stat_after_summing_the_full_team() {
        let mut chart = chart();
        chart.init(0, false).unwrap();
        let mut cards = (1..=5)
            .map(|card_id| prepared_card(card_id, card_id, 1, Attribute::Cool, 1000, 0.10))
            .collect::<Vec<_>>();
        for card in &mut cards {
            card.event_add_stat.performance = 0.3;
        }

        let result = calculate_single_song(
            &cards,
            &chart,
            &AreaItemPercent::empty(),
            &selected_items(),
            SongMode::Mixed,
        )
        .unwrap();

        assert_eq!(result.stat, 15_001);
    }

    #[test]
    fn strict_exact_single_coverage_fallback_matches_bruteforce_with_queued_rateup_skills() {
        let mut chart = overlapping_chart();
        chart.init(0, false).unwrap();
        assert!(
            !chart.warning.is_empty(),
            "fixture must exercise the single-song arbitrary-window coverage fallback"
        );
        let durations = [3.0, 4.5, 5.0, 5.5, 6.0, 6.5, 7.0, 8.0];
        let mut cards = (0..8)
            .map(|idx| {
                prepared_card(
                    idx as u32 + 1,
                    idx as u32 + 1,
                    1,
                    Attribute::Cool,
                    900 + idx as i32 * 37,
                    0.40 + idx as f64 * 0.10,
                )
            })
            .collect::<Vec<_>>();
        for (idx, card) in cards.iter_mut().enumerate() {
            card.skill.duration = durations[idx];
            if idx == 2 || idx == 4 || idx == 6 {
                card.skill.rateup = true;
                card.skill.score_up = 1.0;
                card.score_up.default = 1.0;
            }
        }
        let area = AreaItemPercent::empty();
        let selected_items = selected_items();
        let exact =
            calculate_single_song(&cards, &chart, &area, &selected_items, SongMode::Mixed).unwrap();
        let numeric =
            numeric_card_sources(&cards, &area, &selected_items, SongMode::Mixed).unwrap();
        let brute = brute_force_single_song(&numeric, &chart);

        assert_eq!(exact.score, brute.score);
        assert_eq!(exact.stat, brute.stat);
    }

    fn brute_force_single_song(
        cards: &[candidate::ResolvedSingleCard],
        chart: &Chart,
    ) -> SingleSongResult {
        let mut best: Option<SingleSongResult> = None;
        let mut selected = Vec::new();

        enumerate_card_sets(cards, 0, &mut selected, &mut |team| {
            for order in permutations(team) {
                for captain_idx in 0..TEAM_SIZE {
                    let mut sa = 0.0;
                    let mut skills = Vec::with_capacity(TEAM_SIZE + 1);
                    for &card_idx in &order {
                        let card = &cards[card_idx];
                        sa += card.stat;
                        skills.push(card.skill);
                    }
                    let captain_card = &cards[order[captain_idx]];
                    skills.push(captain_card.skill);
                    let skills: [TeamCardSkill; TEAM_SIZE + 1] = skills.try_into().unwrap();
                    let stat = crate::floor_team_stat([sa]);
                    let candidate = SingleSongResult {
                        score: chart
                            .get_score_for_six_skills(&skills, stat, false)
                            .unwrap(),
                        stat,
                        team_card_ids: order.iter().map(|&idx| cards[idx].card_id).collect(),
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
        cards: &[candidate::ResolvedSingleCard],
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
                .any(|&selected_idx| cards[selected_idx].character_id == cards[idx].character_id)
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

    fn selected_items() -> SelectedAreaItems {
        SelectedAreaItems {
            band: "1".to_owned(),
            attribute: "cool".to_owned(),
            magazine: Magazine::Performance,
        }
    }

    fn chart() -> Chart {
        let mut nodes = Vec::new();
        for idx in 0..6 {
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: idx as f64 * 10.0,
            });
            nodes.push(ChartNode {
                node_type: ChartNodeType::Node,
                time: idx as f64 * 10.0 + 1.0,
            });
        }
        Chart::new(5, nodes)
    }

    fn overlapping_chart() -> Chart {
        let mut nodes = Vec::new();
        for idx in 0..6 {
            let start = idx as f64 * 5.25;
            nodes.push(ChartNode {
                node_type: ChartNodeType::Skill,
                time: start,
            });
            for offset in [0.5, 1.5, 2.5, 3.5, 4.5] {
                nodes.push(ChartNode {
                    node_type: ChartNodeType::Node,
                    time: start + offset,
                });
            }
        }
        Chart::new(27, nodes)
    }

    fn prepared_card(
        card_id: u32,
        character_id: u32,
        band_id: u32,
        attribute: Attribute,
        stat: i32,
        score_up: f64,
    ) -> PreparedCard {
        PreparedCard {
            card_id,
            character_id,
            band_id,
            rarity: 4,
            attribute,
            level: 60,
            training: true,
            illust_training_status: true,
            episodes: [true, true],
            limit_break_rank: 0,
            skill_level: 5,
            stat: StatValue {
                performance: stat as f64,
                technique: stat as f64,
                visual: stat as f64,
            },
            event_add_stat: StatValue::zero(),
            skill: TeamCardSkill {
                card_id,
                duration: 5.0,
                score_up,
                rateup: false,
            },
            score_up: ScoreUp {
                default: score_up,
                unification_activate_effect_value: None,
                unification_activate_condition_band_id: None,
                unification_activate_condition_type: None,
            },
        }
    }
}
