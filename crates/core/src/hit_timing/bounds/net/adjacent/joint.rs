//! Optimize the six requests jointly over a member subset and native clock.
//! Once two prefixes expose the same future clock, their exact earlier skill
//! order is irrelevant to the local per-start rows and can be merged by max.
use super::*;

#[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
struct Interval {
    start: u32,
    end: u32,
    correction: u32,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct Key {
    mask: u8,
    hit: u32,
    // First frame at which the last requested skill permits a new start.
    ready: u32,
    // Only correcting intervals can affect subsequent trigger acceptance.
    // Retain pending intervals too: another trigger can arrive after they start.
    active: [Interval; 6],
}

#[cfg(test)]
pub(super) fn independent_cap(members: [usize; 5], caps: &[[f64; 6]]) -> f64 {
    independent_best(members, caps).0
}

pub(super) fn independent_best(members: [usize; 5], caps: &[[f64; 6]]) -> (f64, [usize; 6]) {
    let mut dp = [f64::NEG_INFINITY; 32];
    let mut orders = [[usize::MAX; 6]; 32];
    dp[0] = 0.;
    for mask in 0usize..31 {
        if dp[mask] == f64::NEG_INFINITY {
            continue;
        }
        let slot = mask.count_ones() as usize;
        for (i, &k) in members.iter().enumerate() {
            if mask & (1 << i) == 0 {
                let next = mask | (1 << i);
                let value = (dp[mask] + caps[k][slot]).next_up();
                let mut order = orders[mask];
                order[slot] = k;
                if value > dp[next] || value == dp[next] && order < orders[next] {
                    dp[next] = value;
                    orders[next] = order;
                }
            }
        }
    }
    let captain = members
        .into_iter()
        .min_by(|&a, &b| caps[b][5].total_cmp(&caps[a][5]).then(a.cmp(&b)))
        .unwrap();
    orders[31][5] = captain;
    (((dp[31] + caps[captain][5]).next_up()).max(0.), orders[31])
}

#[cfg(test)]
pub(super) fn schedule(
    domains: &[Vec<(u32, Judgement)>; 6],
    ranks: [usize; 6],
    members: [usize; 5],
    playing: [u32; 5],
    correction: [u32; 5],
    options: WindowOptions,
    stats: &mut TimingAdjacentStats,
    value: impl FnMut(usize, usize, u32, &mut TimingAdjacentStats) -> Result<f64, ReplayError>,
) -> Result<Option<f64>, ReplayError> {
    schedule_impl(
        domains, ranks, members, playing, correction, None, 0., options, stats, value,
    )
}

pub(super) fn schedule_bounded(
    domains: &[Vec<(u32, Judgement)>; 6],
    ranks: [usize; 6],
    members: [usize; 5],
    playing: [u32; 5],
    correction: [u32; 5],
    caps: [[f64; 6]; 5],
    incumbent: f64,
    options: WindowOptions,
    stats: &mut TimingAdjacentStats,
    value: impl FnMut(usize, usize, u32, &mut TimingAdjacentStats) -> Result<f64, ReplayError>,
) -> Result<Option<f64>, ReplayError> {
    schedule_impl(
        domains,
        ranks,
        members,
        playing,
        correction,
        Some(caps),
        incumbent,
        options,
        stats,
        value,
    )
}

/// Complete the remaining assignment from its actual forward sum. A reverse
/// suffix sum added once would not preserve per-slot floating next_up rounding.
pub(super) fn remaining_cap(prefix: f64, used: u8, caps: &[[f64; 6]; 5]) -> f64 {
    let mut dp = [f64::NEG_INFINITY; 32];
    dp[usize::from(used)] = prefix;
    for mask in usize::from(used)..31 {
        if dp[mask] == f64::NEG_INFINITY {
            continue;
        }
        let slot = mask.count_ones() as usize;
        for (i, row) in caps.iter().enumerate() {
            if mask & (1 << i) == 0 {
                let next = mask | (1 << i);
                dp[next] = dp[next].max((dp[mask] + row[slot]).next_up());
            }
        }
    }
    let encore = caps
        .iter()
        .map(|row| row[5])
        .fold(f64::NEG_INFINITY, f64::max);
    (dp[31] + encore).next_up()
}

fn schedule_impl(
    domains: &[Vec<(u32, Judgement)>; 6],
    ranks: [usize; 6],
    members: [usize; 5],
    playing: [u32; 5],
    correction: [u32; 5],
    caps: Option<[[f64; 6]; 5]>,
    mut incumbent: f64,
    options: WindowOptions,
    stats: &mut TimingAdjacentStats,
    mut value: impl FnMut(usize, usize, u32, &mut TimingAdjacentStats) -> Result<f64, ReplayError>,
) -> Result<Option<f64>, ReplayError> {
    debug_assert!(members.windows(2).all(|p| p[0] <= p[1]));
    let mut remaining = caps.map(remaining::Caps::new);
    if remaining.as_mut().is_some_and(|c| c.prunes(0., 0, incumbent, stats)) {
        stats.roster_bound_pruned += 1;
        return Ok(Some(incumbent));
    }
    let cooling = countdown(0.75) + 1;
    let earliest: [u32; 6] =
        std::array::from_fn(|slot| domains[slot].iter().map(|p| p.0).min().unwrap_or(u32::MAX));
    let choices: [Vec<usize>; 32] = std::array::from_fn(|mask| {
        (0..5)
            .filter(|&i| {
                if mask == 31 {
                    i == 0 || members[i] != members[i - 1]
                } else {
                    mask & (1 << i) == 0
                        && (i == 0 || members[i] != members[i - 1] || mask & (1 << (i - 1)) != 0)
                }
            })
            .collect()
    });
    let empty = [Interval::default(); 6];
    let mut states = HashMap::from([(
        Key {
            mask: 0,
            hit: 0,
            ready: 0,
            active: empty,
        },
        0_f64,
    )]);
    let mut transitions = 0;
    for slot in 0..6 {
        let mut next = HashMap::<Key, f64>::new();
        for (key, score) in states {
            if slot > 0 && remaining.as_mut()
                .is_some_and(|c| c.prunes(score, key.mask, incumbent, stats))
            {
                stats.roster_bound_pruned += 1;
                continue;
            }
            for &member in &choices[usize::from(key.mask)] {
                let after = key.mask | (1 << member);
                if caps.as_ref().is_some_and(|c| {
                    let upper = (score + c[member][slot]).next_up();
                    if slot == 5 {
                        upper <= incumbent
                    } else {
                        remaining.as_mut().unwrap().prunes(upper, after, incumbent, stats)
                    }
                }) {
                    stats.roster_bound_pruned += 1;
                    continue;
                }
                for &(hit, raw) in &domains[slot] {
                    transitions += 1;
                    stats.schedule_transitions += 1;
                    stats.roster_transitions += 1;
                    if transitions > options.max_transitions {
                        stats.roster_budget_fallbacks += 1;
                        return Ok(None);
                    }
                    if slot > 0
                        && (hit < key.hit || (hit == key.hit && ranks[slot] <= ranks[slot - 1]))
                    {
                        continue;
                    }
                    if raw < Judgement::Great
                        && !key.active.iter().any(|interval| {
                            interval.correction != 0
                                && interval.start < hit
                                && hit <= interval.end
                                && corrected(raw, interval.correction).is_some()
                        })
                    {
                        continue;
                    }
                    let start = hit.max(key.ready);
                    let score = (score + value(slot, member, start, stats)?).next_up();
                    if remaining.as_mut().is_some_and(|c| {
                        if slot == 5 {
                            score <= incumbent
                        } else {
                            c.prunes(score, after, incumbent, stats)
                        }
                    }) {
                        stats.roster_bound_pruned += 1;
                        continue;
                    }
                    if slot == 5 {
                        incumbent = incumbent.max(score);
                    }
                    let mut out = Key {
                        mask: after,
                        hit: 0,
                        ready: 0,
                        active: empty,
                    };
                    if slot < 5 {
                        let first = earliest[slot + 1];
                        out.hit = if hit < first { 0 } else { hit };
                        let ready = start + playing[member] + cooling + 1;
                        out.ready = if ready <= first { 0 } else { ready };
                        let new = Interval {
                            start,
                            end: start + playing[member],
                            correction: correction[member],
                        };
                        // FIFO gives chronological intervals. Compact in that
                        // order so histories with equal observable intervals
                        // have identical keys, regardless of earlier slots.
                        for (i, interval) in key
                            .active
                            .into_iter()
                            .chain(std::iter::once(new))
                            .filter(|v| v.correction != 0 && v.end >= first)
                            .enumerate()
                        {
                            out.active[i] = interval;
                        }
                    }
                    match next.entry(out) {
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            *entry.get_mut() = entry.get().max(score);
                            stats.merged_roster_prefixes += 1;
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(score);
                        }
                    }
                    stats.peak_roster_states = stats.peak_roster_states.max(next.len());
                    if next.len() > options.max_states {
                        stats.roster_budget_fallbacks += 1;
                        return Ok(None);
                    }
                }
            }
        }
        states = next;
    }
    Ok(Some(states.into_values().fold(incumbent.max(0.), f64::max)))
}

#[cfg(test)]
mod tests;
