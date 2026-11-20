//! Exact power independent functions for certified independent graph intervals.
//! These retain operand multisets, not numeric scores at previous powers. Every
//! query evaluates native f32 arithmetic afresh. A compile budget only falls
//! back to the full numeric DP for that interval; it never limits correctness.
use super::*;
use crate::hit_timing::compiled::Term;
use crate::scoring::ScoringError;
use std::collections::{BTreeMap, HashSet};

const CACHE_BYTES: usize = 8 * 1024 * 1024;
const LIVE_BYTES: usize = 4 * 1024 * 1024;
const WORK: usize = 200_000;
const LABELS: usize = 64;

pub(super) fn enabled() -> bool {
    // Exact symbolic functions remain an opt-in experiment: the real-chart
    // cold-build gate did not improve latency with bounded Pareto labels.
    std::env::var("BANDORI_TIMING_DISABLE_COMPONENT_FUNCTIONS").as_deref() == Ok("0")
        && std::env::var("BANDORI_TIMING_DISABLE_VALUE_BLOCKS").as_deref() != Ok("1")
}

#[derive(Clone, Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionStats {
    pub circuits: circuit::CircuitStats,
    pub queries: u64,
    pub independent_intervals: u64,
    pub builds: u64,
    pub hits: u64,
    pub budget_fallbacks: u64,
    pub native_fallbacks: u64,
    pub compile_transitions: u64,
    pub compiled_variants: u64,
    pub native_term_evaluations: u64,
    pub variant_additions: u64,
    pub retained_bytes: usize,
    pub entries: usize,
    pub evictions: u64,
}

struct Function {
    terms: Vec<Term>,
    variants: Vec<Vec<(usize, u32)>>,
}
impl Function {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.terms.capacity() * std::mem::size_of::<Term>()
            + self.variants.capacity() * std::mem::size_of::<Vec<(usize,u32)>>()
            + self.variants.iter().map(|v| v.capacity() * std::mem::size_of::<(usize,u32)>()).sum::<usize>()
    }
    fn score(&self, scorer: NoteScorer, stats: &mut FunctionStats) -> Result<u64, ReplayError> {
        // Includes every reachable operand, even if its plan was later dominated
        // or infeasible. Native arithmetic errors must not be hidden by compile.
        let terms = self.terms.iter().map(|t| scorer.score_with_judgement_multiplier(
            f32::from_bits(t[0]), f32::from_bits(t[1]), f32::from_bits(t[2]), 1.)
            .map(|n| n.final_score)).collect::<Result<Vec<_>,_>>()?;
        stats.native_term_evaluations += terms.len() as u64;
        let mut best = 0;
        for variant in &self.variants {
            stats.variant_additions += variant.len() as u64;
            let mut total = 0u64;
            for &(term,count) in variant {
                total = total.checked_add(u64::from(terms[term]) * u64::from(count))
                    .ok_or(ScoringError::TotalScoreOverflow)?;
            }
            best = best.max(total);
        }
        Ok(best)
    }
}

struct Entry { function: Option<Arc<Function>>, bytes: usize, stamp: u64 }
#[derive(Default)]
pub(super) struct Cache {
    pub(super) circuits: circuit::Cache,
    entries: HashMap<Arc<Vec<u8>>, Entry>,
    bytes: usize,
    clock: u64,
    stats: FunctionStats,
}
impl Cache {
    pub(super) fn stats(&self) -> FunctionStats {
        let mut stats = self.stats.clone();
        stats.circuits = self.circuits.stats();
        stats.retained_bytes = self.bytes;
        stats.entries = self.entries.len();
        stats
    }
    fn store(&mut self, key: Arc<Vec<u8>>, function: Option<Arc<Function>>) {
        let bytes = key.capacity() + std::mem::size_of::<Entry>()
            + function.as_ref().map_or(0, |f| f.bytes());
        if bytes > CACHE_BYTES { return; }
        while self.bytes + bytes > CACHE_BYTES {
            let key = self.entries.iter().min_by_key(|(_,e)| e.stamp).unwrap().0.clone();
            self.bytes -= self.entries.remove(&key).unwrap().bytes;
            self.stats.evictions += 1;
        }
        self.bytes += bytes;
        self.entries.insert(key,Entry { function,bytes,stamp:self.clock });
    }
}

