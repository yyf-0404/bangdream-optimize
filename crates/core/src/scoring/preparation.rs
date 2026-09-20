//! Preserve per-axis native operands for production scoring and search.
use super::{nonnegative, AreaBonus, DeckPower, MemberPower, ScoringError};
use crate::{
    AreaItemConfig, AreaItemDefinition, AreaItemType, Attribute, EventBonus, PreparedCard,
    SelectedAreaItems,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PowerPreparationError {
    #[error(transparent)]
    Arithmetic(#[from] ScoringError),
    #[error(transparent)]
    Preparation(#[from] crate::PreparationError),
    #[error("card {card_id} has a non-integer or out-of-range original parameter")]
    InvalidOriginal { card_id: u32 },
    #[error("area item {item_id} at level {level} lacks original percentages; reload master data")]
    MissingPercentages { item_id: u32, level: u8 },
    #[error("area item {item_id} is absent or repeated in the explicit equipment order")]
    InvalidEquipmentOrder { item_id: u32 },
    #[error("selected {kind} item group {key} is not available")]
    InvalidSelection { kind: &'static str, key: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedPowerCard {
    pub card_id: u32,
    pub band_id: u32,
    pub attribute: Attribute,
    pub original: [u32; 3],
    pub event_percent: f32,
    pub extra_event: [f32; 3],
}

impl PreparedPowerCard {
    /// Original growth remains the existing, separately rounded integer model.
    /// event_add_stat is deliberately not read: it has already lost ordering.
    pub fn new(card: &PreparedCard, event: &EventBonus) -> Result<Self, PowerPreparationError> {
        let values = [card.stat.performance, card.stat.technique, card.stat.visual];
        if values
            .iter()
            .any(|&v| !v.is_finite() || v < 0.0 || v >= 4_294_967_296.0 || v.fract() != 0.0)
        {
            return Err(PowerPreparationError::InvalidOriginal {
                card_id: card.card_id,
            });
        }
        let original = values.map(|v| v as u32);
        let sum = |values: Vec<f64>| -> Result<f32, ScoringError> {
            let mut total = 0.0_f32;
            for v in values {
                total += nonnegative(v as f32, "event percentage")?;
            }
            nonnegative(total, "event percentage total")
        };
        let character = sum(event
            .characters
            .iter()
            .filter(|b| b.character_id == card.character_id)
            .map(|b| b.percent)
            .collect())?;
        let attribute = sum(event
            .attributes
            .iter()
            .filter(|b| b.attribute == card.attribute)
            .map(|b| b.percent)
            .collect())?;
        let joint = if character > 0.0 && attribute > 0.0 {
            nonnegative(
                event.event_attribute_and_character_parameter_percent as f32,
                "joint event percentage",
            )?
        } else {
            0.0
        };
        let member = sum(event
            .members
            .iter()
            .filter(|b| b.card_id == card.card_id)
            .map(|b| b.percent)
            .collect())?;
        let limit = nonnegative(
            event
                .limit_breaks
                .get(&card.rarity)
                .and_then(|b| b.get(&card.limit_break_rank))
                .copied()
                .unwrap_or(0.0) as f32,
            "limit-break event percentage",
        )?;
        // EventBuffData: ((character + attribute) + joint) + situation + limit.
        let event_percent = nonnegative(
            ((character + attribute) + joint) + member + limit,
            "event percentage",
        )?;
        let mut extra_event = [0.0; 3];
        if character > 0.0 && attribute > 0.0 {
            if let Some(extra) = event.event_character_parameter_bonus {
                for (i, percent) in [extra.performance, extra.technique, extra.visual]
                    .into_iter()
                    .enumerate()
                {
                    let percent = nonnegative(percent as f32, "extra event percentage")?;
                    extra_event[i] = nonnegative(
                        (original[i] as f32 * percent) / 100.0_f32,
                        "extra event value",
                    )?;
                }
            }
        }
        Ok(Self {
            card_id: card.card_id,
            band_id: card.band_id,
            attribute: card.attribute,
            original,
            event_percent,
            extra_event,
        })
    }

    pub fn area_bonus(&self, equipment: &[EquippedAreaItem]) -> Result<AreaBonus, ScoringError> {
        let points: Vec<_> = equipment
            .iter()
            .filter(|item| item.applies_to(self))
            .map(|item| item.percent_points)
            .collect();
        AreaBonus::from_effects(&[], &points)
    }
    pub fn power(&self, equipment: &[EquippedAreaItem]) -> Result<MemberPower, ScoringError> {
        let area = self.area_bonus(equipment)?;
        MemberPower::from_bonuses(
            self.original,
            area.fixed,
            area.rate,
            self.event_percent,
            self.extra_event,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EquippedAreaItem {
    pub item_id: u32,
    pub band_ids: Vec<u32>,
    pub attributes: Vec<Attribute>,
    pub percent_points: [f32; 3],
}
impl EquippedAreaItem {
    fn applies_to(&self, card: &PreparedPowerCard) -> bool {
        (self.band_ids.is_empty() || self.band_ids.contains(&card.band_id))
            && (self.attributes.is_empty()
                || self.attributes.contains(&Attribute::All)
                || self.attributes.contains(&card.attribute))
    }
}

/// Resolve the app's existing grouped equipment choices. `item_order` must
/// explicitly contain every owned item once; it is never silently re-sorted.
/// Inventory JSON has no original MapController placement order, so callers
/// must record their chosen order rather than claim to recover device state.
pub fn selected_equipment(
    inventory: &BTreeMap<String, AreaItemConfig>,
    definitions: &BTreeMap<u32, AreaItemDefinition>,
    selected: &SelectedAreaItems,
    item_order: &[u32],
) -> Result<Vec<EquippedAreaItem>, PowerPreparationError> {
    let groups = crate::calculate_area_item_percent(inventory, definitions)?;
    for (kind, key, exists) in [
        (
            "band",
            selected.band.as_str(),
            groups.band.contains_key(&selected.band),
        ),
        (
            "attribute",
            selected.attribute.as_str(),
            groups.attribute.contains_key(&selected.attribute),
        ),
        (
            "magazine",
            selected.magazine.as_str(),
            groups.magazine.contains_key(selected.magazine.as_str()),
        ),
    ] {
        if !exists {
            return Err(PowerPreparationError::InvalidSelection {
                kind,
                key: key.to_owned(),
            });
        }
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for &item_id in item_order {
        let Some(config) = inventory.get(&item_id.to_string()) else {
            return Err(PowerPreparationError::InvalidEquipmentOrder { item_id });
        };
        if !seen.insert(item_id) {
            return Err(PowerPreparationError::InvalidEquipmentOrder { item_id });
        }
        let def = &definitions[&item_id]; // validated by calculate_area_item_percent
        let key = def.target_key()?;
        let chosen = match def.item_type() {
            AreaItemType::Band => selected.band.as_str(),
            AreaItemType::Attribute => selected.attribute.as_str(),
            AreaItemType::Magazine => selected.magazine.as_str(),
        };
        if key != chosen || config.level == 0 {
            continue;
        }
        // Keep the old app's whole-set availability and 59/72 alternative rules.
        let disabled = inventory.iter().any(|(id, level)| {
            level.level == 0
                && !matches!(id.as_str(), "59" | "72")
                && definitions.get(&id.parse().unwrap()).is_some_and(|other| {
                    other.item_type() == def.item_type()
                        && other.target_key().ok().as_deref() == Some(&key)
                })
        });
        if disabled {
            continue;
        }
        if matches!(item_id, 59 | 72) {
            if let (Some(shell), Some(coffee)) = (inventory.get("59"), inventory.get("72")) {
                let unused = if shell.level < coffee.level { 59 } else { 72 };
                if item_id == unused {
                    continue;
                }
            }
        }
        let points = def.percent_points.get(&config.level).copied().ok_or(
            PowerPreparationError::MissingPercentages {
                item_id,
                level: config.level,
            },
        )?;
        result.push(EquippedAreaItem {
            item_id,
            band_ids: def.target_band_ids.clone(),
            attributes: def.target_attributes.clone(),
            percent_points: points,
        });
    }
    for id in inventory.keys() {
        let item_id = id.parse().expect("inventory keys validated above");
        if !seen.contains(&item_id) {
            return Err(PowerPreparationError::InvalidEquipmentOrder { item_id });
        }
    }
    Ok(result)
}

pub fn deck_power(
    cards: &[&PreparedPowerCard],
    equipment: &[EquippedAreaItem],
) -> Result<DeckPower, ScoringError> {
    let members = cards
        .iter()
        .map(|card| card.power(equipment))
        .collect::<Result<Vec<_>, _>>()?;
    DeckPower::from_members(&members)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preparation::{StatRate, StatValue};
    use crate::{EventAttributeBonus, EventCharacterBonus, Magazine, ScoreUp, TeamCardSkill};

    fn card() -> PreparedCard {
        PreparedCard {
            native_power: None,
            card_id: 1,
            character_id: 1,
            band_id: 4,
            rarity: 5,
            attribute: Attribute::Cool,
            level: 60,
            training: true,
            illust_training_status: true,
            episodes: [true; 2],
            limit_break_rank: 0,
            skill_level: 5,
            stat: StatValue {
                performance: 10001.0,
                technique: 11003.0,
                visual: 12007.0,
            },
            event_add_stat: StatValue {
                performance: 999999.0,
                technique: 999999.0,
                visual: 999999.0,
            },
            skill: TeamCardSkill {
                card_id: 1,
                duration: 7.0,
                score_up: 1.2,
                rateup: false,
            },
            score_up: ScoreUp {
                default: 1.2,
                unification_activate_effect_value: None,
                unification_activate_condition_band_id: None,
                unification_activate_condition_type: None,
            },
        }
    }
    fn definition(id: u32, band: bool, points: f32) -> AreaItemDefinition {
        AreaItemDefinition {
            area_item_id: id,
            target_band_ids: if band { vec![4] } else { vec![] },
            target_attributes: if band { vec![] } else { vec![Attribute::Cool] },
            percents: BTreeMap::from([(1, StatRate::all(points as f64 / 100.0))]),
            percent_points: BTreeMap::from([(1, [points; 3])]),
        }
    }
    fn selection() -> SelectedAreaItems {
        SelectedAreaItems {
            band: "4".into(),
            attribute: "cool".into(),
            magazine: Magazine::Performance,
        }
    }
    #[test]
    fn reads_original_axes_and_separates_event_bonus() {
        let mut event = EventBonus::default();
        event.attributes.push(EventAttributeBonus {
            attribute: Attribute::Cool,
            percent: 10.0,
        });
        event.characters.push(EventCharacterBonus {
            character_id: 1,
            percent: 20.0,
        });
        event.event_attribute_and_character_parameter_percent = 20.0;
        event.event_character_parameter_bonus = Some(StatRate {
            performance: 50.0,
            technique: 0.0,
            visual: 0.0,
        });
        let p = PreparedPowerCard::new(&card(), &event).unwrap();
        assert_eq!(p.event_percent, 50.0);
        assert_eq!(p.extra_event, [5000.5, 0.0, 0.0]);
        assert_eq!(
            p.power(&[]).unwrap().final_parameters(),
            [20002.0, 16504.5, 18010.5]
        );
        let clean = PreparedPowerCard::new(&card(), &EventBonus::default()).unwrap();
        assert_eq!(
            clean.power(&[]).unwrap().final_parameters(),
            [10001.0, 11003.0, 12007.0]
        );
        event.attributes[0].percent = 0.0;
        let p = PreparedPowerCard::new(&card(), &event).unwrap();
        assert_eq!(p.event_percent, 20.0);
        assert_eq!(p.extra_event, [0.0; 3]);
        let mut c = card();
        c.stat.performance = 10001.25;
        assert!(matches!(
            PreparedPowerCard::new(&c, &event),
            Err(PowerPreparationError::InvalidOriginal { .. })
        ));
    }
    #[test]
    fn raw_equipment_respects_order_targets_and_incomplete_groups() {
        let mut inventory = BTreeMap::from([
            ("1".into(), AreaItemConfig { level: 1 }),
            ("2".into(), AreaItemConfig { level: 1 }),
            ("50".into(), AreaItemConfig { level: 1 }),
        ]);
        let definitions = BTreeMap::from([
            (1, definition(1, true, 5.5)),
            (2, definition(2, true, 6.5)),
            (50, definition(50, false, 20.0)),
        ]);
        let equipment =
            selected_equipment(&inventory, &definitions, &selection(), &[50, 2, 1]).unwrap();
        assert_eq!(
            equipment.iter().map(|i| i.item_id).collect::<Vec<_>>(),
            vec![50, 2, 1]
        );
        let mut p = PreparedPowerCard::new(&card(), &EventBonus::default()).unwrap();
        assert_eq!(p.area_bonus(&equipment).unwrap().rate, [0.32; 3]);
        p.band_id = 1;
        assert_eq!(p.area_bonus(&equipment).unwrap().rate, [0.2; 3]);
        inventory.get_mut("2").unwrap().level = 0;
        assert_eq!(
            selected_equipment(&inventory, &definitions, &selection(), &[1, 2, 50])
                .unwrap()
                .len(),
            1
        );
        assert!(selected_equipment(&inventory, &definitions, &selection(), &[1, 50]).is_err());
        assert!(selected_equipment(&inventory, &definitions, &selection(), &[1, 1, 50]).is_err());
    }
    #[test]
    fn old_snapshots_decode_but_cannot_silently_reconstruct_percentages() {
        let mut value = serde_json::to_value(definition(1, true, 5.5)).unwrap();
        value.as_object_mut().unwrap().remove("percentPoints");
        let decoded: AreaItemDefinition = serde_json::from_value(value).unwrap();
        let mut selected = selection();
        selected.attribute = crate::preparation::ALL_ATTRIBUTE_KEY.into();
        assert!(matches!(
            selected_equipment(
                &BTreeMap::from([("1".into(), AreaItemConfig { level: 1 })]),
                &BTreeMap::from([(1, decoded)]),
                &selected,
                &[1]
            ),
            Err(PowerPreparationError::Preparation(
                crate::PreparationError::NativePower { .. }
            ))
        ));
    }
    #[test]
    fn shell_and_coffee_choose_one_before_native_accumulation() {
        let inventory = BTreeMap::from([
            ("59".into(), AreaItemConfig { level: 1 }),
            ("72".into(), AreaItemConfig { level: 1 }),
        ]);
        let mut a = definition(59, false, 10.0);
        a.target_attributes = vec![
            Attribute::Cool,
            Attribute::Happy,
            Attribute::Powerful,
            Attribute::Pure,
        ];
        let mut b = a.clone();
        b.area_item_id = 72;
        let defs = BTreeMap::from([(59, a), (72, b)]);
        let selected = SelectedAreaItems {
            band: crate::preparation::ALL_BAND_KEY.into(),
            attribute: "cool,happy,powerful,pure".into(),
            magazine: Magazine::Performance,
        };
        let equipment = selected_equipment(&inventory, &defs, &selected, &[72, 59]).unwrap();
        assert_eq!(equipment.len(), 1);
        assert_eq!(equipment[0].item_id, 59);
    }
}
