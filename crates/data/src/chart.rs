use crate::{
    utils::{get_array, get_f64},
    DataError,
};
use bangdream_optimize_core::{Chart, ChartNode, ChartNodeType};
use serde_json::Value;

pub fn chart_from_bestdori(level: i32, chart_data: &Value) -> Result<Chart, DataError> {
    let notes = chart_data.as_array().ok_or(DataError::InvalidField {
        field: "chart",
        value: "expected array".to_owned(),
    })?;

    let mut bpm_points: Vec<BpmPoint> = notes
        .iter()
        .filter(|note| note.get("type").and_then(Value::as_str) == Some("BPM"))
        .map(|note| {
            Ok(BpmPoint {
                beat: get_f64(note, "beat")?,
                bpm: get_f64(note, "bpm")?,
                time: 0.0,
            })
        })
        .collect::<Result<Vec<_>, DataError>>()?;
    bpm_points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    compute_bpm_times(&mut bpm_points)?;

    let mut nodes = Vec::new();
    for note in notes {
        match note.get("type").and_then(Value::as_str) {
            Some("Long") | Some("Slide") => {
                for connection in get_array(note, "connections")? {
                    if connection
                        .get("hidden")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    nodes.push(ChartNode {
                        node_type: node_type(connection),
                        time: time_for_beat(&bpm_points, get_f64(connection, "beat")?)?,
                    });
                }
            }
            Some("Single") | Some("Directional") => {
                nodes.push(ChartNode {
                    node_type: node_type(note),
                    time: time_for_beat(&bpm_points, get_f64(note, "beat")?)?,
                });
            }
            _ => {}
        }
    }

    Ok(Chart::new(level, nodes))
}

#[derive(Debug, Clone, Copy)]
struct BpmPoint {
    beat: f64,
    bpm: f64,
    time: f64,
}

fn compute_bpm_times(points: &mut [BpmPoint]) -> Result<(), DataError> {
    if points.is_empty() {
        return Err(DataError::MissingField { field: "BPM" });
    }

    points[0].time = 0.0;
    for idx in 1..points.len() {
        points[idx].time = points[idx - 1].time
            + (points[idx].beat - points[idx - 1].beat) * (60.0 / points[idx - 1].bpm);
    }
    Ok(())
}

fn time_for_beat(points: &[BpmPoint], beat: f64) -> Result<f64, DataError> {
    let mut last = points
        .first()
        .ok_or(DataError::MissingField { field: "BPM" })?;
    for point in points {
        if point.beat > beat {
            break;
        }
        last = point;
    }
    Ok(last.time + 60.0 / last.bpm * (beat - last.beat))
}

fn node_type(value: &Value) -> ChartNodeType {
    if value.get("skill").and_then(Value::as_bool).unwrap_or(false) {
        ChartNodeType::Skill
    } else {
        ChartNodeType::Node
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_bestdori_chart_to_timed_nodes() {
        let chart = chart_from_bestdori(
            25,
            &json!([
                {"type": "BPM", "beat": 0, "bpm": 120},
                {"type": "Single", "beat": 1, "skill": true},
                {"type": "Single", "beat": 2},
                {"type": "Long", "connections": [
                    {"beat": 3},
                    {"beat": 4, "skill": true},
                    {"beat": 5, "hidden": true}
                ]}
            ]),
        )
        .unwrap();

        assert_eq!(chart.level, 25);
        assert_eq!(chart.count, 4);
        assert_eq!(chart.nodes[0].node_type, ChartNodeType::Skill);
        assert!((chart.nodes[0].time - 0.5).abs() < 1e-9);
        assert_eq!(
            chart
                .nodes
                .iter()
                .filter(|node| node.node_type == ChartNodeType::Skill)
                .count(),
            2
        );
    }
}
