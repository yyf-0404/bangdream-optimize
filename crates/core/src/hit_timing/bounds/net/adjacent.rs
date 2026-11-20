//! Couple six per-start net bounds through the native FIFO skill clock.
//! Trigger legality remembers every not-yet-expired earlier skill, including
//! chains; local ordinary-note plans remain an explicit upper relaxation.
use super::*;
use crate::hit_timing::independent::{FrontierNetWindows, WindowOptions, WindowStats};
use std::sync::{Arc, Mutex};

mod joint;
mod remaining;

#[derive(Clone, Default, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingAdjacentStats {
    pub revision: &'static str,
    pub sequence_queries: u64,
    pub sequence_hits: u64,
    pub roster_queries: u64,
    pub roster_hits: u64,
    pub skipped_sequences: u64,
    pub schedule_transitions: u64,
    pub peak_schedule_states: usize,
    pub schedule_budget_fallbacks: u64,
    pub roster_transitions: u64,
    pub peak_roster_states: usize,
    pub merged_roster_prefixes: u64,
    pub roster_budget_fallbacks: u64,
    pub roster_bound_pruned: u64,
    pub remaining_cap_queries: u64,
    pub remaining_cap_fast_queries: u64,
    pub remaining_cap_exact_queries: u64,
    pub remaining_cap_suffix_builds: u64,
    pub remaining_cap_transitions: u64,
    pub roster_seed_queries: u64,
    pub roster_seed_seconds: f64,
    pub roster_cache_skips: u64,
    pub row_cache_skips: u64,
    pub row_builds: u64,
    pub row_hits: u64,
    pub row_seconds: f64,
    pub cached_rows: usize,
    pub cached_sequences: usize,
    pub cached_rosters: usize,
    pub windows: WindowStats,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct ClockKey {
    hit: u32,
    // MAX means no future trigger can observe this interval or its cooldown.
    // Retain the immediately preceding start until its FIFO ready frame is
    // no later than every possible next hit.
    starts: [u32; 6],
}

pub(crate) struct Adjacent {
    skills: Vec<ReplaySkill>,
    domains: [Vec<(u32, Judgement)>; 6],
    ranks: [usize; 6],
    caps: Vec<[f64; 6]>,
    windows: FrontierNetWindows,
    rows: HashMap<(usize, usize, u32), f64>,
    sequences: HashMap<[usize; 6], f64>,
    rosters: HashMap<[usize; 5], f64>,
    options: WindowOptions,
    stats: TimingAdjacentStats,
}

impl TimingNetUpper {
    /// Lazy full-window rows connected by actual trigger times, previous skill
    /// duration, native cooldown and correction of queued trigger notes.
    pub fn new_adjacent_net(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
        options: WindowOptions,
    ) -> Result<Self, ReplayError> {
        // A pruning bound must never inherit a fixed-order diagnostic domain.
        let options = WindowOptions {
            optimize_touch_order: true,
            ..options
        };
        let start = Instant::now();
        Self::new_stateful(input, skills, max_power)?.with_adjacent(input, skills, options, start, None)
    }
    pub(super) fn with_adjacent(self, input: &ReplayInput, skills: &[ReplaySkill],
        options: WindowOptions, start: Instant, frames: Option<FrameBounds>) -> Result<Self, ReplayError> {
        let mut table = self;
        let options = WindowOptions { optimize_touch_order: true, ..options };
        let frames = match frames { Some(frames) => frames, None => FrameBounds::new(input, skills)? };
        let references = frames.note_baselines();
        let event_ranks = crate::hit_timing::routing::event_ranks(input);
        let triggers: [usize; 6] = std::array::from_fn(|s| {
            input
                .notes
                .iter()
                .position(|n| n.trigger == Some(s))
                .unwrap()
        });
        let domains = std::array::from_fn(|s| {
            frames.notes[triggers[s]]
                .iter()
                .map(|f| (f.time, f.raw))
                .collect()
        });
        let engine = Adjacent {
            skills: skills.to_vec(),
            domains,
            ranks: if options.optimize_touch_order {
                std::array::from_fn(|s| s)
            } else {
                triggers.map(|i| event_ranks[i])
            },
            caps: table.gains.clone(),
            windows: FrontierNetWindows::new(input, skills, references, options)?,
            rows: HashMap::new(),
            sequences: HashMap::new(),
            rosters: HashMap::new(),
            options,
            stats: TimingAdjacentStats {
                revision: "adjacent-fifo-net-120-v4",
                ..Default::default()
            },
        };
        table.adjacent = Some(Arc::new(Mutex::new(engine)));
        table.stats.adjacent_enabled = true;
        table.stats.build_seconds = start.elapsed().as_secs_f64();
        Ok(table)
    }
    pub fn adjacent_stats(&self) -> Option<TimingAdjacentStats> {
        self.adjacent.as_ref().map(|a| a.lock().unwrap().stats())
    }
}

