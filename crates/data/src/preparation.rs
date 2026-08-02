use crate::{DataError, GameDataSnapshot};
use bangdream_optimize_core::{
    calculate_area_item_percent, event_point_bonus_percent, prepare_cards, AreaItemPercent,
    CardDefinition, Chart, EventBonus, EventBonusApplication, EventType, PlayerConfig,
    PreferredItemTarget, PreparedCard, SongSelection,
};
use std::collections::BTreeMap;

const POINT_BONUS_MICROS_PER_PERCENT: f64 = 1_000_000.0;
const POINT_BONUS_MICROS_PER_TENTH_PERCENT: u64 = 100_000;

#[derive(Debug, Clone)]
pub struct PreparedEventContext {
    pub event_id: u32,
    pub event_type: EventType,
    pub cards_with_stat_bonus: Vec<PreparedCard>,
    pub cards_without_event_bonus: Vec<PreparedCard>,
    pub point_bonus_micros: BTreeMap<u32, u64>,
    pub all_point_bonus_micros: BTreeMap<u32, u64>,
    pub area_item_percent: AreaItemPercent,
    pub preferred: Option<PreferredItemTarget>,
}

impl PreparedEventContext {
    /// Maximum-score searches treat event bonuses as deck-power bonuses, including Challenge.
    pub fn maximize_cards(&self) -> &[PreparedCard] {
        &self.cards_with_stat_bonus
    }

    /// Target-PT searches treat Challenge, Live Try, and Mission Live bonuses as PT multipliers.
    pub fn score_range_cards(&self) -> &[PreparedCard] {
        if event_uses_point_bonus(self.event_type) {
            &self.cards_without_event_bonus
        } else {
            &self.cards_with_stat_bonus
        }
    }

    pub fn pt_maximize_cards(&self, application: EventBonusApplication) -> &[PreparedCard] {
        match application {
            EventBonusApplication::TeamStat => &self.cards_with_stat_bonus,
            EventBonusApplication::PointMultiplier => &self.cards_without_event_bonus,
        }
    }

    pub fn pt_maximize_point_bonus_micros(
        &self,
        application: EventBonusApplication,
    ) -> &BTreeMap<u32, u64> {
        match application {
            EventBonusApplication::TeamStat => &self.point_bonus_micros,
            EventBonusApplication::PointMultiplier => &self.all_point_bonus_micros,
        }
    }
}

fn event_uses_point_bonus(event_type: EventType) -> bool {
    matches!(
        event_type,
        EventType::LiveTry | EventType::Challenge | EventType::Festival | EventType::MissionLive
    )
}

fn point_bonus_percent_to_micros(percent: f64) -> u64 {
    let micros = (percent.max(0.0) * POINT_BONUS_MICROS_PER_PERCENT).round() as u64;
    debug_assert_eq!(micros % POINT_BONUS_MICROS_PER_TENTH_PERCENT, 0);
    micros
}

pub fn prepare_event_context(
    data: &GameDataSnapshot,
    player: &PlayerConfig,
    event_id: Option<u32>,
) -> Result<PreparedEventContext, DataError> {
    let event_id = event_id
        .or(player.current_event)
        .ok_or(DataError::MissingCurrentEvent)?;
    let event = data.events.get(&event_id).ok_or(DataError::MissingEntity {
        kind: "event",
        id: event_id.to_string(),
    })?;
    let card_definitions = player_card_definitions(player, data)?;
    let cards = prepare_cards(
        &card_definitions,
        &player.card_list,
        &player.character_bouns,
        &event.event_bonus,
    )?;
    let uses_point_bonus = event_uses_point_bonus(event.event_type);
    let cards_without_event_bonus = prepare_cards(
        &card_definitions,
        &player.card_list,
        &player.character_bouns,
        &EventBonus::default(),
    )?;
    let all_point_bonus_micros = cards_without_event_bonus
        .iter()
        .map(|card| {
            (
                card.card_id,
                point_bonus_percent_to_micros(event_point_bonus_percent(card, &event.event_bonus)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let point_bonus_micros = if uses_point_bonus {
        all_point_bonus_micros.clone()
    } else {
        BTreeMap::new()
    };
    let area_item_percent =
        calculate_area_item_percent(&player.area_item, &data.area_item_definitions)?;

    Ok(PreparedEventContext {
        event_id,
        event_type: event.event_type,
        cards_with_stat_bonus: cards,
        cards_without_event_bonus,
        point_bonus_micros,
        all_point_bonus_micros,
        area_item_percent,
        preferred: event.preferred.clone(),
    })
}

pub fn event_songs(player: &PlayerConfig, event_id: u32) -> Result<Vec<SongSelection>, DataError> {
    player
        .event_songs
        .get(&event_id.to_string())
        .cloned()
        .ok_or(DataError::MissingEventSongs { event_id })
}

pub fn initialized_charts(
    data: &GameDataSnapshot,
    song_list: &[SongSelection],
    event_type: EventType,
) -> Result<Vec<Chart>, DataError> {
    let is_medley = event_type == EventType::Medley;
    let mut combo = 0;
    let mut charts = Vec::with_capacity(song_list.len());

    for song in song_list {
        let mut chart =
            data.chart(song.song_id, song.difficulty)
                .cloned()
                .ok_or(DataError::MissingEntity {
                    kind: "chart",
                    id: format!("{}:{}", song.song_id, song.difficulty),
                })?;
        chart.init(combo, is_medley)?;
        combo += chart.count as i32;
        charts.push(chart);
    }

    Ok(charts)
}

pub(crate) fn player_card_definitions(
    player: &PlayerConfig,
    data: &GameDataSnapshot,
) -> Result<Vec<CardDefinition>, DataError> {
    player
        .card_list
        .keys()
        .map(|card_id| {
            let parsed_id = card_id
                .parse::<u32>()
                .map_err(|_| DataError::InvalidField {
                    field: "cardList.cardId",
                    value: card_id.clone(),
                })?;
            data.card_definitions
                .get(&parsed_id)
                .cloned()
                .ok_or(DataError::MissingEntity {
                    kind: "card",
                    id: card_id.clone(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_multiplier_events_include_festival_solo() {
        assert!(event_uses_point_bonus(EventType::Festival));
        assert!(event_uses_point_bonus(EventType::Challenge));
        assert!(!event_uses_point_bonus(EventType::Versus));
        assert!(!event_uses_point_bonus(EventType::Medley));
    }

    #[test]
    fn point_bonus_float_artifacts_still_map_to_tenth_percent_units() {
        assert_eq!(point_bonus_percent_to_micros(0.20000000298023224), 200_000);
        assert_eq!(point_bonus_percent_to_micros(0.6000000238418579), 600_000);
        assert_eq!(point_bonus_percent_to_micros(15.0), 15_000_000);
    }
}