/// Splitting requires BOTH one geometry vertex and an idle FIFO for every
/// prefix plan before every possible suffix input. Trigger actions may span
/// several frame layers; no split can cut through that domain. All native
/// Combo ranks, fever and chart count remain the original global ones.
fn intervals(model: &JointTimeline) -> Result<Vec<(usize,usize)>, ReplayError> {
    let graph = &model.graph;
    let mut suffix_first = vec![u32::MAX; graph.layers.len() + 1];
    let mut last_trigger = vec![0; model.input.skills.len()];
    let mut masks = vec![0u64; graph.layers.len()];
    if model.input.skills.len() > 63 { return Ok(vec![]); }
    for (layer_index,layer) in graph.layers.iter().enumerate().rev() {
        let mut first = suffix_first[layer_index + 1];
        for edge in &layer.edges {
            if let Some(a) = edge.action {
                for &(note,frame) in graph.actions.get(a).hits.iter() {
                    first = first.min(frame);
                    if let Some(slot) = model.input.notes[note].trigger {
                        masks[layer_index] |= 1 << slot;
                        last_trigger[slot] = last_trigger[slot].max(layer_index + 1);
                    }
                }
            }
        }
        suffix_first[layer_index] = first;
    }
    let ranges = geometry::skill_ranges(&model.input, |i| {
        let frames = model.input.notes[i].frame_domain(Judgement::Bad)?;
        Ok((*frames.first().unwrap(),*frames.last().unwrap()))
    })?;
    let cooling = countdown(0.75) + 1;
    let ready: Vec<_> = ranges.iter().map(|&(slot,_,last)| (slot,last + cooling + 1)).collect();
    let (mut start,mut touched,mut previous) = (0,0u64,0u64);
    let mut result = vec![];
    for cut in 1..graph.layers.len() {
        touched |= masks[cut-1];
        if touched == previous || graph.layers[cut-1].count != 1 { continue; }
        if last_trigger.iter().enumerate().any(|(s,&last)| touched & (1 << s) != 0 && last > cut) {
            continue;
        }
        let latest = ready.iter().filter(|&&(s,_)| touched & (1 << s) != 0)
            .map(|&(_,r)| r).max().unwrap_or(0);
        if latest <= suffix_first[cut] {
            result.push((start,cut)); start = cut; previous = touched;
        }
    }
    result.push((start,graph.layers.len()));
    Ok(result)
}

