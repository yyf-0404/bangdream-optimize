//! Full-life, fixed-team skill resolution for the explicit 120 FPS replay.
//! Kept separate from the existing AP-only skill adapter used by live searches.
use super::{card, server_index, BestdoriData};
use crate::{utils::parse_attribute, DataError};
use bangdream_optimize_core::{
    hit_timing::{JudgementCorrection, ReplaySkill, ScoreEffect},
    Attribute, Server,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimingTeamCard {
    pub card_id: u32,
    pub skill_level: u8,
}

/// Team facts, not conditions copied from the card whose skill is being read.
#[derive(Debug, Clone, Copy)]
pub struct TimingSkillContext {
    pub server: Server,
    pub skill_level: u8,
    pub common_band: Option<u32>,
    pub common_attribute: Option<Attribute>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTimingSkill {
    pub skill_id: u32,
    pub skill: ReplaySkill,
    /// Records actual server selection for each effect, including zero values.
    pub value_sources: BTreeMap<String, Server>,
    pub unification_applied: bool,
    pub known_crescendo_parameters: bool,
    /// Contract: start at 1000 life, with no life loss (effective P/G only).
    pub requires_full_life: bool,
}

impl BestdoriData {
    /// Exact alternatives before a roster is known: bit 0 = common band,
    /// bit 1 = common attribute. Conditions use this card's actual identities.
    /// A search may union them for an upper bound, but must resolve the real
    /// five-member context again before evaluating an attainable score.
    pub fn hit_timing_skill_cases(
        &self,
        server: Server,
        member: TimingTeamCard,
    ) -> Result<[ResolvedTimingSkill; 4], DataError> {
        if let Some(definition) = self.custom_timing_cards.get(&member.card_id) {
            return (0..4).map(|case| custom_skill(definition, TimingSkillContext {
                server, skill_level: member.skill_level,
                common_band: (case & 1 != 0).then_some(definition.band_id),
                common_attribute: (case & 2 != 0).then_some(definition.attribute),
            })).collect::<Result<Vec<_>, _>>()
                .map(|v| v.try_into().expect("four cases"));
        }
        let card = self.entity(&self.cards, "card", member.card_id)?;
        let character = self.entity(&self.characters, "character", card::character_id(card)?)?;
        let band = crate::utils::get_u32(character, "bandId")?;
        let attribute = parse_attribute(crate::utils::get_str(card, "attribute")?)?;
        let id = card::skill_id(card)?;
        let source = self.entity(&self.skills, "skill", id)?;
        (0..4)
            .map(|case| {
                resolve_full_life_skill(
                    id,
                    source,
                    TimingSkillContext {
                        server,
                        skill_level: member.skill_level,
                        common_band: (case & 1 != 0).then_some(band),
                        common_attribute: (case & 2 != 0).then_some(attribute),
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.try_into().expect("four cases"))
    }

    /// Output remains in member order; the caller maps trigger slots to members.
    /// The sixth trigger may reuse the leader's already resolved definition.
    pub fn hit_timing_team_skills(
        &self,
        server: Server,
        members: &[TimingTeamCard; 5],
    ) -> Result<[ResolvedTimingSkill; 5], DataError> {
        let mut facts = Vec::with_capacity(5);
        for member in members {
            if let Some(definition) = self.custom_timing_cards.get(&member.card_id) {
                facts.push((definition.band_id, definition.attribute, member.card_id));
                continue;
            }
            let card = self.entity(&self.cards, "card", member.card_id)?;
            let character_id = card::character_id(card)?;
            let character = self.entity(&self.characters, "character", character_id)?;
            let band = crate::utils::get_u32(character, "bandId")?;
            let attribute = parse_attribute(crate::utils::get_str(card, "attribute")?)?;
            facts.push((band, attribute, card::skill_id(card)?));
        }
        let common_band = facts
            .iter()
            .all(|f| f.0 == facts[0].0)
            .then_some(facts[0].0);
        let common_attribute = facts
            .iter()
            .all(|f| f.1 == facts[0].1)
            .then_some(facts[0].1);
        let resolved = members
            .iter()
            .zip(&facts)
            .map(|(member, fact)| {
                if let Some(definition) = self.custom_timing_cards.get(&member.card_id) {
                    return custom_skill(definition, TimingSkillContext {
                        server, skill_level: member.skill_level, common_band, common_attribute,
                    });
                }
                resolve_full_life_skill(
                    fact.2,
                    self.entity(&self.skills, "skill", fact.2)?,
                    TimingSkillContext {
                        server,
                        skill_level: member.skill_level,
                        common_band,
                        common_attribute,
                    },
                )
            })
            .collect::<Result<Vec<_>, DataError>>()?;
        Ok(resolved.try_into().expect("exactly five input members"))
    }
}

/// Custom cards declare the same plain/crescendo and team-condition parameters
/// as the existing editor. Their definitions are validated before this adapter.
fn custom_skill(definition: &bangdream_optimize_core::CardDefinition, context: TimingSkillContext)
    -> Result<ResolvedTimingSkill, DataError>
{
    if !(1..=5).contains(&context.skill_level) {
        return Err(invalid("customCards.skillLevel", "skill level must be 1..=5"));
    }
    let source = &definition.skill;
    let rate = &source.score_up;
    let unified = rate.unification_activate_effect_value.is_some()
        && (rate.unification_activate_condition_band_id.is_some()
            || rate.unification_activate_condition_type.is_some())
        && rate.unification_activate_condition_band_id
            .is_none_or(|band| context.common_band == Some(band))
        && rate.unification_activate_condition_type
            .is_none_or(|attribute| context.common_attribute == Some(attribute));
    if source.rateup && rate.unification_activate_effect_value.is_some() {
        return Err(invalid("customCards.skill", "crescendo with unification is unsupported"));
    }
    let percent = (if unified { rate.unification_activate_effect_value.unwrap() }
        else { rate.default } * 100.0) as f32;
    let skill = ReplaySkill {
        duration: source.durations[usize::from(context.skill_level - 1)] as f32,
        score: if source.rateup { ScoreEffect::Crescendo { percent, step: 0.5, cap: 150 } }
            else { ScoreEffect::Plain { percent } },
        correction: None,
    };
    Ok(ResolvedTimingSkill {
        skill_id: definition.card_id, skill, value_sources: BTreeMap::new(),
        unification_applied: unified, known_crescendo_parameters: source.rateup,
        requires_full_life: false,
    })
}

/// Resolve only the no-life-loss replay scope, never an AP maximum over effects.
/// Missing server entries fall back JP -> CN -> EN -> TW -> KR; malformed
/// non-null entries fail instead of falling back. Duration uses level, not server.
pub fn resolve_full_life_skill(
    skill_id: u32,
    source: &Value,
    context: TimingSkillContext,
) -> Result<ResolvedTimingSkill, DataError> {
    let invalid = |reason: &str| invalid("hitTimingSkill", &format!("skill {skill_id}: {reason}"));
    if !(1..=5).contains(&context.skill_level) {
        return Err(invalid("skill level must be 1..=5"));
    }
    let duration = source
        .get("duration")
        .and_then(Value::as_array)
        .and_then(|v| v.get(usize::from(context.skill_level - 1)))
        .ok_or_else(|| invalid("missing duration at selected skill level"))?;
    let duration = finite_nonnegative(duration, "duration")?;
    if duration <= 0.0 || duration > 60.0 {
        return Err(invalid("invalid duration"));
    }
    let activation = source
        .get("activationEffect")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("missing activation effects"))?;
    check_fields(
        activation,
        &[
            "activateEffectTypes",
            "unificationActivateEffectValue",
            "unificationActivateConditionBandId",
            "unificationActivateConditionType",
        ],
    )?;
    let effects = activation
        .get("activateEffectTypes")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("missing effect map"))?;
    let mut values = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut correction = None;
    let mut requires_full_life = false;
    for (name, effect) in effects {
        let fields = effect
            .as_object()
            .ok_or_else(|| invalid("effect must be an object"))?;
        check_fields(
            fields,
            &[
                "activateEffectValue",
                "activateEffectValueType",
                "activateCondition",
                "activateConditionLife",
            ],
        )?;
        let (value, server) = server_value(effect, context.server)?;
        sources.insert(name.clone(), server);
        values.insert(name.as_str(), value);
        require_string(
            effect,
            "activateEffectValueType",
            if name == "judge" {
                "real_value"
            } else {
                "rate"
            },
        )?;
        match name.as_str() {
            "score" => {
                score_condition(effect)?;
            }
            "score_continued_note_judge"
            | "score_only_perfect"
            | "score_under_great_half"
            | "score_rate_up_with_perfect" => {
                require_string(effect, "activateCondition", "perfect")?;
            }
            "score_over_life" | "score_under_life" => {
                require_string(effect, "activateCondition", "good")?;
                let threshold = effect
                    .get("activateConditionLife")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("missing life threshold"))?;
                // All accepted judgments are P/G after correction, hence no life
                // loss. Recovery cannot invalidate these full-life predicates.
                if threshold == 0 || threshold > 1000 {
                    return Err(invalid("life threshold outside verified full-life scope"));
                }
                requires_full_life = true;
            }
            "judge" => {
                if value != 0.0 {
                    return Err(invalid("unknown judge effect value"));
                }
                correction = Some(
                    match effect.get("activateCondition").and_then(Value::as_str) {
                        Some("great") => JudgementCorrection::Great,
                        Some("good") => JudgementCorrection::Good,
                        Some("bad") => JudgementCorrection::Bad,
                        _ => return Err(invalid("unsupported judgment correction")),
                    },
                );
            }
            "damage" | "never_die" => {
                require_string(effect, "activateCondition", "none")?;
                if value != 0.0 {
                    return Err(invalid("unknown survival effect"));
                }
                requires_full_life = true;
            }
            _ => return Err(invalid(&format!("unsupported effect {name}"))),
        }
        if !matches!(name.as_str(), "score_over_life" | "score_under_life")
            && fields.contains_key("activateConditionLife")
        {
            return Err(invalid("unexpected life condition"));
        }
    }
    if let Some(once) = source.get("onceEffect") {
        validate_recovery(once, context.skill_level)?;
        requires_full_life = true;
    }
    let unified = unification_value(activation, context)?;
    let score_keys: Vec<_> = values
        .keys()
        .copied()
        .filter(|key| key.starts_with("score"))
        .collect();
    let same_keys = |expected: &[&str]| {
        score_keys.len() == expected.len() && expected.iter().all(|key| values.contains_key(key))
    };
    let mut known_crescendo_parameters = false;
    let score = if same_keys(&[]) {
        if activation.contains_key("unificationActivateEffectValue") {
            return Err(invalid("unification without a score effect"));
        }
        ScoreEffect::Plain { percent: 0.0 }
    } else if same_keys(&["score"]) {
        let percent = unified.unwrap_or(values["score"]);
        if score_condition(&effects["score"])? {
            ScoreEffect::PerfectOnly { percent }
        } else {
            ScoreEffect::Plain { percent }
        }
    } else if same_keys(&["score", "score_continued_note_judge"]) {
        if score_condition(&effects["score"])? {
            return Err(invalid("continuous fallback must allow Great"));
        }
        let high = unified.unwrap_or(values["score_continued_note_judge"]);
        let low = values["score"];
        if high < low {
            return Err(invalid("continuous high value below fallback"));
        }
        ScoreEffect::Continuous { high, low }
    } else if same_keys(&["score_only_perfect"]) {
        ScoreEffect::PerfectOrZero {
            percent: unified.unwrap_or(values["score_only_perfect"]),
        }
    } else if same_keys(&["score_under_great_half"]) {
        ScoreEffect::PerfectOrHalf {
            percent: unified.unwrap_or(values["score_under_great_half"]),
        }
    } else if same_keys(&["score", "score_rate_up_with_perfect"]) {
        if skill_id != 61
            || values["score"] != 100.0
            || values["score_rate_up_with_perfect"] != 0.0
            || score_condition(&effects["score"])?
            || activation.contains_key("unificationActivateEffectValue")
        {
            return Err(invalid("unverified crescendo parameters"));
        }
        // Verified CN 9.4.4 rule and matching skill-61 data; not defaults for
        // arbitrary IDs. Bestdori currently omits the native stack/cap fields.
        known_crescendo_parameters = true;
        ScoreEffect::Crescendo {
            percent: 100.0,
            step: 0.5,
            cap: 150,
        }
    } else if same_keys(&["score_over_life"]) || same_keys(&["score_over_life", "score_under_life"])
    {
        if activation.contains_key("unificationActivateEffectValue") {
            return Err(invalid("unsupported life/unification combination"));
        }
        ScoreEffect::Plain {
            percent: values["score_over_life"],
        }
    } else {
        return Err(invalid("unsupported score-effect combination"));
    };
    Ok(ResolvedTimingSkill {
        skill_id,
        skill: ReplaySkill {
            duration,
            score,
            correction,
        },
        value_sources: sources,
        unification_applied: unified.is_some(),
        known_crescendo_parameters,
        requires_full_life,
    })
}

fn invalid(field: &'static str, reason: &str) -> DataError {
    DataError::InvalidField {
        field,
        value: reason.to_owned(),
    }
}

fn finite_nonnegative(value: &Value, field: &'static str) -> Result<f32, DataError> {
    let number = value
        .as_f64()
        .ok_or_else(|| invalid(field, "expected number"))? as f32;
    if !number.is_finite() || number < 0.0 {
        return Err(invalid(field, "expected finite nonnegative f32"));
    }
    Ok(number)
}

fn check_fields(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), DataError> {
    for field in map.keys() {
        if !allowed.contains(&field.as_str()) {
            return Err(invalid(
                "hitTimingSkill",
                &format!("unsupported field {field}"),
            ));
        }
    }
    Ok(())
}

fn require_string(value: &Value, field: &'static str, expected: &str) -> Result<(), DataError> {
    if value.get(field).and_then(Value::as_str) != Some(expected) {
        return Err(invalid(field, expected));
    }
    Ok(())
}

fn score_condition(effect: &Value) -> Result<bool, DataError> {
    match effect.get("activateCondition").and_then(Value::as_str) {
        Some("perfect") => Ok(true),
        Some("good") => Ok(false),
        _ => Err(invalid(
            "activateCondition",
            "unsupported score judgment threshold",
        )),
    }
}

fn server_value(effect: &Value, requested: Server) -> Result<(f32, Server), DataError> {
    let values = effect
        .get("activateEffectValue")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("activateEffectValue", "expected server array"))?;
    if values.is_empty() || values.len() > 5 {
        return Err(invalid(
            "activateEffectValue",
            "expected at most five server entries",
        ));
    }
    for server in [
        requested,
        Server::Jp,
        Server::Cn,
        Server::En,
        Server::Tw,
        Server::Kr,
    ] {
        if let Some(value) = values.get(server_index(server)).filter(|v| !v.is_null()) {
            return Ok((finite_nonnegative(value, "activateEffectValue")?, server));
        }
    }
    Err(invalid("activateEffectValue", "no server value available"))
}

fn unification_value(
    activation: &Map<String, Value>,
    context: TimingSkillContext,
) -> Result<Option<f32>, DataError> {
    let band = activation.get("unificationActivateConditionBandId");
    let attribute = activation.get("unificationActivateConditionType");
    let Some(value) = activation.get("unificationActivateEffectValue") else {
        if band.is_some() || attribute.is_some() {
            return Err(invalid("unification", "condition without value"));
        }
        return Ok(None);
    };
    if band.is_none() && attribute.is_none() {
        return Err(invalid("unification", "missing team condition"));
    }
    let value = finite_nonnegative(value, "unificationActivateEffectValue")?;
    let required_band = band
        .map(|v| {
            v.as_u64()
                .filter(|&v| v > 0 && v <= u32::MAX as u64)
                .map(|v| v as u32)
                .ok_or_else(|| invalid("unificationActivateConditionBandId", "invalid band"))
        })
        .transpose()?;
    let required_attribute =
        attribute
            .map(|v| {
                parse_attribute(v.as_str().ok_or_else(|| {
                    invalid("unificationActivateConditionType", "expected attribute")
                })?)
            })
            .transpose()?;
    if required_attribute == Some(Attribute::All) {
        return Err(invalid("unification", "unknown all-attribute condition"));
    }
    if required_band.is_some_and(|b| context.common_band != Some(b))
        || required_attribute.is_some_and(|a| context.common_attribute != Some(a))
    {
        return Ok(None);
    }
    Ok(Some(value))
}

fn validate_recovery(once: &Value, level: u8) -> Result<(), DataError> {
    let fields = once
        .as_object()
        .ok_or_else(|| invalid("onceEffect", "expected object"))?;
    check_fields(
        fields,
        &[
            "onceEffectType",
            "onceEffectValueType",
            "onceEffectValue",
            "onceEffectConditionLifeType",
            "onceEffectConditionLife",
        ],
    )?;
    require_string(once, "onceEffectType", "life")?;
    require_string(once, "onceEffectValueType", "real_value")?;
    let value = once
        .get("onceEffectValue")
        .and_then(Value::as_array)
        .and_then(|v| v.get(usize::from(level - 1)))
        .ok_or_else(|| invalid("onceEffectValue", "missing selected recovery level"))?;
    finite_nonnegative(value, "onceEffectValue")?;
    if fields.contains_key("onceEffectConditionLifeType")
        || fields.contains_key("onceEffectConditionLife")
    {
        require_string(once, "onceEffectConditionLifeType", "under_life")?;
        if !once
            .get("onceEffectConditionLife")
            .and_then(Value::as_u64)
            .is_some_and(|v| v > 0 && v <= 1000)
        {
            return Err(invalid(
                "onceEffectConditionLife",
                "outside full-life scope",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
