use crate::DataError;
use bangdream_optimize_core::{preparation::StatRate as PrepStatRate, Attribute, Stat};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn parse_attribute(value: &str) -> Result<Attribute, DataError> {
    match value.to_ascii_lowercase().as_str() {
        "cool" => Ok(Attribute::Cool),
        "happy" => Ok(Attribute::Happy),
        "pure" => Ok(Attribute::Pure),
        "powerful" => Ok(Attribute::Powerful),
        "~all" | "all" => Ok(Attribute::All),
        _ => Err(DataError::InvalidField {
            field: "attribute",
            value: value.to_owned(),
        }),
    }
}

pub(crate) fn stat_from_object(value: &Value) -> Result<Stat, DataError> {
    Ok(Stat {
        performance: get_i32(value, "performance")?,
        technique: get_i32(value, "technique")?,
        visual: get_i32(value, "visual")?,
    })
}

pub(crate) fn stat_rate_from_percent_object(value: &Value) -> Result<PrepStatRate, DataError> {
    Ok(PrepStatRate {
        performance: value
            .get("performance")
            .and_then(Value::as_f64)
            .unwrap_or_default(),
        technique: value
            .get("technique")
            .and_then(Value::as_f64)
            .unwrap_or_default(),
        visual: value
            .get("visual")
            .and_then(Value::as_f64)
            .unwrap_or_default(),
    })
}

pub(crate) fn normalize_object(value: &mut Value, field: &'static str) -> Result<(), DataError> {
    if value.is_object() {
        return Ok(());
    }
    Err(DataError::InvalidField {
        field,
        value: "expected object".to_owned(),
    })
}

pub(crate) fn into_object_map(
    value: Value,
    field: &'static str,
) -> Result<BTreeMap<String, Value>, DataError> {
    value
        .as_object()
        .ok_or(DataError::InvalidField {
            field,
            value: "expected object".to_owned(),
        })?
        .iter()
        .map(|(key, value)| Ok((key.clone(), value.clone())))
        .collect()
}

pub(crate) fn merge_object_map(
    target: &mut BTreeMap<String, Value>,
    source: Value,
    field: &'static str,
) -> Result<(), DataError> {
    let source = source.as_object().ok_or(DataError::InvalidField {
        field,
        value: "expected object".to_owned(),
    })?;
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
    Ok(())
}

pub(crate) fn get_array<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a Vec<Value>, DataError> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or(DataError::MissingField { field })
}

pub(crate) fn optional_array<'a>(value: &'a Value, field: &'static str) -> Option<&'a Vec<Value>> {
    value.get(field).and_then(Value::as_array)
}

pub(crate) fn get_str<'a>(value: &'a Value, field: &'static str) -> Result<&'a str, DataError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(DataError::MissingField { field })
}

pub(crate) fn get_f64(value: &Value, field: &'static str) -> Result<f64, DataError> {
    value
        .get(field)
        .map(|value| parse_numeric_value(value, field))
        .transpose()?
        .ok_or(DataError::MissingField { field })
}

pub(crate) fn get_i32(value: &Value, field: &'static str) -> Result<i32, DataError> {
    get_f64(value, field).map(|value| value as i32)
}

pub(crate) fn get_u32(value: &Value, field: &'static str) -> Result<u32, DataError> {
    get_f64(value, field).map(|value| value as u32)
}

pub(crate) fn get_u8(value: &Value, field: &'static str) -> Result<u8, DataError> {
    get_f64(value, field).map(|value| value as u8)
}

pub(crate) fn parse_numeric_value(value: &Value, field: &'static str) -> Result<f64, DataError> {
    if let Some(value) = value.as_f64() {
        return Ok(value);
    }
    if let Some(value) = value.as_str() {
        return value.parse::<f64>().map_err(|_| DataError::InvalidField {
            field,
            value: value.to_owned(),
        });
    }
    Err(DataError::InvalidField {
        field,
        value: value.to_string(),
    })
}
