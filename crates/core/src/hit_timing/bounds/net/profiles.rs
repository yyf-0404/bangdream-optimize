//! A fixed-input owner retains power-independent NET coefficients and separately
//! bounded immutable preparation. Exact scores, plans and row graphs stay local.
use super::*;
use super::preparation::Preparation;

const CELLS: usize = 4096;
type Key = [u32; 9];

#[derive(Clone, Copy)]
struct Cached {
    key: Key,
    gains: [f64; 6],
    maximum: f32,
}

#[derive(Default, Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingNetCacheStats {
    pub preparation: TimingNetPreparationStats,
    pub queries: u64,
    pub hits: u64,
    pub stores: u64,
    pub evictions: u64,
    pub resident_profiles: usize,
    pub payload_bytes: usize,
}

pub(super) struct Profiles {
    cells: Vec<Option<Cached>>,
    stats: TimingNetCacheStats,
    avalanche: bool,
}
impl Profiles {
    fn new(count: usize) -> Self {
        assert!(count.is_power_of_two());
        let cells = vec![None; count];
        let stats = TimingNetCacheStats {
            payload_bytes: cells.capacity() * std::mem::size_of::<Option<Cached>>(),
            ..Default::default()
        };
        Self { cells, stats,
            avalanche: std::env::var("BANDORI_TIMING_DISABLE_NET_HASH_AVALANCHE").as_deref() != Ok("1") }
    }
    fn index(&self, key: Key) -> usize {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for word in key { hash = (hash ^ u64::from(word)).wrapping_mul(0x100_0000_01b3); }
        if self.avalanche {
            hash = (hash ^ (hash >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
            hash = (hash ^ (hash >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
            hash ^= hash >> 33;
        } else { hash ^= hash >> 32; }
        hash as usize & (self.cells.len() - 1)
    }
    pub(super) fn get(&mut self, key: Key) -> Option<([f64; 6], f32)> {
        self.stats.queries += 1;
        let cell = self.cells[self.index(key)].filter(|c| c.key == key)?;
        self.stats.hits += 1;
        Some((cell.gains, cell.maximum))
    }
    pub(super) fn store(&mut self, key: Key, gains: [f64; 6], maximum: f32) {
        let index = self.index(key);
        if self.cells[index].is_none() { self.stats.resident_profiles += 1; }
        else { self.stats.evictions += 1; }
        self.cells[index] = Some(Cached { key, gains, maximum });
        self.stats.stores += 1;
    }
}

/// The complete replay input is immutable for this owner's lifetime. Domain
/// keys include the strongest correction and both FIFO delay extrema; skill
/// keys retain all native f32 bits. Changing catalogue order is harmless.
pub struct TimingNetCache {
    input: ReplayInput,
    profiles: Profiles,
    preparation: Preparation,
}
impl TimingNetCache {
    pub fn new(input: &ReplayInput) -> Self {
        Self { input: input.clone(), profiles: Profiles::new(CELLS), preparation: Preparation::new() }
    }
    pub fn input(&self) -> &ReplayInput { &self.input }
    pub fn stats(&self) -> TimingNetCacheStats {
        TimingNetCacheStats { preparation: self.preparation.stats.clone(), ..self.profiles.stats.clone() }
    }
    pub fn stateful(&mut self, skills: &[ReplaySkill], cap: ScoringPower)
        -> Result<TimingNetUpper, ReplayError> {
        TimingNetUpper::build_cached(&self.input, skills, cap, true, true,
            std::env::var("BANDORI_TIMING_DISABLE_NET_INTERVAL_INDEX").as_deref() != Ok("1"),
            std::env::var("BANDORI_TIMING_DISABLE_NET_FRAME_RUNS").as_deref() != Ok("1"),
            std::env::var("BANDORI_TIMING_DISABLE_LAZY_NET_ENVELOPES").as_deref() != Ok("1"),
            Some(&mut self.profiles), Some(&mut self.preparation))
    }
    pub fn adjacent(&mut self, skills: &[ReplaySkill], cap: ScoringPower,
        options: super::super::super::independent::WindowOptions)
        -> Result<TimingNetUpper, ReplayError> {
        let start = Instant::now();
        let table = self.stateful(skills, cap)?;
        let frames = self.preparation.frames(&self.input,skills)?;
        table.with_adjacent(&self.input, skills, options, start, Some(frames))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::{JudgementCorrection, ReplayNote};
    fn input(combo: u32) -> ReplayInput {
        let notes: Vec<_> = (0..28).map(|i| ReplayNote { time: 1. + (i / 2) as f64 * 0.1,
            trigger: (i%4==0 && i<24).then_some(i/4), window: NoteWindow::Standard }).collect();
        ReplayInput { power: 300000., play_level: 27, fps: 120,
            hit_frames: notes.iter().map(|n| (n.time*120.).round() as u32).collect(), notes,
            skills: vec![ReplaySkill { duration: 0.1, correction: None,
                score: ScoreEffect::Plain { percent: 100. } }; 6],
            initial_combo: combo, medley_combo: true, fever_frames: Some([125,170]),
            constraints: vec![], hit_order: vec![], native_time_routing: None }
    }
    #[test]
    fn cached_profiles_keep_every_coefficient_bit_across_domains_caps_order_and_eviction() {
        for combo in [0,699] { for count in [1,CELLS] {
            let input = input(combo);
            let mut cache = TimingNetCache { input: input.clone(), profiles: Profiles::new(count),
                preparation: Preparation::new() };
            cache.preparation.enabled = true;
            let effects = [ScoreEffect::Plain { percent: 100. },
                ScoreEffect::PerfectOnly { percent: 140. },
                ScoreEffect::PerfectOrHalf { percent: 120. },
                ScoreEffect::PerfectOrZero { percent: 120. },
                ScoreEffect::Continuous { high: 130., low: 60. },
                ScoreEffect::Crescendo { percent: 100., step: 0.5, cap: 100 },
                ScoreEffect::Crescendo { percent: 100., step: 0.5, cap: 150 }];
            let catalogue: Vec<_> = effects.iter().enumerate().map(|(i, score)| ReplaySkill {
                duration: [0.1,0.75,2.][i%3], score: score.clone(),
                correction: [None,Some(JudgementCorrection::Great),Some(JudgementCorrection::Bad)][i%3],
            }).collect();
            for variant in 0..7 { for cap in [400000.,700000.] {
                let skills: Vec<_> = catalogue.iter().enumerate()
                    .filter(|&(i,_)| variant==0 || variant==6 || (i+variant)%3!=0)
                    .map(|(_,s)| s.clone()).collect();
                let skills: Vec<_> = if variant==6 { skills.into_iter().rev().collect() } else { skills };
                let cap=ScoringPower::new(cap).unwrap();
                let fresh=TimingNetUpper::build_cached(&input,&skills,cap,true,true,true,true,false,None,None).unwrap();
                let cached=cache.stateful(&skills,cap).unwrap();
                assert_eq!(fresh.baseline.to_bits(),cached.baseline.to_bits());
                assert_eq!(fresh.error.to_bits(),cached.error.to_bits());
                assert_eq!(fresh.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>(),
                    cached.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>());
            } }
            if count==CELLS { assert!(cache.stats().hits > 0); }
            else { assert!(cache.stats().evictions > 0); }
            assert!(cache.stats().payload_bytes <= 512*1024);
            assert!(cache.stats().resident_profiles <= count);
            assert!(cache.stats().preparation.frame_payload_bytes <= 4*1024*1024);
            assert!(cache.stats().preparation.rounding_payload_bytes <= 64*1024);
            assert!(cache.stats().preparation.frame_hits > 0);
            assert!(cache.stats().preparation.rounding_hits > 0);
        } }
    }
    #[test]
    fn preparation_eviction_and_disabled_retention_preserve_all_upper_bits() {
        let input = input(699);
        let mut cache = TimingNetCache::new(&input);
        cache.preparation.enabled = true;
        let base = cache.preparation.stats.frame_payload_bytes;
        let controls = [None, Some(JudgementCorrection::Great),
            Some(JudgementCorrection::Good), Some(JudgementCorrection::Bad), None];
        let mut retained = Vec::new();
        for correction in controls {
            let skills = vec![ReplaySkill { duration: 0.1, correction,
                score: ScoreEffect::Plain { percent: 100. } }];
            retained.push(FrameBounds::new(&input, &skills).unwrap().geometry_bytes());
        }
        // A single-entry sized budget forces eviction while still admitting all domains.
        cache.preparation.frame_limit = base + *retained.iter().max().unwrap();
        for (round, correction) in controls.into_iter().enumerate() {
            let skills = vec![ReplaySkill { duration: 0.1, correction,
                score: ScoreEffect::Plain { percent: 100. } }];
            let cap = ScoringPower::new(400000. + round as f32 * 17.).unwrap();
            let fresh = TimingNetUpper::new_stateful(&input, &skills, cap).unwrap();
            let cached = cache.stateful(&skills, cap).unwrap();
            assert_eq!(fresh.baseline.to_bits(), cached.baseline.to_bits());
            assert_eq!(fresh.error.to_bits(), cached.error.to_bits());
            assert_eq!(fresh.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>(),
                cached.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>());
            assert!(cache.stats().preparation.frame_payload_bytes <= cache.preparation.frame_limit);
        }
        assert!(cache.stats().preparation.frame_evictions > 0);
        let mut empty = TimingNetCache::new(&input);
        empty.preparation.enabled = true;
        empty.preparation.frame_limit = base;
        let skills = vec![input.skills[0].clone()];
        let cap = ScoringPower::new(700000.).unwrap();
        let fresh = TimingNetUpper::new_stateful(&input, &skills, cap).unwrap();
        for _ in 0..2 {
            let current = empty.stateful(&skills, cap).unwrap();
            assert_eq!(fresh.baseline.to_bits(), current.baseline.to_bits());
            assert_eq!(fresh.error.to_bits(), current.error.to_bits());
            assert_eq!(fresh.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>(),
                current.gains.iter().map(|g| g.map(f64::to_bits)).collect::<Vec<_>>());
        }
        assert_eq!(empty.stats().preparation.frame_hits, 0);
        assert_eq!(empty.stats().preparation.frame_payload_bytes, base);
    }
}
