use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Server {
    Jp,
    En,
    Tw,
    Cn,
    Kr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    Medley,
    Versus,
    Challenge,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Medley => "medley",
            Self::Versus => "versus",
            Self::Challenge => "challenge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSelection {
    #[serde(deserialize_with = "deserialize_u32_from_number_or_string")]
    pub song_id: u32,
    #[serde(deserialize_with = "deserialize_u8_from_number_or_string")]
    pub difficulty: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Attribute {
    Cool,
    Happy,
    Pure,
    Powerful,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Magazine {
    Performance,
    Technique,
    Visual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedAreaItems {
    pub band: String,
    pub attribute: String,
    pub magazine: Magazine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub performance: i32,
    pub technique: i32,
    pub visual: i32,
}

impl Stat {
    pub fn sum(self) -> i32 {
        self.performance + self.technique + self.visual
    }
}

impl Magazine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Technique => "technique",
            Self::Visual => "visual",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "performance" => Some(Self::Performance),
            "technique" => Some(Self::Technique),
            "visual" => Some(Self::Visual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerCardConfig {
    #[serde(default, deserialize_with = "deserialize_u8_from_number_or_string")]
    pub level: u8,
    #[serde(default = "default_true")]
    pub training: bool,
    #[serde(default = "default_true")]
    pub illust_training_status: bool,
    #[serde(default = "default_episodes")]
    pub episodes: [bool; 2],
    #[serde(deserialize_with = "deserialize_u8_from_number_or_string")]
    pub limit_break_rank: u8,
    #[serde(deserialize_with = "deserialize_u8_from_number_or_string")]
    pub skill_level: u8,
}

fn default_true() -> bool {
    true
}

fn default_episodes() -> [bool; 2] {
    [true, true]
}

impl PlayerCardConfig {
    pub fn level_is_auto_max(&self) -> bool {
        self.level == 0
    }
}

fn deserialize_u8_from_number_or_string<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(U8Visitor)
}

fn deserialize_u32_from_number_or_string<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(U32Visitor)
}

fn deserialize_i64_from_number_or_string<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(I64Visitor)
}

fn deserialize_f64_from_number_or_string<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(F64Visitor)
}

fn deserialize_optional_u32_from_number_or_string<'de, D>(
    deserializer: D,
) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_option(OptionalU32Visitor)
}

fn deserialize_optional_i64_from_number_or_string<'de, D>(
    deserializer: D,
) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_option(OptionalI64Visitor)
}

struct U8Visitor;

impl<'de> Visitor<'de> for U8Visitor {
    type Value = u8;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a u8 number or numeric string")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        u8::try_from(value).map_err(|_| E::custom(format!("value {value} is out of range for u8")))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        u8::try_from(value).map_err(|_| E::custom(format!("value {value} is out of range for u8")))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        value
            .parse::<u8>()
            .map_err(|err| E::custom(format!("invalid u8 string {value:?}: {err}")))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }
}

struct U32Visitor;

impl<'de> Visitor<'de> for U32Visitor {
    type Value = u32;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a u32 number or numeric string")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        u32::try_from(value)
            .map_err(|_| E::custom(format!("value {value} is out of range for u32")))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        u32::try_from(value)
            .map_err(|_| E::custom(format!("value {value} is out of range for u32")))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        value
            .parse::<u32>()
            .map_err(|err| E::custom(format!("invalid u32 string {value:?}: {err}")))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }
}

struct I64Visitor;

impl<'de> Visitor<'de> for I64Visitor {
    type Value = i64;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an i64 number or numeric string")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        i64::try_from(value)
            .map_err(|_| E::custom(format!("value {value} is out of range for i64")))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        value
            .parse::<i64>()
            .map_err(|err| E::custom(format!("invalid i64 string {value:?}: {err}")))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }
}

struct F64Visitor;

impl<'de> Visitor<'de> for F64Visitor {
    type Value = f64;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an f64 number or numeric string")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value as f64)
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value as f64)
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        value
            .parse::<f64>()
            .map_err(|err| E::custom(format!("invalid f64 string {value:?}: {err}")))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }
}

struct OptionalU32Visitor;

impl<'de> Visitor<'de> for OptionalU32Visitor {
    type Value = Option<u32>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("null, a u32 number, or a numeric string")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(None)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_u32_from_number_or_string(deserializer).map(Some)
    }
}

struct OptionalI64Visitor;

