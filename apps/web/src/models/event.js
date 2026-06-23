import {
  numericStringSort,
  positiveIntegerOrUndefined,
} from '../utils.js?v=2';

const DEFAULT_EVENT_DIFFICULTY = 3;
const ACTIVITY_MODES = {
  medley: ['medley'],
  single: ['challenge', 'versus'],
};
const SUPPORTED_EVENT_TYPES = [...ACTIVITY_MODES.medley, ...ACTIVITY_MODES.single];
const DEFAULT_EVENT_TYPE = 'challenge';
export const CUSTOM_EVENT_ID = 0;

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

  function defaultSongListForMode(mode, event) {
    return fixedSongListForMode([], mode, event);
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

  function defaultEditableEvent(mode) {
    return {
      eventType: defaultEventTypeForMode(mode),
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

export function normalizedActivityMode(value) {
  return value === 'medley' ? 'medley' : 'single';
}

export function eventMatchesActivityMode(event, mode) {
  return eventTypesForMode(mode).includes(String(event?.eventType));
}

export function eventTypesForMode(mode) {
  return ACTIVITY_MODES[normalizedActivityMode(mode)];
}

export function defaultEventTypeForMode(mode) {
  return normalizedActivityMode(mode) === 'medley' ? 'medley' : DEFAULT_EVENT_TYPE;
}

export function supportedEventTypeOrDefault(value) {
  return isSupportedEventType(value) ? value : DEFAULT_EVENT_TYPE;
}

export function isSupportedEventType(value) {
  return SUPPORTED_EVENT_TYPES.includes(String(value));
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
