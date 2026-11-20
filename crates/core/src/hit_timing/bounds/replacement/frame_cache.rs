//! Bound to one immutable chart and complete ordered catalogue. No powers,
//! incumbent thresholds, scores or Boolean replacement decisions are stored.
const SETS: usize = 4096;
const PROFILES: usize = 256;
const ROWS: usize = 32768;
const EMPTY: u32 = u32::MAX;
pub(super) const MAX_BYTES: usize = 576 * 1024;
pub(super) const MAX_ROW_BYTES: usize = 768 * 1024;
type ProfileRows = [[f64; 2]; 10];
#[derive(Clone, Copy)]
struct Pair { key: [u32; 3], value: [f64; 6] }
#[derive(Clone, Copy)]
struct Row { key: [u32; 4], value: f64 }
#[derive(Clone, Default, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub queries: u64, pub hits: u64, pub stores: u64, pub evictions: u64, pub payload_bytes: usize,
    pub row_queries: u64, pub row_hits: u64, pub row_stores: u64, pub row_evictions: u64,
    pub row_payload_bytes: usize,
    pub row_cell_builds: u64, pub row_cell_reuses: u64, pub row_cell_payload_bytes: usize,
}
pub(super) struct Cache {
    profiles: Vec<Option<ProfileRows>>, pairs: Vec<Pair>, victims: Vec<u8>, rows: Vec<Row>, count: usize,
    pub(super) enabled: bool, pub(super) rows_enabled: bool, pub(super) stats: Stats,
    cells: Option<std::sync::Arc<super::replanned::cells::Index>>,
}
impl Cache {
    pub(super) fn new(count: usize) -> Self {
        Self { profiles: vec![], pairs: vec![], victims: vec![], rows: vec![], count, cells: None,
            enabled: std::env::var("BANDORI_TIMING_DISABLE_FRAME_COEFFICIENT_CACHE").as_deref() != Ok("1"),
            rows_enabled: std::env::var("BANDORI_TIMING_DISABLE_REPLACEMENT_ROW_CACHE").as_deref() != Ok("1"),
            stats: Stats { payload_bytes: std::mem::size_of::<Self>(), ..Default::default() } }
    }
    pub(super) fn cells(&mut self, build: impl FnOnce() -> super::replanned::cells::Index)
        -> std::sync::Arc<super::replanned::cells::Index> {
        if let Some(index) = &self.cells {
            self.stats.row_cell_reuses += 1; return index.clone();
        }
        let index = std::sync::Arc::new(build());
        self.stats.row_cell_builds += 1;
        self.stats.row_cell_payload_bytes = index.bytes();
        self.cells = Some(index.clone()); index
    }
    fn account(&mut self) {
        self.stats.payload_bytes = std::mem::size_of::<Self>()
            + self.profiles.capacity() * std::mem::size_of::<Option<ProfileRows>>()
            + self.pairs.capacity() * std::mem::size_of::<Pair>()
            + self.victims.capacity() * std::mem::size_of::<u8>();
        self.stats.row_payload_bytes = self.rows.capacity() * std::mem::size_of::<Row>();
        debug_assert!(self.stats.payload_bytes <= MAX_BYTES);
        debug_assert!(self.stats.row_payload_bytes <= MAX_ROW_BYTES);
    }
    pub(super) fn profile(&mut self, id: usize) -> Option<ProfileRows> {
        self.stats.queries += 1;
        let result = self.enabled.then(|| self.profiles.get(id).copied().flatten()).flatten();
        if result.is_some() { self.stats.hits += 1; }
        result
    }
    pub(super) fn store_profile(&mut self, id: usize, rows: ProfileRows) {
        if !self.enabled || id >= self.count.min(PROFILES) { return; }
        if self.profiles.is_empty() { self.profiles = vec![None; self.count.min(PROFILES)]; self.account(); }
        self.profiles[id] = Some(rows); self.stats.stores += 1;
    }
    fn hash(words: impl IntoIterator<Item=u32>) -> usize {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for word in words { hash = (hash ^ u64::from(word)).wrapping_mul(0x100_0000_01b3); }
        hash = (hash ^ (hash >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        hash = (hash ^ (hash >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        (hash ^ (hash >> 33)) as usize
    }
    fn pair_key(key: [usize; 3]) -> Option<[u32; 3]> {
        let key = [u32::try_from(key[0]).ok()?, u32::try_from(key[1]).ok()?, u32::try_from(key[2]).ok()?];
        (key[0] != EMPTY).then_some(key)
    }
    pub(super) fn pair(&mut self, key: [usize; 3]) -> Option<[f64; 6]> {
        self.stats.queries += 1;
        if !self.enabled || self.pairs.is_empty() { return None; }
        let key = Self::pair_key(key)?;
        let set = Self::hash(key) & (SETS - 1);
        for offset in 0..2 {
            let row = self.pairs[2*set+offset];
            if row.key == key {
                self.victims[set] = (1-offset) as u8;
                self.stats.hits += 1; return Some(row.value);
            }
        }
        None
    }
    pub(super) fn store_pair(&mut self, key: [usize; 3], value: [f64; 6]) {
        if !self.enabled { return; }
        let Some(key) = Self::pair_key(key) else { return; };
        if self.pairs.is_empty() {
            self.pairs = vec![Pair { key: [EMPTY;3], value: [0.;6] }; 2*SETS];
            self.victims = vec![0; SETS]; self.account();
        }
        let set = Self::hash(key) & (SETS - 1);
        let offset = (0..2).find(|&i| self.pairs[2*set+i].key == key)
            .or_else(|| (0..2).find(|&i| self.pairs[2*set+i].key[0] == EMPTY))
            .unwrap_or(usize::from(self.victims[set]));
        let index = 2*set+offset;
        self.stats.evictions += u64::from(self.pairs[index].key[0] != EMPTY && self.pairs[index].key != key);
        self.pairs[index] = Pair { key, value };
        self.victims[set] = (1-offset) as u8; self.stats.stores += 1;
    }
    pub(super) fn row(&mut self, key: [u32; 4]) -> Option<f64> {
        self.stats.row_queries += 1;
        if !self.rows_enabled || key[0] == EMPTY || self.rows.is_empty() { return None; }
        let row = self.rows[Self::hash(key) & (ROWS - 1)];
        if row.key == key { self.stats.row_hits += 1; Some(row.value) } else { None }
    }
    pub(super) fn store_row(&mut self, key: [u32; 4], value: f64) {
        if !self.rows_enabled || key[0] == EMPTY { return; }
        if self.rows.is_empty() { self.rows = vec![Row { key: [EMPTY;4], value: 0. }; ROWS]; self.account(); }
        let index = Self::hash(key) & (ROWS - 1);
        self.stats.row_evictions += u64::from(self.rows[index].key[0] != EMPTY && self.rows[index].key != key);
        self.rows[index] = Row { key, value }; self.stats.row_stores += 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collisions_complete_keys_and_profile_limits_keep_the_bank_bounded() {
        let mut cache = Cache::new(5000); cache.enabled = true;
        for id in 0..20000 {
            cache.store_profile(id, [[id as f64;2];10]);
            let key = [id % 2,id,id+1]; cache.store_pair(key,[id as f64;6]);
            assert_eq!(cache.pair(key),Some([id as f64;6]));
            assert!(cache.stats.payload_bytes <= MAX_BYTES);
        }
        assert!(cache.stats.evictions > 0); assert!(cache.profile(PROFILES).is_none());
        let key=[0,1,2]; let set=Cache::hash(Cache::pair_key(key).unwrap()) & (SETS-1);
        let others: Vec<_>=(3..200000).map(|i| [0,i,i+1])
            .filter(|&k| Cache::hash(Cache::pair_key(k).unwrap()) & (SETS-1) == set).take(2).collect();
        cache.store_pair(key,[1.;6]);
        for &other in &others { cache.store_pair(other,[2.;6]); }
        assert!(cache.pair(key).is_none());
        for other in others { assert_eq!(cache.pair(other),Some([2.;6])); }
    }
    #[test]
    fn primitive_rows_preserve_bits_and_never_match_a_collision() {
        let mut cache=Cache::new(10); cache.rows_enabled=true;
        let key=[0,1,2,99]; let slot=Cache::hash(key) & (ROWS-1);
        cache.store_row(key,-0.);
        assert_eq!(cache.row(key).unwrap().to_bits(),(-0_f64).to_bits());
        let other=(100..1000000).map(|t| [1,1,2,t])
            .find(|&k| Cache::hash(k) & (ROWS-1) == slot).unwrap();
        cache.store_row(other,0.137);
        assert!(cache.row(key).is_none());
        assert_eq!(cache.row(other).unwrap().to_bits(),0.137_f64.to_bits());
        assert_eq!(cache.stats.row_payload_bytes,MAX_ROW_BYTES);
        cache.rows_enabled=false;
        assert!(cache.row(other).is_none());
    }
}
