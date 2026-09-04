const PT_MAXIMIZE_LIVE_VARIANTS_BY_EVENT_TYPE = {
  mission_live: ['solo', 'cooperative'],
  live_try: ['solo', 'cooperative'],
  challenge: ['solo', 'cooperative', 'challenge_cp'],
  versus: ['solo', 'versus'],
  festival: ['solo', 'festival'],
  medley: ['medley'],
};

const PT_EVALUATE_LIVE_VARIANTS_BY_EVENT_TYPE = {
  mission_live: ['solo'],
  live_try: ['solo'],
  challenge: ['solo', 'challenge_cp'],
  versus: ['solo', 'versus'],
  festival: ['solo'],
  medley: ['medley'],
};

export const PLAYER_CONFIG_SCHEMA_VERSION = 1;

const COOPERATIVE_LEADER_MODES = new Set([
  'max_stat',
  'specified',
  'random',
]);

export function createDefaultScoreRangeConfig(server = 'cn') {
  return {
    currentPt: 0,
    targetTotalPt: 0,
    autoBaseMultiplier: server === 'jp' ? 0.75 : 0.5,
    missionSupportPtBonus: undefined,
    maxResults: 1,
  };
}

export function normalizeScoreRangeConfig(value = {}, server = 'cn') {
  const defaults = createDefaultScoreRangeConfig(server);
  const autoBaseMultiplier = Number(value?.autoBaseMultiplier);
  return {
    currentPt: nonNegativeIntegerOrDefault(value?.currentPt, defaults.currentPt),
    targetTotalPt: nonNegativeIntegerOrDefault(
      value?.targetTotalPt,
      defaults.targetTotalPt,
    ),
    autoBaseMultiplier: [0.5, 0.75].includes(autoBaseMultiplier)
      ? autoBaseMultiplier
      : defaults.autoBaseMultiplier,
    missionSupportPtBonus: nonNegativeIntegerOrUndefined(
      value?.missionSupportPtBonus,
    ),
    maxResults: 1,
  };
}

export function createDefaultPtMaximizeConfig() {
  return {
    liveVariantByEventType: Object.fromEntries(
      Object.entries(PT_MAXIMIZE_LIVE_VARIANTS_BY_EVENT_TYPE)
        .map(([eventType, variants]) => [eventType, variants[0]]),
    ),
    minimumPersonalStat: 290000,
    missionSupportPtBonus: 100,
    teammateMode: 'uniform',
    cooperativeLeaderMode: 'max_stat',
    cooperativeSpecifiedLeader: 0,
    teammates: Array.from({ length: 4 }, createDefaultTeammate),
    versusTeamRank: 0,
    festivalTeamRank: 0,
    festivalWon: true,
    festivalTeammateMode: 'uniform',
    festivalTeammateScores: Array(4).fill(4000000),
  };
}

export function normalizePtMaximizeConfig(value = {}) {
  const defaults = createDefaultPtMaximizeConfig();
  return {
    liveVariantByEventType: normalizeLiveVariants(value?.liveVariantByEventType),
    minimumPersonalStat: nonNegativeIntegerOrDefault(
      value?.minimumPersonalStat,
      defaults.minimumPersonalStat,
    ),
    missionSupportPtBonus: nonNegativeIntegerOrDefault(
      value?.missionSupportPtBonus,
      defaults.missionSupportPtBonus,
    ),
    teammateMode: normalizedTeammateMode(value?.teammateMode),
    cooperativeLeaderMode: COOPERATIVE_LEADER_MODES.has(value?.cooperativeLeaderMode)
      ? value.cooperativeLeaderMode
      : defaults.cooperativeLeaderMode,
    cooperativeSpecifiedLeader: boundedPlayerIndex(
      value?.cooperativeSpecifiedLeader,
      defaults.cooperativeSpecifiedLeader,
    ),
    teammates: normalizeTeammates(value?.teammates),
    versusTeamRank: boundedPlayerIndex(
      value?.versusTeamRank,
      defaults.versusTeamRank,
    ),
    festivalTeamRank: boundedPlayerIndex(
      value?.festivalTeamRank,
      defaults.festivalTeamRank,
    ),
    festivalWon: value?.festivalWon == null
      ? defaults.festivalWon
      : value.festivalWon === true,
    festivalTeammateMode: normalizedTeammateMode(value?.festivalTeammateMode),
    festivalTeammateScores: normalizeFourIntegers(
      value?.festivalTeammateScores,
      defaults.festivalTeammateScores[0],
    ),
  };
}

export function createDefaultPtEvaluateConfig(server = 'cn') {
  return {
    liveVariantByEventType: Object.fromEntries(
      Object.entries(PT_EVALUATE_LIVE_VARIANTS_BY_EVENT_TYPE)
        .map(([eventType, variants]) => [eventType, variants[0]]),
    ),
    teams: Array.from({ length: 3 }, () => Array(5).fill(0)),
    items: { band: '', attribute: '', magazine: 'performance' },
    scoreMode: 'manual',
    autoBaseMultiplier: server === 'jp' ? 0.75 : 0.5,
    missionSupportPtBonus: 100,
    versusTeamRank: 0,
  };
}

