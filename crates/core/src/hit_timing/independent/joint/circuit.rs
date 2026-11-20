//! A bounded max/add circuit over exact states, with fresh native leaves at
//! every power. No path variants, prefix scores or winners survive a query.
use super::*;
use crate::hit_timing::compiled::Term;
use crate::scoring::ScoringError;
use std::sync::Weak;

const MAX_NODES: usize = 128 * 1024;
const MAX_WORK: u64 = 1_000_000;
const MAX_STATES: usize = 4096;
const MAX_LIVE: usize = 32768;
const PAYLOAD_BYTES: usize = 7 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;

pub(super) fn enabled() -> bool {
    std::env::var("BANDORI_TIMING_DISABLE_SCORE_CIRCUITS").as_deref() == Ok("0")
}

#[derive(Clone, Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircuitStats {
    pub queries: u64,
    pub hits: u64,
    pub builds: u64,
    pub budget_fallbacks: u64,
    pub arithmetic_fallbacks: u64,
    pub compile_work: u64,
    pub compiled_nodes: u64,
    pub evaluated_nodes: u64,
    pub evaluated_terms: u64,
    pub retained_bytes: usize,
    pub entries: usize,
    pub evictions: u64,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum Node { Zero, Term(u32), Add(u32, u32), Max(u32, u32), Scale(u32, u32) }

struct Program { terms: Vec<Term>, nodes: Vec<Node>, root: u32 }
impl Program {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.terms.capacity() * std::mem::size_of::<Term>()
            + self.nodes.capacity() * std::mem::size_of::<Node>()
    }
    fn score(&self, scorer: NoteScorer) -> Result<u64, ScoringError> {
        let terms = self.terms.iter().map(|t| scorer.score_with_judgement_multiplier(
            f32::from_bits(t[0]), f32::from_bits(t[1]), f32::from_bits(t[2]), 1.)
            .map(|s| u64::from(s.final_score))).collect::<Result<Vec<_>, _>>()?;
        let mut values: Vec<u64> = Vec::with_capacity(self.nodes.len());
        for &node in &self.nodes {
            let value = match node {
                Node::Zero => 0,
                Node::Term(t) => terms[t as usize],
                Node::Add(a, b) => values[a as usize].checked_add(values[b as usize])
                    .ok_or(ScoringError::TotalScoreOverflow)?,
                Node::Max(a, b) => values[a as usize].max(values[b as usize]),
                Node::Scale(a, count) => values[a as usize].checked_mul(u64::from(count))
                    .ok_or(ScoringError::TotalScoreOverflow)?,
            };
            values.push(value);
        }
        Ok(values[self.root as usize])
    }
}

