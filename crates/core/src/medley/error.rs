use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("candidate list is empty")]
    EmptyCandidates,

    #[error("event type {event_type} is not supported by team builder")]
    UnsupportedEventType { event_type: String },

    #[error("medley calculation requires exactly three songs, got {count}")]
    InvalidMedleySongCount { count: usize },

    #[error("candidate {candidate_id} has {actual} song scores, expected {expected}")]
    CandidateSongCountMismatch {
        candidate_id: usize,
        expected: usize,
        actual: usize,
    },

    #[error("medley solver failed: {0}")]
    MedleySolver(#[from] bangdream_optimize_medley_solver::MedleySolverError),
}
