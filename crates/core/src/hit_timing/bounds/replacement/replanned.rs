//! Existential replacement witnesses. New inputs are complete, replay-checked
//! Perfect plans; old inputs remain arbitrary. Couple the same future SHAPE,
//! not the same hit frame. Coefficients use a common per-note reference so that
//! an old local upper can never be mistaken for a playable new lower.
use super::*;
use std::sync::{Arc, Mutex};

// Complete successor catalogue, replacement pair, and an immutable owning
// chart/full skill catalogue. No powers or score thresholds enter this value.
pub(super) type SharedDifferences = Arc<Mutex<bank::Bank>>;

mod cache;
mod bank;
mod runs;
pub(super) mod cells;

const CACHE_LIMIT: usize = 200_000;
const WORK_LIMIT: usize = 2_000_000;

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub queries: usize,
    pub hits: usize,
    pub shared_hits: usize,
    pub shared_stores: usize,
    pub plans: usize,
    pub transitions: usize,
    pub old_rows: usize,
    pub old_cell_queries: usize,
    pub old_cell_aliases: usize,
    pub shared_row_hits: usize,
    pub old_frame_evaluations: usize,
    pub old_edge_builds: usize,
    pub old_edge_hits: usize,
    pub suffix_rows: usize,
    pub new_rows: usize,
    pub new_cell_queries: usize,
    pub new_cell_aliases: usize,
    pub edge_builds: usize,
    pub edge_hits: usize,
    pub suffix_hits: usize,
    pub witness_prunes: usize,
    pub suffix_prunes: usize,
    pub budget_fallbacks: usize,
    pub seconds: f64,
}

struct Plan {
    #[cfg(test)]
    hit_frames: Vec<u32>,
    requests: [u32; 6],
    // Input order and Combo/Fever are fixed by the complete legal plan.
    notes: Vec<(u32, f64)>,
    base: f64,
}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct State {
    old: u32,
    new: u32,
}

#[derive(Clone, Copy)]
struct Edge {
    next: State,
    delta: f64,
}

#[derive(Clone, Copy)]
struct IndexedEdge {
    next: State,
    delta: f64,
    index: u32,
}

// Pairing fixes the new clock and gain. Reuse the cached old alternatives
// directly instead of allocating a temporary Vec for every replacement visit.
struct PairedEdges {
    old: Arc<[(u32, f64)]>,
    new: u32,
    gain: f64,
}

impl PairedEdges {
    fn iter(&self) -> impl Iterator<Item = Edge> + '_ {
        self.old.iter().map(|&(old, upper)| Edge {
            next: State { old, new: self.new },
            delta: down(self.gain - upper),
        })
    }
}

type Key = (usize, usize, State); // plan, slot, two ready frames
type Prefix = [Vec<(State, f64)>; 6];

// Preserve the original right-associated suffix additions, then the outer
// prefix/base additions. A flat sum or a subtracted threshold can change the
// directed rounding near an equality boundary.
#[derive(Clone, Copy)]
struct WitnessCutoff {
    best: f64,
    terms: [f64; 8],
    len: usize,
}

impl WitnessCutoff {
    fn new(best: f64, base: f64, prefix: f64) -> Self {
        let mut terms = [0.; 8];
        terms[..2].copy_from_slice(&[base, prefix]);
        Self {
            best,
            terms,
            len: 2,
        }
    }

    fn then(mut self, delta: f64) -> Self {
        self.terms[self.len] = delta;
        self.len += 1;
        self
    }

    fn rejects(&self, mut suffix: f64) -> bool {
        for &term in self.terms[..self.len].iter().rev() {
            suffix = down(term + suffix);
        }
        suffix <= self.best
    }
}

enum Probe {
    Complete(f64),
    Rejected,
}

