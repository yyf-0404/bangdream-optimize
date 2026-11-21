//! Whole-inventory experimental census. No unchecked legacy contribution pruning.
use bangdream_optimize_core::medley::key_pair_bound;
use bangdream_optimize_core::{
    hit_timing::{
        bounds::{
            PoolTimingChoice, PoolTimingUpper, TimingReplacementBounds, TimingReplacementStats,
        },
        ReplayInput, ReplaySkill,
    },
    scoring::{MemberPower, ScoringPower},
    Attribute, PreparedCard, Server,
};
use bangdream_optimize_data::{bestdori::timing_skill::TimingTeamCard, BestdoriData};
use serde::Serialize;
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap},

};
type Error = Box<dyn std::error::Error>;
/// Research-only oracle switch for identical model/requests in timing comparisons.
pub(super) fn window_options() -> bangdream_optimize_core::hit_timing::independent::WindowOptions {
    bangdream_optimize_core::hit_timing::independent::WindowOptions {
        max_cached_component_edges: std::env::var("BANDORI_TIMING_COMPONENT_CACHE_EDGES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(
                bangdream_optimize_core::hit_timing::independent::WindowOptions::default()
                    .max_cached_component_edges,
            ),
        fast_net_queries: std::env::var_os("BANDORI_TIMING_ENABLE_NET_REUSE").is_some()
            && std::env::var_os("BANDORI_TIMING_LEGACY_NET_QUERIES").is_none(),
        compact_value_states: std::env::var("BANDORI_TIMING_COMPACT_VALUE_STATES")
            .ok()
            .map(|v| v == "1")
            .unwrap_or(
                bangdream_optimize_core::hit_timing::independent::WindowOptions::default()
                    .compact_value_states,
            ),
        inline_event_hits: std::env::var("BANDORI_TIMING_INLINE_EVENT_HITS")
            .ok()
            .map(|v| v == "1")
            .unwrap_or(
                bangdream_optimize_core::hit_timing::independent::WindowOptions::default()
                    .inline_event_hits,
            ),
        reuse_graph_storage: std::env::var_os("BANDORI_TIMING_DISABLE_GRAPH_REUSE").is_none()
            && (std::env::var_os("BANDORI_TIMING_ENABLE_GRAPH_REUSE").is_some()
                || bangdream_optimize_core::hit_timing::independent::WindowOptions::default()
                    .reuse_graph_storage),
        ..Default::default()
    }
}

