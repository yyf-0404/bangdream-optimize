const DB_NAME = 'bangdream-optimize-game-data';
const DB_VERSION = 1;
const FILE_STORE = 'files';
const MANIFEST_PATH = 'manifest.json';
const SCORE_RANGE_CHART_META_PATH = 'api/scoreRangeChartMeta.2.json';
const CUSTOM_EVENT_ID = 0;

const REQUIRED_CORE_FILES = [
  'api/cards/all.5.json',
  'api/characters/main.3.json',
  'api/skills/all.10.json',
  'api/areaItems/main.5.json',
  'api/events/all.6.json',
  'api/songs/all.7.json',
];

const OPTIONAL_REPAIR_FILES = [
  'cardsCNfix.json',
  'skillsCNfix.json',
  'areaItemFix.json',
  'eventCharacterParameterBonusFix.json',
];

const DIFFICULTY_NAMES = ['easy', 'normal', 'hard', 'expert', 'special'];

export function createGameDataClient(options = {}) {
  return new GameDataClient(options);
}

export function chartPath(songId, difficulty) {
  const name = DIFFICULTY_NAMES[difficulty];
  if (!name) {
    throw new Error(`invalid difficulty: ${difficulty}`);
  }
  return `api/charts/${songId}/${name}.json`;
}

export function eventPath(eventId) {
  const normalizedId = Number.parseInt(eventId, 10);
  if (!Number.isSafeInteger(normalizedId) || normalizedId <= CUSTOM_EVENT_ID) {
    throw new Error(`invalid event id: ${eventId}`);
  }
  return `api/events/${normalizedId}.json`;
}

export function cardPath(cardId) {
  const normalizedId = Number.parseInt(cardId, 10);
  if (!Number.isSafeInteger(normalizedId) || normalizedId <= 0) {
    throw new Error(`invalid card id: ${cardId}`);
  }
  return `api/cards/${normalizedId}.json`;
}

export class GameDataClient {
  constructor({ baseUrl = '/game-data', onProgress } = {}) {
    this.baseUrl = normalizeBaseUrl(baseUrl);
    this.onProgress = onProgress;
    this.dbPromise = openDatabase();
    this.manifestPromises = new Map();
  }

  async syncCore({ refreshManifest = false } = {}) {
    const manifest = refreshManifest
      ? await this.refreshManifest()
      : await this.loadManifest();
    const result = {};

    for (const path of REQUIRED_CORE_FILES) {
      result[toPayloadKey(path)] = await this.syncFile(path, { manifest });
    }

    for (const path of OPTIONAL_REPAIR_FILES) {
      if (manifest.files[path]) {
        result[toPayloadKey(path)] = await this.syncFile(path, {
          manifest,
          optional: true,
        });
      }
    }

    return result;
  }

  async syncEvent(eventId) {
    if (Number(eventId) === CUSTOM_EVENT_ID) {
      throw new Error('自定义活动没有远端活动详情');
    }
    const path = eventPath(eventId);
    try {
      return await this.syncFile(path, { useManifest: false });
    } catch (error) {
      if (!isHttpStatusError(error, 404)) {
        throw error;
      }
    }

    const events = await this.syncFile('api/events/all.6.json');
    const event = events[String(eventId)];
    if (!event) {
      throw new Error(`event ${eventId} is missing`);
    }
    return event;
  }

  async syncChart(songId, difficulty) {
    const path = chartPath(songId, difficulty);
    return {
      songId,
      difficulty,
      data: await this.syncFile(path, { useManifest: false }),
    };
  }

  async syncScoreRangeChartMeta() {
    return this.syncFile(SCORE_RANGE_CHART_META_PATH);
  }

  async syncCardDetail(cardId) {
    return this.syncFile(cardPath(cardId), { useManifest: false });
  }

  async buildCalculationPayload({ player, server, eventId, options, core: preloadedCore }) {
    const selectedEventId = eventId ?? player.currentEvent;
    if (selectedEventId == null) {
      throw new Error('current event is not set');
    }

    const core = preloadedCore ?? await this.syncCore({ refreshManifest: true });
    const cards = await this.cardsWithRequestedDetails(core.cards, player);
    const event = await this.calculationEvent(
      selectedEventId,
      player,
      core,
    );
    const songs = selectSongs(
      core.songs,
      player.eventSongs?.[String(selectedEventId)] ?? [],
    );
    const charts = await Promise.all(
      (player.eventSongs?.[String(selectedEventId)] ?? []).map((song) =>
        this.syncChart(song.songId, song.difficulty),
      ),
    );

    return {
      cards,
      characters: core.characters,
      skills: core.skills,
      areaItems: core.areaItems,
      cardsFix: core.cardsFix,
      skillsFix: core.skillsFix,
      areaItemsFix: core.areaItemsFix,
      event,
      songs,
      charts,
      player,
      server,
      eventId: selectedEventId,
      options,
    };
  }

