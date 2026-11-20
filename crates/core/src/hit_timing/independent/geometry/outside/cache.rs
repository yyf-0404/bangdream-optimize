//! Chart-scoped proofs. Entry-dependent conditions are checked after every hit.
use super::*;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

#[derive(Hash, PartialEq, Eq)]
struct OutsideKey {
    notes: Vec<(usize, Vec<ActionInterval>)>,
    relations: Vec<Relation>,
}
impl OutsideKey {
    fn new(row: &Row, notes: &[usize]) -> Self {
        let mut incident: Vec<_> = notes.iter().flat_map(|&i| &row.incident[i]).copied().collect();
        incident.sort_unstable();
        incident.dedup();
        Self {
            notes: notes.iter().map(|&i| (i, row.domains[i].clone())).collect(),
            relations: incident.into_iter().map(|i| row.relations[i].clone()).collect(),
        }
    }
    fn bytes(&self) -> usize {
        self.notes.capacity() * std::mem::size_of::<(usize, Vec<ActionInterval>)>()
            + self.notes.iter().map(|(_, domains)| domains.capacity()
                * std::mem::size_of::<ActionInterval>()).sum::<usize>()
            + self.relations.capacity() * std::mem::size_of::<Relation>()
    }
}

struct Entry {
    template: Option<OutsideTemplate>,
    bytes: usize,
    stamp: u64,
}

#[derive(Clone, Default)]
pub(in crate::hit_timing::independent) struct OutsideCacheStats {
    pub queries: u64,
    pub hits: u64,
    pub negative_hits: u64,
    pub proofs: u64,
    pub boundary_rejects: u64,
    pub stores: u64,
    pub evictions: u64,
    pub skipped: u64,
    pub lookup_seconds: f64,
    pub proof_seconds: f64,
}

#[derive(Default)]
pub(in crate::hit_timing::independent) struct OutsideCache {
    entries: HashMap<OutsideKey, Entry>,
    bytes: usize,
    clock: u64,
    pub stats: OutsideCacheStats,
}
impl OutsideCache {
    pub(in crate::hit_timing::independent) fn bytes(&self) -> usize { self.bytes }
    pub(in crate::hit_timing::independent) fn entries(&self) -> usize { self.entries.len() }
    pub(in crate::hit_timing::independent) fn limit(&self) -> usize { MAX_BYTES }