pub(super) struct Replanned {
    profiles: Vec<Profile>,
    maxima: Vec<Vec<[f64; 2]>>,
    notes: Arc<Vec<Vec<frames::Frame>>>,
    runs: Option<Vec<runs::Runs>>,
    spans: Vec<(u32, u32)>,
    interval_index: Arc<super::super::net::intervals::Index>,
    indexed: bool,
    first: [Vec<u32>; 6],
    last: [Vec<u32>; 6],
    triggers: Arc<Vec<Option<usize>>>,
    references: Vec<f64>,
    reference_total: f64,
    requests: [Vec<u32>; 6],
    plans: Vec<Plan>,
    catalogue: Vec<usize>,
    old_rows: HashMap<(usize, usize, u32), f64>,
    old_edges: HashMap<(usize, usize, u32), Arc<[(u32, f64)]>>,
    idle_suffix: HashMap<(usize, usize), Vec<f64>>,
    new_rows: HashMap<(usize, usize, u32), f64>,
    continuations: cache::Cache,
    prefixes: HashMap<usize, Arc<Prefix>>,
    differences: HashMap<(usize, usize), Option<f64>>,
    pub(super) shared: Option<SharedDifferences>,
    shared_catalogue: Option<Option<u32>>,
    pub(super) shared_rows: Option<SharedFrameCoefficients>,
    row_cells_enabled: bool,
    row_cells: Option<Arc<cells::Index>>,
    work: usize,
    work_limit: usize,
}

fn down(x: f64) -> f64 {
    if x == 0. {
        0.
    } else {
        x.next_down()
    }
}
fn up(x: f64) -> f64 {
    if x == 0. {
        0.
    } else {
        x.next_up()
    }
}

