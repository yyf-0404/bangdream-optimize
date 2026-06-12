use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DominatorCoverSummary {
    pub dominators: usize,
    pub blocked_teammates: usize,
    pub other_team_capacity: usize,
    pub free_replacements: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DominanceGraph {
    incoming_by_target: Vec<Vec<usize>>,
}

impl DominanceGraph {
    pub fn new(node_count: usize) -> Self {
        Self {
            incoming_by_target: vec![Vec::new(); node_count],
        }
    }

    pub fn add_edge(&mut self, dominator_idx: usize, target_idx: usize) {
        self.incoming_by_target[target_idx].push(dominator_idx);
    }

    pub fn incoming(&self, target_idx: usize) -> &[usize] {
        &self.incoming_by_target[target_idx]
    }

    pub fn transitive_closure(&mut self) {
        let node_count = self.incoming_by_target.len();
        if node_count == 0 {
            return;
        }

        let word_count = node_count.div_ceil(u64::BITS as usize);
        let mut incoming_bits = vec![vec![0_u64; word_count]; node_count];
        for (target_idx, dominators) in self.incoming_by_target.iter().enumerate() {
            for &dominator_idx in dominators {
                incoming_bits[target_idx][dominator_idx / u64::BITS as usize] |=
                    1_u64 << (dominator_idx % u64::BITS as usize);
            }
        }

        for via_idx in 0..node_count {
            let via_bits = incoming_bits[via_idx].clone();
            for target_idx in 0..node_count {
                if (incoming_bits[target_idx][via_idx / u64::BITS as usize]
                    & (1_u64 << (via_idx % u64::BITS as usize)))
                    == 0
                {
                    continue;
                }
                for word_idx in 0..word_count {
                    incoming_bits[target_idx][word_idx] |= via_bits[word_idx];
                }
            }
        }

        for (target_idx, bits) in incoming_bits.iter_mut().enumerate() {
            bits[target_idx / u64::BITS as usize] &= !(1_u64 << (target_idx % u64::BITS as usize));
            self.incoming_by_target[target_idx] = (0..node_count)
                .filter(|&dominator_idx| {
                    (bits[dominator_idx / u64::BITS as usize]
                        & (1_u64 << (dominator_idx % u64::BITS as usize)))
                        != 0
                })
                .collect();
        }
    }
}

pub fn retain_undominated_same_group<T, G, Group, Dominates>(
    items: Vec<T>,
    group: Group,
    dominates: Dominates,
) -> Vec<T>
where
    G: PartialEq,
    Group: Fn(&T) -> G,
    Dominates: Fn(&T, &T) -> bool,
{
    let keep = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            !items.iter().enumerate().any(|(other_idx, other)| {
                other_idx != idx && group(other) == group(item) && dominates(other, item)
            })
        })
        .collect::<Vec<_>>();

    items
        .into_iter()
        .zip(keep)
        .filter_map(|(item, keep)| keep.then_some(item))
        .collect()
}

pub fn retain_by_same_group_dominator_cover<T, G, Group, Dominates>(
    items: Vec<T>,
    group: Group,
    dominates: Dominates,
    required_cover: usize,
) -> Vec<T>
where
    G: Copy + PartialEq,
    Group: Fn(&T) -> G,
    Dominates: Fn(&T, &T) -> bool,
{
    let graph = dominance_graph_for_items(
        &items,
        |_, _| true,
        |_, dominator, _, target| dominates(dominator, target),
    );
    let keep = items
        .iter()
        .enumerate()
        .map(|(idx, _)| {
            same_group_dominator_cover(&graph, idx, &items, &group, required_cover) < required_cover
        })
        .collect::<Vec<_>>();

    items
        .into_iter()
        .zip(keep)
        .filter_map(|(item, keep)| keep.then_some(item))
        .collect()
}

