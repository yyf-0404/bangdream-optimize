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
    elements.scoreRangeControls.hidden = !scoreRangeActive;
    elements.activitySongCard.hidden = scoreRangeActive;
    for (const control of elements.scoreRangeControls.querySelectorAll('input, select')) {
      control.disabled = !scoreRangeActive;
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
    elements.calculationModeHint.textContent = scoreRangeActive
      ? event?.eventType === 'medley'
        ? '请在巡回演出中使用自动演出，并在第一首歌曲结束后退出。'
        : '请在自由演出中使用自动演出。'
      : '当前模式仅支持巡回演出、挑战 Live 和竞演 Live。';
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
