//! A fixed-chart owner outliving individual skill masks and cached models.
use super::*;
use crate::timing::Instant;

#[derive(Clone, Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JointPreparationStats {
    pub enabled: bool,
    pub bad_interval_scan_enabled: bool,
    pub bad_interval_audit_enabled: bool,
    pub bad_bounds_audited: u64,
    pub protected_scans_audited: u64,
    pub bad_bounds_preparations: u64,
    pub bad_bounds_seconds: f64,
    pub bad_judgement_probes: u64,
    pub bad_same_time_reuses: u64,
    pub bad_bounds_payload_bytes: usize,
    pub protected_scans: u64,
    pub protected_note_visits: u64,
    pub chart_preparations: u64,
    pub geometry_builds: u64,
    pub geometry_reuses: u64,
    pub chart_preparation_seconds: f64,
    pub geometry_key_seconds: f64,
    pub geometry_rebind_seconds: f64,
    pub baseline_replay_seconds: f64,
    pub baseline_full_replays: u64,
    pub baseline_certified_validations: u64,
    pub baseline_skill_envelopes: u64,
    pub row_seconds: f64,
    pub graph_seconds: f64,
    pub operands_seconds: f64,
    pub skill_machines_seconds: f64,
    pub chart_data_payload_bytes: usize,
    pub outside_cache_queries: u64,
    pub outside_cache_hits: u64,
    pub outside_negative_hits: u64,
    pub outside_proofs: u64,
    pub outside_boundary_rejects: u64,
    pub outside_cache_stores: u64,
    pub outside_cache_evictions: u64,
    pub outside_cache_skips: u64,
    pub outside_lookup_seconds: f64,
    pub outside_proof_seconds: f64,
    pub outside_cache_entries: usize,
    pub outside_cache_payload_bytes: usize,
    pub outside_cache_budget_bytes: usize,
    pub functions: functions::FunctionStats,
}

pub(crate) struct JointPreparation {
    prepared: Option<Prepared>,
    bad_bounds: Option<local::BadBounds>,
    outside: geometry::OutsideCache,
    baseline_validation: crate::hit_timing::baseline::BaselineValidation,
    functions: Arc<std::sync::Mutex<functions::Cache>>,
    pub stats: JointPreparationStats,
}
impl Default for JointPreparation {
    fn default() -> Self {
        Self {
            prepared: None,
            bad_bounds: None,
            outside: geometry::OutsideCache::default(),
            baseline_validation: Default::default(),
            functions: Default::default(),
            stats: JointPreparationStats {
                bad_interval_scan_enabled: local::enabled() && local::bad_scan_enabled(),
                bad_interval_audit_enabled: std::env::var("BANDORI_TIMING_AUDIT_BAD_INTERVAL_SCAN")
                    .as_deref() == Ok("1"),
                enabled: local::enabled() && std::env::var("BANDORI_TIMING_DISABLE_OUTSIDE_TRANSFER_CACHE")
                    .as_deref() != Ok("1"),
                ..JointPreparationStats::default()
            },
        }
    }
}
impl JointPreparation {
    pub(crate) fn enabled(&self) -> bool { self.stats.enabled }

    /// Raw note windows are fixed for this owner, independent of skills, routing
    /// lifetime and graph eviction. Only the FIFO envelopes/mask vary per call.
    pub(crate) fn geometry(&mut self, input: &ReplayInput, share_playing_windows: bool)
        -> Result<local::JointGeometry, ReplayError> {
        if !self.stats.bad_interval_scan_enabled {
            return local::geometry(input, local::enabled());
        }
        let start = Instant::now();
        self.baseline_validation.validate(input)?;
        self.stats.baseline_replay_seconds += start.elapsed().as_secs_f64();
        if self.bad_bounds.is_none() {
            let start = Instant::now();
            let bad = local::BadBounds::new(&input.notes)?;
            if self.stats.bad_interval_audit_enabled {
                bad.audit(&input.notes)?;
                self.stats.bad_bounds_audited += 1;
            }
            self.stats.bad_bounds_seconds += start.elapsed().as_secs_f64();
            self.stats.bad_bounds_preparations += 1;
            self.stats.bad_judgement_probes += bad.judgement_probes;
            self.stats.bad_same_time_reuses += bad.same_time_reuses;
            self.stats.bad_bounds_payload_bytes = bad.payload_bytes();
            self.bad_bounds = Some(bad);
        }
        let layout = local::geometry_with_bounds(input, self.bad_bounds.as_ref().unwrap(), share_playing_windows)?;
        if self.stats.bad_interval_audit_enabled {
            local::audit_geometry(input, &layout)?;
            self.stats.protected_scans_audited += 1;
        }
        self.stats.protected_scans += 1;
        self.stats.protected_note_visits += input.notes.len() as u64;
        Ok(layout)
    }

