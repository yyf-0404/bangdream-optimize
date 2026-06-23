import { confirmDialog } from '../ui/confirm.js?v=2';
import { copyTextToClipboard } from '../ui/clipboard.js?v=2';

export function createCalculationActions({
  state,
  elements,
  readPlayer,
  writePlayer,
  savePlayerNow,
  readOptionalInteger,
  applyEventInputToPlayer,
  normalizeCurrentActivityForMode,
  ensureOwnedCardCharacterBonuses,
  ensureCore,
  renderConfigForms,
  renderResultSummary,
  renderMetrics,
  buildDiagnostic,
  diagnosticFileName,
  activatePage,
  setStatus,
  setError,
  eventLabel,
  resultCacheLimit = 20,
  renderResultCache,
  persistResultCache,
  clearPersistedResultCache,
}) {
  const RESULT_CACHE_KEY_VERSION = 2;
  const calculateButton = elements.calculateButton;
  const calculateButtons = Array.from(elements.calculateButtons || []);
  const calculateButtonLabel = calculateButton?.querySelector('.button-label');
  const calculateLabel = calculateButtonLabel?.textContent?.trim()
    || calculateButton?.textContent?.trim()
    || '计算';
  let isCalculating = false;
  const cacheLimit = Number.isInteger(resultCacheLimit) && resultCacheLimit > 0
    ? resultCacheLimit
    : 20;
  const persistCache = typeof persistResultCache === 'function'
    ? persistResultCache
    : async () => {};
  const clearPersisted = typeof clearPersistedResultCache === 'function'
    ? clearPersistedResultCache
    : async () => {};

  function readCurrentEventId(player, optionalEventId) {
    if (optionalEventId !== undefined) {
      return optionalEventId;
    }
    if (player.currentEvent == null) {
      throw new Error('未设置活动 ID');
    }
    return Number(player.currentEvent);
  }

  function makeResultCacheKey(player, eventId) {
    const serialized = cloneJson({
      cacheVersion: RESULT_CACHE_KEY_VERSION,
      server: player.server,
      activityMode: player.activityMode,
      eventId,
      eventSearch: player.eventSearch,
      currentEvent: player.currentEvent,
      cards: player.cardList,
      areas: player.areaItem,
      chars: player.characterBouns,
      bonuses: player.eventPresets,
      overrides: player.eventOverrides,
      songs: player.eventSongs,
      eventAttributeAndCharacterBonus: player.eventPresets?.[String(eventId)]?.eventAttributeAndCharacterBonus,
    });
    return JSON.stringify(serialized);
  }

  function getCachedResult(cacheKey) {
    const cache = state.resultCache || [];
    const index = cache.findIndex((entry) => entry.key === cacheKey);
    if (index < 0) {
      return undefined;
    }
    return cache[index];
  }

  async function setCachedResult(cacheKey, {
    player,
    eventId,
    result,
    diagnostic,
  }) {
    const cache = state.resultCache || [];
    const nextCache = cache.filter((entry) => entry.key !== cacheKey);
    nextCache.unshift({
      cacheVersion: RESULT_CACHE_KEY_VERSION,
      key: cacheKey,
      eventLabel: eventLabel(eventId, player),
      eventId,
      result: cloneJson(result),
      diagnostic: cloneJson(diagnostic),
      createdAt: Date.now(),
      accessedAt: Date.now(),
      server: player.server,
      activityMode: player.activityMode,
      totalScore: safeNumber(result?.totalScore),
      totalStat: safeNumber(result?.totalStat),
      songCount: safeInteger(result?.songs?.length),
    });
    if (nextCache.length > cacheLimit) {
      nextCache.length = cacheLimit;
    }
    state.resultCache = nextCache;
    await persistResultCacheState();
  }

  async function persistResultCacheState(activeKey = state.activeResultCacheKey) {
    try {
      await persistCache(state.resultCache || []);
    } catch (error) {
      setError(error);
    }
    renderResultCachePanel(activeKey);
  }

  function renderResultCachePanel(activeKey = state.activeResultCacheKey) {
    if (typeof renderResultCache === 'function') {
      renderResultCache(state.resultCache || [], { activeKey });
    }
  }

  function applyResult(result, diagnostic, cacheKey) {
    elements.result.textContent = JSON.stringify(result, null, 2);
    renderResultSummary(result);
    renderMetrics(result.metrics);
    state.lastDiagnostic = diagnostic;
    state.activeResultCacheKey = cacheKey;
    activatePage('result');
  }

  async function handleCalculate(event) {
    event.preventDefault();
    if (isCalculating) {
      return;
    }
    if (elements.form?.checkValidity && !elements.form.checkValidity()) {
      const firstInvalid = elements.form.querySelector('.is-invalid, :invalid');
      if (firstInvalid?.focus) {
        firstInvalid.focus();
      }
      return;
    }
    isCalculating = true;
    setCalculatingState(true);
    try {
      setStatus('同步数据');
      const player = readPlayer();
      applyEventInputToPlayer(player);
      const eventId = readCurrentEventId(player, readOptionalInteger(elements.eventId.value));
      const core = await ensureCore({ refreshManifest: true });
      normalizeCurrentActivityForMode(player);
      ensureOwnedCardCharacterBonuses(player);
      await savePlayerNow(player);
      writePlayer(player, { autosave: false });
      renderConfigForms(player);
      const cacheKey = makeResultCacheKey(player, eventId);
      state.activeResultCacheKey = cacheKey;
      const cached = getCachedResult(cacheKey);
      if (cached) {
        applyResult(cached.result, cached.diagnostic, cacheKey);
        renderResultCachePanel(cacheKey);
        setStatus('完成（缓存）');
        return;
      }

      setStatus('计算中');
      const result = await state.runtime.calculate({
        player,
        server: player.server,
        eventId,
        options: {},
        core,
      });
      const diagnostic = await buildDiagnostic({
        player,
        server: player.server,
        eventId,
        result,
      });
      await setCachedResult(cacheKey, {
        player,
        eventId,
        result,
        diagnostic,
      });
      applyResult(result, diagnostic, cacheKey);
      setStatus('完成');
    } catch (error) {
      setError(error);
    } finally {
      isCalculating = false;
      setCalculatingState(false);
    }
  }

  async function handleResultCacheAction(event) {
    const button = event.target.closest('[data-result-cache-action="restore"]');
    if (!button) {
      return;
    }
    const cacheKey = String(button.dataset.resultCacheKey || '').trim();
    if (!cacheKey) {
      return;
    }
    event.preventDefault();
    await handleRestoreResultCache(cacheKey);
  }

  async function handleRestoreResultCache(cacheKey) {
    try {
      state.activeResultCacheKey = cacheKey;
      const cached = getCachedResult(cacheKey);
      if (!cached) {
        setStatus('结果缓存未找到');
        return;
      }
      applyResult(cached.result, cached.diagnostic, cacheKey);
      renderResultCachePanel(cacheKey);
      setStatus('已恢复结果缓存');
    } catch (error) {
      setError(error);
    }
  }

  async function handleClearResultCache() {
    try {
      const confirmed = await confirmDialog({
        title: '清空结果缓存',
        lines: ['将删除最近保存的计算结果缓存。'],
        confirmText: '确认清空',
        danger: true,
      });
      if (!confirmed) {
        return;
      }
      state.resultCache = [];
      state.activeResultCacheKey = null;
      await clearPersisted();
      renderResultCachePanel(null);
      setStatus('结果缓存已清空');
    } catch (error) {
      setError(error);
    }
  }

  function safeNumber(value) {
    const number = Number(value);
    return Number.isFinite(number) ? number : undefined;
  }

  function safeInteger(value) {
    const number = Number(value);
    return Number.isInteger(number) ? number : undefined;
  }

  async function handleCopyResult() {
    try {
      if (!state.lastDiagnostic) {
        throw new Error('还没有可复制的 score_check 数据');
      }
      const payload = scoreCheckPayloadFromDiagnostic(state.lastDiagnostic);
      await copyTextToClipboard(JSON.stringify(payload, null, 2));
      setStatus('score_check JSON 已复制');
    } catch (error) {
      setError(error);
    }
  }

  async function handleExportDiagnostics() {
    try {
      if (!state.lastDiagnostic) {
        throw new Error('还没有可导出的诊断数据');
      }
      const fileName = diagnosticFileName(state.lastDiagnostic);
      const json = JSON.stringify(state.lastDiagnostic, null, 2);
      const result = await state.runtime.saveJsonFile({ fileName, text: json });
      if (result === 'cancelled') {
        setStatus('已取消导出诊断');
      } else if (result === 'downloaded') {
        setStatus('诊断已下载');
      } else {
        setStatus('诊断已导出');
      }
    } catch (error) {
      setError(error);
    }
  }

  function setCalculatingState(isBusy) {
    const buttons = calculateButtons.length > 0
      ? calculateButtons
      : [calculateButton].filter(Boolean);
    if (buttons.length === 0) {
      return;
    }
    for (const button of buttons) {
      button.disabled = isBusy;
      button.classList.toggle('is-loading', isBusy);
      button.setAttribute('aria-busy', isBusy ? 'true' : 'false');
      const label = button.querySelector('.button-label');
      if (label) {
        label.textContent = isBusy ? '计算中' : calculateLabel;
      } else {
        button.textContent = isBusy ? '计算中' : calculateLabel;
      }
    }
  }

  return {
    handleCalculate,
    handleCopyResult,
    handleExportDiagnostics,
    handleResultCacheAction,
    handleClearResultCache,
  };
}

