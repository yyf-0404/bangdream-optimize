//! Exact six-layer transducer. States are equivalent only if every remaining
//! duration produces the same judgment interval AND an equivalent successor.
use super::*;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct Transition {
    pub(crate) window: usize,
    pub(crate) next: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueueStateMachine {
    pub(crate) layers: [Vec<[Transition; 17]>; 6],
    windows: Vec<WindowRange>,
    raw_counts: [usize; 6],
}

fn scheduler_key(scheduler: SkillScheduler) -> [u64; 3] {
    match scheduler {
        SkillScheduler::Ideal60(s) => [
            s.queue_until_frame.map_or(u64::MAX, |v| v as u64),
            s.next_queued_start_frame.map_or(u64::MAX, |v| v as u64),
            0,
        ],
        SkillScheduler::Envelope(s) => [
            s.must_queue_until.map_or(u64::MAX, f64::to_bits),
            s.may_queue_until.map_or(u64::MAX, f64::to_bits),
            s.scheduled_end.map_or(u64::MAX, f64::to_bits),
        ],
    }
}

impl QueueStateMachine {
    pub(super) fn new(chart: &Chart) -> Result<Self, ChartError> {
        let mut states = vec![SkillScheduler::new(chart.uses_ideal_60fps_timing())];
        let mut layers: [Vec<[Transition; 17]>; 6] = std::array::from_fn(|_| Vec::new());
        let mut windows = Vec::new();
        let mut window_ids = HashMap::new();
        for (p, layer) in layers.iter_mut().enumerate() {
            let trigger = chart.nodes[chart.skill_node_indices[p]].time;
            let mut next_states = Vec::new();
            let mut next_ids = HashMap::new();
            for &state in &states {
                let mut row = [Transition::default(); 17];
                for (d, &duration) in QUEUE_DURATIONS.iter().enumerate() {
                    let mut next = state;
                    let scheduled = next.schedule(trigger, duration);
                    let range = chart.scheduled_window_range(p, scheduled);
                    let window = *window_ids.entry(range).or_insert_with(|| {
                        windows.push(range);
                        windows.len() - 1
                    });
                    let next_id = if p == 5 {
                        0
                    } else {
                        *next_ids.entry(scheduler_key(next)).or_insert_with(|| {
                            next_states.push(next);
                            next_states.len() - 1
                        })
                    };
                    row[d] = Transition {
                        window,
                        next: next_id,
                    };
                }
                layer.push(row);
            }
            states = next_states;
        }
        let raw_counts = std::array::from_fn(|p| layers[p].len());
        let mut next_classes = vec![0];
        for p in (0..6).rev() {
            let mut signatures = HashMap::new();
            let mut representatives = Vec::new();
            let mut classes = Vec::with_capacity(layers[p].len());
            for mut row in std::mem::take(&mut layers[p]) {
                for edge in &mut row {
                    edge.next = next_classes[edge.next];
                }
                let id = *signatures.entry(row).or_insert_with(|| {
                    representatives.push(row);
                    representatives.len() - 1
                });
                classes.push(id);
            }
            layers[p] = representatives;
            next_classes = classes;
        }
        Ok(Self {
            layers,
            windows,
            raw_counts,
        })
    }

    pub(crate) fn window_meta(&self, chart: &Chart, skill: TeamCardSkill) -> Vec<f64> {
        self.windows
            .iter()
            .map(|range| chart.meta_for_exact_window(range.with_skill(skill)))
            .collect()
    }

    pub(crate) fn max_note_count(&self) -> usize {
        self.windows
            .iter()
            .map(|w| w.end - w.start)
            .max()
            .unwrap_or(0)
    }

    pub(super) fn window_range(&self, id: usize) -> WindowRange {
        self.windows[id]
    }
}

impl Chart {
    pub(in crate::model::chart) fn scheduled_skill_window(
        &self,
        p: usize,
        scheduled: ScheduledSkillWindow,
        skill: TeamCardSkill,
    ) -> ExactSkillWindow {
        self.scheduled_window_range(p, scheduled).with_skill(skill)
    }

    fn scheduled_window_range(&self, p: usize, scheduled: ScheduledSkillWindow) -> WindowRange {
        let index = self.skill_node_indices[p];
        // Match the timeline's immediate activation branch exactly, including
        // skill-tagged judgments and notes with the same timestamp.
        let start = if sgn(scheduled.start - self.nodes[index].time) > 0 {
            self.nodes
                .partition_point(|n| !self.is_after_scoring_boundary(n.time, scheduled.start))
        } else {
            index + 1
        };
        let end = self
            .nodes
            .partition_point(|n| !self.is_after_scoring_boundary(n.time, scheduled.end));
        WindowRange {
            start,
            end,
            run_start: self
                .score_factor_runs
                .partition_point(|run| run.end <= start),
            run_end: self
                .score_factor_runs
                .partition_point(|run| run.start < end),
        }
    }

    pub(crate) fn queue_state_machine(&self) -> Option<&Arc<QueueStateMachine>> {
        self.queue_windows.as_ref().map(|cache| &cache.machine)
    }

    pub fn queue_state_counts(&self) -> Option<([usize; 6], [usize; 6])> {
        self.queue_state_machine()
            .map(|m| (m.raw_counts, std::array::from_fn(|p| m.layers[p].len())))
    }

    /// Exact automatic queue search uses the validated discrete duration alphabet.
    /// Other imported durations keep timeline scoring and a conservative notice.
    pub fn supports_exact_queue_search(&self, durations: impl IntoIterator<Item = f64>) -> bool {
        self.queue_state_machine().is_some()
            && durations.into_iter().all(|d| duration_index(d).is_some())
    }

    fn meta_for_exact_window(&self, window: ExactSkillWindow) -> f64 {
        let mut multiplier = 1.0 + window.score_up;
        let mut value = 0.0;
        for note in window.range_start..window.range_end {
            if window.rateup && sgn(multiplier - 2.5) < 0 {
                multiplier += 0.005;
            }
            value +=
                self.score_factors[note] * self.fever_multiplier_at_node(note) * (multiplier - 1.0);
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimized_states_preserve_all_duration_transitions() {
        for auto in [false, true] {
            for triggers in [
                [1.0, 20.0, 30.0, 40.0, 50.0, 60.0],
                [1.003, 6.011, 11.023, 16.031, 21.003, 26.0],
                [1.0, 16.0, 31.0, 37.0, 44.0, 65.0],
            ] {
                let chart = super::super::tests::chart(triggers, auto);
                let machine = QueueStateMachine::new(&chart).unwrap();
                let mut random = 77u64;
                for _ in 0..3000 {
                    let mut scheduler = SkillScheduler::new(!auto);
                    let mut state = 0;
                    for (p, &trigger) in triggers.iter().enumerate() {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        let d = (random >> 32) as usize % 17;
                        let scheduled = scheduler.schedule(trigger, QUEUE_DURATIONS[d]);
                        let edge = machine.layers[p][state][d];
                        assert_eq!(
                            machine.windows[edge.window],
                            chart.scheduled_window_range(p, scheduled)
                        );
                        state = edge.next;
                    }
                }
                assert!(machine
                    .layers
                    .iter()
                    .enumerate()
                    .all(|(p, layer)| layer.len() <= machine.raw_counts[p]));
            }
        }
    }

    #[test]
    fn chain_states_match_all_orders_captains_and_weighted_scores() {
        for auto in [false, true] {
            for fever in [false, true] {
                let mut chart =
                    super::super::tests::chart([1.003, 6.011, 11.023, 16.031, 21.003, 26.0], auto);
                if fever {
                    chart.fever_start = Some(7.2);
                    chart.fever_end = Some(35.0);
                    chart
                        .init_with_rule_and_fever(1234, true, chart.score_rule, true)
                        .unwrap();
                }
                let skills = std::array::from_fn(|i| TeamCardSkill {
                    card_id: i as u32 + 1,
                    duration: [5.0, 6.5, 7.0, 7.5, 8.0][i],
                    score_up: [0.9, 1.2, 1.5, 1.1, 1.3][i],
                    rateup: i == 1 || i == 2,
                });
                for stat in [1, 12345, 345678] {
                    let scores = chart
                        .state_skill_scores(&skills, stat, true, &mut ExactScoreScratch::default())
                        .unwrap()
                        .unwrap();
                    let mut brute = i32::MIN;
                    let mut expected = [[0; 120]; 5];
                    for (i, &order) in crate::skill_shuffle::tables().orders.iter().enumerate() {
                        for captain in 0..5 {
                            let six = std::array::from_fn(|p| {
                                skills[if p == 5 { captain } else { order[p] }]
                            });
                            let value = chart.get_score_for_six_skills(&six, stat, true).unwrap();
                            assert_eq!(
                                scores.score(order, captain),
                                value,
                                "{auto} {fever} {stat} {order:?} {captain}"
                            );
                            expected[captain][i] = value;
                            brute = brute.max(value);
                        }
                    }
                    assert_eq!(scores.maximum([4, 3, 2, 1, 0], 2).score, brute);
                    assert_eq!(
                        crate::skill_shuffle::exact_order_scores(&chart, &skills, stat, true)
                            .unwrap(),
                        expected
                    );
                    let windows = skills.map(|s| chart.compile_exact_skill_windows(s).unwrap());
                    let result = chart
                        .get_max_score_order_from_exact_windows(
                            &skills,
                            stat,
                            true,
                            [4, 3, 2, 1, 0],
                            2,
                            &windows,
                            &mut ExactScoreScratch::default(),
                        )
                        .unwrap();
                    let reference = chart
                        .exhaustive_max_score_order_fallback(
                            &skills,
                            stat,
                            true,
                            [4, 3, 2, 1, 0],
                            2,
                        )
                        .unwrap();
                    assert_eq!(
                        (result.score, result.order_indices, result.captain_index),
                        (
                            reference.score,
                            reference.order_indices,
                            reference.captain_index
                        )
                    );
                    let upper = skills.map(|s| chart.skill_meta_upper_values(s).unwrap());
                    for p in 0..6 {
                        for card in 0..5 {
                            let d = duration_index(skills[card].duration).unwrap();
                            let machine = chart.queue_state_machine().unwrap();
                            let meta = machine.window_meta(&chart, skills[card]);
                            assert!(machine.layers[p]
                                .iter()
                                .all(|row| meta[row[d].window] <= upper[card][p]));
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PlanEdge {
    next: usize,
    term: usize,
    card: usize,
}

#[derive(Debug)]
struct PlanNode {
    position: usize,
    edges: Vec<PlanEdge>,
}

#[derive(Debug)]
struct TeamPlan {
    nodes: Vec<PlanNode>,
    terms: Vec<(usize, WindowRange)>,
}

impl TeamPlan {
    fn new(machine: &QueueStateMachine, durations: [usize; 5]) -> Self {
        let mut keys = vec![(0u8, 0usize)];
        let mut ids = HashMap::from([((0u8, 0usize), 0)]);
        let mut terms = Vec::new();
        let mut term_ids = HashMap::new();
        let mut nodes = Vec::new();
        let mut i = 0;
        while i < keys.len() {
            let (mask, state) = keys[i];
            let p = mask.count_ones() as usize;
            let mut edges = Vec::new();
            for (card, &duration) in durations.iter().enumerate() {
                if p != 5 && mask & (1 << card) != 0 {
                    continue;
                }
                let edge = machine.layers[p][state][duration];
                let next = if p == 5 {
                    0
                } else {
                    *ids.entry((mask | (1 << card), edge.next))
                        .or_insert_with(|| {
                            keys.push((mask | (1 << card), edge.next));
                            keys.len() - 1
                        })
                };
                let term = *term_ids.entry((card, edge.window)).or_insert_with(|| {
                    terms.push((card, machine.windows[edge.window]));
                    terms.len() - 1
                });
                edges.push(PlanEdge { next, term, card });
            }
            nodes.push(PlanNode { position: p, edges });
            i += 1;
        }
        Self { nodes, terms }
    }
}

#[derive(Debug, Default)]
pub(in crate::model::chart) struct StatePlanCache {
    charts: Vec<(Arc<QueueStateMachine>, HashMap<[usize; 5], Arc<TeamPlan>>)>,
}

impl StatePlanCache {
    fn plan(&mut self, machine: &Arc<QueueStateMachine>, durations: [usize; 5]) -> Arc<TeamPlan> {
        let index = self
            .charts
            .iter()
            .position(|(m, _)| Arc::ptr_eq(m, machine))
            .unwrap_or_else(|| {
                self.charts.push((Arc::clone(machine), HashMap::new()));
                self.charts.len() - 1
            });
        let plans = &mut self.charts[index].1;
        if plans.len() >= 1024 && !plans.contains_key(&durations) {
            plans.clear();
        }
        Arc::clone(
            plans
                .entry(durations)
                .or_insert_with(|| Arc::new(TeamPlan::new(machine, durations))),
        )
    }
}

pub(crate) struct StateSkillScores {
    plan: Arc<TeamPlan>,
    values: Vec<i32>,
    base: i32,
}

impl StateSkillScores {
    pub(crate) fn score(&self, order: [usize; 5], captain: usize) -> i32 {
        let mut node = 0;
        let mut score = self.base;
        for card in order.into_iter().chain([captain]) {
            let edge = self.plan.nodes[node]
                .edges
                .iter()
                .find(|e| e.card == card)
                .unwrap();
            score += self.values[edge.term];
            node = edge.next;
        }
        score
    }

    pub(crate) fn maximum(&self, seed: [usize; 5], captain: usize) -> ExactSkillOrder {
        const SCALE: i64 = 1 << 18;
        let mut best = vec![0i64; self.plan.nodes.len()];
        for (i, node) in self.plan.nodes.iter().enumerate().rev() {
            best[i] = node
                .edges
                .iter()
                .map(|e| {
                    i64::from(self.values[e.term]) * SCALE
                        - ((e.card as i64) << (3 * (5 - node.position)))
                        + if node.position == 5 { 0 } else { best[e.next] }
                })
                .max()
                .unwrap();
        }
        let delta = (best[0] + SCALE - 1) >> 18;
        let path = (delta * SCALE - best[0]) as usize;
        let score = self.base + delta as i32;
        // The old exhaustive chain path preserves a tied seed.
        if self.score(seed, captain) == score {
            return ExactSkillOrder {
                score,
                order_indices: seed,
                captain_index: captain,
            };
        }
        ExactSkillOrder {
            score,
            order_indices: std::array::from_fn(|p| (path >> (3 * (5 - p))) & 7),
            captain_index: path & 7,
        }
    }
}

impl Chart {
    pub(crate) fn state_skill_scores(
        &self,
        team: &[TeamCardSkill; 5],
        stat: i32,
        is_medley: bool,
        scratch: &mut ExactScoreScratch,
    ) -> Result<Option<StateSkillScores>, ChartError> {
        let Some(machine) = self.queue_state_machine() else {
            return Ok(None);
        };
        let mut durations = [0; 5];
        for (i, &skill) in team.iter().enumerate() {
            let Some(d) = window_duration_index(skill) else {
                self.compile_exact_skill_window(0, skill)?;
                return Ok(None);
            };
            durations[i] = d;
        }
        let plan = scratch.queue_plans.plan(machine, durations);
        let compressed = !self.fever_enabled && self.score_as_medley == is_medley;
        let base = if compressed {
            scratch.base_scores.resize(self.score_factor_runs.len(), 0);
            let mut total = 0;
            for (run, value) in self.score_factor_runs.iter().zip(&mut scratch.base_scores) {
                *value = (stat as f64 * self.score_factors[run.start]).floor() as i32;
                total += *value * (run.end - run.start) as i32;
            }
            total
        } else {
            self.exact_no_skill_score_i32(stat, is_medley)?
        };
        let values = plan
            .terms
            .iter()
            .map(|&(card, range)| {
                let skill = team[card];
                let window = range.with_skill(skill);
                if !compressed {
                    return self.skill_delta_for_window_i32(window, stat, is_medley);
                }
                self.skill_delta_from_runs(
                    window,
                    &scratch.base_scores,
                    &mut scratch.rateup_profiles[card],
                )
            })
            .collect();
        Ok(Some(StateSkillScores { plan, values, base }))
    }
}
