//! Stable node indices for hot continuation reads. Index admission is separate
//! from row admission: reaching either limit must keep the old cache policy.
use super::{Arc, HashMap, IndexedEdge, Key, CACHE_LIMIT};

pub(super) const UNINDEXED: u32 = u32::MAX;

#[derive(Default)]
struct Node {
    edges: Option<Arc<[IndexedEdge]>>,
    ordinary: Option<f64>,
    captain_epoch: u64,
    captain: f64,
}

pub(super) struct Captain {
    epoch: u64,
    overflow: HashMap<Key, f64>,
}

pub(super) struct Cache {
    indices: HashMap<Key, u32>,
    nodes: Vec<Node>,
    overflow_edges: HashMap<Key, Arc<[IndexedEdge]>>,
    overflow_ordinary: HashMap<Key, f64>,
    edge_count: usize,
    ordinary_count: usize,
    epoch: u64,
    index_limit: usize,
    row_limit: usize,
}

impl Default for Cache {
    fn default() -> Self {
        Self::with_limits(CACHE_LIMIT, CACHE_LIMIT)
    }
}

impl Cache {
    fn with_limits(index_limit: usize, row_limit: usize) -> Self {
        assert!(index_limit < UNINDEXED as usize);
        Self {
            indices: HashMap::new(),
            nodes: Vec::new(),
            overflow_edges: HashMap::new(),
            overflow_ordinary: HashMap::new(),
            edge_count: 0,
            ordinary_count: 0,
            epoch: 0,
            index_limit,
            row_limit,
        }
    }

    pub(super) fn index(&mut self, key: Key) -> u32 {
        if key.1 == 6 {
            return UNINDEXED;
        }
        if let Some(&index) = self.indices.get(&key) {
            return index;
        }
        if self.nodes.len() == self.index_limit {
            return UNINDEXED;
        }
        let index = self.nodes.len() as u32;
        self.nodes.push(Node::default());
        self.indices.insert(key, index);
        index
    }

    pub(super) fn edges(&self, key: Key, index: u32) -> Option<Arc<[IndexedEdge]>> {
        if index == UNINDEXED {
            self.overflow_edges.get(&key).cloned()
        } else {
            self.nodes[index as usize].edges.clone()
        }
    }

    pub(super) fn store_edges(&mut self, key: Key, index: u32, edges: Arc<[IndexedEdge]>) {
        if self.edge_count < self.row_limit {
            if index == UNINDEXED {
                self.overflow_edges.insert(key, edges);
            } else {
                self.nodes[index as usize].edges = Some(edges);
            }
            self.edge_count += 1;
        }
    }

    pub(super) fn ordinary(&self, key: Key, index: u32) -> Option<f64> {
        if index == UNINDEXED {
            self.overflow_ordinary.get(&key).copied()
        } else {
            self.nodes[index as usize].ordinary
        }
    }

    pub(super) fn store_ordinary(&mut self, key: Key, index: u32, value: f64) {
        if self.ordinary_count < self.row_limit {
            if index == UNINDEXED {
                self.overflow_ordinary.insert(key, value);
            } else {
                self.nodes[index as usize].ordinary = Some(value);
            }
            self.ordinary_count += 1;
        }
    }

    pub(super) fn begin_captain(&mut self) -> Captain {
        if self.epoch == u64::MAX {
            for node in &mut self.nodes {
                node.captain_epoch = 0;
            }
            self.epoch = 0;
        }
        self.epoch += 1;
        Captain {
            epoch: self.epoch,
            overflow: HashMap::new(),
        }
    }

    pub(super) fn captain(&self, key: Key, index: u32, captain: &Captain) -> Option<f64> {
        if index == UNINDEXED {
            captain.overflow.get(&key).copied()
        } else {
            let node = &self.nodes[index as usize];
            (node.captain_epoch == captain.epoch).then_some(node.captain)
        }
    }

    pub(super) fn store_captain(
        &mut self,
        key: Key,
        index: u32,
        captain: &mut Captain,
        value: f64,
    ) {
        if index == UNINDEXED {
            captain.overflow.insert(key, value);
        } else {
            let node = &mut self.nodes[index as usize];
            node.captain_epoch = captain.epoch;
            node.captain = value;
        }
    }

    #[cfg(test)]
    pub(super) fn unindexed(row_limit: usize) -> Self {
        Self::with_limits(0, row_limit)
    }

    #[cfg(test)]
    pub(super) fn small(index_limit: usize, row_limit: usize) -> Self {
        Self::with_limits(index_limit, row_limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::bounds::replacement::replanned::State;

    #[test]
    fn captain_epochs_isolate_pairs_and_witnesses_even_at_wraparound() {
        let mut cache = Cache::default();
        let key = (1, 3, State { old: 500, new: 530 });
        let index = cache.index(key);
        let mut first = cache.begin_captain();
        cache.store_captain(key, index, &mut first, -1.);
        assert_eq!(cache.captain(key, index, &first), Some(-1.));
        let mut second = cache.begin_captain();
        assert_eq!(cache.captain(key, index, &second), None);
        cache.store_captain(key, index, &mut second, 2.);
        assert_eq!(cache.captain(key, index, &second), Some(2.));
        cache.epoch = u64::MAX;
        let next = cache.begin_captain();
        assert_eq!(next.epoch, first.epoch);
        assert_eq!(cache.captain(key, index, &next), None);
    }
}