pub fn dominance_graph_for_items<T, Allow, Dominates>(
    items: &[T],
    mut allow: Allow,
    mut dominates: Dominates,
) -> DominanceGraph
where
    Allow: FnMut(usize, &T) -> bool,
    Dominates: FnMut(usize, &T, usize, &T) -> bool,
{
    let mut graph = DominanceGraph::new(items.len());
    for (target_idx, target) in items.iter().enumerate() {
        if !allow(target_idx, target) {
            continue;
        }
        for (dominator_idx, dominator) in items.iter().enumerate() {
            if dominator_idx == target_idx
                || !allow(dominator_idx, dominator)
                || !dominates(dominator_idx, dominator, target_idx, target)
            {
                continue;
            }
            graph.add_edge(dominator_idx, target_idx);
        }
    }
    graph.transitive_closure();
    graph
}

pub fn dominance_graph_for_index_subset<Dominates>(
    node_count: usize,
    indices: &[usize],
    dominates: Dominates,
) -> DominanceGraph
where
    Dominates: FnMut(usize, usize) -> bool,
{
    dominance_graph_for_index_subset_inner(node_count, indices, None, dominates)
}

pub fn dominance_graph_for_index_subset_with_base<Dominates>(
    node_count: usize,
    indices: &[usize],
    base_graph: &DominanceGraph,
    dominates: Dominates,
) -> DominanceGraph
where
    Dominates: FnMut(usize, usize) -> bool,
{
    dominance_graph_for_index_subset_inner(node_count, indices, Some(base_graph), dominates)
}

fn dominance_graph_for_index_subset_inner<Dominates>(
    node_count: usize,
    indices: &[usize],
    base_graph: Option<&DominanceGraph>,
    mut dominates: Dominates,
) -> DominanceGraph
where
    Dominates: FnMut(usize, usize) -> bool,
{
    let mut graph = DominanceGraph::new(node_count);
    if indices.is_empty() {
        return graph;
    }

    let subset_count = indices.len();
    let word_count = subset_count.div_ceil(u64::BITS as usize);
    let mut index_to_pos = vec![usize::MAX; node_count];
    for (pos, &idx) in indices.iter().enumerate() {
        debug_assert!(idx < node_count);
        index_to_pos[idx] = pos;
    }
    let mut incoming_bits = vec![vec![0_u64; word_count]; subset_count];
    if let Some(base_graph) = base_graph {
        for (target_pos, &target_idx) in indices.iter().enumerate() {
            for &dominator_idx in base_graph.incoming(target_idx) {
                let dominator_pos = index_to_pos[dominator_idx];
                if dominator_pos == usize::MAX || dominator_pos == target_pos {
                    continue;
                }
                incoming_bits[target_pos][dominator_pos / u64::BITS as usize] |=
                    1_u64 << (dominator_pos % u64::BITS as usize);
            }
        }
    }
    for (target_pos, &target_idx) in indices.iter().enumerate() {
        for (dominator_pos, &dominator_idx) in indices.iter().enumerate() {
            if dominator_pos == target_pos
                || (incoming_bits[target_pos][dominator_pos / u64::BITS as usize]
                    & (1_u64 << (dominator_pos % u64::BITS as usize)))
                    != 0
                || !dominates(dominator_idx, target_idx)
            {
                continue;
            }
            incoming_bits[target_pos][dominator_pos / u64::BITS as usize] |=
                1_u64 << (dominator_pos % u64::BITS as usize);
        }
    }

    for via_pos in 0..subset_count {
        let via_bits = incoming_bits[via_pos].clone();
        for target_pos in 0..subset_count {
            if (incoming_bits[target_pos][via_pos / u64::BITS as usize]
                & (1_u64 << (via_pos % u64::BITS as usize)))
                == 0
            {
                continue;
            }
            for word_idx in 0..word_count {
                incoming_bits[target_pos][word_idx] |= via_bits[word_idx];
            }
        }
    }

    for (target_pos, &target_idx) in indices.iter().enumerate() {
        incoming_bits[target_pos][target_pos / u64::BITS as usize] &=
            !(1_u64 << (target_pos % u64::BITS as usize));
        graph.incoming_by_target[target_idx] = (0..subset_count)
            .filter_map(|dominator_pos| {
                ((incoming_bits[target_pos][dominator_pos / u64::BITS as usize]
                    & (1_u64 << (dominator_pos % u64::BITS as usize)))
                    != 0)
                    .then_some(indices[dominator_pos])
            })
            .collect();
    }

    graph
}

