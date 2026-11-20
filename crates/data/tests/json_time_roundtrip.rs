use bangdream_optimize_core::hit_timing::{replay, NoteWindow, ReplayInput, ReplayNote};
use bangdream_optimize_data::timing_chart::time_distance::{
    TimeDistanceChart, TimeDistanceScoring,
};
use serde_json::Value;

#[test]
fn chart_decimals_parse_to_the_nearest_f64() {
    // Observed in the 187 SPECIAL source and serialized timing requests.
    for text in ["205.58333333333334", "103.30329021788259"] {
        let expected = text.parse::<f64>().unwrap().to_bits();
        let direct: f64 = serde_json::from_str(text).unwrap();
        let value: Value = serde_json::from_slice(text.as_bytes()).unwrap();
        let reader: f64 = serde_json::from_reader(text.as_bytes()).unwrap();
        for actual in [direct, value.as_f64().unwrap(), reader] {
            assert_eq!(actual.to_bits(), expected, "JSON literal {text}");
        }
    }
}

#[test]
fn replay_note_time_survives_repeated_json_transports() {
    let mut note = ReplayNote {
        time: 103.30329021788259,
        trigger: None,
        window: NoteWindow::Standard,
    };
    let expected = note.time.to_bits();
    for hop in 1..=8 {
        let bytes = serde_json::to_vec(&note).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value["time"].as_f64().unwrap().to_bits(),
            expected,
            "hop {hop}"
        );
        note = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(note.time.to_bits(), expected, "typed hop {hop}");
    }
}

#[test]
fn fractional_chart_beat_keeps_time_through_preparation_and_request_json() {
    // The tempo prefix and fractional beat that exposed the accumulated drift.
    let raw: Value = serde_json::from_str(
        r#"[
            {"type":"BPM","beat":0,"bpm":80},
            {"type":"BPM","beat":32,"bpm":79},
            {"type":"BPM","beat":34,"bpm":73},
            {"type":"BPM","beat":36,"bpm":70},
            {"type":"BPM","beat":38,"bpm":63},
            {"type":"BPM","beat":40,"bpm":43},
            {"type":"BPM","beat":42,"bpm":42},
            {"type":"BPM","beat":44,"bpm":62},
            {"type":"BPM","beat":46,"bpm":63},
            {"type":"BPM","beat":48,"bpm":150},
            {"type":"Single","beat":205.58333333333334,"lane":0}
        ]"#,
    )
    .unwrap();
    let chart = TimeDistanceChart::from_bestdori(&raw).unwrap();
    let mut input = chart
        .input(TimeDistanceScoring {
            power: 300000.25,
            play_level: 27,
            skills: Vec::new(),
            initial_combo: 0,
            medley_combo: true,
            fever: false,
        })
        .unwrap();
    let expected_time = 103.30329021788256_f64.to_bits();
    assert_eq!(input.notes[0].time.to_bits(), expected_time);
    // Preparation uses the first frame at or after the nominal note time.
    assert_eq!(input.hit_frames, [12397]);
    let expected_input = serde_json::to_value(&input).unwrap();
    let expected_score = replay(&input).unwrap().total;
    for _ in 0..4 {
        let bytes = serde_json::to_vec(&input).unwrap();
        input = serde_json::from_slice::<ReplayInput>(&bytes).unwrap();
        assert_eq!(input.notes[0].time.to_bits(), expected_time);
        assert_eq!(serde_json::to_value(&input).unwrap(), expected_input);
        assert_eq!(replay(&input).unwrap().total, expected_score);
    }
}
