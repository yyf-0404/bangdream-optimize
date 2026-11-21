//! One retention budget across fixed charts. Eviction never removes a search
//! path: returned banks keep their models, and a cache miss rebuilds exactly.
use super::*;
use super::super::ReplaySkill;

const DEFAULT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingGeometryPoolStats {
    pub budget_bytes: Option<usize>,
    pub retained_geometry_bytes: usize,
    /// Measured after eviction; construction and caller-owned banks are separate.
    pub peak_retained_geometry_bytes: usize,
    /// Cache-owned payload just before applying the shared budget.
    pub peak_pending_geometry_bytes: usize,
    pub preparations: u64,
    pub evicted_bindings: u64,
    pub evicted_owners: u64,
    /// Removed from cache ownership; returned banks may keep the payload alive.
    pub released_geometry_bytes: u64,
}

pub struct TimingCompileCachePool {
    caches: Vec<TimingCompileCache>,
    clock: u64,
    max_geometry_bytes: usize,
    stats: TimingGeometryPoolStats,
}
impl TimingCompileCachePool {
    pub fn new(inputs: &[ReplayInput], options: SearchOptions) -> Self {
        let disabled = std::env::var("BANDORI_TIMING_DISABLE_SHARED_GEOMETRY_BUDGET")
            .as_deref() == Ok("1")
            || std::env::var("BANDORI_TIMING_DISABLE_JOINT_LOCAL_GEOMETRY").as_deref() == Ok("1");
        let bytes = if disabled { usize::MAX } else {
            std::env::var("BANDORI_TIMING_SHARED_GEOMETRY_BYTES").ok()
                .and_then(|value| value.parse().ok()).unwrap_or(DEFAULT_BYTES)
        };
        Self::with_geometry_budget(inputs, options, bytes)
    }

    pub fn with_geometry_budget(inputs: &[ReplayInput], options: SearchOptions, bytes: usize) -> Self {
        Self {
            caches: inputs.iter().map(|input| TimingCompileCache::new(input, options)).collect(),
            clock: 0,
            max_geometry_bytes: bytes,
            stats: TimingGeometryPoolStats {
                budget_bytes: (bytes != usize::MAX).then_some(bytes),
                ..TimingGeometryPoolStats::default()
            },
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, TimingCompileCache> { self.caches.iter() }

    pub fn with_frame_ranges(mut self, limits: &[Vec<Option<[u32; 2]>>]) -> Self {
        assert_eq!(self.caches.len(), limits.len());
        self.caches = self.caches.into_iter().zip(limits)
            .map(|(cache, ranges)| cache.with_frame_ranges(ranges.clone())).collect();
        self
    }
    pub fn stats(&self) -> TimingGeometryPoolStats { self.stats.clone() }

    pub fn prepare(&mut self, song: usize, skills: &[ReplaySkill])
        -> Result<CompiledTimingCandidates, SearchError> {
        self.prepare_impl(song, skills, false)
    }

    pub fn prepare_for_score(&mut self, song: usize, skills: &[ReplaySkill])
        -> Result<CompiledTimingCandidates, SearchError> {
        self.prepare_impl(song, skills, true)
    }

    fn prepare_impl(&mut self, song: usize, skills: &[ReplaySkill], score_only: bool)
        -> Result<CompiledTimingCandidates, SearchError> {
        self.clock = self.clock.checked_add(1).expect("cache access clock overflow");
        // The chart's existing entry/owner timestamps now use one common clock.
        // Its independent byte and 64-binding rules continue to apply first.
        self.caches[song].exact_clock = self.clock - 1;
        let bank = self.caches[song].prepare_impl(skills, score_only)?;
        self.stats.preparations += 1;
        self.enforce();
        Ok(bank)
    }

    fn retained_bytes(&self) -> usize {
        self.caches.iter().map(TimingCompileCache::exact_geometry_bytes).sum()
    }

    fn enforce(&mut self) {
        let mut bytes = self.retained_bytes();
        self.stats.peak_pending_geometry_bytes = self.stats.peak_pending_geometry_bytes.max(bytes);
        while bytes > self.max_geometry_bytes {
            // Owners and resolved bindings share the same age ordering. On a
            // timestamp tie, release an optional owner before its binding.
            let oldest = self.caches.iter().enumerate().flat_map(|(song, cache)| {
                cache.geometry_owners.values().map(move |(_, stamp)| (*stamp, song, false))
                    .chain(cache.exact_stamps.values().map(move |stamp| (*stamp, song, true)))
            }).min();
            let Some((_, song, binding)) = oldest else { break; };
            let cache = &mut self.caches[song];
            if binding {
                let key = cache.exact_stamps.iter().min_by_key(|(_, stamp)| *stamp).unwrap().0.clone();
                cache.exact.remove(&key);
                cache.exact_stamps.remove(&key);
                self.stats.evicted_bindings += 1;
            } else {
                cache.evict_owner();
                self.stats.evicted_owners += 1;
            }
            let next = self.retained_bytes();
            self.stats.released_geometry_bytes += (bytes - next) as u64;
            bytes = next;
        }
        self.stats.retained_geometry_bytes = bytes;
        self.stats.peak_retained_geometry_bytes = self.stats.peak_retained_geometry_bytes.max(bytes);
    }
}

#[cfg(test)]
mod tests;
