import { SCORING_REVISION } from '../domain/scoring-revision.js';
import { isCompleteTimingResult } from '../domain/hit-timing.js';
import { totalFireCost } from '../shared/utils.js';

export { SCORING_REVISION };

const DB_NAME = 'bangdream-optimize-result-cache-v1';
const DB_VERSION = 1;
const STORE = 'result-cache';
const CACHE_KEY = 'entries';
export const RESULT_CACHE_SCHEMA_VERSION = 16;
// Input keys are unchanged. Only older single-song timing results need a new search.
export const RESULT_CACHE_KEY_VERSION = 15;
export const RESULT_CACHE_LIMIT = 20;

export function resultCacheInvalidationReason(entry) {
  const timing = entry?.result?.hitTiming;
  if (timing?.rangeRestrictions != null) return '此结果含自定义击打范围，需另行计算无限制最高分';
  if (timing && !isCompleteTimingResult(entry.result)) {
    return '此历史结果的击打记录不完整或模型不兼容，需重新计算';
  }
  if (timing?.songs?.length === 1 && entry.cacheVersion < 16) {
    return '单曲击打优化的最高分搜索已修正，需重新计算';
  }
  // Old 5v5 snapshots include teammateScores in the request/result scenario.
  // Keep them readable, but never reuse the inflated PT after this correction.
  const festivalInputs = [
    entry?.diagnostic?.calculationRequest?.festival,
    entry?.result?.scenario?.festival,
  ];
  if (festivalInputs.some(input => input && Object.hasOwn(input, 'teammateScores'))) {
    return '5v5 PT 计算已修正，需重新计算';
  }
  if (entry && (entry.scoringRevision ?? entry.result?.scoringRevision ?? entry.result?.[0]?.scoringRevision) !== SCORING_REVISION) {
    return '此历史结果使用旧版计分规则，需重新计算';
  }
  if (usesPreviousPowerDisplay(entry)) {
    return '综合力显示规则已更新，需重新计算';
  }
  return entry?.reusable === false ? '游戏数据已更新，需重新计算' : '';
}

function usesPreviousPowerDisplay(entry) {
  const version = Number(entry?.cacheVersion);
  // The earlier timing builds had the same scoring revision but older display logic.
  if (version === 13 || version === 14) return true;
  if (![9, 10, 11].includes(version)) return false;
  if (entry.calculationMode === 'scoreRange') return entry.result?.[0]?.eventType === 'versus';
  if (version === 11) return false;
  const { eventType, liveVariant } = entry.result ?? {};
  if (entry.calculationMode === 'maximize') {
    return eventType === 'challenge' || (version === 9 && eventType === 'versus');
  }
  if (version !== 9 || (entry.calculationMode !== 'ptMaximize' && entry.calculationMode !== 'ptEvaluate')) return false;
  return eventType === 'versus' || eventType === 'festival'
    || (eventType === 'challenge' && (liveVariant === 'solo' || liveVariant === 'cooperative'));
}

export function resultTotalStat(result) {
  const value = result?.totalStat
    ?? result?.team?.totalStat
    ?? result?.medley?.totalStat
    ?? result?.[0]?.totalStat;
  return value == null ? undefined : safeNumber(value);
}

export function createResultCacheStorage({ limit = RESULT_CACHE_LIMIT } = {}) {
  const resultCacheLimit = Number.isInteger(limit) && limit > 0 ? limit : RESULT_CACHE_LIMIT;

  async function loadResultCache() {
    const db = await openDatabase();
    const result = await getValue(db, CACHE_KEY);
    const normalized = boundedEntries(result);
    // Preserve supported history and its original scoring revision; only malformed entries are removed.
    if (Array.isArray(result) && normalized.length !== result.length) await putValue(db, CACHE_KEY, normalized);
    return normalized;
  }

  async function saveResultCache(entries) {
    const db = await openDatabase();
    await putValue(db, CACHE_KEY, boundedEntries(entries));
  }

  async function clearResultCache() {
    const db = await openDatabase();
    await deleteValue(db, CACHE_KEY);
  }

  function boundedEntries(entries) {
    const counts = new Map();
    return normalizeEntries(entries).filter(entry => {const n=(counts.get(entry.profileId)||0)+1;counts.set(entry.profileId,n);return n<=resultCacheLimit;});
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
  const cacheVersion = Number(entry.cacheVersion);
  if (!key || !entry.profileId || !Number.isInteger(cacheVersion)
    || cacheVersion < 7 || cacheVersion > RESULT_CACHE_SCHEMA_VERSION) {
    return undefined;
  }
  const calculationMode = ['scoreRange', 'ptMaximize', 'ptEvaluate'].includes(entry.calculationMode)
    ? entry.calculationMode
    : 'maximize';
  const result = cloneJson(entry.result);
  const createdAt = safeNumber(entry.createdAt, Date.now());
  return {
    cacheVersion,
    scoringRevision: entry.scoringRevision ?? result?.scoringRevision ?? result?.[0]?.scoringRevision,
    reusable: !resultCacheInvalidationReason(entry),
    profileId: String(entry.profileId),
    key,
    eventId: Number(entry.eventId) || 0,
    eventLabel: typeof entry.eventLabel === 'string' ? entry.eventLabel : `活动 ${Number(entry.eventId) || 0}`,
    server: typeof entry.server === 'string' ? entry.server : '',
    calculationMode,
    activityMode: typeof entry.activityMode === 'string' ? entry.activityMode : 'medley',
    totalScore: safeNumber(entry.totalScore),
    totalStat: resultTotalStat(result) ?? (result?.medley ? undefined : safeNumber(entry.totalStat)),
    songCount: safeInteger(entry.songCount),
    targetDeltaPt: safeNumber(entry.targetDeltaPt),
    playCount: safeInteger(entry.playCount),
    totalFireCost: safeInteger(entry.totalFireCost)
      ?? safeInteger(result?.[0]?.totalFireCost)
      ?? (calculationMode === 'scoreRange' ? totalFireCost(result?.[0]?.plays) : undefined),
    averagePt: safeNumber(entry.averagePt) ?? averagePtFromResult(result),
    averageScore: safeNumber(entry.averageScore) ?? averageScoreFromResult(result),
    createdAt,
    accessedAt: Math.max(createdAt, safeNumber(entry.accessedAt, createdAt)),
    result,
    diagnostic: cloneJson(entry.diagnostic),
  };
}

function averagePtFromResult(result) {
  const average = result?.team?.evaluation?.averagePt ?? result?.medley?.averagePt;
  const numerator = safeNumber(average?.ptSum);
  const denominator = safeNumber(average?.sampleCount);
  return numerator != null && denominator > 0 ? numerator / denominator : undefined;
}

function averageScoreFromResult(result) {
  const distribution = result?.team?.evaluation?.scoreDistribution;
  if (distribution) {
    return safeAverage(distribution.scoreSum, distribution.sampleCount);
  }
  return safeAverage(result?.medley?.totalScoreSum, result?.medley?.sampleCount);
}

function safeAverage(sum, count) {
  const numerator = safeNumber(sum);
  const denominator = safeNumber(count);
  return numerator != null && denominator > 0 ? numerator / denominator : undefined;
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
