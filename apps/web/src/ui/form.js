import { attributeIconUrls, assetImage } from '../assets/index.js?v=2';
import { emptyMessage, inputCell as baseInputCell } from './dom.js?v=2';
import { clearFieldValidationMessage, setFieldValidationMessage } from './validation.js?v=2';
import {
  ATTRIBUTE_VALUES_WITH_ALL,
  attributeLabel,
  numericStringSort,
  parseEntityId,
  positiveIntegerOrUndefined,
} from '../utils.js?v=2';

export function createFormCells({
  attributeFallback,
  songCoverUrls,
  songLabel,
  songSearchValue,
  getSongRecords = () => ({}),
  installRecoveringDatalistInput,
}) {
  const songPopupPageSize = 80;
  const songSearchDebounceMs = 160;
  const songSearchFrameBudgetMs = 8;
  const songSearchWarmupBudgetMs = 12;
  let cachedSongRecords;
  let cachedSongIds = [];
  let cachedSongEntries = new Map();
  let songSearchWarmupRecords;
  let songSearchWarmupToken = 0;

  function entityCell(id, label, { imageUrls } = {}) {
    const cell = document.createElement('td');
    cell.className = 'entity-cell';
    const content = document.createElement('div');
    content.className = 'entity-content';
    const icon = assetImage(imageUrls, 'entity-icon', label);
    const text = document.createElement('span');
    text.className = 'entity-text';
    const name = document.createElement('span');
    name.className = 'entity-name';
    name.textContent = label;
    const meta = document.createElement('span');
    meta.className = 'entity-meta';
    meta.textContent = `ID ${id}`;
    text.append(name, meta);
    if (icon) {
      content.append(icon);
    }
    content.append(text);
    cell.append(content);
    return cell;
  }

  function inputCell({ value, min, max, step, mode = 'integer', className, disabled = false, onChange }) {
    return baseInputCell({
      value,
      min,
      max,
      step,
      mode,
      className,
      disabled,
      onChange,
    });
  }

  function statRateCell(value, onChange) {
    return inputCell({
      value,
      min: 0,
      step: 0.01,
      mode: 'float',
      className: 'stat-rate-input',
      onChange,
    });
  }

  function percentCell(value, onChange, { disabled = false } = {}) {
    return inputCell({
      value,
      min: 0,
      step: 0.01,
      mode: 'float',
      className: 'stat-rate-input',
      disabled,
      onChange,
    });
  }

  function attributeCell(value, onChange, { disabled = false } = {}) {
    const cell = document.createElement('td');
    const wrap = document.createElement('div');
    wrap.className = 'attribute-select-cell';
    let icon = assetImage(attributeIconUrls(value), 'attribute-icon', attributeLabel(value))
      ?? attributeFallback(value);
    const select = document.createElement('select');
    select.disabled = disabled;
    for (const attribute of ATTRIBUTE_VALUES_WITH_ALL) {
      const option = document.createElement('option');
      option.value = attribute;
      option.textContent = attributeLabel(attribute);
      option.selected = attribute === value;
      select.append(option);
    }
    select.addEventListener('change', () => {
      const nextIcon = assetImage(attributeIconUrls(select.value), 'attribute-icon', attributeLabel(select.value))
        ?? attributeFallback(select.value);
      icon.replaceWith(nextIcon);
      icon = nextIcon;
      onChange(select.value);
    });
    wrap.append(icon, select);
    cell.append(wrap);
    return cell;
  }

  function songSelectCell(value, onChange, { presetSongs = [] } = {}) {
    const cell = document.createElement('td');
    const selected = positiveIntegerOrUndefined(value) ?? 0;
    const wrap = document.createElement('div');
    wrap.className = 'song-input-cell';
    const cover = selected
      ? assetImage(songCoverUrls(selected), 'entity-icon song-inline-cover', songLabel(selected))
      : null;
    const combo = document.createElement('div');
    combo.className = 'song-select-combobox';
    const input = document.createElement('input');
    input.autocomplete = 'off';
    input.placeholder = '选择歌曲';
    input.value = selected ? songSearchValue(selected) : '';
    input.setAttribute('role', 'combobox');
    input.setAttribute('aria-expanded', 'false');
    const popup = document.createElement('div');
    popup.className = 'song-select-popup';
    popup.hidden = true;
    popup.setAttribute('role', 'listbox');
    installRecoveringDatalistInput(input);
    let popupState = emptySongPopupState();
    let scheduledPopupFrame = 0;
    let scheduledPopupTimeout = 0;
    let popupToken = 0;

    function closePopup() {
      cancelScheduledPopupRender();
      cancelPopupScan();
      popupToken += 1;
      popup.hidden = true;
      input.setAttribute('aria-expanded', 'false');
    }

    function openPopup({ debounce = false } = {}) {
      scheduleSongSearchWarmup();
      schedulePopupRender(input.value, { debounce });
      popup.hidden = false;
      input.setAttribute('aria-expanded', 'true');
    }

    function schedulePopupRender(query, { debounce = false } = {}) {
      popupToken += 1;
      const token = popupToken;
      cancelPopupScan();
      if (scheduledPopupTimeout) {
        clearTimeout(scheduledPopupTimeout);
        scheduledPopupTimeout = 0;
      }
      if (scheduledPopupFrame) {
        cancelAnimationFrame(scheduledPopupFrame);
        scheduledPopupFrame = 0;
      }
      if (debounce) {
        scheduledPopupTimeout = setTimeout(() => {
          scheduledPopupTimeout = 0;
          schedulePopupFrame(query, token);
        }, songSearchDebounceMs);
        return;
      }
      schedulePopupFrame(query, token);
    }

    function schedulePopupFrame(query, token) {
      if (scheduledPopupFrame) {
        cancelAnimationFrame(scheduledPopupFrame);
      }
      scheduledPopupFrame = requestAnimationFrame(() => {
        scheduledPopupFrame = 0;
        if (token !== popupToken) {
          return;
        }
        renderPopup(query, token);
      });
    }

    function cancelScheduledPopupRender() {
      if (scheduledPopupTimeout) {
        clearTimeout(scheduledPopupTimeout);
        scheduledPopupTimeout = 0;
      }
      if (scheduledPopupFrame) {
        cancelAnimationFrame(scheduledPopupFrame);
        scheduledPopupFrame = 0;
      }
    }

    function commitInputValue() {
      clearFieldValidationMessage(input);
      if (!input.value.trim()) {
        return;
      }
      try {
        onChange(parseEntityId(input.value, '歌曲'));
        closePopup();
      } catch (error) {
        setFieldValidationMessage(input, error);
      }
    }

    function renderPopup(query, token) {
      cancelPopupScan();
      popup.textContent = '';
      const normalizedQuery = query.trim().toLowerCase();
      const presetIds = uniqueSongIds(presetSongs);
      const presetSet = new Set(presetIds.map(String));
      const normalIds = songRecordIds().filter((id) => !presetSet.has(String(id)));
      const fragment = document.createDocumentFragment();
      let appended = false;

      const matchingPresetIds = presetIds.filter((id) => songMatchesQuery(id, normalizedQuery));
      if (matchingPresetIds.length > 0) {
        fragment.append(sectionLabel('预设'));
        for (const id of matchingPresetIds) {
          fragment.append(songOption(songEntry(id), { preset: true }));
        }
        appended = true;
      }

      popupState = {
        query: normalizedQuery,
        normalIds,
        nextIndex: 0,
        appended,
        exhausted: false,
        moreElement: null,
        token,
      };
      popup.append(fragment);
      appendNextSongOptions(token);
      popup.scrollTop = 0;
    }

    function appendNextSongOptions(token = popupState.token) {
      if (popupState.exhausted || token !== popupState.token) {
        return;
      }
      popupState.moreElement?.remove();
      popupState.moreElement = null;

      const fragment = document.createDocumentFragment();
      let shown = 0;
      const startedAt = performance.now();
      while (
        popupState.nextIndex < popupState.normalIds.length
        && shown < songPopupPageSize
        && shouldContinueSongScan(startedAt, shown)
      ) {
        const id = popupState.normalIds[popupState.nextIndex];
        popupState.nextIndex += 1;
        const entry = popupState.query ? songEntry(id) : { id: Number(id) };
        if (popupState.query && !entry.search.includes(popupState.query)) {
          continue;
        }
        fragment.append(songOption(entry));
        shown += 1;
        popupState.appended = true;
      }

      popupState.exhausted = popupState.nextIndex >= popupState.normalIds.length;
      if (!popupState.appended && popupState.exhausted) {
        fragment.append(emptyMessage('无匹配歌曲', 'song-select-empty', 'div'));
      } else if (!popupState.exhausted) {
        const stillFillingPage = popupState.query && shown < songPopupPageSize;
        popupState.moreElement = moreMessage(stillFillingPage ? '搜索中...' : '滚动加载更多');
        fragment.append(popupState.moreElement);
      }
      popup.append(fragment);
      if (popupState.query && shown < songPopupPageSize && !popupState.exhausted) {
        scheduleNextSongScan(token);
      }
    }

    function scheduleNextSongScan(token) {
      cancelPopupScan();
      popupState.scanFrame = requestAnimationFrame(() => {
        popupState.scanFrame = 0;
        appendNextSongOptions(token);
      });
    }

    function cancelPopupScan() {
      if (popupState.scanFrame) {
        cancelAnimationFrame(popupState.scanFrame);
      }
    }

    function shouldContinueSongScan(startedAt, shown) {
      if (!popupState.query) {
        return true;
      }
      if (shown > 0 && performance.now() - startedAt >= songSearchFrameBudgetMs) {
        return false;
      }
      return performance.now() - startedAt < songSearchFrameBudgetMs;
    }

    function songOption(entry, { preset = false } = {}) {
      const songId = positiveIntegerOrUndefined(entry?.id) ?? 0;
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'song-select-option';
      button.classList.toggle('is-preset', preset);
      button.setAttribute('role', 'option');
      const name = document.createElement('span');
      name.className = 'song-select-option-name';
      name.textContent = entry?.label ?? songLabel(songId);
      const meta = document.createElement('span');
      meta.className = 'song-select-option-meta';
      meta.textContent = `ID ${songId}`;
      button.append(name, meta);
      button.addEventListener('pointerdown', (event) => {
        event.preventDefault();
        input.value = songSearchValue(songId);
        clearFieldValidationMessage(input);
        closePopup();
        onChange(songId);
      });
      return button;
    }

    function songMatchesQuery(id, query) {
      if (!query) {
        return true;
      }
      return songEntry(id).search.includes(query);
    }

    input.addEventListener('focus', () => openPopup());
    input.addEventListener('input', () => openPopup({ debounce: true }));
    input.addEventListener('change', commitInputValue);
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        closePopup();
      }
      if (event.key === 'Enter') {
        commitInputValue();
      }
    });
    popup.addEventListener('scroll', () => {
      const distanceToBottom = popup.scrollHeight - popup.scrollTop - popup.clientHeight;
      if (distanceToBottom <= 16) {
        appendNextSongOptions();
      }
    });
    combo.addEventListener('focusout', (event) => {
      if (!combo.contains(event.relatedTarget)) {
        closePopup();
      }
    });
    if (cover) {
      wrap.append(cover);
    }
    combo.append(input, popup);
    wrap.append(combo);
    cell.append(wrap);
    return cell;
  }

  function songRecordIds() {
    const records = getSongRecords() ?? {};
    if (records !== cachedSongRecords) {
      cachedSongRecords = records;
      cachedSongIds = Object.keys(records).sort(numericStringSort);
      cachedSongEntries = new Map();
    }
    return cachedSongIds;
  }

  function songEntry(id) {
    const songId = positiveIntegerOrUndefined(id) ?? 0;
    if (!cachedSongEntries.has(songId)) {
      const label = songLabel(songId);
      const value = songSearchValue(songId);
      cachedSongEntries.set(songId, {
        id: songId,
        label,
        value,
        search: value.toLowerCase(),
      });
    }
    return cachedSongEntries.get(songId);
  }

  function emptySongPopupState() {
    return {
      query: '',
      normalIds: [],
      nextIndex: 0,
      appended: false,
      exhausted: true,
      moreElement: null,
      scanFrame: 0,
      token: 0,
    };
  }

  function scheduleSongSearchWarmup() {
    const records = getSongRecords() ?? {};
    if (songSearchWarmupRecords === records) {
      return;
    }
    const ids = songRecordIds();
    const token = songSearchWarmupToken + 1;
    songSearchWarmupToken = token;
    let index = ids.findIndex((id) => !cachedSongEntries.has(Number(id)));
    if (index < 0) {
      songSearchWarmupRecords = records;
      return;
    }

    const run = (deadline) => {
      if (token !== songSearchWarmupToken) {
        return;
      }
      const startedAt = performance.now();
      while (
        index < ids.length
        && idleTimeRemaining(deadline) > 1
        && performance.now() - startedAt < songSearchWarmupBudgetMs
      ) {
        songEntry(ids[index]);
        index += 1;
      }
      while (index < ids.length && cachedSongEntries.has(Number(ids[index]))) {
        index += 1;
      }
      if (index >= ids.length) {
        songSearchWarmupRecords = records;
        return;
      }
      requestIdle(run, songSearchWarmupBudgetMs);
    };

    requestIdle(run, songSearchWarmupBudgetMs);
  }

  function uniqueSongIds(songs) {
    const ids = [];
    const seen = new Set();
    for (const song of Array.isArray(songs) ? songs : []) {
      const id = positiveIntegerOrUndefined(song?.songId ?? song);
      if (id == null || seen.has(id)) {
        continue;
      }
      seen.add(id);
      ids.push(id);
    }
    return ids;
  }

  function sectionLabel(text) {
    const label = document.createElement('div');
    label.className = 'song-select-section-label';
    label.textContent = text;
    return label;
  }

  function moreMessage(text) {
    const message = document.createElement('div');
    message.className = 'song-select-more';
    message.textContent = text;
    return message;
  }

  return {
    attributeCell,
    entityCell,
    inputCell,
    percentCell,
    songSelectCell,
    statRateCell,
  };
}

function requestIdle(callback, fallbackBudgetMs) {
  if (typeof requestIdleCallback === 'function') {
    requestIdleCallback(callback, { timeout: 500 });
    return;
  }
  requestAnimationFrame(() => callback({ timeRemaining: () => fallbackBudgetMs }));
}

function idleTimeRemaining(deadline) {
  return typeof deadline?.timeRemaining === 'function'
    ? deadline.timeRemaining()
    : 0;
}