  async buildScoreRangePayload({ player, server, eventId, request, core: preloadedCore }) {
    const selectedEventId = eventId ?? player.currentEvent;
    if (selectedEventId == null) {
      throw new Error('current event is not set');
    }

    const core = preloadedCore ?? await this.syncCore({ refreshManifest: true });
    const [cards, event, scoreRangeChartMeta] = await Promise.all([
      this.cardsWithRequestedDetails(core.cards, player),
      this.calculationEvent(selectedEventId, player, core),
      this.syncScoreRangeChartMeta(),
    ]);
    return {
      cards,
      characters: core.characters,
      skills: core.skills,
      areaItems: core.areaItems,
      cardsFix: core.cardsFix,
      skillsFix: core.skillsFix,
      areaItemsFix: core.areaItemsFix,
      event,
      songs: core.songs,
      scoreRangeChartMeta,
      player,
      server,
      eventId: selectedEventId,
      request,
      nowMillis: Date.now(),
    };
  }

  async buildPtMaximizePayload({ player, server, eventId, request, core: preloadedCore }) {
    const selectedEventId = eventId ?? player.currentEvent;
    if (selectedEventId == null) {
      throw new Error('current event is not set');
    }

    const core = preloadedCore ?? await this.syncCore({ refreshManifest: true });
    const [cards, event] = await Promise.all([
      this.cardsWithRequestedDetails(core.cards, player),
      this.calculationEvent(selectedEventId, player, core),
    ]);
    const songs = selectSongs(core.songs, request.songs ?? []);
    const charts = await Promise.all(
      (request.songs ?? []).map((song) => this.syncChart(song.songId, song.difficulty)),
    );
    return {
      cards,
      characters: core.characters,
      skills: core.skills,
      areaItems: core.areaItems,
      cardsFix: core.cardsFix,
      skillsFix: core.skillsFix,
      areaItemsFix: core.areaItemsFix,
      event,
      songs,
      charts,
      player,
      server,
      eventId: selectedEventId,
      request,
    };
  }

  async calculationEvent(selectedEventId, player, core) {
    const key = String(selectedEventId);
    const isCustomEvent = Number(selectedEventId) === CUSTOM_EVENT_ID;
    const override = isCustomEvent
      ? player.eventOverrides?.[key]
      : undefined;
    if (isCustomEvent && override) {
      return override;
    }
    if (isCustomEvent) {
      throw new Error('未设置自定义活动参数');
    }

    const base = applyEventCharacterParameterBonusFix(
      player.eventPresets?.[key] ?? await this.syncEvent(selectedEventId),
      core.eventCharacterParameterBonusFix,
      selectedEventId,
    );
    return applyEventOverride(base, override);
  }

  async cardsWithRequestedDetails(coreCards, player) {
    let cards = coreCards;
    for (const [cardId, config] of Object.entries(player.cardList ?? {})) {
      const level = Number.parseInt(config?.level ?? 0, 10);
      if (!level || cardHasLevel(coreCards?.[cardId], level)) {
        continue;
      }

      const detail = await this.syncCardDetail(cardId);
      if (cards === coreCards) {
        cards = { ...coreCards };
      }
      cards[cardId] = mergeCardDetail(coreCards?.[cardId], detail);
    }
    return cards;
  }

  async getJson(path) {
    const record = await getRecord(await this.dbPromise, path);
    return record?.json;
  }

  async clearCache() {
    const db = await this.dbPromise;
    await requestToPromise(db.transaction(FILE_STORE, 'readwrite').objectStore(FILE_STORE).clear());
    this.manifestPromises.clear();
  }

  async loadManifest() {
    return this.loadManifestPath(MANIFEST_PATH);
  }

  async refreshManifest() {
    this.manifestPromises.delete(MANIFEST_PATH);
    return this.loadManifestPath(MANIFEST_PATH);
  }

  async loadManifestForPath(path) {
    const manifestPath = manifestPathForFile(path);
    return this.loadManifestPath(manifestPath);
  }

  async loadManifestPath(path) {
    if (!this.manifestPromises.has(path)) {
      this.manifestPromises.set(path, this.fetchManifest(path));
    }
    return this.manifestPromises.get(path);
  }

  async fetchManifest(path) {
    const response = await fetch(this.url(path), {
      cache: 'no-cache',
    });
    if (!response.ok) {
      if (response.status === 404) {
        return { version: undefined, generatedAt: undefined, files: {} };
      }
      const error = new Error(`failed to fetch ${path}: ${response.status}`);
      error.status = response.status;
      throw error;
    }
    const manifest = await response.json();
    if (!manifest.files || typeof manifest.files !== 'object') {
      throw new Error(`${path} manifest.files is missing`);
    }
    await putRecord(await this.dbPromise, {
      path,
      json: manifest,
      meta: { version: manifest.version, generatedAt: manifest.generatedAt },
      updatedAt: Date.now(),
    });
    return manifest;
  }

