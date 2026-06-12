import {
  attributeIconUrls,
  bandIconUrls,
  characterIconUrls,
  assetImage,
  serverIconUrls,
  starIconUrls,
} from '../assets/index.js?v=1';
import { attributeSwatch } from '../ui/attribute.js?v=1';
import { cardPreviewItem as createCardPreviewItem } from '../ui/card-preview.js?v=1';
import { emptyMessage } from '../ui/dom.js?v=1';
import {
  attributeLabel,
  numericStringSort,
  optionText,
  positiveIntegerOrUndefined,
} from '../utils.js?v=1';

const ATTRIBUTE_FILTERS = ['powerful', 'cool', 'happy', 'pure'];
const RARITY_FILTERS = [1, 2, 3, 4, 5];
const SERVER_FILTERS = [
  { value: 'jp', label: 'JP' },
  { value: 'en', label: 'EN' },
  { value: 'tw', label: 'TW' },
  { value: 'cn', label: 'CN' },
  { value: 'kr', label: 'KR' },
];
const CARD_PREVIEW_BATCH_SIZE = 50;
const CARD_SEARCH_DEBOUNCE_MS = 160;
const CARD_SEARCH_FRAME_BUDGET_MS = 8;
const CARD_SEARCH_WARMUP_BUDGET_MS = 12;

