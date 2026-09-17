use super::*;
use crate::model::chart::{ConditionalSkillMeta, QUEUE_DURATIONS};
mod chain;

#[derive(Debug)]
pub(super) enum QueuedChartModels {
    Predecessor(PredecessorChartModels),
    State(chain::ChainModels),
    Unsupported,
}

impl QueuedChartModels {
    pub(super) fn margin(&self, left: usize, right: usize, role: usize) -> Option<f64> {
        match self {
            Self::Predecessor(model) => model.margin(left, right, role),
            Self::State(model) => model.margin(left, right, role),
            Self::Unsupported => None,
        }
    }
}

#[derive(Debug)]
pub(super) struct QueuedModels {
    pub(super) charts: Vec<Option<QueuedChartModels>>,
}

#[derive(Debug)]
pub(super) struct PredecessorChartModels {
    cards: Vec<Option<QueuedCard>>,
    predecessors: Vec<usize>,
    neighbor: [[ValueRange; 17]; 6],
    neighbor_any: [ValueRange; 6],
    // Lower bound on the successor's meta change when its predecessor changes
    // duration. Both sides use the SAME successor, not independent extrema.
    successor_delta: [[[f64; 17]; 17]; 6],
    base_meta: f64,
    rounding_budget: f64,
    stat_low: [f64; 10],
    stat_high: f64,
}

#[derive(Debug)]
struct QueuedCard {
    stat: f64,
    duration: usize,
    meta: ConditionalSkillMeta,
}

impl QueuedModels {
    pub(super) fn new(
        owner: &MedleyContributionDominance<'_>,
        signature: MedleyPruneSignature,
        context: &SignatureContributionContextBounds,
    ) -> Self {
        let charts = owner
            .charts
            .iter()
            .enumerate()
            .map(|(chart_idx, chart)| {
                if owner
                    .profiles
                    .iter()
                    .all(|profile| !profile.has_queued_windows())
                    || chart.warning.is_empty()
                {
                    return None;
                }
                let max_duration = owner
                    .cards
                    .iter()
                    .filter(|c| signature.allows(c))
                    .map(|c| c.skill.duration)
                    .fold(0.0, f64::max);
                if chart.skill_queue_kind(max_duration) == crate::SkillQueueKind::Chain {
                    return Some(
                        chain::ChainModels::new(owner, chart_idx, signature, context)
                            .map(QueuedChartModels::State)
                            .unwrap_or(QueuedChartModels::Unsupported),
                    );
                }
                let cards: Vec<_> = owner
                    .cards
                    .iter()
                    .enumerate()
                    .map(|(idx, card)| {
                        if !signature.allows(card) {
                            return None;
                        }
                        let score_up = card
                            .score_up
                            .resolve(signature.team_band_id(), signature.team_attribute());
                        Some(QueuedCard {
                            stat: owner.profiles[idx].stat,
                            duration: QUEUE_DURATIONS
                                .iter()
                                .position(|&d| d == card.skill.duration)?,
                            meta: *owner.profiles[idx].conditional_meta(score_up, chart_idx)?,
                        })
                    })
                    .collect();
                let mut predecessors: Vec<_> = cards.iter().flatten().map(|c| c.duration).collect();
                if predecessors.is_empty() {
                    return None;
                }
                predecessors.sort_unstable();
                predecessors.dedup();
                let empty = ValueRange {
                    low: f64::INFINITY,
                    high: 0.0,
                };
                let mut neighbor = [[empty; 17]; 6];
                let mut successor_delta = [[[f64::INFINITY; 17]; 17]; 6];
                // Meta profiles repeat across many cards. Collapse them before this
                // small duration-pair precomputation; no per-team neighbor enumeration.
                let mut unique: Vec<&ConditionalSkillMeta> = Vec::new();
                for card in cards.iter().flatten() {
                    if !unique.iter().any(|meta| **meta == card.meta) {
                        unique.push(&card.meta);
                    }
                }
                for meta in unique {
                    for p in 0..6 {
                        for &a in &predecessors {
                            neighbor[p][a].low = neighbor[p][a].low.min(meta[p][a]);
                            neighbor[p][a].high = neighbor[p][a].high.max(meta[p][a]);
                            for &b in &predecessors {
                                successor_delta[p][a][b] =
                                    successor_delta[p][a][b].min(meta[p][a] - meta[p][b]);
                            }
                        }
                    }
                }
                let neighbor_any = std::array::from_fn(|p| ValueRange {
                    low: predecessors
                        .iter()
                        .map(|&d| neighbor[p][d].low)
                        .fold(f64::INFINITY, f64::min),
                    high: predecessors
                        .iter()
                        .map(|&d| neighbor[p][d].high)
                        .fold(0.0, f64::max),
                });
                let mut rounding_by_position = [0.0_f64; 6];
                let mut seen = Vec::new();
                for card in owner.cards.iter().filter(|c| signature.allows(c)) {
                    let score_up = card
                        .score_up
                        .resolve(signature.team_band_id(), signature.team_attribute());
                    let key = (
                        card.skill.duration.to_bits(),
                        card.skill.rateup,
                        score_up.to_bits(),
                    );
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.push(key);
                    let skill = TeamCardSkill {
                        score_up,
                        ..card.skill
                    };
                    let multiplier = if skill.rateup {
                        (1.0 + score_up).max(2.505)
                    } else {
                        1.0 + score_up
                    };
                    for (p, budget) in rounding_by_position.iter_mut().enumerate() {
                        for &d in &predecessors {
                            if let Ok(window) =
                                chart.conditional_skill_window(p, skill, QUEUE_DURATIONS[d])
                            {
                                *budget = budget.max(window.note_count() as f64 * multiplier);
                            }
                        }
                    }
                }
                // Covers floor(team stat), floor(base note score) and floor(skill
                // multiplier). Thus a new queued edge is safe for integer scores,
                // not just for a continuous contribution estimate.
                let rounding_budget = chart.meta.no_skill
                    + neighbor_any.iter().map(|r| r.high).sum::<f64>()
                    + chart.nodes.len() as f64
                    + rounding_by_position.iter().sum::<f64>();
                Some(QueuedChartModels::Predecessor(PredecessorChartModels {
                    cards,
                    predecessors,
                    neighbor,
                    neighbor_any,
                    successor_delta,
                    base_meta: chart.meta.no_skill,
                    rounding_budget,
                    stat_low: std::array::from_fn(|role| {
                        context.teammate_stat_low_by_chart_scenario[chart_idx * 10 + role]
                    }),
                    stat_high: context.stat_high,
                }))
            })
            .collect();
        Self { charts }
    }
}

