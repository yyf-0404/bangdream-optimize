import {
  booleanOrDefault,
  finiteNumberOrZero,
  integerOrZero,
  normalizedAttribute,
  positiveIntegerOrDefault,
  positiveIntegerOrUndefined,
} from '../utils.js?v=3';
import { CUSTOM_EVENT_ID } from './event.js?v=3';
import {
  PLAYER_CONFIG_SCHEMA_VERSION,
  normalizePtMaximizeConfig,
  normalizeScoreRangeConfig,
} from './player-settings.js?v=3';

const SERVER_INDEX = {
  jp: 0,
  en: 1,
  tw: 2,
  cn: 3,
  kr: 4,
};

export function createPlayerModel({
  normalizedActivityMode,
  normalizedCalculationMode,
  eventWithParameterBonusFix,
  defaultEventTypeForMode,
  supportedEventTypeOrDefault,
  maxCardLevel,
  normalizeCardTrainingStatus,
}) {
  function normalizedPlayer(player) {
    const calculationMode = normalizedCalculationMode(player.calculationMode);
    const server = normalizedServer(player.server);
    return {
      playerConfigVersion: PLAYER_CONFIG_SCHEMA_VERSION,
      playerId: integerOrZero(player.playerId),
      server,
      currentEvent: player.currentEvent,
      calculationMode,
      activityMode: normalizedActivityMode(player.activityMode),
      scoreRange: normalizeScoreRangeConfig(player.scoreRange, server),
      ptMaximize: normalizePtMaximizeConfig(player.ptMaximize),
      eventSongs: player.eventSongs ?? {},
      eventPresets: normalizedEventPresets(player.eventPresets),
      eventOverrides: normalizedEventOverrides(player.eventOverrides, calculationMode),
      cardList: normalizedCards(player.cardList),
      areaItem: normalizedAreaItems(player.areaItem),
      characterBouns: normalizedCharacterBonuses(player.characterBouns),
    };
  }

  function normalizedEventPresets(presets = {}) {
    const normalized = {};
    for (const [eventId, event] of Object.entries(presets ?? {})) {
      if (!event || typeof event !== 'object' || Array.isArray(event)) {
        continue;
      }
      normalized[eventId] = cloneJson(event);
    }
    return normalized;
  }

  function normalizedEventOverrides(overrides = {}, calculationMode = 'maximize') {
    const normalized = {};
    for (const [eventId, event] of Object.entries(overrides ?? {})) {
      normalized[eventId] = editableEventOverride(event, calculationMode);
    }
    return normalized;
  }

  function editableEventSnapshot(eventId, player, coreEvents = {}) {
    if (eventId == null) {
      return undefined;
    }
    const base = eventWithParameterBonusFix(
      player?.eventPresets?.[String(eventId)] ?? coreEvents?.[String(eventId)] ?? {},
      eventId,
    );
    const override = Number(eventId) === CUSTOM_EVENT_ID
      ? player?.eventOverrides?.[String(eventId)] ?? {}
      : {};
    const event = {
      ...cloneJson(base),
      ...cloneJson(override),
    };
    if (!event.eventType) {
      event.eventType = defaultEventTypeForMode(
        player?.activityMode,
        player?.calculationMode,
      );
    }
    if (!Object.keys(event).length) {
      return undefined;
    }
    return {
      ...event,
      ...editableEventOverride(event, player?.calculationMode),
    };
  }

  function editableEventOverride(event = {}, calculationMode = 'maximize') {
    const eventType = typeof event.eventType === 'string' && event.eventType.trim()
      ? event.eventType
      : supportedEventTypeOrDefault(event.eventType, calculationMode);
    return {
      eventType,
      attributes: normalizedEventAttributes(event.attributes),
      characters: normalizedEventCharacters(event.characters),
      members: normalizedEventMembers(event.members),
      eventAttributeAndCharacterBonus: normalizedEventAttributeAndCharacterBonus(
        event.eventAttributeAndCharacterBonus,
      ),
      eventCharacterParameterBonus: normalizedEventCharacterParameterBonus(
        event.eventCharacterParameterBonus,
      ),
      limitBreaks: Array.isArray(event.limitBreaks) ? cloneJson(event.limitBreaks) : [],
    };
  }

  function normalizedCards(cards = {}) {
    const normalized = {};
    for (const [cardId, config] of Object.entries(cards ?? {})) {
      normalized[cardId] = normalizedCardConfig(cardId, config);
    }
    return normalized;
  }

  function normalizedCardConfig(cardId, config = {}) {
    const episodes = Array.isArray(config.episodes) ? config.episodes : [];
    return {
      level: positiveIntegerOrDefault(config.level, maxCardLevel(cardId)),
      training: normalizeCardTrainingStatus(cardId, config.training),
      illustTrainingStatus: normalizeCardTrainingStatus(cardId, config.illustTrainingStatus),
      episodes: [
        booleanOrDefault(episodes[0], true),
        booleanOrDefault(episodes[1], true),
      ],
      limitBreakRank: integerOrZero(config.limitBreakRank),
      skillLevel: positiveIntegerOrDefault(config.skillLevel, 5),
    };
  }

  return {
    editableEventOverride,
    editableEventSnapshot,
    normalizedCardConfig,
    normalizedCharacterBonus,
    normalizedEventAttributeAndCharacterBonus,
    normalizedEventAttributes,
    normalizedEventCharacterParameterBonus,
    normalizedEventCharacters,
    normalizedEventMembers,
    normalizedPlayer,
    normalizedServer,
    normalizedStatRate,
  };
}

