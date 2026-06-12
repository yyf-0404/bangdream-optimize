#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FrontierInsertResult {
    pub(crate) inserted: bool,
    pub(crate) removed_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SingleGroupTraceStats {
    pub(crate) group_idx: usize,
    pub(crate) group_count: usize,
    pub(crate) card_count: usize,
    pub(crate) old_states: usize,
    pub(crate) next_states: usize,
    pub(crate) group_ms: f64,
    pub(crate) insert_attempts: usize,
    pub(crate) feasibility_prunes: usize,
    pub(crate) upper_bound_prunes: usize,
    pub(crate) completed_states: usize,
    pub(crate) incumbent_updates: usize,
    pub(crate) frontier_inserted: usize,
    pub(crate) frontier_rejected: usize,
    pub(crate) frontier_removed: usize,
}

impl SingleGroupTraceStats {
    pub(crate) fn record_frontier_insert(&mut self, result: FrontierInsertResult) {
        if result.inserted {
            self.frontier_inserted += 1;
            self.frontier_removed += result.removed_count;
        } else {
            self.frontier_rejected += 1;
        }
    }
}

pub(crate) fn trace_single_group_stats(stats: &SingleGroupTraceStats) {
    eprintln!(
        "single dp group {}/{} cards={} old_states={} next_states={} group_ms={:.3} insert_attempts={} feasibility_prunes={} upper_bound_prunes={} completed={} incumbent_updates={} frontier_inserted={} frontier_rejected={} frontier_removed={}",
        stats.group_idx + 1,
        stats.group_count,
        stats.card_count,
        stats.old_states,
        stats.next_states,
        stats.group_ms,
        stats.insert_attempts,
        stats.feasibility_prunes,
        stats.upper_bound_prunes,
        stats.completed_states,
        stats.incumbent_updates,
        stats.frontier_inserted,
        stats.frontier_rejected,
        stats.frontier_removed,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_frontier_insert_results() {
        let mut stats = SingleGroupTraceStats::default();

        stats.record_frontier_insert(FrontierInsertResult {
            inserted: true,
            removed_count: 2,
        });
        stats.record_frontier_insert(FrontierInsertResult {
            inserted: false,
            removed_count: 0,
        });

        assert_eq!(stats.frontier_inserted, 1);
        assert_eq!(stats.frontier_rejected, 1);
        assert_eq!(stats.frontier_removed, 2);
    }
}
