//! Immutable frame weights are bound to the owner's complete input and raw
//! correction domain. FIFO request intervals remain private to each build.
use super::*;
use std::sync::Arc;

const FRAME_BYTES: usize = 4*1024*1024;
const ROUNDING_BYTES: usize = 64*1024;

#[derive(Default, Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub frame_hits: u64,
    pub frame_builds: u64,
    pub frame_evictions: u64,
    pub frame_payload_bytes: usize,
    pub rounding_hits: u64,
    pub rounding_builds: u64,
    pub rounding_payload_bytes: usize,
}

pub(super) struct Preparation {
    frames: Vec<(u32,FrameBounds)>,
    rounding: Vec<([u32;2],Arc<CompiledScore>,usize)>,
    pub(super) frame_limit: usize,
    pub(super) enabled: bool,
    pub(super) stats: Stats,
}
impl Preparation {
    pub(super) fn new() -> Self {
        Self { frames: Vec::with_capacity(4), rounding: Vec::with_capacity(8),
            frame_limit: FRAME_BYTES,
            enabled: std::env::var("BANDORI_TIMING_DISABLE_NET_GEOMETRY_CACHE").as_deref() != Ok("1"),
            stats: Stats {
                frame_payload_bytes: std::mem::size_of::<Self>()
                    + 4*std::mem::size_of::<(u32,FrameBounds)>(),
                rounding_payload_bytes: 8*std::mem::size_of::<([u32;2],Arc<CompiledScore>,usize)>(),
                ..Default::default()
            } }
    }
    pub(super) fn frames(&mut self, input: &ReplayInput, skills: &[ReplaySkill])
        -> Result<FrameBounds,ReplayError> {
        let correction = skills.iter().map(|s| s.cache_key()[1]).max().unwrap();
        if self.enabled {
            if let Some((_,frames)) = self.frames.iter().find(|r| r.0==correction) {
                self.stats.frame_hits += 1;
                return Ok(frames.clone());
            }
        }
        let frames = FrameBounds::new(input,skills)?;
        self.stats.frame_builds += 1;
        let bytes = frames.geometry_bytes();
        if self.enabled && bytes + std::mem::size_of::<Self>()
            + 4*std::mem::size_of::<(u32,FrameBounds)>() <= self.frame_limit {
            while self.stats.frame_payload_bytes + bytes > self.frame_limit {
                let (_,old)=self.frames.remove(0);
                self.stats.frame_payload_bytes -= old.geometry_bytes();
                self.stats.frame_evictions += 1;
            }
            self.stats.frame_payload_bytes += bytes;
            self.frames.push((correction,frames.clone()));
        }
        Ok(frames)
    }
    pub(super) fn rounding(&mut self, key: [u32;2]) -> Option<Arc<CompiledScore>> {
        if self.enabled {
            if let Some((_,profile,_))=self.rounding.iter().find(|r| r.0==key) {
                self.stats.rounding_hits += 1;
                return Some(profile.clone());
            }
        }
        self.stats.rounding_builds += 1;
        None
    }
    pub(super) fn store_rounding(&mut self, key: [u32;2], profile: Arc<CompiledScore>) {
        let bytes = profile.storage_bytes() + 2*std::mem::size_of::<usize>();
        if self.enabled && bytes+8*std::mem::size_of::<([u32;2],Arc<CompiledScore>,usize)>() <= ROUNDING_BYTES {
            while self.rounding.len()==8 || self.stats.rounding_payload_bytes+bytes>ROUNDING_BYTES {
                let (_,_,old)=self.rounding.remove(0);
                self.stats.rounding_payload_bytes -= old;
            }
            self.stats.rounding_payload_bytes += bytes;
            self.rounding.push((key,profile,bytes));
        }
    }
}
