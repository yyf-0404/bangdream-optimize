//! All available equipment under the app's common-three-song equipment rule.
//! Native per-axis powers, componentwise dominance and certified item bounds.
use super::*;
use bangdream_optimize_core::{area_item_combinations, calculate_area_item_percent};
use bangdream_optimize_core::hit_timing::compiled::{
    TimingCompileCachePool as GeometryPool, TimingGeometryPoolStats,
};

type Error = Box<dyn std::error::Error>;

// Historical allocation probes still use independent chart caches. Keep their
// source buildable and report no shared budget for that explicit legacy mode.
trait GeometryRetentionReport {
    fn geometry_retention_report(&self) -> Option<TimingGeometryPoolStats>;
}
impl GeometryRetentionReport for GeometryPool {
    fn geometry_retention_report(&self) -> Option<TimingGeometryPoolStats> {
        Some(self.stats())
    }
}
impl GeometryRetentionReport for Vec<TimingCompileCache> {
    fn geometry_retention_report(&self) -> Option<TimingGeometryPoolStats> { None }
}

struct ItemCase {
    selected: SelectedAreaItems,
    powers: BTreeMap<u32, MemberPower>,
}

fn dominates(a: &ItemCase, b: &ItemCase) -> bool {
    a.powers
        .iter()
        .all(|(id, p)| p.componentwise_dominates(b.powers[id]))
}

