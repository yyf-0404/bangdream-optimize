const DB_NAME = 'bangdream-optimize-result-cache-v1';
const DB_VERSION = 1;
const STORE = 'result-cache';
const CACHE_KEY = 'entries';

export const RESULT_CACHE_LIMIT = 20;

export function createResultCacheStorage({ limit = RESULT_CACHE_LIMIT } = {}) {
  const resultCacheLimit = Number.isInteger(limit) && limit > 0 ? limit : RESULT_CACHE_LIMIT;

  async function loadResultCache() {
    const db = await openDatabase();
    const result = await getValue(db, CACHE_KEY);
    return normalizeEntries(result).slice(0, resultCacheLimit);
  }

  async function saveResultCache(entries) {
    const db = await openDatabase();
    await putValue(db, CACHE_KEY, normalizeEntries(entries).slice(0, resultCacheLimit));
  }

  async function clearResultCache() {
    const db = await openDatabase();
    await deleteValue(db, CACHE_KEY);
  }

  return {
    resultCacheLimit,
    loadResultCache,
    saveResultCache,
    clearResultCache,
  };
}

function normalizeEntries(entries) {
  return Array.isArray(entries)
    ? [...entries]
      .map(normalizeEntry)
      .filter(Boolean)
      .sort((a, b) => (Number(b.createdAt) || 0) - (Number(a.createdAt) || 0))
    : [];
}

function normalizeEntry(entry) {
  if (!entry || typeof entry !== 'object') {
    return undefined;
  }
  const key = String(entry.key || '').trim();
  if (!key) {
    return undefined;
  }
  return {
    key,
    eventId: Number(entry.eventId) || 0,
    eventLabel: typeof entry.eventLabel === 'string' ? entry.eventLabel : `活动 ${Number(entry.eventId) || 0}`,
    server: typeof entry.server === 'string' ? entry.server : '',
    activityMode: typeof entry.activityMode === 'string' ? entry.activityMode : 'medley',
    totalScore: safeNumber(entry.totalScore),
    totalStat: safeNumber(entry.totalStat),
    songCount: safeInteger(entry.songCount),
    createdAt: safeNumber(entry.createdAt, Date.now()),
    accessedAt: safeNumber(entry.accessedAt, Date.now()),
    result: cloneJson(entry.result),
    diagnostic: cloneJson(entry.diagnostic),
  };
}

function safeNumber(value, fallback) {
  const number = Number(value);
  if (Number.isFinite(number)) {
    return number;
  }
  return fallback;
}

function safeInteger(value) {
  const number = Number(value);
  return Number.isInteger(number) ? number : undefined;
}

function cloneJson(value) {
  return value == null ? value : JSON.parse(JSON.stringify(value));
}

function openDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE)) {
        db.createObjectStore(STORE);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

function getValue(db, key) {
  return requestToPromise(db.transaction(STORE).objectStore(STORE).get(key));
}

function putValue(db, key, value) {
  return requestToPromise(
    db.transaction(STORE, 'readwrite').objectStore(STORE).put(value, key),
  );
}

function deleteValue(db, key) {
  return requestToPromise(
    db.transaction(STORE, 'readwrite').objectStore(STORE).delete(key),
  );
}

function requestToPromise(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