    pub(crate) fn stats(&self) -> JointPreparationStats {
        let mut stats = self.stats.clone();
        stats.baseline_full_replays = self.baseline_validation.stats.full_replays;
        stats.baseline_certified_validations = self.baseline_validation.stats.certified_validations;
        stats.baseline_skill_envelopes = self.baseline_validation.stats.skill_envelopes;
        stats.functions = self.functions.lock().unwrap().stats();
        let outside = &self.outside.stats;
        stats.outside_cache_queries = outside.queries;
        stats.outside_cache_hits = outside.hits;
        stats.outside_negative_hits = outside.negative_hits;
        stats.outside_proofs = outside.proofs;
        stats.outside_boundary_rejects = outside.boundary_rejects;
        stats.outside_cache_stores = outside.stores;
        stats.outside_cache_evictions = outside.evictions;
        stats.outside_cache_skips = outside.skipped;
        stats.outside_lookup_seconds = outside.lookup_seconds;
        stats.outside_proof_seconds = outside.proof_seconds;
        stats.outside_cache_entries = self.outside.entries();
        stats.outside_cache_payload_bytes = self.outside.bytes();
        stats.outside_cache_budget_bytes = self.outside.limit();
        stats
    }

    /// The owning TimingCompileCache fixes chart, routing, baseline, Combo and
    /// Fever for this lifetime. Only resolved skills may vary between calls.
    pub(crate) fn build(
        &mut self,
        input: &ReplayInput,
        layout: local::JointGeometry,
    ) -> Result<JointTimeline, ReplayError> {
        self.build_with_ranges(input,layout,None)?.ok_or(ReplayError::Input("unrestricted row has no baseline"))
    }

    pub(crate) fn build_in_frame_ranges(&mut self,input:&ReplayInput,layout:local::JointGeometry,
        limits:&[Option<[u32;2]>]) -> Result<Option<JointTimeline>,ReplayError> {
        self.build_with_ranges(input,layout,Some(limits))
    }

    /// The owning cache also fixes the user limits. Reuse chart preparation
    /// and outside transfers within that one restricted domain only.
    fn build_with_ranges(&mut self,input:&ReplayInput,layout:local::JointGeometry,
        limits:Option<&[Option<[u32;2]>]>) -> Result<Option<JointTimeline>,ReplayError> {
        debug_assert!(self.enabled());
        let start = Instant::now();
        self.baseline_validation.validate(input)?;
        self.stats.baseline_replay_seconds += start.elapsed().as_secs_f64();
        if self.prepared.is_none() {
            let start = Instant::now();
            let prepared = Prepared::new(input)?;
            self.stats.chart_data_payload_bytes = prepared.chart_payload_bytes();
            self.prepared = Some(prepared);
            self.stats.chart_preparations += 1;
            self.stats.chart_preparation_seconds += start.elapsed().as_secs_f64();
        }
        let prepared = self.prepared.as_ref().unwrap();
        let start = Instant::now();
        let row = prepared.joint_row_with_ranges(&layout.corrections);
        let row=if let Some(limits)=limits {
            let Some(row)=prepared.restrict_row(row,limits)? else {return Ok(None);};row
        } else {row};
        self.stats.row_seconds += start.elapsed().as_secs_f64();
        let mut stats = WindowStats {
            prepared_frames: prepared.frame_count,
            prepared_intervals: prepared.interval_count,
            graph_builds: 1,
            ..WindowStats::default()
        };
        let start = Instant::now();
        let mut graph = prepared.compile_local_cached(&row, &layout.protected,
            &mut self.outside, options(), &mut stats)
            .expect("joint geometry has no exploration budget");
        self.stats.graph_seconds += start.elapsed().as_secs_f64();
        self.stats.geometry_builds += 1;
        let start = Instant::now();
        let prepared_actions = Arc::new(PreparedActions::new(input, &graph)?);
        let value_layout = quotient::Layout::new(input, &graph, &prepared_actions);
        let (value_layout, packed_layout) = JointTimeline::value_views(&graph, value_layout);
        graph.freeze();
        let geometry_bytes = JointTimeline::geometry_footprint(&graph, &prepared_actions)
            + value_layout.as_ref().map_or(0, |l| l.bytes())
            + packed_layout.as_ref().map_or(0, |l| l.bytes());
        self.stats.operands_seconds += start.elapsed().as_secs_f64();
        let start = Instant::now();
        let machines = input.skills.iter().map(|s| Machine::new(s, input.notes.len()))
            .collect::<Result<_, _>>()?;
        let playing = input.skills.iter().map(|s| countdown(s.duration) + 1).collect();
        self.stats.skill_machines_seconds += start.elapsed().as_secs_f64();
        Ok(Some(JointTimeline {
            input: input.clone(), graph: Arc::new(graph), prepared_actions, value_layout, packed_layout, machines,
            playing, stats, geometry_key: layout.key, local_geometry: true, geometry_bytes,
            functions: Arc::clone(&self.functions),
            function_plan: None,
            controls: Controls::prepared(),
        }.with_functions()?))
    }
}