pub(super) fn solve_all(
    r: &Request,
    problem: &Problem,
    workspace: &mut Workspace,
) -> Result<Value, Error> {
    if r.census_only || r.upper_only || r.seed_path.is_none() || !r.adjacent_net_bounds {
        return Err("allItems requires adjacent bounds, a replayable seed and full solving; use itemScanOnly for item bounds".into());
    }
    let started = Instant::now();
    let fixed = problem.fixed_items()?;
    let seed_powers = fixed
        .iter()
        .map(|s| problem.powers(s))
        .collect::<Result<Vec<_>, _>>()?;
    // This validates the original seed at its ORIGINAL powers before using its
    // score to discard another item domain. All three songs must share a set.
    if fixed[1] != fixed[0] || fixed[2] != fixed[0] {
        return Err("allItems seed must use one common equipment set for all three songs".into());
    }
    let (seed_score, _) = validate_seed(r, problem, &seed_powers, false)?;
    let area = calculate_area_item_percent(&problem.player.area_item, &problem.defs)?;
    let selections = area_item_combinations(&area);
    if !selections.contains(&fixed[0]) {
        return Err("seed equipment is outside the all-items domain".into());
    }
    let cases: Vec<_> = selections
        .into_iter()
        .map(|selected| {
            Ok::<_, Error>(ItemCase {
                powers: problem.powers(&selected)?,
                selected,
            })
        })
        .collect::<Result<_, _>>()?;
    let catalogue_seconds = started.elapsed().as_secs_f64();
    let dominance_start = Instant::now();
    let dominators: Vec<_> = (0..cases.len())
        .map(|i| {
            (0..cases.len()).find(|&j| {
                i != j
                    && dominates(&cases[j], &cases[i])
                    && (j < i || !dominates(&cases[i], &cases[j]))
            })
        })
        .collect();
    let active: Vec<_> = (0..cases.len())
        .filter(|&i| dominators[i].is_none())
        .collect();
    let dominance_seconds = dominance_start.elapsed().as_secs_f64();
    eprintln!(
        "all items: {} combinations, {} after per-axis dominance",
        cases.len(),
        active.len()
    );

    let upper_start = Instant::now();
    let mut keys = BTreeMap::new();
    let mut skills = Vec::new();
    let mut skill_cases = Vec::new();
    for card in &problem.prepared {
        let resolved = problem.data.hit_timing_skill_cases(
            problem.server,
            TimingTeamCard {
                card_id: card.card_id,
                skill_level: card.skill_level,
            },
        )?;
        let mut ids = [0; 4];
        for (c, s) in resolved.into_iter().enumerate() {
            let key = s.skill.cache_key();
            ids[c] = *keys.entry(key).or_insert_with(|| {
                skills.push(s.skill);
                skills.len() - 1
            });
        }
        skill_cases.push(ids);
    }
    let max_member = cases
        .iter()
        .flat_map(|c| c.powers.values())
        .map(|p| p.linear_total())
        .fold(0_f64, f64::max);
    let cap = ScoringPower::new((max_member * 5. * 1.001 + 1.) as f32)?;
    let models = (0..3)
        .map(|song| workspace.models.adjacent(song, &skills, cap))
        .collect::<Result<Vec<_>, _>>()?;
    let indices: Vec<_> = (0..problem.prepared.len()).collect();
    let mut bounds = vec![None; cases.len()];
    let mut bound_iterations = vec![0; cases.len()];
    for &i in &active {
        let powers = &cases[i].powers;
        let mut choices = Vec::new();
        for (card, ids) in problem.prepared.iter().zip(&skill_cases) {
            let mut row = [[Default::default(); 3]; 4];
            for c in 0..4 {
                for s in 0..3 {
                    row[c][s] = models[s].choice(ids[c], powers[&card.card_id].linear_total())?;
                }
            }
            choices.push(row);
        }
        let oracles: Vec<_> = (0..3)
            .map(|s| {
                timing_pair::Oracle::new(
                    &models[s],
                    &choices,
                    &problem.prepared,
                    &indices,
                    powers,
                    s,
                )
            })
            .collect();
        let (upper, iterations) =
            timing_pair::Oracle::joint_item_upper(&oracles, problem.prepared.len(), seed_score, 32);
        bounds[i] = Some(upper);
        bound_iterations[i] = iterations;
    }
    let upper_seconds = upper_start.elapsed().as_secs_f64();
    let mut order: Vec<_> = active
        .iter()
        .copied()
        .filter(|&i| bounds[i].unwrap() >= seed_score)
        .collect();
    // The replayed seed already supplies a valid floor for every item domain;
    // solving its own equipment first is not needed to establish that floor.
    // Start with the highest certified upper so a stronger exact incumbent can
    // prune the weaker domains before their roster/three-team enumeration.
    // Ordering is only a heuristic: every domain whose upper still reaches the
    // incumbent is solved completely, including ties and the seed's equipment.
    order.sort_by_key(|&i| (std::cmp::Reverse(bounds[i].unwrap()), i));
    eprintln!(
        "all items: {} after score bounds; preparation {:.3}s",
        order.len(),
        started.elapsed().as_secs_f64()
    );
    let mut report = json!({
        "scope":"allAvailableCommonEquipment", "rawCombinations":cases.len(),
        "afterDominance":active.len(), "afterInitialBounds":order.len(),
        "seedScore":seed_score, "catalogueSeconds":catalogue_seconds,
        "dominanceSeconds":dominance_seconds, "upperSeconds":upper_seconds,
        "sharedTimingPreparation":true,
        "avx2Available":avx2_available(),
        "combinations":cases.iter().enumerate().map(|(i,c)| json!({
            "index":i,"items":c.selected,"dominatedBy":dominators[i],
            "upper":bounds[i],"upperIterations":bound_iterations[i],
        })).collect::<Vec<_>>(),
    });
    if r.item_scan_only {
        return Ok(json!({"allItems":report,"incomplete":true,"itemScanOnly":true}));
    }
    let mut best: Option<Value> = None;
    let mut incumbent = seed_score;
    let mut runs = Vec::new();
    let mut complete = true;
    for i in order {
        if bounds[i].unwrap() < incumbent {
            runs.push(json!({"index":i,"skippedByIncumbent":true}));
            continue;
        }
        eprintln!(
            "all items solving {i}: {:?}; incumbent {incumbent}",
            cases[i].selected
        );
        let begin = Instant::now();
        // One item set shared by all three songs. The original fixed seed is
        // replayed at these powers; no score from another domain is trusted.
        let powers = vec![cases[i].powers.clone(); 3];
        let mut output = solve_one(r, problem, &powers, true, incumbent, workspace)?;
        let seconds = begin.elapsed().as_secs_f64();
        if output.get("incomplete").and_then(Value::as_bool) == Some(true) {
            complete = false;
            runs.push(json!({"index":i,"seconds":seconds,"incomplete":true,"poolScan":output["poolScan"]}));
            break;
        }
        if output.get("noCompetitivePlan").and_then(Value::as_bool) == Some(true) {
            runs.push(json!({"index":i,"seconds":seconds,"noCompetitivePlan":true,"cutoffScore":incumbent,"poolScan":output["poolScan"],"coarseSeconds":output["coarseSeconds"],"enumerationSeconds":output["enumerationSeconds"]}));
            continue;
        }
        let score = output["lazy"]["plan"]["score"]
            .as_u64()
            .ok_or("item run lacks a complete score")?;
        if score > bounds[i].unwrap() {
            return Err("item score exceeds its certified upper".into());
        }
        runs.push(json!({"index":i,"seconds":seconds,"score":score,"cutoffScore":incumbent,"poolScan":output["poolScan"],"adjacentProfiles":output["adjacentProfiles"],"fineSeconds":output["fineSeconds"],"fineEntries":output["fineEntries"],"fineRetainedEntries":output["fineRetainedEntries"],"finePeakRetainedEntries":output["finePeakRetainedEntries"],"fineStageSeconds":output["fineStageSeconds"],"finePreparationStats":output["finePreparationStats"],"jointPreparations":output["jointPreparations"],"sharedGeometryCache":output["sharedGeometryCache"],"exactSecondsBySong":output["exactSecondsBySong"],"solverAndBookkeepingSeconds":output["solverAndBookkeepingSeconds"],"coarseSeconds":output["coarseSeconds"],"enumerationSeconds":output["enumerationSeconds"]}));
        // An earlier domain can finish below the original replayed seed. Do
        // not retain it as the winner: a later domain may only tie that seed.
        if score >= incumbent && (best.is_none() || score > incumbent) {
            incumbent = score;
            output["selectedItems"] = json!(cases[i].selected);
            best = Some(output);
        }
    }
    report["complete"] = json!(complete);
    report["elapsedSeconds"] = json!(started.elapsed().as_secs_f64());
    report["runs"] = json!(runs);
    report["sharedModels"] = workspace.models.report();
    // Lifetime totals after every equipment domain, including later domains
    // that did not improve the retained best result.
    report["sharedGeometryCache"] = json!(workspace.caches.geometry_retention_report());
    report["jointPreparations"] = json!(workspace.caches.iter()
        .map(TimingCompileCache::preparation_stats).collect::<Vec<_>>());
    report["timingCaches"] = json!(workspace.caches.iter()
        .map(|cache| json!({"stats":cache.stats(),"footprint":cache.footprint()}))
        .collect::<Vec<_>>());
    let mut output = best.unwrap_or_else(|| json!({}));
    output["allItems"] = report;
    output["incomplete"] = json!(!complete);
    if complete
        && output["lazy"]["plan"]["score"]
            .as_u64()
            .is_none_or(|s| s < seed_score)
    {
        return Err("all-items search failed to retain a replayed seed score".into());
    }
    Ok(output)
}

fn avx2_available() -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        std::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn case(axes: [u32; 3]) -> ItemCase {
        ItemCase {
            selected: SelectedAreaItems {
                band: "1".into(),
                attribute: "cool".into(),
                magazine: bangdream_optimize_core::Magazine::Performance,
            },
            powers: BTreeMap::from([(
                1,
                MemberPower::from_bonuses(axes, [0.; 3], [0.; 3], 0., [0.; 3]).unwrap(),
            )]),
        }
    }
    #[test]
    fn item_dominance_requires_every_native_axis_not_total_power() {
        let a = case([110, 100, 100]);
        let b = case([100, 100, 100]);
        assert!(dominates(&a, &b));
        assert!(!dominates(&b, &a));
        let c = case([120, 95, 100]);
        assert!(c.powers[&1].linear_total() > a.powers[&1].linear_total());
        assert!(!dominates(&c, &a));
        assert!(!dominates(&a, &c));
    }
}
