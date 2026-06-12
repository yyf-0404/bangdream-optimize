use crate::model::{
    chart::{Chart, TeamCardSkill},
    dp::{DpChartModel, DpModelError, ModelTerm, SongMode},
    preparation::{AreaItemPercent, PreparedCard},
    schema::SelectedAreaItems,
};
pub use bangdream_optimize_single_dp::SingleDpResult as SingleSongDpResult;
use bangdream_optimize_single_dp::{
    solve_single_dp, SingleDpCard, SingleDpError, SingleDpInput, SingleDpTerm, TEAM_SIZE,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SingleSongDpError {
    #[error("at least five cards are required to build a team, got {count}")]
    NotEnoughCards { count: usize },

    #[error("no valid single-song DP result found")]
    NoResult,

    #[error("DP model error: {0}")]
    Model(#[from] DpModelError),
}

impl From<SingleDpError> for SingleSongDpError {
    fn from(error: SingleDpError) -> Self {
        match error {
            SingleDpError::NotEnoughCards { count } => Self::NotEnoughCards { count },
            SingleDpError::NoResult => Self::NoResult,
        }
    }
}

pub fn calculate_single_song_dp(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
) -> Result<SingleSongDpResult, SingleSongDpError> {
    let input = single_dp_input(cards, chart, area_item_percent, selected_items, mode)?;
    solve_single_dp(&input).map_err(Into::into)
}

fn single_dp_input(
    cards: &[PreparedCard],
    chart: &Chart,
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
) -> Result<SingleDpInput, SingleSongDpError> {
    let cards = numeric_card_sources(cards, area_item_percent, selected_items, mode)?;
    let anchor_stat = estimate_single_anchor_stat(&cards);
    let model = DpChartModel::from_chart_with_anchor(chart, anchor_stat)?;
    let mut numeric_cards = Vec::with_capacity(cards.len());

    for card in cards {
        let mut normal_terms = [SingleDpTerm::ZERO; TEAM_SIZE];
        for (position, term) in normal_terms.iter_mut().enumerate() {
            *term = single_dp_term(model.skill_term(chart, position, card.skill)?);
        }

        numeric_cards.push(SingleDpCard {
            card_id: card.card_id,
            group_id: card.character_id,
            stat: card.stat,
            normal_terms,
            captain_term: single_dp_term(model.skill_term(chart, TEAM_SIZE, card.skill)?),
        });
    }

    Ok(SingleDpInput {
        d: model.d,
        c: model.c,
        cards: numeric_cards,
    })
}

fn numeric_card_sources(
    cards: &[PreparedCard],
    area_item_percent: &AreaItemPercent,
    selected_items: &SelectedAreaItems,
    mode: SongMode,
) -> Result<Vec<NumericCardSource>, SingleSongDpError> {
    cards
        .iter()
        .filter(|card| mode.allows(card))
        .map(|card| {
            Ok(NumericCardSource {
                card_id: card.card_id,
                character_id: card.character_id,
                stat: card
                    .add_up_stat(
                        area_item_percent,
                        &selected_items.band,
                        &selected_items.attribute,
                        selected_items.magazine.as_str(),
                    )
                    .floor() as i32,
                skill: mode.resolve_skill(card)?,
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
struct NumericCardSource {
    card_id: u32,
    character_id: u32,
    stat: i32,
    skill: TeamCardSkill,
}

fn estimate_single_anchor_stat(cards: &[NumericCardSource]) -> i32 {
    let mut cards = cards.iter().collect::<Vec<_>>();
    cards.sort_by(|left, right| {
        right
            .stat
            .cmp(&left.stat)
            .then_with(|| left.card_id.cmp(&right.card_id))
    });

    let mut characters = Vec::with_capacity(TEAM_SIZE);
    let mut stat = 0;
    for card in cards {
        if characters.contains(&card.character_id) {
            continue;
        }
        characters.push(card.character_id);
        stat += card.stat;
        if characters.len() == TEAM_SIZE {
            break;
        }
    }

    stat.max(1)
}

fn single_dp_term(term: ModelTerm) -> SingleDpTerm {
    SingleDpTerm {
        sb: term.sb,
        c: term.c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chart::{ChartNode, ChartNodeType},
        preparation::{ScoreUp, StatValue},
        schema::{Attribute, Magazine},
    };

    #[test]
    fn single_song_dp_matches_bruteforce_model() {
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

        let dp = calculate_single_song_dp(&cards, &chart, &area, &selected_items, SongMode::Mixed)
            .unwrap();
        let input =
            single_dp_input(&cards, &chart, &area, &selected_items, SongMode::Mixed).unwrap();
        let brute = brute_force_single_song(&input);

        assert_eq!(dp.score, brute.score);
        assert_eq!(dp.stat, brute.stat);
        assert_eq!(dp.captain_card_id, brute.captain_card_id);
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
            calculate_single_song_dp(&cards, &chart, &area, &selected_items, SongMode::Mixed)
                .unwrap();
        let unified = calculate_single_song_dp(
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
    fn single_song_dp_drops_dominated_same_character_card() {
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

        let dp = calculate_single_song_dp(&cards, &chart, &area, &selected_items, SongMode::Mixed)
            .unwrap();

        assert!(dp.team_card_ids.contains(&2));
        assert!(!dp.team_card_ids.contains(&1));
    }

    fn brute_force_single_song(input: &SingleDpInput) -> SingleSongDpResult {
        let mut best: Option<SingleSongDpResult> = None;
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
                    let candidate = SingleSongDpResult {
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
