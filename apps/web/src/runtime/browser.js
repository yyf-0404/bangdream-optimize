import { createGameDataClient } from '../data/game-sync.js?v=1';
import {
  clearPlayerConfigCache,
  createPlayerConfig,
  deletePlayerConfig,
  duplicatePlayerConfig,
  listPlayerConfigs,
  loadPlayerConfig,
  renamePlayerConfig,
  samplePlayerConfig,
  savePlayerConfig,
  selectPlayerConfig,
} from '../storage/user.js?v=1';

const WASM_ASSET_VERSION = '20260615-current-core';

export async function createBrowserRuntime({ onProgress } = {}) {
  const config = readRuntimeConfig();
  const gameData = createGameDataClient({
    baseUrl: config.gameDataBaseUrl ?? '/game-data',
    onProgress,
  });
  let wasmPromise = null;
  const getWasm = () => {
    wasmPromise ??= loadWasm();
    return wasmPromise;
  };
  const calculationWorker = createCalculationWorkerClient({
    calculateOnMainThread: async (payloadJson) => {
      const wasm = await getWasm();
      return wasm.calculateFromStaticData(payloadJson);
    },
  });

  return {
    kind: 'browser',
    samplePlayerConfig,
    loadPlayerConfig,
    savePlayerConfig,
    listPlayerConfigs,
    selectPlayerConfig,
    createPlayerConfig,
    duplicatePlayerConfig,
    renamePlayerConfig,
    deletePlayerConfig,
    clearLocalCache: clearPlayerConfigCache,
    importBestdoriPlayerProfile: ({ playerId, server, mode = 3 }) =>
      fetchBestdoriPlayerProfile([config.apiBaseUrl], { playerId, server, mode }),
    clearGameCache: () => gameData.clearCache(),
    runtimeInfo: async () => ({
      runtime: 'browser',
      gameDataBaseUrl: config.gameDataBaseUrl ?? '/game-data',
      apiBaseUrl: config.apiBaseUrl,
    }),
    syncEventData: (eventId) => gameData.syncEvent(eventId),
    syncReferenceData: ({ refreshManifest = false } = {}) =>
      gameData.syncCore({ refreshManifest }),
    saveJsonFile: ({ fileName, text }) => {
      downloadJsonFile(fileName, text);
      return 'downloaded';
    },
    calculate: async ({ player, server, eventId, options, core }) => {
      const payload = await gameData.buildCalculationPayload({
        player,
        server,
        eventId,
        options,
        core,
      });
      const resultJson = await calculationWorker.calculate(JSON.stringify(payload));
      return JSON.parse(resultJson);
    },
  };
}

function createCalculationWorkerClient({ calculateOnMainThread }) {
  let worker = null;
  let nextId = 1;
  let disabled = false;
  const pending = new Map();

  function calculate(payloadJson) {
    const activeWorker = ensureWorker();
    if (!activeWorker) {
      return calculateOnMainThread(payloadJson);
    }

    const id = nextId;
    nextId += 1;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      activeWorker.postMessage({
        id,
        type: 'calculate',
        payloadJson,
      });
    });
  }

  function ensureWorker() {
    if (disabled || typeof Worker !== 'function') {
      return null;
    }
    if (worker) {
      return worker;
    }
    try {
      worker = new Worker(
        new URL(`./browser-worker.js?v=${WASM_ASSET_VERSION}`, import.meta.url),
        { type: 'module' },
      );
      worker.addEventListener('message', handleWorkerMessage);
      worker.addEventListener('error', handleWorkerFailure);
      return worker;
    } catch {
      disabled = true;
      worker = null;
      return null;
    }
  }

  function handleWorkerMessage(event) {
    const { id, ok, resultJson, error } = event.data ?? {};
    const request = pending.get(id);
    if (!request) {
      return;
    }
    pending.delete(id);
    if (ok) {
      request.resolve(resultJson);
    } else {
      request.reject(new Error(error || '计算 Worker 执行失败'));
    }
  }

  function handleWorkerFailure(event) {
    disabled = true;
    if (worker) {
      worker.terminate();
      worker = null;
    }
    const error = new Error(event.message || '计算 Worker 加载失败');
    for (const { reject } of pending.values()) {
      reject(error);
    }
    pending.clear();
  }

  return { calculate };
}

function downloadJsonFile(fileName, text) {
  const blob = new Blob([text], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName;
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

async function fetchBestdoriPlayerProfile(apiBaseUrls, { playerId, server, mode }) {
  const configuredBase = normalizeApiBase(apiBaseUrls?.[0]);
  const candidates = configuredBase ? [configuredBase] : [''];
  const errors = [];
  for (const base of candidates) {
    const normalizedBase = normalizeApiBase(base);
    const url = `${normalizedBase}/bestdori/player/${server}/${playerId}?mode=${mode}`;
    try {
      const response = await fetch(url, { cache: 'no-cache' });
      if (!response.ok) {
        let detail = '';
        try {
          const text = await response.text();
          if (text) {
            detail = `: ${text.slice(0, 200)}`;
          }
        } catch {
          // ignore parse errors
        }
        throw new Error(`HTTP ${response.status}${detail}`);
      }
      const payload = await response.json();
      if (!payload?.result || payload?.data?.profile == null) {
        throw new Error('Bestdori 没有返回玩家资料');
      }
      return payload.data.profile;
    } catch (error) {
      errors.push(`${url}: ${error.message}`);
    }
  }
  const message = errors.length
    ? `主乐队导入失败：${errors.join('；')}`
    : '主乐队导入失败';
  throw new Error(message);
}

function normalizeApiBase(baseUrl) {
  const normalized = String(baseUrl ?? '').trim().replace(/\/$/, '');
  if (normalized === 'undefined' || normalized === 'null') {
    return '';
  }
  return normalized === '' ? '' : normalized;
}

async function loadWasm() {
  try {
    const module = await import(`../../pkg/bangdream_optimize_web_wasm.js?v=${WASM_ASSET_VERSION}`);
    await module.default(new URL(
      `../../pkg/bangdream_optimize_web_wasm_bg.wasm?v=${WASM_ASSET_VERSION}`,
      import.meta.url,
    ));
    return module;
  } catch (error) {
    throw new Error(
      `WASM 未构建或无法加载：${error.message}. 请先生成 apps/web/pkg。`,
    );
  }
}

function readRuntimeConfig() {
  return globalThis.BANGDREAM_OPTIMIZE_CONFIG ?? {};
}
