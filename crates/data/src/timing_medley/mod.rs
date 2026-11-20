//! Complete enumeration within a declared real-card pool; all captains/layouts.
//! Shared browser, desktop and research engine for the declared timing model.
use bangdream_optimize_core::{
    hit_timing::{
        bounds::{
            CoarseTimingUpperCache, TimingNetUpper, TimingScoreUpper, TimingUpperCache,
            TimingUpperScratch,
        },
        compiled::{CompiledTimingCandidates, TimingCompileCache, TimingCompileCachePool, TimingScoreScratch},
        enumeration::{enumerate, TimingCardIdentity},
        layouts::{physical_layouts, LayoutMaximum, TimingLayouts},
        lazy_medley::{self, MedleyRefinement},
        search::{SearchError, SearchOptions},
        ReplayInput, ReplaySkill,
    },
    prepare_cards,
    scoring::{
        preparation::{selected_equipment, PreparedPowerCard},
        MemberPower, ScoringPower,
    },
    AreaItemDefinition, PlayerConfig, PreparedCard, SelectedAreaItems, Server,
};
use bangdream_optimize_data::{bestdori::timing_skill::TimingTeamCard, event_bonus, BestdoriData};
use bangdream_optimize_medley_solver::WideMedleySolverInput;
use serde::Deserialize;
use serde_json::{json, Value};
use bangdream_optimize_core::timing::Instant;
use std::{
    collections::BTreeMap,
};
#[path = "timing_cover.rs"]
mod timing_cover;
#[path = "timing_eligibility.rs"]
mod timing_eligibility;
use bangdream_optimize_core::medley::key_pair_bound as timing_key_pair;
#[path = "timing_masks.rs"]
mod timing_masks;
#[path = "timing_memo.rs"]
mod timing_memo;
#[path = "timing_pair.rs"]
mod timing_pair;
#[path = "timing_pool.rs"]
mod timing_pool;
#[path = "timing_product.rs"]
mod timing_product;
#[path = "timing_refine.rs"]
mod timing_refine;
#[path = "timing_replacement.rs"]
mod timing_replacement;

#[path = "timing_items.rs"]
mod timing_items;
#[path = "timing_model_cache.rs"]
mod timing_model_cache;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    #[serde(skip)]
    frame_ranges: Option<[Vec<Option<[u32; 2]>>; 3]>,
    /// Explicit research-only simplification: all scoring windows are based on
    /// chart time; position/speed-dependent green preparation is not used.
    #[serde(default)]
    timing_model: Option<String>,
    /// Source/audit marker for chart-time visible green midpoint candidate ends.
    #[serde(default)]
    candidate_lifetime_model: Option<String>,
    #[serde(default)]
    all_items: bool,
    #[serde(default)]
    item_scan_only: bool,
    #[serde(rename = "sourcePath")]
    _source_path: String,
    #[serde(rename = "resultPath")]
    _result_path: String,
    #[serde(skip)]
    equivalent_score_reuse: Option<bool>,
    inputs: [ReplayInput; 3],
    pool: Vec<u32>,
    max_teams: usize,
    #[serde(default)]
    upper_only: bool,
    #[serde(default)]
    share_equivalent_skills: bool,
    #[serde(default)]
    batch_powers: bool,
    /// When present, audit all requests of the selected song's winning roster.
    #[serde(default)]
    audit_winner_song: Option<usize>,
    #[serde(default)]
    pool_scan: bool,
    #[serde(default)]
    seed_path: Option<String>,
    #[serde(default = "default_nodes")]
    max_nodes: u64,
    #[serde(default)]
    census_only: bool,
    #[serde(default)]
    prepared_cache: bool,
    #[serde(default)]
    pair_bounds: bool,
    #[serde(default)]
    priced_prefix: bool,
    #[serde(default)]
    price_card_filter: bool,
    #[serde(default)]
    joint_power_bins: usize,
    #[serde(default)]
    adaptive_prices: bool,
    /// Opt-in replacement experiment. Without adjacent_net_bounds the proof
    /// assumes independent windows; adjacent mode restricts replacements to
    /// those whose affected suffix is covered by the queued model.
    #[serde(default)]
    contribution_pruning: bool,
    #[serde(default)]
    signature_pruning: bool,
    #[serde(default)]
    cross_duration_pruning: bool,
    #[serde(default)]
    cross_character_pruning: bool,
    #[serde(default)]
    affine_pruning: bool,
    #[serde(default)]
    frame_pruning: bool,
    #[serde(default)]
    net_bounds: bool,
    #[serde(default)]
    state_net_bounds: Option<bool>,
    /// Full-frontier additive contributions with a shared per-note baseline.
    #[serde(default, alias = "allocatedNetBounds")]
    frontier_net_bounds: bool,
    /// Couple starts and corrected trigger legality through the native FIFO clock.
    #[serde(default)]
    adjacent_net_bounds: bool,
    #[serde(default)]
    compact_masks: bool,
    #[serde(default)]
    first_feasible: bool,
    #[serde(default)]
    stream_competitive: bool,
    #[serde(default)]
    companion_bounds: bool,
    #[serde(default)]
    blocking_bounds: bool,
    #[serde(default)]
    blocking_key_cards: usize,
    #[serde(default)]
    key_pair_bounds: bool,
    #[serde(default)]
    early_key_pair_bounds: bool,
    #[serde(default)]
    prefix_key_pair_bounds: bool,
    #[serde(default)]
    refine_pools: bool,
    #[serde(default)]
    refine_cutoffs: bool,
    #[serde(default)]
    refine_replacements: bool,
}
fn default_nodes() -> u64 {
    10_000_000
}