// Exact serialization of the local transition graph. Full operands, trigger
// slots and relevant complete skill descriptors are included; no digest-only
// equality. Vertex/edge order is retained, independent of other intervals.
fn key(model: &JointTimeline, start: usize, end: usize) -> Vec<u8> {
    let mut result = vec![ (end-start) as u64 ];
    let mut actions = BTreeMap::new();
    let mut slots = HashSet::new();
    for layer in &model.graph.layers[start..end] {
        result.extend([layer.count as u64,layer.edges.len() as u64]);
        for edge in &layer.edges {
            let action = edge.action.map(|a| {
                let next = actions.len();
                *actions.entry(a).or_insert(next)
            });
            result.extend([edge.from as u64,edge.to as u64,action.map_or(0,|a| a as u64 + 1)]);
        }
    }
    let mut ordered: Vec<_> = actions.into_iter().collect();
    ordered.sort_unstable_by_key(|a| a.1);
    for (index,_) in ordered {
        let action = &model.graph.actions.get(index);
        result.extend([action.factor.rank as u64,u64::from(action.factor.fever),action.hits.len() as u64,
            u64::from(matches!(action.hits,geometry::ActionHits::Run(_)))]);
        for &(note,frame) in action.hits.iter() {
            let trigger = model.input.notes[note].trigger;
            if let Some(s) = trigger { slots.insert(s); }
            result.extend([note as u64,frame as u64,trigger.map_or(0,|s| s as u64 + 1)]);
        }
        let prepared = model.prepared_actions.actions[index];
        result.extend([prepared.raw as u64,
            model.input.combo_multiplier(model.input.initial_combo + action.factor.rank as u32).to_bits() as u64]);
    }
    // Touch edges operate inside a layer, ordinary edges on the previous one.
    let touches: Vec<_> = model.graph.touches.iter().filter(|t| start <= t.layer && t.layer < end).collect();
    result.push(touches.len() as u64);
    for touch in touches {
        result.extend([(touch.layer-start) as u64,touch.inputs.len() as u64]);
        result.extend(touch.inputs.iter().map(|&i| i as u64));
    }
    let mut slots: Vec<_> = slots.into_iter().collect(); slots.sort_unstable();
    result.push(slots.len() as u64);
    for slot in slots { result.push(slot as u64); result.extend(model.input.skills[slot].cache_key().map(u64::from)); }
    // Prefix-free unsigned varints preserve complete equality while avoiding
    // eight bytes for every small vertex/action index. They are not hashes.
    let mut bytes = Vec::new();
    for mut value in result {
        while value >= 128 { bytes.push((value as u8 & 127) | 128); value >>= 7; }
        bytes.push(value as u8);
    }
    bytes
}

struct Segment { start: usize, end: usize, key: Arc<Vec<u8>> }
pub(super) struct Plan { segments: Vec<Segment> }
pub(super) fn prepare(model: &JointTimeline) -> Result<Option<Plan>,ReplayError> {
    let intervals = intervals(model)?;
    if intervals.len() < 2 { return Ok(None); }
    let cache = model.functions.lock().unwrap();
    Ok(Some(Plan { segments: intervals.into_iter().map(|(start,end)| {
        let key = Arc::new(key(model,start,end));
        let key = cache.entries.get_key_value(&key).map_or(key.clone(),|(key,_)| key.clone());
        Segment { start,end,key }
    }).collect() }))
}