impl Replanned {
    pub(super) fn new(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        frame: &frames::FrameBounds,
    ) -> Result<Self, ReplayError> {
        let mut profiles = Vec::new();
        for skill in skills {
            let mut state = SkillState::new(skill)?;
            let mut states = Vec::new();
            for _ in 0..=input.notes.len() {
                let mut spoiled = state.clone();
                spoiled.multiplier(Judgement::Great);
                states.push([
                    state.clone().multiplier(Judgement::Great),
                    state.clone().multiplier(Judgement::Perfect),
                    spoiled.clone().multiplier(Judgement::Great),
                    spoiled.multiplier(Judgement::Perfect),
                ]);
                state.multiplier(Judgement::Perfect);
            }
            profiles.push(Profile::new(countdown(skill.duration) + 1,
                skill.cache_key()[1], states));
        }
        let maxima = profiles
            .iter()
            .map(|p| {
                let mut upper = [0_f64; 2];
                p.states
                    .iter()
                    .map(|r| {
                        for g in 0..2 {
                            upper[g] = upper[g].max(f64::from(r[g])).max(f64::from(r[g + 2]));
                        }
                        upper
                    })
                    .collect()
            })
            .collect();
        let spans: Vec<_> = frame
            .notes
            .iter()
            .map(|fs| {
                (
                    fs.iter().map(|f| f.time).min().unwrap(),
                    fs.iter().map(|f| f.time).max().unwrap(),
                )
            })
            .collect();
        let endpoints = |end: bool| {
            std::array::from_fn(|slot| {
                let mut values: Vec<_> = spans
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| frame.triggers[i].is_none_or(|s| s > slot))
                    .map(|(_, &(a, b))| if end { b } else { a })
                    .collect();
                values.sort_unstable();
                values
            })
        };
        let first = endpoints(false);
        let last = endpoints(true);
        let references = frame.note_baselines();
        let reference_total = references.iter().fold(0., |v, &x| up(v + x));
        let mut requests: [Vec<u32>; 6] = std::array::from_fn(|_| vec![]);
        for (i, n) in input.notes.iter().enumerate() {
            if let Some(s) = n.trigger {
                requests[s] = frame.notes[i].iter().map(|f| f.time).collect();
                requests[s].sort_unstable();
                requests[s].dedup();
            }
        }
        let mut plans = Vec::new();
        let domains: Vec<Vec<u32>> = frame
            .notes
            .iter()
            .map(|fs| fs.iter().map(|f| f.time).collect())
            .collect();
        for replayed in crate::hit_timing::witness::perfect(input, &domains) {
            let mut req = [0; 6];
            for (i, n) in input.notes.iter().enumerate() {
                if let Some(s) = n.trigger {
                    req[s] = replayed.hit_frames[i];
                }
            }
            // The native replay validates request order, routing and constraints.
            let scale = down(
                down(
                    f64::from(1_f32 + (input.play_level - 5) as f32 * 0.01_f32)
                        / f64::from(input.notes.len() as f32),
                ) * 3.,
            );
            let notes: Vec<_> = replayed
                .trace
                .iter()
                .map(|t| {
                    let w = down(
                        down(
                            down(scale * f64::from(Judgement::Perfect.multiplier()))
                                * f64::from(input.combo_multiplier(t.combo)),
                        ) * f64::from(t.fever_multiplier),
                    );
                    (t.frame, w)
                })
                .collect();
            let base = notes.iter().fold(0., |v, &(_, w)| down(v + w));
            plans.push(Plan {
                #[cfg(test)]
                hit_frames: replayed.hit_frames,
                requests: req,
                notes,
                base,
            });
        }
        Ok(Self {
            profiles,
            maxima,
            notes: frame.notes.clone(),
            runs: (std::env::var("BANDORI_TIMING_DISABLE_REPLACEMENT_MAX_RUNS").as_deref()!=Ok("1"))
                .then(|| frame.notes.iter().map(|fs| runs::Runs::new(fs)).collect()),
            spans,
            interval_index: Arc::clone(&frame.interval_index),
            indexed: std::env::var("BANDORI_TIMING_DISABLE_REPLACEMENT_INTERVAL_INDEX")
                .as_deref() != Ok("1"),
            first,
            last,
            triggers: frame.triggers.clone(),
            references,
            reference_total,
            requests,
            plans,
            catalogue: (0..skills.len()).collect(),
            old_rows: HashMap::new(),
            old_edges: HashMap::new(),
            idle_suffix: HashMap::new(),
            new_rows: HashMap::new(),
            continuations: cache::Cache::default(),
            prefixes: HashMap::new(),
            differences: HashMap::new(),
            shared: None,
            shared_catalogue: None,
            shared_rows: None,
            row_cells_enabled: std::env::var("BANDORI_TIMING_DISABLE_PROOF_ROW_CELLS").as_deref()!=Ok("1"),
            row_cells: None,
            work: 0,
            work_limit: WORK_LIMIT,
        })
    }

    pub(super) fn set_catalogue(&mut self, ids: &[usize]) {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        assert!(ids.iter().all(|&i| i < self.profiles.len()));
        if ids != self.catalogue {
            self.catalogue = ids;
            self.shared_catalogue = None;
            // Cached edges contain node indices. Rebuild both together so no
            // row can address a node from a different successor catalogue.
            self.continuations = cache::Cache::default();
            self.prefixes.clear();
            self.differences.clear();
        }
    }

    pub(super) fn witness_floor(&self) -> f64 {
        // Every witness is raw Perfect, and all admitted skills have a P
        // multiplier >= 1. This baseline is valid for every successor choice.
        self.plans
            .iter()
            .map(|p| p.base)
            .fold(f64::INFINITY, f64::min)
    }

    fn tick(&mut self, stats: &mut Stats) -> Option<()> {
        self.work += 1;
        stats.transitions += 1;
        (self.work <= self.work_limit).then_some(())
    }

    fn old_upper(&mut self, slot: usize, id: usize, start: u32, stats: &mut Stats) -> f64 {
        let row_start = if self.row_cells_enabled {
            if self.row_cells.is_none() {
                self.row_cells = Some(if let Some(cache) = &self.shared_rows {
                    cache.lock().unwrap().cells(|| cells::Index::new(&self.profiles,&self.notes))
                } else { Arc::new(cells::Index::new(&self.profiles,&self.notes)) });
            }
            stats.old_cell_queries += 1;
            let key = self.row_cells.as_ref().unwrap().start(id,self.profiles[id].playing,start);
            stats.old_cell_aliases += usize::from(key != start);
            key
        } else { start };
        let key = (slot, id, row_start);
        if let Some(&v) = self.old_rows.get(&key) {
            return v;
        }
        let shared_key = Self::row_key(0,slot,id,row_start);
        if let Some(value) = shared_key.and_then(|k| self.shared_rows.as_ref()
            .and_then(|c| c.lock().unwrap().row(k))) {
            stats.shared_row_hits += 1;
            if self.old_rows.len() < CACHE_LIMIT { self.old_rows.insert(key,value); }
            return value;
        }
        let p = &self.profiles[id];
        let end = start + p.playing;
        let mut total = 0.;
        let candidates = if self.indexed { self.interval_index.query(start + 1, end) }
            else { (0..self.notes.len()).collect() };
        for i in candidates {
            let fs = &self.notes[i];
            if self.triggers[i].is_some_and(|s| s <= slot) {
                continue;
            }
            if self.spans[i].1 <= start || self.spans[i].0 > end {
                continue;
            }
            // Bound the number of earlier Perfects by notes that can actually
            // intersect this window, rather than letting a short crescendo
            // window attain the whole-song maximum.
            let latest = self.spans[i].1.min(end);
            let count = self.first[slot].partition_point(|&f| f <= latest)
                - self.last[slot].partition_point(|&f| f <= start)
                - 1;
            let mut best = 0_f64; // omitting a mandatory negative only widens U
            let mut evaluate = |raw,weight| {
                stats.old_frame_evaluations += 1;
                if let Some(g) = frames::corrected(raw, p.correction) {
                    let grade = if g == 0 {
                        Judgement::Great
                    } else {
                        Judgement::Perfect
                    };
                    let scored =
                        up(up(weight * f64::from(grade.multiplier()))
                            * self.maxima[id][count][g]);
                    best = best.max(up(scored - self.references[i]));
                }
            };
            if let Some(runs) = &self.runs {
                for (raw,weight) in runs[i].candidates(start,end) { evaluate(raw,weight); }
            } else {
                for f in fs.iter().filter(|f| start < f.time && f.time <= end) { evaluate(f.raw,f.weight[1]); }
            }
            total = up(total + best);
        }
        stats.old_rows += 1;
        if self.old_rows.len() < CACHE_LIMIT {
            self.old_rows.insert(key, total);
        }
        if let (Some(shared),Some(key)) = (&self.shared_rows,shared_key) {
            shared.lock().unwrap().store_row(key,total);
        }
        total
    }

    fn row_key(kind: u32, slot: usize, id: usize, start: u32) -> Option<[u32;4]> {
        Some([kind,u32::try_from(slot).ok()?,u32::try_from(id).ok()?,start])
    }

    fn new_lower(&mut self, plan: usize, id: usize, start: u32, stats: &mut Stats) -> f64 {
        let p = &self.profiles[id];
        let notes = &self.plans[plan].notes;
        let first = notes.partition_point(|&(t, _)| t <= start);
        let last = notes.partition_point(|&(t, _)| t <= start + p.playing);
        let row_start = if self.row_cells_enabled {
            stats.new_cell_queries += 1;
            let key = cells::witness_start(notes,first,last,p.playing,start);
            stats.new_cell_aliases += usize::from(key != start);
            key
        } else { start };
        let key = (plan, id, row_start);
        if let Some(&v) = self.new_rows.get(&key) {
            return v;
        }
        let shared_key = Self::row_key(1,plan,id,row_start);
        if let Some(value) = shared_key.and_then(|k| self.shared_rows.as_ref()
            .and_then(|c| c.lock().unwrap().row(k))) {
            stats.shared_row_hits += 1;
            if self.new_rows.len() < CACHE_LIMIT { self.new_rows.insert(key,value); }
            return value;
        }
        let mut total = 0.;
        for (count, &(_, weight)) in notes[first..last].iter().enumerate() {
            // Raw P is unchanged by any correction. Perfect-only, continuous
            // and crescendo all use their actual native state progression.
            let gain = down(f64::from(p.states[count][1]) - 1.);
            debug_assert!(gain >= 0.);
            total = down(total + down(weight * gain));
        }
        stats.new_rows += 1;
        if self.new_rows.len() < CACHE_LIMIT {
            self.new_rows.insert(key, total);
        }
        if let (Some(shared),Some(key)) = (&self.shared_rows,shared_key) {
            shared.lock().unwrap().store_row(key,total);
        }
        total
    }

    fn canonical(&self, plan: usize, slot: usize, state: State) -> State {
        if slot == 6 {
            return State { old: 0, new: 0 };
        }
        State {
            old: if state.old <= self.requests[slot][0] {
                0
            } else {
                state.old
            },
            new: if state.new <= self.plans[plan].requests[slot] {
                0
            } else {
                state.new
            },
        }
    }

    fn pair_edges(
        &mut self,
        plan: usize,
        slot: usize,
        state: State,
        a: usize,
        b: usize,
        stats: &mut Stats,
    ) -> Option<PairedEdges> {
        let ns = state.new.max(self.plans[plan].requests[slot]);
        let gain = self.new_lower(plan, a, ns, stats);
        let cooldown = countdown(0.75) + 2;
        let next_new = self
            .canonical(
                plan,
                slot + 1,
                State {
                    old: 0,
                    new: ns + self.profiles[a].playing + cooldown,
                },
            )
            .new;
        let old = self.old_edges(slot, b, state.old, stats);
        // Charge the whole row before exposing it, preserving the original
        // failure boundary and cache effects even when a budget ends mid-row.
        for _ in 0..old.len() {
            self.tick(stats)?;
        }
        Some(PairedEdges {
            old,
            new: next_new,
            gain,
        })
    }

    fn old_edges(
        &mut self,
        slot: usize,
        id: usize,
        ready: u32,
        stats: &mut Stats,
    ) -> Arc<[(u32, f64)]> {
        let key = (slot, id, ready);
        if let Some(edges) = self.old_edges.get(&key) {
            stats.old_edge_hits += 1;
            return edges.clone();
        }
        stats.old_edge_builds += 1;
        let delay = self.profiles[id].playing + countdown(0.75) + 2;
        let next_hit = if slot == 5 {
            u32::MAX
        } else {
            self.requests[slot + 1][0]
        };
        let cut = self.requests[slot].partition_point(|&r| r + delay <= next_hit);
        let begin = self.requests[slot].partition_point(|&r| r <= ready);
        let mut merged = BTreeMap::<u32, f64>::new();
        // Every already queued request has the SAME actual start, exactly ready.
        if begin > 0 {
            let next = if ready + delay <= next_hit {
                0
            } else {
                ready + delay
            };
            merged.insert(next, self.old_upper(slot, id, ready, stats));
        }
        // These unqueued starts all lead to the same idle next state. A reverse
        // maximum gives their worst old contribution with one indexed lookup.
        if begin < cut {
            let table_key = (slot, id);
            if !self.idle_suffix.contains_key(&table_key) {
                let mut table = vec![0_f64; cut + 1];
                for i in (0..cut).rev() {
                    table[i] =
                        table[i + 1].max(self.old_upper(slot, id, self.requests[slot][i], stats));
                }
                stats.suffix_rows += cut;
                self.idle_suffix.insert(table_key, table);
            }
            let upper = self.idle_suffix[&table_key][begin];
            merged
                .entry(0)
                .and_modify(|v| *v = v.max(upper))
                .or_insert(upper);
        }
        // Keep distinct busy successor frames; a suffix maximum cannot erase
        // the correlation between contribution and the next actual start.
        for i in begin.max(cut)..self.requests[slot].len() {
            let start = self.requests[slot][i];
            merged.insert(start + delay, self.old_upper(slot, id, start, stats));
        }
        let edges: Arc<[(u32, f64)]> = merged.into_iter().collect::<Vec<_>>().into();
        if self.old_edges.len() < CACHE_LIMIT {
            self.old_edges.insert(key, edges.clone());
        }
        edges
    }

    fn common_edges(&mut self, key: Key, stats: &mut Stats) -> Option<Arc<[IndexedEdge]>> {
        let index = self.continuations.index(key);
        self.common_edges_at(key, index, stats)
    }

    fn common_edges_at(
        &mut self,
        key: Key,
        index: u32,
        stats: &mut Stats,
    ) -> Option<Arc<[IndexedEdge]>> {
        if let Some(v) = self.continuations.edges(key, index) {
            stats.edge_hits += 1;
            return Some(v);
        }
        stats.edge_builds += 1;
        let (plan, slot, state) = key;
        let mut merged = BTreeMap::<State, f64>::new();
        // As in the old engine, equal successor pairs share the worst edge.
        // On this old-upper/new-P-witness path correction cannot affect clocks.
        for id in self.catalogue.clone() {
            for e in self.pair_edges(plan, slot, state, id, id, stats)?.iter() {
                merged
                    .entry(e.next)
                    .and_modify(|v| *v = v.min(e.delta))
                    .or_insert(e.delta);
            }
        }
        // Recursion only reads these rows. Share their ownership across mutable
        // cache calls instead of copying every edge at each suffix visit.
        let edges: Arc<[_]> = merged
            .into_iter()
            .map(|(next, delta)| IndexedEdge {
                next,
                delta,
                index: self.continuations.index((plan, slot + 1, next)),
            })
            .collect::<Vec<_>>()
            .into();
        self.continuations.store_edges(key, index, edges.clone());
        Some(edges)
    }

    fn prefix(&mut self, plan: usize, stats: &mut Stats) -> Option<Arc<Prefix>> {
        if let Some(v) = self.prefixes.get(&plan) {
            return Some(v.clone());
        }
        let mut rows: Prefix = std::array::from_fn(|_| vec![]);
        rows[0].push((State { old: 0, new: 0 }, 0.));
        for slot in 0..5 {
            let mut next = BTreeMap::<State, f64>::new();
            for (state, value) in rows[slot].clone() {
                for edge in self.common_edges((plan, slot, state), stats)?.iter() {
                    let v = down(value + edge.delta);
                    next.entry(edge.next)
                        .and_modify(|x| *x = x.min(v))
                        .or_insert(v);
                }
            }
            rows[slot + 1] = next.into_iter().collect();
        }
        let rows = Arc::new(rows);
        self.prefixes.insert(plan, rows.clone());
        Some(rows)
    }

    fn suffix(
        &mut self,
        key: Key,
        index: u32,
        encore: Option<(usize, usize)>,
        captain: &mut cache::Captain,
        stats: &mut Stats,
    ) -> Option<f64> {
        let (plan, slot, state) = key;
        if slot == 6 {
            return Some(0.);
        }
        let cached = if encore.is_some() {
            self.continuations.captain(key, index, captain)
        } else {
            self.continuations.ordinary(key, index)
        };
        if let Some(v) = cached {
            stats.suffix_hits += 1;
            return Some(v);
        }
        // Equal clocks do NOT imply zero: the two sides use different inputs.
        let mut value = f64::INFINITY;
        if let Some((a, b)) = encore.filter(|_| slot == 5) {
            for edge in self.pair_edges(plan, slot, state, a, b, stats)?.iter() {
                // Slot six's suffix is exactly zero; retain both downward
                // rounding operations from the original recursive path.
                value = value.min(down(edge.delta + 0.));
            }
        } else {
            for edge in self.common_edges_at(key, index, stats)?.iter() {
                value = value.min(down(
                    edge.delta
                        + self.suffix(
                            (plan, slot + 1, edge.next),
                            edge.index,
                            encore,
                            captain,
                            stats,
                        )?,
                ));
            }
        }
        if encore.is_some() {
            self.continuations.store_captain(key, index, captain, value);
        } else {
            self.continuations.store_ordinary(key, index, value);
        }
        Some(value)
    }

    // Look for ONE old continuation that defeats this new witness. Completed
    // minima may be cached normally; interrupted minima are never cached.
    // This is used only for later witnesses' pair-dependent captain suffixes.
    fn captain_probe(
        &mut self,
        key: Key,
        index: u32,
        pair: (usize, usize),
        captain: &mut cache::Captain,
        cutoff: WitnessCutoff,
        stats: &mut Stats,
    ) -> Option<Probe> {
        let (plan, slot, state) = key;
        if let Some(value) = self.continuations.captain(key, index, captain) {
            stats.suffix_hits += 1;
            return Some(if cutoff.rejects(value) {
                stats.suffix_prunes += 1;
                Probe::Rejected
            } else {
                Probe::Complete(value)
            });
        }
        let mut value = f64::INFINITY;
        if slot == 5 {
            for edge in self
                .pair_edges(plan, slot, state, pair.0, pair.1, stats)?
                .iter()
            {
                value = value.min(down(edge.delta + 0.));
                if cutoff.rejects(value) {
                    stats.suffix_prunes += 1;
                    return Some(Probe::Rejected);
                }
            }
        } else {
            for edge in self.common_edges_at(key, index, stats)?.iter() {
                match self.captain_probe(
                    (plan, slot + 1, edge.next),
                    edge.index,
                    pair,
                    captain,
                    cutoff.then(edge.delta),
                    stats,
                )? {
                    Probe::Rejected => return Some(Probe::Rejected),
                    Probe::Complete(tail) => value = value.min(down(edge.delta + tail)),
                }
            }
        }
        self.continuations.store_captain(key, index, captain, value);
        Some(Probe::Complete(value))
    }

    pub(super) fn difference(&mut self, a: usize, b: usize, stats: &mut Stats) -> Option<f64> {
        stats.queries += 1;
        stats.plans = self.plans.len();
        if let Some(&v) = self.differences.get(&(a, b)) {
            stats.hits += 1;
            return v;
        }
        if self.shared_catalogue.is_none() {
            self.shared_catalogue = self.shared.as_ref().map(|s| s.lock().unwrap().catalogue(&self.catalogue));
        }
        let key = self.shared_catalogue.flatten().map(|id| (id,a,b));
        if let (Some(shared), Some(key)) = (&self.shared, &key) {
            if let Some(&v) = shared.lock().unwrap().get(key) {
                stats.shared_hits += 1;
                self.differences.insert((a, b), Some(v));
                return Some(v);
            }
        }
        self.work = 0;
        let result = self.compute(a, b, stats);
        if result.is_none() && self.work > self.work_limit {
            stats.budget_fallbacks += 1;
        }
        self.differences.insert((a, b), result);
        // Cache completed certificates only. A budget failure must not become
        // a cross-domain negative proof; cache saturation merely stops reuse.
        if let (Some(shared), Some(key), Some(value)) = (&self.shared, key, result) {
            let mut shared = shared.lock().unwrap();
            if shared.store(key, value) {
                stats.shared_stores += 1;
            }
        }
        result
    }

    fn compute(&mut self, a: usize, b: usize, stats: &mut Stats) -> Option<f64> {
        self.compute_with_witness_pruning::<true>(a, b, stats)
    }

    fn compute_with_witness_pruning<const PRUNE: bool>(
        &mut self,
        a: usize,
        b: usize,
        stats: &mut Stats,
    ) -> Option<f64> {
        if self.plans.is_empty() || self.catalogue.is_empty() {
            return None;
        }
        let mut roles = [f64::NEG_INFINITY; 10];
        for plan in 0..self.plans.len() {
            let prefix = self.prefix(plan, stats)?;
            let base = down(self.plans[plan].base - self.reference_total);
            // Generation stamps discard the previous pair/witness's encore
            // values without clearing a large table at every replacement.
            let mut captain = self.continuations.begin_captain();
            'roles: for role in 0..10 {
                let slot = role % 5;
                let encore = (role >= 5).then_some((a, b));
                let mut value = f64::INFINITY;
                'states: for &(state, before) in &prefix[slot] {
                    for edge in self.pair_edges(plan, slot, state, a, b, stats)?.iter() {
                        let key = (plan, slot + 1, edge.next);
                        let index = self.continuations.index(key);
                        let rest = if PRUNE && plan > 0 && encore.is_some() {
                            let cutoff =
                                WitnessCutoff::new(roles[role], base, down(before + edge.delta));
                            match self.captain_probe(
                                key,
                                index,
                                (a, b),
                                &mut captain,
                                cutoff,
                                stats,
                            )? {
                                Probe::Complete(value) => value,
                                Probe::Rejected => {
                                    stats.witness_prunes += 1;
                                    continue 'roles;
                                }
                            }
                        } else {
                            self.suffix(key, index, encore, &mut captain, stats)?
                        };
                        value = value.min(down(down(before + edge.delta) + rest));
                        // A witness must cover EVERY old continuation. Once one
                        // continuation is no better than the existing complete
                        // witness, taking further minima cannot improve this
                        // role. Compare with exactly the final rounding order;
                        // publish neither a partial minimum nor a budget failure.
                        if PRUNE && plan > 0 && down(base + value) <= roles[role] {
                            stats.witness_prunes += 1;
                            break 'states;
                        }
                    }
                }
                // A different complete witness may be selected for each role.
                roles[role] = roles[role].max(down(base + value));
            }
        }
        let answer = roles.into_iter().fold(f64::INFINITY, f64::min);
        answer.is_finite().then_some(answer)
    }
}

#[cfg(test)]
mod tests;