pub fn same_group_dominator_cover<T, G, Group>(
    graph: &DominanceGraph,
    target_idx: usize,
    items: &[T],
    group: Group,
    max_cover: usize,
) -> usize
where
    G: PartialEq,
    Group: Fn(&T) -> G,
{
    let target_group = group(&items[target_idx]);
    graph
        .incoming(target_idx)
        .iter()
        .filter(|&&dominator_idx| group(&items[dominator_idx]) == target_group)
        .take(max_cover)
        .count()
}

pub fn cross_group_dominator_cover<T, Group>(
    graph: &DominanceGraph,
    target_idx: usize,
    items: &[T],
    group: Group,
    team_size: usize,
    team_count: usize,
) -> usize
where
    Group: Fn(&T) -> u32,
{
    let counts_by_group = dominator_counts_by_group(graph, target_idx, items, &group);
    dominator_cover_after_worst_teammate_groups(
        counts_by_group,
        group(&items[target_idx]),
        team_size,
        team_count,
    )
}

pub fn dominator_counts_by_group<T, Group>(
    graph: &DominanceGraph,
    target_idx: usize,
    items: &[T],
    group: Group,
) -> BTreeMap<u32, usize>
where
    Group: Fn(&T) -> u32,
{
    let mut counts_by_group = BTreeMap::new();
    for &dominator_idx in graph.incoming(target_idx) {
        *counts_by_group
            .entry(group(&items[dominator_idx]))
            .or_default() += 1;
    }
    counts_by_group
}

pub fn dominator_cover_after_worst_teammate_groups(
    counts_by_group: BTreeMap<u32, usize>,
    target_group_id: u32,
    team_size: usize,
    team_count: usize,
) -> usize {
    dominator_cover_summary_after_worst_teammate_groups(
        &counts_by_group,
        target_group_id,
        team_size,
        team_count,
    )
    .free_replacements
}

