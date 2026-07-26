pub mod bestdori;
#[cfg(feature = "native-cache")]
pub mod cache;
pub mod chart;
pub mod error;
pub mod event;
pub mod filesystem;
pub mod maximize;
pub mod preparation;
pub mod pt_maximize;
pub mod score_range;
pub mod snapshot;
pub mod traits;
pub(crate) mod utils;

pub use bestdori::BestdoriData;
#[cfg(feature = "native-cache")]
pub use cache::{BestdoriCachedFilesystemCalculationInputBuilder, BestdoriStaticMirrorConfig};
pub use chart::chart_from_bestdori;
pub use error::DataError;
pub use event::event_bonus;
pub use filesystem::{
    update_all_score_range_chart_meta, update_published_score_range_chart_meta,
    update_score_range_chart_meta, BestdoriFilesystemCalculationInputBuilder,
    BestdoriFilesystemConfig,
};
pub use maximize::{SnapshotCalculationInputBuilder, SnapshotMaximizeInputBuilder};
pub use preparation::{
    event_songs, initialized_charts, prepare_event_context, PreparedEventContext,
};
pub use pt_maximize::SnapshotPtMaximizeInputBuilder;
pub use score_range::{
    is_score_range_song_available, prepare_score_range_input,
    published_score_range_song_selections, PreparedScoreRangeInput, SnapshotScoreRangeInputBuilder,
};
pub use snapshot::{CalculationDataSnapshot, EventCalculationData, EventData, GameDataSnapshot};
pub use traits::{
    CalculationInputBuilder, MaximizeInputBuilder, PlayerConfigRepository, PlayerConfigStore,
    PtMaximizeInputBuilder, ScoreRangeInputBuilder,
};

pub type BestdoriFilesystemCalculator = BestdoriFilesystemCalculationInputBuilder;
#[cfg(feature = "native-cache")]
pub type BestdoriCachedFilesystemCalculator = BestdoriCachedFilesystemCalculationInputBuilder;
