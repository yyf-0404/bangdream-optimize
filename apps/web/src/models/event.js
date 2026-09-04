import {
  numericStringSort,
  positiveIntegerOrUndefined,
} from '../utils.js?v=3';

const DEFAULT_EVENT_DIFFICULTY = 3;
const CALCULATION_EVENT_TYPES = {
  maximize: {
    medley: ['medley'],
    single: ['challenge', 'versus'],
  },
  scoreRange: {
    medley: ['medley'],
    single: ['challenge', 'versus', 'live_try', 'festival', 'mission_live'],
  },
  ptMaximize: {
    medley: ['medley'],
    single: ['challenge', 'versus', 'live_try', 'festival', 'mission_live'],
  },
  ptEvaluate: {
    medley: ['medley'],
    single: ['challenge', 'versus', 'live_try', 'festival', 'mission_live'],
  },
};
const DEFAULT_EVENT_TYPE = 'challenge';
const HIDDEN_EVENT_IDS = new Set([5001]);
export const CUSTOM_EVENT_ID = 0;
const DEFAULT_SERVER_INDEX = 3;

export function isHiddenEventId(value) {
  return HIDDEN_EVENT_IDS.has(Number(value));
}

export function createEventModel({
  getSongRecords,
  getEventCharacterParameterBonusFix,
  serverScopedValue,
  cloneJson,
}) {
  function eventWithParameterBonusFix(event, eventId) {
    if (event?.eventCharacterParameterBonus != null) {
      return event;
    }
    const fixedValue = getEventCharacterParameterBonusFix()?.[String(eventId)];
    if (fixedValue == null) {
      return event;
    }
    return {
      ...event,
      eventCharacterParameterBonus: cloneJson(fixedValue),
    };
  }

  function eventSongsFromPreset(event) {
    const entries = [];
    const sources = [
      serverScopedSongList(event?.musics),
      serverScopedSongList(event?.music),
      event?.eventMusics,
      event?.eventMusic,
      event?.musicIds,
    ];

    for (const source of sources) {
      collectSongEntries(source, entries);
      if (entries.length > 0) {
        break;
      }
    }

    return entries.map((song) => ({
      songId: song.songId,
      difficulty: song.difficulty ?? DEFAULT_EVENT_DIFFICULTY,
    }));
  }

  function ensureSongListForMode(player, eventId, event) {
    player.eventSongs[String(eventId)] = fixedSongListForMode(
      player.eventSongs[String(eventId)],
      player.activityMode,
      event,
    );
  }

  function defaultSongListForMode(mode, event, cachedSongs = []) {
    return fixedSongListForMode(cachedSongs, mode, event);
  }

  function fixedSongListForMode(cachedSongs, mode, event) {
    const normalizedMode = normalizedActivityMode(mode);
    const expected = requiredSongCountForMode(normalizedMode);
    const cached = normalizedSongSelections(cachedSongs);
    if (cached.length === expected && cached.every((song) => song.songId > 0)) {
      return cached;
    }

    const preset = normalizedSongSelections(eventSongsFromPreset(event));
    const source = preset.length > 0 ? preset : cached;
    const result = [];
    for (let index = 0; index < expected; index += 1) {
      const fallback = source[index] ?? {};
      result.push({
        songId: positiveIntegerOrUndefined(fallback.songId) ?? firstSongId() ?? 0,
        difficulty: parseDifficulty(fallback.difficulty) ?? DEFAULT_EVENT_DIFFICULTY,
      });
    }
    return result;
  }

  function normalizedSongSelections(songs = []) {
    return (Array.isArray(songs) ? songs : []).map((song) => ({
      songId: positiveIntegerOrUndefined(song?.songId) ?? 0,
      difficulty: parseDifficulty(song?.difficulty) ?? DEFAULT_EVENT_DIFFICULTY,
    }));
  }

  function defaultEditableEvent(mode, calculationMode = 'maximize') {
    return {
      eventType: defaultEventTypeForMode(mode, calculationMode),
      attributes: [],
      characters: [],
      members: [],
      eventAttributeAndCharacterBonus: {
        pointPercent: 0,
        parameterPercent: 0,
      },
      eventCharacterParameterBonus: {
        performance: 0,
        technique: 0,
        visual: 0,
      },
      limitBreaks: [],
    };
  }

  function firstSongId() {
    const [first] = Object.keys(getSongRecords() ?? {}).sort(numericStringSort);
    return first == null ? undefined : Number(first);
  }

  function serverScopedSongList(value) {
    if (!Array.isArray(value)) {
      return value;
    }
    return value.some(isSongEntry) ? value : serverScopedValue(value);
  }

  return {
    activityModeForEvent,
    defaultEditableEvent,
    defaultEventTypeForMode,
    defaultSongListForMode,
    ensureSongListForMode,
    eventMatchesActivityMode,
    eventSongsFromPreset,
    eventTypesForMode,
    eventWithParameterBonusFix,
    fixedSongListForMode,
    isSupportedEventType,
    normalizedActivityMode,
    parseDifficulty,
    supportedEventTypeOrDefault,
  };
}

