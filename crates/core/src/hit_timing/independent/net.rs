//! Signed continuous net contributions against a fixed per-note reference.
//! A local graph is a relaxation of a full six-skill plan, not an independently
//! playable song. Only notes inside this window contribute actual minus base.
use super::{geometry::RowRules, *};
use crate::hit_timing::NoteWindow;
use crate::timing::Instant;

struct NetMachine {
    machine: Machine,
    playing: u32,
    correction: u32,
    prefixes: machine::PrefixCache,
}

pub(crate) struct FrontierNetWindows {
    windows: IndependentWindows,
    references: Vec<f64>,
    triggers: [usize; 6],
    requests: [(u32, u32); 6],
    outside_correction: u32,
    reference_total: f64,
    machines: HashMap<[u32; 6], NetMachine>,
}
impl FrontierNetWindows {
    /// One fixed actual start, evaluated lazily by the adjacent-window clock.
    /// The trigger may have been queued earlier; this row relaxes its exact hit.
    pub fn at_start(
        &mut self,
        slot: usize,
        skill: &ReplaySkill,
        request: u32,
    ) -> Result<(f64, WindowStats), ReplayError> {
        // In one-edge bound mode, six trigger hits cannot fit a complete
        // successful graph. Publish the already permitted full envelope
        // directly. An impossible row may get a wider bound than zero; the
        // complete exact hit optimizer and roster optimum remain unchanged.
        if self.windows.options.max_graph_edges <= 1
            && std::env::var("BANDORI_TIMING_DISABLE_NET_ENVELOPE_ONLY")
                .as_deref() != Ok("1") {
            return self.at_start_envelope(slot, skill, request);
        }
        if self.windows.options.fast_net_queries {
            self.at_start_cached(slot, skill, request)
        } else {
            self.at_start_legacy(slot, skill, request)
        }
    }
    fn at_start_envelope(&mut self, slot: usize, skill: &ReplaySkill, request: u32)
        -> Result<(f64, WindowStats), ReplayError> {
        let started = Instant::now();
        let prepared = &self.windows.prepared;
        let mut stats = WindowStats { request_frames: 1, ..Default::default() };
        let cached = match self.machines.entry(skill.cache_key()) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                stats.net_machine_hits = 1;
                entry.into_mut()
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                stats.net_machine_builds = 1;
                entry.insert(NetMachine { machine: Machine::new(skill, prepared.input.notes.len())?,
                    playing: super::super::bounds::countdown(skill.duration) + 1,
                    correction: skill.cache_key()[1], prefixes: HashMap::new() })
            }
        };
        let costs = Costs::net(&prepared.input, &cached.machine, self.reference_total)?;
        stats.net_setup_seconds = started.elapsed().as_secs_f64();
        let started = Instant::now();
        let envelope = prepared.net_envelope_only(self.triggers[slot], request,
            cached.playing, cached.correction, RowRules { references: Some(&self.references),
                outside_correction: self.outside_correction, trigger_before: true });
        stats.net_row_seconds = started.elapsed().as_secs_f64();
        let Some(envelope) = envelope else {
            stats.infeasible_requests = 1;
            return Ok((0., stats));
        };
        let started = Instant::now();
        let upper = costs.upper(&envelope)?.coefficient;
        stats.net_envelope_seconds = started.elapsed().as_secs_f64();
        stats.budget_requests = 1;
        Ok((upper, stats))
    }
    fn at_start_cached(
        &mut self,
        slot: usize,
        skill: &ReplaySkill,
        request: u32,
    ) -> Result<(f64, WindowStats), ReplayError> {
        let started = Instant::now();
        let prepared = &self.windows.prepared;
        let options = self.windows.options;
        let mut stats = WindowStats {
            request_frames: 1,
            ..Default::default()
        };
        let cached = match self.machines.entry(skill.cache_key()) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                stats.net_machine_hits = 1;
                entry.into_mut()
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                stats.net_machine_builds = 1;
                entry.insert(NetMachine {
                    machine: Machine::new(skill, prepared.input.notes.len())?,
                    playing: super::super::bounds::countdown(skill.duration) + 1,
                    correction: skill.cache_key()[1],
                    prefixes: HashMap::new(),
                })
            }
        };
        let collapse = options.collapse_perfect_runs && cached.machine.monotone;
        let mut costs = Costs::net_cached(
            &prepared.input,
            &cached.machine,
            self.reference_total,
            &mut cached.prefixes,
        )?;
        stats.net_setup_seconds = started.elapsed().as_secs_f64();
        let rules = || RowRules {
            references: Some(self.references.as_slice()),
            outside_correction: self.outside_correction,
            trigger_before: true,
        };
        let key = (slot, cached.playing, cached.correction, collapse, request);
        let mut row = None;
        let graph = if let Some(graph) = self.windows.graphs.get(&key) {
            stats.graph_hits += 1;
            Some(graph.clone())
        } else {
            let started = Instant::now();
            row = prepared.row_without_envelope(
                self.triggers[slot],
                request,
                cached.playing,
                cached.correction,
                rules(),
            );
            stats.net_row_seconds += started.elapsed().as_secs_f64();
            let Some(row) = row.as_ref() else {
                stats.infeasible_requests = 1;
                return Ok((0., stats));
            };
            stats.graph_builds += 1;
            let started = Instant::now();
            let graph = prepared
                .compile(row, collapse, options, &mut stats)
                .map(Arc::new);
            stats.net_graph_seconds += started.elapsed().as_secs_f64();
            if let Some(graph) = &graph {
                if self.windows.cached_edges + graph.edge_count <= options.max_cached_edges {
                    self.windows.cached_edges += graph.edge_count;
                    self.windows.graphs.insert(key, graph.clone());
                }
            }
            graph
        };
        let started = Instant::now();
        let answer = match graph {
            Some(graph) => solve::evaluate_value(
                &graph,
                &mut costs,
                WindowObjective::Continuous,
                options,
                &mut stats,
            )?,
            None => None,
        };
        stats.net_value_seconds += started.elapsed().as_secs_f64();
        let upper = match answer {
            None => {
                stats.budget_requests += 1;
                // A cached graph came from a valid row; materialize that row only if
                // the value-state budget needs the original fallback envelope.
                if row.is_none() {
                    let started = Instant::now();
                    row = prepared.row_without_envelope(
                        self.triggers[slot],
                        request,
                        cached.playing,
                        cached.correction,
                        rules(),
                    );
                    stats.net_row_seconds += started.elapsed().as_secs_f64();
                }
                let started = Instant::now();
                let envelope =
                    prepared.envelope(row.as_ref().expect("cached graph has a source row"));
                let value = costs.upper(&envelope)?.coefficient;
                stats.net_envelope_seconds += started.elapsed().as_secs_f64();
                value
            }
            Some(None) => {
                stats.infeasible_requests += 1;
                0.
            }
            Some(Some(value)) => (value.coefficient + costs.coefficient_error())
                .next_up()
                .max(0.),
        };
        stats.prefix_tables = costs.prefix_count();
        Ok((upper, stats))
    }
    fn at_start_legacy(
        &mut self,
        slot: usize,
        skill: &ReplaySkill,
        request: u32,
    ) -> Result<(f64, WindowStats), ReplayError> {
        let started = Instant::now();
        let prepared = &self.windows.prepared;
        let options = self.windows.options;
        let playing = super::super::bounds::countdown(skill.duration) + 1;
        let correction = skill.cache_key()[1];
        let machine = Machine::new(skill, prepared.input.notes.len())?;
        let collapse = options.collapse_perfect_runs && machine.monotone;
        let reference_total = self
            .references
            .iter()
            .fold(0_f64, |a, &b| (a + b).next_up());
        let mut costs = Costs::net(&prepared.input, &machine, reference_total)?;
        let mut stats = WindowStats {
            request_frames: 1,
            net_machine_builds: 1,
            net_setup_seconds: started.elapsed().as_secs_f64(),
            ..WindowStats::default()
        };
        let started = Instant::now();
        let Some(row) = prepared.row_with_rules(
            self.triggers[slot],
            request,
            playing,
            correction,
            RowRules {
                references: Some(&self.references),
                outside_correction: self.outside_correction,
                trigger_before: true,
            },
        ) else {
            stats.net_row_seconds = started.elapsed().as_secs_f64();
            stats.infeasible_requests = 1;
            return Ok((0., stats));
        };
        stats.net_row_seconds = started.elapsed().as_secs_f64();
        let started = Instant::now();
        let envelope = costs.upper(&row.envelope)?.coefficient;
        stats.net_envelope_seconds = started.elapsed().as_secs_f64();
        let key = (slot, playing, correction, collapse, request);
        let started = Instant::now();
        let graph = if let Some(graph) = self.windows.graphs.get(&key) {
            stats.graph_hits += 1;
            Some(graph.clone())
        } else {
            stats.graph_builds += 1;
            prepared
                .compile(&row, collapse, options, &mut stats)
                .map(|g| {
                    let graph = Arc::new(g);
                    if self.windows.cached_edges + graph.edge_count <= options.max_cached_edges {
                        self.windows.cached_edges += graph.edge_count;
                        self.windows.graphs.insert(key, graph.clone());
                    }
                    graph
                })
        };
        stats.net_graph_seconds = started.elapsed().as_secs_f64();
        let started = Instant::now();
        let answer = match graph {
            Some(graph) => solve::evaluate_ordered(
                &graph,
                &mut costs,
                WindowObjective::Continuous,
                options,
                &mut stats,
            )?,
            None => None,
        };
        stats.net_value_seconds = started.elapsed().as_secs_f64();
        let upper = match answer {
            None => {
                stats.budget_requests += 1;
                envelope
            }
            Some(None) => {
                stats.infeasible_requests += 1;
                0.
            }
            Some(Some((value, _))) => (value.coefficient + costs.coefficient_error())
                .next_up()
                .max(0.),
        };
        stats.prefix_tables = costs.prefix_count();
        Ok((upper, stats))
    }
    pub fn new(
        input: &ReplayInput,
        catalogue: &[ReplaySkill],
        references: Vec<f64>,
        options: WindowOptions,
    ) -> Result<Self, ReplayError> {
        if catalogue.is_empty() || input.skills.len() != 6 {
            return Err(ReplayError::Input(
                "frontier net upper needs a catalogue and six requests",
            ));
        }
        for (i, s) in catalogue.iter().enumerate() {
            s.validate(i)?;
        }
        if references.len() != input.notes.len()
            || references.iter().any(|b| !b.is_finite() || *b < 0.)
        {
            return Err(ReplayError::Input("invalid per-note net reference"));
        }
        // Bounds always cover the complete touch-order domain, including when
        // the caller passes fixed-order diagnostic options by mistake.
        let mut options = WindowOptions {
            optimize_touch_order: true,
            ..options
        };
        // Charts with dispatch/arrival chains can require costly repeated local
        // graphs even when every node has an ordinary time window. At one
        // edge the compiler returns its complete per-note optimistic envelope;
        // the final hit optimizer still checks the exact connection edges.
        if input
            .constraints
            .iter()
            .any(|c| c.require_dispatch_order || c.earliest_after.is_some())
            || input
                .notes
                .iter()
                .any(|n| matches!(n.window, NoteWindow::NativeGreen { .. }))
        {
            let cap = std::env::var("BANDORI_CHAIN_GRAPH_EDGE_BUDGET")
                .or_else(|_| std::env::var("BANDORI_GREEN_GRAPH_EDGE_BUDGET"))
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|&n| n > 0)
                .unwrap_or(1);
            options.max_graph_edges = options.max_graph_edges.min(cap);
        }
        let windows = IndependentWindows::new(input, options)?;
        let mut triggers = [usize::MAX; 6];
        for (i, note) in input.notes.iter().enumerate() {
            if let Some(slot) = note.trigger {
                triggers[slot] = i;
            }
        }
        if triggers.contains(&usize::MAX) {
            return Err(ReplayError::Input(
                "frontier net upper needs all six requests",
            ));
        }
        let outside_correction = catalogue.iter().map(|s| s.cache_key()[1]).max().unwrap();
        let shortest = catalogue
            .iter()
            .map(|s| super::super::bounds::countdown(s.duration) + 1)
            .min()
            .unwrap();
        let longest = catalogue
            .iter()
            .map(|s| super::super::bounds::countdown(s.duration) + 1)
            .max()
            .unwrap();
        let cooling = super::super::bounds::countdown(0.75) + 1;
        let mut requests = [(0, 0); 6];
        let mut previous: Option<(u32, u32)> = None;
        for slot in 0..6 {
            let frames = windows
                .prepared
                .correctable_requests(triggers[slot], outside_correction);
            let Some(&first) = frames.first() else {
                return Err(ReplayError::Input("empty frontier net trigger domain"));
            };
            let (mut first, mut last) = (first, *frames.last().unwrap());
            // Existing delayed scoring domain, including chained requests.
            // The independent member-replacement model is not changed here.
            if let Some((a, b)) = previous {
                first = first.max(a + 1);
                last = last.max(b + 1);
            }
            previous = Some((first + shortest + cooling, last + longest + cooling));
            requests[slot] = (first, last);
        }
        let reference_total = references.iter().fold(0_f64, |a, &b| (a + b).next_up());
        Ok(Self {
            windows,
            references,
            triggers,
            requests,
            outside_correction,
            reference_total,
            machines: HashMap::new(),
        })
    }
    pub fn profile(&mut self, skill: &ReplaySkill) -> Result<([f64; 6], WindowStats), ReplayError> {
        let mut upper = [0.; 6];
        let mut total = WindowStats::default();
        for slot in 0..6 {
            let (value, stats) = self.slot(slot, skill)?;
            upper[slot] = value;
            total.accumulate(&stats);
        }
        Ok((upper, total))
    }
    fn slot(
        &mut self,
        slot: usize,
        skill: &ReplaySkill,
    ) -> Result<(f64, WindowStats), ReplayError> {
        skill.validate(slot)?;
        let prepared = &self.windows.prepared;
        let options = self.windows.options;
        let playing = super::super::bounds::countdown(skill.duration) + 1;
        let correction = skill.cache_key()[1];
        let machine = Machine::new(skill, prepared.input.notes.len())?;
        let collapse = options.collapse_perfect_runs && machine.monotone;
        let reference_total = self
            .references
            .iter()
            .fold(0_f64, |a, &b| (a + b).next_up());
        let mut costs = Costs::net(&prepared.input, &machine, reference_total)?;
        let row = |request| {
            prepared.row_with_rules(
                self.triggers[slot],
                request,
                playing,
                correction,
                RowRules {
                    references: Some(&self.references),
                    outside_correction: self.outside_correction,
                    trigger_before: true,
                },
            )
        };
        let mut stats = WindowStats {
            prepared_frames: prepared.frame_count,
            prepared_intervals: prepared.interval_count,
            ..WindowStats::default()
        };
        let mut candidates = vec![];
        for request in self.requests[slot].0..=self.requests[slot].1 {
            stats.request_frames += 1;
            if let Some(row) = row(request) {
                candidates.push((request, costs.upper(&row.envelope)?.coefficient));
            } else {
                stats.infeasible_requests += 1;
            }
        }
        // Keep only scalar envelopes until a row actually needs solving.
        candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        // Negative local maxima are permitted internally. Publishing max(0,U)
        // is an explicit upper relaxation for nonnegative pool linearization.
        let mut bound = 0_f64;
        for (request, envelope) in candidates {
            if options.prune_request_frames && envelope <= bound {
                stats.skipped_requests += 1;
                continue;
            }
            let key = (slot, playing, correction, collapse, request);
            // This cache instance serves net queries only. Its references,
            // outside correction and delayed-domain policy never change.
            let graph = if let Some(graph) = self.windows.graphs.get(&key) {
                stats.graph_hits += 1;
                Some(graph.clone())
            } else {
                stats.graph_builds += 1;
                prepared
                    .compile(&row(request).unwrap(), collapse, options, &mut stats)
                    .map(|g| {
                        let graph = Arc::new(g);
                        if self.windows.cached_edges + graph.edge_count <= options.max_cached_edges
                        {
                            self.windows.cached_edges += graph.edge_count;
                            self.windows.graphs.insert(key, graph.clone());
                        }
                        graph
                    })
            };
            let answer = match graph {
                Some(graph) => solve::evaluate_ordered(
                    &graph,
                    &mut costs,
                    WindowObjective::Continuous,
                    options,
                    &mut stats,
                )?,
                None => None,
            };
            match answer {
                None => {
                    stats.budget_requests += 1;
                    bound = bound.max(envelope);
                }
                Some(None) => stats.infeasible_requests += 1,
                Some(Some((value, _))) => {
                    bound = bound.max((value.coefficient + costs.coefficient_error()).next_up());
                }
            }
        }
        stats.prefix_tables = costs.prefix_count();
        Ok((bound, stats))
    }
}

#[cfg(test)]
mod tests;
