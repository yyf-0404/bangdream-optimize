use super::*;

mod assignment;
mod ordered;
mod state;
pub(crate) use ordered::OrderedQueueScoreCache;
pub(crate) use state::QueueStateMachine;
pub(super) use state::StatePlanCache;

pub(crate) const QUEUE_DURATIONS: [f64; 17] = SKILL_DURATIONS;
pub(crate) type ConditionalSkillMeta = [[f64; 17]; 6];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
struct WindowRange {
    start: usize,
    end: usize,
    run_start: usize,
    run_end: usize,
}

impl WindowRange {
    fn from_window(window: ExactSkillWindow) -> Self {
        Self {
            start: window.range_start,
            end: window.range_end,
            run_start: window.run_start,
            run_end: window.run_end,
        }
    }

    fn with_skill(self, skill: TeamCardSkill) -> ExactSkillWindow {
        ExactSkillWindow {
            range_start: self.start,
            range_end: self.end,
            run_start: self.run_start,
            run_end: self.run_end,
            score_up: skill.score_up,
            rateup: skill.rateup,
        }
    }
}

fn duration_index(duration: f64) -> Option<usize> {
    QUEUE_DURATIONS
        .binary_search_by(|value| value.total_cmp(&duration))
        .ok()
}

fn window_duration_index(skill: TeamCardSkill) -> Option<usize> {
    // The cached geometry is shared, but must not bypass the narrower set of
    // durations accepted by the rate-up scorer. Uncached compilation validates.
    if skill.rateup && !RATEUP_DURATIONS.contains(&skill.duration) {
        return None;
    }
    duration_index(skill.duration)
}

/// Geometry depends only on the initialized chart and the two durations, never
/// on a card's score multiplier, identity or team stat. Rebuilt by Chart::init.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct QueueWindowCache {
    windows: Vec<[[WindowRange; 17]; 6]>,
    predecessor_independent: Vec<[bool; 6]>,
    kinds: [SkillQueueKind; 17],
    machine: std::sync::Arc<QueueStateMachine>,
}

impl QueueWindowCache {
    pub(super) fn new(chart: &Chart) -> Result<Self, ChartError> {
        let mut windows = Vec::with_capacity(17);
        for duration in QUEUE_DURATIONS {
            let skill = TeamCardSkill {
                card_id: 0,
                duration,
                score_up: 0.0,
                rateup: false,
            };
            let mut positions = [[WindowRange::default(); 17]; 6];
            for (position, predecessors) in positions.iter_mut().enumerate() {
                for (index, &previous) in QUEUE_DURATIONS.iter().enumerate() {
                    predecessors[index] = WindowRange::from_window(
                        chart.conditional_skill_window_uncached(position, skill, previous)?,
                    );
                }
            }
            windows.push(positions);
        }
        Ok(Self {
            machine: std::sync::Arc::new(QueueStateMachine::new(chart)?),
            predecessor_independent: windows
                .iter()
                .map(|positions| positions.map(|row| row.iter().all(|range| *range == row[0])))
                .collect(),
            windows,
            kinds: QUEUE_DURATIONS.map(|duration| chart.skill_queue_kind_uncached(duration)),
        })
    }
}