impl Adjacent {
    pub(crate) fn stats(&self) -> TimingAdjacentStats {
        TimingAdjacentStats {
            cached_rows: self.rows.len(),
            cached_sequences: self.sequences.len(),
            cached_rosters: self.rosters.len(),
            ..self.stats.clone()
        }
    }
    fn independent_cap(&self, sequence: &[usize; 6]) -> f64 {
        sequence
            .iter()
            .enumerate()
            .fold(0_f64, |v, (s, &k)| (v + self.caps[k][s]).next_up())
    }
    pub(crate) fn sequence(&mut self, sequence: [usize; 6]) -> Result<f64, ReplayError> {
        self.stats.sequence_queries += 1;
        if let Some(&answer) = self.sequences.get(&sequence) {
            self.stats.sequence_hits += 1;
            return Ok(answer);
        }
        let playing = sequence.map(|k| countdown(self.skills[k].duration) + 1);
        let correction = sequence.map(|k| self.skills[k].cache_key()[1]);
        let cap = self.independent_cap(&sequence);
        let windows = &mut self.windows;
        let rows = &mut self.rows;
        let cache_limit = self.options.max_cached_edges;
        let stats = &mut self.stats;
        let answer = schedule(
            &self.domains,
            self.ranks,
            playing,
            correction,
            self.options,
            stats,
            |slot, request, stats| {
                let k = sequence[slot];
                if let Some(&v) = rows.get(&(slot, k, request)) {
                    stats.row_hits += 1;
                    return Ok(v);
                }
                let start = Instant::now();
                let (upper, row_stats) = windows.at_start(slot, &self.skills[k], request)?;
                let upper = upper.min(self.caps[k][slot]);
                stats.windows.accumulate(&row_stats);
                stats.row_seconds += start.elapsed().as_secs_f64();
                stats.row_builds += 1;
                if rows.len() < cache_limit / 32 {
                    rows.insert((slot, k, request), upper);
                } else {
                    stats.row_cache_skips += 1;
                }
                Ok(upper)
            },
        )?
        .unwrap_or(cap)
        .min(cap);
        if self.sequences.len() < cache_limit / 8 {
            self.sequences.insert(sequence, answer);
        }
        Ok(answer)
    }
    pub(crate) fn roster(&mut self, mut members: [usize; 5]) -> Result<f64, ReplayError> {
        self.stats.roster_queries += 1;
        members.sort_unstable();
        if let Some(&v) = self.rosters.get(&members) {
            self.stats.roster_hits += 1;
            return Ok(v);
        }
        let (cap, seed_order) = joint::independent_best(members, &self.caps);
        // Normally this evaluates one member of the max-of-upper-bounds
        // objective, not a feasible lower bound on the actual game score.
        // A sequence-budget fallback instead supplies the maximal separable
        // roster cap, safely terminating this query at that cap; the sequence
        // fallback counter remains visible even if the joint root is pruned.
        let started = Instant::now();
        let seed = self.sequence(seed_order)?.max(0.);
        self.stats.roster_seed_queries += 1;
        self.stats.roster_seed_seconds += started.elapsed().as_secs_f64();
        let playing = members.map(|k| countdown(self.skills[k].duration) + 1);
        let correction = members.map(|k| self.skills[k].cache_key()[1]);
        let windows = &mut self.windows;
        let rows = &mut self.rows;
        let cache_limit = self.options.max_cached_edges;
        let answer = joint::schedule_bounded(
            &self.domains,
            self.ranks,
            members,
            playing,
            correction,
            members.map(|k| self.caps[k]),
            seed,
            self.options,
            &mut self.stats,
            |slot, member, request, stats| {
                let k = members[member];
                if let Some(&v) = rows.get(&(slot, k, request)) {
                    stats.row_hits += 1;
                    return Ok(v);
                }
                let start = Instant::now();
                let (upper, row_stats) = windows.at_start(slot, &self.skills[k], request)?;
                let upper = upper.min(self.caps[k][slot]);
                stats.windows.accumulate(&row_stats);
                stats.row_seconds += start.elapsed().as_secs_f64();
                stats.row_builds += 1;
                if rows.len() < cache_limit / 32 {
                    rows.insert((slot, k, request), upper);
                } else {
                    stats.row_cache_skips += 1;
                }
                Ok(upper)
            },
        )?
        .unwrap_or(cap)
        .min(cap);
        if self.rosters.len() < self.options.max_cached_edges / 64 {
            self.rosters.insert(members, answer);
        } else {
            self.stats.roster_cache_skips += 1;
        }
        Ok(answer)
    }

