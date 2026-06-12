use crate::{
    utils::{optional_array, parse_attribute, parse_numeric_value},
    DataError,
};
use bangdream_optimize_core::{preparation::StatRate as PrepStatRate, AreaItemDefinition};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) fn area_item_definition(
    area_item_id: u32,
    area_item: &Value,
    server_index: usize,
) -> Result<AreaItemDefinition, DataError> {
    let target_band_ids = optional_array(area_item, "targetBandIds")
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|value| value.as_u64().map(|value| value as u32))
        .collect();
    let target_attributes = optional_array(area_item, "targetAttributes")
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(Value::as_str)
        .map(parse_attribute)
        .collect::<Result<Vec<_>, DataError>>()?;

    let performance = area_item
        .get("performance")
        .and_then(Value::as_object)
        .ok_or(DataError::MissingField {
            field: "areaItem.performance",
        })?;
    let technique = area_item
        .get("technique")
        .and_then(Value::as_object)
        .ok_or(DataError::MissingField {
            field: "areaItem.technique",
        })?;
    let visual =
        area_item
            .get("visual")
            .and_then(Value::as_object)
            .ok_or(DataError::MissingField {
                field: "areaItem.visual",
            })?;

    let mut percents = BTreeMap::new();
    for level in performance.keys().filter_map(|key| key.parse::<u8>().ok()) {
        percents.insert(
            level,
            PrepStatRate {
                performance: area_item_percent_at(performance, level, server_index)?,
                technique: area_item_percent_at(technique, level, server_index)?,
                visual: area_item_percent_at(visual, level, server_index)?,
            },
        );
    }

    Ok(AreaItemDefinition {
        area_item_id,
        target_band_ids,
        target_attributes,
        percents,
    })
}

fn area_item_percent_at(
    values: &Map<String, Value>,
    level: u8,
    server_index: usize,
) -> Result<f64, DataError> {
    let values = values
        .get(&level.to_string())
        .and_then(Value::as_array)
        .ok_or(DataError::MissingField {
            field: "areaItem.percent[level][server]",
        })?;
    let value = values
        .get(server_index)
        .filter(|value| !value.is_null())
        .or_else(|| values.iter().find(|value| !value.is_null()));
    let Some(value) = value else {
        return Ok(0.0);
    };
    parse_numeric_value(value, "areaItem.percent").map(|value| value / 100.0)
}
