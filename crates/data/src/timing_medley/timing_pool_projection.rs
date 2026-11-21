//! Complete key-occupancy envelopes before enumerating actual five-card teams.
//! Every key remains a separate choice. Non-keys can retain their three-song
//! skill profiles while relaxing power within each profile group.
use super::*;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub keys: Vec<u32>,
    pub group_nodes: u64,
    pub member_nodes: u64,
    pub leaves: u64,
    pub audited_teams: usize,
    pub occupied_masks: usize,
    pub complete: bool,
    pub excludes_all_plans: bool,
    pub envelope_seconds: f64,
    pub table_seconds: f64,
    pub root_companions: [Option<i128>; 3],
    pub update_pruning: bool,
    pub skill_groups: bool,
    pub non_key_alternatives: u64,
    pub non_key_groups: u64,
    pub max_non_key_groups: usize,
    pub update_checks: u64,
    pub covered_nodes: u64,
    pub checked_masks: u64,
    /// Research audit: exact maxima, not merely the root companion totals.
    pub mask_uppers: Vec<(usize, [u64; 3])>,
}

// Each source card belongs to exactly one group. Profile identity is compared
// separately for each song: the same card can resolve differently by song.
// Unknown identities are safe to merge, but remain an unknown/relaxed choice.
fn merge_non_key(groups: &mut Vec<Choice>, value: Choice, split: bool) {
    let key = value.map(|c| c.skill_profile());
    if let Some(group) = groups
        .iter_mut()
        .find(|g| !split || g.map(|c| c.skill_profile()) == key)
    {
        *group = union(*group, value);
    } else {
        groups.push(value);
    }
}

// Exact possible added key masks for each suffix of the five fixed characters.
// A zero choice exists only when that character has a non-key alternative.
fn suffix_masks(options: &[Vec<usize>; 5]) -> [Vec<usize>; 6] {
    let mut result: [Vec<usize>; 6] = std::array::from_fn(|_| vec![]);
    result[5].push(0);
    for depth in (0..5).rev() {
        for &bit in &options[depth] {
            for i in 0..result[depth + 1].len() {
                let rest = result[depth + 1][i];
                result[depth].push(bit | rest);
            }
        }
        result[depth].sort_unstable();
        result[depth].dedup();
    }
    result
}

