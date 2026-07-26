import {
  PLAYER_CONFIG_SCHEMA_VERSION,
  createDefaultPtMaximizeConfig,
  createDefaultScoreRangeConfig,
} from '../models/player-settings.js?v=3';

const DB_NAME = 'bangdream-optimize-user-data-v1';
const LEGACY_DB_NAME = 'bangdream-optimize-user-data';
const DB_VERSION = 1;
const STORE = 'settings';
const LEGACY_PLAYER_KEY = 'player-config';
const PLAYER_PROFILE_PREFIX = 'player-config:';
const PLAYER_PROFILES_KEY = 'player-config-profiles';
const ACTIVE_PLAYER_PROFILE_KEY = 'active-player-config-id';
const DEFAULT_PROFILE_ID = 'default';

export async function loadPlayerConfig() {
  const db = await openDatabase();
  const { activeId } = await ensurePlayerProfiles(db);
  return (await getValue(db, playerProfileKey(activeId))) ?? samplePlayerConfig();
}

export async function savePlayerConfig(player) {
  const db = await openDatabase();
  const { activeId } = await ensurePlayerProfiles(db);
  await putValue(db, playerProfileKey(activeId), player);
  await touchPlayerProfile(db, activeId);
}

export async function listPlayerConfigs() {
  const db = await openDatabase();
  return ensurePlayerProfiles(db);
}

export async function selectPlayerConfig(id) {
  const db = await openDatabase();
  const { profiles } = await ensurePlayerProfiles(db);
  if (!profiles.some((profile) => profile.id === id)) {
    throw new Error(`配置不存在：${id}`);
  }
  await putValue(db, ACTIVE_PLAYER_PROFILE_KEY, id);
  return (await getValue(db, playerProfileKey(id))) ?? samplePlayerConfig();
}

export async function createPlayerConfig({ name, player } = {}) {
  const db = await openDatabase();
  const { profiles } = await ensurePlayerProfiles(db);
  const profile = newPlayerProfile(name);
  profiles.push(profile);
  await putValue(db, PLAYER_PROFILES_KEY, profiles);
  await putValue(db, playerProfileKey(profile.id), player ?? samplePlayerConfig());
  await putValue(db, ACTIVE_PLAYER_PROFILE_KEY, profile.id);
  return profile;
}

export async function duplicatePlayerConfig({ name, player } = {}) {
  return createPlayerConfig({
    name,
    player: player ?? await loadPlayerConfig(),
  });
}

export async function renamePlayerConfig(id, name) {
  const db = await openDatabase();
  const { profiles } = await ensurePlayerProfiles(db);
  const profile = profiles.find((entry) => entry.id === id);
  if (!profile) {
    throw new Error(`配置不存在：${id}`);
  }
  profile.name = normalizeProfileName(name, profile.name);
  profile.updatedAt = Date.now();
  await putValue(db, PLAYER_PROFILES_KEY, profiles);
  return profile;
}

export async function deletePlayerConfig(id) {
  const db = await openDatabase();
  const { profiles, activeId } = await ensurePlayerProfiles(db);
  if (profiles.length <= 1) {
    throw new Error('至少保留一份配置');
  }
  const nextProfiles = profiles.filter((profile) => profile.id !== id);
  if (nextProfiles.length === profiles.length) {
    throw new Error(`配置不存在：${id}`);
  }
  const nextActiveId = activeId === id ? nextProfiles[0].id : activeId;
  await deleteValue(db, playerProfileKey(id));
  await putValue(db, PLAYER_PROFILES_KEY, nextProfiles);
  await putValue(db, ACTIVE_PLAYER_PROFILE_KEY, nextActiveId);
  return (await getValue(db, playerProfileKey(nextActiveId))) ?? samplePlayerConfig();
}

export async function clearPlayerConfigCache() {
  const db = await openDatabase();
  await clearStore(db);
  await deleteDatabase(LEGACY_DB_NAME);
  await ensurePlayerProfiles(db);
}

export function samplePlayerConfig() {
  return {
    playerConfigVersion: PLAYER_CONFIG_SCHEMA_VERSION,
    playerId: 0,
    server: 'cn',
    currentEvent: undefined,
    calculationMode: 'ptMaximize',
    activityMode: 'single',
    scoreRange: createDefaultScoreRangeConfig('cn'),
    ptMaximize: createDefaultPtMaximizeConfig(),
    eventSongs: {},
    eventPresets: {},
    eventOverrides: {},
    cardList: {},
    areaItem: {},
    characterBouns: {},
  };
}

