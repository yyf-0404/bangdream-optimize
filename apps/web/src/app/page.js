export function createPageController({
  elements,
  normalizePlayer,
  editableEventSnapshot,
  normalizedActivityMode,
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
    elements.activityMode.value = normalized.activityMode;
    elements.eventId.value = normalized.currentEvent == null ? '' : String(normalized.currentEvent);
    elements.eventSearch.value = normalized.currentEvent == null
      ? ''
      : eventSearchValue(normalized.currentEvent, normalized);
    renderReferenceOptions();
    renderPageForms(page, normalized);
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
      renderPageForms(page, safeReadPlayer());
    }
  }

  function renderPageForms(page, player) {
    const normalized = normalizePlayer(player);
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