impl ExactSkillWindow {
    pub(crate) fn note_count(&self) -> usize {
        self.range_end - self.range_start
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillQueueKind {
    #[default]
    None,
    Single,
    Chain,
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn chart(triggers: [f64; 6], auto: bool) -> Chart {
        let mut nodes: Vec<_> = (0..1500)
            .map(|i| ChartNode {
                time: i as f64 / 20.0,
                node_type: ChartNodeType::Node,
            })
            .collect();
        nodes.extend(triggers.map(|time| ChartNode {
            time,
            node_type: ChartNodeType::Skill,
        }));
        nodes.sort_by(|a, b| a.time.total_cmp(&b.time));
        let mut chart = Chart::new(27, nodes);
        if auto {
            chart.init_auto_with_base_multiplier(0.75).unwrap();
        } else {
            chart.init(0, true).unwrap();
        }
        chart
    }

    #[test]
    fn predecessor_matrix_matches_full_scheduler_for_every_order_and_captain() {
        for auto in [false, true] {
            for triggers in [
                [1.003, 7.013, 22.111, 28.221, 44.0, 50.3],
                [1.0, 16.0, 31.0, 46.0, 61.0, 66.0],
            ] {
                let chart = chart(triggers, auto);
                let skills = std::array::from_fn(|i| TeamCardSkill {
                    card_id: i as u32,
                    duration: [5.0, 5.5, 6.0, 6.5, 7.0][i],
                    score_up: 0.8 + i as f64 * 0.15,
                    rateup: i == 1 || i == 4,
                });
                assert_eq!(chart.skill_queue_kind(7.0), SkillQueueKind::Single);
                for stat in [1, 12345, 543210] {
                    let matrix = chart
                        .predecessor_skill_score_matrix(
                            &skills,
                            stat,
                            !auto,
                            &mut ExactScoreScratch::default(),
                        )
                        .unwrap()
                        .unwrap();
                    let mut best = i32::MIN;
                    for order in &crate::skill_shuffle::tables().orders {
                        for captain in 0..5 {
                            let six = std::array::from_fn(|i| {
                                skills[if i == 5 { captain } else { order[i] }]
                            });
                            let exact = chart.get_score_for_six_skills(&six, stat, !auto).unwrap();
                            assert_eq!(
                                matrix.score(*order, captain),
                                exact,
                                "auto={auto} stat={stat} order={order:?} captain={captain}"
                            );
                            best = best.max(exact);
                        }
                    }
                    assert_eq!(matrix.maximum().score, best);
                }
            }
        }
    }

    #[test]
    fn chain_bypasses_predecessor_matrix() {
        let chart = chart([1.0, 6.0, 11.0, 30.0, 45.0, 60.0], false);
        assert_eq!(chart.skill_queue_kind(3.0), SkillQueueKind::None);
        assert_eq!(chart.skill_queue_kind(7.0), SkillQueueKind::Chain);
        let skills = [TeamCardSkill {
            card_id: 1,
            duration: 7.0,
            score_up: 1.0,
            rateup: false,
        }; 5];
        assert!(chart
            .predecessor_skill_score_matrix(
                &skills,
                100000,
                true,
                &mut ExactScoreScratch::default()
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn cached_windows_match_scheduler_and_rebuild_after_reinitialization() {
        let mut chart = chart([1.003, 7.013, 22.111, 28.221, 44.0, 50.3], false);
        let original = chart.clone();
        assert!(std::sync::Arc::ptr_eq(
            chart.queue_windows.as_ref().unwrap(),
            original.queue_windows.as_ref().unwrap()
        ));
        for rule in [
            ScoreRule::STANDARD,
            ScoreRule::auto_with_base_multiplier(0.75),
        ] {
            chart.init_with_rule(1234, true, rule).unwrap();
            assert!(!std::sync::Arc::ptr_eq(
                chart.queue_windows.as_ref().unwrap(),
                original.queue_windows.as_ref().unwrap()
            ));
            for duration in QUEUE_DURATIONS.into_iter().chain([6.500_001]) {
                assert_eq!(
                    chart.skill_queue_kind(duration),
                    chart.skill_queue_kind_uncached(duration)
                );
                let skill = TeamCardSkill {
                    card_id: 1,
                    duration,
                    score_up: 1.15,
                    rateup: RATEUP_DURATIONS.contains(&duration),
                };
                for previous in QUEUE_DURATIONS.into_iter().chain([6.3]) {
                    for position in 0..6 {
                        let cached = chart
                            .conditional_skill_window(position, skill, previous)
                            .unwrap();
                        let expected = chart
                            .conditional_skill_window_uncached(position, skill, previous)
                            .unwrap();
                        assert_eq!(
                            WindowRange::from_window(cached),
                            WindowRange::from_window(expected)
                        );
                        assert_eq!(cached.score_up.to_bits(), expected.score_up.to_bits());
                        assert_eq!(cached.rateup, expected.rateup);
                    }
                }
            }
        }
        // Reusing a Chart for a different trigger layout must drop all old ranges.
        let triggers = [1.0, 13.0, 25.0, 37.0, 49.0, 61.0];
        for (index, time) in chart.skill_node_indices.clone().into_iter().zip(triggers) {
            chart.nodes[index].time = time;
        }
        chart.init(0, true).unwrap();
        assert!(chart.queue_windows.is_none());
        assert_eq!(chart.skill_queue_kind(8.0), SkillQueueKind::None);
        assert_eq!(original.skill_queue_kind(7.0), SkillQueueKind::Single);
    }

    #[test]
    fn queued_portable_native_and_fever_scores_match_brute_force() {
        for auto in [false, true] {
            for fever in [false, true] {
                let mut chart = chart([1.0, 7.0, 24.0, 40.0, 56.0, 62.0], auto);
                if fever {
                    chart.fever_start = Some(4.0);
                    chart.fever_end = Some(34.0);
                    let rule = chart.score_rule;
                    chart
                        .init_with_rule_and_fever(287, true, rule, true)
                        .unwrap();
                }
                for duration in [6.5, 6.500_001, 7.0] {
                    let skills = std::array::from_fn(|i| TeamCardSkill {
                        card_id: i as u32,
                        // Equal skill columns and equal predecessor durations both occur.
                        duration: if i < 3 { duration } else { 7.0 },
                        score_up: if i < 3 { 1.000001 } else { 1.3 },
                        rateup: i == 4,
                    });
                    assert_eq!(chart.skill_queue_kind(7.0), SkillQueueKind::Single);
                    for stat in [1, 294_731, 543_210] {
                        let portable = chart
                            .predecessor_matrix_impl(
                                &skills,
                                stat,
                                true,
                                &mut ExactScoreScratch::default(),
                            )
                            .unwrap();
                        let native = chart
                            .predecessor_skill_score_matrix(
                                &skills,
                                stat,
                                true,
                                &mut ExactScoreScratch::compressed(),
                            )
                            .unwrap()
                            .unwrap();
                        assert_eq!(native.base, portable.base);
                        assert_eq!(native.deltas, portable.deltas);
                        let mut best = None;
                        for order in &crate::skill_shuffle::tables().orders {
                            for captain in 0..5 {
                                let six = std::array::from_fn(|p| {
                                    skills[if p == 5 { captain } else { order[p] }]
                                });
                                let exact =
                                    chart.get_score_for_six_skills(&six, stat, true).unwrap();
                                assert_eq!(native.score(*order, captain), exact,
                                    "auto={auto} fever={fever} duration={duration} stat={stat} order={order:?} captain={captain}");
                                let key = (exact, std::cmp::Reverse((*order, captain)));
                                if best.is_none_or(|old| key > old) {
                                    best = Some(key);
                                }
                            }
                        }
                        let (score, std::cmp::Reverse((order_indices, captain_index))) =
                            best.unwrap();
                        assert_eq!(
                            native.maximum(),
                            ExactSkillOrder {
                                score,
                                order_indices,
                                captain_index
                            }
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn packed_assignment_preserves_optimum_and_lexicographic_ties() {
        let mut random = 0x1234_4321_0101u64;
        for sample in 0..128 {
            let mut matrix = PredecessorSkillScoreMatrix {
                base: 31,
                deltas: [[[0; 5]; 5]; 6],
            };
            for delta in matrix.deltas.iter_mut().flatten().flatten() {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                *delta = if sample == 0 {
                    0
                } else {
                    (((random >> 32) % 13) as i32 - 6) * if sample % 2 == 0 { 1 } else { 300_001 }
                };
            }
            let mut expected = None;
            for &order in &crate::skill_shuffle::tables().orders {
                for captain in 0..5 {
                    let key = (
                        matrix.score(order, captain),
                        std::cmp::Reverse((order, captain)),
                    );
                    if expected.is_none_or(|old| key > old) {
                        expected = Some(key);
                    }
                }
            }
            let (score, std::cmp::Reverse((order_indices, captain_index))) = expected.unwrap();
            assert_eq!(
                matrix.maximum(),
                ExactSkillOrder {
                    score,
                    order_indices,
                    captain_index
                }
            );
        }
    }

    #[test]
    fn cached_geometry_keeps_duration_validation() {
        let chart = chart([1.0, 7.0, 24.0, 40.0, 56.0, 62.0], false);
        for (duration, rateup) in [(5.3, false), (6.2, true), (7.5, true)] {
            let invalid = TeamCardSkill {
                card_id: 1,
                duration,
                score_up: 1.0,
                rateup,
            };
            assert!(matches!(
                chart.conditional_skill_window(1, invalid, 7.0),
                Err(ChartError::UnsupportedSkillDuration { .. })
            ));
            let mut team = [TeamCardSkill {
                duration: 7.0,
                rateup: false,
                ..invalid
            }; 5];
            team[3] = invalid;
            assert!(matches!(
                chart.predecessor_skill_score_matrix(
                    &team,
                    300000,
                    true,
                    &mut ExactScoreScratch::compressed()
                ),
                Err(ChartError::UnsupportedSkillDuration { .. })
            ));
        }
    }
}

pub(crate) struct PredecessorSkillScoreMatrix {
    pub(crate) base: i32,
    // First position ignores predecessor. Sixth position repeats the captain.
    pub(crate) deltas: [[[i32; 5]; 5]; 6],
}

impl PredecessorSkillScoreMatrix {
    pub(crate) fn score(&self, order: [usize; 5], captain: usize) -> i32 {
        self.base
            + self.deltas[0][0][order[0]]
            + (1..5)
                .map(|p| self.deltas[p][order[p - 1]][order[p]])
                .sum::<i32>()
            + self.deltas[5][order[4]][captain]
    }

    pub(crate) fn maximum(&self) -> ExactSkillOrder {
        assignment::maximum(&self.deltas, self.base)
    }
}

impl Chart {
    /// Longest duration gives the latest reachable release at every trigger.
    /// Two consecutive delayed activations require more than predecessor duration.
    pub fn skill_queue_kind(&self, max_duration: f64) -> SkillQueueKind {
        if let (Some(cache), Some(index)) = (&self.queue_windows, duration_index(max_duration)) {
            return cache.kinds[index];
        }
        self.skill_queue_kind_uncached(max_duration)
    }

    fn skill_queue_kind_uncached(&self, max_duration: f64) -> SkillQueueKind {
        if self.warning.is_empty() {
            return SkillQueueKind::None;
        }
        let mut scheduler = SkillScheduler::new(self.uses_ideal_60fps_timing());
        let mut previous_queued = false;
        let mut result = SkillQueueKind::None;
        for &index in &self.skill_node_indices {
            let trigger = self.nodes[index].time;
            let window = scheduler.schedule(trigger, max_duration);
            let queued = if self.uses_ideal_60fps_timing() {
                window.queue_risk
            } else {
                sgn(window.start - trigger) > 0
            };
            if queued && previous_queued {
                return SkillQueueKind::Chain;
            }
            if queued {
                result = SkillQueueKind::Single;
            }
            previous_queued = queued;
        }
        result
    }

    pub(crate) fn conditional_skill_window(
        &self,
        activation: usize,
        skill: TeamCardSkill,
        predecessor_duration: f64,
    ) -> Result<ExactSkillWindow, ChartError> {
        if activation < 6 {
            if let (Some(cache), Some(current), Some(previous)) = (
                &self.queue_windows,
                window_duration_index(skill),
                duration_index(predecessor_duration),
            ) {
                return Ok(cache.windows[current][activation][previous].with_skill(skill));
            }
        }
        self.conditional_skill_window_uncached(activation, skill, predecessor_duration)
    }

    fn conditional_skill_window_uncached(
        &self,
        activation: usize,
        skill: TeamCardSkill,
        predecessor_duration: f64,
    ) -> Result<ExactSkillWindow, ChartError> {
        let ordinary = self.compile_exact_skill_window(activation, skill)?;
        if activation == 0 || self.warning.is_empty() {
            return Ok(ordinary);
        }
        let previous = self.nodes[self.skill_node_indices[activation - 1]].time;
        let trigger = self.nodes[self.skill_node_indices[activation]].time;
        let mut scheduler = SkillScheduler::new(self.uses_ideal_60fps_timing());
        scheduler.schedule(previous, predecessor_duration);
        let scheduled = scheduler.schedule(trigger, skill.duration);
        Ok(self.scheduled_skill_window(activation, scheduled, skill))
    }

    pub(crate) fn conditional_skill_meta(
        &self,
        skill: TeamCardSkill,
    ) -> Result<ConditionalSkillMeta, ChartError> {
        let mut values = [[0.0; 17]; 6];
        for (position, row) in values.iter_mut().enumerate() {
            let mut last_range = None;
            let mut last_value = 0.0;
            for (index, &duration) in QUEUE_DURATIONS.iter().enumerate() {
                let window = self.conditional_skill_window(position, skill, duration)?;
                let range = (window.range_start, window.range_end);
                if last_range != Some(range) {
                    let mut multiplier = 1.0 + skill.score_up;
                    last_value = 0.0;
                    for note in window.range_start..window.range_end {
                        if skill.rateup && sgn(multiplier - 2.5) < 0 {
                            multiplier += 0.005;
                        }
                        last_value += self.score_factors[note]
                            * self.fever_multiplier_at_node(note)
                            * (multiplier - 1.0);
                    }
                    last_range = Some(range);
                }
                row[index] = last_value;
            }
        }
        Ok(values)
    }

    /// Only used as an upper bound. Conditional windows include every supported
    /// predecessor duration; never use this maximum as a dominance comparison.
    pub(crate) fn skill_meta_upper_values(
        &self,
        skill: TeamCardSkill,
    ) -> Result<[f64; 6], ChartError> {
        if self.warning.is_empty() {
            let mut result = [0.0; 6];
            for (position, value) in result.iter_mut().enumerate() {
                *value = self.skill_meta_value(position, skill)?;
            }
            return Ok(result);
        }
        // A two-duration window is not an upper bound on a chained queue.
        if self.skill_queue_kind(8.0) == SkillQueueKind::Chain {
            if let (Some(machine), Some(d)) =
                (self.queue_state_machine(), window_duration_index(skill))
            {
                let values = machine.window_meta(self, skill);
                return Ok(std::array::from_fn(|p| {
                    machine.layers[p]
                        .iter()
                        .map(|row| values[row[d].window])
                        .fold(0.0, f64::max)
                }));
            }
            // Tolerated near-grid durations use the exact timeline for scoring.
            // All judgments is a conservative bound when geometry cannot cache.
            self.compile_exact_skill_window(0, skill)?;
            let multiplier = if skill.rateup {
                skill.score_up.max(1.505)
            } else {
                skill.score_up
            };
            let upper = self
                .score_factors
                .iter()
                .enumerate()
                .map(|(i, f)| f * self.fever_multiplier_at_node(i) * multiplier)
                .sum();
            return Ok([upper; 6]);
        }
        Ok(self
            .conditional_skill_meta(skill)?
            .map(|row| row.into_iter().fold(0.0, f64::max)))
    }

    pub(crate) fn predecessor_skill_score_matrix(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        scratch: &mut ExactScoreScratch,
    ) -> Result<Option<PredecessorSkillScoreMatrix>, ChartError> {
        let duration = team.iter().map(|s| s.duration).fold(0.0, f64::max);
        if self.skill_queue_kind(duration) != SkillQueueKind::Single {
            return Ok(None);
        }
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        if compressed::native_kernel_enabled() && avx2_available() {
            // SAFETY: runtime AVX2 detection guards the specialized kernel.
            return unsafe { self.predecessor_matrix_avx2(team, stat, is_medley, scratch) }
                .map(Some);
        }
        self.predecessor_matrix_impl(team, stat, is_medley, scratch)
            .map(Some)
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[target_feature(enable = "avx2")]
    unsafe fn predecessor_matrix_avx2(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        scratch: &mut ExactScoreScratch,
    ) -> Result<PredecessorSkillScoreMatrix, ChartError> {
        self.predecessor_matrix_impl(team, stat, is_medley, scratch)
    }

    #[inline(always)]
    fn predecessor_matrix_impl(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        scratch: &mut ExactScoreScratch,
    ) -> Result<PredecessorSkillScoreMatrix, ChartError> {
        let compressed = !self.fever_enabled && self.score_as_medley == is_medley;
        let mut base = 0;
        if compressed {
            scratch.base_scores.resize(self.score_factor_runs.len(), 0);
            for (run, value) in self.score_factor_runs.iter().zip(&mut scratch.base_scores) {
                *value = (stat as f64 * self.score_factors[run.start]).floor() as i32;
                base += *value * (run.end - run.start) as i32;
            }
        } else {
            base = self.exact_no_skill_score_i32(stat, is_medley)?;
        }
        let duration_indices = team.map(window_duration_index);
        let cached_durations = duration_indices.iter().all(Option::is_some);
        let same_predecessor = team[1..]
            .iter()
            .all(|skill| skill.duration.to_bits() == team[0].duration.to_bits());
        let mut deltas = [[[0; 5]; 5]; 6];
        for (current, &skill) in team.iter().enumerate() {
            if let Some(equal) = (0..current).find(|&index| {
                team[index].duration.to_bits() == skill.duration.to_bits()
                    && team[index].score_up.to_bits() == skill.score_up.to_bits()
                    && team[index].rateup == skill.rateup
            }) {
                for position in &mut deltas {
                    for previous in position {
                        previous[current] = previous[equal];
                    }
                }
                continue;
            }
            let mut score_window = |window: ExactSkillWindow| {
                if compressed {
                    let profile = &mut scratch.rateup_profiles[current];
                    if skill.rateup {
                        profile.prepare(skill.score_up, window.range_end - window.range_start);
                    }
                    let mut total = 0;
                    for index in window.run_start..window.run_end {
                        let run = &self.score_factor_runs[index];
                        let start = run.start.max(window.range_start);
                        let end = run.end.min(window.range_end);
                        let base = scratch.base_scores[index];
                        if skill.rateup {
                            total += compressed::constant_base_rateup_delta(
                                base,
                                &profile.multipliers
                                    [start - window.range_start..end - window.range_start],
                            );
                        } else {
                            total += ((base as f64 * (1.0 + skill.score_up)).floor() as i32 - base)
                                * (end - start) as i32;
                        }
                    }
                    total
                } else {
                    self.skill_delta_for_window_i32(window, stat, is_medley)
                }
            };
            for (position, matrix) in deltas.iter_mut().enumerate() {
                // Most activations cannot queue. Their window is identical for
                // all predecessors, so evaluate and broadcast one value. The
                // table shortcut only applies when every duration was validated.
                let independent = same_predecessor
                    || (cached_durations
                        && self.queue_windows.as_ref().is_some_and(|cache| {
                            cache.predecessor_independent[duration_indices[current].unwrap()]
                                [position]
                        }));
                if independent {
                    let window = if let (Some(cache), Some(c), Some(p)) = (
                        &self.queue_windows,
                        duration_indices[current],
                        duration_indices[0],
                    ) {
                        cache.windows[c][position][p].with_skill(skill)
                    } else {
                        self.conditional_skill_window_uncached(position, skill, team[0].duration)?
                    };
                    let value = score_window(window);
                    for previous in matrix {
                        previous[current] = value;
                    }
                    continue;
                }
                let mut ranges = [(0, 0); 5];
                let mut values = [0; 5];
                let mut count = 0;
                for (previous, predecessor) in team.iter().enumerate() {
                    let window = if let (Some(cache), Some(c), Some(p)) = (
                        &self.queue_windows,
                        duration_indices[current],
                        duration_indices[previous],
                    ) {
                        cache.windows[c][position][p].with_skill(skill)
                    } else {
                        self.conditional_skill_window_uncached(
                            position,
                            skill,
                            predecessor.duration,
                        )?
                    };
                    let range = (window.range_start, window.range_end);
                    let value =
                        if let Some(index) = ranges[..count].iter().position(|key| *key == range) {
                            values[index]
                        } else {
                            let value = score_window(window);
                            ranges[count] = range;
                            values[count] = value;
                            count += 1;
                            value
                        };
                    matrix[previous][current] = value;
                }
            }
        }
        Ok(PredecessorSkillScoreMatrix { base, deltas })
    }
}