#[path = "timing_pool_projection.rs"]
mod projection;
pub(super) type Choice = [PoolTimingChoice; 3];
use super::timing_pair::{Oracle, Pair, PricedPrefix};
fn union(a: Choice, b: Choice) -> Choice {
    std::array::from_fn(|s| a[s].union(b[s]))
}
fn minimum(a: Choice, b: Choice) -> Choice {
    std::array::from_fn(|s| a[s].minimum_components(b[s]))
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub inventory_cards: usize,
    pub skill_variants: usize,
    pub retained_cards: usize,
    /// Removed ID followed by three retained, simultaneously available witnesses.
    pub replacements: Vec<[u32; 4]>,
    pub contribution_replacements: Vec<[u32; 4]>,
    pub contribution_pruning: bool,
    pub replacement_seconds: f64,
    pub replacement_profiles: Vec<TimingReplacementStats>,
    pub signature_pruning: bool,
    pub cross_duration_pruning: bool,
    pub cross_character_pruning: bool,
    pub affine_pruning: bool,
    pub frame_pruning: bool,
    pub net_bounds: bool,
    pub state_net_bounds: bool,
    pub frontier_net_bounds: bool,
    pub adjacent_net_bounds: bool,
    pub forced_product_bounds: bool,
    pub forced_product_mode: u8,
    pub precise_cover_mode: u8,
    pub repeat_cover_mode: u8,
    pub unification_cover_mode: u8,
    pub legacy_prune_framework: bool,
    pub song_eligibility: Option<super::timing_eligibility::Stats>,
    pub separable_gate: bool,
    pub separable_queries: u64,
    pub separable_rejected: u64,
    pub lazy_song_bounds: bool,
    pub lazy_song_checks: u64,
    pub lazy_song_refinements: [u64; 3],
    pub adjacent_profiles: Vec<bangdream_optimize_core::hit_timing::bounds::TimingAdjacentStats>,
    pub net_profiles: Vec<bangdream_optimize_core::hit_timing::bounds::TimingNetStats>,
    pub assignment_profiles: Vec<serde_json::Value>,
    pub signature_seconds: f64,
    pub signature_retained_cards: usize,
    pub signatures: Vec<super::timing_replacement::SignatureStats>,
    pub refinement: Option<super::timing_refine::Stats>,
    pub preparation_seconds: f64,
    pub single_upper_seconds: f64,
    pub enumeration_seconds: f64,
    pub enumeration_profile: Option<EnumerationProfile>,
    pub single_upper: [u64; 3],
    pub pair_seconds: f64,
    pub pair_bounds: Vec<super::timing_pair::Stats>,
    pub joint_power_bins: usize,
    pub joint_power_seconds: f64,
    pub joint_price_sets: usize,
    pub blocking_bounds: bool,
    pub blocking_key_cards: usize,
    pub price_key_cards: Vec<u32>,
    pub early_key_pair_bounds: Option<key_pair_bound::Stats>,
    pub prefix_key_pair_bounds: Option<projection::Stats>,
    pub blocking_seconds: f64,
    pub blocking_price_sets: usize,
    pub triple_prices: Option<super::timing_pair::TripleStats>,
    pub priced_prefix: bool,
    pub price_prefix_pruned: u64,
    pub price_card_seconds: f64,
    pub price_retained_cards: usize,
    pub price_removed_cards: Vec<u32>,
    pub seed_score: u64,
    pub local_seed_score: u64,
    pub group_nodes: u64,
    pub upper_group_nodes: u64,
    pub card_nodes: u64,
    pub pruned_group_nodes: u64,
    pub pruned_card_nodes: u64,
    pub retained: usize,
    pub qualifying_rosters: u64,
    pub counted_subtrees: u64,
    pub census_only: bool,
    pub small_pool_audited: usize,
    pub complete: bool,
    pub stop_reason: Option<String>,
}
/// Sampled stage times are NOT additive wall times. Scale each stage by its
/// own calls/samples; different depths and early exits have different mixes.
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumerationProfile {
    pub sample_stride: u64,
    pub by_depth: [NodeProfile; 6],
    pub priced_preparation_seconds: f64,
    pub priced_preparations: u64,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeProfile {
    pub nodes: u64,
    pub priced_rejected: u64,
    pub upper_rejected: u64,
    pub priced: StageProfile,
    pub separable: StageProfile,
    pub upper: StageProfile,
    pub companions: StageProfile,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageProfile {
    pub calls: u64,
    pub samples: u64,
    pub sampled_seconds: f64,
}
impl StageProfile {
    fn record(&mut self, start: Option<Instant>) {
        self.calls += 1;
        if let Some(start) = start {
            self.samples += 1;
            self.sampled_seconds += start.elapsed().as_secs_f64();
        }
    }
}
struct Group {
    cards: Vec<usize>,
    case_cards: [Vec<usize>; 2],
    relaxed: Choice,
    cases: [Choice; 2],
}
/// Each suffix uses the original left fold, preserving complete gain rows and
/// their floating-point bit patterns. A reverse fold can change row identity.
fn suffix_unions(groups: &[Group]) -> Option<Vec<Choice>> {
    if std::env::var("BANDORI_TIMING_DISABLE_SUFFIX_UNIONS").as_deref() == Ok("1") {
        return None;
    }
    Some(
        (0..=groups.len())
            .map(|from| remaining_union(groups, from, None))
            .collect(),
    )
}
fn remaining_union(groups: &[Group], from: usize, suffix: Option<&[Choice]>) -> Choice {
    if let Some(suffix) = suffix {
        suffix[from]
    } else {
        groups[from..]
            .iter()
            .map(|g| g.relaxed)
            .reduce(union)
            .unwrap_or_default()
    }
}
struct Leaf {
    indices: [usize; 5],
    /// Already computed by the complete five-member enumeration. This remains
    /// an upper, before resolving native layout powers or detailed timelines.
    upper: [u64; 3],
}
struct Scan<'a> {
    models: Vec<PoolTimingUpper>,
    choices: Vec<[Choice; 4]>,
    cards: &'a [PreparedCard],
    max_nodes: u64,
    max_teams: usize,
    stats: Stats,
    output: Vec<Leaf>,
    pairs: Vec<Pair>,
    power_sums: Vec<[f64; 3]>,
    prefix_bound: Option<key_pair_bound::PrefixUpper>,
    separable_queries: Cell<u64>,
    separable_rejected: Cell<u64>,
    lazy_song_checks: Cell<u64>,
    lazy_song_refinements: Cell<[u64; 3]>,
}

pub fn enumerate_pool(
    cards: &[PreparedCard],
    powers: &[BTreeMap<u32, MemberPower>],
    data: &BestdoriData,
    server: Server,
    inputs: &[ReplayInput; 3],
    seed: u64,
    local_seed: u64,
    max_teams: usize,
    max_nodes: u64,
    census_only: bool,
    plan_only: bool,
    pair_bounds: bool,
    priced_prefix: bool,
    price_card_filter: bool,
    joint_power_bins: usize,
    adaptive_prices: bool,
    contribution_pruning: bool,
    signature_pruning: bool,
    cross_duration_pruning: bool,
    cross_character_pruning: bool,
    affine_pruning: bool,
    frame_pruning: bool,
    net_bounds: bool,
    state_net_bounds: bool,
    frontier_net_bounds: bool,
    adjacent_net_bounds: bool,
    refine_pools: bool,
    refine_cutoffs: bool,
    refine_replacements: bool,
    blocking_bounds: bool,
    blocking_key_cards: usize,
    early_key_pair_bounds: bool,
    prefix_key_pair_bounds: bool,
    model_cache: &mut super::timing_model_cache::Models,
) -> Result<(Vec<[usize; 5]>, Stats), Error> {
    if early_key_pair_bounds && (census_only || seed == 0) {
        return Err(
            "early key-pair filtering needs materialized rosters and a validated seed".into(),
        );
    }
    if prefix_key_pair_bounds && (census_only || seed == 0) {
        return Err("prefix key-pair bounds need materialized rosters and a validated seed".into());
    }
    if refine_pools && !signature_pruning {
        return Err("pool refinement requires conditional pruning".into());
    }
    if joint_power_bins > 64 {
        return Err("joint power bins must be 0..64".into());
    }
    if cards.iter().any(|c| c.attribute == Attribute::All) {
        return Err("real-card pool census requires one of the four game attributes".into());
    }
    let start = Instant::now();
    let mut keys = HashMap::new();
    let mut skills: Vec<ReplaySkill> = vec![];
    let mut cases = vec![];
    for card in cards {
        let resolved = data.hit_timing_skill_cases(
            server,
            TimingTeamCard {
                card_id: card.card_id,
                skill_level: card.skill_level,
            },
        )?;
        let mut ids = [0; 4];
        for (i, s) in resolved.into_iter().enumerate() {
            let key = serde_json::to_string(&s.skill)?;
            ids[i] = *keys.entry(key).or_insert_with(|| {
                skills.push(s.skill);
                skills.len() - 1
            });
        }
        cases.push(ids);
    }
    let max_member = powers
        .iter()
        .flat_map(|p| p.values().map(|p| p.linear_total()))
        .fold(0., f64::max);
    // Deliberately wider than the native 5-member accumulation envelope.
    let cap = ScoringPower::new((max_member * 5. * 1.001 + 1.) as f32)?;
    let models = inputs
        .iter()
        .enumerate()
        .map(|(song, i)| {
            if adjacent_net_bounds {
                model_cache.adjacent(song, &skills, cap)
            } else if frontier_net_bounds {
                PoolTimingUpper::new_frontier_net(i, &skills, cap, window_options())
            } else if state_net_bounds {
                PoolTimingUpper::new_stateful_net(i, &skills, cap)
            } else if net_bounds {
                PoolTimingUpper::new_net(i, &skills, cap)
            } else {
                PoolTimingUpper::new(i, &skills, cap)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut choices = vec![];
    for (card, ids) in cards.iter().zip(&cases) {
        let mut rows = [[PoolTimingChoice::default(); 3]; 4];
        for c in 0..4 {
            for s in 0..3 {
                rows[c][s] = models[s].choice(ids[c], powers[s][&card.card_id].linear_total())?;
            }
        }
        choices.push(rows);
    }
    let mut scan = Scan {
        models,
        choices,
        cards,
        max_nodes,
        max_teams,
        output: vec![],
        pairs: vec![],
        prefix_bound: None,
        separable_queries: Cell::new(0),
        separable_rejected: Cell::new(0),
        lazy_song_checks: Cell::new(0),
        lazy_song_refinements: Cell::new([0; 3]),
        power_sums: cards
            .iter()
            .map(|c| std::array::from_fn(|s| powers[s][&c.card_id].linear_total()))
            .collect(),
        stats: Stats {
            inventory_cards: cards.len(),
            skill_variants: skills.len(),
            seed_score: seed,
            local_seed_score: local_seed,
            preparation_seconds: start.elapsed().as_secs_f64(),
            census_only,
            priced_prefix,
            net_bounds,
            state_net_bounds,
            frontier_net_bounds,
            adjacent_net_bounds,
            forced_product_bounds: super::timing_product::enabled(),
            forced_product_mode: super::timing_product::mode(),
            precise_cover_mode: super::timing_cover::mode(),
            repeat_cover_mode: super::timing_cover::repeat_mode(),
            legacy_prune_framework: super::timing_replacement::framework_enabled(),
            unification_cover_mode: super::timing_cover::unification_mode(),
            separable_gate: adjacent_net_bounds
                && std::env::var_os("BANDORI_TIMING_DISABLE_SEPARABLE_GATE").is_none(),
            lazy_song_bounds: std::env::var("BANDORI_TIMING_LAZY_SONG_BOUNDS").as_deref()
                == Ok("1"),
            blocking_bounds,
            blocking_key_cards,
            ..Stats::default()
        },
    };
    let replacement_start = Instant::now();
    let mut replacement_models = if contribution_pruning {
        inputs
            .iter()
            .enumerate()
            .map(|(song, i)| {
                if adjacent_net_bounds {
                    model_cache.replacement(song, &skills, cap, frame_pruning)
                } else {
                    TimingReplacementBounds::new(i, &skills, cap)
                }
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    scan.stats.contribution_pruning = contribution_pruning;
    // Same character/attribute preserves
    // all unification facts. Three retained witnesses are necessary: each of
    // the OTHER two teams can consume at most one card of this character.
    // Equal axes use the deterministic sort order, so witness chains cannot
    // cycle and a removed card is never used as somebody else's witness.
    let mut ranked: Vec<_> = (0..cards.len()).collect();
    let rank = |i: usize| {
        powers
            .iter()
            .map(|p| p[&cards[i].card_id].linear_total())
            .sum::<f64>()
    };
    ranked.sort_by(|&a, &b| {
        rank(b)
            .total_cmp(&rank(a))
            .then(cards[a].card_id.cmp(&cards[b].card_id))
    });
    let mut retained: Vec<usize> = vec![];
    for i in ranked {
        let mut witnesses = Vec::new();
        let mut uses_contribution = false;
        for &j in &retained {
            if cards[i].character_id != cards[j].character_id
                || cards[i].band_id != cards[j].band_id
                || cards[i].attribute != cards[j].attribute
            {
                continue;
            }
            let exact = cases[i] == cases[j]
                && powers
                    .iter()
                    .all(|p| p[&cards[j].card_id].componentwise_dominates(p[&cards[i].card_id]));
            let mut certified = exact;
            if !exact && contribution_pruning {
                certified = true;
                'conditions: for c in 0..4 {
                    for s in 0..3 {
                        if !replacement_models[s].can_replace(
                            cases[j][c],
                            cases[i][c],
                            powers[s][&cards[j].card_id],
                            powers[s][&cards[i].card_id],
                        )? {
                            certified = false;
                            break 'conditions;
                        }
                    }
                }
            }
            if certified {
                uses_contribution |= !exact;
                witnesses.push(j);
                if witnesses.len() == 3 {
                    break;
                }
            }
        }
        if witnesses.len() == 3 {
            scan.stats.replacements.push([
                cards[i].card_id,
                cards[witnesses[0]].card_id,
                cards[witnesses[1]].card_id,
                cards[witnesses[2]].card_id,
            ]);
            if uses_contribution {
                scan.stats
                    .contribution_replacements
                    .push(*scan.stats.replacements.last().unwrap());
            }
        } else {
            retained.push(i);
        }
    }
    scan.stats.retained_cards = retained.len();
    scan.stats.replacement_seconds = replacement_start.elapsed().as_secs_f64();
    scan.stats.replacement_profiles = replacement_models
        .iter()
        .map(|m| m.stats().clone())
        .collect();
    eprintln!(
        "three-witness dominance: {} -> {} cards",
        cards.len(),
        retained.len()
    );
    let mut partitions = vec![];
    for attr in [
        None,
        Some(Attribute::Cool),
        Some(Attribute::Happy),
        Some(Attribute::Pure),
        Some(Attribute::Powerful),
    ] {
        let mut grouped: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for &i in &retained {
            let c = &cards[i];
            if attr.is_none_or(|a| a == c.attribute) {
                grouped.entry(c.character_id).or_default().push(i);
            }
        }
        let mut groups = vec![];
        for (_, mut indices) in grouped {
            indices.sort_by(|&a, &b| {
                let rank = |i: usize| {
                    powers
                        .iter()
                        .map(|p| p[&cards[i].card_id].linear_total())
                        .sum::<f64>()
                };
                rank(b)
                    .total_cmp(&rank(a))
                    .then(cards[a].card_id.cmp(&cards[b].card_id))
            });
            let c = if attr.is_some() { 2 } else { 0 };
            let cases = std::array::from_fn(|b| {
                indices
                    .iter()
                    .map(|&i| scan.choices[i][c + b])
                    .reduce(union)
                    .unwrap()
            });
            groups.push(Group {
                case_cards: [indices.clone(), indices.clone()],
                cards: indices,
                relaxed: union(cases[0], cases[1]),
                cases,
            });
        }
        groups.sort_by_key(|g| std::cmp::Reverse(scan.upper(&[g.relaxed; 5]).iter().sum::<u64>()));
        partitions.push((attr, groups));
    }
    let start = Instant::now();
    // Character-group maxima give a certified single-song cap. Each group's
    // card and conditional skill choices remain relaxed, not an actual team.
    for (_, groups) in &partitions {
        let suffix = suffix_unions(groups);
        scan.groups(groups, suffix.as_deref(), &mut vec![], 0, true, None)?;
        if scan.stats.stop_reason.is_some() {
            break;
        }
    }
    scan.stats.single_upper_seconds = start.elapsed().as_secs_f64();
    if scan.stats.stop_reason.is_some() {
        scan.stats.separable_queries = scan.separable_queries.get();
        scan.stats.separable_rejected = scan.separable_rejected.get();
        return Ok((
            scan.output.into_iter().map(|r| r.indices).collect(),
            scan.stats,
        ));
    }
    eprintln!(
        "full-pool single-song upper {:?}, seed {}",
        scan.stats.single_upper, seed
    );
    let mut eligible = vec![true; cards.len()];
    let mut conditional = vec![[true; 4]; cards.len()];
    if pair_bounds || priced_prefix || price_card_filter || joint_power_bins > 0 || adaptive_prices
    {
        let start = Instant::now();
        let oracles: Vec<_> = (0..3)
            .map(|s| {
                Oracle::new(
                    &scan.models[s],
                    &scan.choices,
                    cards,
                    &retained,
                    &powers[s],
                    s,
                )
            })
            .collect();
        scan.pairs = (0..3)
            .map(|s| {
                Pair::new(
                    &oracles[(s + 1) % 3],
                    &oracles[(s + 2) % 3],
                    cards.len(),
                    64,
                )
            })
            .collect();
        if priced_prefix || price_card_filter || adaptive_prices {
            scan.stats.triple_prices = Some(Pair::add_triple_prices(
                &mut scan.pairs,
                &oracles,
                cards.len(),
                local_seed,
                adaptive_prices,
            ));
        }
        scan.stats.pair_seconds = start.elapsed().as_secs_f64();
        eprintln!(
            "priced two-song upper {:?}: {:.3}s",
            scan.pairs
                .iter()
                .map(|p| p.stats.root_upper)
                .collect::<Vec<_>>(),
            scan.stats.pair_seconds
        );
        if joint_power_bins > 0 {
            let start = Instant::now();
            let joint: Vec<_> = (0..3)
                .map(|s| {
                    Oracle::with_power_bins(
                        &scan.models[s],
                        &scan.choices,
                        cards,
                        &retained,
                        &powers[s],
                        s,
                        joint_power_bins,
                    )
                })
                .collect();
            scan.stats.joint_price_sets = Pair::tighten_joint(&mut scan.pairs, &joint);
            scan.stats.joint_power_bins = joint_power_bins;
            scan.stats.joint_power_seconds = start.elapsed().as_secs_f64();
            eprintln!(
                "joint power range upper {:?}: {:.3}s",
                scan.pairs
                    .iter()
                    .map(|p| p.stats.root_upper)
                    .collect::<Vec<_>>(),
                scan.stats.joint_power_seconds
            );
        }
        if price_card_filter {
            let start = Instant::now();
            let uppers = Pair::card_uppers(&scan.pairs, &oracles, cards.len());
            eligible = uppers
                .iter()
                .map(|u| u.iter().any(|&v| v >= seed))
                .collect();
            scan.stats.price_retained_cards = retained.iter().filter(|&&i| eligible[i]).count();
            scan.stats.price_removed_cards = retained
                .iter()
                .filter(|&&i| !eligible[i])
                .map(|&i| cards[i].card_id)
                .collect();
            for (attr, groups) in &mut partitions {
                for g in groups.iter_mut() {
                    g.cards.retain(|&i| eligible[i]);
                    g.case_cards = [g.cards.clone(), g.cards.clone()];
                    if g.cards.is_empty() {
                        continue;
                    }
                    let c = if attr.is_some() { 2 } else { 0 };
                    g.cases = std::array::from_fn(|b| {
                        g.cards
                            .iter()
                            .map(|&i| scan.choices[i][c + b])
                            .reduce(union)
                            .unwrap()
                    });
                    g.relaxed = union(g.cases[0], g.cases[1]);
                }
                groups.retain(|g| !g.cards.is_empty());
            }
            scan.stats.price_card_seconds = start.elapsed().as_secs_f64();
            eprintln!(
                "forced-member price filter: {} -> {} cards: {:.3}s",
                retained.len(),
                scan.stats.price_retained_cards,
                scan.stats.price_card_seconds
            );
        }
    }
    if signature_pruning {
        if frame_pruning {
            for (s, model) in replacement_models.iter_mut().enumerate() {
                model.tighten_frames(&inputs[s], &skills)?;
            }
        }
        let start = Instant::now();
        let available: Vec<_> = retained.iter().copied().filter(|&i| eligible[i]).collect();
        let other_upper = std::array::from_fn(|s| {
            let independent =
                scan.stats.single_upper[(s + 1) % 3] + scan.stats.single_upper[(s + 2) % 3];
            scan.pairs
                .get(s)
                .map_or(independent, |p| p.upper(&[]).min(independent))
        });
        let (allowed, stats, eligibility) = super::timing_replacement::prune(
            cards,
            &available,
            powers,
            &cases,
            &scan.choices,
            &scan.models,
            &scan.pairs,
            &mut replacement_models,
            other_upper,
            seed,
            cross_duration_pruning,
            cross_character_pruning,
            affine_pruning,
            None,
        )?;
        conditional = allowed;
        scan.stats.song_eligibility = eligibility;
        scan.stats.signature_seconds = start.elapsed().as_secs_f64();
        if plan_only {
            // A character may appear once in each of the three teams. The
            // surviving cards must fill fifteen slots even before assignment.
            let mut per_character = BTreeMap::new();
            let mut slots = 0;
            for &i in &available {
                if conditional[i].iter().any(|&enabled| enabled) {
                    let count = per_character.entry(cards[i].character_id).or_insert(0);
                    if *count < 3 {
                        *count += 1;
                        slots += 1;
                    }
                    if slots == 15 { break; }
                }
            }
            if slots < 15 {
                scan.stats.complete = true;
                return Ok((Vec::new(), scan.stats));
            }
        }
        if refine_pools {
            let mut stats = super::timing_refine::rebuild(
                cards,
                powers,
                inputs,
                &skills,
                &cases,
                &conditional,
                cap,
                &mut scan.models,
                &mut scan.choices,
                &mut scan.pairs,
                &mut scan.stats.single_upper,
                joint_power_bins,
                net_bounds,
                state_net_bounds,
                frontier_net_bounds,
                adjacent_net_bounds,
                model_cache,
            )?;
            if refine_cutoffs {
                for _ in 0..8 {
                    let start = Instant::now();
                    let mut row = super::timing_refine::cutoffs(
                        cards,
                        powers,
                        &mut conditional,
                        &scan.models,
                        &scan.choices,
                        &scan.pairs,
                        scan.stats.single_upper,
                        seed,
                    );
                    stats.cutoff_seconds += start.elapsed().as_secs_f64();
                    if refine_replacements {
                        let start = Instant::now();
                        let other_upper = std::array::from_fn(|s| {
                            let other = scan.stats.single_upper[(s + 1) % 3]
                                + scan.stats.single_upper[(s + 2) % 3];
                            scan.pairs.get(s).map_or(other, |p| p.upper(&[]).min(other))
                        });
                        // Reuse the same pruning framework in later passes.
                        // Prior memberships restrict the input, not witnesses'
                        // possible uniformity destinations.
                        let (next, proofs, eligibility) = super::timing_replacement::prune(
                            cards,
                            &available,
                            powers,
                            &cases,
                            &scan.choices,
                            &scan.models,
                            &scan.pairs,
                            &mut replacement_models,
                            other_upper,
                            seed,
                            cross_duration_pruning,
                            cross_character_pruning
                                && (super::timing_cover::repeat_mode() > 0
                                    || super::timing_replacement::framework_enabled()),
                            affine_pruning,
                            Some(&conditional),
                        )?;
                        assert!(next
                            .iter()
                            .zip(&conditional)
                            .all(|(n, p)| (0..4).all(|c| !n[c] || p[c])));
                        conditional = next;
                        row.replacements = proofs;
                        row.song_eligibility = eligibility;
                        row.replacement_seconds = start.elapsed().as_secs_f64();
                    }
                    row.after_replacement = conditional.iter().flatten().filter(|&&v| v).count();
                    let stable = row.after_replacement == row.before;
                    eprintln!(
                        "refined condition cutoff: {} -> {} memberships",
                        row.before, row.after_replacement
                    );
                    stats.cutoff_rounds.push(row);
                    if stable {
                        stats.cutoff_stable = true;
                        break;
                    }
                    // Keep all existing range certificates. Rebuilding the
                    // cheap unrestricted oracle avoids repeating 64-bin work
                    // on every fixed-point step; minima preserve tight bounds.
                    let next = super::timing_refine::rebuild(
                        cards,
                        powers,
                        inputs,
                        &skills,
                        &cases,
                        &conditional,
                        cap,
                        &mut scan.models,
                        &mut scan.choices,
                        &mut scan.pairs,
                        &mut scan.stats.single_upper,
                        0,
                        net_bounds,
                        state_net_bounds,
                        frontier_net_bounds,
                        adjacent_net_bounds,
                        model_cache,
                    )?;
                    stats.skills_after = next.skills_after;
                    stats.memberships = next.memberships;
                    stats.model_seconds += next.model_seconds;
                    stats.oracle_seconds += next.oracle_seconds;
                    stats.single_after = next.single_after;
                    stats.pairs_after = next.pairs_after;
                    stats.price_sets += next.price_sets;
                }
            }
            eprintln!("rebuilt pool bounds: {} -> {} skills, singles {:?} -> {:?}, pairs {:?} -> {:?}, {:.3}s",
                stats.skills_before, stats.skills_after, stats.single_before, stats.single_after,
                stats.pairs_before, stats.pairs_after, stats.model_seconds + stats.oracle_seconds);
            scan.stats.refinement = Some(stats);
        }
        for (attr, groups) in &mut partitions {
            let c = if attr.is_some() { 2 } else { 0 };
            for g in groups.iter_mut() {
                g.case_cards = std::array::from_fn(|b| {
                    g.cards
                        .iter()
                        .copied()
                        .filter(|&i| conditional[i][c + b])
                        .collect()
                });
                g.cases = std::array::from_fn(|b| {
                    g.case_cards[b]
                        .iter()
                        .map(|&i| scan.choices[i][c + b])
                        .reduce(union)
                        .unwrap_or_default()
                });
                g.relaxed = union(g.cases[0], g.cases[1]);
                g.cards
                    .retain(|&i| conditional[i][c] || conditional[i][c + 1]);
            }
            groups.retain(|g| !g.cards.is_empty());
        }
        scan.stats.signature_pruning = true;
        scan.stats.cross_duration_pruning = cross_duration_pruning;
        scan.stats.cross_character_pruning = cross_character_pruning;
        scan.stats.affine_pruning = affine_pruning;
        scan.stats.frame_pruning = frame_pruning;
        scan.stats.signature_retained_cards =
            conditional.iter().filter(|r| r.iter().any(|x| *x)).count();
        scan.stats.signatures = stats;
        eprintln!(
            "signature pools: {} -> {} union cards, {:.3}s",
            available.len(),
            scan.stats.signature_retained_cards,
            scan.stats.signature_seconds
        );
    }
    if blocking_bounds && !scan.pairs.is_empty() {
        let start = Instant::now();
        let available: Vec<_> = retained
            .iter()
            .copied()
            .filter(|&i| eligible[i] && conditional[i].iter().any(|&v| v))
            .collect();
        let oracles: Vec<_> = (0..3)
            .map(|s| {
                Oracle::conditional(
                    &scan.models[s],
                    &scan.choices,
                    cards,
                    &available,
                    &powers[s],
                    s,
                    0,
                    signature_pruning.then_some(conditional.as_slice()),
                )
            })
            .collect();
        scan.stats.blocking_price_sets = Pair::prepare_blocking(
            &mut scan.pairs,
            &oracles,
            &cards.iter().map(|c| c.character_id).collect::<Vec<_>>(),
            blocking_key_cards,
        );
        scan.stats.blocking_seconds = start.elapsed().as_secs_f64();
        eprintln!(
            "blocked-card upper tables: {} price sets, {:?} affected cards, {:.3}s",
            scan.stats.blocking_price_sets,
            scan.pairs
                .iter()
                .map(|p| p.stats.blocking_cards)
                .collect::<Vec<_>>(),
            scan.stats.blocking_seconds
        );
    }
    if prefix_key_pair_bounds {
        let (bound, stats) = projection::prepare(&scan, &partitions, plan_only)?;
        eprintln!("projected prefix keys: {} masks, {} groups / {} members, complete={}, {:.3}s envelopes + {:.3}s table",
            stats.occupied_masks, stats.group_nodes, stats.member_nodes, stats.complete,
            stats.envelope_seconds, stats.table_seconds);
        let no_plan = stats.excludes_all_plans;
        scan.prefix_bound = bound;
        scan.stats.prefix_key_pair_bounds = Some(stats);
        if no_plan {
            scan.stats.complete = true;
            return Ok((Vec::new(), scan.stats));
        }
    }
    if std::env::var("BANGDREAM_OPTIMIZE_POOL_PROFILE").as_deref() == Ok("1") {
        scan.stats.enumeration_profile = Some(EnumerationProfile {
            sample_stride: 1024,
            ..EnumerationProfile::default()
        });
    }
    let start = Instant::now();
    for (attr, groups) in &partitions {
        let suffix = suffix_unions(groups);
        scan.groups(groups, suffix.as_deref(), &mut vec![], 0, false, *attr)?;
        if scan.stats.stop_reason.is_some() {
            break;
        }
    }
    scan.stats.enumeration_seconds = start.elapsed().as_secs_f64();
    scan.stats.adjacent_profiles = scan
        .models
        .iter()
        .filter_map(|m| m.adjacent_stats())
        .collect();
    scan.stats.net_profiles = scan
        .models
        .iter()
        .filter_map(|m| m.net_stats().cloned())
        .collect();
    scan.stats.assignment_profiles = scan.models.iter()
        .map(|m| serde_json::to_value(m.assignment_stats()).unwrap()).collect();
    scan.stats.complete = scan.stats.stop_reason.is_none();
    scan.output.sort_unstable_by_key(|row| row.indices);
    scan.output.dedup_by_key(|row| row.indices);
    scan.stats.retained = scan.output.len();
    // Keep the independent enumeration oracle for debug builds and research
    // runs. A complete production plan already uses the certified pool search.
    if (cfg!(debug_assertions) || !plan_only) && cards.len() <= 25 && scan.stats.complete {
        use bangdream_optimize_core::hit_timing::{
            bounds::CoarseTimingUpperCache,
            enumeration::{enumerate, TimingCardIdentity},
            layouts::physical_layouts,
        };
        let identities: Vec<_> = retained
            .iter()
            .map(|&i| TimingCardIdentity {
                card_id: cards[i].card_id,
                character_id: cards[i].character_id,
            })
            .collect();
        let layouts = physical_layouts();
        let audit_bounds = !refine_pools || retained.iter().any(|&i| {
            eligible[i] && conditional[i].iter().any(|&enabled| enabled)
        });
        let mut coarse = if audit_bounds && !net_bounds {
            inputs.iter().map(|i| CoarseTimingUpperCache::new(i, cap))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        let mut expected = vec![];
        // Refinement rebuilds geometry and rounding envelopes from the surviving
        // catalogue. An upper from the old, wider catalogue need not fit inside
        // the tighter pool upper, even though both enclose every playable score.
        // Keep the independent oracle on the same immutable source domain.
        let audit_skills = audit_catalogue(&skills, &cases, &conditional, refine_pools);
        let net_audit = if audit_bounds && net_bounds {
            inputs
                .iter()
                .map(|i| {
                    if adjacent_net_bounds {
                        bangdream_optimize_core::hit_timing::bounds::TimingNetUpper::new_adjacent_net(i, &audit_skills, cap, window_options())
                    } else if frontier_net_bounds {
                        bangdream_optimize_core::hit_timing::bounds::TimingNetUpper::new_frontier_net(
                            i, &audit_skills, cap, window_options(),
                        )
                    } else if state_net_bounds {
                        bangdream_optimize_core::hit_timing::bounds::TimingNetUpper::new_stateful(
                            i, &audit_skills, cap,
                        )
                    } else {
                        bangdream_optimize_core::hit_timing::bounds::TimingNetUpper::new(
                            i, &audit_skills, cap,
                        )
                    }
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        enumerate(&identities, 100_000, |row| {
            let indices = row.map(|j| retained[j]);
            let band = indices
                .iter()
                .all(|&i| cards[i].band_id == cards[indices[0]].band_id);
            let attr = indices
                .iter()
                .all(|&i| cards[i].attribute == cards[indices[0]].attribute);
            let context = usize::from(band) + 2 * usize::from(attr);
            let eligible_team = indices
                .iter()
                .all(|&i| eligible[i] && conditional[i][context]);
            // Rebuilt certificates cover the surviving domain only. Deleted
            // teams still count toward enumeration auditing, not score bounds.
            scan.stats.small_pool_audited += 1;
            if refine_pools && !eligible_team {
                return Ok(());
            }
            let bound = scan.upper(&indices.map(|i| scan.choices[i][context]));
            let mut sorted = indices;
            sorted.sort_unstable();
            let stored = scan
                .output
                .binary_search_by_key(&sorted, |r| r.indices)
                .ok();
            let members = std::array::from_fn(|j| skills[cases[indices[j]][context]].clone());
            for s in 0..3 {
                let params = indices.map(|i| powers[s][&cards[i].card_id].final_parameters());
                let mut p = ScoringPower::from_member_parameters(&params)?;
                for order in &layouts {
                    p = p.max(ScoringPower::from_member_parameters(&order.map(|i| params[i]))?);
                }
                let coarse_value = if net_bounds {
                    net_audit[s].upper_roster(&members, p)?
                } else {
                    coarse[s].prepare_roster(&members)?.upper(p)?
                };
                assert!(coarse_value <= bound[s],
                    "same-catalogue roster upper {coarse_value} exceeds pool {} for song {s}, cards {:?}",
                    bound[s], indices.map(|i| cards[i].card_id));
                if let Some(i) = stored {
                    assert!(
                        coarse_value <= scan.output[i].upper[s],
                        "stored leaf bound underestimated roster"
                    );
                }
                for (excluded, pair) in scan.pairs.iter().enumerate() {
                    if s != excluded {
                        pair.audit_team(
                            &indices,
                            coarse_value,
                            usize::from(s != (excluded + 1) % 3),
                        );
                    }
                }
                if refine_pools {
                    assert!(coarse_value <= scan.stats.single_upper[s]);
                } else {
                    assert!(bound[s] <= scan.stats.single_upper[s]);
                }
            }
            let prefix_ok = if priced_prefix && eligible_team {
                let groups: Vec<Vec<usize>> = indices
                    .iter()
                    .map(|&i| {
                        retained
                            .iter()
                            .copied()
                            .filter(|&j| {
                                eligible[j]
                                    && conditional[j][context]
                                    && cards[j].character_id == cards[i].character_id
                                    && (!attr || cards[j].attribute == cards[i].attribute)
                            })
                            .collect()
                    })
                    .collect();
                let lists: Vec<_> = groups.iter().map(Vec::as_slice).collect();
                PricedPrefix::new(
                    &scan.models,
                    &scan.choices,
                    &scan.power_sums,
                    &lists,
                    context,
                    &scan.pairs,
                )
                .competitive(&scan.pairs, &indices, seed)
            } else {
                true
            };
            if eligible_team && scan.competitive(bound, &indices) && prefix_ok {
                let mut ids = indices;
                ids.sort_unstable();
                expected.push(ids);
            }
            Ok::<_, Error>(())
        })?;
        expected.sort_unstable();
        assert_eq!(expected.len() as u64, scan.stats.qualifying_rosters);
        if !census_only {
            assert_eq!(
                expected,
                scan.output.iter().map(|r| r.indices).collect::<Vec<_>>()
            );
        }
    }
    scan.stats.replacement_profiles = replacement_models
        .iter()
        .map(|m| m.stats().clone())
        .collect();
    scan.stats.price_key_cards = Pair::key_cards(&scan.pairs, cards.len(), 16)
        .into_iter()
        .map(|i| cards[i].card_id)
        .collect();
    // A partial enumeration cannot certify the missing companion maxima. Do
    // this only after completeness and the small-pool enumeration audit above.
    if early_key_pair_bounds && scan.stats.complete {
        let candidates = scan
            .output
            .iter()
            .map(|r| {
                let mut scores = [0; 3];
                for s in 0..3 {
                    scores[s] = i64::try_from(r.upper[s])?;
                }
                Ok(key_pair_bound::Candidate {
                    cards: r.indices.map(|i| cards[i].card_id as usize),
                    scores,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let keys: Vec<_> = scan
            .stats
            .price_key_cards
            .iter()
            .map(|&id| id as usize)
            .collect();
        let (kept, stats) = key_pair_bound::filter(&candidates, &keys, i64::try_from(seed)?)?;
        let mut positions = kept.into_iter().peekable();
        scan.output = scan
            .output
            .into_iter()
            .enumerate()
            .filter_map(|(i, row)| {
                if positions.peek() == Some(&i) {
                    positions.next();
                    Some(row)
                } else {
                    None
                }
            })
            .collect();
        eprintln!("early key-card pair upper: {} -> {} rosters before layout preparation, {:.3}s prepare + {:.3}s filter",
            stats.before, stats.after, stats.preparation_seconds, stats.filter_seconds);
        scan.stats.retained = scan.output.len();
        scan.stats.early_key_pair_bounds = Some(stats);
    }
    scan.stats.separable_queries = scan.separable_queries.get();
    scan.stats.separable_rejected = scan.separable_rejected.get();
    scan.stats.lazy_song_checks = scan.lazy_song_checks.get();
    scan.stats.lazy_song_refinements = scan.lazy_song_refinements.get();
    scan.stats.pair_bounds = scan.pairs.into_iter().map(|p| p.stats).collect();
    Ok((
        scan.output.into_iter().map(|r| r.indices).collect(),
        scan.stats,
    ))
}
impl Scan<'_> {
    /// Boolean branch gate only. A surviving leaf still requires all three
    /// fully refined uppers, so an uncomputed cheap value never enters a table.
    fn branch_competitive(&self, choices: &[Choice; 5], blocked: &[usize]) -> bool {
        self.lazy_song_checks.set(self.lazy_song_checks.get() + 1);
        let cheap = std::array::from_fn(|s| {
            self.models[s]
                .separable_upper(&std::array::from_fn(|i| choices[i][s]))
                .expect("validated power cap")
        });
        let projected = self.prefix_bound.as_ref().map(|p| p.companions(blocked));
        let companions = std::array::from_fn::<_, 3, _>(|s| {
            let independent =
                self.stats.single_upper[(s + 1) % 3] + self.stats.single_upper[(s + 2) % 3];
            let pair = self
                .pairs
                .get(s)
                .map_or(independent, |p| p.upper(blocked).min(independent));
            match projected {
                Some(p) => p[s].map(|x| x.min(i128::from(pair))),
                None => Some(i128::from(pair)),
            }
        });
        lazy_competitive(cheap, companions, self.stats.seed_score, |s| {
            let values = std::array::from_fn(|i| choices[i][s]);
            if !self.models[s].has_adjacent_roster(&values) {
                return cheap[s];
            }
            let mut counts = self.lazy_song_refinements.get();
            counts[s] += 1;
            self.lazy_song_refinements.set(counts);
            self.models[s].upper(&values).expect("validated power cap")
        })
    }
    /// The cheap bound only rejects nodes. Every surviving node still uses the
    /// coupled value, including all three leaf scores used by pair envelopes.
    fn separable_rejects(&self, choices: &[Choice; 5], blocked: &[usize], max_only: bool) -> bool {
        if !self.stats.separable_gate
            || !(0..3).any(|s| self.models[s].has_adjacent_roster(&std::array::from_fn(|i| choices[i][s])))
        {
            return false;
        }
        self.separable_queries.set(self.separable_queries.get() + 1);
        let bound = std::array::from_fn(|s| {
            self.models[s]
                .separable_upper(&std::array::from_fn(|i| choices[i][s]))
                .expect("validated power cap")
        });
        let rejected = if max_only {
            (0..3).all(|s| bound[s] <= self.stats.single_upper[s])
        } else {
            !self.competitive(bound, blocked)
        };
        self.separable_rejected
            .set(self.separable_rejected.get() + u64::from(rejected));
        rejected
    }
    fn competitive_upper(&self, choices: &[Choice; 5], blocked: &[usize]) -> Option<[u64; 3]> {
        if self.separable_rejects(choices, blocked, false) {
            return None;
        }
        let upper = self.upper(choices);
        self.competitive(upper, blocked).then_some(upper)
    }
    fn upper(&self, choices: &[Choice; 5]) -> [u64; 3] {
        std::array::from_fn(|s| {
            self.models[s]
                .upper(&std::array::from_fn(|i| choices[i][s]))
                .expect("validated power cap")
        })
    }
    fn competitive(&self, u: [u64; 3], blocked: &[usize]) -> bool {
        let projected = self.prefix_bound.as_ref().map(|p| p.companions(blocked));
        (0..3).any(|s| {
            if projected.as_ref().is_some_and(|p| {
                p[s].is_none_or(|other| {
                    other + i128::from(u[s]) < i128::from(self.stats.seed_score)
                })
            }) {
                return false;
            }
            let independent =
                self.stats.single_upper[(s + 1) % 3] + self.stats.single_upper[(s + 2) % 3];
            let pair = self
                .pairs
                .get(s)
                .map_or(independent, |p| p.upper(blocked).min(independent));
            u[s] + pair >= self.stats.seed_score
        })
    }
    fn census_competitive(&self, u: [u64; 3], blocked: &[usize], remaining: &[&[usize]]) -> bool {
        (0..3).any(|s| {
            let independent =
                self.stats.single_upper[(s + 1) % 3] + self.stats.single_upper[(s + 2) % 3];
            let pair = self.pairs.get(s).map_or(independent, |p| {
                p.worst_blocking_upper(blocked, remaining).min(independent)
            });
            u[s] + pair >= self.stats.seed_score
        })
    }
    fn stopped(&mut self) -> bool {
        if self.stats.stop_reason.is_some() {
            return true;
        }
        if self.stats.card_nodes + self.stats.group_nodes + self.stats.upper_group_nodes
            > self.max_nodes
        {
            self.stats.stop_reason = Some(format!(
                "node budget {} reached; coverage incomplete",
                self.max_nodes
            ));
            return true;
        }
        false
    }
    fn groups(
        &mut self,
        groups: &[Group],
        suffix: Option<&[Choice]>,
        chosen: &mut Vec<usize>,
        from: usize,
        max_only: bool,
        attr: Option<Attribute>,
    ) -> Result<(), Error> {
        if max_only {
            self.stats.upper_group_nodes += 1;
        } else {
            self.stats.group_nodes += 1;
        }
        if self.stopped() {
            return Ok(());
        }
        let left = 5 - chosen.len();
        if groups.len() - from < left {
            return Ok(());
        }
        let remaining = remaining_union(groups, from, suffix);
        let choices = std::array::from_fn(|i| {
            if i < chosen.len() {
                groups[chosen[i]].relaxed
            } else {
                remaining
            }
        });
        if self.separable_rejects(&choices, &[], max_only) {
            if !max_only {
                self.stats.pruned_group_nodes += 1;
            }
            return Ok(());
        }
        let bound = self.upper(&choices);
        if max_only {
            if (0..3).all(|s| bound[s] <= self.stats.single_upper[s]) {
                return Ok(());
            }
        } else if !self.competitive(bound, &[]) {
            self.stats.pruned_group_nodes += 1;
            return Ok(());
        }
        if left == 0 {
            let band = self.cards[groups[chosen[0]].cards[0]].band_id;
            let common_band = chosen
                .iter()
                .all(|&g| self.cards[groups[g].cards[0]].band_id == band);
            let alternatives =
                std::array::from_fn(|i| groups[chosen[i]].cases[usize::from(common_band)]);
            if self.separable_rejects(&alternatives, &[], max_only) {
                if !max_only {
                    self.stats.pruned_group_nodes += 1;
                }
                return Ok(());
            }
            let bound = self.upper(&alternatives);
            if max_only {
                for s in 0..3 {
                    self.stats.single_upper[s] = self.stats.single_upper[s].max(bound[s]);
                }
                return Ok(());
            }
            if !self.competitive(bound, &[]) {
                self.stats.pruned_group_nodes += 1;
                return Ok(());
            }
            let lists: Vec<_> = chosen
                .iter()
                .map(|&i| groups[i].case_cards[usize::from(common_band)].as_slice())
                .collect();
            if lists.iter().any(|g| g.is_empty()) {
                return Ok(());
            }
            let context = usize::from(common_band) + if attr.is_some() { 2 } else { 0 };
            let prepare_start = self
                .stats
                .enumeration_profile
                .as_ref()
                .map(|_| Instant::now());
            let priced = self.stats.priced_prefix.then(|| {
                PricedPrefix::new(
                    &self.models,
                    &self.choices,
                    &self.power_sums,
                    &lists,
                    context,
                    &self.pairs,
                )
            });
            if let (Some(p), Some(start)) = (&mut self.stats.enumeration_profile, prepare_start) {
                p.priced_preparation_seconds += start.elapsed().as_secs_f64();
                p.priced_preparations += 1;
            }
            if priced
                .as_ref()
                .is_some_and(|p| !p.competitive(&self.pairs, &[], self.stats.seed_score))
            {
                self.stats.pruned_group_nodes += 1;
                self.stats.price_prefix_pruned += 1;
                return Ok(());
            }
            let minima = std::array::from_fn(|g| {
                lists[g]
                    .iter()
                    .map(|&i| self.choices[i][context])
                    .reduce(minimum)
                    .unwrap()
            });
            let mut values = alternatives;
            self.members(
                &lists,
                &mut values,
                &minima,
                &mut vec![],
                context,
                attr,
                priced.as_ref(),
            )?;
        } else {
            for i in from..=groups.len() - left {
                chosen.push(i);
                self.groups(groups, suffix, chosen, i + 1, max_only, attr)?;
                chosen.pop();
                if self.stopped() {
                    break;
                }
            }
        }
        Ok(())
    }
    fn members(
        &mut self,
        lists: &[&[usize]],
        values: &mut [Choice; 5],
        minima: &[Choice; 5],
        chosen: &mut Vec<usize>,
        context: usize,
        attr: Option<Attribute>,
        priced: Option<&PricedPrefix>,
    ) -> Result<(), Error> {
        self.stats.card_nodes += 1;
        if self.stopped() {
            return Ok(());
        }
        let depth = chosen.len();
        let sampled = if let Some(p) = &mut self.stats.enumeration_profile {
            p.by_depth[depth].nodes += 1;
            // Mix high bits so sampling does not lock onto one subtree depth.
            self.stats.card_nodes.wrapping_mul(0x9e3779b97f4a7c15) >> 54 == 0
        } else {
            false
        };
        let start = sampled.then(Instant::now);
        let rejected =
            priced.is_some_and(|p| !p.competitive(&self.pairs, chosen, self.stats.seed_score));
        if let Some(p) = &mut self.stats.enumeration_profile {
            p.by_depth[depth].priced.record(start);
            p.by_depth[depth].priced_rejected += u64::from(rejected);
        }
        if rejected {
            self.stats.pruned_card_nodes += 1;
            self.stats.price_prefix_pruned += 1;
            return Ok(());
        }
        let start = sampled.then(Instant::now);
        let upper = if self.stats.lazy_song_bounds && depth < 5 && !self.stats.census_only {
            let rejected = !self.branch_competitive(values, chosen);
            if let Some(p) = &mut self.stats.enumeration_profile {
                p.by_depth[depth].upper.record(start);
                p.by_depth[depth].upper_rejected += u64::from(rejected);
            }
            if rejected {
                self.stats.pruned_card_nodes += 1;
                return Ok(());
            }
            None
        } else {
            let queries_before = self.separable_queries.get();
            let rejected = self.separable_rejects(values, chosen, false);
            if let Some(p) = &mut self.stats.enumeration_profile {
                if self.separable_queries.get() != queries_before {
                    p.by_depth[depth].separable.record(start);
                }
                p.by_depth[depth].upper_rejected += u64::from(rejected);
            }
            if rejected {
                self.stats.pruned_card_nodes += 1;
                return Ok(());
            }
            let start = sampled.then(Instant::now);
            let upper = self.upper(values);
            if let Some(p) = &mut self.stats.enumeration_profile {
                p.by_depth[depth].upper.record(start);
            }
            let start = sampled.then(Instant::now);
            let rejected = !self.competitive(upper, chosen);
            if let Some(p) = &mut self.stats.enumeration_profile {
                p.by_depth[depth].companions.record(start);
                p.by_depth[depth].upper_rejected += u64::from(rejected);
            }
            if rejected {
                self.stats.pruned_card_nodes += 1;
                return Ok(());
            }
            Some(upper)
        };
        // The old subtree-count shortcut only models future price deductions.
        // A future exclusion also lowers companion maxima: enumerate leaves
        // instead of incorrectly counting those newly rejected combinations.
        if self.stats.census_only && !self.stats.blocking_bounds && chosen.len() < 5 {
            let least = std::array::from_fn(|i| {
                if i < chosen.len() {
                    values[i]
                } else {
                    minima[i]
                }
            });
            if self.census_competitive(self.upper(&least), chosen, &lists[chosen.len()..])
                && priced.is_none_or(|p| {
                    p.census_competitive(&self.pairs, chosen, self.stats.seed_score)
                })
            {
                let left = &lists[chosen.len()..];
                let mut count = left.iter().map(|l| l.len() as u64).product::<u64>();
                if attr.is_none() {
                    for a in [
                        Attribute::Cool,
                        Attribute::Happy,
                        Attribute::Pure,
                        Attribute::Powerful,
                    ] {
                        if chosen.iter().all(|&i| self.cards[i].attribute == a) {
                            count -= left
                                .iter()
                                .map(|l| {
                                    l.iter().filter(|&&i| self.cards[i].attribute == a).count()
                                        as u64
                                })
                                .product::<u64>();
                        }
                    }
                }
                self.stats.qualifying_rosters += count;
                self.stats.counted_subtrees += 1;
                return Ok(());
            }
        }
        if chosen.len() == 5 {
            if attr.is_none()
                && chosen
                    .iter()
                    .all(|&i| self.cards[i].attribute == self.cards[chosen[0]].attribute)
            {
                return Ok(());
            }
            if self.stats.census_only {
                self.stats.qualifying_rosters += 1;
                self.stats.counted_subtrees += 1;
                return Ok(());
            }
            if self.output.len() >= self.max_teams {
                self.stats.stop_reason = Some(format!(
                    "retained-roster budget {} reached; coverage incomplete",
                    self.max_teams
                ));
                return Ok(());
            }
            let mut ids: [usize; 5] = chosen.clone().try_into().unwrap();
            ids.sort_unstable();
            self.output.push(Leaf {
                indices: ids,
                upper: upper.expect("leaf scores are always fully refined"),
            });
            self.stats.qualifying_rosters += 1;
        } else {
            let old = values[depth];
            for &i in lists[chosen.len()] {
                chosen.push(i);
                values[depth] = self.choices[i][context];
                self.members(lists, values, minima, chosen, context, attr, priced)?;
                chosen.pop();
                if self.stopped() {
                    break;
                }
            }
            values[depth] = old;
        }
        Ok(())
    }
}

fn audit_catalogue(
    skills: &[ReplaySkill],
    cases: &[[usize; 4]],
    allowed: &[[bool; 4]],
    refined: bool,
) -> Vec<ReplaySkill> {
    let ids: std::collections::BTreeSet<_> = cases.iter().zip(allowed)
        .flat_map(|(row, allow)| (0..4).filter(|&c| allow[c]).map(|c| row[c]))
        .collect();
    if !refined || ids.is_empty() {
        // Empty refinement retains the original model; no eligible team is
        // queried by the audit in that case.
        skills.to_vec()
    } else {
        ids.into_iter().map(|i| skills[i].clone()).collect()
    }
}

fn lazy_competitive(
    cheap: [u64; 3],
    companions: [Option<i128>; 3],
    seed: u64,
    mut refine: impl FnMut(usize) -> u64,
) -> bool {
    (0..3).any(|s| {
        companions[s].is_some_and(|other| {
            i128::from(cheap[s]) + other >= i128::from(seed)
                && i128::from(refine(s)) + other >= i128::from(seed)
        })
    })
}

#[cfg(test)]
#[path = "timing_pool_lazy_tests.rs"]
mod lazy_tests;

use bangdream_optimize_core::timing::Instant;
