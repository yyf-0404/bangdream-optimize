//! Turn the reachable weak certificate relation into a strict order. Within
//! an equivalence class use a stable serial; between classes keep dominance.
use std::collections::{HashMap, HashSet};

pub(super) struct CertificateOrder {
    component: Vec<usize>,
    serial: Vec<(u32, usize)>,
    edges: HashSet<(usize, usize)>,
}

impl CertificateOrder {
    /// None keeps the normal divided-pruning path without a second pass.
    /// All certificates must compose on the same immutable stage domain.
    pub(super) fn new(
        serial: Vec<(u32, usize)>,
        pairs: &HashMap<(usize, usize), (bool, bool)>,
    ) -> Option<Self> {
        let n = serial.len();
        let mut outgoing = vec![Vec::new(); n];
        let mut incoming = vec![Vec::new(); n];
        for (&(a, b), &(ab, ba)) in pairs {
            for (from, to, present) in [(a, b, ab), (b, a, ba)] {
                if present {
                    outgoing[from].push(to);
                    incoming[to].push(from);
                }
            }
        }
        let mut degree: Vec<_> = incoming.iter().map(Vec::len).collect();
        let mut ready: Vec<_> = (0..n).filter(|&i| degree[i] == 0).collect();
        let mut visited = 0;
        while let Some(i) = ready.pop() {
            visited += 1;
            for &to in &outgoing[i] {
                degree[to] -= 1;
                if degree[to] == 0 {
                    ready.push(to);
                }
            }
        }
        if visited == n {
            return None;
        }

        // Iterative Kosaraju: a long certificate chain must not consume the
        // program stack. Only genuinely cyclic stages need these two walks.
        let mut seen = vec![false; n];
        let mut finish = Vec::with_capacity(n);
        for root in 0..n {
            if seen[root] {
                continue;
            }
            seen[root] = true;
            let mut stack = vec![(root, 0)];
            while let Some(&(v, next)) = stack.last() {
                if let Some(&to) = outgoing[v].get(next) {
                    stack.last_mut().unwrap().1 += 1;
                    if !seen[to] {
                        seen[to] = true;
                        stack.push((to, 0));
                    }
                } else {
                    finish.push(v);
                    stack.pop();
                }
            }
        }
        let mut component = vec![usize::MAX; n];
        let mut count = 0;
        for root in finish.into_iter().rev() {
            if component[root] != usize::MAX {
                continue;
            }
            component[root] = count;
            let mut stack = vec![root];
            while let Some(v) = stack.pop() {
                for &to in &incoming[v] {
                    if component[to] == usize::MAX {
                        component[to] = count;
                        stack.push(to);
                    }
                }
            }
            count += 1;
        }
        let mut edges = HashSet::new();
        for (from, targets) in outgoing.iter().enumerate() {
            for &to in targets {
                if component[from] != component[to] {
                    edges.insert((component[from], component[to]));
                }
            }
        }
        Some(Self {
            component,
            serial,
            edges,
        })
    }

    pub(super) fn pair(&self, a: usize, b: usize) -> (bool, bool) {
        if a == b {
            return (false, false);
        }
        let (ca, cb) = (self.component[a], self.component[b]);
        if ca == cb {
            // Mutual paths certify both directions, even if a direct pair's
            // sufficient tests only proved one. IDs never replace that proof.
            let ab = self.serial[a] < self.serial[b];
            (ab, !ab)
        } else {
            // Expand component edges to ALL endpoints. Otherwise reorienting
            // an equivalent intermediate could lose an external strict edge.
            (
                self.edges.contains(&(ca, cb)),
                self.edges.contains(&(cb, ca)),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closure(mut graph: Vec<Vec<bool>>) -> Vec<Vec<bool>> {
        for k in 0..graph.len() {
            for a in 0..graph.len() {
                for b in 0..graph.len() {
                    graph[a][b] |= graph[a][k] && graph[k][b];
                }
            }
        }
        graph
    }

    #[test]
    fn serials_break_indirect_equivalence_without_reversing_external_dominance() {
        let pairs = HashMap::from([
            ((0, 1), (true, false)),
            ((1, 2), (true, false)),
            ((0, 2), (false, true)),
            ((0, 3), (false, true)),
            ((1, 4), (true, false)),
        ]);
        let order = CertificateOrder::new(
            vec![(30, 0), (10, 1), (20, 2), (90, 3), (1, 4), (50, 5)],
            &pairs,
        )
        .unwrap();
        assert_eq!(order.pair(0, 1), (false, true));
        assert_eq!(order.pair(1, 2), (true, false));
        assert_eq!(order.pair(0, 2), (false, true));
        for i in 0..3 {
            // Higher ID can still dominate a different, strictly weaker class.
            assert_eq!(order.pair(3, i), (true, false));
            assert_eq!(order.pair(i, 4), (true, false));
            assert_eq!(order.pair(i, 5), (false, false));
            assert_eq!(order.pair(i, i), (false, false));
        }
    }

    #[test]
    fn every_four_node_certificate_graph_has_the_exact_serialized_preorder_closure() {
        let serial = vec![(30, 0), (10, 1), (20, 2), (10, 3)];
        // Exhaust all 2^12 directed graphs, including disconnected classes,
        // two-way ties, indirect cycles, and multiple equivalent components.
        for mask in 0..1usize << 12 {
            let mut raw = vec![vec![false; 4]; 4];
            let mut pairs = HashMap::new();
            let mut bit = 0;
            for a in 0..4 {
                for b in a + 1..4 {
                    raw[a][b] = mask & (1 << bit) != 0;
                    raw[b][a] = mask & (1 << (bit + 1)) != 0;
                    pairs.insert((a, b), (raw[a][b], raw[b][a]));
                    bit += 2;
                }
            }
            let mut normalized = raw.clone();
            if let Some(order) = CertificateOrder::new(serial.clone(), &pairs) {
                for a in 0..4 {
                    for b in 0..4 {
                        normalized[a][b] = order.pair(a, b).0;
                    }
                }
            }
            let before = closure(raw);
            let after = closure(normalized);
            for a in 0..4 {
                for b in 0..4 {
                    let expected =
                        a != b && before[a][b] && (!before[b][a] || serial[a] < serial[b]);
                    assert_eq!(after[a][b], expected, "mask={mask}, {a}->{b}");
                }
            }
        }
    }
}