    pub(in crate::hit_timing::independent::geometry) fn run(
        &mut self,
        prepared: &Prepared,
        row: &Row,
        notes: &[usize],
        settled: &[bool],
        keys: &[Key],
    ) -> Option<Action> {
        use crate::timing::Instant;
        self.stats.queries += 1;
        self.clock += 1;
        let start = Instant::now();
        let key = OutsideKey::new(row, notes);
        let cached = self.entries.get_mut(&key).map(|entry| {
            entry.stamp = self.clock;
            entry.template.clone()
        });
        self.stats.lookup_seconds += start.elapsed().as_secs_f64();
        let template = if let Some(template) = cached {
            self.stats.hits += 1;
            self.stats.negative_hits += u64::from(template.is_none());
            template
        } else {
            let start = Instant::now();
            let template = prepared.outside_template(row, notes);
            self.stats.proof_seconds += start.elapsed().as_secs_f64();
            self.stats.proofs += 1;
            let bytes = std::mem::size_of::<OutsideKey>() + std::mem::size_of::<Entry>()
                + key.bytes() + template.as_ref().map_or(0, OutsideTemplate::bytes);
            if bytes > MAX_BYTES {
                self.stats.skipped += 1;
            } else {
                while self.bytes + bytes > MAX_BYTES || self.entries.len() >= MAX_ENTRIES {
                    let old = self.entries.iter().min_by_key(|(_, entry)| entry.stamp)
                        .unwrap().0;
                    // Unique stamps allow eviction without copying the key.
                    let old_stamp = self.entries[old].stamp;
                    self.entries.retain(|_, entry| {
                        if entry.stamp == old_stamp {
                            self.bytes -= entry.bytes;
                            false
                        } else { true }
                    });
                    self.stats.evictions += 1;
                }
                self.bytes += bytes;
                self.entries.insert(key, Entry { template: template.clone(), bytes, stamp: self.clock });
                self.stats.stores += 1;
            }
            template
        }?;
        if !template.accepts(settled, keys) {
            self.stats.boundary_rejects += 1;
            return None;
        }
        Some(template.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::{NativeTimeRouting, NativeTimeTarget, NoteWindow, ReplayNote};

    fn input(times: &[f64]) -> ReplayInput {
        let frames: Vec<_> = times.iter().map(|t| (t * 120.).round() as u32).collect();
        ReplayInput {
            power: 300040.75, play_level: 27, fps: 120, initial_combo: 49,
            medley_combo: true, fever_frames: None, constraints: vec![],
            skills: vec![], hit_order: vec![],
            notes: times.iter().map(|&time| ReplayNote { time, trigger: None,
                window: NoteWindow::Standard }).collect(),
            native_time_routing: Some(NativeTimeRouting {
                distance_mode: Default::default(),
                music_positions: (0..=300).map(|f| f as f32 * 10.).collect(),
                targets: frames.iter().enumerate().map(|(i, &f)| Some(NativeTimeTarget {
                    button: i as u8, absolute_pos: f as i32 * 10,
                    active_from_frame: 0, active_until_frame: 300,
                })).collect(),
                candidate_order: (0..times.len()).collect(), connections: vec![], event_order: vec![],
            }),
            hit_frames: frames,
        }
    }

    #[test]
    fn reused_proof_rechecks_external_consumption_without_caching_a_boundary_failure() {
        let mut input = input(&[0.1, 0.1]);
        input.native_time_routing.as_mut().unwrap().targets[1].as_mut().unwrap().button = 0;
        let prepared = Prepared::new(&input).unwrap();
        let row = prepared.joint_row();
        let keys = vec![Key { marks: vec![], bindings: vec![] }];
        let mut cache = OutsideCache::default();
        let allowed = cache.run(&prepared, &row, &[1], &[true, false], &keys).unwrap();
        assert!(cache.run(&prepared, &row, &[1], &[false, false], &keys).is_none());
        assert_eq!(allowed.hits, cache.run(&prepared, &row, &[1], &[true, false], &keys).unwrap().hits);
        assert_eq!(cache.stats.proofs, 1);
        assert_eq!(cache.stats.hits, 2);
        assert_eq!(cache.stats.boundary_rejects, 1);
    }

    #[test]
    fn reused_green_proof_requires_empty_touched_bindings_on_every_entry() {
        let input = crate::hit_timing::time_connections::request();
        let prepared = Prepared::new(&input).unwrap();
        let row = prepared.joint_row();
        let notes: Vec<_> = (0..input.notes.len()).collect();
        let settled = vec![false; notes.len()];
        let mut keys = vec![Key { marks: vec![], bindings: vec![0; prepared.binding_slots] }];
        let mut cache = OutsideCache::default();
        assert!(cache.run(&prepared, &row, &notes, &settled, &keys).is_some());
        keys.push(Key { marks: vec![], bindings: vec![1; prepared.binding_slots] });
        assert!(cache.run(&prepared, &row, &notes, &settled, &keys).is_none());
        keys.pop();
        assert!(cache.run(&prepared, &row, &notes, &settled, &keys).is_some());
        assert_eq!(cache.stats.proofs, 1);
        assert_eq!(cache.stats.hits, 2);
        assert_eq!(cache.stats.boundary_rejects, 1);
    }

    #[test]
    fn intrinsic_failures_are_cached_but_domains_and_retained_relations_distinguish_proofs() {
        let input = input(&[0.1, 0.125]);
        let prepared = Prepared::new(&input).unwrap();
        let mut row = prepared.joint_row();
        let keys = vec![Key { marks: vec![], bindings: vec![] }];
        let settled = [false, false];
        let mut cache = OutsideCache::default();
        assert!(cache.run(&prepared, &row, &[0, 1], &settled, &keys).is_some());
        // Same chart and domains, but a retained impossible gap changes the
        // legality proof. Its negative result is independent of entry state.
        row.relations.push(Relation { before: 0, after: 1, gap: 100,
            earliest_after: None, strict_event_order: false });
        row.incident[0].push(0);
        row.incident[1].push(0);
        assert!(cache.run(&prepared, &row, &[0, 1], &settled, &keys).is_none());
        assert!(cache.run(&prepared, &row, &[0, 1], &settled, &keys).is_none());
        row.domains[0][0].first += 1;
        assert!(cache.run(&prepared, &row, &[0, 1], &settled, &keys).is_none());
        assert_eq!(cache.stats.proofs, 3);
        assert_eq!(cache.stats.negative_hits, 1);
    }

    #[test]
    fn an_oversized_proof_is_returned_complete_without_retention() {
        let input = input(&[0.1]);
        let prepared = Prepared::new(&input).unwrap();
        let mut row = prepared.joint_row();
        let perfect = *row.domains[0].iter().find(|d| d.perfect).unwrap();
        row.domains[0] = vec![perfect; MAX_BYTES / std::mem::size_of::<ActionInterval>() + 1];
        let mut cache = OutsideCache::default();
        let action = cache.run(&prepared, &row, &[0], &[false],
            &[Key { marks: vec![], bindings: vec![] }]).unwrap();
        assert_eq!(&*action.hits, &[(0, input.hit_frames[0])]);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
        assert_eq!(cache.stats.skipped, 1);
    }
}
