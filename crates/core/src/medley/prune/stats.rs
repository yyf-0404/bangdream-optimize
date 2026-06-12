use super::signature::{signature_label, MedleyPruneSignature};

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::medley) struct MedleyPruneTrace {
    pub(in crate::medley) context_ms: f64,
    pub(in crate::medley) upper_bounds_init_ms: f64,
    pub(in crate::medley) signatures_ms: f64,
    pub(in crate::medley) active_indices_ms: f64,
    pub(in crate::medley) hard_graph_ms: f64,
    pub(in crate::medley) hard_cover_ms: f64,
    pub(in crate::medley) contribution_context_ms: f64,
    pub(in crate::medley) contribution_graph_ms: f64,
    pub(in crate::medley) contribution_cover_ms: f64,
    pub(in crate::medley) upper_bound_ms: f64,
    pub(in crate::medley) completion_ms: f64,
    pub(in crate::medley) capacity_ms: f64,
    pub(in crate::medley) signature_count: usize,
    pub(in crate::medley) contribution_graph_count: usize,
}

impl MedleyPruneTrace {
    pub(in crate::medley) fn add(&mut self, other: &Self) {
        self.context_ms += other.context_ms;
        self.upper_bounds_init_ms += other.upper_bounds_init_ms;
        self.signatures_ms += other.signatures_ms;
        self.active_indices_ms += other.active_indices_ms;
        self.hard_graph_ms += other.hard_graph_ms;
        self.hard_cover_ms += other.hard_cover_ms;
        self.contribution_context_ms += other.contribution_context_ms;
        self.contribution_graph_ms += other.contribution_graph_ms;
        self.contribution_cover_ms += other.contribution_cover_ms;
        self.upper_bound_ms += other.upper_bound_ms;
        self.completion_ms += other.completion_ms;
        self.capacity_ms += other.capacity_ms;
        self.signature_count += other.signature_count;
        self.contribution_graph_count += other.contribution_graph_count;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::medley) struct DominanceRejectionCounts {
    pub(in crate::medley) stat: usize,
    pub(in crate::medley) unification: usize,
    pub(in crate::medley) meta: usize,
    pub(in crate::medley) tie: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::medley) struct SignaturePoolStats {
    pub(in crate::medley) signature: Option<MedleyPruneSignature>,
    pub(in crate::medley) allowed_count: usize,
    pub(in crate::medley) active_count: usize,
    pub(in crate::medley) same_character_pruned: usize,
    pub(in crate::medley) cross_character_pruned: usize,
    pub(in crate::medley) score_contribution_same_pruned: usize,
    pub(in crate::medley) score_contribution_cross_pruned: usize,
    pub(in crate::medley) upper_bound_pruned: usize,
    pub(in crate::medley) max_same_character_cover: usize,
    pub(in crate::medley) max_cross_character_cover: usize,
    pub(in crate::medley) max_score_contribution_same_cover: usize,
    pub(in crate::medley) max_score_contribution_cross_cover: usize,
    pub(in crate::medley) estimated_candidates: usize,
    pub(in crate::medley) trace: MedleyPruneTrace,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::medley) struct MedleyCardPruneStats {
    pub(in crate::medley) raw_count: usize,
    pub(in crate::medley) same_character_pruned: usize,
    pub(in crate::medley) cross_character_pruned: usize,
    pub(in crate::medley) score_contribution_same_pruned: usize,
    pub(in crate::medley) score_contribution_cross_pruned: usize,
    pub(in crate::medley) upper_bound_context_pruned: usize,
    pub(in crate::medley) active_count: usize,
    pub(in crate::medley) character_count: usize,
    pub(in crate::medley) characters_with_four_or_more_cards: usize,
    pub(in crate::medley) unified_band_count: usize,
    pub(in crate::medley) unified_attribute_count: usize,
    pub(in crate::medley) unified_band_attribute_count: usize,
    pub(in crate::medley) obligation_count: usize,
    pub(in crate::medley) max_obligations_per_card: usize,
    pub(in crate::medley) max_same_character_cover: usize,
    pub(in crate::medley) max_cross_character_cover: usize,
    pub(in crate::medley) max_score_contribution_same_cover: usize,
    pub(in crate::medley) max_score_contribution_cross_cover: usize,
    pub(in crate::medley) max_cross_character_cover_ignoring_unification: usize,
    pub(in crate::medley) cross_prunable_ignoring_unification: usize,
    pub(in crate::medley) rejection_counts: DominanceRejectionCounts,
}

impl MedleyCardPruneStats {
    fn after_same_character_prune(self) -> usize {
        self.raw_count.saturating_sub(self.same_character_pruned)
    }