pub fn dominator_cover_summary_after_worst_teammate_groups(
    counts_by_group: &BTreeMap<u32, usize>,
    target_group_id: u32,
    team_size: usize,
    team_count: usize,
) -> DominatorCoverSummary {
    let dominators: usize = counts_by_group.values().sum();
    let teammate_slots = team_size.saturating_sub(1);
    let other_team_count = team_count.saturating_sub(1);
    let other_team_slot_count = team_size * other_team_count;
    let total_other_team_capacity: usize = counts_by_group
        .values()
        .map(|&count| count.min(other_team_count))
        .sum();
    let max_capacity_removed = total_other_team_capacity;
    let mut dp = vec![vec![-1_isize; max_capacity_removed + 1]; team_size];
    dp[0][0] = 0;

    for (&group_id, &count) in counts_by_group {
        if group_id == target_group_id {
            continue;
        }
        let capacity_removed = count.min(other_team_count);
        for chosen in (0..teammate_slots).rev() {
            let (current_layers, next_layers) = dp.split_at_mut(chosen + 1);
            let current = &current_layers[chosen];
            let next = &mut next_layers[0];
            for removed_capacity in 0..=max_capacity_removed - capacity_removed {
                let blocked_teammates = current[removed_capacity];
                if blocked_teammates < 0 {
                    continue;
                }
                let next_removed_capacity = removed_capacity + capacity_removed;
                let next_blocked_teammates = blocked_teammates + count as isize;
                if next_blocked_teammates > next[next_removed_capacity] {
                    next[next_removed_capacity] = next_blocked_teammates;
                }
            }
        }
    }

    let mut worst = DominatorCoverSummary {
        dominators,
        blocked_teammates: 0,
        other_team_capacity: total_other_team_capacity.min(other_team_slot_count),
        free_replacements: dominators
            .saturating_sub(total_other_team_capacity.min(other_team_slot_count)),
    };
    for chosen_states in &dp {
        for (removed_capacity, &blocked_teammates) in chosen_states.iter().enumerate() {
            if blocked_teammates < 0 {
                continue;
            }
            let blocked_teammates = blocked_teammates as usize;
            let remaining_dominators = dominators.saturating_sub(blocked_teammates);
            let other_team_capacity = total_other_team_capacity
                .saturating_sub(removed_capacity)
                .min(other_team_slot_count);
            let free_replacements = remaining_dominators.saturating_sub(other_team_capacity);
            if free_replacements < worst.free_replacements
                || (free_replacements == worst.free_replacements
                    && blocked_teammates > worst.blocked_teammates)
            {
                worst = DominatorCoverSummary {
                    dominators,
                    blocked_teammates,
                    other_team_capacity,
                    free_replacements,
                };
            }
        }
    }

    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominance_graph_transitive_closure_adds_indirect_edges() {
        let mut graph = DominanceGraph::new(4);
        graph.add_edge(0, 1);
        graph.add_edge(1, 2);
        graph.add_edge(2, 3);
        graph.add_edge(3, 3);

        graph.transitive_closure();

        assert_eq!(graph.incoming(0), &[]);
        assert_eq!(graph.incoming(1), &[0]);
        assert_eq!(graph.incoming(2), &[0, 1]);
        assert_eq!(graph.incoming(3), &[0, 1, 2]);
    }

    #[test]
    fn subset_dominance_graph_closes_over_original_indices() {
        let graph = dominance_graph_for_index_subset(7, &[1, 3, 5], |left, right| {
            matches!((left, right), (1, 3) | (3, 5))
        });

        assert_eq!(graph.incoming(0), &[]);
        assert_eq!(graph.incoming(1), &[]);
        assert_eq!(graph.incoming(3), &[1]);
        assert_eq!(graph.incoming(5), &[1, 3]);
        assert_eq!(graph.incoming(6), &[]);
    }

    #[test]
    fn subset_dominance_graph_uses_base_edges() {
        let mut base = DominanceGraph::new(7);
        base.add_edge(1, 3);
        base.transitive_closure();

        let graph =
            dominance_graph_for_index_subset_with_base(7, &[1, 3, 5], &base, |left, right| {
                matches!((left, right), (3, 5))
            });

        assert_eq!(graph.incoming(1), &[]);
        assert_eq!(graph.incoming(3), &[1]);
        assert_eq!(graph.incoming(5), &[1, 3]);
    }

    #[test]
    fn cover_does_not_block_target_group_bucket() {
        let mut counts = BTreeMap::new();
        counts.insert(99, 5);
        counts.insert(1, 4);
        counts.insert(2, 3);
        counts.insert(3, 2);
        counts.insert(4, 1);
        counts.insert(5, 1);

        let summary = dominator_cover_summary_after_worst_teammate_groups(&counts, 99, 5, 3);

        assert_eq!(summary.dominators, 16);
        assert_eq!(summary.blocked_teammates, 10);
        assert_eq!(summary.other_team_capacity, 3);
        assert_eq!(summary.free_replacements, 3);
    }

    #[test]
    fn optimized_cover_matches_bruteforce_for_small_groups() {
        let mut counts = BTreeMap::new();
        counts.insert(99, 2);
        counts.insert(1, 4);
        counts.insert(2, 3);
        counts.insert(3, 2);
        counts.insert(4, 1);
        counts.insert(5, 5);

        let optimized = dominator_cover_summary_after_worst_teammate_groups(&counts, 99, 5, 3);
        let brute_force = brute_force_cover_summary(&counts, 99, 5, 3);

        assert_eq!(optimized, brute_force);
    }

    fn brute_force_cover_summary(
        counts_by_group: &BTreeMap<u32, usize>,
        target_group_id: u32,
        team_size: usize,
        team_count: usize,
    ) -> DominatorCoverSummary {
        let groups = counts_by_group
            .iter()
            .filter(|(group_id, _)| **group_id != target_group_id)
            .map(|(&group_id, &count)| (group_id, count))
            .collect::<Vec<_>>();
        let dominators = counts_by_group.values().sum();
        let other_team_count = team_count.saturating_sub(1);
        let other_team_slot_count = team_size * other_team_count;
        let total_other_team_capacity: usize = counts_by_group
            .values()
            .map(|&count| count.min(other_team_count))
            .sum();
        let mut worst = DominatorCoverSummary {
            dominators,
            blocked_teammates: 0,
            other_team_capacity: total_other_team_capacity.min(other_team_slot_count),
            free_replacements: dominators
                .saturating_sub(total_other_team_capacity.min(other_team_slot_count)),
        };

        for mask in 0..(1_usize << groups.len()) {
            if mask.count_ones() as usize > team_size.saturating_sub(1) {
                continue;
            }
            let mut blocked_teammates = 0;
            let mut removed_capacity = 0;
            for (group_idx, (_, count)) in groups.iter().enumerate() {
                if (mask & (1 << group_idx)) == 0 {
                    continue;
                }
                blocked_teammates += count;
                removed_capacity += count.min(&other_team_count);
            }
            let remaining_dominators = dominators.saturating_sub(blocked_teammates);
            let other_team_capacity = total_other_team_capacity
                .saturating_sub(removed_capacity)
                .min(other_team_slot_count);
            let free_replacements = remaining_dominators.saturating_sub(other_team_capacity);
            if free_replacements < worst.free_replacements
                || (free_replacements == worst.free_replacements
                    && blocked_teammates > worst.blocked_teammates)
            {
                worst = DominatorCoverSummary {
                    dominators,
                    blocked_teammates,
                    other_team_capacity,
                    free_replacements,
                };
            }
        }

        worst
    }

    #[test]
    fn same_group_cover_can_retain_by_required_dominator_count() {
        #[derive(Clone)]
        struct Item {
            group_id: u32,
            value: i32,
        }

        let items = vec![
            Item {
                group_id: 1,
                value: 10,
            },
            Item {
                group_id: 1,
                value: 20,
            },
            Item {
                group_id: 2,
                value: 30,
            },
        ];
        let retained = retain_by_same_group_dominator_cover(
            items,
            |item| item.group_id,
            |left, right| left.value >= right.value,
            1,
        );

        assert_eq!(retained.len(), 2);
        assert!(retained
            .iter()
            .any(|item| item.group_id == 1 && item.value == 20));
        assert!(retained
            .iter()
            .any(|item| item.group_id == 2 && item.value == 30));
    }

    #[test]
    fn graph_cover_helpers_count_same_and_cross_group_dominators() {
        #[derive(Clone)]
        struct Item {
            group_id: u32,
            value: i32,
        }

        let mut items = vec![Item {
            group_id: 99,
            value: 1,
        }];
        let mut value = 2;
        for (group_id, count) in [(99, 5), (1, 4), (2, 3), (3, 2), (4, 1), (5, 1)] {
            for _ in 0..count {
                items.push(Item { group_id, value });
                value += 1;
            }
        }
        let graph = dominance_graph_for_items(
            &items,
            |_, _| true,
            |_, left, _, right| left.value > right.value,
        );

        let same_cover = same_group_dominator_cover(&graph, 0, &items, |item| item.group_id, 3);
        let cross_cover =
            cross_group_dominator_cover(&graph, 0, &items, |item| item.group_id, 5, 3);

        assert_eq!(same_cover, 3);
        assert!(cross_cover > 0);
    }
}
