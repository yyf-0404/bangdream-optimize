import {validationError, validateAt} from '../models/validation-error.js';
import {revealValidationError, clearValidationReveal} from '../ui/validation.js';
import { confirmDialog } from '../ui/confirm.js?v=3';
import {copyImageToClipboard} from '../ui/clipboard.js';
import {renderResultImage, offerResultImage} from '../ui/result-image.js';
import { totalFireCost } from '../utils.js?v=3';
import {
  ptEvaluateLiveVariant,
  ptEvaluateSupportsAuto,
  ptMaximizeLiveVariant,
  withPtEvaluateLiveVariant,
  withPtMaximizeLiveVariant,
} from '../models/player-settings.js?v=3';
import { validatePtEvaluateTeamSelection } from '../models/pt-evaluate-validation.js?v=1';
import {customCardPresentationKey} from '../models/custom-cards.js';
import {equipmentAvailable} from '../domain/area.js';

export function createCalculationActions({
  state,
  elements,
  readPlayer,
  writePlayer,
  savePlayerNow,
  readOptionalInteger,
  parseEntityId,
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
  getActivePage = () => globalThis.location?.hash,
  hasOpenDialog = () => !!globalThis.document?.querySelector?.('dialog[open]'),
  setStatus,
  setError,
  eventLabel,
  normalizedPlayer,
  areaItemGroups,
  mainBandCardIds,
  importMainBandCards,
  importMainBandCharacterBonuses,
  importEnabledAreaItems,
  cardCharacterId,
  resultCacheLimit = 20,
  renderResultCache,
  persistResultCache,
  clearPersistedResultCache,
  yieldForPaint = yieldToBrowserPaint,
}) {
  const RESULT_CACHE_KEY_VERSION = 8;
  const calculateButton = elements.calculateButton;
  const calculateButtons = Array.from(elements.calculateButtons || []);
  const calculateButtonLabel = calculateButton?.querySelector('.button-label');
  const calculateLabel = calculateButtonLabel?.textContent?.trim()
    || calculateButton?.textContent?.trim()
    || '计算';
  const calculationLabels = new Map();
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

  function makeResultCacheKey(player, eventId, profileId = state.activePlayerProfileId) {
    player = normalizedPlayer(player);
    const serialized = cloneJson({
      cacheVersion: RESULT_CACHE_KEY_VERSION,
      profileId,
      skillShuffleModel: 'cn-9.4.2',
      server: player.server,
      calculationMode: player.calculationMode,
      activityMode: player.activityMode,
      scoreRange: player.scoreRange,
      ptMaximize: player.ptMaximize,
      ptEvaluate: player.ptEvaluate,
      eventId,
      eventSearch: player.eventSearch,
      currentEvent: player.currentEvent,
      cards: Object.fromEntries(Object.entries(player.cardList||{}).map(([id,card])=>{const {illustTrainingStatus,...growth}=card;return [id,growth];})),
      customCards: Object.fromEntries(Object.entries(player.customCards||{}).map(([id,c])=>[id,{uid:c.uid,enabled:c.enabled,definition:c.definition,growth:c.growth,editor:customCardPresentationKey(c)}])),
      areas: player.areaItem,
      chars: player.characterBouns,
      bonuses: player.eventPresets,
      overrides: player.eventOverrides,
      songs: player.eventSongs,
      eventAttributeAndCharacterBonus: player.eventPresets?.[String(eventId)]?.eventAttributeAndCharacterBonus,
    });
    return JSON.stringify(serialized, (_key, value) => value && typeof value === 'object' && !Array.isArray(value) ? Object.fromEntries(Object.entries(value).sort(([a],[b])=>a.localeCompare(b))) : value);
  }

  function getCachedResult(cacheKey) {
    const cache = state.resultCache || [];
    const index = cache.findIndex((entry) => entry.key === cacheKey && entry.profileId === state.activePlayerProfileId);
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
    profileId = state.activePlayerProfileId,
  }) {
    const cache = state.resultCache || [];
    const nextCache = cache.filter((entry) => entry.key !== cacheKey);
    nextCache.unshift({
      cacheVersion: RESULT_CACHE_KEY_VERSION,
      profileId,
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
    let profileCount = 0;
    state.resultCache = nextCache.filter(entry => entry.profileId !== profileId || ++profileCount <= cacheLimit);
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
      renderResultCache((state.resultCache || []).filter(entry=>entry.profileId===state.activePlayerProfileId), { activeKey });
    }
  }

  function applyResult(result, diagnostic, cacheKey, profileId = state.activePlayerProfileId, reveal = true) {
    state.profileDiagnostics ??= {};
    state.profileDiagnostics[profileId] = {result,diagnostic,cacheKey};
    if (profileId !== state.activePlayerProfileId) return;
    elements.result.textContent = JSON.stringify(result, null, 2);
    renderResultSummary(result, { diagnostic });
    renderMetrics(result.metrics);
    state.lastDiagnostic = diagnostic;
    state.activeResultCacheKey = cacheKey;
    if (reveal) activatePage('result');
  }

  function applyFailureDiagnostic(diagnostic, profileId = state.activePlayerProfileId, reveal = true) {
    state.profileDiagnostics ??= {};
    state.profileDiagnostics[profileId] = {diagnostic};
    if (profileId !== state.activePlayerProfileId) return;
    elements.result.textContent = JSON.stringify(diagnostic, null, 2);
    renderResultSummary(null, { diagnostic });
    renderMetrics(null);
    state.lastDiagnostic = diagnostic;
    state.activeResultCacheKey = null;
    renderResultCachePanel(null);
    if (reveal) activatePage('result');
  }

  async function handleCalculate(event) {
    event.preventDefault();
    if (isCalculating) {
      return;
    }
    clearValidationReveal();
    if (elements.form?.checkValidity && !elements.form.checkValidity()) {
      const firstInvalid = elements.form.querySelector('input:invalid,select:invalid,textarea:invalid');
      const error = new Error(firstInvalid?.validationMessage || '请补全标出的计算参数');
      revealValidationError(error, {activatePage, field: firstInvalid});
      setError(error);
      return;
    }
    try {
      validatePtEvaluateBeforeCalculation();
    } catch (error) {
      revealValidationError(error, {activatePage});
      setError(error);
      return;
    }
    const requestProfileId = state.activePlayerProfileId;
    const requestPage = getActivePage();
    const revealResult = () => !hasOpenDialog() && (getActivePage() === requestPage || getActivePage() === '#result');
    const requestPlayer = readPlayer();
    const originalPlayerText = JSON.stringify(requestPlayer);
    isCalculating = true;
    setCalculatingState(true);
    try {
      const player = requestPlayer;
      if (requestProfileId !== state.activePlayerProfileId) throw new Error('档案已切换，请重新开始计算');
      validateAt('#activity-event-select', () => applyEventInputToPlayer(player));
      applyScoreRangeInputToPlayer(player);
      applyPtMaximizeInputToPlayer(player);
      applyPtEvaluateInputToPlayer(player);
      const eventId = readCurrentEventId(player, readOptionalInteger(elements.eventId.value));
      const scoreRangeRequest = player.calculationMode === 'scoreRange' ? validateAt('#bo-calculation-settings', () => readScoreRangeRequest()) : undefined;
      const ptMaximizeRequest = player.calculationMode === 'ptMaximize' ? validateAt('#bo-calculation-settings', () => readPtMaximizeRequest(player, eventId)) : undefined;
      const ptEvaluateRequest = player.calculationMode === 'ptEvaluate' ? validateAt('[data-section=bo-teams]', () => readPtEvaluateRequest(player, eventId)) : undefined;
      setStatus('准备计算');
      await yieldForPaint();
      setStatus('同步数据');
      const core = await ensureCore({ refreshManifest: true });
      normalizeCurrentActivityForMode(player);
      ensureOwnedCardCharacterBonuses(player);
      if (requestProfileId !== state.activePlayerProfileId) throw new Error('档案已切换，请重新开始计算');
      // Synchronization may yield while the user edits. Keep those newer edits;
      // this request still calculates against the snapshot captured on submit.
      if (JSON.stringify(readPlayer()) === originalPlayerText) {
        writePlayer(player, { autosave: false });
        renderConfigForms(player);
        await savePlayerNow(player);
      }
      if (requestProfileId !== state.activePlayerProfileId) throw new Error('档案已切换，请重新开始计算');
      const cacheKey = makeResultCacheKey(player, eventId, requestProfileId);
      state.activeResultCacheKey = cacheKey;
      const cached = getCachedResult(cacheKey);
      if (cached) {
        applyResult(cached.result, cached.diagnostic, cacheKey, requestProfileId, revealResult());
        renderResultCachePanel(cacheKey);
        setStatus('完成（缓存）');
        return;
      }

      setStatus('计算中');
      let result;
      let calculationRequest;
      try {
        calculationRequest = scoreRangeRequest ?? ptMaximizeRequest ?? ptEvaluateRequest;
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
            : player.calculationMode === 'ptEvaluate'
              ? await calculatePtEvaluate({
                player,
                eventId,
                core,
                request: ptEvaluateRequest,
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
          calculationRequest,
        });
        applyFailureDiagnostic(diagnostic, requestProfileId, revealResult());
        setStatus(`计算失败：${diagnostic.error?.title ?? '已生成诊断'}`);
        return;
      }
      const diagnostic = await buildDiagnostic({
        player,
        server: player.server,
        eventId,
        result,
        calculationRequest,
      });
      const resultCacheSaved = await setCachedResult(cacheKey, {
        player,
        eventId,
        result,
        diagnostic,
        profileId: requestProfileId,
      });
      applyResult(result, diagnostic, cacheKey, requestProfileId, revealResult());
      setStatus(resultCacheSaved ? '完成' : '完成（结果缓存保存失败）');
    } catch (error) {
      revealValidationError(error, {activatePage});
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

  function validatePtEvaluateBeforeCalculation() {
    const player = readPlayer();
    if (player.calculationMode !== 'ptEvaluate') {
      return;
    }
    applyEventInputToPlayer(player);
    applyPtEvaluateInputToPlayer(player);
    const eventId = readCurrentEventId(player, readOptionalInteger(elements.eventId.value));
    validateAt('[data-section=bo-teams]', () => readPtEvaluateRequest(player, eventId));
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

  async function calculatePtEvaluate({ player, eventId, core, request }) {
    if (typeof state.runtime.ptEvaluate !== 'function') {
      throw new Error('当前运行时不支持指定队伍计算');
    }
    return state.runtime.ptEvaluate({
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
      throw validationError('目标总 PT 必须大于当前 PT', '[data-setting="scoreRange.targetTotalPt"]');
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
      revealValidationError(error, {activatePage});
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
      throw validationError('未设置活动类型', '#activity-event-select');
    }
    const form = readPtMaximizeForm({
      strict: true,
      config: player.ptMaximize,
      eventType,
    });
    const liveVariant = ptMaximizeLiveVariant(form, eventType);
    const missionSupportRequired = eventType === 'mission_live'
      && (liveVariant === 'solo' || liveVariant === 'cooperative');
    if (missionSupportRequired && form.missionSupportPtBonus == null) {
      throw new Error('任务 Live 必须填写支援乐队 PT 加成');
    }
    const songs = player.eventSongs?.[String(eventId)] ?? [];
    const request = {
      eventType,
      liveVariant,
      songs,
      minimumPersonalStat: liveVariant === 'cooperative'
        ? form.minimumPersonalStat
        : undefined,
      // Game data is authoritative and may correct a stale frontend event type.
      // Keep the support value on every request so a corrected mission_live
      // request never reaches the core without its required input. Other event
      // types ignore this field.
      missionSupportPtBonus: form.missionSupportPtBonus ?? 100,
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
      revealValidationError(error, {activatePage});
      setError(error);
    }
  }

  function applyPtEvaluateInputToPlayer(player) {
    player.ptEvaluate = readPtEvaluateForm({
      strict: false,
      config: player.ptEvaluate,
      eventType: ptMaximizeEventType(player),
      server: player.server,
    });
  }

  function readPtEvaluateRequest(player, eventId) {
    const eventType = ptMaximizeEventType(player, eventId);
    if (!eventType) {
      throw validationError('未设置活动类型', '#activity-event-select');
    }
    const form = readPtEvaluateForm({
      strict: true,
      config: player.ptEvaluate,
      eventType,
      server: player.server,
    });
    const liveVariant = ptEvaluateLiveVariant(form, eventType);
    const teamCount = liveVariant === 'medley' ? 3 : 1;
    const teams = form.teams.slice(0, teamCount).map((cardIds) => ({
      cardIds,
      captainCardId: cardIds[2],
    }));
    const request = {
      eventType,
      liveVariant,
      songs: player.eventSongs?.[String(eventId)] ?? [],
      teams,
      items: form.items,
      scoreMode: form.scoreMode === 'auto'
        ? { mode: 'auto', baseMultiplier: form.autoBaseMultiplier }
        : { mode: 'manual' },
      missionSupportPtBonus: form.missionSupportPtBonus ?? 100,
    };
    if (liveVariant === 'versus') {
      request.versus = { teamRank: form.versusTeamRank };
    }
    validateAt('[data-section=bo-teams]', () => validatePtEvaluateTeamSelection(player, request, cardCharacterId));
    return request;
  }

  function readPtEvaluateForm({ strict, config, eventType, server }) {
    const existingTeams = config?.teams ?? [];
    const inputs = Array.from(elements.ptEvaluateTeams.querySelectorAll('.pt-evaluate-card-input'));
    const teams = Array.from({ length: 3 }, (_, teamIndex) =>
      Array.from({ length: 5 }, (_, cardIndex) => {
        const input = inputs.find((candidate) =>
          Number(candidate.dataset.teamIndex) === teamIndex
            && Number(candidate.dataset.cardIndex) === cardIndex,
        );
        if (!input) {
          return Number(existingTeams[teamIndex]?.[cardIndex]) || 0;
        }
        const value = String(input.value ?? '').trim();
        if (!value) {
          if (strict) {
            throw validationError(`队伍 ${teamIndex + 1} 的卡位 ${cardIndex + 1} 不能为空`, '[data-section=bo-teams]');
          }
          return 0;
        }
        try {
          return parseEntityId(value, `队伍 ${teamIndex + 1} 卡位 ${cardIndex + 1}`);
        } catch (error) {
          if (strict) {
            throw error;
          }
          return Number(existingTeams[teamIndex]?.[cardIndex]) || 0;
        }
      }),
    );
    const liveVariant = selectedRadioValue(
      elements.ptEvaluateLiveVariant,
      ptEvaluateLiveVariant(config, eventType),
    );
    const selectedScoreMode = selectedRadioValue(elements.ptEvaluateScoreMode, 'manual');
    const scoreMode = ptEvaluateSupportsAuto(liveVariant) ? selectedScoreMode : 'manual';
    const autoBaseMultiplier = Number(elements.ptEvaluateAutoBaseMultiplier.value);
    if (strict && scoreMode === 'auto' && ![0.5, 0.75].includes(autoBaseMultiplier)) {
      throw new Error('Auto 倍率必须为 0.5 或 0.75');
    }
    const itemValue = (select, previous) => String(select?.value ?? previous ?? '');
    const items = {
      band: itemValue(elements.ptEvaluateBandItem, config?.items?.band),
      attribute: itemValue(elements.ptEvaluateAttributeItem, config?.items?.attribute),
      magazine: itemValue(elements.ptEvaluateMagazineItem, config?.items?.magazine),
    };
    if (strict && Object.values(items).some((value) => !value)) {
      throw validationError('必须完整选择乐队、属性和杂志道具', '[data-section=bo-equipment]');
    }
    const currentPlayer = strict ? readPlayer() : null;
    if (strict && Object.entries(items).some(([category, value]) => !equipmentAvailable(
      areaItemGroups(currentPlayer).find(g => g.category === category && g.key.split(':').slice(1).join(':') === value), currentPlayer))) {
      throw validationError('所选区域道具组包含 0 级道具', '[data-section=bo-equipment]');
    }
    return {
      liveVariantByEventType: withPtEvaluateLiveVariant(
        config,
        eventType,
        liveVariant,
        server,
      ).liveVariantByEventType,
      teams,
      items,
      scoreMode,
      autoBaseMultiplier: [0.5, 0.75].includes(autoBaseMultiplier)
        ? autoBaseMultiplier
        : server === 'jp' ? 0.75 : 0.5,
      missionSupportPtBonus: readFormInteger(
        elements.ptEvaluateMissionSupportPt,
        '支援乐队 PT 加成',
        { optional: true, strict },
      ),
      versusTeamRank: Number(selectedRadioValue(elements.ptEvaluateVersusRank, '0')) || 0,
    };
  }

  function handlePtEvaluateInputChange() {
    try {
      const player = readPlayer();
      applyPtEvaluateInputToPlayer(player);
      writePlayer(player);
      renderConfigForms(player);
    } catch (error) {
      revealValidationError(error, {activatePage});
      setError(error);
    }
  }

  async function loadMainBandDraft(current, teamIndex) {
    if (!Number.isSafeInteger(Number(current.playerId)) || Number(current.playerId) <= 0) {
      throw new Error('请先在档案与数据中填写玩家 ID，再导入主乐队。');
    }
    await ensureCore();
    const profile = await state.runtime.importBestdoriPlayerProfile({playerId:current.playerId,server:current.server,mode:3});
    const cardIds = mainBandCardIds(profile);
    importMainBandCards(current, profile);
    importMainBandCharacterBonuses(current, profile);
    importEnabledAreaItems(current, profile);
    current.ptEvaluate.teams[teamIndex] = cardIds;
    current.ptEvaluate.items = selectedImportedAreaItems(profile, current);
    return current;
  }

  async function handlePtEvaluateTeamAction(event) {
    const button = event.target.closest('.pt-evaluate-import-main-band');
    if (!button) {
      return;
    }
    event.preventDefault();
    button.disabled = true;
    try {
      setStatus('导入主乐队配置');
      await ensureCore();
      const current = normalizedPlayer(readPlayer());
      const profile = await state.runtime.importBestdoriPlayerProfile({
        playerId: current.playerId,
        server: current.server,
        mode: 3,
      });
      const cardIds = mainBandCardIds(profile);
      importMainBandCards(current, profile);
      importMainBandCharacterBonuses(current, profile);
      importEnabledAreaItems(current, profile);
      const teamIndex = Math.max(0, Math.min(2, Number(button.dataset.teamIndex) || 0));
      current.ptEvaluate.teams[teamIndex] = cardIds;
      current.ptEvaluate.items = selectedImportedAreaItems(profile, current);
      writePlayer(current);
      renderConfigForms(current);
      setStatus(
        `主乐队配置已导入：${cardIds.length} 张卡牌，`
        + `${Object.keys(current.areaItem ?? {}).length} 个区域道具，`
        + `${Object.keys(current.characterBouns ?? {}).length} 个角色加成`,
      );
    } catch (error) {
      revealValidationError(error, {activatePage});
      setError(error);
    } finally {
      button.disabled = false;
    }
  }

  function selectedImportedAreaItems(profile, player) {
    const enabledIds = new Set(
      (profile?.enabledUserAreaItems?.entries ?? [])
        .map((entry) => String(Number(entry?.areaItemCategory) || ''))
        .filter(Boolean),
    );
    const byCategory = new Map();
    for (const group of areaItemGroups(player)) {
      const enabledCount = group.areaItemIds.filter((id) => enabledIds.has(String(id))).length;
      const score = enabledCount * 1_000
        + Number(group.rate?.performance ?? 0)
        + Number(group.rate?.technique ?? 0)
        + Number(group.rate?.visual ?? 0);
      const current = byCategory.get(group.category);
      if (!current || score > current.score) {
        byCategory.set(group.category, { group, score });
      }
    }
    const value = (category, fallback) => {
      const key = byCategory.get(category)?.group?.key;
      return key ? key.split(':').slice(1).join(':') : fallback;
    };
    return {
      band: value('band', player.ptEvaluate?.items?.band ?? ''),
      attribute: value('attribute', player.ptEvaluate?.items?.attribute ?? ''),
      magazine: value('magazine', player.ptEvaluate?.items?.magazine ?? 'performance'),
    };
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
      revealValidationError(error, {activatePage});
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
      revealValidationError(error, {activatePage});
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
      state.resultCache = (state.resultCache || []).filter(entry=>entry.profileId!==state.activePlayerProfileId);
      state.activeResultCacheKey = null;
      try {
        await persistCache(state.resultCache);
      } catch (error) {
        state.resultCache = previousCache;
        state.activeResultCacheKey = previousActiveKey;
        renderResultCachePanel(previousActiveKey);
        throw error;
      }
      renderResultCachePanel(null);
      setStatus('结果缓存已清空');
    } catch (error) {
      revealValidationError(error, {activatePage});
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

  async function handleSaveResultImage() {
    const button = elements.saveResultImage;
    if (button.disabled) return;
    button.disabled = true;
    button.setAttribute('aria-busy', 'true');
    button.textContent = '正在生成图片…';
    const image = renderResultImage(document.querySelector('#result-design'), state.runtime);
    image.catch(() => {});
    try {
      await copyImageToClipboard(image, state.runtime);
      setStatus('结果图片已复制，可直接粘贴');
      const notice = document.querySelector('#result-action-status');
      notice.textContent = '结果图片已复制，可直接粘贴 ';
      const preview = document.createElement('button');preview.type='button';preview.className='text-button';preview.textContent='查看图片';
      const blob = await image;preview.onclick=()=>offerResultImage(blob, state.runtime, {copied:true});notice.append(preview);
    } catch (error) {
      try { offerResultImage(await image, state.runtime); }
      catch (renderError) { setError(renderError); document.querySelector('#result-action-status').textContent = renderError.message; }
    } finally {
      button.disabled = !state.lastDiagnostic?.result;
      button.removeAttribute('aria-busy');
      button.textContent = '保存结果图片';
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
      const notice = globalThis.document?.querySelector('#result-action-status');
      if (notice) notice.textContent = result === 'cancelled' ? '已取消导出诊断' : `已导出 ${fileName}`;
      if (result === 'cancelled') {
        setStatus('已取消导出诊断');
      } else if (result === 'downloaded') {
        setStatus('诊断已下载');
      } else {
        setStatus('诊断已导出');
      }
    } catch (error) {
      revealValidationError(error, {activatePage});
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
      const label = button.querySelector('.button-label');
      if (!calculationLabels.has(button)) calculationLabels.set(button, label?.textContent?.trim() || button.textContent?.trim() || calculateLabel);
      button.disabled = isBusy;
      button.classList.toggle('is-loading', isBusy);
      button.setAttribute('aria-busy', isBusy ? 'true' : 'false');
      if (label) {
        label.textContent = isBusy ? '计算中' : calculationLabels.get(button);
      } else {
        button.textContent = isBusy ? '计算中' : calculationLabels.get(button);
      }
    }
  }

  function syncProfileResult() {
    if (state.displayedResultProfileId !== state.activePlayerProfileId) {
      state.displayedResultProfileId = state.activePlayerProfileId;
      const saved = state.profileDiagnostics?.[state.activePlayerProfileId];
      state.lastDiagnostic = saved?.diagnostic ?? null;
      state.activeResultCacheKey = saved?.cacheKey ?? null;
      renderResultSummary(saved?.result, {diagnostic:saved?.diagnostic});
      renderMetrics(saved?.result?.metrics);
    }
    renderResultCachePanel();
    const notice = elements.resultSummary?.parentElement?.querySelector('.result-stale');
    if (notice) notice.hidden = !state.lastDiagnostic || makeResultCacheKey(readPlayer(),readPlayer().currentEvent) === makeResultCacheKey(state.lastDiagnostic.player,state.lastDiagnostic.eventId);
  }

  return {
    syncProfileResult,
    handleCalculate,
    handleSaveResultImage,
    handleExportDiagnostics,
    handleResultCacheAction,
    handleClearResultCache,
    handleScoreRangeInputChange,
    handlePtMaximizeInputChange,
    handlePtEvaluateInputChange,
    handlePtEvaluateTeamAction,
    loadMainBandDraft,
  };
}

function yieldToBrowserPaint() {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame !== 'function') {
      setTimeout(resolve, 0);
      return;
    }
    let settled = false;
    let fallbackTimer;
    const done = () => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(fallbackTimer);
      resolve();
    };
    requestAnimationFrame(() => setTimeout(done, 0));
    fallbackTimer = setTimeout(done, 50);
  });
}

function cloneJson(value) {
  return value == null ? value : JSON.parse(JSON.stringify(value));
}
