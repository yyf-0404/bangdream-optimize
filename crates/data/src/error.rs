use thiserror::Error;

#[derive(Debug, Error)]
pub enum DataError {
    #[error("player {player_id} was not found")]
    PlayerNotFound { player_id: i64 },

    #[error("{kind} id {id} is missing")]
    MissingEntity { kind: &'static str, id: String },

    #[error("{field} is missing")]
    MissingField { field: &'static str },

    #[error("{field} has invalid value: {value}")]
    InvalidField { field: &'static str, value: String },

    #[error("current event is not set")]
    MissingCurrentEvent,

    #[error("event songs for event {event_id} are missing")]
    MissingEventSongs { event_id: u32 },

    #[error("storage error: {message}")]
    Storage { message: String },

    #[error("file {path} could not be read: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("file {path} contains invalid JSON: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("http request failed for {url}: {source}")]
    #[cfg(feature = "native-cache")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("http request for {url} returned status {status}")]
    #[cfg(feature = "native-cache")]
    HttpStatus { url: String, status: u16 },

    #[error("JSON serialization failed: {message}")]
    JsonString { message: String },

    #[error("chart error: {0}")]
    Chart(#[from] bangdream_optimize_core::ChartError),

    #[error("preparation error: {0}")]
    Preparation(#[from] bangdream_optimize_core::PreparationError),

    #[error("calculation error: {0}")]
    Maximize(#[from] bangdream_optimize_core::MaximizeError),

    #[error("score-range error: {0}")]
    ScoreRange(#[from] bangdream_optimize_core::ScoreRangeError),

    #[error("data preparation is not implemented yet")]
    NotImplemented,
}
