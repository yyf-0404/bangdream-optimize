use super::*;
#[cfg(test)]
use crate::hit_timing::NoteWindow;
use crate::hit_timing::{
    routing::{candidate_is_nearer, event_ranks}, JudgementCorrection, ScoreEffect,
};
use crate::scoring::Judgement;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

mod component_cache;
mod hits;
pub(super) mod storage;
mod outside;
mod sections;
mod temporal;
mod envelope;
pub(super) mod incremental;
pub(super) use temporal::TemporalBlock;
pub(super) use outside::OutsideCache;
mod touch_order;
use component_cache::{ComponentCache, ComponentKey};
pub(super) use hits::ActionHits;

/// A necessary (not sufficient) active interval for every correction skill.
/// Request endpoints include all raw Bad frames, so they over-cover every valid
/// plan. FIFO start = max(request, previous start + playing + cooling + 1);
/// monotonicity propagates endpoint bounds through any length of skill queue.
pub(super) fn correction_ranges(
    input: &ReplayInput,
) -> Result<Vec<(Judgement, u32, u32)>, ReplayError> {
    let ranges = skill_ranges(input, |index| {
        let frames = input.notes[index].frame_domain(Judgement::Bad)?;
        Ok((*frames.first().ok_or(ReplayError::Input("empty correction trigger domain"))?,
            *frames.last().unwrap()))
    })?;
    Ok(corrections_from_ranges(input, &ranges))
}

pub(super) fn corrections_from_ranges(
    input: &ReplayInput,
    ranges: &[(usize, u32, u32)],
) -> Vec<(Judgement, u32, u32)> {
    ranges.iter().filter_map(|&(slot, first, last)| {
        input.skills[slot].correction.and_then(|c| match c {
            JudgementCorrection::Good => Some((Judgement::Good, first, last)),
            JudgementCorrection::Bad => Some((Judgement::Bad, first, last)),
            _ => None,
        })
    }).collect()
}

/// Inclusive FIFO playing envelopes, using exact raw Bad request endpoints.
pub(super) fn skill_ranges(
    input: &ReplayInput,
    mut bounds: impl FnMut(usize) -> Result<(u32, u32), ReplayError>,
) -> Result<Vec<(usize, u32, u32)>, ReplayError> {
    use crate::hit_timing::bounds::countdown;
    let mut triggers: Vec<_> = input
        .notes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| n.trigger.map(|slot| (slot, i)))
        .collect();
    triggers.sort_unstable();
    let cooling = countdown(0.75) + 1;
    let (mut ready_first, mut ready_last) = (0u32, 0u32);
    let mut ranges = vec![];
    for (slot, index) in triggers {
        let (first, last) = bounds(index)?;
        let first = first.max(ready_first);
        let last = last.max(ready_last);
        let skill = &input.skills[slot];
        let playing = countdown(skill.duration) + 1;
        ranges.push((slot, first + 1, last + playing));
        ready_first = first + playing + cooling + 1;
        ready_last = last + playing + cooling + 1;
    }
    Ok(ranges)
}