fn covered(
    best: &[Option<[u64; 3]>],
    suffix: &[usize],
    mask: usize,
    upper: [u64; 3],
    checked: &mut u64,
) -> bool {
    suffix.iter().all(|&extra| {
        *checked += 1;
        best[mask | extra].is_some_and(|b| (0..3).all(|s| b[s] >= upper[s]))
    })
}
struct Projection<'a, 'b> {
    scan: &'a Scan<'b>,
    keys: Vec<usize>,
    bits: Vec<usize>,
    best: Vec<Option<[u64; 3]>>,
    stats: Stats,
}
impl Projection<'_, '_> {
    fn stopped(&self) -> bool {
        self.stats.group_nodes + self.stats.member_nodes > self.scan.max_nodes
    }
    fn groups(
        &mut self,
        groups: &[Group],
        suffix: Option<&[Choice]>,
        chosen: &mut Vec<usize>,
        from: usize,
        attr: bool,
    ) {
        self.stats.group_nodes += 1;
        if self.stopped() || groups.len() - from < 5 - chosen.len() {
            return;
        }
        let remaining = remaining_union(groups, from, suffix);
        let values = std::array::from_fn(|i| {
            if i < chosen.len() {
                groups[chosen[i]].relaxed
            } else {
                remaining
            }
        });
        if self.scan.competitive_upper(&values, &[]).is_none() {
            return;
        }
        if chosen.len() < 5 {
            for i in from..=groups.len() - (5 - chosen.len()) {
                chosen.push(i);
                self.groups(groups, suffix, chosen, i + 1, attr);
                chosen.pop();
                if self.stopped() {
                    break;
                }
            }
            return;
        }
        let band = self.scan.cards[groups[chosen[0]].cards[0]].band_id;
        let same_band = chosen
            .iter()
            .all(|&g| self.scan.cards[groups[g].cards[0]].band_id == band);
        let context = usize::from(same_band) + 2 * usize::from(attr);
        let mut options: [Vec<(Option<usize>, Choice)>; 5] = std::array::from_fn(|_| vec![]);
        for g in 0..5 {
            let mut other = Vec::new();
            for &i in &groups[chosen[g]].case_cards[usize::from(same_band)] {
                let value = self.scan.choices[i][context];
                if self.bits[i] != 0 {
                    options[g].push((Some(i), value));
                } else {
                    self.stats.non_key_alternatives += 1;
                    merge_non_key(&mut other, value, self.stats.skill_groups);
                }
            }
            self.stats.non_key_groups += other.len() as u64;
            self.stats.max_non_key_groups = self.stats.max_non_key_groups.max(other.len());
            for value in other {
                options[g].push((None, value));
            }
            if options[g].is_empty() {
                return;
            }
        }
        let mut values =
            std::array::from_fn(|g| options[g].iter().map(|v| v.1).reduce(union).unwrap());
        let masks = self.stats.update_pruning.then(|| {
            suffix_masks(&std::array::from_fn(|g| {
                options[g]
                    .iter()
                    .map(|v| v.0.map_or(0, |i| self.bits[i]))
                    .collect()
            }))
        });
        self.members(&options, &mut values, &mut vec![], masks.as_ref(), 0, 0);
    }
    fn members(
        &mut self,
        options: &[Vec<(Option<usize>, Choice)>; 5],
        values: &mut [Choice; 5],
        chosen_keys: &mut Vec<usize>,
        masks: Option<&[Vec<usize>; 6]>,
        depth: usize,
        mask: usize,
    ) {
        self.stats.member_nodes += 1;
        if self.stopped() {
            return;
        }
        let upper = if let Some(masks) = masks {
            let cheap = std::array::from_fn(|s| {
                self.scan.models[s]
                    .separable_upper(&values.map(|c| c[s]))
                    .expect("validated power cap")
            });
            if !self.scan.competitive(cheap, chosen_keys) {
                return;
            }
            self.stats.update_checks += 1;
            // This skips only redundant updates of *every* possible occupied
            // mask, not a competitiveness check using a partial companion table.
            // The separable bound is monotone under componentwise unions and
            // encloses every descendant's coupled upper in the same slot order.
            if covered(
                &self.best,
                &masks[depth],
                mask,
                cheap,
                &mut self.stats.checked_masks,
            ) {
                self.stats.covered_nodes += 1;
                return;
            }
            // Reuse the cheap values for merged/unknown skills. Do not run the
            // same separable gate twice before the coupled query.
            let upper = std::array::from_fn(|s| {
                let choices = values.map(|c| c[s]);
                if self.scan.models[s].has_adjacent_roster(&choices) {
                    self.scan.models[s]
                        .upper(&choices)
                        .expect("validated power cap")
                } else {
                    cheap[s]
                }
            });
            self.scan.competitive(upper, chosen_keys).then_some(upper)
        } else {
            // Only forced keys pay prices; unknown ordinary cards are relaxed.
            self.scan.competitive_upper(values, chosen_keys)
        };
        let Some(upper) = upper else {
            return;
        };
        if depth == 5 {
            let best = self.best[mask].get_or_insert(upper);
            for s in 0..3 {
                best[s] = best[s].max(upper[s]);
            }
            self.stats.leaves += 1;
            return;
        }
        let old = values[depth];
        for &(key, value) in &options[depth] {
            values[depth] = value;
            if let Some(i) = key {
                chosen_keys.push(i);
            }
            self.members(
                options,
                values,
                chosen_keys,
                masks,
                depth + 1,
                mask | key.map_or(0, |i| self.bits[i]),
            );
            if key.is_some() {
                chosen_keys.pop();
            }
            if self.stopped() {
                break;
            }
        }
        values[depth] = old;
    }
}

