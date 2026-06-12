use crate::model::chart::{Chart, ChartNode, ChartNodeType, TeamCardSkill};
use crate::model::preparation::{PreparedCard, ScoreUp, StatValue};
use crate::model::schema::{Attribute, Magazine, SelectedAreaItems};

pub(in crate::medley) fn chart() -> Chart {
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
    let mut chart = Chart::new(5, nodes);
    chart.init(0, false).unwrap();
    chart
}

pub(in crate::medley) fn medley_charts() -> Vec<Chart> {
    vec![chart(), chart(), chart()]
}

pub(in crate::medley) fn selected_cool_items() -> SelectedAreaItems {
    SelectedAreaItems {
        band: "1".to_owned(),
        attribute: "cool".to_owned(),
        magazine: Magazine::Performance,
    }
}

pub(in crate::medley) fn prepared_card(
    card_id: u32,
    character_id: u32,
    band_id: u32,
    attribute: Attribute,
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
