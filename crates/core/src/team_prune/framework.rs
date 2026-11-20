//! Shared fixed-point and divided-graph pruning used by both scoring models.
//! A model supplies dominance on a COMMON stage domain. Edges must compose,
//! preserve already-satisfied uniformity, and use an acyclic tie convention.
use super::hard::{cross_cover, same_character_cover};
pub use super::signature::MedleyPruneSignature;
use crate::{model::preparation::PreparedCard, timing::Timer};
pub use bangdream_optimize_team_prune::DominanceGraph;
use std::collections::BTreeMap;

const LEAF_SIZE: usize = 128;

pub fn signatures(cards: &[PreparedCard]) -> Vec<MedleyPruneSignature> {
    super::signature::seed_signatures(cards)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    SameCharacter,
    CrossCharacter,
}

pub struct Graphs {
    pub hard: DominanceGraph,
    pub contribution: DominanceGraph,
}

/// IDs refer to the cards of this stage, never to a previous compressed stage.
pub trait Model {
    type Error;
    fn subset(&mut self, indices: &[usize], close: bool) -> Result<Graphs, Self::Error>;
    fn cross(&mut self, left: &[usize], right: &[usize]) -> Result<Graphs, Self::Error>;
    fn observe(
        &mut self,
        _phase: Phase,
        _target: usize,
        _graphs: &Graphs,
        _hard_cover: usize,
        _combined_cover: usize,
    ) {
    }
}

#[derive(Default)]
pub struct StageStats {
    pub cover_ms: f64,
    pub hard_merge_ms: f64,
    pub contribution_merge_ms: f64,
}

/// Rebuild each phase from its input survivors; no witness locks or arbitrary
/// pass limit. Both production and timing research use this exact driver.
pub fn fixed_point<E>(
    mut active: Vec<usize>,
    mut stage: impl FnMut(&[usize], Phase) -> Result<Vec<usize>, E>,
) -> Result<(Vec<usize>, usize), E> {
    let mut passes = 0;
    loop {
        passes += 1;
        let before = active.len();
        for phase in [Phase::SameCharacter, Phase::CrossCharacter] {
            let next = stage(&active, phase)?;
            debug_assert!(next.iter().all(|i| active.contains(i)));
            active = next;
        }
        if active.len() == before {
            return Ok((active, passes));
        }
    }
}

pub fn same_character<M: Model>(
    cards: &[PreparedCard],
    indices: &[usize],
    team_count: usize,
    model: &mut M,
) -> Result<(Vec<usize>, StageStats), M::Error> {
    let mut groups = BTreeMap::<u32, Vec<usize>>::new();
    for &i in indices {
        groups.entry(cards[i].character_id).or_default().push(i);
    }
    let mut survivors = Vec::with_capacity(indices.len());
    let mut stats = StageStats::default();
    for group in groups.into_values() {
        let graphs = model.subset(&group, group.len() > LEAF_SIZE)?;
        for i in group {
            let start = Timer::start();
            let hard = same_character_cover(&graphs.hard, i, cards, team_count);
            let combined = same_character_cover(&graphs.contribution, i, cards, team_count);
            stats.cover_ms += start.elapsed_ms();
            model.observe(Phase::SameCharacter, i, &graphs, hard, combined);
            if combined < team_count {
                survivors.push(i);
            }
        }
    }
    survivors.sort_unstable();
    Ok((survivors, stats))
}

pub fn cross_character<M: Model>(
    cards: &[PreparedCard],
    signature: MedleyPruneSignature,
    indices: &[usize],
    eligibility: &[u8],
    team_count: usize,
    chart_count: usize,
    model: &mut M,
) -> Result<(Vec<usize>, StageStats), M::Error> {
    let mut stats = StageStats::default();
    let (survivors, _) = divided(
        cards,
        signature,
        indices,
        eligibility,
        team_count,
        chart_count,
        model,
        &mut stats,
    )?;
    Ok((survivors, stats))
}

