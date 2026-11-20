//! Request-scoped, bounded reuse across equipment. A model is bound to the
//! complete immutable chart/Combo/routing/options and ordered skill catalogue.
//! Only native rounding envelopes depend on the new equipment's power cap.
use bangdream_optimize_core::{
    hit_timing::{
        bounds::{PoolTimingUpper, TimingReplacementBounds, TimingReplacementCache, TimingNetCache},
        independent::WindowOptions,
        ReplayError, ReplayInput, ReplaySkill,
    },
    scoring::ScoringPower,
};
use serde_json::{json, Value};
use std::collections::VecDeque;

struct Entry {
    keys: Vec<[u32; 6]>,
    model: PoolTimingUpper,
}

pub(super) struct Models {
    coefficients: [TimingNetCache; 3],
    options: WindowOptions,
    capacity: usize,
    same_hit_frames: bool,
    entries: [VecDeque<Entry>; 3],
    replacements: [VecDeque<(Vec<[u32; 6]>, TimingReplacementCache)>; 3],
    hits: [usize; 3],
    builds: [usize; 3],
    evictions: [usize; 3],
}
impl Models {
    pub fn new(inputs: &[ReplayInput; 3], options: WindowOptions, capacity: usize) -> Self {
        Self {
            coefficients: std::array::from_fn(|s| TimingNetCache::new(&inputs[s])),
            options,
            capacity,
            same_hit_frames: false,
            entries: std::array::from_fn(|_| VecDeque::new()),
            replacements: std::array::from_fn(|_| VecDeque::new()),
            hits: [0; 3],
            builds: [0; 3],
            evictions: [0; 3],
        }
    }
    pub fn with_same_hit_frames(mut self, required: bool) -> Self {
        self.same_hit_frames = required;
        self
    }
    pub fn adjacent(
        &mut self,
        song: usize,
        skills: &[ReplaySkill],
        cap: ScoringPower,
    ) -> Result<PoolTimingUpper, ReplayError> {
        let keys: Vec<_> = skills.iter().map(ReplaySkill::cache_key).collect();
        let entries = self
            .entries
            .get_mut(song)
            .ok_or(ReplayError::Input("unknown cache song"))?;
        if let Some(i) = entries.iter().position(|e| e.keys == keys) {
            let entry = entries.remove(i).unwrap();
            let model = entry.model.with_power_cap(cap)?;
            entries.push_back(entry);
            self.hits[song] += 1;
            return Ok(model);
        }
        let model = if std::env::var("BANDORI_TIMING_DISABLE_NET_PROFILE_CACHE").as_deref() == Ok("1") {
            PoolTimingUpper::new_adjacent_net(self.coefficients[song].input(), skills, cap, self.options)?
        } else {
            PoolTimingUpper::new_cached_adjacent_net(&mut self.coefficients[song], skills, cap, self.options)?
        };
        self.builds[song] += 1;
        if self.capacity > 0 {
            if entries.len() == self.capacity {
                entries.pop_front();
                self.evictions[song] += 1;
            }
            entries.push_back(Entry {
                keys,
                model: model.clone(),
            });
        }
        Ok(model)
    }
    pub fn report(&self) -> Value {
        json!({"capacityPerSong":self.capacity,"hits":self.hits,"builds":self.builds,
            "coefficientCaches":self.coefficients.iter().map(TimingNetCache::stats).collect::<Vec<_>>(),
            "replacementFrameCaches":self.replacements.iter().enumerate().flat_map(|(s, rows)|
                rows.iter().map(move |(_,c)| json!({"song":s,"cache":c.frame_cache_stats()}))).collect::<Vec<_>>(),
            "evictions":self.evictions,
            "counterScope":"cumulative per cached model; do not sum repeated per-item snapshots",
            "models":self.entries.iter().enumerate().flat_map(|(s, rows)| rows.iter().map(move |e|
                json!({"song":s,"skills":e.keys.len(),"adjacent":e.model.adjacent_stats()})
            )).collect::<Vec<_>>()})
    }
    pub fn replacement(
        &mut self,
        song: usize,
        skills: &[ReplaySkill],
        cap: ScoringPower,
        tighten: bool,
    ) -> Result<TimingReplacementBounds, ReplayError> {
        let model = self.replacement_model(song, skills, cap, tighten)?;
        Ok(if self.same_hit_frames { model.require_same_hit_frames() } else { model })
    }
    fn replacement_model(
        &mut self, song: usize, skills: &[ReplaySkill], cap: ScoringPower, tighten: bool,
    ) -> Result<TimingReplacementBounds, ReplayError> {
        if self.capacity == 0 {
            let mut model = TimingReplacementBounds::new_adjacent(self.coefficients[song].input(), skills, cap)?;
            if tighten { model.tighten_frames(self.coefficients[song].input(), skills)?; }
            return Ok(model);
        }
        let keys: Vec<_> = skills.iter().map(ReplaySkill::cache_key).collect();
        let rows = &mut self.replacements[song];
        if let Some(i) = rows.iter().position(|(k, _)| *k == keys) {
            let entry = rows.remove(i).unwrap();
            let model = if tighten { entry.1.prepare_tightened(cap)? } else { entry.1.prepare(cap)? };
            rows.push_back(entry);
            return Ok(model);
        }
        let entry = TimingReplacementCache::new(self.coefficients[song].input(), skills);
        let model = if tighten { entry.prepare_tightened(cap)? } else { entry.prepare(cap)? };
        if rows.len() == self.capacity {
            rows.pop_front();
        }
        rows.push_back((keys, entry));
        Ok(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bangdream_optimize_core::hit_timing::{NoteWindow, ReplayNote, ScoreEffect};
    #[test]
    fn isolates_song_combo_catalogue_order_and_bounded_eviction() {
        let skill = |percent| ReplaySkill {
            duration: 0.15,
            score: ScoreEffect::Plain { percent },
            correction: None,
        };
        let notes: Vec<_> = (0..6)
            .map(|s| ReplayNote {
                time: 1. + s as f64,
                trigger: Some(s),
                window: NoteWindow::PinnedPerfect {
                    frame: (120 + 120 * s) as u32,
                },
            })
            .collect();
        let input = ReplayInput {
            power: 300000.,
            play_level: 27,
            fps: 120,
            hit_frames: (1..=6).map(|s| s * 120).collect(),
            notes,
            skills: vec![skill(100.); 6],
            initial_combo: 0,
            medley_combo: true,
            fever_frames: None,
            constraints: vec![],
            hit_order: vec![],
            native_time_routing: None,
        };
        let inputs = std::array::from_fn(|s| {
            let mut i = input.clone();
            i.initial_combo = s as u32 * 100;
            i
        });
        let mut cache = Models::new(&inputs, Default::default(), 1);
        let skills = [skill(100.), skill(120.)];
        let cap = ScoringPower::new(600000.).unwrap();
        for s in 0..3 {
            let model = cache.adjacent(s, &skills, cap).unwrap();
            let fresh =
                PoolTimingUpper::new_adjacent_net(&inputs[s], &skills, cap, Default::default())
                    .unwrap();
            let choices = std::array::from_fn(|i| model.choice(i % 2, 60000.).unwrap());
            assert_eq!(
                model.upper(&choices).unwrap(),
                fresh.upper(&choices).unwrap()
            );
        }
        cache
            .adjacent(0, &skills, ScoringPower::new(700000.).unwrap())
            .unwrap();
        assert_eq!(cache.hits, [1, 0, 0]);
        cache
            .adjacent(0, &[skills[1].clone(), skills[0].clone()], cap)
            .unwrap();
        cache.adjacent(0, &skills, cap).unwrap();
        assert_eq!(cache.builds, [3, 1, 1]);
        assert_eq!(cache.evictions, [2, 0, 0]);
        let mut uncached = Models::new(&inputs, Default::default(), 0);
        uncached.adjacent(0, &skills, cap).unwrap();
        uncached.adjacent(0, &skills, cap).unwrap();
        assert_eq!(uncached.builds, [2, 0, 0]);
        assert_eq!(uncached.hits, [0; 3]);
    }
}
