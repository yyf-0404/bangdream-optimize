import {
  ptEvaluateLiveVariant,
  ptEvaluateSupportsAuto,
  ptMaximizeLiveVariant,
} from '../models/player-settings.js?v=3';
import { assetImage } from '../assets/index.js?v=3';
import { cardPreviewContent } from '../ui/card-preview.js?v=3';

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
  areaItemGroups,
  areaItemGroupIconUrls,
  formatAreaItemRate,
  cardAttribute,
  cardIconUrls,
  cardLabel,
  cardName,
  cardRarity,
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
    const ptEvaluateActive = calculationMode === 'ptEvaluate';
    elements.scoreRangeControls.hidden = !scoreRangeActive;
    elements.activitySongCard.hidden = scoreRangeActive;
    for (const control of elements.scoreRangeControls.querySelectorAll('input, select')) {
      control.disabled = !scoreRangeActive;
    }
    elements.ptMaximizeControls.hidden = !ptMaximizeActive;
    for (const control of elements.ptMaximizeControls.querySelectorAll('input, select')) {
      control.disabled = !ptMaximizeActive;
    }
    elements.ptEvaluateControls.hidden = !ptEvaluateActive;
    for (const control of elements.ptEvaluateControls.querySelectorAll('input, select, button')) {
      control.disabled = !ptEvaluateActive;
    }
    for (const panel of [elements.ptEvaluateItemPanel, elements.ptEvaluateTeamPanel]) {
      panel.hidden = !ptEvaluateActive;
      for (const control of panel.querySelectorAll('input, select, button')) {
        control.disabled = !ptEvaluateActive;
      }
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
    renderPtEvaluateControls(player, event, ptEvaluateActive);
    elements.calculationModeHint.hidden = ptMaximizeActive || ptEvaluateActive;
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

  function renderPtEvaluateControls(player, event, active) {
    const variantsByEvent = {
      mission_live: [['solo', '自由演出']],
      live_try: [['solo', '自由演出']],
      challenge: [['solo', '自由演出'], ['challenge_cp', '挑战演出']],
      versus: [['solo', '自由演出'], ['versus', '竞演演出']],
      festival: [['solo', '自由演出']],
      medley: [['medley', '巡回演出']],
    };
    const config = player.ptEvaluate ?? {};
    const variants = variantsByEvent[event?.eventType] ?? [];
    const selected = ptEvaluateLiveVariant(config, event?.eventType);
    elements.ptEvaluateLiveVariant.replaceChildren(...variants.map(([value, text]) => {
      const label = document.createElement('label');
      const input = document.createElement('input');
      input.type = 'radio';
      input.name = 'pt-evaluate-live-variant';
      input.value = value;
      input.checked = value === selected;
      input.disabled = !active;
      const span = document.createElement('span');
      span.textContent = text;
      label.append(input, span);
      return label;
    }));
    const autoAllowed = ptEvaluateSupportsAuto(selected);
    const scoreMode = autoAllowed ? config.scoreMode ?? 'manual' : 'manual';
    setRadioValue(elements.ptEvaluateScoreMode, scoreMode);
    const scoreModeField = elements.ptEvaluateScoreMode.closest('fieldset');
    if (scoreModeField) {
      scoreModeField.hidden = !autoAllowed;
    }
    for (const input of elements.ptEvaluateScoreMode.querySelectorAll('input')) {
      input.disabled = !active || !autoAllowed;
    }
    elements.ptEvaluateAutoBaseMultiplier.value =
      String(config.autoBaseMultiplier ?? (player.server === 'jp' ? 0.75 : 0.5));
    const auto = autoAllowed && scoreMode === 'auto';
    elements.ptEvaluateAutoMultiplierField.hidden = !auto;
    elements.ptEvaluateAutoBaseMultiplier.required = auto;
    elements.ptEvaluateAutoBaseMultiplier.disabled = !active || !auto;
    const missionLive = event?.eventType === 'mission_live';
    elements.ptEvaluateMissionSupportField.hidden = !missionLive;
    elements.ptEvaluateMissionSupportPt.required = missionLive;
    elements.ptEvaluateMissionSupportPt.value = String(config.missionSupportPtBonus ?? 100);
    const versus = selected === 'versus';
    elements.ptEvaluateVersusRankField.hidden = !versus;
    setRadioValue(elements.ptEvaluateVersusRank, config.versusTeamRank ?? 0);
    renderPtEvaluateItemOptions(player, config.items ?? {});
    renderPtEvaluateTeams(player, config.teams ?? [], selected === 'medley', active);
  }

  function renderPtEvaluateItemOptions(player, selectedItems) {
    const groups = areaItemGroups(player);
    replaceAreaItemOptions(
      elements.ptEvaluateBandItem,
      groups.filter((group) => group.category === 'band'),
      selectedItems.band,
      player,
      elements.ptEvaluateBandItemPreview,
      elements.ptEvaluateBandItemName,
      elements.ptEvaluateBandItemRate,
      elements.ptEvaluateBandItemCycle,
    );
    replaceAreaItemOptions(
      elements.ptEvaluateAttributeItem,
      groups.filter((group) => group.category === 'attribute'),
      selectedItems.attribute,
      player,
      elements.ptEvaluateAttributeItemPreview,
      elements.ptEvaluateAttributeItemName,
      elements.ptEvaluateAttributeItemRate,
      elements.ptEvaluateAttributeItemCycle,
    );
    replaceAreaItemOptions(
      elements.ptEvaluateMagazineItem,
      groups.filter((group) => group.category === 'magazine'),
      selectedItems.magazine,
      player,
      elements.ptEvaluateMagazineItemPreview,
      elements.ptEvaluateMagazineItemName,
      elements.ptEvaluateMagazineItemRate,
      elements.ptEvaluateMagazineItemCycle,
    );
  }

  function replaceAreaItemOptions(
    select,
    groups,
    selected,
    player,
    preview,
    name,
    rate,
    trigger,
  ) {
    const options = groups.map((group) => {
      const option = document.createElement('option');
      option.value = group.key.split(':').slice(1).join(':');
      const unavailable = group.areaItemIds.some((id) =>
        !['59', '72'].includes(String(id))
          && Number(player?.areaItem?.[String(id)]?.level ?? 0) <= 0,
      );
      option.textContent = unavailable ? `${group.label}（含 0 级）` : group.label;
      option.dataset.unavailable = unavailable ? '1' : '0';
      return option;
    });
    select.replaceChildren(...options);
    const wanted = String(selected ?? '');
    const matching = options.find((option) => option.value === wanted);
    const fallback = options.find((option) => option.dataset.unavailable !== '1') ?? options[0];
    select.value = (matching ?? fallback)?.value ?? '';
    const selectedGroup = groups.find((group) =>
      group.key.split(':').slice(1).join(':') === select.value,
    );
    const image = assetImage(
      areaItemGroupIconUrls(selectedGroup),
      'pt-evaluate-item-image',
      selectedGroup?.label ?? '',
    );
    preview.replaceChildren(...(image ? [image] : []));
    preview.hidden = !image;
    trigger.classList.toggle('without-image', !image);
    name.textContent = selectedGroup?.label ?? '无可用道具';
    rate.textContent = selectedGroup
      ? `加成 ${formatAreaItemRate(selectedGroup.rate)}`
      : '';
    trigger.title = selectedGroup
      ? `当前：${selectedGroup.label}。左键切换下一项，右键返回上一项`
      : '没有可用道具';
  }

  function renderPtEvaluateTeams(player, teams, medley, active) {
    const teamCount = medley ? 3 : 1;
    const fragment = document.createDocumentFragment();
    for (let teamIndex = 0; teamIndex < teamCount; teamIndex += 1) {
      const section = document.createElement('section');
      section.className = 'pt-evaluate-team-card';
      section.dataset.teamIndex = String(teamIndex);
      const header = document.createElement('div');
      header.className = 'pt-evaluate-team-card-header';
      const title = document.createElement('h4');
      title.textContent = medley ? `第 ${teamIndex + 1} 曲队伍` : '队伍';
      const importButton = document.createElement('button');
      importButton.type = 'button';
      importButton.className = 'compact-button pt-evaluate-import-main-band';
      importButton.dataset.teamIndex = String(teamIndex);
      importButton.textContent = '导入主乐队';
      importButton.disabled = !active;
      header.append(title, importButton);

      const cardGrid = document.createElement('div');
      cardGrid.className = 'pt-evaluate-card-grid';
      for (let cardIndex = 0; cardIndex < 5; cardIndex += 1) {
        const label = document.createElement('label');
        label.className = 'pt-evaluate-card-choice';
        label.classList.toggle('is-captain', cardIndex === 2);
        const caption = document.createElement('span');
        caption.className = 'pt-evaluate-card-choice-label';
        caption.textContent = cardIndex === 2 ? '卡位 3 · 队长' : `卡位 ${cardIndex + 1}`;
        const preview = document.createElement('span');
        preview.className = 'pt-evaluate-card-choice-preview';
        const input = document.createElement('input');
        input.className = 'pt-evaluate-card-input';
        input.dataset.teamIndex = String(teamIndex);
        input.dataset.cardIndex = String(cardIndex);
        input.setAttribute('list', 'card-options');
        input.placeholder = '选择卡牌';
        input.disabled = !active;
        const cardId = Number(teams[teamIndex]?.[cardIndex]) || 0;
        input.value = cardId > 0 ? String(cardId) : '';
        input.setAttribute(
          'aria-label',
          `${medley ? `第 ${teamIndex + 1} 曲` : ''}${caption.textContent}`,
        );
        input.title = cardId > 0 ? cardLabel(cardId) : '';
        if (cardId > 0) {
          preview.append(cardPreviewContent({
            id: cardId,
            name: cardName(cardId),
            rarity: cardRarity(cardId),
            attribute: cardAttribute(cardId),
            imageUrls: cardIconUrls(cardId, player.cardList?.[String(cardId)]),
          }));
        } else {
          const empty = document.createElement('span');
          empty.className = 'pt-evaluate-card-choice-empty';
          empty.textContent = '未选择卡牌';
          preview.append(empty);
        }
        label.append(caption, preview, input);
        cardGrid.append(label);
      }
      const hint = document.createElement('small');
      hint.className = 'pt-evaluate-import-hint';
      hint.textContent = '导入会覆盖当前队伍的卡牌配置、区域道具等级及角色加成等级，并更新当前道具选择。';
      section.append(header, cardGrid, hint);
      fragment.append(section);
    }
    elements.ptEvaluateTeams.replaceChildren(fragment);
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