export function normalizePtEvaluateConfig(value = {}, server = 'cn') {
  const defaults = createDefaultPtEvaluateConfig(server);
  const sourceTeams = Array.isArray(value?.teams) ? value.teams : [];
  const multiplier = Number(value?.autoBaseMultiplier);
  return {
    liveVariantByEventType: normalizeVariants(
      value?.liveVariantByEventType,
      PT_EVALUATE_LIVE_VARIANTS_BY_EVENT_TYPE,
    ),
    teams: Array.from({ length: 3 }, (_, teamIndex) =>
      Array.from({ length: 5 }, (_, cardIndex) =>
        nonNegativeIntegerOrDefault(sourceTeams[teamIndex]?.[cardIndex], 0))),
    items: {
      band: String(value?.items?.band ?? ''),
      attribute: String(value?.items?.attribute ?? ''),
      magazine: ['performance', 'technique', 'visual'].includes(value?.items?.magazine)
        ? value.items.magazine
        : defaults.items.magazine,
    },
    scoreMode: value?.scoreMode === 'auto' ? 'auto' : 'manual',
    autoBaseMultiplier: [0.5, 0.75].includes(multiplier)
      ? multiplier
      : defaults.autoBaseMultiplier,
    missionSupportPtBonus: nonNegativeIntegerOrDefault(
      value?.missionSupportPtBonus,
      defaults.missionSupportPtBonus,
    ),
    versusTeamRank: boundedPlayerIndex(value?.versusTeamRank, 0),
  };
}

export function ptEvaluateLiveVariant(config, eventType) {
  const variants = PT_EVALUATE_LIVE_VARIANTS_BY_EVENT_TYPE[eventType];
  const selected = config?.liveVariantByEventType?.[eventType];
  return variants?.includes(selected) ? selected : variants?.[0] ?? 'solo';
}

export function ptEvaluateSupportsAuto(liveVariant) {
  return liveVariant === 'solo' || liveVariant === 'medley';
}

export function withPtEvaluateLiveVariant(config, eventType, liveVariant, server = 'cn') {
  const normalized = normalizePtEvaluateConfig(config, server);
  const variants = PT_EVALUATE_LIVE_VARIANTS_BY_EVENT_TYPE[eventType];
  if (variants?.includes(liveVariant)) {
    normalized.liveVariantByEventType[eventType] = liveVariant;
  }
  return normalized;
}

export function ptMaximizeLiveVariant(config, eventType) {
  const variants = PT_MAXIMIZE_LIVE_VARIANTS_BY_EVENT_TYPE[eventType];
  if (!variants) {
    return 'solo';
  }
  const selected = config?.liveVariantByEventType?.[eventType];
  return variants.includes(selected) ? selected : variants[0];
}

export function withPtMaximizeLiveVariant(config, eventType, liveVariant) {
  const normalized = normalizePtMaximizeConfig(config);
  const variants = PT_MAXIMIZE_LIVE_VARIANTS_BY_EVENT_TYPE[eventType];
  if (!variants?.includes(liveVariant)) {
    return normalized;
  }
  normalized.liveVariantByEventType[eventType] = liveVariant;
  return normalized;
}

function createDefaultTeammate() {
  return {
    expectedStat: 290000,
    leaderScoreUp: 130,
    leaderSkillDuration: 7,
  };
}

function normalizeLiveVariants(value) {
  return normalizeVariants(value, PT_MAXIMIZE_LIVE_VARIANTS_BY_EVENT_TYPE);
}

function normalizeVariants(value, variantsByEventType) {
  const source = value && typeof value === 'object' && !Array.isArray(value) ? value : {};
  return Object.fromEntries(
    Object.entries(variantsByEventType)
      .map(([eventType, variants]) => [
        eventType,
        variants.includes(source[eventType]) ? source[eventType] : variants[0],
      ]),
  );
}

function normalizeTeammates(value) {
  const source = Array.isArray(value) ? value : [];
  return Array.from({ length: 4 }, (_, index) => {
    const defaults = createDefaultTeammate();
    return {
      expectedStat: nonNegativeIntegerOrDefault(
        source[index]?.expectedStat,
        defaults.expectedStat,
      ),
      leaderScoreUp: nonNegativeNumberOrDefault(
        source[index]?.leaderScoreUp,
        defaults.leaderScoreUp,
      ),
      leaderSkillDuration: nonNegativeNumberOrDefault(
        source[index]?.leaderSkillDuration,
        defaults.leaderSkillDuration,
      ),
    };
  });
}

function normalizeFourIntegers(value, fallback) {
  const source = Array.isArray(value) ? value : [];
  return Array.from({ length: 4 }, (_, index) =>
    nonNegativeIntegerOrDefault(source[index], fallback));
}

function normalizedTeammateMode(value) {
  return value === 'individual' ? 'individual' : 'uniform';
}

function boundedPlayerIndex(value, fallback) {
  return Math.min(4, nonNegativeIntegerOrDefault(value, fallback));
}

function nonNegativeIntegerOrDefault(value, fallback) {
  if (value == null || String(value).trim() === '') {
    return fallback;
  }
  const number = Number(value);
  return Number.isSafeInteger(number) && number >= 0 ? number : fallback;
}

function nonNegativeIntegerOrUndefined(value) {
  if (value == null || String(value).trim() === '') {
    return undefined;
  }
  const number = Number(value);
  return Number.isSafeInteger(number) && number >= 0 ? number : undefined;
}

function nonNegativeNumberOrDefault(value, fallback) {
  if (value == null || String(value).trim() === '') {
    return fallback;
  }
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? number : fallback;
}
