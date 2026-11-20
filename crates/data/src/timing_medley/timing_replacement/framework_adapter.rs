//! The production partition/cover/fixed-point driver with timing dominance.
//! Each stage compares on one common domain, so graph closure composes proofs.
use super::*;
use bangdream_optimize_core::pruning::{self, DominanceGraph, Graphs, Model, Phase};
use std::{collections::HashMap};

#[path = "framework_adapter/order.rs"]
mod order;
use order::CertificateOrder;

type Error = Box<dyn std::error::Error>;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    passes: usize,
    comparisons: usize,
    graph_seconds: f64,
    cover_seconds: f64,
    cross_band_edges: usize,
    cross_attribute_edges: usize,
    correction_checks: usize,
    correction_passes: usize,
    replanned_checks: usize,
    replanned_passes: usize,
    equivalence_reorders: usize,
    stages: Vec<StageReport>,
}

#[derive(Serialize)]
struct StageReport {
    phase: &'static str,
    before: Vec<u32>,
    after: Vec<u32>,
    removals: Vec<Removal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Diagnostics>,
}

/// Observational counters, disabled in ordinary runs. Failed pairs evaluate
/// every song rather than stopping at the first; no pruning decision changes.
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostics {
    directed_pairs: usize,
    failure_masks: BTreeMap<u8, usize>,
    failures_by_song_and_class: [BTreeMap<&'static str, usize>; 3],
    single_song_failure_classes: [BTreeMap<&'static str, usize>; 3],
    survivors: Vec<Survivor>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Survivor {
    card: u32,
    character: u32,
    skill: usize,
    power: [f64; 3],
    witnesses: Vec<u32>,
    cover: usize,
}

#[derive(Serialize)]
struct Removal {
    card: u32,
    witnesses: Vec<u32>,
    cover: usize,
}

/// Validate the data premise once, outside the quadratic pair comparison.
/// A new uniformity flag may improve skill values, but must not alter duration
/// or correction. The supplied model checks every relevant judgement state.
fn validate_unification(
    retained: &[usize],
    cases: &[[usize; 4]],
    models: &mut [TimingReplacementBounds],
) -> Result<(), Error> {
    let mut checked = BTreeSet::new();
    for &i in retained {
        for from in 0..4 {
            for to in 0..4 {
                if from == to
                    || from & to != from
                    || !checked.insert((cases[i][from], cases[i][to]))
                {
                    continue;
                }
                for model in models.iter_mut() {
                    if model.loss_coefficient(cases[i][to], cases[i][from])? != Some(0.) {
                        return Err(format!(
                            "timing pruning: nonmonotone uniformity {from}->{to} at pool index {i}"
                        )
                        .into());
                    }
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prune(
    cards: &[PreparedCard],
    retained: &[usize],
    powers: &[BTreeMap<u32, MemberPower>],
    cases: &[[usize; 4]],
    choices: &[[Choice; 4]],
    uppers: &[PoolTimingUpper],
    pairs: &[Pair],
    models: &mut [TimingReplacementBounds],
    other_upper: [u64; 3],
    seed: u64,
    cross_duration: bool,
    cross_character: bool,
    affine: bool,
    prior: Option<&[[bool; 4]]>,
) -> Result<
    (
        Vec<[bool; 4]>,
        Vec<SignatureStats>,
        Option<super::super::timing_eligibility::Stats>,
    ),
    Error,
> {
    validate_unification(retained, cases, models)?;
    let (eligibility, eligibility_stats) = super::super::timing_eligibility::build(
        cards,
        retained,
        powers,
        choices,
        uppers,
        pairs,
        other_upper,
        seed,
        prior,
    );
    let required = std::array::from_fn(|s| seed.saturating_sub(other_upper[s]) as f64);
    let mut allowed = vec![[false; 4]; cards.len()];
    let mut stats = vec![];
    for signature in pruning::signatures(cards) {
        let band = signature.team_band_id();
        let attribute = signature.team_attribute();
        let context = usize::from(band.is_some()) + 2 * usize::from(attribute.is_some());
        let mut indices: Vec<_> = retained
            .iter()
            .copied()
            .filter(|&i| signature.allows(&cards[i]) && prior.is_none_or(|p| p[i][context]))
            .collect();
        if indices.is_empty() {
            continue;
        }
        let mut row = SignatureStats {
            band,
            attribute,
            context,
            before: indices.len(),
            score_floor: required,
            ..Default::default()
        };
        let mut forced: Vec<_> = (0..3)
            .map(|s| {
                Oracle::fixed_context(&uppers[s], choices, cards, &indices, &powers[s], s, context)
                    .forced(&vec![0.; cards.len()])
            })
            .collect();
        if super::super::timing_product::enabled() {
            let start = Instant::now();
            for s in 0..3 {
                let product = super::super::timing_product::bounds(
                    &uppers[s], choices, cards, &indices, s, context,
                );
                for &i in &indices {
                    row.forced_product.queries += 1;
                    row.forced_product.tighter +=
                        usize::from((product[i] as f64) < forced[s][i].floor());
                    forced[s][i] = forced[s][i].min(product[i] as f64);
                }
            }
            row.forced_product.seconds = start.elapsed().as_secs_f64();
        }
        indices.retain(|&i| {
            let keep =
                (0..3).any(|s| eligibility[i] & (1 << s) != 0 && forced[s][i] >= required[s]);
            if !keep {
                row.cutoff_removed.push(cards[i].card_id);
            }
            keep
        });
        let mut report = Report::default();
        let (kept, passes) = pruning::fixed_point(indices, |active, phase| {
            if phase == Phase::CrossCharacter && !cross_character {
                return Ok::<_, Error>(active.to_vec());
            }
            if active.is_empty() {
                return Ok(vec![]);
            }
            let groups = group_indices(cards, active);
            let before = active.iter().map(|&i| cards[i].card_id).collect();
            if groups.len() < 5 {
                report.stages.push(StageReport {
                    phase: "incomplete",
                    before,
                    after: vec![],
                    removals: vec![],
                    diagnostics: None,
                });
                return Ok(vec![]);
            }
            let local_cards: Vec<_> = active.iter().map(|&i| cards[i].clone()).collect();
            // Fixed for this entire proof phase. Pair comparisons must use the
            // same input values, but need not repeat six tree lookups per pair.
            let pair_inputs = (std::env::var("BANDORI_TIMING_DISABLE_DENSE_PRUNING_INPUTS")
                .as_deref() != Ok("1"))
                .then(|| active.iter().map(|&i| (
                    std::array::from_fn(|s| powers[s][&cards[i].card_id]),
                    cases[i][context],
                )).collect());
            let local_ids: Vec<_> = (0..active.len()).collect();
            let masks: Vec<_> = active.iter().map(|&i| eligibility[i]).collect();
            let (ranges, contexts) = replacement_context(
                &groups, cards, powers, cases, context, models, required, affine,
            );
            // Per-target domains cannot be transitively closed. Use their union,
            // including all intermediates even when a divided child deletes one.
            let range: [[f64; 2]; 3] = std::array::from_fn(|s| {
                [
                    ranges
                        .values()
                        .map(|r| r[s][0])
                        .fold(f64::INFINITY, f64::min),
                    ranges.values().map(|r| r[s][1]).fold(0., f64::max),
                ]
            });
            let common = std::array::from_fn(|s| TimingAffineContext {
                other_power: range[s],
                required_score: required[s],
                other_coefficient: std::array::from_fn(|slot| {
                    [
                        contexts
                            .values()
                            .map(|r| r[s].other_coefficient[slot][0])
                            .fold(f64::INFINITY, f64::min),
                        contexts
                            .values()
                            .map(|r| r[s].other_coefficient[slot][1])
                            .fold(0., f64::max),
                    ]
                }),
            });
            let skill_ids: Vec<_> = active
                .iter()
                .map(|&i| cases[i][context])
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let coefficient = std::array::from_fn(|s| models[s].coefficient_upper(&skill_ids));
            let replanned_enabled =
                std::env::var("BANDORI_TIMING_REPLANNED_REPLACEMENT").as_deref() != Ok("0");
            if replanned_enabled {
                for model in models.iter_mut() {
                    model.set_replanned_catalogue(&skill_ids);
                }
            }
            let mut model = TimingStage {
                cards: &local_cards,
                indices: active,
                models,
                powers,
                cases,
                context,
                pair_inputs,
                coefficient,
                range,
                common,
                required,
                cross_duration,
                affine,
                comparisons: 0,
                cross_band_edges: 0,
                cross_attribute_edges: 0,
                graph_seconds: 0.,
                removals: vec![],
                cache: HashMap::new(),
                order: None,
                diagnostics: (std::env::var("BANDORI_TIMING_PRUNE_DIAGNOSTICS").as_deref()
                    == Ok("1"))
                .then(Diagnostics::default),
                observed: HashMap::new(),
                correction_enabled: std::env::var("BANDORI_TIMING_CORRECTION_REPLACEMENT")
                    .as_deref()
                    != Ok("0"),
                correction_checks: 0,
                correction_passes: 0,
                replanned_enabled,
                replanned_checks: 0,
                replanned_passes: 0,
            };
            let (kept, stage) = model.prune_phase(phase, signature, &local_ids, &masks)?;
            report.equivalence_reorders += usize::from(model.order.is_some());
            report.comparisons += model.comparisons;
            report.graph_seconds +=
                model.graph_seconds + (stage.hard_merge_ms + stage.contribution_merge_ms) / 1000.;
            report.cover_seconds += stage.cover_ms / 1000.;
            report.cross_band_edges += model.cross_band_edges;
            report.cross_attribute_edges += model.cross_attribute_edges;
            report.correction_checks += model.correction_checks;
            report.correction_passes += model.correction_passes;
            report.replanned_checks += model.replanned_checks;
            report.replanned_passes += model.replanned_passes;
            if let Some(d) = &mut model.diagnostics {
                d.survivors = kept
                    .iter()
                    .map(|&i| {
                        let (witnesses, cover) = model.observed.remove(&i).unwrap();
                        Survivor {
                            card: local_cards[i].card_id,
                            character: local_cards[i].character_id,
                            skill: cases[active[i]][context],
                            power: std::array::from_fn(|s| {
                                powers[s][&local_cards[i].card_id].linear_total()
                            }),
                            witnesses,
                            cover,
                        }
                    })
                    .collect();
            }
            report.stages.push(StageReport {
                phase: if phase == Phase::SameCharacter {
                    "same"
                } else {
                    "cross"
                },
                before,
                after: kept.iter().map(|&i| local_cards[i].card_id).collect(),
                removals: model.removals,
                diagnostics: model.diagnostics,
            });
            Ok(kept.iter().map(|&i| active[i]).collect())
        })?;
        report.passes = passes;
        for &i in &kept {
            allowed[i][context] = true;
        }
        row.after = kept.len();
        row.cover_seconds = report.cover_seconds;
        row.legacy_framework = Some(report);
        stats.push(row);
    }
    Ok((allowed, stats, Some(eligibility_stats)))
}

fn group_indices(cards: &[PreparedCard], active: &[usize]) -> BTreeMap<u32, Vec<usize>> {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for &i in active {
        groups.entry(cards[i].character_id).or_default().push(i);
    }
    groups
}

struct TimingStage<'a> {
    cards: &'a [PreparedCard],
    indices: &'a [usize],
    models: &'a mut [TimingReplacementBounds],
    powers: &'a [BTreeMap<u32, MemberPower>],
    cases: &'a [[usize; 4]],
    context: usize,
    pair_inputs: Option<Vec<([MemberPower; 3], usize)>>,
    coefficient: [f64; 3],
    range: [[f64; 2]; 3],
    common: [TimingAffineContext; 3],
    required: [f64; 3],
    cross_duration: bool,
    affine: bool,
    comparisons: usize,
    cross_band_edges: usize,
    cross_attribute_edges: usize,
    graph_seconds: f64,
    removals: Vec<Removal>,
    cache: HashMap<(usize, usize), (bool, bool)>,
    order: Option<CertificateOrder>,
    diagnostics: Option<Diagnostics>,
    observed: HashMap<usize, (Vec<u32>, usize)>,
    correction_enabled: bool,
    correction_checks: usize,
    correction_passes: usize,
    replanned_enabled: bool,
    replanned_checks: usize,
    replanned_passes: usize,
}

impl TimingStage<'_> {
    fn prune_phase(
        &mut self,
        phase: Phase,
        signature: pruning::MedleyPruneSignature,
        ids: &[usize],
        masks: &[u8],
    ) -> Result<(Vec<usize>, pruning::StageStats), Error> {
        let cards = self.cards;
        let run = |model: &mut Self| {
            if phase == Phase::SameCharacter {
                pruning::same_character(cards, ids, 3, model)
            } else {
                pruning::cross_character(cards, signature, ids, masks, 3, 3, model)
            }
        };
        let (mut kept, mut stats) = run(self)?;
        let serials = cards
            .iter()
            .zip(self.indices)
            .map(|(card, &original)| (card.card_id, original))
            .collect();
        if let Some(order) = CertificateOrder::new(serials, &self.cache) {
            // A cycle of sound, composable weak certificates proves equivalence.
            // Reorient the entire class, then restart from ALL stage inputs:
            // deletions made with the old orientation cannot be kept.
            self.order = Some(order);
            self.removals.clear();
            self.observed.clear();
            let (next, extra) = run(self)?;
            kept = next;
            stats.cover_ms += extra.cover_ms;
            stats.hard_merge_ms += extra.hard_merge_ms;
            stats.contribution_merge_ms += extra.contribution_merge_ms;
        }
        Ok((kept, stats))
    }

    fn dominates(&mut self, new: usize, old: usize) -> Result<bool, Error> {
        self.comparisons += 1;
        let (a, b) = self.pair_inputs.as_ref().map_or_else(
            || (self.cases[self.indices[new]][self.context],
                self.cases[self.indices[old]][self.context]),
            |inputs| (inputs[new].1, inputs[old].1));
        let mut failure_mask = 0_u8;
        let mut failed_classes = [""; 3];
        for s in 0..3 {
            let (pa, pb) = self.pair_inputs.as_ref().map_or_else(
                || (self.powers[s][&self.cards[new].card_id],
                    self.powers[s][&self.cards[old].card_id]),
                |inputs| (inputs[new].0[s], inputs[old].0[s]));
            let good = if self.cross_duration {
                self.models[s].can_replace_extended_context(
                    a,
                    b,
                    pa,
                    pb,
                    self.range[s],
                    self.coefficient[s],
                    self.required[s],
                )?
            } else {
                self.models[s].can_replace_in_context(
                    a,
                    b,
                    pa,
                    pb,
                    self.range[s],
                    self.coefficient[s],
                    self.required[s],
                )?
            };
            let good = good
                || (self.correction_enabled && {
                    self.correction_checks += 1;
                    let pass = self.models[s].can_replace_correction_context(
                        a,
                        b,
                        pa,
                        pb,
                        self.range[s],
                        self.coefficient[s],
                        self.required[s],
                    )?;
                    self.correction_passes += usize::from(pass);
                    pass
                });
            let good = good
                || (self.affine
                    && self.models[s].can_replace_affine_context(
                        a,
                        b,
                        pa,
                        pb,
                        &self.common[s],
                        self.cross_duration,
                    )?);
            let good = good
                || (self.replanned_enabled && self.cross_duration && {
                    self.replanned_checks += 1;
                    let pass = self.models[s].can_replace_replanned_context(
                        a,
                        b,
                        pa,
                        pb,
                        self.range[s],
                        self.coefficient[s],
                        self.required[s],
                    )?;
                    self.replanned_passes += usize::from(pass);
                    pass
                });
            if !good {
                if let Some(d) = &mut self.diagnostics {
                    failure_mask |= 1 << s;
                    failed_classes[s] = self.models[s].comparison_class(a, b);
                    *d.failures_by_song_and_class[s]
                        .entry(failed_classes[s])
                        .or_default() += 1;
                } else {
                    return Ok(false);
                }
            }
        }
        if let Some(d) = &mut self.diagnostics {
            d.directed_pairs += 1;
            *d.failure_masks.entry(failure_mask).or_default() += 1;
            if failure_mask.count_ones() == 1 {
                let s = failure_mask.trailing_zeros() as usize;
                *d.single_song_failure_classes[s]
                    .entry(failed_classes[s])
                    .or_default() += 1;
            }
        }
        Ok(failure_mask == 0)
    }

    fn pair(&mut self, a: usize, b: usize, graph: &mut DominanceGraph) -> Result<(), Error> {
        let (a, b) = if a < b { (a, b) } else { (b, a) };
        let (ab, ba) = if let Some(order) = &self.order {
            // The retry uses one frozen proof graph. Do not introduce new
            // certificates after fixing its equivalence classes and serials.
            order.pair(a, b)
        } else if let Some(&value) = self.cache.get(&(a, b)) {
            value
        } else {
            let mut ab = self.dominates(a, b)?;
            let mut ba = self.dominates(b, a)?;
            if ab && ba {
                ab = (self.cards[a].card_id, self.indices[a])
                    < (self.cards[b].card_id, self.indices[b]);
                ba = !ab;
            }
            self.cache.insert((a, b), (ab, ba));
            let edges = usize::from(ab) + usize::from(ba);
            self.cross_band_edges +=
                edges * usize::from(self.cards[a].band_id != self.cards[b].band_id);
            self.cross_attribute_edges +=
                edges * usize::from(self.cards[a].attribute != self.cards[b].attribute);
            (ab, ba)
        };
        if ab {
            graph.add_edge(a, b);
        }
        if ba {
            graph.add_edge(b, a);
        }
        Ok(())
    }
}

impl Model for TimingStage<'_> {
    type Error = Error;
    fn subset(&mut self, indices: &[usize], close: bool) -> Result<Graphs, Error> {
        let start = Instant::now();
        let mut graph = DominanceGraph::new(self.cards.len());
        for (pos, &a) in indices.iter().enumerate() {
            for &b in &indices[pos + 1..] {
                self.pair(a, b, &mut graph)?;
            }
        }
        if close {
            graph.transitive_closure_for_subset(indices);
        }
        self.graph_seconds += start.elapsed().as_secs_f64();
        Ok(Graphs {
            hard: DominanceGraph::new(self.cards.len()),
            contribution: graph,
        })
    }
    fn cross(&mut self, left: &[usize], right: &[usize]) -> Result<Graphs, Error> {
        let start = Instant::now();
        let mut graph = DominanceGraph::new(self.cards.len());
        for &a in left {
            for &b in right {
                self.pair(a, b, &mut graph)?;
            }
        }
        self.graph_seconds += start.elapsed().as_secs_f64();
        Ok(Graphs {
            hard: DominanceGraph::new(self.cards.len()),
            contribution: graph,
        })
    }
    fn observe(
        &mut self,
        phase: Phase,
        target: usize,
        graphs: &Graphs,
        _hard: usize,
        cover: usize,
    ) {
        if self.diagnostics.is_some() {
            self.observed.insert(
                target,
                (
                    graphs
                        .contribution
                        .incoming(target)
                        .iter()
                        .map(|&i| self.cards[i].card_id)
                        .collect(),
                    cover,
                ),
            );
        }
        if cover >= if phase == Phase::SameCharacter { 3 } else { 1 } {
            self.removals.push(Removal {
                card: self.cards[target].card_id,
                witnesses: graphs
                    .contribution
                    .incoming(target)
                    .iter()
                    .map(|&i| self.cards[i].card_id)
                    .collect(),
                cover,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bangdream_optimize_core::{
        hit_timing::{
            JudgementCorrection, NoteWindow, ReplayInput, ReplayNote, ReplaySkill, ScoreEffect,
        },
        scoring::ScoringPower,
    };

    fn skill(percent: f32) -> ReplaySkill {
        ReplaySkill {
            duration: 0.4,
            correction: None,
            score: ScoreEffect::Plain { percent },
        }
    }

    fn model(skills: &[ReplaySkill]) -> TimingReplacementBounds {
        let notes: Vec<_> = (0..12)
            .map(|i| ReplayNote {
                time: 0.5 + i as f64 * 0.3,
                trigger: (i % 2 == 0).then_some(i / 2),
                window: NoteWindow::Standard,
            })
            .collect();
        let input = ReplayInput {
            power: 300_000.,
            play_level: 27,
            fps: 120,
            hit_frames: notes
                .iter()
                .map(|n| (n.time * 120.).round() as u32)
                .collect(),
            notes,
            skills: vec![skills[0].clone(); 6],
            initial_combo: 0,
            medley_combo: true,
            fever_frames: None,
            constraints: vec![],
            hit_order: vec![],
            native_time_routing: None,
        };
        TimingReplacementBounds::new_adjacent(&input, skills, ScoringPower::new(600_000.).unwrap())
            .unwrap()
    }

    #[test]
    fn uniformity_checks_joint_effect_and_rejects_changed_skill_clock_or_correction() {
        let good = [skill(100.), skill(110.), skill(120.), skill(130.)];
        assert!(validate_unification(&[0], &[[0, 1, 2, 3]], &mut [model(&good)]).is_ok());
        for changed in [
            skill(115.),
            ReplaySkill {
                duration: 0.5,
                ..skill(130.)
            },
            ReplaySkill {
                correction: Some(JudgementCorrection::Bad),
                ..skill(130.)
            },
        ] {
            let mut bad = good.clone();
            bad[3] = changed;
            assert!(validate_unification(&[0], &[[0, 1, 2, 3]], &mut [model(&bad)]).is_err());
        }
    }

    fn equal_card(id: u32) -> PreparedCard {
        use bangdream_optimize_core::{ScoreUp, StatValue, TeamCardSkill};
        PreparedCard {
            native_power: None,
            card_id: id,
            character_id: (id - 1) % 5 + 1,
            band_id: 1,
            rarity: 4,
            attribute: Attribute::Cool,
            level: 60,
            training: true,
            illust_training_status: true,
            episodes: [true; 2],
            limit_break_rank: 0,
            skill_level: 5,
            stat: StatValue {
                performance: 1000.,
                technique: 1000.,
                visual: 1000.,
            },
            event_add_stat: StatValue::zero(),
            skill: TeamCardSkill {
                card_id: id,
                duration: 5.,
                score_up: 1.,
                rateup: false,
            },
            score_up: ScoreUp {
                default: 1.,
                unification_activate_effect_value: None,
                unification_activate_condition_band_id: None,
                unification_activate_condition_type: None,
            },
        }
    }

    #[test]
    fn cyclic_equivalence_restarts_the_whole_phase_and_keeps_three_copies_per_role() {
        // Synthetic valid weak certificates between genuinely equal cards.
        // 150 crosses the framework's divided-graph boundary (128).
        for phase in [Phase::SameCharacter, Phase::CrossCharacter] {
            for reversed in [false, true] {
                let mut cards: Vec<_> = (1..=150).map(equal_card).collect();
                if reversed {
                    cards.reverse();
                }
                let ids: Vec<_> = (0..cards.len()).collect();
                let mut cache = HashMap::new();
                for a in 0..cards.len() {
                    for b in a + 1..cards.len() {
                        cache.insert((a, b), (false, false));
                    }
                }
                let groups: Vec<Vec<_>> = if phase == Phase::SameCharacter {
                    group_indices(&cards, &ids).into_values().collect()
                } else {
                    vec![ids.clone()]
                };
                for mut group in groups {
                    group.sort_by_key(|&i| cards[i].card_id);
                    for (&from, &to) in group.iter().zip(group.iter().cycle().skip(1)) {
                        let value = if from < to {
                            cache.get_mut(&(from, to)).unwrap()
                        } else {
                            cache.get_mut(&(to, from)).unwrap()
                        };
                        if from < to {
                            value.0 = true;
                        } else {
                            value.1 = true;
                        }
                    }
                }
                let frozen = cache.clone();
                let mut models = [];
                let mut stage = TimingStage {
                    cards: &cards,
                    indices: &ids,
                    models: &mut models,
                    powers: &[],
                    cases: &[],
                    context: 0,
                    pair_inputs: None,
                    coefficient: [0.; 3],
                    range: [[0.; 2]; 3],
                    common: std::array::from_fn(|_| TimingAffineContext {
                        other_power: [0.; 2],
                        other_coefficient: [[0.; 2]; 10],
                        required_score: 0.,
                    }),
                    required: [0.; 3],
                    cross_duration: true,
                    affine: true,
                    comparisons: 0,
                    cross_band_edges: 0,
                    cross_attribute_edges: 0,
                    graph_seconds: 0.,
                    removals: vec![],
                    cache,
                    order: None,
                    diagnostics: Some(Diagnostics::default()),
                    observed: HashMap::new(),
                    correction_enabled: true,
                    correction_checks: 0,
                    correction_passes: 0,
                    replanned_enabled: true,
                    replanned_checks: 0,
                    replanned_passes: 0,
                };
                let (kept, _) = stage
                    .prune_phase(
                        phase,
                        pruning::MedleyPruneSignature::Mixed,
                        &ids,
                        &vec![7; 150],
                    )
                    .unwrap();
                assert!(stage.order.is_some());
                assert_eq!(stage.cache, frozen, "retry cannot change the proof domain");
                assert_eq!(stage.comparisons, 0, "no numerical comparisons on retry");
                let mut kept_ids: Vec<_> = kept.iter().map(|&i| cards[i].card_id).collect();
                kept_ids.sort_unstable();
                assert_eq!(kept_ids, (1..=15).collect::<Vec<_>>(), "{phase:?}");
                assert_eq!(stage.removals.len(), 135, "discard earlier removal records");
                for &i in &kept {
                    assert!(stage.observed.contains_key(&i));
                }
            }
        }
    }
}

use bangdream_optimize_core::timing::Instant;