export function scoreCheckPayloadFromDiagnostic(diagnostic) {
  const result = diagnostic?.result;
  const player = diagnostic?.player;
  if (!result || !player) {
    throw new Error('诊断数据缺少结果或玩家配置');
  }
  if (!result.items) {
    throw new Error('当前结果没有道具选择，无法生成 score_check 数据');
  }

  const eventId = firstDefinedNumber(
    diagnostic.eventId,
    player.currentEvent,
    result.eventId,
  );
  const eventKey = String(eventId);
  const eventKeys = scoreCheckEventKeys(diagnostic, player, result);
  const songs = scoreCheckSongs(result);

  return {
    server: diagnostic.server,
    eventId,
    result: scoreCheckResult(result),
    player: {
      playerId: Number.isFinite(Number(player.playerId)) ? Number(player.playerId) : 0,
      currentEvent: eventId,
      eventSongs: {
        [eventKey]: songs,
      },
      eventPresets: pickObjectKeys(player.eventPresets, eventKeys),
      eventOverrides: pickObjectKeys(player.eventOverrides, eventKeys),
      cardList: pickObjectKeys(player.cardList, scoreCheckCardIds(result)),
      areaItem: cloneJson(player.areaItem ?? {}),
      characterBouns: cloneJson(player.characterBouns ?? {}),
    },
  };
}