export function createReferenceView({
  elements,
  currentServer,
  getCore,
  getPlayer,
  cardLabel,
  cardName,
  cardIconUrls,
  songLabel,
  areaItemLabel,
  characterLabel,
  supportedEventRecords,
  eventLabel,
  normalizedActivityMode,
}) {
  const cardFilters = {
    bands: null,
    attributes: null,
    rarities: null,
    characters: null,
    ownership: null,
    releaseServers: null,
  };
  const selectedPreviewCardIds = new Set();
  let cardPreviewIndexKey = '';
  let cardPreviewIndex = [];
  let cardPreviewIdListKey = '';
  let cardPreviewIdList = [];
  let cardPreviewVisibleCount = CARD_PREVIEW_BATCH_SIZE;
  let scheduledCardPreviewFrame = 0;
  let scheduledCardPreviewTimeout = 0;
  let cardPreviewSearchToken = 0;
  let cardSearchWarmupKey = '';
  let cardSearchWarmupToken = 0;
  let ownedCardIds = new Set();

  function renderReferenceOptions() {
    const server = currentServer();
    const core = getCore();
    bindCardAddPanelToggle();
    if (cardAddPanelActive()) {
      renderCardAddFilters(core);
      scheduleCardAddPreviewRender({ idle: true });
    }
    if (!cardPageActive()) {
      renderOptionsCached(
        elements.cardOptions,
        releasedCardRecords(core?.cards),
        cardLabel,
        `cards:${server}`,
      );
    }
    // Songs use the paged combobox in form.js; keeping a full datalist here
    // creates thousands of unused <option> nodes on first data load.
    if (elements.songOptions) {
      clearOptionsCache(elements.songOptions, `songs:${server}:lazy`);
    }
    renderOptionsCached(
      elements.areaItemOptions,
      core?.areaItems,
      areaItemLabel,
      `areaItems:${server}`,
    );
    renderOptionsCached(
      elements.characterOptions,
      core?.characters,
      characterLabel,
      `characters:${server}`,
    );
    renderOptionsCached(
      elements.eventOptions,
      supportedEventRecords(),
      eventLabel,
      `events:${server}:${normalizedActivityMode(elements.activityMode.value)}`,
      {
        descending: true,
      },
    );
  }

  return {
    matchingCardPreviewIds,
    renderReferenceOptions,
    warmupCardSearchIndex,
  };

  function renderCardAddFilters(core) {
    const container = elements.cardAddFilters;
    if (!container) {
      return;
    }
    if (!core?.cards || !core?.characters) {
      container.textContent = '';
      return;
    }
    bindCardSearchInput();
    const key = cardIndexKey(core);
    if (container.dataset.filterControlsKey === key) {
      updateCardFilterStates();
      return;
    }
    container.textContent = '';
    container.dataset.filterControlsKey = key;
    container.append(
      filterRow('乐队', 'bands', bandOptions(core), (option) =>
        assetImage(bandIconUrls(option.value), 'card-filter-icon', option.label),
      ),
      filterRow('属性', 'attributes', ATTRIBUTE_FILTERS.map((attribute) => ({
        value: attribute,
        label: attributeLabel(attribute),
      })), (option) =>
        assetImage(attributeIconUrls(option.value), 'card-filter-icon', option.label),
      ),
      filterRow('稀有度', 'rarities', RARITY_FILTERS.map((rarity) => ({
        value: rarity,
        label: `${rarity}星`,
      })), (option) =>
        assetImage(starIconUrls(option.value), 'card-filter-icon card-filter-star', option.label),
      ),
      filterRow('角色', 'characters', characterOptions(core), (option) =>
        assetImage(characterIconUrls(option.value), 'card-filter-icon', option.label),
      ),
      filterRow('有/无', 'ownership', [
        { value: 'owned', label: '已有' },
        { value: 'missing', label: '未有' },
      ], (option) => ownershipIcon(option.value)),
      filterRow('服务器', 'releaseServers', SERVER_FILTERS, (option) => serverIcon(option)),
    );
  }

  function filterRow(label, key, options, iconFn) {
    const row = document.createElement('section');
    row.className = `card-filter-row card-filter-row-${key}`;
    const title = document.createElement('div');
    title.className = 'card-filter-label';
    title.textContent = label;
    const buttons = document.createElement('div');
    buttons.className = 'card-filter-buttons';
    for (const option of options) {
      buttons.append(filterButton(key, option, options, iconFn));
    }
    buttons.append(allButton(key, options));
    row.append(title, buttons);
    return row;
  }

  function filterButton(key, option, options, iconFn) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'card-filter-button';
    button.dataset.filterKey = key;
    button.dataset.filterValue = String(option.value);
    button.classList.toggle('is-active', filterIncludes(key, option.value));
    button.title = option.label;
    button.setAttribute('aria-label', option.label);
    const icon = iconFn(option);
    if (icon) {
      button.append(icon);
    } else {
      button.textContent = option.label;
    }
    button.addEventListener('click', () => {
      const current = explicitFilterSet(key, options);
      const value = String(option.value);
      if (current.has(value)) {
        current.delete(value);
      } else {
        current.add(value);
      }
      cardFilters[key] = current.size === options.length ? null : current;
      resetCardPreviewWindow();
      updateCardFilterStates();
      scheduleCardAddPreviewRender();
    });
    return button;
  }

  function renderCardAddPreview(core) {
    const preview = elements.cardAddPreview;
    if (!preview) {
      return;
    }
    preview.textContent = '';
    if (!core?.cards || !core?.characters) {
      return;
    }
    bindCardSearchInput();
    refreshOwnedCardIdsIfNeeded();
    const query = currentCardSearchQuery();
    if (query) {
      renderCardAddSearchPreview(core, query);
      return;
    }
    cardPreviewSearchToken += 1;
    const results = filteredCardEntries(core, cardPreviewVisibleCount);
    renderCardPreviewResults(results);
    scheduleCardSearchWarmup(core);
  }

  function renderCardPreviewResults(results) {
    const preview = elements.cardAddPreview;
    if (!preview) {
      return;
    }
    const visibleIds = new Set(results.map((entry) => entry.id));
    for (const selectedId of [...selectedPreviewCardIds]) {
      if (!visibleIds.has(selectedId)) {
        selectedPreviewCardIds.delete(selectedId);
      }
    }
    if (results.length === 0) {
      preview.append(emptyMessage('没有匹配卡牌', 'card-preview-empty'));
      return;
    }
    const fragment = document.createDocumentFragment();
    for (const entry of results) {
      fragment.append(cardPreviewItem(entry));
    }
    preview.append(fragment);
  }

  function renderCardAddSearchPreview(core, query) {
    const preview = elements.cardAddPreview;
    if (!preview) {
      return;
    }
    const token = cardPreviewSearchToken + 1;
    cardPreviewSearchToken = token;
    if (cardSearchIndexReady(core)) {
      renderCardPreviewResults(searchCardEntries(core, query, cardPreviewVisibleCount));
      return;
    }
    const entries = cardIndex(core);
    if (cardSearchIndexReady(core)) {
      renderCardPreviewResults(searchCardEntries(core, query, cardPreviewVisibleCount));
      return;
    }
    const results = [];
    let index = 0;

    const step = () => {
      if (token !== cardPreviewSearchToken) {
        return;
      }
      const startedAt = performance.now();
      while (
        index < entries.length
        && performance.now() - startedAt < CARD_SEARCH_FRAME_BUDGET_MS
      ) {
        const entry = entries[index];
        index += 1;
        if (!cardEntryMatchesFilters(entry) || !cardEntryMatchesQuery(entry, query)) {
          continue;
        }
        results.push(entry);
        if (results.length >= cardPreviewVisibleCount) {
          break;
        }
      }
      if (index < entries.length && results.length < cardPreviewVisibleCount) {
        requestAnimationFrame(step);
        return;
      }
      if (token !== cardPreviewSearchToken) {
        return;
      }
      renderCardPreviewResults(results);
    };

    requestAnimationFrame(step);
  }

  function cardPreviewItem(entry) {
    const nameText = cardName(entry.id);
    const item = createCardPreviewItem({
      id: entry.id,
      name: nameText,
      rarity: entry.rarity,
      attribute: entry.attribute,
      imageUrls: cardIconUrls(entry.id, { illustTrainingStatus: previewIllustTrainingStatus() }),
      selected: selectedPreviewCardIds.has(entry.id),
      interactive: true,
    });
    setPreviewButtonSelected(item, selectedPreviewCardIds.has(entry.id));
    return item;
  }

  function handlePreviewSelect(event) {
    const item = event.target.closest?.('.card-preview-item');
    if (!item || !elements.cardAddPreview?.contains(item)) {
      return;
    }
    event.preventDefault();
    const id = item.dataset.cardId;
    if (!id) {
      return;
    }
    if (selectedPreviewCardIds.has(id)) {
      selectedPreviewCardIds.delete(id);
      setPreviewButtonSelected(item, false);
    } else {
      selectedPreviewCardIds.add(id);
      setPreviewButtonSelected(item, true);
    }
  }

  function setPreviewButtonSelected(button, selected) {
    button.classList.toggle('is-selected', selected);
    button.setAttribute('aria-pressed', selected ? 'true' : 'false');
  }

  function bindCardSearchInput() {
    const input = elements.newCardId;
    if (input && input.dataset.cardSearchBound !== '1') {
      input.dataset.cardSearchBound = '1';
      input.addEventListener('input', () => {
        resetCardPreviewWindow();
        scheduleCardAddPreviewRender({ debounce: true });
      });
    }
    const illust = elements.defaultCardIllust;
    if (illust && illust.dataset.cardPreviewIllustBound !== '1') {
      illust.dataset.cardPreviewIllustBound = '1';
      illust.addEventListener('change', () => scheduleCardAddPreviewRender());
    }
    const preview = elements.cardAddPreview;
    if (preview && preview.dataset.cardPreviewBound !== '1') {
      preview.dataset.cardPreviewBound = '1';
      preview.addEventListener('click', handlePreviewSelect);
      preview.addEventListener('scroll', handleCardPreviewScroll);
      preview.addEventListener('keydown', (event) => {
        if (event.key !== 'Enter' && event.key !== ' ') {
          return;
        }
        handlePreviewSelect(event);
      });
      preview.addEventListener('card-preview-clear', () => {
        selectedPreviewCardIds.clear();
        resetCardPreviewWindow();
        scheduleCardAddPreviewRender();
      });
    }
  }

  function bindCardAddPanelToggle() {
    const button = elements.toggleCardAddPanel;
    const content = elements.cardAddContent;
    if (!button || !content || button.dataset.cardAddPanelBound === '1') {
      return;
    }
    button.dataset.cardAddPanelBound = '1';
    setCardAddPanelExpanded(!content.hidden);
    button.addEventListener('click', () => {
      const expanded = content.hidden;
      setCardAddPanelExpanded(expanded);
      if (expanded) {
        renderReferenceOptions();
      } else {
        cancelCardPreviewRender();
      }
    });
  }

  function setCardAddPanelExpanded(expanded) {
    const button = elements.toggleCardAddPanel;
    const content = elements.cardAddContent;
    if (!button || !content) {
      return;
    }
    content.hidden = !expanded;
    button.setAttribute('aria-expanded', expanded ? 'true' : 'false');
    const label = button.querySelector('span');
    if (label) {
      label.textContent = expanded ? '收起' : '展开';
    }
  }

  function handleCardPreviewScroll() {
    const preview = elements.cardAddPreview;
    if (!preview || preview.scrollTop + preview.clientHeight < preview.scrollHeight - 80) {
      return;
    }
    const nextCount = cardPreviewVisibleCount + CARD_PREVIEW_BATCH_SIZE;
    refreshOwnedCardIdsIfNeeded();
    if (currentCardSearchQuery()) {
      cardPreviewVisibleCount = nextCount;
      scheduleCardAddPreviewRender();
      return;
    }
    const entries = filteredCardEntries(getCore(), nextCount);
    if (entries.length <= cardPreviewVisibleCount) {
      return;
    }
    const fragment = document.createDocumentFragment();
    for (const entry of entries.slice(cardPreviewVisibleCount)) {
      fragment.append(cardPreviewItem(entry));
    }
    cardPreviewVisibleCount = nextCount;
    preview.append(fragment);
  }

  function resetCardPreviewWindow() {
    cardPreviewVisibleCount = CARD_PREVIEW_BATCH_SIZE;
    if (elements.cardAddPreview) {
      elements.cardAddPreview.scrollTop = 0;
    }
  }

  function scheduleCardAddPreviewRender({ debounce = false, idle = false } = {}) {
    if (scheduledCardPreviewTimeout) {
      clearTimeout(scheduledCardPreviewTimeout);
      scheduledCardPreviewTimeout = 0;
    }
    if (debounce) {
      scheduledCardPreviewTimeout = setTimeout(() => {
        scheduledCardPreviewTimeout = 0;
        scheduleCardAddPreviewRender();
      }, CARD_SEARCH_DEBOUNCE_MS);
      return;
    }
    if (scheduledCardPreviewFrame) {
      cancelAnimationFrame(scheduledCardPreviewFrame);
    }
    cardPreviewSearchToken += 1;
    const scheduledToken = cardPreviewSearchToken;
    const schedule = idle
      ? (callback) => requestIdle(() => requestAnimationFrame(callback))
      : requestAnimationFrame;
    scheduledCardPreviewFrame = schedule(() => {
      scheduledCardPreviewFrame = 0;
      if (scheduledToken !== cardPreviewSearchToken) {
        return;
      }
      renderCardAddPreview(getCore());
    });
  }

  function cancelCardPreviewRender() {
    if (scheduledCardPreviewTimeout) {
      clearTimeout(scheduledCardPreviewTimeout);
      scheduledCardPreviewTimeout = 0;
    }
    if (scheduledCardPreviewFrame) {
      cancelAnimationFrame(scheduledCardPreviewFrame);
      scheduledCardPreviewFrame = 0;
    }
    cardPreviewSearchToken += 1;
  }

  function allButton(key, options) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'card-filter-all';
    button.dataset.filterKey = key;
    button.dataset.filterAll = '1';
    button.classList.toggle('is-active', cardFilters[key] == null);
    button.textContent = '全选';
    button.addEventListener('click', () => {
      cardFilters[key] = cardFilters[key] == null
        ? new Set()
        : null;
      resetCardPreviewWindow();
      updateCardFilterStates();
      scheduleCardAddPreviewRender();
    });
    return button;
  }

  function renderCardAddControls(core) {
    renderCardAddFilters(core);
    renderCardAddPreview(core);
  }

  function matchingCardPreviewIds() {
    const core = getCore();
    if (!core?.cards || !core?.characters) {
      return [];
    }
    refreshOwnedCardIdsIfNeeded();
    return filteredCardEntries(core, Number.MAX_SAFE_INTEGER)
      .map((entry) => Number.parseInt(entry.id, 10))
      .filter((cardId) => Number.isInteger(cardId) && cardId > 0);
  }

  function updateCardFilterStates() {
    const container = elements.cardAddFilters;
    if (!container) {
      return;
    }
    for (const button of container.querySelectorAll('[data-filter-key]')) {
      const key = button.dataset.filterKey;
      if (button.dataset.filterAll === '1') {
        button.classList.toggle('is-active', cardFilters[key] == null);
      } else {
        button.classList.toggle('is-active', filterIncludes(key, button.dataset.filterValue));
      }
    }
  }

  function cardAddPanelActive() {
    if (!cardPageActive() || elements.cardAddContent?.hidden) {
      return false;
    }
    return true;
  }

  function cardPageActive() {
    const panel = elements.cardAddPreview?.closest('[data-page-panel]');
    return panel == null || !panel.hidden;
  }

  function filterIncludes(key, value) {
    const selected = cardFilters[key];
    return selected == null || selected.has(String(value));
  }

  function explicitFilterSet(key, options) {
    const selected = cardFilters[key];
    if (selected != null) {
      return new Set(selected);
    }
    return new Set(options.map((option) => String(option.value)));
  }

  function filteredCardEntries(core, limit) {
    const query = currentCardSearchQuery();
    if (!query) {
      return defaultCardEntries(core, limit);
    }
    return searchCardEntries(core, query, limit);
  }

  function searchCardEntries(core, query, limit) {
    const results = [];
    for (const entry of cardIndex(core)) {
      if (!cardEntryMatchesFilters(entry) || !cardEntryMatchesQuery(entry, query)) {
        continue;
      }
      results.push(entry);
      if (results.length >= limit) {
        break;
      }
    }
    return results;
  }

  function bucketedCardEntries(buckets, limit) {
    const results = [];
    for (let rarity = 5; rarity >= 1; rarity -= 1) {
      for (const entry of buckets.get(rarity) ?? []) {
        results.push(entry);
        if (results.length >= limit) {
          return results;
        }
      }
    }
    return results;
  }

  function defaultCardEntries(core, limit) {
    const records = core?.cards ?? {};
    const buckets = new Map();
    for (const cardId of cardIdsDescending(core)) {
      const card = records[cardId];
      const rarity = positiveIntegerOrUndefined(card?.rarity) ?? 0;
      if (rarity < 1 || rarity > 5) {
        continue;
      }
      const entry = cardIndexEntry(cardId, card, core);
      if (!cardEntryMatchesFilters(entry)) {
        continue;
      }
      const bucket = buckets.get(rarity) ?? [];
      bucket.push(entry);
      buckets.set(rarity, bucket);
      if (rarity === 5 && bucket.length >= limit) {
        break;
      }
    }
    return bucketedCardEntries(buckets, limit);
  }

  function cardIdsDescending(core) {
    const key = cardIndexKey(core);
    if (cardPreviewIdListKey === key) {
      return cardPreviewIdList;
    }
    cardPreviewIdListKey = key;
    cardPreviewIdList = Object.keys(core?.cards ?? {})
      .filter((cardId) => cardReleasedOnAnyServer(core.cards[cardId]))
      .sort((left, right) => Number(right) - Number(left));
    return cardPreviewIdList;
  }

  function cardIndex(core) {
    const key = cardIndexKey(core);
    if (cardPreviewIndexKey === key) {
      return cardPreviewIndex;
    }
    cardPreviewIndexKey = key;
    cardPreviewIndex = Object.entries(core?.cards ?? {})
      .filter(([, card]) => cardReleasedOnAnyServer(card))
      .map(([cardId, card]) => cardIndexEntry(cardId, card, core))
      .sort(cardPreviewSort);
    cardSearchWarmupKey = key;
    cardSearchWarmupToken += 1;
    return cardPreviewIndex;
  }

  function cardIndexKey(core) {
    return [
      currentServer(),
      Object.keys(core?.cards ?? {}).length,
      Object.keys(core?.cardsFix ?? {}).length,
      Object.keys(core?.characters ?? {}).length,
    ].join(':');
  }

  function cardIndexEntry(cardId, card, core) {
    const id = String(cardId);
    const characterId = positiveIntegerOrUndefined(card?.characterId);
    const character = characterId == null ? undefined : core?.characters?.[String(characterId)];
    const rarity = positiveIntegerOrUndefined(card?.rarity) ?? 0;
    return {
      id,
      idNumber: Number(cardId),
      characterId: characterId == null ? '' : String(characterId),
      bandId: positiveIntegerOrUndefined(character?.bandId),
      attribute: card?.attribute,
      rarity,
      releasedAt: Array.isArray(card?.releasedAt) ? card.releasedAt : undefined,
      searchText: cardSearchText(id, card, core),
    };
  }

  function cardEntryMatchesQuery(entry, query) {
    if (!query) {
      return true;
    }
    return ensureCardSearchText(entry).includes(query);
  }

  function currentCardSearchQuery() {
    return String(elements.newCardId?.value ?? '').trim().toLowerCase();
  }

  function previewIllustTrainingStatus() {
    return elements.defaultCardIllust?.checked ?? true;
  }

  function scheduleCardSearchWarmup(core) {
    const key = cardIndexKey(core);
    if (cardSearchWarmupKey === key || currentCardSearchQuery()) {
      return;
    }
    const entries = cardIndex(core);
    const token = cardSearchWarmupToken + 1;
    cardSearchWarmupToken = token;
    let index = entries.findIndex((entry) => entry.searchText == null);
    if (index < 0) {
      cardSearchWarmupKey = key;
      return;
    }

    const run = (deadline) => {
      if (token !== cardSearchWarmupToken || currentCardSearchQuery()) {
        return;
      }
      const startedAt = performance.now();
      while (
        index < entries.length
        && idleTimeRemaining(deadline) > 1
        && performance.now() - startedAt < CARD_SEARCH_WARMUP_BUDGET_MS
      ) {
        ensureCardSearchText(entries[index]);
        index += 1;
      }
      while (index < entries.length && entries[index].searchText != null) {
        index += 1;
      }
      if (index >= entries.length) {
        cardSearchWarmupKey = key;
        return;
      }
      requestIdle(run);
    };

    requestIdle(run);
  }

  function warmupCardSearchIndex() {
    if (!cardAddPanelActive()) {
      return;
    }
    const core = getCore();
    if (!core?.cards || !core?.characters) {
      return;
    }
    requestIdle(() => scheduleCardSearchWarmup(core));
  }

  function cardSearchIndexReady(core) {
    return cardSearchWarmupKey === cardIndexKey(core);
  }

  function ensureCardSearchText(entry) {
    if (entry.searchText == null) {
      const name = cardName(entry.id);
      entry.searchText = `${entry.id} ${name}`.toLowerCase();
    }
    return entry.searchText;
  }

  function cardSearchText(cardId, card, core) {
    const parts = [String(cardId)];
    appendSearchText(parts, card?.prefix);
    appendSearchText(parts, core?.cardsFix?.[String(cardId)]?.prefix);
    return [...new Set(parts.filter(Boolean))].join(' ').toLowerCase();
  }

  function appendSearchText(parts, value) {
    if (Array.isArray(value)) {
      for (const item of value) {
        appendSearchText(parts, item);
      }
      return;
    }
    const text = String(value ?? '').trim();
    if (text) {
      parts.push(text);
    }
  }

  function releasedCardRecords(records) {
    if (!records) {
      return records;
    }
    return Object.fromEntries(
      Object.entries(records).filter(([, card]) => cardReleasedOnAnyServer(card)),
    );
  }

  function cardReleasedOnAnyServer(card) {
    if (!Array.isArray(card?.releasedAt)) {
      return true;
    }
    const now = Date.now();
    return card.releasedAt.some((timestamp) => {
      const releasedAt = Number(timestamp);
      return Number.isFinite(releasedAt) && releasedAt > 0 && releasedAt <= now;
    });
  }

  function cardPreviewSort(left, right) {
    const rarityOrder = right.rarity - left.rarity;
    if (rarityOrder !== 0) {
      return rarityOrder;
    }
    return right.idNumber - left.idNumber;
  }

  function cardEntryMatchesFilters(entry) {
    if (
      !filterIncludes('characters', entry.characterId)
      || !filterIncludes('bands', entry.bandId)
      || !filterIncludes('attributes', entry.attribute)
      || !filterIncludes('rarities', entry.rarity)
      || !cardEntryMatchesReleaseServerFilter(entry)
    ) {
      return false;
    }
    if (cardFilters.ownership == null) {
      return true;
    }
    return filterIncludes('ownership', cardOwned(entry.id) ? 'owned' : 'missing');
  }

  function cardEntryMatchesReleaseServerFilter(entry) {
    const selected = cardFilters.releaseServers;
    if (selected == null) {
      return true;
    }
    for (const server of selected) {
      if (cardReleasedOnServer(entry.releasedAt, server)) {
        return true;
      }
    }
    return false;
  }

  function cardFiltersAllSelected() {
    return cardFilters.bands == null
      && cardFilters.attributes == null
      && cardFilters.rarities == null
      && cardFilters.characters == null
      && cardFilters.ownership == null
      && cardFilters.releaseServers == null;
  }

  function cardOwned(cardId) {
    return ownedCardIds.has(String(cardId));
  }

  function refreshOwnedCardIdsIfNeeded() {
    if (cardFilters.ownership == null) {
      return;
    }
    refreshOwnedCardIds();
  }

  function refreshOwnedCardIds() {
    try {
      ownedCardIds = new Set(Object.keys(getPlayer?.()?.cardList ?? {}));
    } catch {
      ownedCardIds = new Set();
    }
  }

  function bandOptions(core) {
    const bands = new Map();
    for (const character of Object.values(core?.characters ?? {})) {
      const bandId = positiveIntegerOrUndefined(character?.bandId);
      if (bandId == null || bandId >= 1000 || bands.has(bandId)) {
        continue;
      }
      bands.set(bandId, {
        value: bandId,
        label: `乐队 ${bandId}`,
      });
    }
    return [...bands.values()].sort((left, right) => Number(left.value) - Number(right.value));
  }

  function characterOptions(core) {
    return Object.keys(core?.characters ?? {})
      .sort(numericStringSort)
      .map((characterId) => ({
        value: characterId,
        label: characterLabel(characterId),
      }));
  }

}

