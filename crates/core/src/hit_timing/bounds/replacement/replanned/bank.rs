//! Intern complete successor catalogues once. A token is local to this
//! immutable chart/ordered-skill owner and never crosses its lifetime.
use super::*;
const MAX_CATALOGUES: usize = 4096;
const MAX_CATALOGUE_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(in crate::hit_timing::bounds::replacement) struct Bank {
    catalogues: HashMap<Vec<usize>, u32>,
    catalogue_bytes: usize,
    values: HashMap<(u32, usize, usize), f64>,
}
impl Bank {
    pub(super) fn catalogue(&mut self, ids: &[usize]) -> Option<u32> {
        if let Some(&id) = self.catalogues.get(ids) { return Some(id); }
        let bytes = ids.len().checked_mul(std::mem::size_of::<usize>())?;
        if self.catalogues.len() >= MAX_CATALOGUES
            || self.catalogue_bytes.saturating_add(bytes) > MAX_CATALOGUE_BYTES { return None; }
        let id = self.catalogues.len() as u32;
        let key = ids.to_vec();
        self.catalogue_bytes += key.capacity() * std::mem::size_of::<usize>();
        self.catalogues.insert(key, id); Some(id)
    }
    pub(super) fn get(&self, key: &(u32, usize, usize)) -> Option<&f64> { self.values.get(key) }
    pub(super) fn store(&mut self, key: (u32, usize, usize), value: f64) -> bool {
        if self.values.len() >= CACHE_LIMIT { return false; }
        self.values.insert(key, value); true
    }
    #[cfg(test)]
    pub(super) fn len(&self) -> usize { self.values.len() }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_catalogues_have_distinct_owner_local_tokens_and_bounded_payload() {
        let mut bank = Bank::default();
        let a=bank.catalogue(&[0,2,4]).unwrap();
        let b=bank.catalogue(&[0,3,4]).unwrap();
        assert_ne!(a,b); assert_eq!(bank.catalogue(&[0,2,4]),Some(a));
        bank.store((a,1,2),-0.);
        assert!(bank.get(&(b,1,2)).is_none());
        assert_eq!(bank.get(&(a,1,2)).unwrap().to_bits(),(-0_f64).to_bits());
        for n in 0..10000 { bank.catalogue(&[n,n+1,n+2]); }
        assert_eq!(bank.catalogues.len(),MAX_CATALOGUES);
        assert!(bank.catalogue_bytes<=MAX_CATALOGUE_BYTES);
        assert!(bank.catalogue(&[999999]).is_none());
        assert_eq!(bank.catalogue(&[0,2,4]),Some(a));
    }
}
