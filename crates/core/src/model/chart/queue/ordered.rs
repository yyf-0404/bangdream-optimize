//! Single-song search already assigns skill positions. Cache individual state
//! transition scores instead of compiling one full note timeline per permutation.
use super::*;
use std::collections::HashMap;

struct StatScores {
    base: i32,
    runs: Vec<i32>,
    deltas: HashMap<(u16, usize), i32>,
}

pub(crate) struct OrderedQueueScoreCache<'a> {
    chart: &'a Chart,
    skills: Vec<TeamCardSkill>,
    durations: Vec<usize>,
    profiles: Vec<RateUpProfileScratch>,
    stats: HashMap<i32, StatScores>,
}

impl<'a> OrderedQueueScoreCache<'a> {
    pub(crate) fn new(chart: &'a Chart, skills: &[TeamCardSkill]) -> Option<Self> {
        // The single-song caller scores with is_medley=false. A mismatched
        // chart must reach the existing validation error in the fallback.
        if chart.score_as_medley {
            return None;
        }
        chart.queue_state_machine()?;
        let durations = skills
            .iter()
            .map(|&s| window_duration_index(s))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            chart,
            skills: skills.to_vec(),
            durations,
            profiles: skills
                .iter()
                .map(|_| RateUpProfileScratch::default())
                .collect(),
            stats: HashMap::new(),
        })
    }

    pub(crate) fn score(&mut self, stat: i32, skill_ids: [u16; 6]) -> Result<i32, ChartError> {
        let chart = self.chart;
        // Bound memory independently of the number of leaves / stat values.
        if self.stats.len() >= 128 && !self.stats.contains_key(&stat) {
            self.stats.clear();
        }
        let compressed = !chart.fever_enabled;
        if !self.stats.contains_key(&stat) {
            let mut runs = Vec::new();
            let base = if compressed {
                let mut total = 0;
                for run in &chart.score_factor_runs {
                    let value = (stat as f64 * chart.score_factors[run.start]).floor() as i32;
                    total += value * (run.end - run.start) as i32;
                    runs.push(value);
                }
                total
            } else {
                chart.exact_no_skill_score_i32(stat, chart.score_as_medley)?
            };
            self.stats.insert(
                stat,
                StatScores {
                    base,
                    runs,
                    deltas: HashMap::new(),
                },
            );
        }
        let cached = self.stats.get_mut(&stat).unwrap();
        let machine = chart.queue_state_machine().unwrap();
        let mut state = 0;
        let mut score = cached.base;
        for (p, id) in skill_ids.into_iter().enumerate() {
            let index = usize::from(id);
            let edge = machine.layers[p][state][self.durations[index]];
            let delta = cached.deltas.entry((id, edge.window)).or_insert_with(|| {
                let range = machine.window_range(edge.window);
                let window = range.with_skill(self.skills[index]);
                if compressed {
                    chart.skill_delta_from_runs(window, &cached.runs, &mut self.profiles[index])
                } else {
                    chart.skill_delta_for_window_i32(window, stat, chart.score_as_medley)
                }
            });
            score += *delta;
            state = edge.next;
        }
        Ok(score)
    }
}

impl Chart {
    pub(super) fn skill_delta_from_runs(
        &self,
        window: ExactSkillWindow,
        base_scores: &[i32],
        profile: &mut RateUpProfileScratch,
    ) -> i32 {
        if window.rateup {
            profile.prepare(window.score_up, window.range_end - window.range_start);
        }
        let mut value = 0;
        for index in window.run_start..window.run_end {
            let run = &self.score_factor_runs[index];
            let start = run.start.max(window.range_start);
            let end = run.end.min(window.range_end);
            let base = base_scores[index];
            value += if window.rateup {
                compressed::constant_base_rateup_delta(
                    base,
                    &profile.multipliers[start - window.range_start..end - window.range_start],
                )
            } else {
                ((base as f64 * (1.0 + window.score_up)).floor() as i32 - base)
                    * (end - start) as i32
            };
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_state_cache_matches_all_timeline_orders_and_survives_eviction() {
        for auto in [false, true] {
            for fever in [false, true] {
                for triggers in [
                    [1.003, 6.011, 11.023, 16.031, 21.003, 26.0],
                    [1.0, 16.0, 31.0, 37.0, 55.0, 75.0],
                ] {
                    let mut chart = super::super::tests::chart(triggers, auto);
                    chart.fever_start = Some(7.2);
                    chart.fever_end = Some(35.0);
                    chart
                        .init_with_rule_and_fever(0, false, chart.score_rule, fever)
                        .unwrap();
                    let skills = std::array::from_fn::<_, 5, _>(|i| TeamCardSkill {
                        card_id: i as u32 + 1,
                        duration: [5.0, 6.5, 7.0, 7.5, 8.0][i],
                        score_up: [0.9, 1.2, 1.5, 1.1, 1.3][i],
                        rateup: i == 1 || i == 2,
                    });
                    let mut cache = OrderedQueueScoreCache::new(&chart, &skills).unwrap();
                    for stat in [1, 301_337] {
                        for &order in &crate::skill_shuffle::tables().orders {
                            for captain in 0..5 {
                                let ids = std::array::from_fn(|p| {
                                    if p < 5 {
                                        order[p] as u16
                                    } else {
                                        captain
                                    }
                                });
                                let expected = chart
                                    .get_score_for_six_skills(
                                        &ids.map(|i| skills[i as usize]),
                                        stat,
                                        false,
                                    )
                                    .unwrap();
                                assert_eq!(
                                    cache.score(stat, ids).unwrap(),
                                    expected,
                                    "auto={auto} fever={fever} stat={stat} ids={ids:?}"
                                );
                            }
                        }
                    }
                    for stat in 100_000..100_130 {
                        let ids = [0, 1, 2, 3, 4, 2];
                        assert_eq!(
                            cache.score(stat, ids).unwrap(),
                            chart
                                .get_score_for_six_skills(
                                    &ids.map(|i| skills[i as usize]),
                                    stat,
                                    false
                                )
                                .unwrap()
                        );
                        assert!(cache.stats.len() <= 128);
                    }
                    let mut unsupported = skills;
                    unsupported[1].duration = 6.500_001;
                    assert!(OrderedQueueScoreCache::new(&chart, &unsupported).is_none());
                }
            }
        }
    }
}
