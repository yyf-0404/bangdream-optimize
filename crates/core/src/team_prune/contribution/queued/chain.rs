//! Coupled timing states for contribution dominance. Both histories always use
//! the same subsequent skill; independent window extrema are not subtracted.
use super::*;
use crate::model::chart::QueueStateMachine;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug)]
struct Shape {
    duration: usize,
    meta: Vec<f64>,
}

type PairState = (usize, usize, usize);

#[derive(Debug, Clone, Copy)]
struct PairEdge {
    delta: f64,
    next_a: usize,
    next_b: usize,
}

#[derive(Debug, Default)]
struct Continuations {
    ordinary: HashMap<PairState, f64>,
    captain: HashMap<(PairState, usize, usize), f64>,
    edges: HashMap<PairState, Arc<[PairEdge]>>,
}

#[derive(Debug)]
pub(in crate::team_prune::contribution) struct ChainModels {
    machine: Arc<QueueStateMachine>,
    shapes: Vec<Shape>,
    cards: Vec<Option<(f64, usize)>>,
    total_meta: Vec<[ValueRange; 10]>,
    // Timing loss is independent of card/team stats and reused by skill shape.
    differences: RefCell<HashMap<(usize, usize, usize), f64>>,
    reachable: [Vec<usize>; 6],
    continuations: RefCell<Continuations>,
    base: f64,
    rounding: f64,
    rest_low: [f64; 10],
    rest_high: f64,
}

impl ChainModels {
    pub(super) fn new(
        owner: &MedleyContributionDominance<'_>,
        chart_idx: usize,
        signature: MedleyPruneSignature,
        context: &SignatureContributionContextBounds,
    ) -> Option<Self> {
        let chart = &owner.charts[chart_idx];
        let machine = Arc::clone(chart.queue_state_machine()?);
        let mut shapes = Vec::new();
        let mut keys = Vec::new();
        let mut cards = Vec::with_capacity(owner.cards.len());
        let mut multiplier = 1.0_f64;
        for (i, card) in owner.cards.iter().enumerate() {
            if !signature.allows(card) {
                cards.push(None);
                continue;
            }
            let duration = QUEUE_DURATIONS
                .iter()
                .position(|&d| d == card.skill.duration)?;
            let score_up = card
                .score_up
                .resolve(signature.team_band_id(), signature.team_attribute());
            let key = (duration, score_up.to_bits(), card.skill.rateup);
            let shape = keys.iter().position(|&k| k == key).unwrap_or_else(|| {
                keys.push(key);
                shapes.push(Shape {
                    duration,
                    meta: machine.window_meta(
                        chart,
                        TeamCardSkill {
                            score_up,
                            ..card.skill
                        },
                    ),
                });
                shapes.len() - 1
            });
            multiplier = multiplier.max(if card.skill.rateup {
                (1.0 + score_up).max(2.505)
            } else {
                1.0 + score_up
            });
            cards.push(Some((owner.profiles[i].stat, shape)));
        }
        if shapes.is_empty() {
            return None;
        }
        let mut reachable: [Vec<usize>; 6] = std::array::from_fn(|_| Vec::new());
        reachable[0].push(0);
        for p in 0..5 {
            let mut next = Vec::new();
            for &q in &reachable[p] {
                for shape in &shapes {
                    next.push(machine.layers[p][q][shape.duration].next);
                }
            }
            next.sort_unstable();
            next.dedup();
            reachable[p + 1] = next;
        }
        let total_meta: Vec<[ValueRange; 10]> = (0..shapes.len())
            .map(|shape| {
                std::array::from_fn(|role| {
                    let mut next = vec![ValueRange {
                        low: 0.0,
                        high: 0.0,
                    }];
                    for p in (0..6).rev() {
                        let forced = p == role % 5 || (role >= 5 && p == 5);
                        next = machine.layers[p]
                            .iter()
                            .map(|row| {
                                let mut range = ValueRange {
                                    low: f64::INFINITY,
                                    high: f64::NEG_INFINITY,
                                };
                                for (i, s) in shapes.iter().enumerate() {
                                    if forced && i != shape {
                                        continue;
                                    }
                                    let edge = row[s.duration];
                                    range.low =
                                        range.low.min(s.meta[edge.window] + next[edge.next].low);
                                    range.high =
                                        range.high.max(s.meta[edge.window] + next[edge.next].high);
                                }
                                range
                            })
                            .collect();
                    }
                    next[0]
                })
            })
            .collect();
        let max_meta = total_meta
            .iter()
            .flat_map(|r| r.iter())
            .map(|r| r.high)
            .fold(0.0, f64::max);
        // Floor(stat), base-note floor and skill floor; a conservative allowance
        // for one replacement, including all six queued windows and rate-up.
        let rounding = chart.meta.no_skill
            + max_meta
            + chart.nodes.len() as f64
            + 6.0 * machine.max_note_count() as f64 * multiplier;
        Some(Self {
            machine,
            shapes,
            cards,
            total_meta,
            differences: RefCell::new(HashMap::new()),
            reachable,
            continuations: RefCell::new(Continuations::default()),
            base: chart.meta.no_skill,
            rounding,
            rest_low: std::array::from_fn(|r| {
                context.teammate_stat_low_by_chart_scenario[chart_idx * 10 + r]
            }),
            rest_high: context.stat_high,
        })
    }