    // Keep the original complete-sequence calculation as an independent test
    // oracle. Production roster queries share member-subset clock prefixes.
    #[cfg(test)]
    fn roster_enumerated(&mut self, mut members: [usize; 5]) -> Result<f64, ReplayError> {
        members.sort_unstable();
        let mut sequences = Vec::new();
        fn visit(
            members: [usize; 5],
            used: u8,
            order: &mut [usize; 6],
            output: &mut Vec<[usize; 6]>,
        ) {
            let slot = used.count_ones() as usize;
            if slot == 5 {
                for i in 0..5 {
                    if i > 0 && members[i] == members[i - 1] {
                        continue;
                    }
                    order[5] = members[i];
                    output.push(*order);
                }
                return;
            }
            let mut last = None;
            for i in 0..5 {
                if used & (1 << i) != 0 || last == Some(members[i]) {
                    continue;
                }
                last = Some(members[i]);
                order[slot] = members[i];
                visit(members, used | (1 << i), order, output);
            }
        }
        visit(members, 0, &mut [0; 6], &mut sequences);
        let mut ranked: Vec<_> = sequences
            .into_iter()
            .map(|s| (self.independent_cap(&s), s))
            .collect();
        ranked.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut best = 0_f64;
        for (cap, sequence) in ranked {
            if cap <= best {
                self.stats.skipped_sequences += 1;
                continue;
            }
            best = best.max(self.sequence(sequence)?);
        }
        Ok(best)
    }
}

/// Native requests are FIFO; a skill starts after the input frame and covers
/// (start, start+playing]. A request on the cooling transition frame waits one
/// further frame, exactly as SkillClock::after_hits. No one-step chain assumption.
fn schedule(
    domains: &[Vec<(u32, Judgement)>; 6],
    ranks: [usize; 6],
    playing: [u32; 6],
    correction: [u32; 6],
    options: WindowOptions,
    stats: &mut TimingAdjacentStats,
    mut value: impl FnMut(usize, u32, &mut TimingAdjacentStats) -> Result<f64, ReplayError>,
) -> Result<Option<f64>, ReplayError> {
    let cooling = countdown(0.75) + 1;
    let mut states = HashMap::from([(
        ClockKey {
            hit: 0,
            starts: [u32::MAX; 6],
        },
        0_f64,
    )]);
    let mut transitions = 0;
    for slot in 0..6 {
        let mut next = HashMap::<ClockKey, f64>::new();
        for (key, score) in states {
            for &(hit, raw) in &domains[slot] {
                transitions += 1;
                stats.schedule_transitions += 1;
                if transitions > options.max_transitions {
                    stats.schedule_budget_fallbacks += 1;
                    return Ok(None);
                }
                if slot > 0 && (hit < key.hit || (hit == key.hit && ranks[slot] <= ranks[slot - 1]))
                {
                    continue;
                }
                if raw < Judgement::Great
                    && !(0..slot).any(|j| {
                        let start = key.starts[j];
                        start != u32::MAX
                            && start < hit
                            && hit <= start + playing[j]
                            && corrected(raw, correction[j]).is_some()
                    })
                {
                    continue;
                }
                let start = if slot == 0 || key.starts[slot - 1] == u32::MAX {
                    hit
                } else {
                    hit.max(key.starts[slot - 1] + playing[slot - 1] + cooling + 1)
                };
                let score = (score + value(slot, start, stats)?).next_up();
                let mut out = ClockKey {
                    hit,
                    starts: key.starts,
                };
                out.starts[slot] = start;
                if slot < 5 {
                    let earliest = domains[slot + 1]
                        .iter()
                        .map(|p| p.0)
                        .min()
                        .unwrap_or(u32::MAX);
                    for j in 0..slot {
                        if out.starts[j] != u32::MAX && out.starts[j] + playing[j] < earliest {
                            out.starts[j] = u32::MAX;
                        }
                    }
                    // The next row cannot distinguish earlier input times once
                    // all its possible hits are strictly later. Do not merge a
                    // tie: same-frame event ordering still matters there.
                    if out.hit < earliest {
                        out.hit = 0;
                    }
                    if start + playing[slot] + cooling + 1 <= earliest {
                        // FIFO ensures all older skills have ended too. Every
                        // next hit starts immediately, so this complete prefix
                        // is observationally equivalent to an idle clock.
                        out.starts = [u32::MAX; 6];
                    }
                } else {
                    // No later trigger can observe any part of this clock.
                    out.hit = 0;
                    out.starts = [u32::MAX; 6];
                }
                next.entry(out)
                    .and_modify(|v| *v = v.max(score))
                    .or_insert(score);
                stats.peak_schedule_states = stats.peak_schedule_states.max(next.len());
                if next.len() > options.max_states {
                    stats.schedule_budget_fallbacks += 1;
                    return Ok(None);
                }
            }
        }
        states = next;
    }
    Ok(Some(states.into_values().fold(0., f64::max)))
}

#[cfg(test)]
mod tests;