async function ensurePlayerProfiles(db) {
  const storedProfiles = await getValue(db, PLAYER_PROFILES_KEY);
  if (Array.isArray(storedProfiles)) {
    const profiles = storedProfiles
      .map(normalizedPlayerProfile)
      .filter((profile) => profile.id);
    if (profiles.length === 0) {
      const legacyPlayer = (await getValue(db, LEGACY_PLAYER_KEY))
        ?? await takeLegacyPlayerConfig();
      return createDefaultPlayerProfile(db, legacyPlayer);
    }
    await deleteValue(db, LEGACY_PLAYER_KEY);
    await deleteDatabase(LEGACY_DB_NAME);
    await putValue(db, PLAYER_PROFILES_KEY, profiles);
    let activeId = await getValue(db, ACTIVE_PLAYER_PROFILE_KEY);
    if (!profiles.some((profile) => profile.id === activeId)) {
      activeId = profiles[0].id;
      await putValue(db, ACTIVE_PLAYER_PROFILE_KEY, activeId);
    }
    if (await getValue(db, playerProfileKey(activeId)) == null) {
      await putValue(db, playerProfileKey(activeId), samplePlayerConfig());
    }
    return { profiles, activeId };
  }

  const legacyPlayer = (await getValue(db, LEGACY_PLAYER_KEY))
    ?? await takeLegacyPlayerConfig();
  return createDefaultPlayerProfile(db, legacyPlayer);
}

async function createDefaultPlayerProfile(db, player = samplePlayerConfig()) {
  const profile = {
    id: DEFAULT_PROFILE_ID,
    name: '默认配置',
    updatedAt: Date.now(),
  };
  await putValue(db, PLAYER_PROFILES_KEY, [profile]);
  await putValue(db, playerProfileKey(profile.id), player);
  await putValue(db, ACTIVE_PLAYER_PROFILE_KEY, profile.id);
  await deleteValue(db, LEGACY_PLAYER_KEY);
  await deleteDatabase(LEGACY_DB_NAME);
  return { profiles: [profile], activeId: profile.id };
}

async function takeLegacyPlayerConfig() {
  const db = await openNamedDatabase(LEGACY_DB_NAME);
  let player;
  if (db.objectStoreNames.contains(STORE)) {
    player = await getValue(db, LEGACY_PLAYER_KEY);
  }
  db.close();
  await deleteDatabase(LEGACY_DB_NAME);
  return player;
}

async function touchPlayerProfile(db, id) {
  const profiles = (await getValue(db, PLAYER_PROFILES_KEY)) ?? [];
  const profile = profiles.find((entry) => entry.id === id);
  if (!profile) {
    return;
  }
  profile.updatedAt = Date.now();
  await putValue(db, PLAYER_PROFILES_KEY, profiles.map(normalizedPlayerProfile));
}

function newPlayerProfile(name) {
  return {
    id: `cfg-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
    name: normalizeProfileName(name, '新配置'),
    updatedAt: Date.now(),
  };
}

function normalizedPlayerProfile(profile) {
  if (!profile?.id) {
    return { id: '', name: '', updatedAt: 0 };
  }
  return {
    id: String(profile.id),
    name: normalizeProfileName(profile?.name, '未命名配置'),
    updatedAt: Number(profile?.updatedAt) || 0,
  };
}

function normalizeProfileName(value, fallback) {
  const name = String(value ?? '').trim();
  return name || fallback;
}

function playerProfileKey(id) {
  return `${PLAYER_PROFILE_PREFIX}${id}`;
}

function openDatabase() {
  return openNamedDatabase(DB_NAME, {
    version: DB_VERSION,
    createStore: true,
  });
}

function openNamedDatabase(name, {
  version,
  createStore = false,
} = {}) {
  return new Promise((resolve, reject) => {
    const request = version == null
      ? indexedDB.open(name)
      : indexedDB.open(name, version);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (createStore && !db.objectStoreNames.contains(STORE)) {
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

function clearStore(db) {
  return requestToPromise(
    db.transaction(STORE, 'readwrite').objectStore(STORE).clear(),
  );
}

function deleteDatabase(name) {
  return new Promise((resolve, reject) => {
    const request = indexedDB.deleteDatabase(name);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
    request.onblocked = () => resolve();
  });
}

function requestToPromise(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