    fn difference(&self, a: usize, b: usize, role: usize) -> f64 {
        if let Some(&value) = self.differences.borrow().get(&(a, b, role)) {
            return value;
        }
        let p = role % 5;
        let mut cache = self.continuations.borrow_mut();
        if cache.captain.len() > 65_536 {
            cache.captain.clear();
        }
        // Before the replacement both histories are identical. Enumerate the
        // reachable prefix states once, without repeating that prefix DP for
        // every pair of card shapes and every possible replacement position.
        let value = self.reachable[p]
            .iter()
            .map(|&q| {
                let sa = &self.shapes[a];
                let sb = &self.shapes[b];
                let ea = self.machine.layers[p][q][sa.duration];
                let eb = self.machine.layers[p][q][sb.duration];
                sa.meta[ea.window] - sb.meta[eb.window]
                    + self.continuation((p + 1, ea.next, eb.next), a, b, role >= 5, &mut cache)
            })
            .fold(f64::INFINITY, f64::min);
        self.differences.borrow_mut().insert((a, b, role), value);
        value
    }

    fn continuation(
        &self,
        key: PairState,
        a: usize,
        b: usize,
        captain: bool,
        cache: &mut Continuations,
    ) -> f64 {
        let (p, qa, qb) = key;
        if p == 6 || (!captain && qa == qb) {
            return 0.0;
        }
        let cached = if captain {
            cache.captain.get(&(key, a, b))
        } else {
            cache.ordinary.get(&key)
        };
        if let Some(&value) = cached {
            return value;
        }
        let value = if captain && p == 5 {
            let sa = &self.shapes[a];
            let sb = &self.shapes[b];
            sa.meta[self.machine.layers[p][qa][sa.duration].window]
                - sb.meta[self.machine.layers[p][qb][sb.duration].window]
        } else {
            let edges = Arc::clone(cache.edges.entry(key).or_insert_with(|| {
                let mut by_duration: [Option<PairEdge>; 17] = [None; 17];
                for shape in &self.shapes {
                    let ea = self.machine.layers[p][qa][shape.duration];
                    let eb = self.machine.layers[p][qb][shape.duration];
                    let delta = shape.meta[ea.window] - shape.meta[eb.window];
                    let edge = by_duration[shape.duration].get_or_insert(PairEdge {
                        delta,
                        next_a: ea.next,
                        next_b: eb.next,
                    });
                    edge.delta = edge.delta.min(delta);
                }
                by_duration.into_iter().flatten().collect::<Vec<_>>().into()
            }));
            edges
                .iter()
                .map(|edge| {
                    edge.delta
                        + self.continuation((p + 1, edge.next_a, edge.next_b), a, b, captain, cache)
                })
                .fold(f64::INFINITY, f64::min)
        };
        if captain {
            cache.captain.insert((key, a, b), value);
        } else {
            cache.ordinary.insert(key, value);
        }
        value
    }

    #[cfg(test)]
    fn suffix(
        &self,
        p: usize,
        qa: usize,
        qb: usize,
        a: usize,
        b: usize,
        role: usize,
        memo: &mut HashMap<(usize, usize, usize), f64>,
    ) -> f64 {
        if p == 6 {
            return 0.0;
        }
        // Convergence removes timing differences only. A captain replacement
        // must still be inserted again at activation six.
        if qa == qb && p > role % 5 && role < 5 {
            return 0.0;
        }
        if let Some(&v) = memo.get(&(p, qa, qb)) {
            return v;
        }
        let fixed = p == role % 5 || (role >= 5 && p == 5);
        let mut minimum = f64::INFINITY;
        for i in 0..if fixed { 1 } else { self.shapes.len() } {
            let sa = &self.shapes[if fixed { a } else { i }];
            let sb = &self.shapes[if fixed { b } else { i }];
            let ea = self.machine.layers[p][qa][sa.duration];
            let eb = self.machine.layers[p][qb][sb.duration];
            let value = sa.meta[ea.window] - sb.meta[eb.window]
                + self.suffix(p + 1, ea.next, eb.next, a, b, role, memo);
            minimum = minimum.min(value);
        }
        memo.insert((p, qa, qb), minimum);
        minimum
    }

    #[cfg(test)]
    pub(super) fn check_shared_continuations(&self) {
        for a in 0..self.shapes.len() {
            for b in 0..self.shapes.len() {
                for role in 0..10 {
                    let reference = self.suffix(0, 0, 0, a, b, role, &mut HashMap::new());
                    assert_eq!(self.difference(a, b, role), reference);
                }
            }
        }
    }

    pub(super) fn margin(&self, left: usize, right: usize, role: usize) -> Option<f64> {
        let (stat_a, a) = self.cards[left]?;
        let (stat_b, b) = self.cards[right]?;
        let difference = self.difference(a, b, role);
        let delta_stat = stat_a - stat_b;
        let meta_b = if delta_stat >= 0.0 {
            self.total_meta[b][role].low
        } else {
            self.total_meta[b][role].high
        };
        Some(
            [self.rest_low[role], self.rest_high]
                .into_iter()
                .map(|rest| {
                    (rest + stat_a) * difference + delta_stat * (self.base + meta_b) - self.rounding
                })
                .fold(f64::INFINITY, f64::min),
        )
    }
}
