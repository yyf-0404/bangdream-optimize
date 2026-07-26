pub mod event_pt;
pub mod maximize;
pub mod medley;
pub mod model;
pub mod pt_maximize;
pub mod score_range;
pub mod single;
pub(crate) mod team_prune;
mod timing;

/// Backward-compatible imports for callers migrating from the old module name.
pub mod calculation {
    pub use crate::maximize::*;
}

pub mod candidate {
    pub use crate::medley::candidate::*;
}

pub mod chart {
    pub use crate::model::chart::*;
}

pub mod dp_model {
    pub use crate::model::dp::*;
}

pub mod error {
    pub use crate::medley::error::*;
}

pub mod preparation {
    pub use crate::model::preparation::*;
}

pub mod schema {
    pub use crate::model::schema::*;
}

pub mod team {
    pub use crate::medley::team::*;
}

/// Floors the sum of unrounded card-stat contributions exactly once.
pub fn floor_team_stat(stats: impl IntoIterator<Item = f64>) -> i32 {
    stats
        .into_iter()
        .sum::<f64>()
        .floor()
        .clamp(i32::MIN as f64, i32::MAX as f64) as i32
}
pub use event_pt::{
    apply_point_bonus as apply_event_point_bonus, challenge_cp_gain, challenge_cp_points,
    cooperative_points, festival_multiplayer_points, medley_three_song_points, solo_points,
    versus_multiplayer_points, EventPtError,
};
pub use maximize::{
    calculate_best_result_for_items as maximize_result_for_items, mode_candidates,
    CalculationError as MaximizeError, ItemSearchOptions as MaximizeOptions,
};
pub use maximize::{
    calculate_best_result_for_items, CalculationError, ItemSearchOptions, PreferredItemTarget,
};
pub use medley::candidate::{calculate_from_candidates, CandidateBuildRequest, TeamCandidate};
pub use medley::error::BuildError;
pub use medley::team::{build_team_candidates, TeamBuildError, TeamGenerationOptions};
pub use model::chart::{
    AutoMultiplierGroup, Chart, ChartError, ChartNode, ChartNodeType, ComboMode,
    CompressedAutoScore, MaxMetaOrder, MaxScoreOrder, ScoreRule, SimultaneousSkillOrder,
    TeamCardSkill,
};
pub use model::dp::{floor_score, DpChartModel, DpModelError, ModelTerm, SongMode};
pub use model::preparation::{
    area_item_combinations, calculate_area_item_percent, event_point_bonus_percent, prepare_card,
    prepare_cards, AreaItemDefinition, AreaItemPercent, AreaItemType, CardDefinition,
    EventAttributeBonus, EventBonus, EventCharacterBonus, EventMemberBonus, PreparationError,
    PreparedCard, ScoreUp, SkillDefinition, StatValue,
};
pub use model::schema::*;
pub use pt_maximize::{
    evaluate_cooperative_team, evaluate_full_team, event_bonus_application, search_medley,
    search_medley_with_metrics, search_single_song, search_single_song_with_metrics,
    search_team_for_mode, supports_live_variant, AveragePt, CaptainScoreDistribution,
    CooperativeInput, CooperativeLeaderSelection, CooperativePtScenario, CooperativeTeammate,
    EventBonusApplication, FestivalInput, FixedTeamPtEvaluation, FixedTeamPtScenario, LiveVariant,
    PtMaximizeError, PtMaximizeMedleyMetrics, PtMaximizeMedleyResult, PtMaximizeMedleyTeamResult,
    PtMaximizeMetrics, PtMaximizeRequest, PtMaximizeResult, PtMaximizeScenarioSummary,
    PtMaximizeSearchScenario, PtMaximizeSingleMetrics, PtMaximizeTeamResult, ScoreHistogram,
    TeammateInput, VersusInput,
};
pub use score_range::{
    auto_base_multiplier, bucket_teams_by_skill, enumerate_score_range_teams, points_for_score,
    points_for_score_with_support, prepare_score_range_team_domain, score_interval_for_points,
    score_interval_for_points_with_support, score_range_item_combinations, search_score_range,
    ScoreInterval, ScoreRangeChartMeta, ScoreRangeChartMetaFile, ScoreRangeDurationTemplate,
    ScoreRangeError, ScoreRangePlay, ScoreRangePtError, ScoreRangeRequest, ScoreRangeResult,
    ScoreRangeSong, ScoreRangeTeam, ScoreRangeTeamDomain, SkillBucketKey, SongKey,
    FIRE_MULTIPLIERS, SCORE_RANGE_CHART_META_PATH, SCORE_RANGE_CHART_META_SCHEMA_VERSION,
    SCORE_RANGE_SKILL_DURATIONS_MILLIS,
};
pub use single::{calculate_single_song, SingleSongError, SingleSongResult};