#[derive(Clone)]
struct Label { state: usize, terms: Rc<Vec<Term>> }
type Bank = Vec<Vec<Label>>;
fn dominates(a: &[Term], b: &[Term]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a,b)| (0..3).all(|i| a[i] >= b[i]))
}
fn insert(values: &mut Vec<Label>, value: Label, live: &mut usize) -> bool {
    if values.iter().any(|v| v.state == value.state && dominates(&v.terms,&value.terms)) { return true; }
    values.retain(|v| {
        let removed = v.state == value.state && dominates(&value.terms,&v.terms);
        if removed { *live -= 32 + v.terms.len() * 12; }
        !removed
    });
    *live += 32 + value.terms.len() * 12;
    values.push(value);
    values.len() <= LABELS && *live <= LIVE_BYTES
}
fn compile(model: &JointTimeline, start: usize, end: usize, stats: &mut FunctionStats) -> Option<Function> {
    let mut costs = JointCosts::new(model,ScoringPower::new(0.).ok()?).ok()?;
    let mut values: Bank = vec![vec![Label { state:0,terms:Rc::new(vec![]) }]];
    let mut seen = HashSet::new();
    let mut work = 0;
    let mut touch_index = model.graph.touches.partition_point(|t| t.layer < start);
    for layer_index in start..end {
        let layer = &model.graph.layers[layer_index];
        let mut next: Bank = vec![vec![];layer.count];
        let mut live = 0;
        let touch = model.graph.touches.get(touch_index).filter(|t| t.layer == layer_index);
        if touch.is_some() { touch_index += 1; }
        if let Some(touch) = touch {
            for (from,&to) in touch.inputs.iter().enumerate() {
                for value in &values[from] {
                    if !insert(&mut next[to],value.clone(),&mut live) { return None; }
                }
            }
        }
        for edge in &layer.edges {
            let source = if touch.is_some() { next[edge.from].clone() } else { values[edge.from].clone() };
            for mut label in source {
                work += 1; stats.compile_transitions += 1;
                if work > WORK { return None; }
                if let Some(action) = edge.action {
                    let Some(add) = costs.symbolic_terms(action,label.state) else { continue; };
                    if add.iter().flatten().any(|&v| !f32::from_bits(v).is_finite() || f32::from_bits(v) < 0.) {
                        return None;
                    }
                    seen.extend(add.iter().copied());
                    let Some((state,_)) = costs.action_at(action,&model.graph.actions.get(action),label.state).ok()?
                        else { continue; };
                    if costs.states.len() > 2048 || seen.len() > 2048 { return None; }
                    let mut terms = (*label.terms).clone(); terms.extend(add); terms.sort_unstable();
                    label = Label { state,terms:Rc::new(terms) };
                }
                if !insert(&mut next[edge.to],label,&mut live) { return None; }
            }
        }
        values = next;
    }
    let mut complete = vec![]; let mut live = 0;
    for mut label in values.into_iter().flatten() {
        // Every exit state is idle before the next interval's first input, or
        // terminal. Its numeric state ID carries no observable future effect.
        label.state = 0;
        if !insert(&mut complete,label,&mut live) { return None; }
    }
    if complete.is_empty() { return None; }
    let mut terms: Vec<_> = seen.into_iter().collect(); terms.sort_unstable();
    let ids: HashMap<_,_> = terms.iter().enumerate().map(|(i,&t)| (t,i)).collect();
    let variants = complete.into_iter().map(|label| {
        let mut counts = BTreeMap::new();
        for t in label.terms.iter() { *counts.entry(ids[t]).or_insert(0u32) += 1; }
        counts.into_iter().collect()
    }).collect();
    Some(Function {terms,variants})
}

pub(super) fn score(model: &JointTimeline, power: ScoringPower)
    -> Result<Option<(u64,WindowStats)>, SearchError> {
    let Some(plan) = &model.function_plan else { return Ok(None); };
    let scorer = NoteScorer::new(power,model.input.play_level,model.input.notes.len() as u32)?;
    let mut cache = model.functions.lock().unwrap();
    cache.stats.queries += 1;
    let mut total = 0u64; let mut stats = WindowStats::default();
    for Segment {start,end,key} in &plan.segments {
        let (start,end) = (*start,*end);
        cache.stats.independent_intervals += 1;
        cache.clock += 1; let clock = cache.clock;
        let function = if let Some(entry) = cache.entries.get_mut(key) {
            entry.stamp = clock;
            let function = entry.function.clone(); cache.stats.hits += 1; function
        } else {
            cache.stats.builds += 1;
            let function = compile(model,start,end,&mut cache.stats).map(Arc::new);
            if let Some(function) = &function { cache.stats.compiled_variants += function.variants.len() as u64; }
            else { cache.stats.budget_fallbacks += 1; }
            cache.store(Arc::clone(key),function.clone()); function
        };
        let value = if let Some(function) = function {
            function.score(scorer,&mut cache.stats)?
        } else {
            cache.stats.native_fallbacks += 1;
            let mut costs = JointCosts::new(model,power)?;
            let mut partial = WindowStats::default();
            let value = solve::blocks::evaluate_range(&model.graph,start,end,&mut costs,options(),&mut partial)?
                .unwrap().ok_or(ReplayError::Input("certified independent interval is infeasible"))?.score;
            costs.finish_stats(&mut partial); stats.accumulate(&partial); value
        };
        total = total.checked_add(value).ok_or(ScoringError::TotalScoreOverflow)?;
    }
    Ok(Some((total,stats)))
}

#[cfg(test)]
mod tests;
