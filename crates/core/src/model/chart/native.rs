//! Production native arithmetic using the existing scheduler.
//! This migration does not change implicit hit timing.
use super::*;
use crate::scoring::{
    skill_multiplier_from_percent, CompiledScore, Crescendo, Judgement, NoteFactors, ScoringError,
};

#[derive(Debug, Error)]
pub enum NativeChartError {
    #[error(transparent)]
    Chart(#[from] ChartError),
    #[error(transparent)]
    Arithmetic(#[from] ScoringError),
    #[error("skill duration must be finite and nonnegative")]
    InvalidDuration,
}

#[derive(Clone, Copy)]
pub(super) enum Effect {
    Plain(f32),
    Crescendo(Crescendo),
}
impl Effect {
    pub(super) fn from_parts(score_up: f64, rateup: bool) -> Self {
        let percent = (score_up * 100.0) as f32;
        if rateup {
            // Imported/custom cards are validated by preparation. Retain the
            // historical constant branch for a synthetic base above the cap.
            if percent > 150.0 {
                return Self::Plain(ordinary_multiplier(score_up));
            }
            Self::Crescendo(Crescendo::new(percent, 0.5, 150).expect("validated skill"))
        } else {
            Self::Plain(ordinary_multiplier(score_up))
        }
    }
    fn from_skill(skill: TeamCardSkill) -> Result<Self, NativeChartError> {
        if !skill.duration.is_finite() || skill.duration < 0.0 {
            return Err(NativeChartError::InvalidDuration);
        }
        // score_up is a per-skill f64 rate, never an aggregated score factor.
        skill_multiplier_from_percent((skill.score_up * 100.0) as f32)?;
        Ok(Self::from_parts(skill.score_up, skill.rateup))
    }
    pub(super) fn next_multiplier(&mut self) -> f32 {
        match self {
            Self::Plain(rate) => *rate,
            Self::Crescendo(state) => state.multiplier_for_note(Judgement::Perfect),
        }
    }
}

#[inline]
pub(super) fn ordinary_multiplier(score_up: f64) -> f32 {
    skill_multiplier_from_percent((score_up * 100.0) as f32).expect("validated skill")
}

#[inline]
pub(super) fn settle(first: i32, skill: f32, fever: f32) -> i32 {
    (first as f32 * (fever * skill)) as i32
}

impl Chart {
    /// Validate once before inner integer/SIMD accumulation. The fast kernels
    /// must not wrap or saturate malformed custom inputs.
    pub(crate) fn validate_native_domain(
        &self,
        stat: f64,
        skills: &[TeamCardSkill],
        medley: bool,
    ) -> Result<(), ChartError> {
        let power = crate::scoring::ScoringPower::new(stat as f32)?;
        let scorer = crate::scoring::NoteScorer::new(power, self.level, self.count as u32)?;
        let mut maximum = 1.0_f32;
        for skill in skills {
            if !skill.duration.is_finite() || skill.duration < 0.0 {
                return Err(ScoringError::InvalidValue("skill duration").into());
            }
            let rate = skill_multiplier_from_percent((skill.score_up * 100.0) as f32)?;
            maximum = maximum.max(if skill.rateup { rate.max(2.5) } else { rate });
        }
        let combo = combo_mod(
            self.combo + self.count as i32,
            medley,
            self.score_rule.combo_mode,
        ) as f32;
        let fever = if self.fever_enabled { 2.0 } else { 1.0 };
        let note = scorer.score_with_judgement_multiplier(
            self.score_rule.base_multiplier as f32,
            combo,
            maximum,
            fever,
        )?;
        if u64::from(note.final_score) * self.count as u64 > i32::MAX as u64 {
            return Err(ScoringError::TotalScoreOverflow.into());
        }
        Ok(())
    }

    pub(crate) fn native_base_scores_equal(&self, low: f64, high: f64, medley: bool) -> bool {
        let low = self.native_base(low);
        let high = self.native_base(high);
        if self.score_as_medley == medley {
            let low = low * self.score_rule.base_multiplier as f32;
            let high = high * self.score_rule.base_multiplier as f32;
            return self
                .score_factor_runs
                .iter()
                .all(|run| (low * run.combo) as i32 == (high * run.combo) as i32);
        }
        self.score_factor_runs.iter().all(|run| {
            self.native_first(low, run.start, medley) == self.native_first(high, run.start, medley)
        })
    }

    /// Outward allowance for comparing two continuous estimates. Only the
    /// lower-scoring side loses floors; both sides incur binary32 rounding.
    pub(crate) fn native_contribution_error(
        &self,
        linear_power: f64,
        duration: f64,
        multiplier: f64,
    ) -> f64 {
        let active = self.native_possible_skill_notes(duration);
        let floors: f64 = active
            .iter()
            .enumerate()
            .map(|(i, &active)| {
                let fever = self.fever_multiplier_at_node(i);
                // The first truncation loses <1 before Fever/skill. An active
                // note may also lose <1 at the second truncation. With no skill,
                // Fever is an integer (1 or 2), so there is no second floor loss.
                fever
                    + if active {
                        fever * (multiplier - 1.0) + 1.0
                    } else {
                        0.0
                    }
            })
            .sum();
        floors + 2.0 * crate::scoring::SCORE_GAMMA * linear_power * self.meta.no_skill * multiplier
    }

    fn native_possible_skill_notes(&self, max_duration: f64) -> Vec<bool> {
        let mut active = vec![false; self.count];
        let mut scheduler = SkillScheduler::new(self.uses_ideal_60fps_timing());
        // A shorter duration cannot delay a subsequent release beyond the
        // all-longest timeline. Cover from the earliest (immediate) start to
        // that latest end. This also encloses single and chained delays, without
        // assuming all six windows can hit the chart's densest region.
        for (position, &index) in self.skill_node_indices.iter().take(6).enumerate() {
            let trigger = self.nodes[index].time;
            let latest = scheduler.schedule(trigger, max_duration);
            let window = self.scheduled_skill_window(
                position,
                ScheduledSkillWindow {
                    start: trigger,
                    end: latest.end,
                    queue_risk: false,
                },
                TeamCardSkill {
                    card_id: 0,
                    duration: max_duration,
                    score_up: 0.0,
                    rateup: false,
                },
            );
            active[window.range_start..window.range_end].fill(true);
        }
        active
    }

    #[inline]
    pub(super) fn native_base(&self, power: f64) -> f32 {
        let level = 1.0_f32 + (self.level - 5) as f32 * 0.01_f32;
        ((power as f32 * level) / self.count as f32) * 3.0_f32
    }

    #[inline]
    pub(super) fn native_first(&self, base: f32, node: usize, medley: bool) -> i32 {
        let judged = base * self.score_rule.base_multiplier as f32;
        let combo = combo_mod(
            self.combo + node as i32 + 1,
            medley,
            self.score_rule.combo_mode,
        ) as f32;
        (judged * combo) as i32
    }
    /// Resolve the existing timeline to explicit operands, then use native
    /// arithmetic. Keeping this separate makes timing changes independently testable.
    /// Medley Combo, Auto rates and AP-only skill state come from the existing
    /// mode model; this entry does not independently validate those policies.
    pub fn native_score_notes(
        &self,
        skill_order: &[TeamCardSkill],
        is_medley: bool,
    ) -> Result<Vec<NoteFactors>, NativeChartError> {
        if self.count == 0 {
            return Err(ChartError::EmptyChart.into());
        }
        let mut scheduler = SkillScheduler::new(self.uses_ideal_60fps_timing());
        let mut events = VecDeque::<(f64, Effect)>::new();
        let mut effect = Effect::Plain(1.0);
        let mut activation = 0;
        let mut notes = Vec::with_capacity(self.nodes.len());
        for (i, node) in self.nodes.iter().enumerate() {
            while events
                .front()
                .is_some_and(|(time, _)| scheduler.event_is_due(node.time, *time))
            {
                effect = events.pop_front().expect("event exists").1;
            }
            notes.push(NoteFactors {
                judgement: self.score_rule.base_multiplier as f32,
                combo: combo_mod(
                    self.combo + i as i32 + 1,
                    is_medley,
                    self.score_rule.combo_mode,
                ) as f32,
                fever: self.fever_multiplier_at_node(i) as f32,
                skill: effect.next_multiplier(),
            });
            if node.node_type != ChartNodeType::Skill {
                continue;
            }
            let skill = skill_order
                .get(activation)
                .copied()
                .ok_or(ChartError::MissingSkillProfile { activation })?;
            let next = Effect::from_skill(skill)?;
            let scheduled = scheduler.schedule(node.time, skill.duration);
            if sgn(scheduled.start - node.time) > 0 {
                events.push_back((scheduled.start, next));
            } else {
                effect = next;
            }
            events.push_back((scheduled.end, Effect::Plain(1.0)));
            activation += 1;
        }
        Ok(notes)
    }

    pub fn compile_native_score(
        &self,
        skill_order: &[TeamCardSkill],
        is_medley: bool,
    ) -> Result<CompiledScore, NativeChartError> {
        Ok(CompiledScore::new(
            self.level,
            &self.native_score_notes(skill_order, is_medley)?,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::{NoteScorer, ScoringPower};
    #[test]
    fn contribution_note_envelope_covers_immediate_single_and_chained_delays() {
        for triggers in [
            [0.001, 15.0, 30.0, 45.0, 60.0, 75.0],
            [0.001, 6.0, 25.0, 31.0, 60.0, 66.0],
            [0.001, 4.0, 8.0, 12.0, 16.0, 20.0],
        ] {
            for auto in [false, true] {
                let mut nodes = (0..700)
                    .map(|i| ChartNode {
                        time: i as f64 * 0.137 + 0.001,
                        node_type: ChartNodeType::Node,
                    })
                    .collect::<Vec<_>>();
                nodes.extend(triggers.map(|time| ChartNode {
                    time,
                    node_type: ChartNodeType::Skill,
                }));
                let mut chart = Chart::new_with_fever_section(27, nodes, Some(35.0), Some(55.0));
                chart
                    .init_with_rule_and_fever(
                        0,
                        false,
                        if auto {
                            ScoreRule::AUTO
                        } else {
                            ScoreRule::STANDARD
                        },
                        true,
                    )
                    .unwrap();
                let covered = chart.native_possible_skill_notes(7.5);
                assert!(covered.iter().filter(|&&v| v).count() < chart.count);
                let budget = chart.native_contribution_error(400000.25, 7.5, 2.5);
                for code in 0..729 {
                    let mut digits = code;
                    let skills = std::array::from_fn::<_, 6, _>(|i| {
                        let duration = [3.0, 5.5, 7.5][digits % 3];
                        digits /= 3;
                        TeamCardSkill {
                            card_id: i as u32,
                            duration,
                            score_up: 1.5,
                            rateup: false,
                        }
                    });
                    let notes = chart.native_score_notes(&skills, false).unwrap();
                    for (i, note) in notes.iter().enumerate() {
                        assert!(note.skill == 1.0 || covered[i], "uncovered active note {i}, code={code}, auto={auto}, triggers={triggers:?}");
                    }
                    let scorer = NoteScorer::new(
                        ScoringPower::new(400000.25).unwrap(),
                        chart.level,
                        chart.count as u32,
                    )
                    .unwrap();
                    let actual: u64 = notes
                        .iter()
                        .map(|n| {
                            scorer
                                .score_with_judgement_multiplier(
                                    n.judgement,
                                    n.combo,
                                    n.skill,
                                    n.fever,
                                )
                                .unwrap()
                                .final_score as u64
                        })
                        .sum();
                    let continuous: f64 = notes
                        .iter()
                        .enumerate()
                        .map(|(i, n)| {
                            400000.25
                                * chart.score_factors[i]
                                * f64::from(n.fever)
                                * f64::from(n.skill)
                        })
                        .sum();
                    assert!(
                        continuous - actual as f64 <= budget,
                        "budget={budget}, loss={}",
                        continuous - actual as f64
                    );
                }
            }
        }
    }

    #[test]
    fn new_chart_entry_reproduces_four_arithmetic_regressions() {
        for (power, count, fever, expected) in [
            (300040.75, 760, false, 1288520),
            (300149.0, 760, false, 1289040),
            (300000.0, 500, true, 2537240),
        ] {
            let mut chart = Chart::new_with_fever_section(
                27,
                (0..count)
                    .map(|i| ChartNode {
                        time: i as f64,
                        node_type: ChartNodeType::Node,
                    })
                    .collect(),
                Some(-1.0),
                Some(count as f64 + 1.0),
            );
            chart
                .init_with_rule_and_fever(0, false, ScoreRule::STANDARD, fever)
                .unwrap();
            assert_eq!(
                chart
                    .compile_native_score(&[], false)
                    .unwrap()
                    .score(ScoringPower::new(power).unwrap())
                    .unwrap(),
                expected
            );
        }
        let mut chart = Chart::new(
            27,
            (0..121)
                .map(|i| ChartNode {
                    time: i as f64 / 100.0,
                    node_type: if i == 0 {
                        ChartNodeType::Skill
                    } else {
                        ChartNodeType::Node
                    },
                })
                .collect(),
        );
        chart.init(0, false).unwrap();
        let skill = TeamCardSkill {
            card_id: 61,
            duration: 3.0,
            score_up: 1.0,
            rateup: true,
        };
        assert_eq!(
            chart
                .compile_native_score(&[skill], false)
                .unwrap()
                .score(ScoringPower::new(400000.0).unwrap())
                .unwrap(),
            3737735
        );
    }

    #[test]
    fn compiled_scores_match_note_replay_for_overlap_auto_and_medley() {
        for auto in [false, true] {
            for medley in [false, true] {
                let mut chart = Chart::new_with_fever_section(
                    27,
                    (0..1600)
                        .map(|i| ChartNode {
                            time: i as f64 / 60.0,
                            node_type: if [0, 120, 240, 360, 900, 1300].contains(&i) {
                                ChartNodeType::Skill
                            } else {
                                ChartNodeType::Node
                            },
                        })
                        .collect(),
                    Some(4.0),
                    Some(20.0),
                );
                chart
                    .init_with_rule_and_fever(
                        650,
                        medley,
                        if auto {
                            ScoreRule::AUTO
                        } else {
                            ScoreRule::STANDARD
                        },
                        true,
                    )
                    .unwrap();
                let skills = std::array::from_fn::<_, 6, _>(|i| TeamCardSkill {
                    card_id: i as u32,
                    duration: if i % 2 == 0 { 7.0 } else { 3.0 },
                    score_up: 1.0,
                    rateup: i == 1,
                });
                let notes = chart.native_score_notes(&skills, medley).unwrap();
                let compiled = chart.compile_native_score(&skills, medley).unwrap();
                let power = ScoringPower::new(300149.75).unwrap();
                let scorer = NoteScorer::new(power, 27, 1600).unwrap();
                let direct: u64 = notes
                    .iter()
                    .map(|n| {
                        scorer
                            .score_with_judgement_multiplier(n.judgement, n.combo, n.skill, n.fever)
                            .unwrap()
                            .final_score as u64
                    })
                    .sum();
                assert_eq!(compiled.score(power).unwrap(), direct);
                assert_eq!(compiled.score_scalar(power).unwrap(), direct);
                assert!(notes.iter().any(|n| n.skill > 2.0));
            }
        }
    }
}
