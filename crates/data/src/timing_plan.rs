//! User frame restrictions for a saved fixed-team/fixed-skill-order witness.
//! This endpoint has no game-data IO and never claims a new full-pool optimum.
use bangdream_optimize_core::{
    hit_timing::{independent::JointTimeline, replay, ReplayInput, ReplayResult, TIMING_REVISION},
    scoring::{Judgement, ARITHMETIC_REVISION},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use bangdream_optimize_core::timing::Instant;
use crate::timing_chart::time_distance::TIME_DISTANCE_PREPARATION_REVISION;

#[cfg(test)]
mod simplify;

pub const PLAN_REVISION: &str = "bounded-hit-plan-120-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteFrameRange {
    pub note_index: usize,
    pub first_frame: u32,
    pub last_frame: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimingPlanRequest {
    pub scoring_revision: String,
    pub timing_revision: String,
    pub preparation_revision: String,
    pub input: ReplayInput,
    pub ranges: Vec<NoteFrameRange>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingPlanResponse {
    pub plan_revision: String,
    pub status: String,
    pub message: String,
    pub note_indices: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ReplayResult>,
    pub elapsed_ms: f64,
}

pub fn optimize_timing_plan(request: TimingPlanRequest) -> TimingPlanResponse {
    let started = Instant::now();
    let mut response = TimingPlanResponse {
        plan_revision: PLAN_REVISION.to_owned(), status: "invalid".to_owned(),
        message: String::new(), note_indices: vec![], result: None, elapsed_ms: 0.,
    };
    let run = |response: &mut TimingPlanResponse| {
        if request.scoring_revision != ARITHMETIC_REVISION || request.timing_revision != TIMING_REVISION
            || request.preparation_revision != TIME_DISTANCE_PREPARATION_REVISION {
            response.message = "该击打记录的模型版本不兼容，请重新计算原始方案".to_owned();
            return;
        }
        if request.input.native_time_routing.as_ref().is_none_or(|r|
            r.distance_mode != bangdream_optimize_core::hit_timing::CandidateDistanceMode::ChartTime) {
            response.message = "自定义范围需要当前时间距离模型的击打记录".to_owned();
            return;
        }
        if let Err(error) = replay(&request.input) {
            response.message = format!("原始击打记录无法重放：{error}");
            return;
        }
        let mut limits = vec![None; request.input.notes.len()];
        let mut seen = BTreeSet::new();
        for range in &request.ranges {
            if range.note_index >= limits.len() || !seen.insert(range.note_index)
                || range.first_frame > range.last_frame || range.last_frame > 72_000 {
                response.note_indices = vec![range.note_index];
                response.message = "按键编号必须有效且不重复，帧范围需为 0–72000 内的整数，最早帧不能晚于最晚帧".to_owned();
                return;
            }
            let note = &request.input.notes[range.note_index];
            let frames = match note.frame_domain(Judgement::Bad) {
                Ok(value) => value,
                Err(error) => { response.message = error.to_string(); return; }
            };
            let target = request.input.native_time_routing.as_ref().unwrap().targets[range.note_index].as_ref();
            if !frames.iter().any(|&f| range.first_frame <= f && f <= range.last_frame
                && target.is_none_or(|t| t.active_from_frame <= f && f <= t.active_until_frame)) {
                response.status = "infeasible".to_owned();
                response.note_indices = vec![range.note_index];
                response.message = format!("按键 #{} 的限制范围与有效击打窗口没有交集", range.note_index + 1);
                return;
            }
            limits[range.note_index] = Some([range.first_frame, range.last_frame]);
        }
        let result = JointTimeline::maximum_in_frame_ranges(&request.input, &limits)
            .and_then(|winner| winner.map(|w| JointTimeline::simplest_maximum(&request.input, &limits, &w.result)).transpose());
        match result {
            Ok(Some(winner)) => {
                response.status = "complete".to_owned();
                response.message = "已在当前队伍、技能顺序和自定义范围内完成精确重算".to_owned();
                response.result = Some(winner);
            }
            Ok(None) => {
                response.status = "infeasible".to_owned();
                response.note_indices = seen.into_iter().collect();
                response.message = "这些范围与判定、同轨候选或连接间隔要求无法同时满足，请放宽范围或撤销最近修改".to_owned();
            }
            Err(error) => {
                response.status = "error".to_owned();
                response.message = format!("击打方案重算未完成：{error}");
            }
        }
    };
    run(&mut response);
    response.elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing_chart::time_distance::{TimeDistanceChart, TimeDistanceScoring};
    use serde_json::json;

    fn request() -> TimingPlanRequest {
        let chart = TimeDistanceChart::from_bestdori(&json!([
            {"type":"BPM","beat":0,"bpm":120},
            {"type":"Single","beat":1,"lane":1},
            {"type":"Single","beat":1.25,"lane":2}
        ])).unwrap();
        TimingPlanRequest {
            scoring_revision: ARITHMETIC_REVISION.to_owned(), timing_revision: TIMING_REVISION.to_owned(),
            preparation_revision: TIME_DISTANCE_PREPARATION_REVISION.to_owned(),
            input: chart.input(TimeDistanceScoring { power: 300040.75, play_level: 27,
                skills: vec![], initial_combo: 0, medley_combo: true, fever: false }).unwrap(),
            ranges: vec![],
        }
    }

    #[test]
    fn timing_plan_exact_result_keeps_power_and_replays_inside_inclusive_limits() {
        let mut input = request();
        input.ranges = vec![NoteFrameRange { note_index: 0, first_frame: 58, last_frame: 58 }];
        let mut replay_input = input.input.clone();
        let response = optimize_timing_plan(input);
        assert_eq!(response.status, "complete");
        let result = response.result.unwrap();
        assert_eq!(result.hit_frames[0], 58);
        assert_eq!(result.scoring_power.to_bits(), replay_input.power.to_bits());
        replay_input.hit_frames = result.hit_frames.clone();
        replay_input.hit_order = result.hit_order.clone();
        assert_eq!(replay(&replay_input).unwrap(), result);
    }

    #[test]
    fn timing_plan_rejects_incompatible_versions_duplicate_ids_and_reversed_ranges() {
        let mut old = request(); old.scoring_revision = "old".to_owned();
        assert_eq!(optimize_timing_plan(old).status, "invalid");
        let mut old = request(); old.timing_revision = "typed-time-windows-v12".to_owned();
        assert_eq!(optimize_timing_plan(old).status, "invalid");
        let mut old = request(); old.preparation_revision = "visible-typed-time-midpoint-120-v5".to_owned();
        assert_eq!(optimize_timing_plan(old).status, "invalid");
        for ranges in [
            vec![NoteFrameRange { note_index: 0, first_frame: 62, last_frame: 60 }],
            vec![NoteFrameRange { note_index: 9, first_frame: 60, last_frame: 60 }],
            vec![NoteFrameRange { note_index: 0, first_frame: 60, last_frame: 60 }; 2],
        ] {
            let mut input = request(); input.ranges = ranges;
            let response = optimize_timing_plan(input);
            assert_eq!(response.status, "invalid"); assert!(response.result.is_none());
        }
    }

    #[test]
    fn timing_plan_outside_window_is_an_explicit_infeasible_result() {
        let mut input = request();
        input.ranges = vec![NoteFrameRange { note_index: 1, first_frame: 0, last_frame: 0 }];
        let response = optimize_timing_plan(input);
        assert_eq!(response.status, "infeasible"); assert_eq!(response.note_indices, vec![1]);
        assert!(response.result.is_none());
    }
}