#[derive(Clone)]
struct Interval {
    first: u32,
    last: u32,
    raw: Judgement,
    fever: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ActionInterval {
    pub first: u32,
    pub last: u32,
    pub perfect: bool,
    pub inside: bool,
    /// Zero outside skills; 1..=6 identify the actual active skill slot.
    pub slot: u8,
    pub fever: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Factor {
    pub perfect: bool,
    pub inside: bool,
    /// Zero outside skills; 1..=6 identify the actual active skill slot.
    pub slot: u8,
    pub fever: bool,
    /// Fixed per-note reference; a collapsed action carries the whole run sum.
    pub reference: f64,
    /// Number of settled notes including this hit, excluding the carried Combo.
    pub rank: usize,
}

#[derive(Clone)]
pub(super) struct Action {
    pub factor: Factor,
    /// A single boundary hit, or a deterministic all-Perfect run in actual order.
    pub hits: ActionHits,
}
#[derive(Clone, Copy)]
pub(super) struct Edge {
    pub from: usize,
    pub to: usize,
    pub action: Option<usize>,
}
pub(super) struct Layer {
    pub count: usize,
    pub edges: storage::Edges,
}
pub(super) struct TouchDag {
    pub layer: usize,
    /// Previous-frontier node -> DAG vertex. Every vertex is also a legal exit.
    pub inputs: Vec<usize>,
    pub incremental: Option<Box<incremental::Proof>>,
}
pub(super) struct Graph {
    pub layers: Vec<Layer>,
    pub actions: storage::Actions,
    pub note_count: usize,
    pub edge_count: usize,
    pub temporal: Vec<temporal::TemporalBlock>,
    pub touches: Vec<TouchDag>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Relation {
    before: usize,
    after: usize,
    gap: u32,
    earliest_after: Option<std::sync::Arc<crate::hit_timing::ArrivalCurve>>,
    /// Skill triggers must also follow their actual same-frame dispatch order.
    strict_event_order: bool,
}

impl Relation {
    fn allows_frames(&self, before: u32, after: u32) -> bool {
        crate::hit_timing::constraints::required_after(
            self.gap,
            self.earliest_after.as_deref(),
            before,
        )
        .is_some_and(|f| after >= f)
    }
}

#[derive(Clone)]
pub(super) struct Row {
    pub domains: Vec<Vec<ActionInterval>>,
    pub envelope: Vec<Vec<Factor>>,
    spans: Vec<(u32, u32)>,
    relations: Vec<Relation>,
    incident: Vec<Vec<usize>>,
    references: Vec<f64>,
}

#[derive(Default)]
pub(super) struct RowRules<'a> {
    pub references: Option<&'a [f64]>,
    pub outside_correction: u32,
    /// A delayed skill can start after its trigger was hit.
    pub trigger_before: bool,
}

pub(super) struct Prepared {
    pub input: ReplayInput,
    intervals: Vec<Vec<Interval>>,
    ranks: Vec<usize>,
    relations: Vec<Relation>,
    roots: [Vec<usize>; 7],
    heads: Vec<Option<usize>>,
    ends: Vec<Option<usize>>,
    connection_slots: Vec<usize>,
    binding_slots: usize,
    blocker_cache: HashMap<(usize, u32), Vec<usize>>,
    component_cache: Mutex<ComponentCache>,
    envelope_cache: Mutex<envelope::Cache>,
    pub frame_count: usize,
    pub interval_count: usize,
    pub reference_coefficient: Option<f64>,
}

impl Prepared {
    /// Retained chart vector payload, excluding input snapshots, shared arrival
    /// curves, HashMap buckets and allocator metadata.
    pub(super) fn chart_payload_bytes(&self) -> usize {
        self.intervals.capacity() * std::mem::size_of::<Vec<Interval>>()
            + self.intervals.iter().map(|v| v.capacity() * std::mem::size_of::<Interval>()).sum::<usize>()
            + self.relations.capacity() * std::mem::size_of::<Relation>()
            + (self.ranks.capacity() + self.connection_slots.capacity()
                + self.roots.iter().map(Vec::capacity).sum::<usize>()) * std::mem::size_of::<usize>()
            + (self.heads.capacity() + self.ends.capacity()) * std::mem::size_of::<Option<usize>>()
            + self.blocker_cache.values().map(|v| v.capacity() * std::mem::size_of::<usize>()).sum::<usize>()
            + self.envelope_cache.lock().unwrap().bytes()
    }

    pub fn new(input: &ReplayInput) -> Result<Self, ReplayError> {
        let mut widened = input.clone();
        widened.skills = vec![
            crate::hit_timing::ReplaySkill {
                duration: 1.,
                score: ScoreEffect::Plain { percent: 0. },
                correction: Some(JudgementCorrection::Bad),
            };
            input.skills.len().max(1)
        ];
        let domains = crate::hit_timing::search::frame_domains(&widened)?;
        let mut frame_count = 0;
        let mut intervals = Vec::with_capacity(domains.len());
        for (i, mut frames) in domains.into_iter().enumerate() {
            frames.sort_unstable();
            let mut note: Vec<Interval> = vec![];
            for frame in frames {
                let raw = input.notes[i].raw_judgement(frame)?;
                let fever = input
                    .fever_frames
                    .is_some_and(|[a, b]| a <= frame && frame < b);
                frame_count += 1;
                if let Some(last) = note
                    .last_mut()
                    .filter(|v| v.last + 1 == frame && v.raw == raw && v.fever == fever)
                {
                    last.last = frame;
                } else {
                    note.push(Interval {
                        first: frame,
                        last: frame,
                        raw,
                        fever,
                    });
                }
            }
            intervals.push(note);
        }
        let mut relations: Vec<_> = input
            .constraints
            .iter()
            .map(|c| Relation {
                before: c.before,
                after: c.after,
                gap: c.minimum_gap_frames,
                earliest_after: c.earliest_after.clone().map(std::sync::Arc::new),
                strict_event_order: c.require_dispatch_order,
            })
            .collect();
        let mut triggers: Vec<_> = input
            .notes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| n.trigger.map(|slot| (slot, i)))
            .collect();
        triggers.sort_unstable();
        for pair in triggers.windows(2) {
            relations.push(Relation {
                before: pair[0].1,
                after: pair[1].1,
                gap: 0,
                earliest_after: None,
                strict_event_order: true,
            });
        }
        let mut roots: [Vec<usize>; 7] = std::array::from_fn(|_| vec![]);
        let mut heads = vec![None; input.notes.len()];
        let mut ends = heads.clone();
        let mut connection_slots = vec![];
        let mut slots = HashMap::new();
        if let Some(r) = &input.native_time_routing {
            for &i in &r.candidate_order {
                roots[r.targets[i].as_ref().unwrap().button as usize].push(i);
            }
            for (i, c) in r.connections.iter().enumerate() {
                heads[c.head] = Some(i);
                ends[c.end] = Some(i);
                let key = (r.targets[c.head].as_ref().unwrap().button, c.finger_id);
                let count = slots.len();
                connection_slots.push(*slots.entry(key).or_insert(count));
            }
        }
        let mut reference = input.clone();
        reference.skills.clear();
        for n in &mut reference.notes {
            n.trigger = None;
        }
        let reference_coefficient = crate::hit_timing::replay(&reference)
            .ok()
            .map(|r| super::machine::coefficient(input, &r));
        let mut result = Self {
            interval_count: intervals.iter().map(Vec::len).sum(),
            input: input.clone(),
            envelope_cache: Mutex::new(envelope::Cache::new(&intervals)),
            intervals,
            ranks: event_ranks(input),
            relations,
            roots,
            heads,
            ends,
            connection_slots,
            binding_slots: slots.len(),
            frame_count,
            blocker_cache: HashMap::new(),
            component_cache: Mutex::new(ComponentCache::default()),
            reference_coefficient,
        };
        if result.input.native_time_routing.is_some() {
            for i in 0..result.intervals.len() {
                for interval in result.intervals[i].clone() {
                    for f in interval.first..=interval.last {
                        let blockers = result.compute_blockers(i, f);
                        if !blockers.is_empty() {
                            result.blocker_cache.insert((i, f), blockers);
                        }
                    }
                }
            }
        }
        Ok(result)
    }
    pub fn requests(&self, trigger: usize) -> Vec<u32> {
        // The trigger cannot correct itself using the skill it is about to start.
        self.correctable_requests(trigger, 0)
    }
    pub fn correctable_requests(&self, trigger: usize, correction: u32) -> Vec<u32> {
        let threshold = match correction {
            2 => Judgement::Good,
            3 => Judgement::Bad,
            _ => Judgement::Great,
        };
        self.intervals[trigger]
            .iter()
            .filter(|i| i.raw >= threshold)
            .flat_map(|i| i.first..=i.last)
            .collect()
    }
    pub fn isolated(&self, trigger: usize, skill: &ReplaySkill) -> ReplayInput {
        let mut input = self.input.clone();
        for (i, note) in input.notes.iter_mut().enumerate() {
            note.trigger = (i == trigger).then_some(0);
        }
        input.skills = vec![skill.clone()];
        input
    }

    pub fn row(&self, trigger: usize, request: u32, playing: u32, correction: u32) -> Option<Row> {
        self.row_with_rules(trigger, request, playing, correction, RowRules::default())
    }
    pub fn row_with_rules(
        &self,
        trigger: usize,
        request: u32,
        playing: u32,
        correction: u32,
        rules: RowRules<'_>,
    ) -> Option<Row> {
        let mut row = self.row_without_envelope(trigger, request, playing, correction, rules)?;
        row.envelope = self.envelope(&row);
        Some(row)
    }

    /// Keep the complete geometry while deferring the fallback-only upper bound.
    pub(super) fn row_without_envelope(
        &self,
        trigger: usize,
        request: u32,
        playing: u32,
        correction: u32,
        rules: RowRules<'_>,
    ) -> Option<Row> {
        let references = rules
            .references
            .map_or_else(|| vec![0.; self.input.notes.len()], |r| r.to_vec());
        let mut domains: Vec<Vec<ActionInterval>> = vec![];
        for i in 0..self.intervals.len() {
            let mut choices: Vec<ActionInterval> = vec![];
            self.row_choices(i, trigger, request, playing, correction,
                rules.outside_correction, rules.trigger_before, &mut choices);
            if choices.is_empty() { return None; }
            domains.push(choices);
        }
        self.finish_row(domains, references)
    }

    fn row_choices(&self, i: usize, trigger: usize, request: u32, playing: u32,
        correction: u32, outside_correction: u32, trigger_before: bool,
        choices: &mut Vec<ActionInterval>) {
            choices.clear();
            for interval in &self.intervals[i] {
                // Split at both ends without collapsing the remaining legal frames.
                for (first, last, inside) in [
                    (interval.first, interval.last.min(request), false),
                    (
                        interval.first.max(request + 1),
                        interval.last.min(request + playing),
                        true,
                    ),
                    (
                        interval.first.max(request + playing + 1),
                        interval.last,
                        false,
                    ),
                ] {
                    if first > last {
                        continue;
                    }
                    let correction = if inside {
                        correction
                    } else {
                        outside_correction
                    };
                    let corrected = if correction > 0 {
                        let threshold = match correction {
                            1 => Judgement::Great,
                            2 => Judgement::Good,
                            _ => Judgement::Bad,
                        };
                        if interval.raw >= threshold {
                            Judgement::Perfect
                        } else {
                            interval.raw
                        }
                    } else {
                        interval.raw
                    };
                    if corrected < Judgement::Great {
                        continue;
                    }
                    let (mut first, mut last) = (first, last);
                    if i == trigger {
                        if !trigger_before {
                            first = first.max(request);
                        }
                        last = last.min(request);
                    }
                    if let (Some(r), Some(end)) = (&self.input.native_time_routing, self.ends[i]) {
                        let c = &r.connections[end];
                        if !c.release_inside {
                            continue;
                        }
                        let head = r.targets[c.head].as_ref().unwrap();
                        first = first.max(head.active_from_frame);
                        last = last.min(head.active_until_frame);
                    }
                    if first > last {
                        continue;
                    }
                    let value = ActionInterval {
                        first,
                        last,
                        perfect: corrected == Judgement::Perfect,
                        inside,
                        slot: u8::from(inside),
                        fever: interval.fever,
                    };
                    if let Some(previous) = choices.last_mut().filter(|p| {
                        p.last + 1 == first
                            && p.perfect == value.perfect
                            && p.inside == inside
                            && p.fever == value.fever
                    }) {
                        previous.last = last;
                    } else {
                        choices.push(value);
                    }
                }
            }
            choices.sort_unstable_by_key(|i| i.first);
    }

    /// The exact same per-note NET envelope as row_with_rules, without any
    /// graph-only relations, incident lists, or outside factors. All notes
    /// still determine optimistic Combo ranks. Only zero contributions are
    /// omitted; the native coefficient error continues to use the full chart.
    pub(super) fn net_envelope_only(&self, trigger: usize, request: u32,
        playing: u32, correction: u32, rules: RowRules<'_>) -> Option<Vec<Vec<Factor>>> {
        if std::env::var("BANDORI_TIMING_DISABLE_SPARSE_NET_ENVELOPES").as_deref()!=Ok("1") {
            return self.sparse_net_envelope(trigger,request,playing,correction,rules);
        }
        self.net_envelope_legacy(trigger,request,playing,correction,rules)
    }
    fn net_envelope_legacy(&self, trigger: usize, request: u32,
        playing: u32, correction: u32, rules: RowRules<'_>) -> Option<Vec<Vec<Factor>>> {
        let mut choices = Vec::new();
        let mut starts = Vec::with_capacity(self.intervals.len());
        let mut envelope = Vec::new();
        for i in 0..self.intervals.len() {
            self.row_choices(i, trigger, request, playing, correction,
                rules.outside_correction, rules.trigger_before, &mut choices);
            starts.push(choices.first()?.first);
            let inside: Vec<_> = choices.iter().filter(|c| c.inside).map(|c| Factor {
                perfect: c.perfect, inside: true, slot: c.slot, fever: c.fever,
                reference: rules.references.map_or(0., |r| r[i]),
                rank: c.last as usize,
            }).collect();
            if !inside.is_empty() { envelope.push(inside); }
        }
        starts.sort_unstable();
        for factor in envelope.iter_mut().flatten() {
            factor.rank = starts.partition_point(|&first| first <= factor.rank as u32);
        }
        Some(envelope)
    }

    /// Every skill trigger is fixed; ordinary notes retain their full legal
    /// domains. Labels include the actual active slot so equal grades on either
    /// side of a skill change never merge into one run or cache entry.
    pub(super) fn timeline_row(
        &self,
        timeline: &crate::hit_timing::clock::Timeline,
        frames: &[u32],
        movable: &[bool],
    ) -> Option<Row> {
        let mut domains = Vec::with_capacity(self.intervals.len());
        for (i, intervals) in self.intervals.iter().enumerate() {
            let mut choices: Vec<ActionInterval> = vec![];
            for interval in intervals {
                let (mut first, mut last) = (interval.first, interval.last);
                if self.input.notes[i].trigger.is_some() || !movable[i] {
                    first = first.max(frames[i]);
                    last = last.min(frames[i]);
                }
                if let (Some(r), Some(end)) = (&self.input.native_time_routing, self.ends[i]) {
                    let connection = &r.connections[end];
                    if !connection.release_inside {
                        continue;
                    }
                    let head = r.targets[connection.head].as_ref().unwrap();
                    first = first.max(head.active_from_frame);
                    last = last.min(head.active_until_frame);
                }
                for frame in first..=last {
                    let slot = timeline.slot(frame);
                    let slot_tag = slot.map_or(0, |s| s as u8 + 1);
                    let correction = slot.and_then(|s| self.input.skills[s].correction);
                    let threshold = correction.map(|c| match c {
                        JudgementCorrection::Great => Judgement::Great,
                        JudgementCorrection::Good => Judgement::Good,
                        JudgementCorrection::Bad => Judgement::Bad,
                    });
                    let grade = if threshold.is_some_and(|g| interval.raw >= g) {
                        Judgement::Perfect
                    } else {
                        interval.raw
                    };
                    if grade < Judgement::Great {
                        continue;
                    }
                    let perfect = grade == Judgement::Perfect;
                    if let Some(previous) = choices.last_mut().filter(|p| {
                        p.last + 1 == frame
                            && p.slot == slot_tag
                            && p.perfect == perfect
                            && p.fever == interval.fever
                    }) {
                        previous.last = frame;
                    } else {
                        choices.push(ActionInterval {
                            first: frame,
                            last: frame,
                            perfect,
                            inside: slot.is_some(),
                            slot: slot_tag,
                            fever: interval.fever,
                        });
                    }
                }
            }
            if choices.is_empty() {
                return None;
            }
            domains.push(choices);
        }
        self.finish_row(domains, vec![0.; self.input.notes.len()])
    }

    /// Geometry for joint trigger/ordinary placement. Grades are checked again
    /// under the dynamically active skill when evaluating an actual hit.
    pub(super) fn joint_row(&self) -> Row {
        let corrections = correction_ranges(&self.input).expect("validated replay input");
        self.joint_row_with_ranges(&corrections)
    }
    pub(super) fn joint_row_with_ranges(&self, corrections: &[(Judgement, u32, u32)]) -> Row {
        let domains = self
            .intervals
            .iter()
            .enumerate()
            .map(|(i, intervals)| {
                let mut domains = vec![];
                for interval in intervals {
                    let mut allowed = if interval.raw >= Judgement::Great {
                        vec![(interval.first, interval.last)]
                    } else {
                        corrections
                            .iter()
                            .filter(|(minimum, _, _)| interval.raw >= *minimum)
                            .filter_map(|&(_, a, b)| {
                                let first = interval.first.max(a);
                                let last = interval.last.min(b);
                                (first <= last).then_some((first, last))
                            })
                            .collect::<Vec<_>>()
                    };
                    allowed.sort_unstable();
                    let mut merged: Vec<(u32, u32)> = vec![];
                    for (a, b) in allowed {
                        if let Some(last) = merged
                            .last_mut()
                            .filter(|last| a <= last.1.saturating_add(1))
                        {
                            last.1 = last.1.max(b);
                        } else {
                            merged.push((a, b));
                        }
                    }
                    for (mut first, mut last) in merged {
                        if let (Some(r), Some(end)) =
                            (&self.input.native_time_routing, self.ends[i])
                        {
                            let c = &r.connections[end];
                            if !c.release_inside {
                                continue;
                            }
                            let head = r.targets[c.head].as_ref().unwrap();
                            first = first.max(head.active_from_frame);
                            last = last.min(head.active_until_frame);
                        }
                        if first <= last {
                            domains.push(ActionInterval {
                                first,
                                last,
                                perfect: interval.raw == Judgement::Perfect,
                                inside: false,
                                slot: 0,
                                fever: interval.fever,
                            });
                        }
                    }
                }
                domains
            })
            .collect();
        // Every valid low-judgment hit has an actually active repairing skill,
        // whose activation is covered above. No feasible plan is removed.
        self.finish_row(domains, vec![0.; self.input.notes.len()])
            .unwrap()
    }

    fn finish_row(&self, domains: Vec<Vec<ActionInterval>>, references: Vec<f64>) -> Option<Row> {
        let spans: Vec<_> = domains
            .iter()
            .map(|d| (d.first().unwrap().first, d.last().unwrap().last))
            .collect();
        let relations: Vec<_> = self
            .relations
            .iter()
            .filter(|r| {
                let a = spans[r.before].1;
                let b = spans[r.after].0;
                // Already true for EVERY remaining frame pair: no timestamp state.
                // A dynamic curve may contain holes/nonmonotone values: checking
                // only the latest predecessor is not a proof of redundancy.
                r.earliest_after.is_some()
                    || !(r.allows_frames(a, b) && (!r.strict_event_order || a < b))
            })
            .cloned()
            .collect();
        let mut incident = vec![vec![]; domains.len()];
        for (i, r) in relations.iter().enumerate() {
            incident[r.before].push(i);
            incident[r.after].push(i);
        }
        Some(Row {
            domains,
            spans,
            relations,
            incident,
            envelope: vec![],
            references,
        })
    }

    /// Restrict decisions only. Candidate lifetimes and blocker identities stay
    /// unchanged: a note forbidden to hit now can still intercept another hit.
    pub(super) fn restrict_row(&self, row: Row, limits: &[Option<[u32; 2]>])
        -> Result<Option<Row>, ReplayError> {
        if limits.len() != row.domains.len() {
            return Err(ReplayError::Input("frame limits must match the note count"));
        }
        let mut domains = row.domains;
        for (index, (choices, limit)) in domains.iter_mut().zip(limits).enumerate() {
            if let Some([first, last]) = *limit {
                if first > last || last > crate::hit_timing::MAX_SECONDS * crate::hit_timing::REPLAY_FPS {
                    return Err(ReplayError::Note { index, reason: "invalid inclusive frame limits" });
                }
                choices.retain_mut(|domain| {
                    domain.first = domain.first.max(first);
                    domain.last = domain.last.min(last);
                    domain.first <= domain.last
                });
            }
            if choices.is_empty() { return Ok(None); }
        }
        Ok(self.finish_row(domains, row.references))
    }

    pub(super) fn envelope(&self, row: &Row) -> Vec<Vec<Factor>> {
        let mut starts: Vec<_> = row.spans.iter().map(|s| s.0).collect();
        starts.sort_unstable();
        row.domains
            .iter()
            .enumerate()
            .map(|(note, d)| {
                d.iter()
                    .map(|i| Factor {
                        perfect: i.perfect,
                        inside: i.inside,
                        slot: i.slot,
                        fever: i.fever,
                        reference: row.references[note],
                        rank: starts.partition_point(|&f| f <= i.last),
                    })
                    .collect()
            })
            .collect()
    }

    /// Same-lane nearest-root lookup compiled into "these roots must be consumed".
    /// Uses the declared distance mode and preserves supplied list ties.
    fn compute_blockers(&self, note: usize, frame: u32) -> Vec<usize> {
        let Some(r) = &self.input.native_time_routing else {
            return vec![];
        };
        let Some(target) = &r.targets[note] else {
            return vec![];
        };
        if self.ends[note].is_some() {
            return vec![];
        }
        let mut before = true;
        self.roots[target.button as usize]
            .iter()
            .copied()
            .filter(|&i| {
                if i == note {
                    before = false;
                    return false;
                }
                let c = r.targets[i].as_ref().unwrap();
                if frame < c.active_from_frame || frame > c.active_until_frame {
                    return false;
                }
                if before {
                    !candidate_is_nearer(&self.input, i, note, frame)
                } else {
                    candidate_is_nearer(&self.input, note, i, frame)
                }
            })
            .collect()
    }
    fn blockers(&self, note: usize, frame: u32) -> &[usize] {
        self.blocker_cache
            .get(&(note, frame))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn perfect_run(&self, row: &Row, notes: &[usize], settled: &[bool]) -> Option<Action> {
        let first = row.domains[notes[0]][0];
        let mut hits = vec![];
        for &i in notes {
            if self.input.notes[i].trigger.is_some()
                || self.heads[i].is_some()
                || self.ends[i].is_some()
                || row.domains[i].iter().any(|d| {
                    d.inside != first.inside || d.slot != first.slot || d.fever != first.fever
                })
            {
                return None;
            }
            let domain = row.domains[i].iter().filter(|d| d.perfect);
            let frame = domain
                .clone()
                .find(|d| d.first <= self.input.hit_frames[i] && self.input.hit_frames[i] <= d.last)
                .map(|_| self.input.hit_frames[i])
                .or_else(|| domain.map(|d| d.first).next())?;
            hits.push((i, frame));
        }
        hits.sort_unstable_by_key(|&(i, f)| (f, self.ranks[i]));
        // A relation fully contained in this component cannot affect a later
        // component once all of its endpoints have settled. Retain one legal
        // all-Perfect schedule when its endpoints satisfy every relation.
        let chosen: HashMap<_, _> = hits.iter().copied().collect();
        for &i in notes {
            for &index in &row.incident[i] {
                let relation = &row.relations[index];
                let (&before, &after) = (
                    chosen.get(&relation.before)?,
                    chosen.get(&relation.after)?,
                );
                if !relation.allows_frames(before, after)
                    || (relation.strict_event_order
                        && (before, self.ranks[relation.before])
                            >= (after, self.ranks[relation.after]))
                {
                    return None;
                }
            }
        }
        let mut local = vec![];
        for &(i, f) in &hits {
            if self
                .blockers(i, f)
                .iter()
                .any(|j| !settled[*j] && !local.contains(j))
            {
                return None;
            }
            local.push(i);
        }
        Some(Action {
            factor: Factor {
                perfect: true,
                inside: first.inside,
                slot: first.slot,
                fever: first.fever,
                // All notes stay on the same side of the window. Subtracting
                // their fixed sum does not change all-Perfect run dominance.
                reference: notes.iter().map(|&i| row.references[i]).sum(),
                rank: 0,
            },
            hits: hits.into(),
        })
    }

    pub fn compile(
        &self,
        row: &Row,
        collapse: bool,
        options: WindowOptions,
        stats: &mut WindowStats,
    ) -> Option<Graph> {
        self.compile_with_runs(row, collapse, None, None, options, stats)
    }

    pub(super) fn compile_local(
        &self,
        row: &Row,
        protected: &[bool],
        options: WindowOptions,
        stats: &mut WindowStats,
    ) -> Option<Graph> {
        self.compile_with_runs(row, true, Some(protected), None, options, stats)
    }

    pub(super) fn compile_local_cached(
        &self,
        row: &Row,
        protected: &[bool],
        cache: &mut OutsideCache,
        options: WindowOptions,
        stats: &mut WindowStats,
    ) -> Option<Graph> {
        self.compile_with_runs(row, true, Some(protected), Some(cache), options, stats)
    }

    fn compile_with_runs(
        &self,
        row: &Row,
        collapse: bool,
        protected: Option<&[bool]>,
        mut outside_cache: Option<&mut OutsideCache>,
        options: WindowOptions,
        stats: &mut WindowStats,
    ) -> Option<Graph> {
        let pinned = protected.filter(|_| collapse
            && std::env::var("BANDORI_TIMING_DISABLE_OUTSIDE_NOTE_PINNING").as_deref() != Ok("1"))
            .and_then(|mask| self.pin_mixed_outside_notes(row, mask));
        if let Some((count, _)) = &pinned { stats.outside_pinned_notes += count; }
        let row = pinned.as_ref().map_or(row, |(_, row)| row);
        stats.graph_reuse_builds += usize::from(options.reuse_graph_storage);
        stats.inline_event_builds += usize::from(options.inline_event_hits);
        stats.component_cache_enabled_builds += usize::from(options.max_cached_component_edges > 0);
        let n = self.input.notes.len();
        let mut graph = Graph {
            layers: vec![],
            actions: vec![].into(),
            note_count: n,
            edge_count: 0,
            temporal: vec![],
            touches: vec![],
        };
        let mut keys = vec![Key {
            marks: vec![],
            bindings: vec![0; self.binding_slots],
        }];
        let mut active = vec![];
        let mut settled = vec![false; n];
        let mut expired = 0;
        let mut frame_templates = touch_order::template::Cache::new(options.frame_templates
            && std::env::var("BANDORI_TIMING_DISABLE_NATIVE_FRAME_TEMPLATES").as_deref() == Ok("0"));
        // Within one (note, frame) event only the Combo rank varies by state.
        // Retain this small lookup's capacity across events, not their actions.
        let mut event_actions: Vec<(usize, usize)> = vec![];
        let mut lives: Vec<_> = row.spans.iter().map(|s| s.1).collect();
        for c in &row.relations {
            // Retain timestamps only until the OTHER note's last possible input.
            lives[c.before] = lives[c.before].max(row.spans[c.after].1);
            lives[c.after] = lives[c.after].max(row.spans[c.before].1);
        }
        // Span components choose runs, not exponential component permutations.
        // Ambiguous components below still use a narrow event frontier.
        let mut sorted: Vec<_> = (0..n).collect();
        sorted.sort_unstable_by_key(|&i| (row.spans[i].0, self.ranks[i]));
        let mut components: Vec<Vec<usize>> = vec![];
        let mut end = 0;
        for i in sorted {
            if components.is_empty() || row.spans[i].0 > end {
                components.push(vec![]);
                end = row.spans[i].1;
            } else {
                end = end.max(row.spans[i].1);
            }
            components.last_mut().unwrap().push(i);
        }
        if let Some(protected) = protected {
            components = self.outside_components(row, components, protected);
        }
        for component in components {
            if collapse {
                let run = if let Some(protected) = protected {
                    component.iter().all(|&i| !protected[i]).then(|| {
                        if let Some(cache) = outside_cache.as_deref_mut() {
                            cache.run(self, row, &component, &settled, &keys)
                        } else {
                            self.outside_run(row, &component, &settled, &keys)
                        }
                    }).flatten()
                } else {
                    self.perfect_run(row, &component, &settled)
                };
                if let Some(mut run) = run {
                    run.factor.rank = expired + active.len() + 1;
                    stats.collapsed_notes += component.len();
                    // Adjacent certain components become one transfer/prefix query.
                    let previous = graph
                        .layers
                        .last()
                        .and_then(|l| {
                            let a = l.edges.first()?.action?;
                            l.edges
                                .iter()
                                .all(|e| e.from == e.to && e.action == Some(a))
                                .then_some(a)
                        })
                        .filter(|&a| {
                            let p = &graph.actions.get(a);
                            (protected.is_none() || matches!(p.hits, ActionHits::Run(_)))
                                && p.factor.perfect
                                && p.factor.inside == run.factor.inside
                                && p.factor.slot == run.factor.slot
                                && p.factor.fever == run.factor.fever
                                && p.factor.rank + p.hits.len() == run.factor.rank
                        });
                    if let Some(a) = previous {
                        graph.actions.wide_mut()[a].factor.reference += run.factor.reference;
                        graph.actions.wide_mut()[a].hits.extend_from_slice(&run.hits);
                    } else {
                        let index = graph.actions.len();
                        graph.actions.push(run);
                        stats.graph_actions += 1;
                        let edges: Vec<_> = (0..keys.len())
                            .map(|i| Edge {
                                from: i,
                                to: i,
                                action: Some(index),
                            })
                            .collect();
                        if !push(&mut graph, keys.len(), edges, options, stats) {
                            return None;
                        }
                    }
                    for i in component {
                        settled[i] = true;
                        expired += 1;
                    }
                    continue;
                }
            }
            let component_key = (options.max_cached_component_edges > 0
                && active.is_empty()
                && keys.len() == 1
                && component.iter().all(|&i| {
                    row.incident[i].is_empty() && self.heads[i].is_none() && self.ends[i].is_none()
                }))
            .then(|| {
                ComponentKey::new(self, row, &component, &keys[0], &settled, expired, options)
            });
            if let Some(key) = &component_key {
                stats.component_cache_queries += 1;
                let cached = self.component_cache.lock().unwrap().get(key);
                if let Some(cached) = cached {
                    stats.component_cache_hits += 1;
                    if !cached.append(&mut graph, options, stats) {
                        return None;
                    }
                    for &i in &component {
                        settled[i] = true;
                        expired += 1;
                    }
                    // This component has no binding writes or retained timestamps.
                    // Its sole exit state is exactly the entry binding state.
                    continue;
                }
            }
            let component_start = (graph.layers.len(), graph.actions.len(), graph.edge_count);
            let mut frames: BTreeMap<u32, Vec<(usize, ActionInterval)>> = BTreeMap::new();
            for &i in &component {
                for &d in &row.domains[i] {
                    for frame in d.first..=d.last {
                        frames.entry(frame).or_default().push((i, d));
                    }
                }
            }
            let mut temporal = temporal::Tracker::new(self, row, &frames, options);
            let mut incremental = incremental::Tracker::default();
            for (frame, mut events) in frames {
                temporal.begin(frame, graph.layers.len());
                events.sort_unstable_by_key(|&(i, _)| self.ranks[i]);
                for &(i, _) in &events {
                    if row.spans[i].0 == frame && !active.contains(&i) {
                        active.push(i);
                        for key in &mut keys {
                            key.marks.push(0);
                        }
                    }
                }
                if options.optimize_touch_order {
                    keys = self.compile_touch_frame(
                        row, frame, &events, &active, &settled, expired, keys, &mut graph, options,
                        stats, &mut frame_templates,
                    )?;
                } else {
                    for (note, d) in events {
                        if options.reuse_graph_storage {
                            event_actions.clear();
                        }
                        let bit = active.iter().position(|&i| i == note).unwrap();
                        let blockers = self.blockers(note, frame);
                        let mut next = vec![];
                        let mut map = HashMap::new();
                        let mut edges = vec![];
                        for (from, key) in keys.iter().enumerate() {
                            let to = if options.reuse_graph_storage {
                                intern_borrowed(key, &mut next, &mut map, stats)
                            } else {
                                intern(key.clone(), &mut next, &mut map)
                            };
                            edges.push(Edge {
                                from,
                                to,
                                action: None,
                            });
                            if key.marks[bit] != 0
                                || blockers.iter().any(|&i| {
                                    !settled[i]
                                        && active
                                            .iter()
                                            .position(|&a| a == i)
                                            .is_none_or(|j| key.marks[j] == 0)
                                })
                            {
                                continue;
                            }
                            // Keep the eager clone in the legacy path for A/B
                            // comparisons; rejected hits need no clone with reuse.
                            let hit = (!options.reuse_graph_storage).then(|| key.clone());
                            let mut legal = true;
                            for &relation in &row.incident[note] {
                                let r = &row.relations[relation];
                                let other = if r.before == note { r.after } else { r.before };
                                let stamp = active
                                    .iter()
                                    .position(|&i| i == other)
                                    .map(|j| key.marks[j])
                                    .unwrap_or(0);
                                if stamp > 1 {
                                    let (a, b) = if r.before == note {
                                        (frame, stamp - 1)
                                    } else {
                                        (stamp - 1, frame)
                                    };
                                    legal &= r.allows_frames(a, b)
                                        && (!r.strict_event_order
                                            || (a, self.ranks[r.before])
                                                < (b, self.ranks[r.after]));
                                }
                            }
                            if let Some(c) = self.ends[note] {
                                let slot = self.connection_slots[c];
                                if key.bindings[slot] != c + 1 {
                                    legal = false;
                                }
                            }
                            if !legal {
                                continue;
                            }
                            let mut hit = hit.unwrap_or_else(|| key.clone());
                            if let Some(c) = self.ends[note] {
                                hit.bindings[self.connection_slots[c]] = 0;
                            }
                            if let Some(c) = self.heads[note] {
                                hit.bindings[self.connection_slots[c]] = c + 1;
                            }
                            let rank = expired + key.marks.iter().filter(|&&m| m != 0).count() + 1;
                            hit.marks[bit] = if row.incident[note].is_empty() {
                                1
                            } else {
                                frame + 1
                            };
                            forget_closed_timestamps(row, &active, &settled, note, &mut hit);
                            let to = intern(hit, &mut next, &mut map);
                            let reused = if options.reuse_graph_storage {
                                event_actions
                                    .iter()
                                    .find_map(|&(r, a)| (r == rank).then_some(a))
                            } else {
                                None
                            };
                            let action = if let Some(action) = reused {
                                stats.graph_action_reuses += 1;
                                action
                            } else {
                                let action = graph.actions.len();
                                graph.actions.push(Action {
                                    factor: Factor {
                                        perfect: d.perfect,
                                        inside: d.inside,
                                        slot: d.slot,
                                        fever: d.fever,
                                        reference: row.references[note],
                                        rank,
                                    },
                                    hits: ActionHits::event(note, frame, options.inline_event_hits),
                                });
                                stats.graph_actions += 1;
                                if options.reuse_graph_storage {
                                    event_actions.push((rank, action));
                                }
                                action
                            };
                            edges.push(Edge {
                                from,
                                to,
                                action: Some(action),
                            });
                        }
                        if !push(&mut graph, next.len(), edges, options, stats) {
                            return None;
                        }
                        keys = next;
                    }
                }
                if options.incremental_touch && options.touch_dag
                    && options.max_cached_component_edges == 0 {
                    incremental.touch(self, row, frame, &active, &keys, &mut graph, stats);
                }
                temporal.after_touches(frame, &keys, &mut graph, stats);
                let due: Vec<_> = active
                    .iter()
                    .enumerate()
                    .filter_map(|(b, &i)| (row.spans[i].1 <= frame).then_some(b))
                    .collect();
                let retire: Vec<_> = active
                    .iter()
                    .enumerate()
                    .filter_map(|(b, &i)| (lives[i] <= frame).then_some(b))
                    .collect();
                // After all touches at this frame, a predecessor with only
                // already-satisfied fixed, strict outgoing gaps no longer
                // needs its exact timestamp. Merge those histories before the
                // next frame instead of carrying one state per hit frame.
                let may_saturate = active.iter().enumerate().any(|(bit, &note)| {
                    row.incident[note].iter().any(|&index| {
                        let r = &row.relations[index];
                        r.before == note
                            && r.strict_event_order
                            && r.earliest_after.is_none()
                            && r.gap <= 1
                    }) && keys.iter().any(|key| key.marks[bit] > 1)
                });
                if !due.is_empty() || may_saturate {
                    let mut next = vec![];
                    let mut map = HashMap::new();
                    let mut edges = vec![];
                    for (from, key) in keys.iter().enumerate() {
                        if due.iter().any(|&b| key.marks[b] == 0) {
                            continue;
                        }
                        let mut key = key.clone();
                        if may_saturate {
                            saturate_completed_frame_timestamps(
                                row, &active, &settled, frame, &mut key,
                            );
                        }
                        for &b in retire.iter().rev() {
                            key.marks.remove(b);
                        }
                        let to = intern(key, &mut next, &mut map);
                        edges.push(Edge {
                            from,
                            to,
                            action: None,
                        });
                    }
                    incremental.retire(&edges, &next);
                    if !push(&mut graph, next.len(), edges, options, stats) {
                        return None;
                    }
                    keys = next;
                    for &b in retire.iter().rev() {
                        settled[active.remove(b)] = true;
                        expired += 1;
                    }
                }
                incremental.after_frame(&active);
                temporal.after_frame(frame, graph.layers.len(), &keys);
                if keys.is_empty() {
                    return Some(graph);
                }
            }
            if let Some(key) = component_key {
                debug_assert!(active.is_empty());
                debug_assert_eq!(keys.len(), 1);
                self.component_cache.lock().unwrap().insert(
                    key,
                    &graph,
                    component_start,
                    options.max_cached_component_edges,
                    stats,
                );
            }
        }
        debug_assert_eq!(expired, n);
        debug_assert!(active.is_empty());
        Some(graph)
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    // Zero=unsettled; incident-note timestamps remain until the constraint closes.
    // Nonincident notes use 1, so choosing different equivalent frames merges.
    marks: Vec<u32>,
    bindings: Vec<usize>,
}
// A timestamp is read only when the OTHER endpoint settles. Once every
// incident endpoint has settled, all these checks have already succeeded on
// this path. Preserve the consumed bit and binding, but discard the time. The
// value DP and action witnesses independently retain skill history/hit times.
fn forget_closed_timestamps(
    row: &Row,
    active: &[usize],
    settled: &[bool],
    just_settled: usize,
    key: &mut Key,
) {
    let neighbours = row.incident[just_settled].iter().map(|&r| {
        let r = &row.relations[r];
        if r.before == just_settled {
            r.after
        } else {
            r.before
        }
    });
    for note in std::iter::once(just_settled).chain(neighbours) {
        let Some(bit) = active.iter().position(|&n| n == note) else {
            continue;
        };
        if key.marks[bit] <= 1 {
            continue;
        }
        if row.incident[note].iter().all(|&r| {
            let r = &row.relations[r];
            let other = if r.before == note { r.after } else { r.before };
            settled[other]
                || active
                    .iter()
                    .position(|&n| n == other)
                    .is_some_and(|b| key.marks[b] != 0)
        }) {
            key.marks[bit] = 1;
        }
    }
}

// Called only after every hit opportunity at `frame`. A strict gap of zero or
// one is satisfied at every later frame once its predecessor has been hit.
// The sentinel mark 1 keeps the consumed bit but discards the frame number.
fn saturate_completed_frame_timestamps(
    row: &Row,
    active: &[usize],
    settled: &[bool],
    frame: u32,
    key: &mut Key,
) {
    for (bit, &note) in active.iter().enumerate() {
        if key.marks[bit] <= 1 {
            continue;
        }
        let satisfied = row.incident[note].iter().all(|&index| {
            let r = &row.relations[index];
            let other = if r.before == note { r.after } else { r.before };
            if settled[other]
                || active
                    .iter()
                    .position(|&i| i == other)
                    .is_some_and(|other_bit| key.marks[other_bit] != 0)
            {
                return true;
            }
            r.before == note
                && r.strict_event_order
                && r.earliest_after.is_none()
                && frame
                    .checked_add(1)
                    .zip((key.marks[bit] - 1).checked_add(r.gap))
                    .is_some_and(|(next, required)| next >= required)
        });
        if satisfied {
            key.marks[bit] = 1;
        }
    }
}
fn intern(key: Key, states: &mut Vec<Key>, map: &mut HashMap<Key, usize>) -> usize {
    if let Some(&i) = map.get(&key) {
        i
    } else {
        let i = states.len();
        states.push(key.clone());
        map.insert(key, i);
        i
    }
}
fn intern_borrowed(
    key: &Key,
    states: &mut Vec<Key>,
    map: &mut HashMap<Key, usize>,
    stats: &mut WindowStats,
) -> usize {
    if let Some(&i) = map.get(key) {
        stats.graph_key_reuses += 1;
        i
    } else {
        let i = states.len();
        states.push(key.clone());
        map.insert(key.clone(), i);
        i
    }
}
fn push(
    graph: &mut Graph,
    count: usize,
    edges: Vec<Edge>,
    options: WindowOptions,
    stats: &mut WindowStats,
) -> bool {
    stats.graph_peak_states = stats.graph_peak_states.max(count);
    if count > options.max_states || graph.edge_count + edges.len() > options.max_graph_edges {
        return false;
    }
    stats.graph_edges += edges.len();
    graph.edge_count += edges.len();
    graph.layers.push(Layer { count, edges: edges.into() });
    true
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod correction_range_tests;

#[cfg(test)]
mod completed_timestamps_tests;