function serverIcon(option) {
  return assetImage(
    serverIconUrls(option.value),
    'card-filter-icon card-filter-server-icon',
    option.label,
  );
}

function ownershipIcon(value) {
  const icon = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  icon.setAttribute('class', `card-filter-icon ownership-icon ownership-${value}`);
  icon.setAttribute('viewBox', '0 0 24 24');
  icon.setAttribute('aria-hidden', 'true');
  const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  path.setAttribute('fill', 'currentColor');
  path.setAttribute(
    'd',
    value === 'owned'
      ? 'M9.5 16.7 4.8 12l1.4-1.4 3.3 3.3 8.3-8.3L19.2 7 9.5 16.7Z'
      : 'M6.4 5 12 10.6 17.6 5 19 6.4 13.4 12 19 17.6 17.6 19 12 13.4 6.4 19 5 17.6 10.6 12 5 6.4 6.4 5Z',
  );
  icon.append(path);
  return icon;
}

function cardReleasedOnServer(releasedAt, server) {
  if (!Array.isArray(releasedAt)) {
    return false;
  }
  const index = serverIndex(server);
  if (index == null) {
    return false;
  }
  const timestamp = Number(releasedAt[index]);
  return Number.isFinite(timestamp) && timestamp > 0 && timestamp <= Date.now();
}

