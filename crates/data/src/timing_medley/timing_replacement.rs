//! Conditional replacement experiment, using the supplied timing proof model.
//! Production scoring remains unchanged.
use super::{
    timing_pair::{Oracle, Pair},
    timing_pool::Choice,
};
use bangdream_optimize_core::{
    hit_timing::bounds::{PoolTimingUpper, TimingAffineContext, TimingReplacementBounds},
    scoring::MemberPower,
    Attribute, PreparedCard,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[path = "timing_replacement/framework_adapter.rs"]
mod framework_adapter;
#[path = "timing_replacement/unification.rs"]
mod unification;

pub(super) fn framework_enabled() -> bool {
    std::env::var("BANDORI_TIMING_LEGACY_FRAMEWORK").as_deref() != Ok("0")
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureStats {
    band: Option<u32>,
    attribute: Option<Attribute>,
    context: usize,
    before: usize,
    after: usize,
    score_floor: [f64; 3],
    cutoff_removed: Vec<u32>,
    replacements: Vec<[u32; 4]>,
    lower_power_witnesses: usize,
    cross_attribute_witnesses: usize,
    cross_role_attribute_witnesses: usize,
    cross_role_band_witnesses: usize,
    unification_guard: bool,
    unification_nonmonotone_transitions: usize,
    cross_duration_witnesses: usize,
    cross_replacements: Vec<CrossReplacement>,
    cross_domain_cards: usize,
    cross_domain_characters: usize,
    cross_domain_refreshed: bool,
    affine_checks: usize,
    affine_passes: usize,
    forced_product: super::timing_product::Stats,
    cover_checks: usize,
    precise_checks: usize,
    precise_passes: usize,
    precise_removals: usize,
    precise_prefixes: usize,
    cover_seconds: f64,
    song_obligations_removed: usize,
    song_specific_removals: usize,
    legacy_framework: Option<framework_adapter::Report>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CrossReplacement {
    removed: u32,
    witnesses: Vec<u32>,
    guaranteed_free: usize,
    coarse_free: usize,
    full_coarse_free: usize,
    target_songs: u8,
    by_song: Vec<SongReplacement>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SongReplacement {
    song: usize,
    witnesses: Vec<u32>,
    guaranteed_free: usize,
    coarse_free: usize,
}

/// Four conditional memberships per card. Unified contexts implicitly refer
/// to that card's own band/attribute; mixed contexts require the usual breaks
/// during enumeration. Same-character replacements retain the attribute unless
/// all members certify that newly unifying attributes cannot lower their skill.
pub fn prune(
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
        Option<super::timing_eligibility::Stats>,
    ),
    Box<dyn std::error::Error>,
> {
    if framework_enabled() {
        return framework_adapter::prune(
            cards,
            retained,
            powers,
            cases,
            choices,
            uppers,
            pairs,
            models,
            other_upper,
            seed,
            cross_duration,
            cross_character,
            affine,
            prior,
        );
    }
    let mut allowed = vec![[false; 4]; cards.len()];
    let mut stats = vec![];
    let cover_mode = super::timing_cover::mode();
    let unify_mode = super::timing_cover::unification_mode();
    let relax_unification = unify_mode > 0 && (prior.is_none() || unify_mode >= 2);
    // Freeze the global snapshot for this pass. Mode 2 needs it only for the
    // cross-character fallback; modes 3/4 also narrow target obligations in
    // later condition-preserving passes (which keep three common witnesses).
    let (eligibility, eligibility_stats) =
        if cover_mode >= 2 && (cross_character || cover_mode >= 3) {
            let (masks, stats) = super::timing_eligibility::build(
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
            (masks, Some(stats))
        } else {
            (vec![7; cards.len()], None)
        };
    let bands: Vec<_> = cards
        .iter()
        .map(|c| c.band_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(Some)
        .chain(std::iter::once(None))
        .collect();
    for attr in [
        None,
        Some(Attribute::Cool),
        Some(Attribute::Happy),
        Some(Attribute::Pure),
        Some(Attribute::Powerful),
    ] {
        for &band in &bands {
            let context = usize::from(band.is_some()) + 2 * usize::from(attr.is_some());
            let mut indices: Vec<_> = retained
                .iter()
                .copied()
                .filter(|&i| {
                    band.is_none_or(|b| cards[i].band_id == b)
                        && attr.is_none_or(|a| cards[i].attribute == a)
                        && prior.is_none_or(|p| p[i][context])
                })
                .collect();
            let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
            for &i in &indices {
                groups.entry(cards[i].character_id).or_default().push(i);
            }
            if groups.len() < 5 {
                if prior.is_some() && !indices.is_empty() {
                    stats.push(SignatureStats {
                        band,
                        attribute: attr,
                        context,
                        before: indices.len(),
                        after: 0,
                        score_floor: std::array::from_fn(|s| {
                            seed.saturating_sub(other_upper[s]) as f64
                        }),
                        cutoff_removed: indices.iter().map(|&i| cards[i].card_id).collect(),
                        replacements: vec![],
                        lower_power_witnesses: 0,
                        cross_attribute_witnesses: 0,
                        cross_role_attribute_witnesses: 0,
                        cross_role_band_witnesses: 0,
                        unification_guard: relax_unification,
                        unification_nonmonotone_transitions: 0,
                        cross_duration_witnesses: 0,
                        cross_replacements: vec![],
                        cross_domain_cards: 0,
                        cross_domain_characters: 0,
                        cross_domain_refreshed: false,
                        affine_checks: 0,
                        affine_passes: 0,
                        forced_product: Default::default(),
                        cover_checks: 0,
                        precise_checks: 0,
                        precise_passes: 0,
                        precise_removals: 0,
                        precise_prefixes: 0,
                        cover_seconds: 0.,
                        song_obligations_removed: 0,
                        song_specific_removals: 0,
                        legacy_framework: None,
                    });
                }
                continue;
            }
            let ids: Vec<_> = indices
                .iter()
                .map(|&i| cases[i][context])
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let mut coefficient: [f64; 3] =
                std::array::from_fn(|s| models[s].coefficient_upper(&ids));
            let required: [f64; 3] =
                std::array::from_fn(|s| seed.saturating_sub(other_upper[s]) as f64);
            let mut forced: Vec<_> = (0..3)
                .map(|s| {
                    Oracle::fixed_context(
                        &uppers[s], choices, cards, &indices, &powers[s], s, context,
                    )
                    .forced(&vec![0.; cards.len()])
                })
                .collect();
            // The new envelope is a cutoff proof, not a witness-order change.
            // Locks make heuristic ranking nonmonotonic: a lower bound value
            // could otherwise retain more witnesses despite rejecting no card.
            let ranking: Vec<_> = (0..cards.len())
                .map(|i| {
                    (0..3)
                        .map(|s| forced[s][i])
                        .fold(f64::NEG_INFINITY, f64::max)
                })
                .collect();
            let mut product_stats = super::timing_product::Stats::default();
            if super::timing_product::enabled() {
                let start = bangdream_optimize_core::timing::Instant::now();
                let was_viable: Vec<_> = (0..cards.len())
                    .map(|i| (0..3).any(|s| forced[s][i] >= required[s]))
                    .collect();
                for s in 0..3 {
                    let product = super::timing_product::bounds(
                        &uppers[s], choices, cards, &indices, s, context,
                    );
                    for &i in &indices {
                        product_stats.queries += 1;
                        let bound = product[i] as f64;
                        product_stats.tighter += usize::from(bound < forced[s][i].floor());
                        product_stats.new_song_rejections +=
                            usize::from(bound < required[s] && forced[s][i] >= required[s]);
                        forced[s][i] = forced[s][i].min(bound);
                    }
                }
                product_stats.new_condition_rejections = indices
                    .iter()
                    .filter(|&&i| was_viable[i] && (0..3).all(|s| forced[s][i] < required[s]))
                    .count();
                product_stats.seconds = start.elapsed().as_secs_f64();
            }
            let viable: Vec<[bool; 3]> = (0..cards.len())
                .map(|i| {
                    std::array::from_fn(|s| {
                        forced[s][i] >= required[s]
                            && (cover_mode < 3 || eligibility[i] & (1 << s) != 0)
                    })
                })
                .collect();
            let song_obligations_removed = indices
                .iter()
                .map(|&i| {
                    (0..3)
                        .filter(|&s| forced[s][i] >= required[s] && !viable[i][s])
                        .count()
                })
                .sum();
            let mut active = vec![false; cards.len()];
            let mut locked = vec![false; cards.len()];
            let mut row = SignatureStats {
                band,
                attribute: attr,
                context,
                before: indices.len(),
                after: 0,
                score_floor: required,
                cutoff_removed: vec![],
                replacements: vec![],
                lower_power_witnesses: 0,
                cross_attribute_witnesses: 0,
                cross_role_attribute_witnesses: 0,
                cross_role_band_witnesses: 0,
                unification_guard: relax_unification,
                unification_nonmonotone_transitions: 0,
                cross_duration_witnesses: 0,
                cross_replacements: vec![],
                cross_domain_cards: 0,
                cross_domain_characters: 0,
                cross_domain_refreshed: false,
                affine_checks: 0,
                affine_passes: 0,
                forced_product: product_stats,
                cover_checks: 0,
                precise_checks: 0,
                precise_passes: 0,
                precise_removals: 0,
                precise_prefixes: 0,
                cover_seconds: 0.,
                song_obligations_removed,
                song_specific_removals: 0,
                legacy_framework: None,
            };
            // Changing an attribute can turn a mixed team into a uniform team.
            // Only permit this when that transition cannot weaken ANY affected
            // skill. Uniform pools keep the attribute, so transitions never
            // return to mixed and cannot create cross-signature cycles.
            let unification = if relax_unification {
                let mut monotone = vec![[true; 4]; cards.len()];
                for &i in &indices {
                    for destination in 0..4 {
                        if destination != context && destination & context == context {
                            for model in models.iter_mut() {
                                monotone[i][destination] &= model
                                    .loss_coefficient(cases[i][destination], cases[i][context])?
                                    == Some(0.);
                            }
                            row.unification_nonmonotone_transitions +=
                                usize::from(!monotone[i][destination]);
                        }
                    }
                }
                Some(unification::Guard::new(
                    context,
                    cards
                        .iter()
                        .map(|c| unification::Identity {
                            band: c.band_id,
                            attribute: c.attribute as u8,
                        })
                        .collect(),
                    &indices,
                    &monotone,
                    prior,
                ))
            } else {
                None
            };
            let mut can_unify = BTreeMap::new();
            if unification.is_none() && attr.is_none() {
                for &i in &indices {
                    let key = format!("{:?}", cards[i].attribute);
                    let mut monotone = true;
                    for model in models.iter_mut() {
                        monotone &= model
                            .loss_coefficient(cases[i][context + 2], cases[i][context])?
                            == Some(0.);
                    }
                    can_unify
                        .entry(key)
                        .and_modify(|v: &mut bool| *v &= monotone)
                        .or_insert(monotone);
                }
            }
            let mut can_unify_band = BTreeMap::new();
            if unification.is_none() && band.is_none() && cross_character {
                for &i in &indices {
                    let mut monotone = true;
                    for model in models.iter_mut() {
                        monotone &= model
                            .loss_coefficient(cases[i][context + 1], cases[i][context])?
                            == Some(0.);
                    }
                    can_unify_band
                        .entry(cards[i].band_id)
                        .and_modify(|v: &mut bool| *v &= monotone)
                        .or_insert(monotone);
                }
            }
            for &i in &indices {
                active[i] = viable[i].iter().any(|v| *v);
                if !active[i] {
                    row.cutoff_removed.push(cards[i].card_id);
                }
            }
            let (mut ranges, mut affine_contexts) = replacement_context(
                &groups, cards, powers, cases, context, models, required, affine,
            );
            // Ranking only picks which witnesses to retain; signed comparisons
            // examine both power directions and are not restricted by ranking.
            let rank = |i: usize| ranking[i];
            indices.sort_by(|&a, &b| {
                rank(a)
                    .total_cmp(&rank(b))
                    .then(cards[b].card_id.cmp(&cards[a].card_id))
            });
            for &i in &indices {
                if !active[i] || locked[i] {
                    continue;
                }
                let mut candidates: Vec<_> = groups[&cards[i].character_id]
                    .iter()
                    .copied()
                    .filter(|&j| {
                        j != i
                            && active[j]
                            && unification.as_ref().map_or_else(
                                || {
                                    cards[j].attribute == cards[i].attribute
                                        || (prior.is_none()
                                            && attr.is_none()
                                            && can_unify[&format!("{:?}", cards[j].attribute)])
                                },
                                |guard| guard.allows(i, j),
                            )
                    })
                    .collect();
                candidates.sort_by(|&a, &b| {
                    rank(b)
                        .total_cmp(&rank(a))
                        .then(cards[a].card_id.cmp(&cards[b].card_id))
                });
                let mut witnesses = vec![];
                for j in candidates {
                    let mut dominates = true;
                    for s in 0..3 {
                        if !viable[i][s] {
                            continue;
                        }
                        let mut ok = if cross_duration {
                            models[s].can_replace_extended_context(
                                cases[j][context],
                                cases[i][context],
                                powers[s][&cards[j].card_id],
                                powers[s][&cards[i].card_id],
                                ranges[&cards[i].character_id][s],
                                coefficient[s],
                                required[s],
                            )?
                        } else {
                            models[s].can_replace_in_context(
                                cases[j][context],
                                cases[i][context],
                                powers[s][&cards[j].card_id],
                                powers[s][&cards[i].card_id],
                                ranges[&cards[i].character_id][s],
                                coefficient[s],
                                required[s],
                            )?
                        };
                        if !ok && affine {
                            row.affine_checks += 1;
                            ok = models[s].can_replace_affine_context(
                                cases[j][context],
                                cases[i][context],
                                powers[s][&cards[j].card_id],
                                powers[s][&cards[i].card_id],
                                &affine_contexts[&cards[i].character_id][s],
                                cross_duration,
                            )?;
                            row.affine_passes += usize::from(ok);
                        }
                        if !ok {
                            dominates = false;
                            break;
                        }
                    }
                    if dominates {
                        witnesses.push(j);
                    }
                    if witnesses.len() == 3 {
                        break;
                    }
                }
                if witnesses.len() == 3 {
                    active[i] = false;
                    for &j in &witnesses {
                        locked[j] = true;
                        row.cross_attribute_witnesses +=
                            usize::from(cards[j].attribute != cards[i].attribute);
                        row.cross_duration_witnesses += usize::from(
                            models[0].different_duration(cases[j][context], cases[i][context]),
                        );
                        row.lower_power_witnesses += usize::from((0..3).any(|s| {
                            viable[i][s]
                                && powers[s][&cards[j].card_id].linear_total()
                                    < powers[s][&cards[i].card_id].linear_total()
                        }));
                    }
                    row.replacements.push([
                        cards[i].card_id,
                        cards[witnesses[0]].card_id,
                        cards[witnesses[1]].card_id,
                        cards[witnesses[2]].card_id,
                    ]);
                }
            }
            if cross_character {
                use super::timing_cover::{self, Group as CoverGroup};
                if prior.is_some() && timing_cover::repeat_mode() >= 2 {
                    // A same-role replacement either stays in this condition
                    // or leaves it monotonically. Rebuild this condition from
                    // its survivors once, retaining all witness locks.
                    let fresh: BTreeMap<u32, Vec<usize>> = groups
                        .iter()
                        .filter_map(|(&ch, members)| {
                            let members: Vec<_> =
                                members.iter().copied().filter(|&j| active[j]).collect();
                            (!members.is_empty()).then_some((ch, members))
                        })
                        .collect();
                    if fresh.len() >= 5 {
                        groups = fresh;
                        let ids: Vec<_> = groups
                            .values()
                            .flatten()
                            .map(|&j| cases[j][context])
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect();
                        coefficient = std::array::from_fn(|s| models[s].coefficient_upper(&ids));
                        (ranges, affine_contexts) = replacement_context(
                            &groups, cards, powers, cases, context, models, required, affine,
                        );
                        row.cross_domain_refreshed = true;
                    }
                }
                row.cross_domain_cards = groups.values().map(Vec::len).sum();
                row.cross_domain_characters = groups.len();
                let required_breaks = u8::from(band.is_none()) | (u8::from(attr.is_none()) << 1);
                for &i in &indices {
                    if !active[i] || locked[i] {
                        continue;
                    }
                    // With the guard, any added uniformity is a nonnegative
                    // gain at identical timing, including a joint upgrade.
                    let mut candidates: Vec<_> = indices
                        .iter()
                        .copied()
                        .filter(|&j| {
                            j != i
                                && active[j]
                                && unification.as_ref().map_or_else(
                                    || {
                                        cards[j].attribute == cards[i].attribute
                                            && (cards[j].band_id == cards[i].band_id
                                                || (prior.is_none()
                                                    && band.is_none()
                                                    && can_unify_band[&cards[j].band_id]))
                                    },
                                    |guard| guard.allows(i, j),
                                )
                        })
                        .collect();
                    candidates.sort_by(|&a, &b| {
                        rank(b)
                            .total_cmp(&rank(a))
                            .then(cards[a].card_id.cmp(&cards[b].card_id))
                    });
                    let mut witnesses = vec![];
                    let mut song_witnesses: [Vec<usize>; 3] = std::array::from_fn(|_| vec![]);
                    for j in candidates {
                        let mut dominates = true;
                        for s in 0..3 {
                            if !viable[i][s] {
                                continue;
                            }
                            let mut ok = if cross_duration {
                                models[s].can_replace_extended_context(
                                    cases[j][context],
                                    cases[i][context],
                                    powers[s][&cards[j].card_id],
                                    powers[s][&cards[i].card_id],
                                    ranges[&cards[i].character_id][s],
                                    coefficient[s],
                                    required[s],
                                )?
                            } else {
                                models[s].can_replace_in_context(
                                    cases[j][context],
                                    cases[i][context],
                                    powers[s][&cards[j].card_id],
                                    powers[s][&cards[i].card_id],
                                    ranges[&cards[i].character_id][s],
                                    coefficient[s],
                                    required[s],
                                )?
                            };
                            if !ok && affine {
                                row.affine_checks += 1;
                                ok = models[s].can_replace_affine_context(
                                    cases[j][context],
                                    cases[i][context],
                                    powers[s][&cards[j].card_id],
                                    powers[s][&cards[i].card_id],
                                    &affine_contexts[&cards[i].character_id][s],
                                    cross_duration,
                                )?;
                                row.affine_passes += usize::from(ok);
                            }
                            if !ok {
                                dominates = false;
                                if cover_mode < 4 {
                                    break;
                                }
                            } else if cover_mode >= 4 {
                                song_witnesses[s].push(j);
                            }
                        }
                        if dominates {
                            witnesses.push(j);
                        }
                    }
                    let target_songs =
                        (0..3).fold(0, |mask, s| mask | (u8::from(viable[i][s]) << s));
                    let mut coverage = |list: &[usize], songs: u8| {
                        let started = bangdream_optimize_core::timing::Instant::now();
                        row.cover_checks += 1;
                        let covered: Vec<_> = groups
                            .iter()
                            .map(|(&ch, members)| CoverGroup {
                                character: ch,
                                breaks: members
                                    .iter()
                                    .map(|&j| {
                                        u8::from(cards[j].band_id != cards[i].band_id)
                                            | (u8::from(cards[j].attribute != cards[i].attribute)
                                                << 1)
                                    })
                                    .collect::<BTreeSet<_>>()
                                    .into_iter()
                                    .collect(),
                                witnesses: list
                                    .iter()
                                    .filter(|&&j| cards[j].character_id == ch)
                                    .count(),
                            })
                            .collect();
                        let coarse = timing_cover::free_replacements(
                            &covered,
                            cards[i].character_id,
                            required_breaks,
                        )
                        .unwrap_or(0);
                        let mut free = coarse;
                        if coarse == 0 && cover_mode > 0 && !list.is_empty() {
                            row.precise_checks += 1;
                            let masks: Vec<Vec<_>> = covered
                                .iter()
                                .map(|g| {
                                    list.iter()
                                        .filter(|&&j| cards[j].character_id == g.character)
                                        .map(|&j| eligibility[j])
                                        .collect()
                                })
                                .collect();
                            free = timing_cover::precise_free_replacements(
                                &covered,
                                &masks,
                                cards[i].character_id,
                                required_breaks,
                                songs,
                            )
                            .unwrap_or(0);
                            row.precise_passes += usize::from(free > 0);
                        }
                        row.cover_seconds += started.elapsed().as_secs_f64();
                        (free, coarse)
                    };
                    let (guaranteed_free, coarse_free, full_coarse_free, by_song) =
                        if let Some(proof) = minimal_cover(&witnesses, target_songs, &mut coverage)
                        {
                            witnesses = proof.witnesses;
                            (proof.free, proof.coarse, proof.full_coarse, vec![])
                        } else if cover_mode >= 4 {
                            let Some(proofs) =
                                song_covers(&song_witnesses, target_songs, &mut coverage)
                            else {
                                continue;
                            };
                            // Each possible target song has its own sufficient
                            // witness set. Lock their UNION to preserve every
                            // proof; never treat one song's dominators as valid
                            // substitutes on another song.
                            witnesses = proofs
                                .iter()
                                .flat_map(|(_, p)| p.witnesses.iter().copied())
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect();
                            let free = proofs.iter().map(|(_, p)| p.free).min().unwrap();
                            let coarse = proofs.iter().map(|(_, p)| p.coarse).min().unwrap();
                            let rows = proofs
                                .into_iter()
                                .map(|(song, p)| SongReplacement {
                                    song,
                                    witnesses: p
                                        .witnesses
                                        .iter()
                                        .map(|&j| cards[j].card_id)
                                        .collect(),
                                    guaranteed_free: p.free,
                                    coarse_free: p.coarse,
                                })
                                .collect();
                            (free, coarse, 0, rows)
                        } else {
                            continue;
                        };
                    row.song_specific_removals += usize::from(!by_song.is_empty());
                    row.precise_removals += usize::from(full_coarse_free == 0);
                    row.precise_prefixes += usize::from(coarse_free == 0);
                    active[i] = false;
                    for &j in &witnesses {
                        locked[j] = true;
                        row.cross_role_attribute_witnesses +=
                            usize::from(cards[j].attribute != cards[i].attribute);
                        row.cross_role_band_witnesses +=
                            usize::from(cards[j].band_id != cards[i].band_id);
                        row.cross_duration_witnesses += usize::from(
                            models[0].different_duration(cases[j][context], cases[i][context]),
                        );
                    }
                    row.cross_replacements.push(CrossReplacement {
                        removed: cards[i].card_id,
                        guaranteed_free,
                        coarse_free,
                        full_coarse_free,
                        target_songs,
                        by_song,
                        witnesses: witnesses.iter().map(|&j| cards[j].card_id).collect(),
                    });
                }
            }
            for &i in &indices {
                allowed[i][context] = active[i];
            }
            row.after = indices.iter().filter(|&&i| active[i]).count();
            stats.push(row);
        }
    }
    Ok((allowed, stats, eligibility_stats))
}

fn replacement_context(
    groups: &BTreeMap<u32, Vec<usize>>,
    cards: &[PreparedCard],
    powers: &[BTreeMap<u32, MemberPower>],
    cases: &[[usize; 4]],
    context: usize,
    models: &[TimingReplacementBounds],
    required: [f64; 3],
    affine: bool,
) -> (
    BTreeMap<u32, [[f64; 2]; 3]>,
    BTreeMap<u32, Vec<TimingAffineContext>>,
) {
    // Every character has a four-OTHER-character power range; never
    // use its own alternatives as a fictitious teammate.
    let ranges: BTreeMap<_, [[f64; 2]; 3]> = groups
        .keys()
        .map(|&ch| {
            (
                ch,
                std::array::from_fn(|s| {
                    let mut low = vec![];
                    let mut high = vec![];
                    for (&other, list) in groups {
                        if other == ch {
                            continue;
                        }
                        low.push(
                            list.iter()
                                .map(|&i| powers[s][&cards[i].card_id].linear_total())
                                .fold(f64::INFINITY, f64::min),
                        );
                        high.push(
                            list.iter()
                                .map(|&i| powers[s][&cards[i].card_id].linear_total())
                                .fold(0_f64, f64::max),
                        );
                    }
                    low.sort_by(f64::total_cmp);
                    high.sort_by(|a, b| b.total_cmp(a));
                    [
                        low[..4]
                            .iter()
                            .fold(0_f64, |a, b| (a + b).next_down().max(0.)),
                        high[..4].iter().fold(0_f64, |a, b| (a + b).next_up()),
                    ]
                }),
            )
        })
        .collect();
    let affine_contexts: BTreeMap<u32, Vec<TimingAffineContext>> = if affine {
        let rows: Vec<Vec<_>> = (0..3)
            .map(|s| {
                groups
                    .iter()
                    .map(|(&ch, indices)| {
                        (
                            ch,
                            indices
                                .iter()
                                .map(|&i| models[s].scenario_coefficients(cases[i][context]))
                                .collect(),
                        )
                    })
                    .collect()
            })
            .collect();
        groups
            .keys()
            .map(|&ch| {
                (
                    ch,
                    (0..3)
                        .map(|s| TimingAffineContext {
                            other_power: ranges[&ch][s],
                            other_coefficient: companion_ranges(&rows[s], ch),
                            required_score: required[s],
                        })
                        .collect(),
                )
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    (ranges, affine_contexts)
}

struct CoverProof {
    witnesses: Vec<usize>,
    free: usize,
    coarse: usize,
    full_coarse: usize,
}

fn minimal_cover(
    list: &[usize],
    songs: u8,
    coverage: &mut impl FnMut(&[usize], u8) -> (usize, usize),
) -> Option<CoverProof> {
    let (free, full_coarse) = coverage(list, songs);
    if free == 0 {
        return None;
    }
    let (mut low, mut high) = (1, list.len());
    while low < high {
        let mid = (low + high) / 2;
        if coverage(&list[..mid], songs).0 > 0 {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    let witnesses = list[..low].to_vec();
    let (free, coarse) = coverage(&witnesses, songs);
    Some(CoverProof {
        witnesses,
        free,
        coarse,
        full_coarse,
    })
}

fn song_covers(
    lists: &[Vec<usize>; 3],
    songs: u8,
    coverage: &mut impl FnMut(&[usize], u8) -> (usize, usize),
) -> Option<Vec<(usize, CoverProof)>> {
    let mut proofs = vec![];
    for song in 0..3 {
        if songs & (1 << song) != 0 {
            proofs.push((song, minimal_cover(&lists[song], 1 << song, coverage)?));
        }
    }
    if proofs.is_empty() {
        None
    } else {
        Some(proofs)
    }
}

/// Four distinct characters occupy four DISTINCT first-round slots. If the
/// removed card is not captain, one of those very same teammates must provide
/// the encore: its normal+encore row is never assembled from different cards.
fn companion_ranges(groups: &[(u32, Vec<[[f64; 2]; 10]>)], excluded: u32) -> [[f64; 2]; 10] {
    let mut dp = [[f64::INFINITY, f64::NEG_INFINITY]; 64];
    dp[0] = [0., 0.];
    for (_, cards) in groups.iter().filter(|(ch, _)| *ch != excluded) {
        let row: [[f64; 2]; 10] = std::array::from_fn(|s| {
            [
                cards.iter().map(|c| c[s][0]).fold(f64::INFINITY, f64::min),
                cards
                    .iter()
                    .map(|c| c[s][1])
                    .fold(f64::NEG_INFINITY, f64::max),
            ]
        });
        let previous = dp;
        for (state, prior) in previous.into_iter().enumerate() {
            let mask = state & 31;
            if mask.count_ones() >= 4 || !prior[0].is_finite() {
                continue;
            }
            for slot in 0..5 {
                if mask & (1 << slot) != 0 {
                    continue;
                }
                for captain in 0..2 {
                    if captain == 1 && state & 32 != 0 {
                        continue;
                    }
                    let next = state | (1 << slot) | (captain << 5);
                    let r = row[slot + captain * 5];
                    dp[next][0] = dp[next][0].min((prior[0] + r[0]).next_down());
                    dp[next][1] = dp[next][1].max((prior[1] + r[1]).next_up());
                }
            }
        }
    }
    std::array::from_fn(|s| dp[(31 ^ (1 << (s % 5))) | if s < 5 { 32 } else { 0 }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_specific_cover_requires_every_target_song_and_locks_the_union() {
        use super::super::timing_cover::{precise_free_replacements, Group};
        let global_masks = [1, 1, 2, 2];
        let mut coverage = |list: &[usize], songs: u8| {
            let groups: Vec<_> = (0..5)
                .map(|character| Group {
                    character,
                    breaks: vec![0],
                    witnesses: if character == 0 { list.len() } else { 0 },
                })
                .collect();
            let masks = vec![
                list.iter().map(|&i| global_masks[i]).collect(),
                vec![],
                vec![],
                vec![],
                vec![],
            ];
            let coarse = super::super::timing_cover::free_replacements(&groups, 0, 0).unwrap();
            (
                precise_free_replacements(&groups, &masks, 0, 0, songs).unwrap(),
                coarse,
            )
        };
        let mut lists = [vec![0, 1], vec![2, 3], vec![]];
        let proofs = song_covers(&lists, 3, &mut coverage).unwrap();
        assert_eq!(
            proofs.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(proofs[0].1.witnesses, vec![0]);
        assert_eq!(proofs[1].1.witnesses, vec![2]);
        let locked: BTreeSet<_> = proofs
            .iter()
            .flat_map(|(_, p)| p.witnesses.iter().copied())
            .collect();
        assert_eq!(locked, BTreeSet::from([0, 2]));
        lists[1].clear();
        assert!(song_covers(&lists, 3, &mut coverage).is_none());
        assert!(song_covers(&lists, 1, &mut coverage).is_some());
        assert!(song_covers(&lists, 0, &mut coverage).is_none());
    }

    #[test]
    fn teammate_dp_matches_distinct_character_slot_and_captain_enumeration() {
        let groups: Vec<_> = (0..6)
            .map(|ch| {
                (
                    ch,
                    (0..2)
                        .map(|card| {
                            let slots: [f64; 6] = std::array::from_fn(|s| {
                                ((ch as usize * 19 + card * 11 + s * 7) % 23) as f64 - 5.
                            });
                            std::array::from_fn(|s| {
                                let v = slots[s % 5] + if s >= 5 { slots[5] } else { 0. };
                                [v, v]
                            })
                        })
                        .collect(),
                )
            })
            .collect();
        for excluded in 0..6 {
            let actual = companion_ranges(&groups, excluded);
            for scenario in 0..10 {
                let slots: Vec<_> = (0..5).filter(|&s| s != scenario % 5).collect();
                let mut expected = [f64::INFINITY, f64::NEG_INFINITY];
                fn visit(
                    groups: &[(u32, Vec<[[f64; 2]; 10]>)],
                    slots: &[usize],
                    used: u32,
                    captain: bool,
                    need_captain: bool,
                    sum: f64,
                    result: &mut [f64; 2],
                ) {
                    if slots.is_empty() {
                        if captain == need_captain {
                            result[0] = result[0].min(sum);
                            result[1] = result[1].max(sum);
                        }
                        return;
                    }
                    for (ch, cards) in groups {
                        if used & (1 << ch) != 0 {
                            continue;
                        }
                        for card in cards {
                            visit(
                                groups,
                                &slots[1..],
                                used | (1 << ch),
                                captain,
                                need_captain,
                                sum + card[slots[0]][0],
                                result,
                            );
                            if need_captain && !captain {
                                visit(
                                    groups,
                                    &slots[1..],
                                    used | (1 << ch),
                                    true,
                                    need_captain,
                                    sum + card[slots[0] + 5][0],
                                    result,
                                );
                            }
                        }
                    }
                }
                visit(
                    &groups,
                    &slots,
                    1 << excluded,
                    false,
                    scenario < 5,
                    0.,
                    &mut expected,
                );
                assert!(actual[scenario][0] <= expected[0] && actual[scenario][1] >= expected[1]);
                assert!((actual[scenario][0] - expected[0]).abs() < 1e-10);
                assert!((actual[scenario][1] - expected[1]).abs() < 1e-10);
            }
        }
    }
}