// New net-table runs keep one chronological skill state by default. Explicit
// false preserves old A/B requests; runs without net tables remain unchanged.
fn net_bound_modes(net: bool, state: Option<bool>) -> (bool, bool) {
    let state = state.unwrap_or(net);
    (net || state, state)
}

struct Roster {
    cards: [u32; 5],
    skills: [ReplaySkill; 5],
    max_power: [ScoringPower; 3],
}
struct Fine {
    layouts: TimingLayouts,
    upper: Vec<u64>,
}
struct Exact {
    maximum: LayoutMaximum,
    power: ScoringPower,
}

fn int_score(value: u64) -> Result<i32, SearchError> {
    i32::try_from(value).map_err(|_| SearchError::Options("score overflow"))
}

struct Problem {
    saved: Value,
    seed: Option<Value>,
    player: PlayerConfig,
    server: Server,
    data: BestdoriData,
    prepared: Vec<PreparedCard>,
    native: Vec<PreparedPowerCard>,
    characters: BTreeMap<u32, u32>,
    defs: BTreeMap<u32, AreaItemDefinition>,
    item_order: Vec<u32>,
}
impl Problem {
    fn from_source(r: &Request, source: Value, saved: Value, seed: Option<Value>)
        -> Result<Self, Box<dyn std::error::Error>> {
        let player: PlayerConfig = serde_json::from_value(source["player"].clone())?;
        let server: Server = serde_json::from_value(source["server"].clone())?;
        let mut data = BestdoriData::from_values(
            source["cards"].clone(),
            source["characters"].clone(),
            source["skills"].clone(),
            source["areaItems"].clone(),
        )?;
        let optional = |k: &str| source.get(k).filter(|v| !v.is_null()).cloned();
        data.apply_repairs(
            optional("cardsFix"),
            optional("skillsFix"),
            optional("areaItemsFix"),
        )?;
        let defs = r
            .pool
            .iter()
            .map(|&id| data.card_definition(id))
            .collect::<Result<Vec<_>, _>>()?;
        let event = event_bonus(&source["event"])?;
        let prepared = prepare_cards(&defs, &player.card_list, &player.character_bouns, &event)?;
        if prepared.len() != r.pool.len() {
            return Err("pool must contain distinct owned cards".into());
        }
        let native = prepared
            .iter()
            .map(|c| PreparedPowerCard::new(c, &event))
            .collect::<Result<Vec<_>, _>>()?;
        let characters: BTreeMap<_, _> = prepared
            .iter()
            .map(|c| (c.card_id, c.character_id))
            .collect();
        let defs = data.area_item_definitions(server)?;
        let mut item_order = player
            .area_item
            .keys()
            .map(|k| k.parse::<u32>())
            .collect::<Result<Vec<_>, _>>()?;
        item_order.sort_unstable();
        Ok(Self {
            saved,
            seed,
            player,
            server,
            data,
            prepared,
            native,
            characters,
            defs,
            item_order,
        })
    }
    fn powers(
        &self,
        selected: &SelectedAreaItems,
    ) -> Result<BTreeMap<u32, MemberPower>, Box<dyn std::error::Error>> {
        let e = selected_equipment(
            &self.player.area_item,
            &self.defs,
            selected,
            &self.item_order,
        )?;
        Ok(self
            .native
            .iter()
            .map(|c| c.power(&e).map(|p| (c.card_id, p)))
            .collect::<Result<_, _>>()?)
    }
    fn fixed_items(&self) -> Result<[SelectedAreaItems; 3], Box<dyn std::error::Error>> {
        Ok((0..3)
            .map(|s| {
                serde_json::from_value(
                    self.saved["songs"][s]
                        .get("items")
                        .unwrap_or(&self.saved["items"])
                        .clone(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .unwrap())
    }
}
struct Workspace {
    models: timing_model_cache::Models,
    fine_caches: Vec<TimingUpperCache>,
    fine_memos: Vec<timing_memo::Memo<TimingScoreUpper>>,
    bank_memos: Vec<timing_memo::Memo<CompiledTimingCandidates>>,
    caches: TimingCompileCachePool,
}
impl Workspace {
    fn new(r: &Request) -> Self {
        Self {
            models: timing_model_cache::Models::new(
                &r.inputs,
                timing_pool::window_options(),
                if std::env::var("BANDORI_TIMING_SHARED_ITEM_MODELS").as_deref() == Ok("0") {
                    0
                } else {
                    8
                },
            ).with_same_hit_frames(r.frame_ranges.is_some()),
            fine_caches: r.inputs.iter().enumerate().map(|(song,input)| {
                let cache=TimingUpperCache::new(input);
                match &r.frame_ranges {
                    Some(limits)=>cache.with_frame_ranges(limits[song].clone()),
                    None=>cache,
                }
            }).collect(),
            fine_memos: (0..3)
                .map(|_| timing_memo::Memo::new(if r.prepared_cache { 1024 } else { 0 }))
                .collect(),
            bank_memos: (0..3)
                // The core cache already owns exact models and enforces its
                // geometry byte budget. An outer strong cache defeats eviction.
                .map(|_| timing_memo::Memo::new(if r.prepared_cache
                    && std::env::var("BANDORI_TIMING_DISABLE_JOINT_LOCAL_GEOMETRY")
                        .as_deref() == Ok("1") { 128 } else { 0 }))
                .collect(),
            caches: match &r.frame_ranges {
                Some(limits) => TimingCompileCachePool::new(&r.inputs, SearchOptions::default()).with_frame_ranges(limits),
                None => TimingCompileCachePool::new(&r.inputs, SearchOptions::default()),
            },
        }
    }
}
fn validate_seed(
    r: &Request,
    problem: &Problem,
    member_powers: &[BTreeMap<u32, MemberPower>],
    changed_items: bool,
) -> Result<(u64, Vec<[u32; 5]>), Box<dyn std::error::Error>> {
    let Problem {
        player,
        server,
        data,
        characters,
        ..
    } = problem;
    let server = *server;
    let mut seed_score = 0u64;
    let mut seed_rosters = Vec::new();
    if let Some(artifact) = &problem.seed {
        let winners = artifact["output"]["winners"]
            .as_array()
            .ok_or("seed needs three winners")?;
        if winners.len() != 3 {
            return Err("seed needs exactly three winners".into());
        }
        let mut used = std::collections::BTreeSet::new();
        for (s, w) in winners.iter().enumerate() {
            let team: [u32; 5] = serde_json::from_value(w["team"].clone())?;
            if team
                .iter()
                .any(|id| !r.pool.contains(id) || !used.insert(*id))
            {
                return Err("seed cards must be owned, in pool and disjoint".into());
            }
            if team
                .iter()
                .map(|id| characters[id])
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != 5
            {
                return Err("seed repeats a character".into());
            }
            let skills = data.hit_timing_team_skills(
                server,
                &team.map(|card_id| TimingTeamCard {
                    card_id,
                    skill_level: player.card_list[&card_id.to_string()].skill_level,
                }),
            )?;
            let order: [u32; 6] = serde_json::from_value(w["skillOrder"].clone())?;
            let slots = order.map(|id| team.iter().position(|&c| c == id).unwrap_or(usize::MAX));
            if !bangdream_optimize_core::hit_timing::lazy_orders::reachable_orders()
                .iter()
                .any(|o| o.slots == slots)
            {
                return Err("seed has an impossible native order".into());
            }
            let mut inp = r.inputs[s].clone();
            inp.power = ScoringPower::from_member_parameters(
                &team.map(|id| member_powers[s][&id].final_parameters()),
            )?
            .value();
            inp.skills = slots.map(|i| skills[i].skill.clone()).to_vec();
            let mut expected = w["input"].clone();
            if changed_items {
                expected["power"] = json!(inp.power);
            }
            if serde_json::to_value(&inp)? != expected {
                return Err(
                    "seed chart, profile power or resolved skill does not match this request"
                        .into(),
                );
            }
            inp.hit_frames = serde_json::from_value(w["result"]["hitFrames"].clone())?;
            inp.hit_order = serde_json::from_value(
                w["result"]
                    .get("hitOrder")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            )?;
            let result = bangdream_optimize_core::hit_timing::replay(&inp)?;
            if r.frame_ranges.as_ref().is_some_and(|all| all[s].iter().zip(&result.hit_frames)
                .any(|(limit, &f)| limit.is_some_and(|[a,b]| f<a || f>b))) {
                return Err("seed violates user frame limits".into());
            }
            if !changed_items
                && result.total != w["result"]["total"].as_u64().ok_or("seed score missing")?
            {
                return Err("seed score replay mismatch".into());
            }
            seed_score += result.total;
            seed_rosters.push(team);
        }
    }
    Ok((seed_score, seed_rosters))
}
fn trace(row: serde_json::Value) {
    if std::env::var("BANDORI_TIMING_DISABLE_BUFFERED_TRACE").as_deref() == Ok("1") {
        eprintln!("exact-trace {}",row);
    } else {
        eprintln!("exact-trace {}",row.to_string());
    }
}

fn solve(mut r: Request, problem: &Problem) -> Result<Value, Box<dyn std::error::Error>> {
    if r.candidate_lifetime_model.as_deref().is_some_and(|model| {
        !matches!(model, "greenHeadMidpoint" | "greenVisibleMidpoint")
    })
        || (r.candidate_lifetime_model.is_some() && r.timing_model.is_none())
    {
        return Err("unsupported chart-time candidate lifetime model".into());
    }
    if let Some(model) = &r.timing_model {
        if !matches!(model.as_str(), "timeDistance" | "timeDistanceControl" | "timeDistanceZeroGapControl" | "timeDistanceZeroGap" | "typedTimeDistance")
            || r.inputs.iter().any(|input| {
                !input.native_time_routing.as_ref().is_some_and(|route| {
                    route.distance_mode
                        == bangdream_optimize_core::hit_timing::CandidateDistanceMode::ChartTime
                })
                    || input.notes.iter().any(|n| {
                        use bangdream_optimize_core::hit_timing::NoteWindow;
                        !matches!(n.window, NoteWindow::Standard)
                            && !(model == "typedTimeDistance" && matches!(n.window,
                                NoteWindow::WidenedEndpoint | NoteWindow::SlideTime { .. }))
                    })
                    || input.constraints.iter().any(|c| {
                        c.minimum_gap_frames != if matches!(model.as_str(), "timeDistanceZeroGapControl" | "timeDistanceZeroGap" | "typedTimeDistance") { 0 } else { 1 }
                            || !c.require_dispatch_order
                            || c.earliest_after.is_some()
                    })
            })
        {
            return Err(
                "timing model requires matching declared windows and visible-chain edges".into(),
            );
        }
    }
    let (net_bounds, state_net_bounds) = net_bound_modes(
        r.net_bounds || r.frontier_net_bounds || r.adjacent_net_bounds,
        r.state_net_bounds,
    );
    r.net_bounds = net_bounds;
    r.state_net_bounds = Some(state_net_bounds || r.frontier_net_bounds || r.adjacent_net_bounds);
    r.key_pair_bounds |= r.early_key_pair_bounds || r.prefix_key_pair_bounds;
    r.pair_bounds |= r.key_pair_bounds;
    if r.early_key_pair_bounds && (r.census_only || r.seed_path.is_none()) {
        return Err("early key-pair filtering needs complete rosters and a validated seed".into());
    }
    r.refine_cutoffs |= r.refine_replacements;
    r.affine_pruning |= r.frame_pruning;
    r.signature_pruning |= r.affine_pruning;
    // Actual candidate scoring jointly optimizes queued windows. An independent
    // replacement certificate cannot justify discarding one of those candidates.
    if (r.contribution_pruning
        || r.signature_pruning
        || r.cross_duration_pruning
        || r.cross_character_pruning)
        && !r.adjacent_net_bounds
    {
        return Err("replacement pruning requires adjacentNetBounds: independent-window replacement is not a certificate for joint queued-window scoring".into());
    }
    if r.audit_winner_song.is_some_and(|s| s >= 3) {
        return Err("audit song must be 0..2".into());
    }
    let mut workspace = Workspace::new(&r);
    let output = if r.all_items {
        timing_items::solve_all(&r, &problem, &mut workspace)?
    } else {
        if r.item_scan_only {
            return Err("itemScanOnly requires allItems".into());
        }
        let fixed = problem.fixed_items()?;
        let powers = fixed
            .iter()
            .map(|i| problem.powers(i))
            .collect::<Result<Vec<_>, _>>()?;
        solve_one(&r, &problem, &powers, false, 0, &mut workspace)?
    };
    Ok(output)
}
fn solve_one(
    r: &Request,
    problem: &Problem,
    member_powers: &[BTreeMap<u32, MemberPower>],
    changed_items: bool,
    external_cutoff: u64,
    workspace: &mut Workspace,
) -> Result<Value, Box<dyn std::error::Error>> {
    let state_net_bounds = r.state_net_bounds.unwrap_or(r.net_bounds)
        || r.frontier_net_bounds
        || r.adjacent_net_bounds;
    let Problem {
        player,
        server,
        data,
        prepared,
        characters,
        ..
    } = problem;
    let server = *server;
    let (local_seed_score, seed_rosters) = validate_seed(r, problem, member_powers, changed_items)?;
    let seed_score = local_seed_score.max(external_cutoff);
    let layouts = physical_layouts();
    let mut pool_scan = None;
    let mut enumerated = Vec::new();
    let use_pool_scan = r.pool_scan
        || r.contribution_pruning
        || r.signature_pruning
        || r.cross_duration_pruning
        || r.cross_character_pruning
        || r.refine_pools
        || r.refine_cutoffs
        || r.pair_bounds
        || r.priced_prefix
        || r.price_card_filter
        || r.joint_power_bins > 0
        || r.adaptive_prices
        || r.census_only
        || r.blocking_bounds;
    if use_pool_scan {
        let (indices, stats) = timing_pool::enumerate_pool(
            &prepared,
            &member_powers,
            &data,
            server,
            &r.inputs,
            seed_score,
            local_seed_score,
            r.max_teams,
            r.max_nodes,
            r.census_only,
            r.pair_bounds || r.blocking_bounds,
            r.priced_prefix,
            r.price_card_filter,
            r.joint_power_bins,
            r.adaptive_prices,
            r.contribution_pruning
                || r.signature_pruning
                || r.cross_duration_pruning
                || r.cross_character_pruning
                || r.refine_pools
                || r.refine_cutoffs,
            r.signature_pruning
                || r.cross_duration_pruning
                || r.cross_character_pruning
                || r.refine_pools
                || r.refine_cutoffs,
            r.cross_duration_pruning,
            r.cross_character_pruning,
            r.affine_pruning,
            r.frame_pruning,
            r.net_bounds,
            state_net_bounds,
            r.frontier_net_bounds,
            r.adjacent_net_bounds,
            r.refine_pools || r.refine_cutoffs,
            r.refine_cutoffs,
            r.refine_replacements,
            r.blocking_bounds,
            r.blocking_key_cards,
            r.early_key_pair_bounds,
            r.prefix_key_pair_bounds,
            &mut workspace.models,
        )?;
        let by_id: BTreeMap<_, _> = r.pool.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        enumerated = indices
            .into_iter()
            .map(|t| {
                let mut row = t.map(|i| by_id[&prepared[i].card_id]);
                row.sort_unstable();
                row
            })
            .collect();
        if !stats.complete || r.census_only {
            return Ok(
                json!({"pool":r.pool,"incomplete":!stats.complete,"censusOnly":r.census_only,"poolScan":stats,"rosters":enumerated.iter().map(|t|t.map(|i|r.pool[i])).collect::<Vec<_>>()}),
            );
        }
        pool_scan = Some(stats);
    }
    let identities: Vec<_> = r
        .pool
        .iter()
        .map(|&card_id| TimingCardIdentity {
            card_id,
            character_id: characters[&card_id],
        })
        .collect();
    let mut rosters = Vec::new();
    let mut layout_power_variants = [0usize; 3];
    let start = Instant::now();
    let enumeration = if use_pool_scan {
        None
    } else {
        Some(enumerate(&identities, r.max_teams, |indices| {
            enumerated.push(indices);
            Ok::<_, Box<dyn std::error::Error>>(())
        })?)
    };
    let (mut masks, mask_stats) = timing_masks::build(&enumerated, r.pool.len(), r.compact_masks);
    let roster_count = enumerated.len();
    eprintln!(
        "preparing {roster_count} rosters; masks {} used cards / {} words",
        mask_stats.used_cards, mask_stats.words
    );
    for indices in enumerated {
        let cards = indices.map(|i| r.pool[i]);
        let members = cards.map(|card_id| TimingTeamCard {
            card_id,
            skill_level: player.card_list[&card_id.to_string()].skill_level,
        });
        let skills = data.hit_timing_team_skills(server, &members)?;
        let mut powers = Vec::new();
        for song in 0..3 {
            let members = cards.map(|id| member_powers[song][&id]);
            let mut bits = std::collections::BTreeSet::new();
            for order in layouts.iter() {
                let parameters = order.map(|i| members[i].final_parameters());
                bits.insert(ScoringPower::from_member_parameters(&parameters)?);
            }
            layout_power_variants[song] += bits.len();
            powers.push(*bits.last().unwrap());
        }
        rosters.push(Roster {
            cards,
            skills: skills.map(|s| s.skill),
            max_power: powers.try_into().unwrap(),
        });
        if rosters.len() % 100000 == 0 {
            eprintln!(
                "roster preparation {}/{roster_count}: {:.3}s",
                rosters.len(),
                start.elapsed().as_secs_f64()
            );
        }
    }
    if rosters.is_empty() {
        if external_cutoff > 0 {
            return Ok(
                json!({"noCompetitivePlan":true,"cutoffScore":external_cutoff,"poolScan":pool_scan}),
            );
        }
        return Err("no legal rosters in pool".into());
    }
    let enumeration_seconds = start.elapsed().as_secs_f64();
    eprintln!(
        "{} complete rosters: {:.3}s",
        rosters.len(),
        enumeration_seconds
    );
    let max_power = rosters.iter().flat_map(|r| r.max_power).max().unwrap();
    let mut coarse = r
        .inputs
        .iter()
        .map(|i| CoarseTimingUpperCache::new(i, max_power))
        .collect::<Result<Vec<_>, _>>()?;
    let start = Instant::now();
    let mut uppers = Vec::new();
    let mut catalogue = BTreeMap::new();
    if r.net_bounds {
        for roster in &rosters {
            for skill in &roster.skills {
                catalogue.insert(skill.cache_key(), skill.clone());
            }
        }
    }
    let catalogue: Vec<_> = catalogue.into_values().collect();
    let net = if r.net_bounds {
        r.inputs
            .iter()
            .map(|i| {
                if r.adjacent_net_bounds {
                    TimingNetUpper::new_adjacent_net(
                        i,
                        &catalogue,
                        max_power,
                        timing_pool::window_options(),
                    )
                } else if r.frontier_net_bounds {
                    TimingNetUpper::new_frontier_net(
                        i,
                        &catalogue,
                        max_power,
                        timing_pool::window_options(),
                    )
                } else if state_net_bounds {
                    TimingNetUpper::new_stateful(i, &catalogue, max_power)
                } else {
                    TimingNetUpper::new(i, &catalogue, max_power)
                }
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    for (i, roster) in rosters.iter().enumerate() {
        let mut row = [0; 3];
        for s in 0..3 {
            row[s] = int_score(
                coarse[s]
                    .prepare_roster(&roster.skills)?
                    .upper(roster.max_power[s])?,
            )?;
            if r.net_bounds {
                row[s] = row[s].min(int_score(
                    net[s].upper_roster(&roster.skills, roster.max_power[s])?,
                )?);
            }
        }
        uppers.push(row);
        if (i + 1) % 100000 == 0 {
            eprintln!(
                "coarse bounds {}/{roster_count}: {:.3}s",
                i + 1,
                start.elapsed().as_secs_f64()
            );
        }
    }
    let coarse_seconds = start.elapsed().as_secs_f64();
    let coarse_stats: Vec<_> = coarse.iter().map(|c| c.stats().clone()).collect();
    drop(coarse);
    let key_pair_bounds = if r.key_pair_bounds {
        if seed_rosters.len() != 3 || seed_score == 0 {
            return Err("key-pair bounds require a validated three-team seed".into());
        }
        let keys = &pool_scan
            .as_ref()
            .ok_or("key-pair bounds need complete pool enumeration")?
            .price_key_cards;
        let candidates: Vec<_> = rosters
            .iter()
            .zip(&uppers)
            .map(|(r, s)| timing_key_pair::Candidate {
                cards: r.cards.map(|c| c as usize),
                scores: s.map(i64::from),
            })
            .collect();
        let keys: Vec<_> = keys.iter().map(|&c| c as usize).collect();
        let (kept, stats) =
            timing_key_pair::filter(&candidates, &keys, i64::try_from(seed_score)?)?;
        eprintln!("key-card pair upper: {} -> {} rosters, {} keys / {} masks, {:.3}s prepare + {:.3}s filter",
            stats.before, stats.after, stats.keys.len(), stats.occupied_masks,
            stats.preparation_seconds, stats.filter_seconds);
        let mut active = vec![false; rosters.len()];
        for &i in &kept {
            active[i] = true;
        }
        rosters = rosters
            .into_iter()
            .enumerate()
            .filter_map(|(i, r)| active[i].then_some(r))
            .collect();
        uppers = uppers
            .into_iter()
            .enumerate()
            .filter_map(|(i, r)| active[i].then_some(r))
            .collect();
        masks = masks
            .into_iter()
            .enumerate()
            .filter_map(|(i, r)| active[i].then_some(r))
            .collect();
        Some(stats)
    } else {
        None
    };
    let mut output = json!({"pool":r.pool, "enumeration":enumeration, "enumerationSeconds":enumeration_seconds, "coarseSeconds":coarse_seconds, "coarseStats":coarse_stats, "layoutPowerVariants":layout_power_variants, "rosters":rosters.iter().map(|t|t.cards).collect::<Vec<_>>(), "upperScores":uppers});
    output["keyPairBounds"] = json!(key_pair_bounds);
    output["poolScan"] = json!(pool_scan);
    output["maskStats"] = json!(mask_stats);
    output["cutoffScore"] = json!(seed_score);
    output["localSeedScore"] = json!(local_seed_score);
    output["adjacentProfiles"] = json!(net
        .iter()
        .filter_map(|n| n.adjacent_stats())
        .collect::<Vec<_>>());
    output["netProfiles"] = json!(net.iter().map(|n| n.stats()).collect::<Vec<_>>());
    if r.upper_only {
        return Ok(output);
    }
    if rosters.is_empty() && external_cutoff > 0 {
        output["noCompetitivePlan"] = json!(true);
        return Ok(output);
    }
    let Workspace {
        fine_caches,
        fine_memos,
        bank_memos,
        caches,
        ..
    } = workspace;
    let mut fine: BTreeMap<(usize, usize), Fine> = BTreeMap::new();
    let mut maxima: BTreeMap<(usize, usize), Exact> = BTreeMap::new();
    let mut fine_entries = 0;
    let mut fine_peak_retained_entries = 0;
    let mut upper_scratch = TimingUpperScratch::default();
    let mut scratch = r.equivalent_score_reuse.map(TimingScoreScratch::with_equivalent_score_reuse)
        .unwrap_or_default();
    let mut fine_seconds = 0.;
    let mut fine_model_seconds = 0.;
    let mut fine_arithmetic_seconds = 0.;
    let mut fine_net_seconds = 0.;
    let mut fine_requests = 0;
    let mut fine_prepares = 0;
    let mut timing_prepares = 0;
    let mut actual_score_queries = 0;
    let mut exact_seconds_by_song = [0.; 3];
    let trace_exact = std::env::var("BANDORI_TIMING_TRACE_EXACT").as_deref() == Ok("1");
    let start = Instant::now();
    let initial_uppers = uppers.clone();
    let warm_start = ((r.first_feasible || r.stream_competitive) && seed_rosters.len() == 3)
        .then(|| {
            seed_rosters
                .iter()
                .map(|seed| {
                    rosters
                        .iter()
                        .position(|r| r.cards.iter().all(|id| seed.contains(id)))
                })
                .collect::<Option<Vec<_>>>()
        })
        .flatten()
        .map(|indices| <[usize; 3]>::try_from(indices).unwrap());
    let policy = if r.stream_competitive && r.companion_bounds {
        lazy_medley::CandidatePolicy::StreamCompatibleFrom(warm_start)
    } else if r.stream_competitive {
        warm_start.map_or(
            lazy_medley::CandidatePolicy::StreamCompetitive,
            lazy_medley::CandidatePolicy::StreamCompetitiveFrom,
        )
    } else if r.first_feasible {
        warm_start.map_or(
            lazy_medley::CandidatePolicy::FirstCompetitive,
            lazy_medley::CandidatePolicy::FirstCompetitiveFrom,
        )
    } else {
        lazy_medley::CandidatePolicy::HighestUpper
    };
    let lazy = lazy_medley::solve_staged_with_policy(
        WideMedleySolverInput {
            current_best: -1,
            team_masks: masks,
            scores: uppers,
        },
        2,
        policy,
        |team, song, stage| {
            let roster = &rosters[team];
            if stage == 0 {
                let start = Instant::now();
                if trace_exact {
                    trace(json!({"event":"fineStart","team":team,"song":song}));
                }
                let parameters = roster
                    .cards
                    .map(|id| member_powers[song][&id].final_parameters());
                let power =
                    |l: [usize; 5]| ScoringPower::from_member_parameters(&l.map(|i| parameters[i]));
                let layouts = if r.share_equivalent_skills {
                    TimingLayouts::for_skills(&roster.skills, power)?
                } else {
                    TimingLayouts::new(power)?
                };
                let mut upper = vec![0; layouts.requests().len()];
                if r.batch_powers {
                    for group in layouts.configurations() {
                        let skills = layouts.requests()[group[0]]
                            .slots
                            .map(|i| roster.skills[i].clone());
                        let model_start = Instant::now();
                        let bound = fine_memos[song].get(&skills, || {
                            fine_prepares += 1;
                            fine_caches[song].prepare(&skills)
                        })?;
                        fine_model_seconds += model_start.elapsed().as_secs_f64();
                        let arithmetic_start = Instant::now();
                        for &i in group {
                            upper[i] =
                                bound.score(layouts.requests()[i].power, &mut upper_scratch)?;
                        }
                        fine_arithmetic_seconds += arithmetic_start.elapsed().as_secs_f64();
                    }
                } else {
                    for (i, query) in layouts.requests().iter().enumerate() {
                        let skills = query.slots.map(|i| roster.skills[i].clone());
                        let model_start = Instant::now();
                        let bound = fine_memos[song]
                            .get(&skills, || {
                                fine_prepares += 1;
                                fine_caches[song].prepare(&skills)
                            })?;
                        fine_model_seconds += model_start.elapsed().as_secs_f64();
                        let arithmetic_start = Instant::now();
                        upper[i] = bound.score(query.power, &mut upper_scratch)?;
                        fine_arithmetic_seconds += arithmetic_start.elapsed().as_secs_f64();
                    }
                }
                fine_requests += upper.len();
                if r.net_bounds {
                    let net_start = Instant::now();
                    for (i, query) in layouts.requests().iter().enumerate() {
                        let skills = query.slots.map(|id| roster.skills[id].clone());
                        upper[i] = upper[i].min(net[song].upper_sequence(&skills, query.power)?);
                    }
                    fine_net_seconds += net_start.elapsed().as_secs_f64();
                }
                let bound = int_score(*upper.iter().max().unwrap())?;
                let bound = if r.net_bounds {
                    bound.min(initial_uppers[team][song])
                } else {
                    bound
                };
                fine.insert((team, song), Fine { layouts, upper });
                fine_entries += 1;
                fine_peak_retained_entries = fine_peak_retained_entries.max(fine.len());
                fine_seconds += start.elapsed().as_secs_f64();
                if trace_exact {
                    trace(json!({"event":"fineEnd","team":team,"song":song,
                        "seconds":start.elapsed().as_secs_f64(),"bound":bound}));
                }
                return Ok(MedleyRefinement::Upper(bound));
            }
            // The staged solver never scores this entry twice. Release its
            // layout/request data after maximizing; winners retain exact power.
            let entry = fine.remove(&(team, song)).expect("exact stage needs a fine entry");
            let exact_start = Instant::now();
            if trace_exact {
                trace(json!({"event":"rosterStart","team":team,"song":song,
                    "cards":roster.cards,"elapsedSeconds":start.elapsed().as_secs_f64()}));
            }
            let mut scored = vec![None; entry.upper.len()];
            let max = entry
                .layouts
                .maximize(&entry.upper, |index, query| {
                    if let Some(value) = scored[index] {
                        return Ok(value);
                    }
                    let skills = query.slots.map(|i| roster.skills[i].clone());
                    let prepare_start = Instant::now();
                    if trace_exact {
                        trace(json!({"event":"prepareStart","team":team,
                            "song":song,"index":index,"power":query.power.value(),"skills":skills}));
                    }
                    let bank = bank_memos[song].get(&skills, || {
                        timing_prepares += 1;
                        caches.prepare(song, &skills)
                    })?;
                    if trace_exact {
                        trace(json!({"event":"prepareEnd","team":team,
                            "song":song,"index":index,"seconds":prepare_start.elapsed().as_secs_f64()}));
                    }
                    if r.batch_powers {
                        for &i in &entry.layouts.configurations()[query.configuration] {
                            let query_start = Instant::now();
                            let value =
                                bank.score(entry.layouts.requests()[i].power, &mut scratch)?;
                            if trace_exact {
                                trace(json!({"event":"scoreEnd","team":team,
                                    "song":song,"index":i,"power":entry.layouts.requests()[i].power.value(),
                                    "seconds":query_start.elapsed().as_secs_f64(),"score":value}));
                            }
                            assert!(value <= entry.upper[i]);
                            scored[i] = Some(value);
                            actual_score_queries += 1;
                        }
                    } else {
                        let query_start = Instant::now();
                        scored[index] = Some(bank.score(query.power, &mut scratch)?);
                        if trace_exact {
                            trace(json!({"event":"scoreEnd","team":team,
                                "song":song,"index":index,"power":query.power.value(),
                                "seconds":query_start.elapsed().as_secs_f64(),"score":scored[index]}));
                        }
                        actual_score_queries += 1;
                    }
                    Ok(scored[index].unwrap())
                })
                .map_err(|e| {
                    eprintln!("{e}");
                    SearchError::Options("layout maximum failed")
                })?;
            let value = int_score(max.score)?;
            exact_seconds_by_song[song] += exact_start.elapsed().as_secs_f64();
            if trace_exact {
                trace(json!({"event":"rosterEnd","team":team,"song":song,
                    "seconds":exact_start.elapsed().as_secs_f64(),"fineSeconds":fine_seconds,
                    "exactSecondsBySong":exact_seconds_by_song,"exactScoreReuse":scratch.exact_score_stats()}));
            }
            eprintln!(
                "roster {team} song {song}: {value}, {}/{} requests",
                max.scored_requests, max.possible_requests
            );
            let power = entry.layouts.requests()[max.chosen_request].power;
            maxima.insert((team, song), Exact { maximum: max, power });
            if r.audit_winner_song == Some(song) {
                fine.insert((team, song), entry);
            }
            Ok(MedleyRefinement::Exact(value))
        },
    );
    let lazy = match lazy {
        Ok(value) => value,
        Err(lazy_medley::LazyMedleyError::Solver(
            bangdream_optimize_medley_solver::MedleySolverError::NoValidPlan,
        )) if external_cutoff > 0 => {
            output["noCompetitivePlan"] = json!(true);
            return Ok(output);
        }
        Err(error) => return Err(error.into()),
    };
    let lazy_seconds = start.elapsed().as_secs_f64();
    let mut winners = Vec::new();
    for (song, &team) in lazy.plan.indices.iter().enumerate() {
        let exact = &maxima[&(team, song)];
        let max = &exact.maximum;
        let roster = &rosters[team];
        let layout = max.recommended_layout.map(|i| roster.cards[i]);
        let actual_power = ScoringPower::from_member_parameters(
            &layout.map(|id| member_powers[song][&id].final_parameters()),
        )?;
        assert_eq!(actual_power, exact.power);
        let mut input = r.inputs[song].clone();
        input.skills = max.chosen_slots.map(|i| roster.skills[i].clone()).to_vec();
        input.power = exact.power.value();
        let bank = caches.prepare(song, &input.skills)?;
        let replay = bank.materialize(exact.power)?;
        assert_eq!(replay.total, max.score);
        winners.push(json!({"team":layout, "roster":roster.cards, "skillOrder":max.chosen_slots.map(|i|roster.cards[i]), "maximum":max, "input":input, "result":replay}));
    }
    output["lazySeconds"] = json!(lazy_seconds);
    output["firstFeasible"] = json!(r.first_feasible);
    output["streamCompetitive"] = json!(r.stream_competitive);
    output["companionBounds"] = json!(r.companion_bounds);
    output["warmStart"] = json!(warm_start);
    output["fineSeconds"] = json!(fine_seconds);
    output["fineStageSeconds"] = json!({"modelPreparation":fine_model_seconds,
        "nativeArithmetic":fine_arithmetic_seconds,"adjacentUpper":fine_net_seconds});
    output["finePreparationStats"] = json!(fine_caches.iter().map(TimingUpperCache::stats).collect::<Vec<_>>());
    output["fineEntries"] = json!(fine_entries);
    output["fineRetainedEntries"] = json!(fine.len());
    output["finePeakRetainedEntries"] = json!(fine_peak_retained_entries);
    output["fineRequests"] = json!(fine_requests);
    output["finePrepares"] = json!(fine_prepares);
    output["timingPrepares"] = json!(timing_prepares);
    output["jointPreparations"] = json!(caches.iter().map(TimingCompileCache::preparation_stats).collect::<Vec<_>>());
    output["sharedGeometryCache"] = json!(caches.stats());
    output["preparationMemos"] = json!({"fine":fine_memos.iter().map(|m|&m.stats).collect::<Vec<_>>(),"banks":bank_memos.iter().map(|m|&m.stats).collect::<Vec<_>>(),"fineCapacityPerSong":if r.prepared_cache{1024}else{0},"bankCapacityPerSong":if r.prepared_cache && std::env::var("BANDORI_TIMING_DISABLE_JOINT_LOCAL_GEOMETRY").as_deref() == Ok("1") {128}else{0}, "geometryCacheBudgetBytesPerSong":if std::env::var("BANDORI_TIMING_DISABLE_JOINT_LOCAL_GEOMETRY").as_deref() == Ok("1") {None}else{Some(256usize*1024*1024)}});
    output["actualScoreQueries"] = json!(actual_score_queries);
    output["exactScoreReuse"] = json!(scratch.exact_score_stats());
    output["exactSecondsBySong"] = json!(exact_seconds_by_song);
    output["solverAndBookkeepingSeconds"] =
        json!(lazy_seconds - fine_seconds - exact_seconds_by_song.iter().sum::<f64>());
    output["lazy"] = json!(lazy);
    output["evaluated"] = json!(maxima
        .iter()
        .map(|(&(team, song), m)| json!({"team":team,"song":song,"maximum":m.maximum}))
        .collect::<Vec<_>>());
    output["timingCaches"] = json!(caches
        .iter()
        .map(|c| json!({"stats":c.stats(),"footprint":c.footprint()}))
        .collect::<Vec<_>>());
    output["winners"] = json!(winners);
    if let Some(song) = r.audit_winner_song {
        let start = Instant::now();
        let team = lazy.plan.indices[song];
        let entry = &fine[&(team, song)];
        let mut values = Vec::new();
        for (i, query) in entry.layouts.requests().iter().enumerate() {
            let skills = query.slots.map(|i| rosters[team].skills[i].clone());
            let score = caches
                .prepare(song, &skills)?
                .score(query.power, &mut scratch)?;
            assert!(score <= entry.upper[i]);
            values.push(score);
        }
        let best = *values.iter().max().unwrap();
        let eager = entry
            .layouts
            .maximize(&vec![best; values.len()], |i, _| Ok(values[i]))?;
        let lazy = &maxima[&(team, song)].maximum;
        assert_eq!(
            (
                lazy.score,
                lazy.recommended_layout,
                lazy.max_score_path_count,
                lazy.chosen_request
            ),
            (
                eager.score,
                eager.recommended_layout,
                eager.max_score_path_count,
                eager.chosen_request
            )
        );
        output["winnerEagerAudit"] = json!({"song":song,"requests":values.len(),"seconds":start.elapsed().as_secs_f64(),"maximum":eager});
    }
    Ok(output)
}

#[cfg(test)]
mod net_bound_policy_tests {
    use super::net_bound_modes;

    #[test]
    fn net_bounds_default_to_state_history_and_preserve_explicit_ab_requests() {
        assert_eq!(net_bound_modes(true, None), (true, true));
        assert_eq!(net_bound_modes(false, None), (false, false));
        assert_eq!(net_bound_modes(true, Some(false)), (true, false));
        assert_eq!(net_bound_modes(false, Some(false)), (false, false));
        assert_eq!(net_bound_modes(false, Some(true)), (true, true));
        assert_eq!(net_bound_modes(true, Some(true)), (true, true));
    }
}


#[path = "timing_seed_proposal.rs"]
mod timing_seed_proposal;
mod product;
mod single;
pub use product::{
    maximize_timing_in_ranges, maximize_timing_medley, maximize_timing_medley_in_ranges,
    TimingMaximizeInput, TimingMedleyInput, TimingSongChart,
};

/// Compatibility adapter for research requests. Files are read only by the CLI.
/// Production constructs the problem from typed in-memory inputs.
#[doc(hidden)]
pub fn solve_research(request: Value, source: Value, saved: Value, seed: Option<Value>)
    -> Result<Value, crate::DataError>
{
    let request: Request = serde_json::from_value(request).map_err(engine_error)?;
    let problem = Problem::from_source(&request, source, saved, seed).map_err(engine_error)?;
    solve(request, &problem).map_err(engine_error)
}

#[doc(hidden)]
pub fn propose_research_seed(source: &Value, cards: &[PreparedCard], area: &bangdream_optimize_core::AreaItemPercent)
    -> Result<Value, crate::DataError>
{
    timing_seed_proposal::propose(source, cards, area).map_err(engine_error)
}

fn engine_error(error: impl std::fmt::Display) -> crate::DataError {
    crate::DataError::InvalidField { field: "hitTiming", value: error.to_string() }
}
