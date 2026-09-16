//! Optional second-team rejection certificates. No candidate bitmap is built.
//! A rejection is sufficient to prove that the current score prefix has no
//! third team disjoint from both selected teams; an inconclusive query leaves
//! the existing AVX2 traversal untouched.
use super::BandScore;

#[derive(Clone, Copy)]
pub(super) enum Masks<'a> {
    Narrow(&'a [u64]),
    Wide(&'a [Vec<u64>]),
}

impl<'a> Masks<'a> {
    fn len(self) -> usize {
        match self {
            Self::Narrow(masks) => masks.len(),
            Self::Wide(masks) => masks.len(),
        }
    }

    fn get(self, index: usize) -> &'a [u64] {
        match self {
            Self::Narrow(masks) => std::slice::from_ref(&masks[index]),
            Self::Wide(masks) => &masks[index],
        }
    }

    fn overlap(self, left: usize, right: usize) -> bool {
        self.get(left)
            .iter()
            .zip(self.get(right))
            .any(|(a, b)| a & b != 0)
    }
}

// 0: baseline; 1: compatible bound; 2: bound + common cards; 3: prefix reuse.
#[cfg(feature = "experimental-avx2-second-prune")]
pub(super) fn mode() -> u8 {
    match std::env::var("BANGDREAM_OPTIMIZE_AVX2_SECOND_PRUNE").as_deref() {
        Ok("off") => 0,
        Ok("bound") => 1,
        Ok("common") => 2,
        _ => 3,
    }
}

#[derive(Default)]
struct Stats {
    queries: u64,
    bound_rejects: u64,
    common_rejects: u64,
    bound_cache_misses: u64,
    bound_positions: u64,
    summaries: u64,
    summary_positions: u64,
    skipped_positions: u64,
}

pub(super) struct Gate<'a, const MODE: u8> {
    masks: Masks<'a>,
    order: &'a [usize],
    scores: &'a [BandScore],
    // len + 1 = unknown; len = no compatible candidate.
    best_positions: Vec<usize>,
    current_i: Option<usize>,
    summary_ready: bool,
    summary_prefix: BandScore,
    summary_floor: BandScore,
    summary_end: usize,
    change_positions: Vec<usize>,
    common_masks: Vec<u64>,
    common: Vec<u64>,
    stats: Stats,
    trace: bool,
}

impl<'a, const MODE: u8> Gate<'a, MODE> {
    pub(super) fn new(masks: Masks<'a>, order: &'a [usize], scores: &'a [BandScore]) -> Self {
        Self {
            masks,
            order,
            scores,
            // Per-song pruning shortens order but keeps original candidate IDs.
            best_positions: vec![order.len() + 1; masks.len()],
            current_i: None,
            summary_ready: false,
            summary_prefix: 0,
            summary_floor: 0,
            summary_end: 0,
            change_positions: Vec::new(),
            common_masks: Vec::new(),
            common: vec![0; masks.get(0).len()],
            stats: Stats::default(),
            trace: std::env::var_os("BANGDREAM_OPTIMIZE_SECOND_PRUNE_TRACE").is_some(),
        }
    }

    fn best_position(&mut self, team: usize) -> usize {
        let cached = self.best_positions[team];
        if cached <= self.order.len() {
            return cached;
        }
        self.stats.bound_cache_misses += 1;
        let mut result = self.order.len();
        for (position, &candidate) in self.order.iter().enumerate() {
            self.stats.bound_positions += 1;
            if !self.masks.overlap(team, candidate) {
                result = position;
                break;
            }
        }
        self.best_positions[team] = result;
        result
    }

    fn build_summary(&mut self, i: usize, first: usize, prefix: BandScore, floor: BandScore) {
        self.change_positions.clear();
        self.common_masks.clear();
        self.common
            .copy_from_slice(self.masks.get(self.order[first]));
        self.change_positions.push(first);
        self.common_masks.extend_from_slice(&self.common);
        self.summary_prefix = prefix;
        self.summary_floor = floor;
        self.summary_end = self.order.len();
        self.summary_ready = true;
        self.stats.summaries += 1;
        if self.common.iter().all(|&word| word == 0) {
            return;
        }
        for position in first + 1..self.order.len() {
            if prefix.saturating_add(self.scores[position]) < floor {
                self.summary_end = position;
                break;
            }
            self.stats.summary_positions += 1;
            let candidate = self.order[position];
            if self.masks.overlap(i, candidate) {
                continue;
            }
            let mut changed = false;
            for (word, &next) in self.common.iter_mut().zip(self.masks.get(candidate)) {
                let intersection = *word & next;
                changed |= intersection != *word;
                *word = intersection;
            }
            if changed {
                self.change_positions.push(position);
                self.common_masks.extend_from_slice(&self.common);
                if self.common.iter().all(|&word| word == 0) {
                    break;
                }
            }
        }
    }

    /// Calls for each fixed i follow descending second-song score, and the
    /// caller's floor only rises. Thus the queried third-score prefix can only
    /// shrink, making a summary of the first prefix valid for later queries.
    pub(super) fn rejected_prefix_limit(
        &mut self,
        i: usize,
        j: usize,
        prefix: BandScore,
        floor: BandScore,
    ) -> Option<usize> {
        self.stats.queries += 1;
        let first_i = self.best_position(i);
        let first_j = self.best_position(j);
        let bound_position = first_i.max(first_j);
        if bound_position == self.order.len()
            || prefix.saturating_add(self.scores[bound_position]) < floor
        {
            self.stats.bound_rejects += 1;
            // This position is outside the eligible score prefix. Reuse that
            // fact when counting skipped positions, rather than binary-searching
            // the entire candidate list for every rejected pair.
            return Some(bound_position);
        }
        if MODE == 1 {
            return None;
        }
        if self.current_i != Some(i) {
            self.current_i = Some(i);
            self.summary_ready = false;
        }
        if !self.summary_ready {
            self.build_summary(i, first_i, prefix, floor);
        }
        debug_assert!(prefix <= self.summary_prefix && floor >= self.summary_floor);
        let words = self.common.len();
        for (step, &position) in self.change_positions.iter().enumerate().rev() {
            if prefix.saturating_add(self.scores[position]) < floor {
                continue;
            }
            let common = &self.common_masks[step * words..(step + 1) * words];
            if common
                .iter()
                .zip(self.masks.get(j))
                .any(|(a, b)| a & b != 0)
            {
                self.stats.common_rejects += 1;
                return Some(
                    self.change_positions
                        .get(step + 1)
                        .copied()
                        .unwrap_or(self.summary_end),
                );
            }
            return None;
        }
        unreachable!("the compatible-score bound admitted the first compatible third team")
    }

    pub(super) fn skipped_positions(&mut self, count: usize) {
        self.stats.skipped_positions += count as u64;
    }
}

impl<const MODE: u8> Drop for Gate<'_, MODE> {
    fn drop(&mut self) {
        if self.trace {
            eprintln!(
                "AVX2 second prune: mode={} third_candidates={} queries={} bound_rejects={} common_rejects={} bound_cache_misses={} bound_positions={} summaries={} summary_positions={} skipped_positions={}",
                MODE, self.order.len(), self.stats.queries, self.stats.bound_rejects,
                self.stats.common_rejects, self.stats.bound_cache_misses,
                self.stats.bound_positions, self.stats.summaries,
                self.stats.summary_positions, self.stats.skipped_positions,
            );
        }
    }
}