impl<'de> Visitor<'de> for OptionalI64Visitor {
    type Value = Option<i64>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("null, an i64 number, or a numeric string")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(None)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_i64_from_number_or_string(deserializer).map(Some)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaItemConfig {
    #[serde(deserialize_with = "deserialize_u8_from_number_or_string")]
    pub level: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterBonusConfig {
    pub potential: StatRate,
    pub character_task: StatRate,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatRate {
    #[serde(deserialize_with = "deserialize_f64_from_number_or_string")]
    pub performance: f64,
    #[serde(deserialize_with = "deserialize_f64_from_number_or_string")]
    pub technique: f64,
    #[serde(deserialize_with = "deserialize_f64_from_number_or_string")]
    pub visual: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerConfig {
    #[serde(
        default,
        rename = "_id",
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_i64_from_number_or_string"
    )]
    pub mongo_id: Option<i64>,
    #[serde(deserialize_with = "deserialize_i64_from_number_or_string")]
    pub player_id: i64,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_u32_from_number_or_string"
    )]
    pub current_event: Option<u32>,
    #[serde(default)]
    pub event_songs: BTreeMap<String, Vec<SongSelection>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub event_presets: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub event_overrides: BTreeMap<String, Value>,
    #[serde(default)]
    pub card_list: BTreeMap<String, PlayerCardConfig>,
    #[serde(default)]
    pub area_item: BTreeMap<String, AreaItemConfig>,
    #[serde(default)]
    pub character_bouns: BTreeMap<String, CharacterBonusConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildResult {
    pub event_id: u32,
    pub event_type: EventType,
    pub total_score: i32,
    pub total_stat: i32,
    pub songs: Vec<SongBuildResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<SelectedAreaItems>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<CalculationMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculationMetrics {
    pub core_version: String,
    pub card_count: usize,
    pub song_count: usize,
    pub item_combinations_before: usize,
    pub item_combinations_after: usize,
    pub total_elapsed_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medley: Option<MedleyCalculationMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single: Option<SingleCalculationMetrics>,
}

impl Default for CalculationMetrics {
    fn default() -> Self {
        Self {
            core_version: env!("CARGO_PKG_VERSION").to_owned(),
            card_count: 0,
            song_count: 0,
            item_combinations_before: 0,
            item_combinations_after: 0,
            total_elapsed_ms: 0.0,
            medley: None,
            single: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MedleyCalculationMetrics {
    pub candidate_count: usize,
    pub solver_candidate_count: usize,
    pub solver_filter_ms: f64,
    pub solver_ms: f64,
    #[serde(default)]
    pub seed_ms: f64,
    #[serde(default)]
    pub item_upper_bound_ms: f64,
    #[serde(default)]
    pub candidate_build_count: usize,
    #[serde(default)]
    pub solver_count: usize,
    #[serde(default)]
    pub seed_count: usize,
    #[serde(default)]
    pub item_upper_bound_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_build_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_card_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SingleCalculationMetrics {
    pub mode_count: usize,
    pub valid_mode_count: usize,
    pub solve_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongBuildResult {
    pub song_id: u32,
    pub difficulty: u8,
    pub score: i32,
    pub stat: i32,
    pub team_card_ids: Vec<u32>,
    pub captain_card_id: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_card_config_deserializes_legacy_mongodb_shape() {
        let config: PlayerCardConfig =
            serde_json::from_str(r#"{"limitBreakRank":2,"skillLevel":5}"#).unwrap();

        assert_eq!(config.level, 0);
        assert!(config.level_is_auto_max());
        assert!(config.training);
        assert!(config.illust_training_status);
        assert_eq!(config.episodes, [true, true]);
        assert_eq!(config.limit_break_rank, 2);
        assert_eq!(config.skill_level, 5);
    }

    #[test]
    fn player_card_config_accepts_legacy_illust_training_field() {
        let config: PlayerCardConfig = serde_json::from_str(
            r#"{"illustTrainingStatus":false,"limitBreakRank":0,"skillLevel":1}"#,
        )
        .unwrap();

        assert!(config.training);
        assert!(!config.illust_training_status);
        assert_eq!(config.episodes, [true, true]);
    }

    #[test]
    fn player_config_accepts_legacy_numeric_strings() {
        let config: PlayerConfig = serde_json::from_str(
            r#"{
                "_id":"123",
                "playerId":"123",
                "currentEvent":"100",
                "eventSongs":{"100":[{"songId":"287","difficulty":"3"}]},
                "cardList":{"1":{"level":"50","limitBreakRank":"2","skillLevel":"5"}},
                "areaItem":{"10":{"level":"7"}},
                "characterBouns":{
                    "1":{
                        "potential":{"performance":"0.1","technique":"0.2","visual":"0.3"},
                        "characterTask":{"performance":0,"technique":1,"visual":2}
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(config.mongo_id, Some(123));
        assert_eq!(config.player_id, 123);
        assert_eq!(config.current_event, Some(100));
        assert_eq!(config.event_songs["100"][0].song_id, 287);
        assert_eq!(config.event_songs["100"][0].difficulty, 3);
        assert_eq!(config.card_list["1"].level, 50);
        assert_eq!(config.card_list["1"].limit_break_rank, 2);
        assert_eq!(config.card_list["1"].skill_level, 5);
        assert_eq!(config.area_item["10"].level, 7);
        assert_eq!(config.character_bouns["1"].potential.performance, 0.1);
    }
}