export function normalizedServer(value) {
  return Object.prototype.hasOwnProperty.call(SERVER_INDEX, String(value))
    ? String(value)
    : 'cn';
}

export function normalizedEventAttributes(attributes = []) {
  return (Array.isArray(attributes) ? attributes : [])
    .map((bonus) => ({
      attribute: normalizedAttribute(bonus?.attribute),
      percent: finiteNumberOrZero(bonus?.percent),
    }))
    .filter((bonus) => bonus.attribute);
}

export function normalizedEventCharacters(characters = []) {
  return (Array.isArray(characters) ? characters : [])
    .map((bonus) => ({
      characterId: positiveIntegerOrUndefined(bonus?.characterId),
      percent: finiteNumberOrZero(bonus?.percent),
    }))
    .filter((bonus) => bonus.characterId != null);
}

export function normalizedEventMembers(members = []) {
  return (Array.isArray(members) ? members : [])
    .map((bonus) => ({
      situationId: positiveIntegerOrUndefined(bonus?.situationId ?? bonus?.cardId),
      percent: finiteNumberOrZero(bonus?.percent),
    }))
    .filter((bonus) => bonus.situationId != null);
}

export function normalizedEventAttributeAndCharacterBonus(value = {}) {
  return {
    pointPercent: finiteNumberOrZero(value?.pointPercent),
    parameterPercent: finiteNumberOrZero(value?.parameterPercent),
  };
}

export function normalizedEventCharacterParameterBonus(value = {}) {
  return {
    performance: finiteNumberOrZero(value?.performance),
    technique: finiteNumberOrZero(value?.technique),
    visual: finiteNumberOrZero(value?.visual),
  };
}

export function normalizedAreaItems(areaItems = {}) {
  const normalized = {};
  for (const [areaItemId, config] of Object.entries(areaItems ?? {})) {
    normalized[areaItemId] = {
      level: integerOrZero(config?.level),
    };
  }
  return normalized;
}

export function normalizedCharacterBonuses(characterBonuses = {}) {
  const normalized = {};
  for (const [characterId, bonus] of Object.entries(characterBonuses ?? {})) {
    normalized[characterId] = normalizedCharacterBonus(bonus);
  }
  return normalized;
}

export function normalizedCharacterBonus(bonus = {}) {
  return {
    potential: normalizedStatRate(bonus.potential),
    characterTask: normalizedStatRate(bonus.characterTask),
  };
}

export function normalizedStatRate(rate = {}) {
  return {
    performance: finiteNumberOrZero(rate.performance),
    technique: finiteNumberOrZero(rate.technique),
    visual: finiteNumberOrZero(rate.visual),
  };
}

export function readFiniteInput(input, label) {
  if (!input.value.trim()) {
    return 0;
  }
  const number = Number(input.value);
  if (!Number.isFinite(number) || number < 0) {
    throw new Error(`${label} 无效：${input.value}`);
  }
  return number;
}

export function cloneJson(value) {
  return value == null ? value : JSON.parse(JSON.stringify(value));
}