#[allow(clippy::too_many_arguments)]
fn divided<M: Model>(
    cards: &[PreparedCard],
    signature: MedleyPruneSignature,
    indices: &[usize],
    eligibility: &[u8],
    team_count: usize,
    chart_count: usize,
    model: &mut M,
    stats: &mut StageStats,
) -> Result<(Vec<usize>, Graphs), M::Error> {
    let (indices, mut graphs) = if indices.len() > LEAF_SIZE {
        let middle = indices.len() / 2;
        let (mut left, mut a) = divided(
            cards,
            signature,
            &indices[..middle],
            eligibility,
            team_count,
            chart_count,
            model,
            stats,
        )?;
        let (right, b) = divided(
            cards,
            signature,
            &indices[middle..],
            eligibility,
            team_count,
            chart_count,
            model,
            stats,
        )?;
        let cross = model.cross(&left, &right)?;
        left.extend_from_slice(&right);
        let start = Timer::start();
        a.hard.extend_edges_from(&b.hard);
        a.hard.extend_edges_from(&cross.hard);
        if left.len() > LEAF_SIZE {
            a.hard.transitive_closure_for_subset(&left);
        }
        stats.hard_merge_ms += start.elapsed_ms();
        let start = Timer::start();
        a.contribution.extend_edges_from(&b.contribution);
        a.contribution.extend_edges_from(&a.hard);
        a.contribution.extend_edges_from(&cross.contribution);
        if left.len() > LEAF_SIZE {
            a.contribution.transitive_closure_for_subset(&left);
        }
        stats.contribution_merge_ms += start.elapsed_ms();
        (left, a)
    } else {
        (indices.to_vec(), model.subset(indices, false)?)
    };
    let mut survivors = Vec::with_capacity(indices.len());
    for i in indices {
        let start = Timer::start();
        let hard = cross_cover(
            &graphs.hard,
            i,
            cards,
            signature,
            team_count,
            eligibility,
            chart_count,
        );
        let combined = if super::hard::cover_optimization_enabled()
            && graphs.hard.incoming(i) == graphs.contribution.incoming(i)
        {
            hard
        } else {
            cross_cover(
                &graphs.contribution,
                i,
                cards,
                signature,
                team_count,
                eligibility,
                chart_count,
            )
        };
        stats.cover_ms += start.elapsed_ms();
        model.observe(Phase::CrossCharacter, i, &graphs, hard, combined);
        if combined == 0 {
            survivors.push(i);
        }
    }
    graphs.hard.retain_nodes(&survivors);
    graphs.contribution.retain_nodes(&survivors);
    Ok((survivors, graphs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    #[test]
    fn fixed_point_rebuilds_cross_from_same_survivors_and_repeats() {
        let mut calls = vec![];
        let (kept, passes) = fixed_point(vec![0, 1, 2, 3], |ids, phase| {
            calls.push((ids.to_vec(), phase));
            Ok::<_, Infallible>(
                ids.iter()
                    .copied()
                    .filter(|&i| match phase {
                        Phase::SameCharacter => i != 0,
                        Phase::CrossCharacter => i != 1,
                    })
                    .collect(),
            )
        })
        .unwrap();
        assert_eq!(kept, vec![2, 3]);
        assert_eq!(passes, 2);
        assert_eq!(calls[1], (vec![1, 2, 3], Phase::CrossCharacter));
        assert_eq!(calls[2], (vec![2, 3], Phase::SameCharacter));
    }

    // A separately scored monotone model isolates the shared coverage driver
    // from either contribution implementation. Full three-team enumeration
    // checks cross-signature proof composition, including actual upgrades.
    struct Additive<'a>(&'a [PreparedCard]);
    impl Additive<'_> {
        fn graph(&self, pairs: impl Iterator<Item = (usize, usize)>) -> Graphs {
            let mut graph = DominanceGraph::new(self.0.len());
            for (a, b) in pairs {
                let pa = self.0[a].stat.performance;
                let pb = self.0[b].stat.performance;
                if pa > pb || (pa == pb && self.0[a].card_id < self.0[b].card_id) {
                    graph.add_edge(a, b);
                }
            }
            Graphs {
                hard: graph.clone(),
                contribution: graph,
            }
        }
    }
    impl Model for Additive<'_> {
        type Error = Infallible;
        fn subset(&mut self, ids: &[usize], close: bool) -> Result<Graphs, Infallible> {
            let mut g = self.graph(
                ids.iter()
                    .flat_map(|&a| ids.iter().filter(move |&&b| a != b).map(move |&b| (a, b))),
            );
            if close {
                g.hard.transitive_closure_for_subset(ids);
                g.contribution.transitive_closure_for_subset(ids);
            }
            Ok(g)
        }
        fn cross(&mut self, left: &[usize], right: &[usize]) -> Result<Graphs, Infallible> {
            Ok(self.graph(
                left.iter()
                    .flat_map(|&a| right.iter().flat_map(move |&b| [(a, b), (b, a)])),
            ))
        }
    }

    fn solve_toy(cards: &[PreparedCard], memberships: &[[bool; 4]]) -> i32 {
        solve_toy_with(cards, memberships, |power, context| {
            power + if context & 1 != 0 { 500 } else { 0 } + if context & 2 != 0 { 200 } else { 0 }
        })
    }

    fn solve_toy_with(
        cards: &[PreparedCard],
        memberships: &[[bool; 4]],
        mut score: impl FnMut(i32, usize) -> i32,
    ) -> i32 {
        let size = 1_usize << cards.len();
        let mut best = vec![i32::MIN / 4; size];
        let mut teams = vec![];
        for mask in 0..size {
            if mask.count_ones() != 5 {
                continue;
            }
            let ids: Vec<_> = (0..cards.len()).filter(|&i| mask & (1 << i) != 0).collect();
            if ids
                .iter()
                .map(|&i| cards[i].character_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != 5
            {
                continue;
            }
            let band = ids
                .iter()
                .all(|&i| cards[i].band_id == cards[ids[0]].band_id);
            let attribute = ids
                .iter()
                .all(|&i| cards[i].attribute == cards[ids[0]].attribute);
            let context = usize::from(band) + 2 * usize::from(attribute);
            if ids.iter().any(|&i| !memberships[i][context]) {
                continue;
            }
            let power = ids
                .iter()
                .map(|&i| cards[i].stat.performance as i32)
                .sum::<i32>();
            let score = score(power, context);
            best[mask] = score;
            teams.push((mask, score));
        }
        // Independent subset maximum, then enumerate two explicit disjoint
        // teams; the remaining subset supplies the third. No pruning calls.
        for bit in 0..cards.len() {
            for mask in 0..size {
                if mask & (1 << bit) != 0 {
                    best[mask] = best[mask].max(best[mask ^ (1 << bit)]);
                }
            }
        }
        let mut answer = i32::MIN;
        for &(a, x) in &teams {
            for &(b, y) in &teams {
                if a & b == 0 {
                    answer = answer.max(x + y + best[(size - 1) ^ (a | b)]);
                }
            }
        }
        answer
    }

    #[test]
    fn partition_union_preserves_exhaustive_three_team_optimum_after_uniformity_upgrades() {
        use crate::{medley::test_support::prepared_card, Attribute};
        let mut cards = vec![];
        for role in 0..6 {
            for variant in 0..if role == 0 { 4 } else { 3 } {
                let attr = if role == 5 {
                    Attribute::Pure
                } else if variant == 2 {
                    Attribute::Happy
                } else {
                    Attribute::Cool
                };
                let mut card = prepared_card(
                    cards.len() as u32 + 1,
                    role + 1,
                    if role == 5 { 2 } else { 1 },
                    attr,
                );
                card.stat.performance = if role == 5 || variant == 3 {
                    10.
                } else {
                    100. + variant as f64
                };
                cards.push(card);
            }
        }
        let reference = solve_toy(&cards, &vec![[true; 4]; cards.len()]);
        let mut allowed = vec![[false; 4]; cards.len()];
        for signature in signatures(&cards) {
            let active: Vec<_> = (0..cards.len())
                .filter(|&i| signature.allows(&cards[i]))
                .collect();
            let (kept, _) = fixed_point(active, |ids, phase| {
                let local: Vec<_> = ids.iter().map(|&i| cards[i].clone()).collect();
                if local
                    .iter()
                    .map(|c| c.character_id)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    < 5
                {
                    return Ok::<_, Infallible>(vec![]);
                }
                let mut model = Additive(&local);
                let local_ids: Vec<_> = (0..local.len()).collect();
                let kept = if phase == Phase::SameCharacter {
                    same_character(&local, &local_ids, 3, &mut model)?.0
                } else {
                    cross_character(
                        &local,
                        signature,
                        &local_ids,
                        &vec![7; local.len()],
                        3,
                        3,
                        &mut model,
                    )?
                    .0
                };
                Ok(kept.iter().map(|&i| ids[i]).collect())
            })
            .unwrap();
            let context = usize::from(signature.team_band_id().is_some())
                + 2 * usize::from(signature.team_attribute().is_some());
            for i in kept {
                allowed[i][context] = true;
            }
        }
        assert!(!allowed[3][0], "same-role weak fourth copy must be removed");
        assert!(
            allowed
                .iter()
                .zip(&cards)
                .filter(|(_, c)| c.character_id == 6)
                .all(|(a, _)| !a.iter().any(|&x| x)),
            "cross-role weak band must be removed"
        );
        assert_eq!(solve_toy(&cards, &allowed), reference);

        // Also compose the same proven pointwise-power/condition edges with
        // actual native timing scores, not just the additive toy objective.
        // All five resolved skills are identical inside each partition, so
        // physical/skill permutations have exactly the same numerical query.
        // Integer member stats here sum exactly in f32 in every physical order.
        use crate::hit_timing::{
            independent::JointTimeline, NoteWindow, ReplayInput, ReplayNote, ReplaySkill,
            ScoreEffect,
        };
        use crate::scoring::ScoringPower;
        let times = [0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.53];
        let inputs: Vec<_> = (0..4)
            .map(|context| ReplayInput {
                power: 500.,
                play_level: 27,
                fps: 120,
                initial_combo: 18,
                medley_combo: true,
                fever_frames: Some([61, 66]),
                constraints: vec![],
                hit_order: vec![],
                native_time_routing: None,
                notes: times
                    .iter()
                    .enumerate()
                    .map(|(i, &time)| ReplayNote {
                        time,
                        trigger: (i < 6).then_some(i),
                        window: if i < 6 {
                            NoteWindow::PinnedPerfect {
                                frame: (time * 120.0_f64).round() as u32,
                            }
                        } else {
                            NoteWindow::Standard
                        },
                    })
                    .collect(),
                hit_frames: times
                    .iter()
                    .map(|t| (t * 120.0_f64).round() as u32)
                    .collect(),
                skills: vec![
                    ReplaySkill {
                        duration: 0.4,
                        correction: None,
                        score: ScoreEffect::Plain {
                            percent: 100.
                                + if context & 1 != 0 { 10. } else { 0. }
                                + if context & 2 != 0 { 20. } else { 0. }
                        }
                    };
                    6
                ],
            })
            .collect();
        let models: Vec<_> = inputs
            .iter()
            .map(|r| JointTimeline::new(r).unwrap())
            .collect();
        let mut scores = BTreeMap::new();
        let mut oracle = |power, context: usize| {
            *scores.entry((power, context)).or_insert_with(|| {
                let p = ScoringPower::new(power as f32).unwrap();
                let maximum = models[context].maximum(p).unwrap().result.total;
                // Only one ordinary note is movable: exhaust all its legal frames
                // directly through replay, independent of the graph and pruning.
                let mut q = inputs[context].clone();
                q.power = p.value();
                let brute = (45..=80)
                    .filter_map(|f| {
                        q.hit_frames[6] = f;
                        crate::hit_timing::replay(&q).ok().map(|v| v.total)
                    })
                    .max()
                    .unwrap();
                assert_eq!(maximum, brute);
                maximum as i32
            })
        };
        let unpruned = solve_toy_with(&cards, &vec![[true; 4]; cards.len()], &mut oracle);
        assert_eq!(solve_toy_with(&cards, &allowed, &mut oracle), unpruned);
    }

    #[test]
    fn divided_graph_keeps_three_available_copies_without_witness_locks() {
        use crate::{medley::test_support::prepared_card, Attribute};
        let cards: Vec<_> = (0..150)
            .map(|i| {
                let mut c = prepared_card(i + 1, i / 30 + 1, 1, Attribute::Cool);
                c.stat.performance = 100. + (i % 30) as f64;
                c
            })
            .collect();
        let signature = MedleyPruneSignature::UnifiedBandAttribute(1, Attribute::Cool);
        let (kept, _) = cross_character(
            &cards,
            signature,
            &(0..150).collect::<Vec<_>>(),
            &vec![7; 150],
            3,
            3,
            &mut Additive(&cards),
        )
        .unwrap();
        assert_eq!(kept.len(), 15);
        for role in 0..5 {
            assert_eq!(
                kept.iter()
                    .filter(|&&i| i / 30 == role)
                    .copied()
                    .collect::<Vec<_>>(),
                (role * 30 + 27..role * 30 + 30).collect::<Vec<_>>()
            );
        }
    }
}