function serverIndex(server) {
  switch (String(server ?? '').toLowerCase()) {
    case 'jp':
      return 0;
    case 'en':
      return 1;
    case 'tw':
      return 2;
    case 'cn':
      return 3;
    case 'kr':
      return 4;
    default:
      return undefined;
  }
}

function requestIdle(callback) {
  if (typeof requestIdleCallback === 'function') {
    requestIdleCallback(callback, { timeout: 500 });
    return;
  }
  requestAnimationFrame(() => callback({ timeRemaining: () => CARD_SEARCH_WARMUP_BUDGET_MS }));
}

function idleTimeRemaining(deadline) {
  return typeof deadline?.timeRemaining === 'function'
    ? deadline.timeRemaining()
    : CARD_SEARCH_WARMUP_BUDGET_MS;
}

export function installRecoveringDatalistInput(input) {
  if (!input || input.dataset.recoveringDatalist === '1') {
    return;
  }
  input.dataset.recoveringDatalist = '1';

  input.addEventListener('pointerdown', () => {
    if (input.disabled || document.activeElement === input) {
      return;
    }
    input.dataset.recoveringPreviousValue = input.value;
    input.dataset.recoveringActive = '1';
    input.value = '';
  });
  input.addEventListener('change', () => {
    if (input.value.trim()) {
      input.dataset.recoveringActive = '0';
    }
  });
  input.addEventListener('blur', () => {
    if (input.dataset.recoveringActive === '1' && !input.value.trim()) {
      input.value = input.dataset.recoveringPreviousValue ?? '';
    }
    input.dataset.recoveringActive = '0';
  });
  input.addEventListener('keydown', (event) => {
    if (event.key !== 'Escape' || input.dataset.recoveringActive !== '1') {
      return;
    }
    input.value = input.dataset.recoveringPreviousValue ?? '';
    input.dataset.recoveringActive = '0';
    input.blur();
  });
}

function renderOptionsCached(element, records, labelFn, cacheKey, options = {}) {
  const count = records ? Object.keys(records).length : 0;
  const key = `${cacheKey}:${count}`;
  if (element.dataset.optionsKey === key) {
    return;
  }
  element.dataset.optionsKey = key;
  renderOptions(element, records, labelFn, options);
}

function clearOptionsCache(element, cacheKey) {
  if (!element || element.dataset.optionsKey === cacheKey) {
    return;
  }
  element.textContent = '';
  element.dataset.optionsKey = cacheKey;
}

function renderOptions(element, records, labelFn, { descending = false } = {}) {
  element.textContent = '';
  if (!records) {
    return;
  }

  const ids = Object.keys(records).sort(numericStringSort);
  if (descending) {
    ids.reverse();
  }

  const fragment = document.createDocumentFragment();
  for (const id of ids) {
    const label = labelFn(id);
    const text = optionText(id, label);
    const option = document.createElement('option');
    option.value = text;
    option.textContent = text;
    fragment.append(option);
  }
  element.append(fragment);
}
