//! Bounded whole-configuration reuse, in addition to existing sparse profiles.
use bangdream_optimize_core::hit_timing::ReplaySkill;
use std::{collections::HashMap, sync::Arc};
type Key = [[u32; 6]; 6];
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub hits: u64,
    pub builds: u64,
    pub evictions: u64,
    pub peak_entries: usize,
}
pub struct Memo<T> {
    capacity: usize,
    clock: u64,
    entries: HashMap<Key, (Arc<T>, u64)>,
    pub stats: Stats,
}
impl<T> Memo<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            clock: 0,
            entries: HashMap::new(),
            stats: Stats::default(),
        }
    }
    pub fn get<E>(
        &mut self,
        skills: &[ReplaySkill; 6],
        build: impl FnOnce() -> Result<T, E>,
    ) -> Result<Arc<T>, E> {
        let key = skills.each_ref().map(ReplaySkill::cache_key);
        self.clock += 1;
        if let Some((value, stamp)) = self.entries.get_mut(&key) {
            *stamp = self.clock;
            self.stats.hits += 1;
            return Ok(value.clone());
        }
        let value = Arc::new(build()?);
        self.stats.builds += 1;
        if self.capacity != 0 {
            if self.entries.len() == self.capacity {
                let oldest = *self
                    .entries
                    .iter()
                    .min_by_key(|(_, (_, stamp))| *stamp)
                    .unwrap()
                    .0;
                self.entries.remove(&oldest);
                self.stats.evictions += 1;
            }
            self.entries.insert(key, (value.clone(), self.clock));
            self.stats.peak_entries = self.stats.peak_entries.max(self.entries.len());
        }
        Ok(value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bangdream_optimize_core::hit_timing::{JudgementCorrection, ScoreEffect};
    #[test]
    fn memo_is_bounded_exact_and_failed_builds_never_enter() {
        let s = ReplaySkill {
            duration: 7.,
            score: ScoreEffect::Plain { percent: 100. },
            correction: None,
        };
        let a = std::array::from_fn(|_| s.clone());
        let mut b = a.clone();
        b[0].duration = f32::from_bits(7f32.to_bits() + 1);
        let mut c = a.clone();
        c[5].correction = Some(JudgementCorrection::Bad);
        let mut memo = Memo::new(2);
        assert!(memo.get(&a, || Err::<u32, _>(())).is_err());
        assert_eq!(*memo.get(&a, || Ok::<_, ()>(1)).unwrap(), 1);
        memo.get(&b, || Ok::<_, ()>(2)).unwrap();
        assert_eq!(*memo.get::<()>(&a, || panic!("must hit")).unwrap(), 1);
        memo.get(&c, || Ok::<_, ()>(3)).unwrap();
        assert_eq!(*memo.get(&b, || Ok::<_, ()>(4)).unwrap(), 4);
        assert_eq!(
            (
                memo.stats.hits,
                memo.stats.builds,
                memo.stats.evictions,
                memo.stats.peak_entries
            ),
            (1, 4, 2, 2)
        );
    }
}