    fn stat_rejects(self) -> usize {
        self.rejection_counts.stat
    }

    fn unification_rejects(self) -> usize {
        self.rejection_counts.unification
    }

    fn meta_rejects(self) -> usize {
        self.rejection_counts.meta
    }

    fn tie_rejects(self) -> usize {
        self.rejection_counts.tie
    }
}

pub(in crate::medley) fn trace_medley_prune_stats(label: &str, prune_stats: &MedleyCardPruneStats) {
    eprintln!(
        "{label}: raw_cards={} active_cards={} same_pruned={} cross_pruned={} score_contribution_same_pruned={} score_contribution_cross_pruned={} bound_pruned={} after_same_prune={} character_count={} characters_with_4plus={} unified_bands={} unified_attributes={} unified_band_attributes={} obligations={} max_obligations={} max_same_cover={} max_cross_cover={} max_score_contribution_same_cover={} max_score_contribution_cross_cover={} max_cross_cover_no_unification={} cross_prunable_no_unification={} stat_rejects={} unification_rejects={} meta_rejects={} tie_rejects={}",
        prune_stats.raw_count,
        prune_stats.active_count,
        prune_stats.same_character_pruned,
        prune_stats.cross_character_pruned,
        prune_stats.score_contribution_same_pruned,
        prune_stats.score_contribution_cross_pruned,
        prune_stats.upper_bound_context_pruned,
        prune_stats.after_same_character_prune(),
        prune_stats.character_count,
        prune_stats.characters_with_four_or_more_cards,
        prune_stats.unified_band_count,
        prune_stats.unified_attribute_count,
        prune_stats.unified_band_attribute_count,
        prune_stats.obligation_count,
        prune_stats.max_obligations_per_card,
        prune_stats.max_same_character_cover,
        prune_stats.max_cross_character_cover,
        prune_stats.max_score_contribution_same_cover,
        prune_stats.max_score_contribution_cross_cover,
        prune_stats.max_cross_character_cover_ignoring_unification,
        prune_stats.cross_prunable_ignoring_unification,
        prune_stats.stat_rejects(),
        prune_stats.unification_rejects(),
        prune_stats.meta_rejects(),
        prune_stats.tie_rejects(),
    );
}

pub(in crate::medley) fn trace_signature_pool_stats(stats: &SignaturePoolStats) {
    let signature = stats
        .signature
        .map(signature_label)
        .unwrap_or_else(|| "unknown".to_owned());
    eprintln!(
        "medley signature pool: signature={} allowed={} active={} same_pruned={} cross_pruned={} score_contribution_same_pruned={} score_contribution_cross_pruned={} bound_pruned={} max_same_cover={} max_cross_cover={} max_score_contribution_same_cover={} max_score_contribution_cross_cover={} estimated_candidates={}",
        signature,
        stats.allowed_count,
        stats.active_count,
        stats.same_character_pruned,
        stats.cross_character_pruned,
        stats.score_contribution_same_pruned,
        stats.score_contribution_cross_pruned,
        stats.upper_bound_pruned,
        stats.max_same_character_cover,
        stats.max_cross_character_cover,
        stats.max_score_contribution_same_cover,
        stats.max_score_contribution_cross_cover,
        stats.estimated_candidates,
    );
}