  async syncFile(path, { manifest, optional = false, useManifest = true } = {}) {
    manifest ??= useManifest ? await this.loadManifestForPath(path) : undefined;
    const fileMeta = manifestFileMeta(manifest, path);
    if (optional && !fileMeta) {
      return undefined;
    }

    const db = await this.dbPromise;
    const local = await getRecord(db, path);
    if (local && !needsUpdate(local, fileMeta)) {
      this.progress('cache-hit', path);
      return local.json;
    }

    this.progress('fetch-start', path);
    const response = await fetch(this.url(path), {
      cache: 'no-cache',
    });
    if (optional && response.status === 404) {
      this.progress('missing-optional', path);
      return undefined;
    }
    if (!response.ok) {
      const error = new Error(`failed to fetch ${path}: ${response.status}`);
      error.status = response.status;
      throw error;
    }

    const json = await response.json();
    await putRecord(db, {
      path,
      json,
      meta: normalizeFileMeta(fileMeta, response),
      updatedAt: Date.now(),
    });
    this.progress('fetch-done', path);
    return json;
  }

  url(path) {
    return `${this.baseUrl}/${path}`;
  }

  progress(type, path) {
    this.onProgress?.({ type, path });
  }
}

function normalizeBaseUrl(baseUrl) {
  return baseUrl.replace(/\/+$/, '');
}

function manifestPathForFile(path) {
  const slash = path.lastIndexOf('/');
  return slash < 0 ? MANIFEST_PATH : `${path.slice(0, slash)}/manifest.json`;
}

function manifestKeyForFile(path) {
  const slash = path.lastIndexOf('/');
  return slash < 0 ? path : path.slice(slash + 1);
}

function manifestFileMeta(manifest, path) {
  return manifest?.files?.[path] ?? manifest?.files?.[manifestKeyForFile(path)];
}

function isHttpStatusError(error, status) {
  return error?.status === status;
}

function toPayloadKey(path) {
  switch (path) {
    case 'cards.json':
    case 'api/cards/all.5.json':
      return 'cards';
    case 'characters.json':
    case 'api/characters/main.3.json':
      return 'characters';
    case 'skills.json':
    case 'api/skills/all.10.json':
      return 'skills';
    case 'areaItems.json':
    case 'api/areaItems/main.5.json':
      return 'areaItems';
    case 'events.json':
    case 'api/events/all.6.json':
      return 'events';
    case 'songs.json':
    case 'api/songs/all.7.json':
      return 'songs';
    case 'cardsCNfix.json':
      return 'cardsFix';
    case 'skillsCNfix.json':
      return 'skillsFix';
    case 'areaItemFix.json':
      return 'areaItemsFix';
    case 'eventCharacterParameterBonusFix.json':
      return 'eventCharacterParameterBonusFix';
    default:
      return path.replace(/\.json$/, '');
  }
}

function applyEventCharacterParameterBonusFix(event, fix, eventId) {
  if (event.eventCharacterParameterBonus != null) {
    return event;
  }
  const fixedValue = fix?.[String(eventId)];
  if (fixedValue == null) {
    return event;
  }
  return {
    ...event,
    eventCharacterParameterBonus: fixedValue,
  };
}

function applyEventOverride(event, override) {
  if (!override || typeof override !== 'object') {
    return event;
  }
  return {
    ...event,
    ...override,
  };
}

function selectSongs(allSongs, songList) {
  const selected = {};
  for (const song of songList) {
    const key = String(song.songId);
    if (!allSongs[key]) {
      throw new Error(`song ${key} is missing`);
    }
    selected[key] = allSongs[key];
  }
  return selected;
}

function cardHasLevel(card, level) {
  return Boolean(card?.stat?.[String(level)]);
}

function mergeCardDetail(baseCard, detail) {
  if (!baseCard) {
    return detail;
  }
  if (!detail?.stat) {
    return baseCard;
  }
  return {
    ...baseCard,
    stat: detail.stat,
  };
}

function needsUpdate(local, fileMeta) {
  if (!fileMeta) {
    return false;
  }
  if (fileMeta.hash && local.meta?.hash !== fileMeta.hash) {
    return true;
  }
  if (fileMeta.etag && local.meta?.etag !== fileMeta.etag) {
    return true;
  }
  if (fileMeta.version && local.meta?.version !== fileMeta.version) {
    return true;
  }
  if (fileMeta.updatedAt && local.meta?.updatedAt !== fileMeta.updatedAt) {
    return true;
  }
  return false;
}

function normalizeFileMeta(fileMeta, response) {
  return {
    ...(fileMeta ?? {}),
    etag: fileMeta?.etag ?? response.headers.get('etag') ?? undefined,
    lastModified:
      fileMeta?.lastModified ?? response.headers.get('last-modified') ?? undefined,
  };
}

function openDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(FILE_STORE)) {
        db.createObjectStore(FILE_STORE, { keyPath: 'path' });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

function getRecord(db, path) {
  return requestToPromise(db.transaction(FILE_STORE).objectStore(FILE_STORE).get(path));
}

function putRecord(db, record) {
  return requestToPromise(
    db.transaction(FILE_STORE, 'readwrite').objectStore(FILE_STORE).put(record),
  );
}

function requestToPromise(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