function scoreCheckResult(result) {
  return {
    eventId: result.eventId,
    eventType: result.eventType,
    totalScore: result.totalScore,
    totalStat: result.totalStat,
    songs: (result.songs ?? []).map((song) => ({
      songId: song.songId,
      difficulty: song.difficulty,
      score: song.score,
      stat: song.stat,
      teamCardIds: [...(song.teamCardIds ?? [])],
      captainCardId: song.captainCardId,
    })),
    items: cloneJson(result.items),
  };
}

function scoreCheckSongs(result) {
  return (result.songs ?? []).map((song) => ({
    songId: song.songId,
    difficulty: song.difficulty,
  }));
}

function scoreCheckCardIds(result) {
  const cardIds = new Set();
  for (const song of result.songs ?? []) {
    for (const cardId of song.teamCardIds ?? []) {
      cardIds.add(String(cardId));
    }
  }
  return cardIds;
}

function scoreCheckEventKeys(diagnostic, player, result) {
  const keys = new Set();
  for (const value of [diagnostic?.eventId, player?.currentEvent, result?.eventId]) {
    const number = firstDefinedNumber(value);
    if (number != null) {
      keys.add(String(number));
    }
  }
  return keys;
}

function pickObjectKeys(source, keys) {
  const picked = {};
  if (!source || typeof source !== 'object') {
    return picked;
  }
  for (const key of keys) {
    if (Object.prototype.hasOwnProperty.call(source, key)) {
      picked[key] = cloneJson(source[key]);
    }
  }
  return picked;
}

function firstDefinedNumber(...values) {
  for (const value of values) {
    if (value == null || value === '') {
      continue;
    }
    const number = Number(value);
    if (Number.isFinite(number)) {
      return number;
    }
  }
  return undefined;
}

function cloneJson(value) {
  return value == null ? value : JSON.parse(JSON.stringify(value));
}
