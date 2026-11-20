//! User limits are an intersection with hit decisions, never a replacement for
//! native candidate lifetimes. A reference chart prevents stale note numbering.
use super::{ReplayInput, ReplayError};
use crate::SongSelection;
use serde::{Deserialize, Serialize};

pub const RANGE_REVISION: &str = "global-relative-ranges-120-v2";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimingRangeRequest {
    pub revision: String,
    pub songs: Vec<TimingSongRanges>,
}

impl TimingRangeRequest {
    pub fn validate_songs(&self, songs: &[SongSelection]) -> Result<(), ReplayError> {
        if self.revision != RANGE_REVISION {
            return Err(ReplayError::Input("范围版本不兼容，请重新计算原始方案"));
        }
        if self.songs.len() != songs.len() || self.songs.iter().zip(songs).any(|(range, song)| {
            range.song_id != song.song_id || range.difficulty != song.difficulty
        }) {
            return Err(ReplayError::Input("击打范围与当前选曲或难度不一致"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimingSongRanges {
    pub song_id: u32,
    pub difficulty: u8,
    pub reference: ReplayInput,
    pub ranges: Vec<RelativeFrameRange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelativeFrameRange {
    pub note_index: usize,
    pub first_offset: i32,
    pub last_offset: i32,
}

impl TimingSongRanges {
    pub fn limits(&self, input: &ReplayInput) -> Result<Vec<Option<[u32; 2]>>, ReplayError> {
        let old = &self.reference;
        if old.notes != input.notes || old.constraints != input.constraints
            || old.native_time_routing != input.native_time_routing || old.fps != input.fps
            || old.play_level != input.play_level || old.initial_combo != input.initial_combo
            || old.medley_combo != input.medley_combo || old.fever_frames != input.fever_frames {
            return Err(ReplayError::Input("谱面或计分上下文已变化，请重新计算原始方案"));
        }
        let mut limits = vec![None; input.notes.len()];
        for r in &self.ranges {
            if r.note_index >= limits.len() || limits[r.note_index].is_some()
                || r.first_offset > r.last_offset
                || r.first_offset > 0 || r.last_offset < 0
                || !(-72_000..=72_000).contains(&r.first_offset)
                || !(-72_000..=72_000).contains(&r.last_offset) {
                return Err(ReplayError::Input("按键编号或相对帧范围无效、重复；范围必须包含原始帧 0"));
            }
            let base = i64::from(input.chart_reference_frame(r.note_index)?);
            let first = (base + i64::from(r.first_offset)).max(0);
            let last = (base + i64::from(r.last_offset)).min(72_000);
            if first > last {
                return Err(ReplayError::Input("限制范围与可计算帧域没有交集"));
            }
            limits[r.note_index] = Some([first as u32, last as u32]);
        }
        Ok(limits)
    }
}