impl PredecessorChartModels {
    pub(super) fn margin(&self, left: usize, right: usize, role: usize) -> Option<f64> {
        let a = self.cards.get(left)?.as_ref()?;
        let b = self.cards.get(right)?.as_ref()?;
        let position = role % 5;
        let captain = role >= 5;
        let delta_stat = a.stat - b.stat;
        let unchanged = |range: ValueRange| {
            delta_stat
                * if delta_stat >= 0.0 {
                    range.low
                } else {
                    range.high
                }
        };
        let mut margin = f64::INFINITY;
        // Sum of minima of affine functions is concave in teammate stat, so
        // checking the two endpoints covers its entire interval.
        for rest_stat in [self.stat_low[role], self.stat_high] {
            let sa = rest_stat + a.stat;
            let sb = rest_stat + b.stat;
            let mut value = delta_stat * self.base_meta;
            for p in 0..6 {
                if p == position || (p == 5 && captain) {
                    value += if p == 5 && position == 4 {
                        sa * a.meta[p][a.duration] - sb * b.meta[p][b.duration]
                    } else {
                        self.predecessors
                            .iter()
                            .map(|&d| sa * a.meta[p][d] - sb * b.meta[p][d])
                            .fold(f64::INFINITY, f64::min)
                    };
                } else if p == position + 1 {
                    value += unchanged(self.neighbor[p][b.duration]);
                    if a.duration != b.duration {
                        value += sa * self.successor_delta[p][a.duration][b.duration];
                    }
                } else {
                    value += unchanged(self.neighbor_any[p]);
                }
            }
            margin = margin.min(value);
        }
        Some(margin - self.rounding_budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medley::test_support::prepared_card;
    use crate::team_prune::hard::medley_card_prune_profiles;
    use crate::{Attribute, ChartNode, ChartNodeType};

    #[test]
    fn queued_contribution_edges_preserve_all_orders_and_captains() {
        check_contribution_edges([1.0, 6.0, 22.0, 27.3, 45.0, 50.0], false, false);
    }

    #[test]
    fn chained_contribution_bounds_cover_all_orders_and_captain_repeats() {
        for auto in [false, true] {
            check_contribution_edges([1.0, 6.0, 11.0, 16.3, 23.0, 29.0], auto, true);
            check_contribution_edges([1.0, 16.0, 31.0, 37.0, 44.0, 60.0], auto, true);
        }
    }

    fn check_contribution_edges(triggers: [f64; 6], auto: bool, chain: bool) {
        let mut nodes: Vec<_> = (0..300)
            .map(|i| ChartNode {
                time: i as f64 * 0.21,
                node_type: ChartNodeType::Node,
            })
            .collect();
        nodes.extend(triggers.map(|time| ChartNode {
            time,
            node_type: ChartNodeType::Skill,
        }));
        nodes.sort_by(|a, b| a.time.total_cmp(&b.time));
        let mut chart = Chart::new(27, nodes);
        chart
            .init_with_rule(
                0,
                true,
                if auto {
                    crate::ScoreRule::AUTO
                } else {
                    crate::ScoreRule::STANDARD
                },
            )
            .unwrap();
        assert_eq!(
            chart.skill_queue_kind(7.5),
            if chain {
                crate::SkillQueueKind::Chain
            } else {
                crate::SkillQueueKind::Single
            }
        );
        let mut cards: Vec<_> = (0..12)
            .map(|i| prepared_card(i + 1, if i < 8 { 1 } else { i - 6 }, 1, Attribute::Cool))
            .collect();
        let variants = [
            (60000.3, 0.8, 5.0),
            (30000.7, 1.2, 5.0),
            (50000.4, 0.9, 7.5),
            (45000.2, 1.1, 3.0),
            (40000.6, 1.5, 6.5),
            (39999.8, 1.2, 7.0),
            (40000.1, 1.0, 5.5),
            (35000.4, 1.3, 6.0),
        ];
        let stats: Vec<_> = (0..12)
            .map(|i| if i < 8 { variants[i].0 } else { 40000.1 })
            .collect();
        for (i, card) in cards.iter_mut().enumerate() {
            let (_, score_up, duration) = if i < 8 {
                variants[i]
            } else {
                (0.0, 1.3, [5.0, 6.0, 7.0, 7.5][i - 8])
            };
            card.score_up.default = score_up;
            card.skill.score_up = score_up;
            card.skill.duration = duration;
            card.skill.rateup = i == 6;
        }
        let charts = [chart];
        let profiles = medley_card_prune_profiles(&cards, &charts, &stats).unwrap();
        let mut owner = MedleyContributionDominance::new(&cards, &charts, &profiles, 0);
        let models = owner.models_for_signature(MedleyPruneSignature::Mixed);
        if let QueuedChartModels::State(model) =
            models.queued.as_ref().unwrap().charts[0].as_ref().unwrap()
        {
            model.check_shared_continuations();
        }
        // This edge trades lower skill bonus for higher stat, so hard dominance
        // alone cannot establish it: contribution pruning remains active.
        assert!(!medley_card_dominates_for_signature(
            &cards[0],
            &profiles[0],
            &cards[1],
            &profiles[1],
            MedleyPruneSignature::Mixed
        ));
        assert!(owner.card_can_replace_with_models(0, 1, &models));
        let mut edges = 0;
        for a in 0..8 {
            for b in 0..8 {
                let replaces = owner.card_can_replace_with_models(a, b, &models);
                if !replaces && (!chain || a > 3 || b > 3) {
                    continue;
                }
                edges += usize::from(replaces);
                let margins: [f64; 10] = std::array::from_fn(|role| {
                    models.queued.as_ref().unwrap().charts[0]
                        .as_ref()
                        .unwrap()
                        .margin(a, b, role)
                        .unwrap()
                });
                let ta = [
                    cards[a].skill,
                    cards[8].skill,
                    cards[9].skill,
                    cards[10].skill,
                    cards[11].skill,
                ];
                let tb = [
                    cards[b].skill,
                    cards[8].skill,
                    cards[9].skill,
                    cards[10].skill,
                    cards[11].skill,
                ];
                let sa =
                    crate::floor_team_stat([stats[a], stats[8], stats[9], stats[10], stats[11]]);
                let sb =
                    crate::floor_team_stat([stats[b], stats[8], stats[9], stats[10], stats[11]]);
                for order in &crate::skill_shuffle::tables().orders {
                    for captain in 0..5 {
                        let six = |team: &[TeamCardSkill; 5]| {
                            std::array::from_fn(|p| team[if p == 5 { captain } else { order[p] }])
                        };
                        let left = charts[0]
                            .get_score_for_six_skills(&six(&ta), sa, true)
                            .unwrap();
                        let right = charts[0]
                            .get_score_for_six_skills(&six(&tb), sb, true)
                            .unwrap();
                        let role = order.iter().position(|&c| c == 0).unwrap()
                            + if captain == 0 { 5 } else { 0 };
                        assert!(
                            margins[role] <= f64::from(left - right) + 1e-6,
                            "invalid lower bound {a}->{b} role={role}: {} > {}",
                            margins[role],
                            left - right
                        );
                        assert!(
                            !replaces || left >= right,
                            "edge={a}->{b}, order={order:?}, captain={captain}: {left} < {right}"
                        );
                    }
                }
            }
        }
        assert!(edges > 1);
    }
}
