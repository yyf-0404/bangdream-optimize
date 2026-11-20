//! Quotient repeated frame closures without changing the audit graph.
//!
//! A cell has the same ordinary events, grades, factors and root blockers at
//! every frame. Its events have no retained timestamp or binding writes. The
//! first frame already explores every legal touch sequence (each note once).
//! Repeating that closure cannot add a path if the value clock is also stable.
//! Clock stability is checked against the actual states during each fresh DP,
//! so skill durations can continue to share the same geometry.
use super::*;

pub(in crate::hit_timing::independent) struct TemporalBlock {
    pub start: usize,
    pub after_first: usize,
    /// Resume before the last frame's retirement, preserving every deadline.
    pub resume: usize,
    pub first_frame: u32,
    pub last_frame: u32,
    pub count: usize,
    /// Empty means identity. Otherwise first-frame node -> resume node.
    pub permutation: Vec<usize>,
}

struct Pending {
    first: u32,
    last: u32,
    start: usize,
    after_first: usize,
    keys: Vec<Key>,
}

pub(super) struct Tracker {
    cells: Vec<(u32, u32)>,
    next: usize,
    pending: Option<Pending>,
}

impl Tracker {
    pub fn new(
        prepared: &Prepared,
        row: &Row,
        frames: &BTreeMap<u32, Vec<(usize, ActionInterval)>>,
        options: WindowOptions,
    ) -> Self {
        let mut cells = vec![];
        // Component templates predate these descriptors. Joint scoring uses no
        // component template cache; other consumers retain their original DP.
        let enabled = options.optimize_touch_order && options.max_cached_component_edges == 0
            && std::env::var("BANDORI_TIMING_DISABLE_FRAME_INTERVALS").as_deref() != Ok("1");
        if enabled {
            let mut cell: Option<(u32, u32)> = None;
            let mut previous: Option<(u32, &[(usize, ActionInterval)])> = None;
            for (&frame, events) in frames {
                let ordinary = events.iter().all(|&(note, _)| {
                    prepared.input.notes[note].trigger.is_none()
                        && row.incident[note].is_empty()
                        && prepared.heads[note].is_none()
                        && prepared.ends[note].is_none()
                });
                let same = ordinary && previous.is_some_and(|(last, old)| {
                    last.checked_add(1) == Some(frame) && old == events.as_slice()
                        && events.iter().all(|&(note, _)| {
                            prepared.blockers(note, last) == prepared.blockers(note, frame)
                                && raw(prepared, note, last) == raw(prepared, note, frame)
                        })
                });
                if same {
                    cell.as_mut().unwrap().1 = frame;
                } else {
                    if let Some((first, last)) = cell.take().filter(|(a, b)| a < b) {
                        cells.push((first, last));
                    }
                    if ordinary { cell = Some((frame, frame)); }
                }
                previous = ordinary.then_some((frame, events));
            }
            if let Some((first, last)) = cell.filter(|(a, b)| a < b) {
                cells.push((first, last));
            }
        }
        Self { cells, next: 0, pending: None }
    }

    pub fn begin(&mut self, frame: u32, layer: usize) {
        if let Some(&(first, last)) = self.cells.get(self.next) {
            if frame == first {
                self.pending = Some(Pending {
                    first, last, start: layer, after_first: 0, keys: vec![],
                });
            }
        }
    }

    pub fn after_touches(
        &mut self, frame: u32, keys: &[Key], graph: &mut Graph, stats: &mut WindowStats,
    ) {
        let Some(p) = self.pending.as_ref().filter(|p| p.last == frame) else { return; };
        // Passive retained timestamps may saturate while waiting. Compare the
        // complete keys, not just consumed bits, and retain the exact graph if
        // the first closure cannot be carried to the last retirement boundary.
        if p.keys.len() != keys.len() || p.after_first >= graph.layers.len() {
            stats.temporal_geometry_rejects += 1;
            return;
        }
        let permutation = if p.keys == keys {
            vec![]
        } else {
            let ids: HashMap<_, _> = keys.iter().enumerate().map(|(i, key)| (key, i)).collect();
            let Some(map) = p.keys.iter().map(|key| ids.get(key).copied())
                .collect::<Option<Vec<_>>>() else {
                    stats.temporal_geometry_rejects += 1;
                    return;
                };
            map
        };
        stats.temporal_intervals += 1;
        stats.temporal_candidate_frames += (p.last - p.first) as usize;
        graph.temporal.push(TemporalBlock {
            start: p.start, after_first: p.after_first, resume: graph.layers.len(),
            first_frame: p.first, last_frame: p.last, count: keys.len(), permutation,
        });
    }

    pub fn after_frame(&mut self, frame: u32, layer: usize, keys: &[Key]) {
        if let Some(p) = self.pending.as_mut() {
            if frame == p.first {
                p.after_first = layer;
                p.keys = keys.to_vec();
            }
            if frame == p.last {
                self.pending = None;
                self.next += 1;
            }
        }
    }
}

fn raw(prepared: &Prepared, note: usize, frame: u32) -> Judgement {
    let intervals = &prepared.intervals[note];
    let index = intervals.partition_point(|d| d.last < frame);
    intervals[index].raw
}