struct Builder {
    nodes: Vec<Node>, ids: HashMap<Node, u32>, terms: Vec<Term>, term_ids: HashMap<Term, u32>,
    limit: usize,
}
impl Builder {
    fn new(limit: usize) -> Self {
        Self { nodes: vec![Node::Zero], ids: HashMap::from([(Node::Zero, 0)]),
            terms: vec![], term_ids: HashMap::new(), limit }
    }
    fn node(&mut self, node: Node) -> Option<u32> {
        if let Some(&id) = self.ids.get(&node) { return Some(id); }
        if self.nodes.len() >= self.limit { return None; }
        let id = self.nodes.len() as u32;
        self.nodes.push(node); self.ids.insert(node, id); Some(id)
    }
    fn add(&mut self, a: u32, b: u32) -> Option<u32> {
        if a == 0 { return Some(b); }
        if b == 0 { return Some(a); }
        self.node(Node::Add(a.min(b), a.max(b)))
    }
    fn maximum(&mut self, a: u32, b: u32) -> Option<u32> {
        if a == b { return Some(a); }
        // All accepted native scores are nonnegative.
        if a == 0 { return Some(b); }
        if b == 0 { return Some(a); }
        self.node(Node::Max(a.min(b), a.max(b)))
    }
    fn contribution(&mut self, terms: Vec<Term>) -> Option<u32> {
        let mut counts = std::collections::BTreeMap::new();
        for term in terms {
            if term.iter().any(|&v| !f32::from_bits(v).is_finite() || f32::from_bits(v) < 0.) {
                return None;
            }
            *counts.entry(term).or_insert(0u32) += 1;
        }
        let mut total = 0;
        for (term, count) in counts {
            let next = self.terms.len() as u32;
            let id = *self.term_ids.entry(term).or_insert_with(|| {
                self.terms.push(term); next
            });
            let leaf = self.node(Node::Term(id))?;
            let leaf = if count == 1 { leaf } else { self.node(Node::Scale(leaf, count))? };
            total = self.add(total, leaf)?;
        }
        Some(total)
    }
    fn finish(self, root: u32) -> Program {
        // Drop expression nodes that led only to an infeasible suffix. Keep
        // all encountered native terms: any arithmetic error causes fallback.
        let mut keep = vec![false; self.nodes.len()];
        keep[root as usize] = true; keep[0] = true;
        for i in (0..self.nodes.len()).rev() {
            if !keep[i] { continue; }
            match self.nodes[i] {
                Node::Add(a, b) | Node::Max(a, b) => { keep[a as usize] = true; keep[b as usize] = true; }
                Node::Scale(a, _) => keep[a as usize] = true,
                _ => {},
            }
        }
        let mut map = vec![0; self.nodes.len()];
        let mut nodes = Vec::with_capacity(keep.iter().filter(|&&k| k).count());
        for (i, node) in self.nodes.into_iter().enumerate() {
            if !keep[i] { continue; }
            map[i] = nodes.len() as u32;
            nodes.push(match node {
                Node::Add(a, b) => Node::Add(map[a as usize], map[b as usize]),
                Node::Max(a, b) => Node::Max(map[a as usize], map[b as usize]),
                Node::Scale(a, n) => Node::Scale(map[a as usize], n),
                x => x,
            });
        }
        Program { terms: self.terms, nodes, root: map[root as usize] }
    }
}

type Bank = Vec<Vec<(usize, u32)>>;
fn insert(row: &mut Vec<(usize, u32)>, state: usize, expr: u32, b: &mut Builder) -> Option<()> {
    match row.binary_search_by_key(&state, |&(s, _)| s) {
        Ok(i) => row[i].1 = b.maximum(row[i].1, expr)?,
        Err(i) => row.insert(i, (state, expr)),
    }
    Some(())
}