export function requiredSongCountForMode(mode) {
  return normalizedActivityMode(mode) === 'medley' ? 3 : 1;
}

export function activityModeForEvent(event) {
  return event?.eventType === 'medley' ? 'medley' : 'single';
}

export function normalizedCalculationMode(value) {
  return ['maximize', 'scoreRange', 'ptMaximize', 'ptEvaluate'].includes(value)
    ? value
    : 'ptMaximize';
}

export function normalizedActivityMode(value) {
  return value === 'medley' ? 'medley' : 'single';
}

export function eventMatchesActivityMode(event, mode, calculationMode = 'maximize') {
  return eventTypesForMode(mode, calculationMode).includes(String(event?.eventType));
}

export function eventTypesForMode(mode, calculationMode = 'maximize') {
  return CALCULATION_EVENT_TYPES[normalizedCalculationMode(calculationMode)][
    normalizedActivityMode(mode)
  ];
}

export function defaultEventTypeForMode(mode, _calculationMode = 'maximize') {
  return normalizedActivityMode(mode) === 'medley' ? 'medley' : DEFAULT_EVENT_TYPE;
}

export function supportedEventTypeOrDefault(value, calculationMode = 'maximize') {
  return isSupportedEventType(value, calculationMode) ? value : DEFAULT_EVENT_TYPE;
}

export function isSupportedEventType(value, calculationMode = 'maximize') {
  const type = String(value);
  const modes = CALCULATION_EVENT_TYPES[normalizedCalculationMode(calculationMode)];
  return modes.medley.includes(type) || modes.single.includes(type);
}

export function recentUnfinishedEvent(
  events,
  {
    serverIndex = DEFAULT_SERVER_INDEX,
    now = Date.now(),
    calculationMode = 'ptMaximize',
  } = {},
) {
  const active = [];
  const upcoming = [];
  for (const [eventId, event] of Object.entries(events ?? {})) {
    const id = Number.parseInt(eventId, 10);
    if (
      !Number.isInteger(id)
      || id <= 0
      || isHiddenEventId(id)
      || !eventMatchesActivityMode(event, 'single', calculationMode)
        && !eventMatchesActivityMode(event, 'medley', calculationMode)
    ) {
      continue;
    }
    const endAt = serverEventTimestamp(event?.endAt, serverIndex);
    if (endAt == null || endAt <= now) {
      continue;
    }
    const startAt = serverEventTimestamp(event?.startAt, serverIndex);
    const candidate = { id, event, startAt: startAt ?? Number.NEGATIVE_INFINITY, endAt };
    if (startAt != null && startAt > now) {
      upcoming.push(candidate);
    } else {
      active.push(candidate);
    }
  }
  active.sort((left, right) =>
    right.startAt - left.startAt || right.id - left.id);
  upcoming.sort((left, right) =>
    left.startAt - right.startAt || left.id - right.id);
  return active[0] ?? upcoming[0];
}

function serverEventTimestamp(value, serverIndex) {
  const scoped = Array.isArray(value) ? value[serverIndex] : value;
  const timestamp = Number(scoped);
  return Number.isFinite(timestamp) && timestamp > 0 ? timestamp : undefined;
}

function collectSongEntries(value, entries) {
  if (value == null) {
    return;
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      collectSongEntries(item, entries);
    }
    return;
  }

  if (typeof value === 'number' || typeof value === 'string') {
    const songId = Number.parseInt(value, 10);
    if (Number.isInteger(songId) && songId > 0) {
      entries.push({ songId });
    }
    return;
  }

  if (typeof value !== 'object') {
    return;
  }

  const rawSongId = value.musicId ?? value.songId ?? value.music_id ?? value.id;
  const songId = Number.parseInt(rawSongId, 10);
  if (!Number.isInteger(songId) || songId <= 0) {
    return;
  }

  entries.push({
    songId,
    difficulty: parseDifficulty(value.difficulty ?? value.difficultyIndex),
  });
}

function isSongEntry(value) {
  if (value == null || Array.isArray(value) || typeof value !== 'object') {
    return false;
  }
  const rawSongId = value.musicId ?? value.songId ?? value.music_id ?? value.id;
  const songId = Number.parseInt(rawSongId, 10);
  return Number.isInteger(songId) && songId > 0;
}

export function parseDifficulty(value) {
  if (value == null) {
    return undefined;
  }
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    const index = ['easy', 'normal', 'hard', 'expert', 'special'].indexOf(normalized);
    if (index >= 0) {
      return index;
    }
  }
  const number = Number(value);
  return Number.isInteger(number) && number >= 0 && number <= 4 ? number : undefined;
}
