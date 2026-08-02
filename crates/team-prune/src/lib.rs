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

    pub fn extend_edges_from(&mut self, other: &Self) {
        debug_assert_eq!(
            self.incoming_by_target.len(),
            other.incoming_by_target.len()
        );
        for (target, incoming) in self
            .incoming_by_target
            .iter_mut()
            .zip(&other.incoming_by_target)
        {
            target.extend_from_slice(incoming);
            target.sort_unstable();
            target.dedup();
        }
    }

    pub fn retain_nodes(&mut self, retained_indices: &[usize]) {
        let mut retained = vec![false; self.incoming_by_target.len()];
        for &idx in retained_indices {
            retained[idx] = true;
        }
        for (target_idx, incoming) in self.incoming_by_target.iter_mut().enumerate() {
            if !retained[target_idx] {
                incoming.clear();
            } else {
                incoming.retain(|&dominator_idx| retained[dominator_idx]);
            }
        }
    }

    pub fn transitive_closure_for_subset(&mut self, indices: &[usize]) {
        if indices.is_empty() {
            return;
        }
        let node_count = self.incoming_by_target.len();
        let subset_count = indices.len();
        let word_count = subset_count.div_ceil(u64::BITS as usize);
        let mut index_to_pos = vec![usize::MAX; node_count];
        for (pos, &idx) in indices.iter().enumerate() {
            index_to_pos[idx] = pos;
        }
        let mut incoming_bits = vec![vec![0_u64; word_count]; subset_count];
        for (target_pos, &target_idx) in indices.iter().enumerate() {
            for &dominator_idx in &self.incoming_by_target[target_idx] {
                let dominator_pos = index_to_pos[dominator_idx];
                if dominator_pos != usize::MAX && dominator_pos != target_pos {
                    incoming_bits[target_pos][dominator_pos / u64::BITS as usize] |=
                        1_u64 << (dominator_pos % u64::BITS as usize);
                }
            }
        }
        close_incoming_bits(&mut incoming_bits, word_count);
        write_subset_incoming(self, indices, &mut incoming_bits);
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

fn close_incoming_bits(incoming_bits: &mut [Vec<u64>], word_count: usize) {
    for via_pos in 0..incoming_bits.len() {
        let via_bits = incoming_bits[via_pos].clone();
        for target_pos in 0..incoming_bits.len() {
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
}

fn write_subset_incoming(
    graph: &mut DominanceGraph,
    indices: &[usize],
    incoming_bits: &mut [Vec<u64>],
) {
    for (target_pos, (&target_idx, bits)) in indices.iter().zip(incoming_bits).enumerate() {
        bits[target_pos / u64::BITS as usize] &= !(1_u64 << (target_pos % u64::BITS as usize));
        graph.incoming_by_target[target_idx] = indices
            .iter()
            .enumerate()
            .filter_map(|(dominator_pos, &dominator_idx)| {
                ((bits[dominator_pos / u64::BITS as usize]
                    & (1_u64 << (dominator_pos % u64::BITS as usize)))
                    != 0)
                    .then_some(dominator_idx)
            })
            .collect();
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
    dominance_graph_for_index_subset_inner(node_count, indices, None, true, dominates)
}

pub fn direct_dominance_graph_for_index_subset<Dominates>(
    node_count: usize,
    indices: &[usize],
    dominates: Dominates,
) -> DominanceGraph
where
    Dominates: FnMut(usize, usize) -> bool,
{
    dominance_graph_for_index_subset_inner(node_count, indices, None, false, dominates)
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
    dominance_graph_for_index_subset_inner(node_count, indices, Some(base_graph), true, dominates)
}

pub fn direct_dominance_graph_for_index_subset_with_base<Dominates>(
    node_count: usize,
    indices: &[usize],
    base_graph: &DominanceGraph,
    dominates: Dominates,
) -> DominanceGraph
where
    Dominates: FnMut(usize, usize) -> bool,
{
    dominance_graph_for_index_subset_inner(node_count, indices, Some(base_graph), false, dominates)
}

pub fn direct_dominance_graph_for_cross_subsets<Dominates>(
    node_count: usize,
    left_indices: &[usize],
    right_indices: &[usize],
    mut dominates: Dominates,
) -> DominanceGraph
where
    Dominates: FnMut(usize, usize) -> bool,
{
    let mut graph = DominanceGraph::new(node_count);
    for &left_idx in left_indices {
        for &right_idx in right_indices {
            if dominates(left_idx, right_idx) {
                graph.add_edge(left_idx, right_idx);
            }
            if dominates(right_idx, left_idx) {
                graph.add_edge(right_idx, left_idx);
            }
        }
    }
    graph
}

fn dominance_graph_for_index_subset_inner<Dominates>(
    node_count: usize,
    indices: &[usize],
    base_graph: Option<&DominanceGraph>,
    close_transitively: bool,
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

    if close_transitively {
        close_incoming_bits(&mut incoming_bits, word_count);
    }
    write_subset_incoming(&mut graph, indices, &mut incoming_bits);

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

    #[test]
    fn optimized_cover_matches_bruteforce_for_all_small_count_vectors() {
        for team_size in 2..=5 {
            for team_count in 1..=3 {
                // Target group plus five possible teammate/dominator groups. Base four covers
                // counts 0..=3, including absent groups and counts above other-team capacity.
                for encoded in 0usize..4usize.pow(6) {
                    let mut value = encoded;
                    let mut counts = BTreeMap::new();
                    for group_id in 0..6u32 {
                        let count = value % 4;
                        value /= 4;
                        if count > 0 {
                            counts.insert(group_id, count);
                        }
                    }

                    let optimized = dominator_cover_summary_after_worst_teammate_groups(
                        &counts, 0, team_size, team_count,
                    );
                    let brute_force = brute_force_cover_summary(&counts, 0, team_size, team_count);
                    assert_eq!(
                        optimized, brute_force,
                        "counts={counts:?} team_size={team_size} team_count={team_count}"
                    );
                }
            }
        }
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

    #[test]
    fn direct_subgraphs_can_be_merged_before_subset_closure() {
        let left = direct_dominance_graph_for_index_subset(4, &[0, 1], |left, right| {
            left == 0 && right == 1
        });
        let right = direct_dominance_graph_for_index_subset(4, &[2, 3], |left, right| {
            left == 2 && right == 3
        });
        let cross = direct_dominance_graph_for_cross_subsets(4, &[0, 1], &[2, 3], |left, right| {
            left == 1 && right == 2
        });

        let mut merged = left;
        merged.extend_edges_from(&right);
        merged.extend_edges_from(&cross);
        assert_eq!(merged.incoming(3), &[2]);

        merged.transitive_closure_for_subset(&[0, 1, 2, 3]);
        assert_eq!(merged.incoming(3), &[0, 1, 2]);

        merged.retain_nodes(&[0, 2, 3]);
        assert_eq!(merged.incoming(3), &[0, 2]);
        assert!(merged.incoming(1).is_empty());
    }
}
