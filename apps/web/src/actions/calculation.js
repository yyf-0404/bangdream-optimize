import { confirmDialog } from '../ui/confirm.js?v=3';
import { copyTextToClipboard } from '../ui/clipboard.js?v=3';
import { totalFireCost } from '../utils.js?v=3';
import {
  ptMaximizeLiveVariant,
  withPtMaximizeLiveVariant,
} from '../models/player-settings.js?v=3';

export function createCalculationActions({
  state,
  elements,
  readPlayer,
  writePlayer,
  savePlayerNow,
  readOptionalInteger,
  applyEventInputToPlayer,
  editableEventSnapshot,
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
  const RESULT_CACHE_KEY_VERSION = 4;
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
      calculationMode: player.calculationMode,
      activityMode: player.activityMode,
      scoreRange: player.scoreRange,
      ptMaximize: player.ptMaximize,
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
      calculationMode: player.calculationMode,
      activityMode: player.activityMode,
      totalScore: safeNumber(result?.totalScore),
      totalStat: safeNumber(
        result?.totalStat
          ?? result?.team?.totalStat
          ?? result?.medley?.teams?.reduce((sum, team) => sum + Number(team.totalStat || 0), 0)
          ?? result?.[0]?.totalStat,
      ),
      songCount: safeInteger(result?.songs?.length ?? result?.[0]?.distinctSongCount),
      targetDeltaPt: safeNumber(result?.[0]?.targetDeltaPt),
      playCount: safeInteger(result?.[0]?.playCount),
      totalFireCost: Array.isArray(result)
        ? safeInteger(result[0]?.totalFireCost) ?? totalFireCost(result[0]?.plays)
        : undefined,
      averagePt: result?.team?.evaluation?.averagePt
        ? Number(result.team.evaluation.averagePt.ptSum)
          / Number(result.team.evaluation.averagePt.sampleCount)
        : result?.medley?.averagePt
          ? Number(result.medley.averagePt.ptSum)
            / Number(result.medley.averagePt.sampleCount)
          : undefined,
      averageScore: ptMaximizeAverageScore(result),
    });
    if (nextCache.length > cacheLimit) {
      nextCache.length = cacheLimit;
    }
    state.resultCache = nextCache;
    try {
      await persistResultCacheState();
      return true;
    } catch (error) {
      console.warn(`result-cache-save-error: ${error?.message ?? String(error)}`);
      return false;
    }
  }

  async function persistResultCacheState(activeKey = state.activeResultCacheKey) {
    try {
      await persistCache(state.resultCache || []);
    } finally {
      renderResultCachePanel(activeKey);
    }
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

  function applyFailureDiagnostic(diagnostic) {
    elements.result.textContent = JSON.stringify(diagnostic, null, 2);
    renderResultSummary(null, { diagnostic });
    renderMetrics(null);
    state.lastDiagnostic = diagnostic;
    state.activeResultCacheKey = null;
    renderResultCachePanel(null);
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
      applyScoreRangeInputToPlayer(player);
      applyPtMaximizeInputToPlayer(player);
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

      const scoreRangeRequest = player.calculationMode === 'scoreRange'
        ? readScoreRangeRequest()
        : undefined;
      const ptMaximizeRequest = player.calculationMode === 'ptMaximize'
        ? readPtMaximizeRequest(player, eventId)
        : undefined;
      setStatus('计算中');
      let result;
      try {
        result = player.calculationMode === 'scoreRange'
          ? await calculateScoreRange({
            player,
            eventId,
            core,
            request: scoreRangeRequest,
          })
          : player.calculationMode === 'ptMaximize'
            ? await calculatePtMaximize({
              player,
              eventId,
              core,
              request: ptMaximizeRequest,
            })
          : await state.runtime.calculate({
            player,
            server: player.server,
            eventId,
            options: {},
            core,
          });
      } catch (error) {
        const diagnostic = await buildDiagnostic({
          player,
          server: player.server,
          eventId,
          error,
          phase: 'calculation',
        });
        applyFailureDiagnostic(diagnostic);
        setStatus('计算失败，已生成诊断');
        return;
      }
      const diagnostic = await buildDiagnostic({
        player,
        server: player.server,
        eventId,
        result,
      });
      const resultCacheSaved = await setCachedResult(cacheKey, {
        player,
        eventId,
        result,
        diagnostic,
      });
      applyResult(result, diagnostic, cacheKey);
      setStatus(resultCacheSaved ? '完成' : '完成（结果缓存保存失败）');
    } catch (error) {
      setError(error);
    } finally {
      isCalculating = false;
      setCalculatingState(false);
    }
  }

  async function calculateScoreRange({ player, eventId, core, request }) {
    if (typeof state.runtime.scoreRange !== 'function') {
      throw new Error('当前运行时不支持目标 PT 搜索');
    }
    return state.runtime.scoreRange({
      player,
      server: player.server,
      eventId,
      request,
      core,
    });
  }

  async function calculatePtMaximize({ player, eventId, core, request }) {
    if (typeof state.runtime.ptMaximize !== 'function') {
      throw new Error('当前运行时不支持最大PT（平均）搜索');
    }
    return state.runtime.ptMaximize({
      player,
      server: player.server,
      eventId,
      request,
      core,
    });
  }

  function applyScoreRangeInputToPlayer(player) {
    player.scoreRange = readScoreRangeForm({ strict: false });
  }

  function readScoreRangeRequest() {
    const request = readScoreRangeForm({ strict: true });
    if (request.targetTotalPt <= request.currentPt) {
      throw new Error('目标总 PT 必须大于当前 PT');
    }
    if (
      elements.scoreRangeMissionSupportPt.required
      && request.missionSupportPtBonus == null
    ) {
      throw new Error('Mission Live 必须填写支援 PT 加成');
    }
    return request;
  }

  function readScoreRangeForm({ strict }) {
    const currentPt = readFormInteger(
      elements.scoreRangeCurrentPt,
      '当前 PT',
      { fallback: 0, strict },
    );
    const targetTotalPt = readFormInteger(
      elements.scoreRangeTargetTotalPt,
      '目标总 PT',
      { fallback: 0, strict },
    );
    const autoBaseMultiplier = Number(elements.scoreRangeAutoBaseMultiplier?.value);
    if (![0.5, 0.75].includes(autoBaseMultiplier)) {
      throw new Error('Auto 倍率必须为 0.5 或 0.75');
    }
    const missionSupportPtBonus = readFormInteger(
      elements.scoreRangeMissionSupportPt,
      '支援 PT 加成',
      { optional: true, strict },
    );
    return {
      eventType: 'festival',
      currentPt,
      targetTotalPt,
      autoBaseMultiplier,
      missionSupportPtBonus,
      maxResults: 1,
    };
  }

  function readFormInteger(input, label, {
    fallback,
    optional = false,
    strict = true,
  } = {}) {
    const value = String(input?.value ?? '').trim();
    if (!value) {
      if (optional) {
        return undefined;
      }
      if (!strict) {
        return fallback;
      }
      throw new Error(`${label}不能为空`);
    }
    if (!/^\d+$/.test(value)) {
      if (!strict) {
        return fallback;
      }
      throw new Error(`${label}需为非负整数`);
    }
    const number = Number(value);
    if (!Number.isSafeInteger(number) || number < 0) {
      if (!strict) {
        return fallback;
      }
      throw new Error(`${label}需为非负整数`);
    }
    return number;
  }

  function handleScoreRangeInputChange() {
    try {
      const player = readPlayer();
      applyScoreRangeInputToPlayer(player);
      writePlayer(player);
    } catch (error) {
      setError(error);
    }
  }

  function applyPtMaximizeInputToPlayer(player) {
    player.ptMaximize = readPtMaximizeForm({
      strict: false,
      config: player.ptMaximize,
      eventType: ptMaximizeEventType(player),
    });
  }

  function readPtMaximizeRequest(player, eventId) {
    const eventType = ptMaximizeEventType(player, eventId);
    if (!eventType) {
      throw new Error('未设置活动类型');
    }
    const form = readPtMaximizeForm({
      strict: true,
      config: player.ptMaximize,
      eventType,
    });
    const liveVariant = ptMaximizeLiveVariant(form, eventType);
    if (elements.ptMaximizeMissionSupportPt.required && form.missionSupportPtBonus == null) {
      throw new Error('任务 Live 自由演出必须填写支援乐队 PT 加成');
    }
    const songs = player.eventSongs?.[String(eventId)] ?? [];
    const request = {
      eventType: 'challenge',
      liveVariant,
      songs,
      minimumPersonalStat: liveVariant === 'cooperative'
        ? form.minimumPersonalStat
        : undefined,
      missionSupportPtBonus: elements.ptMaximizeMissionSupportPt.required
        ? form.missionSupportPtBonus
        : undefined,
    };
    if (liveVariant === 'cooperative') {
      if (form.minimumPersonalStat == null) {
        throw new Error('协力演出必须填写自己的最低综合力');
      }
      const teammateCount = form.teammateMode === 'uniform' ? 1 : 4;
      const teammates = form.teammates.slice(0, teammateCount).map((teammate, index) => {
        if (
          teammate.expectedStat == null
          || teammate.leaderScoreUp == null
          || teammate.leaderSkillDuration == null
        ) {
          throw new Error(`协力演出必须完整填写队友 ${index + 1} 参数`);
        }
        return {
          ...teammate,
          leaderScoreUp: teammate.leaderScoreUp / 100,
        };
      });
      request.cooperative = {
        teammates: form.teammateMode === 'uniform' ? teammates[0] : teammates,
        leaderSelection: form.cooperativeLeaderMode === 'specified'
          ? {
              mode: 'specified',
              playerIndex: form.cooperativeSpecifiedLeader,
            }
          : { mode: form.cooperativeLeaderMode },
      };
    } else if (liveVariant === 'versus') {
      request.versus = { teamRank: form.versusTeamRank };
    } else if (liveVariant === 'festival') {
      const scoreCount = form.festivalTeammateMode === 'uniform' ? 1 : 4;
      const teammateScores = form.festivalTeammateScores.slice(0, scoreCount);
      if (teammateScores.some((score) => score == null)) {
        throw new Error('团队演出必须填写队友预计分数');
      }
      request.festival = {
        teammateScores: form.festivalTeammateMode === 'uniform'
          ? teammateScores[0]
          : teammateScores,
        teamRank: form.festivalTeamRank,
        won: form.festivalWon,
      };
    }
    return request;
  }

  function readPtMaximizeForm({ strict, config, eventType }) {
    const teammates = Array.from({ length: 4 }, (_, index) => ({
      expectedStat: readFormInteger(
        elements.ptMaximizeTeammateStats[index],
        `队友 ${index + 1} 综合力`,
        { optional: true, strict },
      ),
      leaderScoreUp: readNonNegativeNumber(
        elements.ptMaximizeTeammateScoreUps[index],
        `队友 ${index + 1} 技能加成`,
        { optional: true, strict },
      ),
      leaderSkillDuration: readNonNegativeNumber(
        elements.ptMaximizeTeammateDurations[index],
        `队友 ${index + 1} 技能时长`,
        { optional: true, strict },
      ),
    }));
    const festivalTeammateScores = Array.from({ length: 4 }, (_, index) =>
      readFormInteger(
        elements.ptMaximizeFestivalTeammateScores[index],
        `队友 ${index + 1} 预计分数`,
        { optional: true, strict },
      ));
    const liveVariant = selectedRadioValue(
      elements.ptMaximizeLiveVariant,
      ptMaximizeLiveVariant(config, eventType),
    );
    return {
      liveVariantByEventType: withPtMaximizeLiveVariant(
        config,
        eventType,
        liveVariant,
      ).liveVariantByEventType,
      minimumPersonalStat: readFormInteger(
        elements.ptMaximizeMinimumStat,
        '最低综合力',
        { optional: true, strict },
      ),
      missionSupportPtBonus: readFormInteger(
        elements.ptMaximizeMissionSupportPt,
        '支援乐队 PT 加成',
        { optional: true, strict },
      ),
      teammateMode: selectedRadioValue(elements.ptMaximizeTeammateMode, 'uniform'),
      cooperativeLeaderMode: selectedRadioValue(
        elements.ptMaximizeCooperativeLeaderMode,
        'max_stat',
      ),
      cooperativeSpecifiedLeader:
        Number(selectedRadioValue(elements.ptMaximizeSpecifiedLeader, '0')) || 0,
      teammates,
      versusTeamRank: Number(selectedRadioValue(elements.ptMaximizeVersusRank, '0')) || 0,
      festivalTeamRank:
        Number(selectedRadioValue(elements.ptMaximizeFestivalRank, '0')) || 0,
      festivalWon: selectedRadioValue(elements.ptMaximizeFestivalWon, 'false') === 'true',
      festivalTeammateMode:
        selectedRadioValue(elements.ptMaximizeFestivalTeammateMode, 'uniform'),
      festivalTeammateScores,
    };
  }

  function ptMaximizeEventType(player, eventId = player.currentEvent) {
    const eventType = editableEventSnapshot(eventId, player)?.eventType;
    return typeof eventType === 'string' && eventType ? eventType : undefined;
  }

  function selectedRadioValue(container, fallback) {
    return container.querySelector('input[type="radio"]:checked')?.value ?? fallback;
  }

  function readNonNegativeNumber(input, label, {
    optional = false,
    strict = true,
  } = {}) {
    const raw = String(input?.value ?? '').trim();
    if (!raw && optional) {
      return undefined;
    }
    const value = Number(raw);
    if (raw && Number.isFinite(value) && value >= 0) {
      return value;
    }
    if (!strict) {
      return undefined;
    }
    throw new Error(`${label}必须是非负数`);
  }

  function handlePtMaximizeInputChange() {
    try {
      const player = readPlayer();
      applyPtMaximizeInputToPlayer(player);
      writePlayer(player);
      renderConfigForms(player);
    } catch (error) {
      setError(error);
    }
  }

  async function handleResultCacheAction(event) {
    const button = event.target.closest('[data-result-cache-action]');
    if (!button) {
      return;
    }
    const cacheKey = String(button.dataset.resultCacheKey || '').trim();
    if (!cacheKey) {
      return;
    }
    event.preventDefault();
    if (button.dataset.resultCacheAction === 'restore') {
      await handleRestoreResultCache(cacheKey);
    } else if (button.dataset.resultCacheAction === 'delete') {
      await handleDeleteResultCache(cacheKey);
    }
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

  async function handleDeleteResultCache(cacheKey) {
    try {
      const cached = getCachedResult(cacheKey);
      if (!cached) {
        setStatus('结果缓存未找到');
        return;
      }
      const confirmed = await confirmDialog({
        title: '删除结果缓存',
        lines: [`将删除“${cached.eventLabel || '所选活动'}”的这条结果缓存。`],
        confirmText: '确认删除',
        danger: true,
      });
      if (!confirmed) {
        return;
      }
      const previousCache = state.resultCache || [];
      const previousActiveKey = state.activeResultCacheKey;
      state.resultCache = (state.resultCache || [])
        .filter((entry) => entry.key !== cacheKey);
      if (state.activeResultCacheKey === cacheKey) {
        state.activeResultCacheKey = null;
      }
      try {
        await persistResultCacheState(state.activeResultCacheKey);
      } catch (error) {
        state.resultCache = previousCache;
        state.activeResultCacheKey = previousActiveKey;
        renderResultCachePanel(previousActiveKey);
        throw error;
      }
      setStatus('已删除结果缓存');
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
      const previousCache = state.resultCache || [];
      const previousActiveKey = state.activeResultCacheKey;
      state.resultCache = [];
      state.activeResultCacheKey = null;
      try {
        await clearPersisted();
      } catch (error) {
        state.resultCache = previousCache;
        state.activeResultCacheKey = previousActiveKey;
        renderResultCachePanel(previousActiveKey);
        throw error;
      }
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

  function ptMaximizeAverageScore(result) {
    const distribution = result?.team?.evaluation?.scoreDistribution;
    if (distribution) {
      return safeAverage(distribution.scoreSum, distribution.sampleCount);
    }
    return safeAverage(result?.medley?.totalScoreSum, result?.medley?.sampleCount);
  }

  function safeAverage(sum, count) {
    const numerator = Number(sum);
    const denominator = Number(count);
    return Number.isFinite(numerator) && Number.isFinite(denominator) && denominator > 0
      ? numerator / denominator
      : undefined;
  }

  async function handleCopyResult() {
    try {
      if (!state.lastDiagnostic) {
        throw new Error('还没有可复制的 score_check 数据');
      }
      const isFailure = state.lastDiagnostic.status === 'failed'
        || state.lastDiagnostic.error != null;
      const payload = isFailure
        ? state.lastDiagnostic
        : Array.isArray(state.lastDiagnostic.result)
          ? state.lastDiagnostic.result
          : scoreCheckPayloadFromDiagnostic(state.lastDiagnostic);
      await copyTextToClipboard(JSON.stringify(payload, null, 2));
      setStatus(isFailure
        ? '诊断 JSON 已复制'
        : Array.isArray(state.lastDiagnostic.result)
          ? '方案 JSON 已复制'
          : 'score_check JSON 已复制');
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
    handleScoreRangeInputChange,
    handlePtMaximizeInputChange,
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
