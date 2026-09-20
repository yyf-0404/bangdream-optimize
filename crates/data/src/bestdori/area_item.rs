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
    let mut percent_points = BTreeMap::new();
    for level in performance.keys().filter_map(|key| key.parse::<u8>().ok()) {
        let points = [
            area_item_percent_at(performance, level, server_index)?,
            area_item_percent_at(technique, level, server_index)?,
            area_item_percent_at(visual, level, server_index)?,
        ];
        percent_points.insert(level, points.map(|v| v as f32));
        percents.insert(
            level,
            PrepStatRate {
                performance: points[0] / 100.0,
                technique: points[1] / 100.0,
                visual: points[2] / 100.0,
            },
        );
    }

    Ok(AreaItemDefinition {
        area_item_id,
        target_band_ids,
        target_attributes,
        percents,
        percent_points,
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
    parse_numeric_value(value, "areaItem.percent")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn retains_master_percentages_and_existing_server_fallback() {
        let raw = json!({"targetBandIds":[4],"targetAttributes":["cool"],
            "performance":{"1":[1.1,null,null,3.3]},
            "technique":{"1":[7.5,null,null,null]},
            "visual":{"1":[18,null,null,22.5]}});
        let cn = area_item_definition(1, &raw, 3).unwrap();
        assert_eq!(cn.percent_points[&1], [3.3, 7.5, 22.5]);
        assert_eq!(cn.percents[&1].performance, 3.3 / 100.0);
        let fallback = area_item_definition(1, &raw, 2).unwrap();
        assert_eq!(fallback.percent_points[&1], [1.1, 7.5, 18.0]);
    }
}