fn compile(model: &JointTimeline, limit: usize, work_limit: u64, stats: &mut CircuitStats) -> Option<Program> {
    if model.graph.layers.is_empty() || model.graph.touches.iter().any(|t| t.incremental.is_some()) {
        return None;
    }
    let mut costs = JointCosts::new(model, ScoringPower::new(0.).ok()?).ok()?;
    let mut builder = Builder::new(limit);
    let mut values: Bank = vec![vec![(0, 0)]];
    let mut touch_index = 0;
    let mut work = 0;
    for (layer_index, layer) in model.graph.layers.iter().enumerate() {
        if layer.count > MAX_LIVE { return None; }
        let mut next: Bank = vec![vec![]; layer.count];
        let touch = model.graph.touches.get(touch_index).filter(|t| t.layer == layer_index);
        if let Some(touch) = touch {
            touch_index += 1;
            for (from, &to) in touch.inputs.iter().enumerate() {
                for &(state, expr) in &values[from] { insert(&mut next[to], state, expr, &mut builder)?; }
            }
        }
        for edge in &layer.edges {
            let source = if touch.is_some() { next[edge.from].clone() } else { values[edge.from].clone() };
            for (mut state, mut expr) in source {
                work += 1; stats.compile_work += 1;
                if work > work_limit { return None; }
                if let Some(index) = edge.action {
                    let Some(terms) = costs.symbolic_terms(index, state) else { continue; };
                    let add = builder.contribution(terms)?;
                    let Some((after, _)) = costs.action_at(index, &model.graph.actions.get(index), state).ok()?
                        else { continue; };
                    if costs.states.len() > MAX_STATES { return None; }
                    state = after; expr = builder.add(expr, add)?;
                }
                insert(&mut next[edge.to], state, expr, &mut builder)?;
            }
        }
        if next.iter().map(Vec::len).sum::<usize>() > MAX_LIVE { return None; }
        values = next;
    }
    let mut root = None;
    for (_, expr) in values.into_iter().flatten() {
        root = Some(match root { None => expr, Some(old) => builder.maximum(old, expr)? });
    }
    let program = builder.finish(root?);
    stats.compiled_nodes += program.nodes.len() as u64;
    Some(program)
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key { geometry: usize, skills: Vec<[u32; 6]> }
enum Status { Pending(u8), Complete(Option<Arc<Program>>) }
struct Entry { _graph: Weak<Graph>, status: Status, stamp: u64, bytes: usize }
#[derive(Default)]
pub(super) struct Cache { entries: HashMap<Key, Entry>, bytes: usize, clock: u64, stats: CircuitStats }
impl Cache {
    pub(super) fn stats(&self) -> CircuitStats {
        let mut stats = self.stats.clone();
        stats.entries = self.entries.len();
        stats.retained_bytes = self.bytes + self.entries.capacity()
            * (std::mem::size_of::<(Key, Entry)>() + 1);
        stats
    }
    fn store(&mut self, model: &JointTimeline, key: Key, status: Status) {
        let bytes = key.skills.capacity() * std::mem::size_of::<[u32; 6]>()
            + match &status { Status::Complete(Some(p)) => p.bytes(), _ => 0 };
        if let Some(old) = self.entries.remove(&key) { self.bytes -= old.bytes; }
        if bytes > PAYLOAD_BYTES { return; }
        while self.bytes + bytes > PAYLOAD_BYTES || self.entries.len() >= MAX_ENTRIES {
            let key = self.entries.iter().min_by_key(|(_, e)| e.stamp).unwrap().0.clone();
            self.bytes -= self.entries.remove(&key).unwrap().bytes;
            self.stats.evictions += 1;
        }
        self.bytes += bytes;
        // Weak ownership keeps the Arc identity unique without retaining the
        // large graph after its normal geometry owner evicts it.
        self.entries.insert(key, Entry { _graph: Arc::downgrade(&model.graph), status,
            stamp: self.clock, bytes });
    }
}

pub(super) fn score(model: &JointTimeline, power: ScoringPower)
    -> Result<Option<(u64, WindowStats)>, SearchError> {
    let key = Key { geometry: Arc::as_ptr(&model.graph) as usize,
        skills: model.input.skills.iter().map(ReplaySkill::cache_key).collect() };
    let mut owner = model.functions.lock().unwrap();
    let cache = &mut owner.circuits;
    cache.stats.queries += 1; cache.clock += 1;
    let stamp = cache.clock;
    let program = if let Some(entry) = cache.entries.get_mut(&key) {
        entry.stamp = stamp;
        match &mut entry.status {
            Status::Complete(program) => { cache.stats.hits += 1; program.clone() },
            Status::Pending(count) if *count < 2 => { *count += 1; return Ok(None); },
            Status::Pending(_) => {
                cache.stats.builds += 1;
                let program = compile(model, MAX_NODES, MAX_WORK, &mut cache.stats).map(Arc::new);
                if program.is_none() { cache.stats.budget_fallbacks += 1; }
                cache.store(model, key, Status::Complete(program.clone())); program
            },
        }
    } else {
        cache.store(model, key, Status::Pending(1)); return Ok(None);
    };
    let Some(program) = program else { return Ok(None); };
    let scorer = NoteScorer::new(power, model.input.play_level, model.input.notes.len() as u32)?;
    // A circuit visits some terms whose paths may later be dominated. Preserve
    // the exact numeric DP's error behavior by falling back on any error.
    match program.score(scorer) {
        Ok(value) => {
            cache.stats.evaluated_nodes += program.nodes.len() as u64;
            cache.stats.evaluated_terms += program.terms.len() as u64;
            Ok(Some((value, WindowStats { compact_value_queries: 1, ..WindowStats::default() })))
        },
        Err(_) => { cache.stats.arithmetic_fallbacks += 1; Ok(None) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::independent::joint::tests::{input, profiles};

    #[test]
    fn score_circuit_matches_fresh_native_dp_with_queue_corrections_and_float_boundaries() {
        let profiles = profiles();
        for (i, skill) in profiles.iter().enumerate() {
            for combo in [18, 699] {
                let mut request = input(&[0.2, 0.3, 0.215, 0.265, 0.5, 0.6],
                    vec![skill.clone(), profiles[(i + 4) % profiles.len()].clone()], 2);
                request.native_time_routing = None;
                request.initial_combo = combo;
                request.medley_combo = true;
                request.fever_frames = Some([26, 62]);
                request.constraints.push(crate::hit_timing::NoteConstraint {
                    before: 2, after: 3, minimum_gap_frames: 1, require_dispatch_order: true,
                    earliest_after: None,
                });
                let model = JointTimeline::new(&request).unwrap();
                let mut stats = CircuitStats::default();
                let circuit = compile(&model, MAX_NODES, MAX_WORK, &mut stats).expect("small exact circuit");
                for p in [0., 0.125, 37.5, 300040.75,
                    f32::from_bits(300040.75_f32.to_bits() + 1), 600000.] {
                    let power = ScoringPower::new(p).unwrap();
                    let expected = model.score_query::<false>(power).unwrap().0;
                    let scorer = NoteScorer::new(power, request.play_level, request.notes.len() as u32).unwrap();
                    assert_eq!(circuit.score(scorer).unwrap(), expected, "profile={i} combo={combo} power={p}");
                }
                for limit in [1, 2, 8] {
                    assert!(compile(&model, limit, MAX_WORK, &mut CircuitStats::default()).is_none());
                }
                assert!(compile(&model, MAX_NODES, 0, &mut CircuitStats::default()).is_none());
            }
        }
    }

    #[test]
    fn score_circuit_reuses_only_structure_and_separates_full_skill_keys_and_geometry() {
        let request = input(&[0.2, 0.7, 0.205, 0.31], vec![profiles()[0].clone(); 2], 1);
        let model = JointTimeline::new(&request).unwrap();
        for p in [0.125, 37.5, 300040.75, 600000., 37.5] {
            let power = ScoringPower::new(p).unwrap();
            let expected = model.score_query::<false>(power).unwrap().0;
            if let Some((actual, _)) = score(&model, power).unwrap() { assert_eq!(actual, expected); }
        }
        let stats = model.functions.lock().unwrap().circuits.stats();
        assert_eq!(stats.builds, 1);
        assert!(stats.evaluated_nodes > 0);
        assert!(stats.hits >= 2);
        let mut skills = request.skills.clone();
        skills[0].score = crate::hit_timing::ScoreEffect::Plain { percent: 120. };
        let rebound = model.with_skills(&skills).unwrap();
        for _ in 0..3 {
            let power = ScoringPower::new(300040.75).unwrap();
            if let Some((actual, _)) = score(&rebound, power).unwrap() {
                assert_eq!(actual, rebound.score_query::<false>(power).unwrap().0);
            }
        }
        let stats = model.functions.lock().unwrap().circuits.stats();
        assert_eq!(stats.entries, 2);
        assert!(stats.retained_bytes <= 8 * 1024 * 1024);
        // A fresh graph, even with identical chart contents, has its own proof.
        let fresh = JointTimeline::new(&request).unwrap();
        assert_ne!(Arc::as_ptr(&fresh.graph), Arc::as_ptr(&model.graph));
        assert!(score(&fresh, ScoringPower::new(37.5).unwrap()).unwrap().is_none());
        assert_eq!(fresh.functions.lock().unwrap().circuits.stats().entries, 1);
    }

    #[test]
    fn score_circuit_arithmetic_errors_return_to_the_original_dp() {
        let request = input(&[0.2, 0.7, 0.205, 0.31], vec![profiles()[0].clone(); 2], 1);
        let model = JointTimeline::new(&request).unwrap();
        for _ in 0..3 { let _ = score(&model, ScoringPower::new(37.5).unwrap()).unwrap(); }
        let power = ScoringPower::new(1e20).unwrap();
        assert!(score(&model, power).unwrap().is_none());
        assert!(model.score_query::<false>(power).is_err());
        assert_eq!(model.functions.lock().unwrap().circuits.stats().arithmetic_fallbacks, 1);
    }
}
