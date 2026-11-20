use super::*;
use crate::hit_timing::independent::{FrontierNetWindows, WindowOptions, WindowStats};

impl TimingNetUpper {
    /// Full-frontier net upper against the SAME per-note baseline as the old
    /// stateful net table. Delayed starts are for scoring, not replacement.
    pub fn new_frontier_net(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
        options: WindowOptions,
    ) -> Result<Self, ReplayError> {
        let start = Instant::now();
        let previous = Self::new_stateful(input, skills, max_power)?;
        let references = FrameBounds::new(input, skills)?.note_baselines();
        let mut windows = FrontierNetWindows::new(input, skills, references, options)?;
        let mut keys = HashMap::new();
        let mut profiles = Vec::<[f64; 6]>::new();
        let mut totals = WindowStats::default();
        let mut catalogue_rows = vec![];
        for skill in skills {
            let key = skill.cache_key();
            let index = if let Some(&index) = keys.get(&key) {
                index
            } else {
                let (profile, stats) = windows.profile(skill)?;
                totals.accumulate(&stats);
                let index = profiles.len();
                keys.insert(key, index);
                profiles.push(profile);
                index
            };
            catalogue_rows.push(index);
        }
        let mut reference_capped_slots = 0;
        let mut max_cap_reduction = 0_f64;
        let gains = catalogue_rows
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                std::array::from_fn(|s| {
                    let raw = profiles[i][s];
                    let old = previous.gains[k][s];
                    if raw > old {
                        reference_capped_slots += 1;
                        max_cap_reduction = max_cap_reduction.max(raw - old);
                    }
                    // Both bound the same sum of (actual_i - B_i) inside slot s.
                    // This intersection would NOT be valid for the old owner split.
                    raw.min(old)
                })
            })
            .collect();
        let stats = TimingNetStats {
            stateful: true,
            delayed_windows: true,
            profiles: profiles.len(),
            request_rows: totals.request_frames,
            state_transitions: totals.transitions,
            state_max_frontier: totals.peak_value_states,
            state_budget_rows: totals.budget_requests as u64,
            state_solved_rows: (totals.request_frames
                - totals.skipped_requests
                - totals.infeasible_requests
                - totals.budget_requests) as u64,
            state_infeasible_rows: totals.infeasible_requests as u64,
            state_skipped_rows: totals.skipped_requests as u64,
            state_rows: totals.request_frames as u64,
            frontier: Some(TimingFrontierStats {
                revision: "frontier-net-common-baseline-120-v2",
                baseline_coefficient: previous.baseline,
                reference_capped_slots,
                max_cap_reduction,
                windows: totals,
            }),
            build_seconds: start.elapsed().as_secs_f64(),
            ..TimingNetStats::default()
        };
        Ok(Self {
            gains,
            stats,
            ..previous
        })
    }
}

#[cfg(test)]
mod tests;