pub fn prepare(
    scan: &Scan<'_>,
    partitions: &[(Option<Attribute>, Vec<Group>)],
    plan_only: bool,
) -> Result<(Option<key_pair_bound::PrefixUpper>, Stats), Error> {
    let start = Instant::now();
    let keys = Pair::key_cards(&scan.pairs, scan.cards.len(), 16);
    let mut bits = vec![0; scan.cards.len()];
    for (bit, &i) in keys.iter().enumerate() {
        bits[i] = 1 << bit;
    }
    let mut p = Projection {
        scan,
        best: vec![None; 1 << keys.len()],
        bits,
        stats: Stats {
            keys: keys.iter().map(|&i| scan.cards[i].card_id).collect(),
            update_pruning: std::env::var("BANDORI_TIMING_PROJECTION_UPDATES").as_deref()
                != Ok("0"),
            skill_groups: std::env::var("BANDORI_TIMING_PROJECTION_SKILL_GROUPS").as_deref()
                != Ok("0"),
            ..Stats::default()
        },
        keys,
    };
    for (attr, groups) in partitions {
        let suffix = suffix_unions(groups);
        p.groups(groups, suffix.as_deref(), &mut vec![], 0, attr.is_some());
        if p.stopped() {
            break;
        }
    }
    p.stats.envelope_seconds = start.elapsed().as_secs_f64();
    p.stats.complete = !p.stopped();
    if !p.stats.complete {
        return Ok((None, p.stats));
    }
    // Independently enumerate the small original card pool, without using
    // projected group/member pruning, and check each competitive leaf envelope.
    if scan.cards.len() <= 25 {
        use bangdream_optimize_core::hit_timing::enumeration::{enumerate, TimingCardIdentity};
        let identities: Vec<_> = scan
            .cards
            .iter()
            .map(|c| TimingCardIdentity {
                card_id: c.card_id,
                character_id: c.character_id,
            })
            .collect();
        enumerate(&identities, 100_000, |ids| {
            let attr = ids
                .iter()
                .all(|&i| scan.cards[i].attribute == scan.cards[ids[0]].attribute)
                .then_some(scan.cards[ids[0]].attribute);
            let band = ids
                .iter()
                .all(|&i| scan.cards[i].band_id == scan.cards[ids[0]].band_id);
            let groups = &partitions.iter().find(|(a, _)| *a == attr).unwrap().1;
            let mut ordered = vec![];
            for &i in &ids {
                let Some(g) = groups
                    .iter()
                    .position(|g| g.case_cards[usize::from(band)].contains(&i))
                else {
                    return Ok::<_, Error>(());
                };
                ordered.push((g, i));
            }
            ordered.sort_unstable();
            let context = usize::from(band) + 2 * usize::from(attr.is_some());
            let upper = scan.upper(&std::array::from_fn(|g| {
                scan.choices[ordered[g].1][context]
            }));
            if scan.competitive(upper, &ids) {
                let mask = ids.iter().fold(0, |m, &i| m | p.bits[i]);
                let envelope =
                    p.best[mask].expect("complete projection covers every competitive key mask");
                assert!(
                    (0..3).all(|s| upper[s] <= envelope[s]),
                    "projected score underestimated a leaf"
                );
                p.stats.audited_teams += 1;
            }
            Ok(())
        })?;
    }
    let start = Instant::now();
    let mut rows = vec![];
    for (mask, upper) in p.best.iter().enumerate() {
        if let Some(upper) = upper {
            p.stats.mask_uppers.push((mask, *upper));
            let mut cards = [usize::MAX; 5];
            let mut count = 0;
            for (bit, &key) in p.keys.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    cards[count] = key;
                    count += 1;
                }
            }
            let mut scores = [0; 3];
            for s in 0..3 {
                scores[s] = i64::try_from(upper[s])?;
            }
            rows.push(key_pair_bound::Candidate { cards, scores });
        }
    }
    let table = key_pair_bound::PrefixUpper::new(&rows, &p.keys)?;
    // Every competitive plan must have a first-song row and two companions
    // mutually disjoint on the projected keys. Non-key conflicts are relaxed.
    if plan_only {
        p.stats.excludes_all_plans = !rows.iter().any(|row| {
            table.companions(&row.cards)[0].is_some_and(|pair| {
                i128::from(row.scores[0]) + pair >= i128::from(scan.stats.seed_score)
            })
        });
    }
    p.stats.root_companions = table.companions(&[]);
    p.stats.occupied_masks = table.occupied_masks();
    p.stats.table_seconds = start.elapsed().as_secs_f64();
    Ok((Some(table), p.stats))
}

#[cfg(test)]
#[path = "timing_pool_projection_tests.rs"]
mod tests;
