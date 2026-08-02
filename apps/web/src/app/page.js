import { ptMaximizeLiveVariant } from '../models/player-settings.js?v=3';

export function createPageController({
  elements,
  normalizePlayer,
  editableEventSnapshot,
  normalizedActivityMode,
  normalizedCalculationMode,
  activityModeForEvent,
  eventSearchValue,
  renderReferenceOptions,
  renderPlayerProfileControls,
  renderEventSummary,
  renderEventParameters,
  renderSongs,
  renderCards,
  renderAreaItems,
  renderCharacterBonuses,
  safeReadPlayer,
}) {
  function renderConfigForms(player, { page = activePage() } = {}) {
    const normalized = normalizePlayer(player);
    const event = editableEventSnapshot(normalized.currentEvent, normalized);
    normalized.activityMode = normalizedActivityMode(
      event ? activityModeForEvent(event) : normalized.activityMode,
    );
    elements.playerId.value = String(normalized.playerId ?? 0);
    elements.playerServer.value = normalized.server;
    renderPlayerProfileControls(normalized);
    renderCalculationControls(normalized, event);
    elements.eventId.value = normalized.currentEvent == null ? '' : String(normalized.currentEvent);
    elements.eventSearch.value = normalized.currentEvent == null
      ? ''
      : eventSearchValue(normalized.currentEvent, normalized);
    renderReferenceOptions();
    renderPageForms(page, normalized, { normalized: true });
  }

  function renderCalculationControls(player, event) {
    const calculationMode = normalizedCalculationMode(player.calculationMode);
    for (const input of elements.calculationModeInputs) {
      input.checked = input.value === calculationMode;
    }
    const scoreRangeActive = calculationMode === 'scoreRange';
    const ptMaximizeActive = calculationMode === 'ptMaximize';
    elements.scoreRangeControls.hidden = !scoreRangeActive;
    elements.activitySongCard.hidden = scoreRangeActive;
    for (const control of elements.scoreRangeControls.querySelectorAll('input, select')) {
      control.disabled = !scoreRangeActive;
    }
    elements.ptMaximizeControls.hidden = !ptMaximizeActive;
    for (const control of elements.ptMaximizeControls.querySelectorAll('input, select')) {
      control.disabled = !ptMaximizeActive;
    }

    const request = player.scoreRange ?? {};
    elements.scoreRangeCurrentPt.value = String(request.currentPt ?? 0);
    elements.scoreRangeTargetTotalPt.value = String(request.targetTotalPt ?? 0);
    elements.scoreRangeAutoBaseMultiplier.value = String(request.autoBaseMultiplier ?? 0.5);
    const autoMultiplierServer = {
      jp: ['日服', 0.75],
      en: ['国际服', 0.5],
      tw: ['繁中服', 0.5],
      cn: ['国服', 0.5],
      kr: ['韩服', 0.5],
    }[player.server] ?? [String(player.server ?? '').toUpperCase(), 0.5];
    elements.scoreRangeAutoBaseMultiplierHint.textContent =
      `当前服务器为${autoMultiplierServer[0]}，建议使用 ${autoMultiplierServer[1]} 倍。`;
    elements.scoreRangeMissionSupportPt.value = request.missionSupportPtBonus == null
      ? ''
      : String(request.missionSupportPtBonus);

    const missionLive = scoreRangeActive && event?.eventType === 'mission_live';
    elements.scoreRangeMissionSupportField.hidden = !missionLive;
    elements.scoreRangeMissionSupportPt.required = missionLive;
    renderPtMaximizeControls(player, event, ptMaximizeActive);
    elements.calculationModeHint.hidden = ptMaximizeActive;
    elements.calculationModeHint.textContent = scoreRangeActive
      ? event?.eventType === 'medley'
        ? '请在巡回演出中使用自动演出，并在第一首歌曲结束后退出。'
        : '请在自由演出中使用自动演出。'
      : '当前模式仅支持巡回演出、挑战 Live 和竞演 Live。';
  }

  function renderPtMaximizeControls(player, event, active) {
    const variantsByEvent = {
      mission_live: [['solo', '自由演出'], ['cooperative', '协力演出']],
      live_try: [['solo', '自由演出'], ['cooperative', '协力演出']],
      challenge: [['solo', '自由演出'], ['cooperative', '协力演出'], ['challenge_cp', '挑战演出']],
      versus: [['solo', '自由演出'], ['versus', '竞演演出']],
      festival: [['solo', '自由演出'], ['festival', '团队演出']],
      medley: [['medley', '巡回演出']],
    };
    const variants = variantsByEvent[event?.eventType] ?? [];
    const request = player.ptMaximize ?? {};
    const selected = ptMaximizeLiveVariant(request, event?.eventType);
    elements.ptMaximizeLiveVariant.replaceChildren(...variants.map(([value, text]) => {
      const label = document.createElement('label');
      const input = document.createElement('input');
      input.type = 'radio';
      input.name = 'pt-maximize-live-variant';
      input.value = value;
      input.checked = value === selected;
      input.disabled = !active;
      const span = document.createElement('span');
      span.textContent = text;
      label.append(input, span);
      return label;
    }));
    elements.ptMaximizeMinimumStat.value = String(request.minimumPersonalStat ?? 290000);
    elements.ptMaximizeMissionSupportPt.value = String(request.missionSupportPtBonus ?? 100);
    const teammateMode = request.teammateMode ?? 'uniform';
    setRadioValue(elements.ptMaximizeTeammateMode, teammateMode);
    setTeammateRowLabels(elements.ptMaximizeCooperativeTeammateLabels, teammateMode);
    const cooperativeLeaderMode = request.cooperativeLeaderMode ?? 'max_stat';
    setRadioValue(elements.ptMaximizeCooperativeLeaderMode, cooperativeLeaderMode);
    setRadioValue(
      elements.ptMaximizeSpecifiedLeader,
      request.cooperativeSpecifiedLeader ?? 0,
    );
    elements.ptMaximizeSpecifiedLeaderField.hidden =
      cooperativeLeaderMode !== 'specified';
    elements.ptMaximizeCooperativeLeaderHint.textContent = {
      max_stat: '选择综合力最高的玩家；综合力相同时，队长随机选择。',
      specified: '固定使用指定玩家的队长技能作为第六个技能。',
      random: '在 5 名玩家中等概率随机选择。',
    }[cooperativeLeaderMode] ?? '';
    const teammates = request.teammates ?? [];
    for (let index = 0; index < 4; index += 1) {
      elements.ptMaximizeTeammateStats[index].value =
        String(teammates[index]?.expectedStat ?? 290000);
      elements.ptMaximizeTeammateScoreUps[index].value =
        String(teammates[index]?.leaderScoreUp ?? 130);
      elements.ptMaximizeTeammateDurations[index].value =
        formatDurationInput(teammates[index]?.leaderSkillDuration ?? 7);
    }
    setRadioValue(elements.ptMaximizeVersusRank, request.versusTeamRank ?? 0);
    const festivalTeammateMode = request.festivalTeammateMode ?? 'uniform';
    setRadioValue(
      elements.ptMaximizeFestivalTeammateMode,
      festivalTeammateMode,
    );
    setTeammateRowLabels(
      elements.ptMaximizeFestivalTeammateLabels,
      festivalTeammateMode,
    );
    for (let index = 0; index < 4; index += 1) {
      elements.ptMaximizeFestivalTeammateScores[index].value =
        request.festivalTeammateScores?.[index] ?? 4000000;
    }
    setRadioValue(elements.ptMaximizeFestivalRank, request.festivalTeamRank ?? 0);
    setRadioValue(elements.ptMaximizeFestivalWon, String(request.festivalWon === true));

    const cooperative = active && selected === 'cooperative';
    elements.ptMaximizeCooperativeFields.hidden = !cooperative;
    elements.ptMaximizeMinimumStatField.hidden = !cooperative;
    elements.ptMaximizeMinimumStat.required = cooperative;
    const versus = active && selected === 'versus';
    elements.ptMaximizeVersusRankField.hidden = !versus;
    const festival = active && selected === 'festival';
    elements.ptMaximizeFestivalOtherFields.hidden = !festival;
    elements.ptMaximizeFestivalTeammateFields.hidden = !festival;
    const missionLive = active
      && event?.eventType === 'mission_live'
      && (selected === 'solo' || selected === 'cooperative');
    elements.ptMaximizeMissionSupportField.hidden = !missionLive;
    elements.ptMaximizeMissionSupportPt.required = missionLive;
    const teammatePaneVisible = cooperative || festival;
    elements.ptMaximizeTeammatePane.hidden = !teammatePaneVisible;
    elements.ptMaximizeParameterGrid.hidden =
      !(cooperative || versus || festival || missionLive);
    elements.ptMaximizeParameterGrid.classList.toggle(
      'has-teammate-pane',
      teammatePaneVisible,
    );
    setRepeatedInputState(
      elements.ptMaximizeTeammateStats,
      teammateMode,
      cooperative,
    );
    setRepeatedInputState(
      elements.ptMaximizeTeammateScoreUps,
      teammateMode,
      cooperative,
    );
    setRepeatedInputState(
      elements.ptMaximizeTeammateDurations,
      teammateMode,
      cooperative,
    );
    setRepeatedInputState(
      elements.ptMaximizeFestivalTeammateScores,
      festivalTeammateMode,
      festival,
    );
  }

  function setRepeatedInputState(inputs, mode, required) {
    for (const [index, input] of Array.from(inputs).entries()) {
      const visible = mode === 'individual' || index === 0;
      const repeatedRow = input.closest('[data-repeated-row]') ?? input.closest('label');
      if (repeatedRow) {
        repeatedRow.hidden = !visible;
      }
      input.required = required && visible;
    }
  }

  function setTeammateRowLabels(labels, mode) {
    for (const [index, label] of Array.from(labels).entries()) {
      label.textContent = mode === 'uniform' && index === 0
        ? '队友'
        : `队友 ${index + 1}`;
    }
  }

  function setRadioValue(container, value) {
    const target = String(value);
    for (const input of container.querySelectorAll('input[type="radio"]')) {
      input.checked = input.value === target;
    }
  }

  function formatDurationInput(value) {
    const duration = Number(value);
    if (!Number.isFinite(duration)) {
      return '7.0';
    }
    return Number.isInteger(duration) ? duration.toFixed(1) : String(duration);
  }

  function activePage() {
    for (const tab of elements.pageTabs) {
      if (tab.classList.contains('active')) {
        return tab.dataset.page;
      }
    }
    return 'activity';
  }

  function activatePage(page, { render = true } = {}) {
    for (const tab of elements.pageTabs) {
      tab.classList.toggle('active', tab.dataset.page === page);
    }
    elements.form.classList.toggle('is-result-page', page === 'result');
    for (const panel of elements.pagePanels) {
      const active = panel.dataset.pagePanel === page;
      panel.hidden = !active;
      panel.classList.toggle('active', active);
    }
    if (render) {
      if (page === 'cards') {
        renderReferenceOptions();
      }
      renderPageForms(page, safeReadPlayer(), { normalized: true });
    }
  }

  function renderPageForms(page, player, { normalized: alreadyNormalized = false } = {}) {
    const normalized = alreadyNormalized ? player : normalizePlayer(player);
    switch (page) {
      case 'activity':
        renderEventSummary(normalized.currentEvent);
        renderEventParameters(normalized);
        renderSongs(normalized);
        break;
      case 'cards':
        renderCards(normalized);
        break;
      case 'player':
        renderAreaItems(normalized);
        renderCharacterBonuses(normalized);
        break;
      default:
        break;
    }
  }

  return {
    activatePage,
    activePage,
    renderConfigForms,
    renderPageForms,
  };
}
