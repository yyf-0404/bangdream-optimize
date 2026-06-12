import { parseEntityId } from '../utils.js';

const DATALIST_INPUT_KEYS = [
  'eventSearch',
  'newEventCharacterId',
  'newEventMemberCardId',
];

const CHANGE_BINDINGS = [
  ['playerJson', 'handlePlayerJsonChange'],
  ['playerProfile', 'handlePlayerProfileChange'],
  ['playerProfileName', 'handlePlayerProfileNameChange'],
  ['playerId', 'handlePlayerIdChange'],
  ['playerServer', 'handlePlayerServerChange'],
  ['activityMode', 'handleActivityModeChange'],
  ['eventSearch', 'handleEventSearchChange'],
  ['eventCombinedPercent', 'handleEventScalarChange'],
  ['eventCharacterParamPerformance', 'handleEventCharacterParamChange'],
  ['eventCharacterParamTechnique', 'handleEventCharacterParamChange'],
  ['eventCharacterParamVisual', 'handleEventCharacterParamChange'],
  ['eventId', 'handleEventIdChange'],
];

const CLICK_BINDINGS = [
  ['newPlayerProfile', 'handleNewPlayerProfile'],
  ['copyPlayerProfile', 'handleCopyPlayerProfile'],
  ['deletePlayerProfile', 'handleDeletePlayerProfile'],
  ['importMainBand', 'handleImportMainBand'],
  ['openBestdoriProfileDialog', 'handleOpenBestdoriProfileDialog'],
  ['exportCompactProfile', 'handleExportCompactProfile'],
  ['exportProfileBestdori', 'handleExportCompactProfileBestdori'],
  ['exportProfileBase64', 'handleExportCompactProfileAsBase64'],
  ['closeExportProfileDialog', 'handleCloseExportProfileDialog'],
  ['importBestdoriProfile', 'handleImportBestdoriProfile'],
  ['importBase64Profile', 'handleImportCompactProfile'],
  ['closeBestdoriProfileDialog', 'handleCloseBestdoriProfileDialog'],
  ['clearGameCache', 'handleClearGameCache'],
  ['refreshCoreGameData', 'handleRefreshCoreGameData'],
  ['syncAllGameData', 'handleSyncAllGameData'],
  ['openDesktopDownloads', 'handleOpenDesktopDownloads'],
  ['closeDesktopDownloadsDialog', 'handleCloseDesktopDownloadsDialog'],
  ['customEvent', 'handleCustomEvent'],
  ['addEventAttribute', 'handleAddEventAttribute'],
  ['addEventCharacter', 'handleAddEventCharacter'],
  ['addEventMember', 'handleAddEventMember'],
  ['resultCacheList', 'handleResultCacheAction'],
  ['clearResultCache', 'handleClearResultCache'],
  ['addCard', 'handleAddCard'],
  ['addAllCards', 'handleAddAllCards'],
  ['toggleAreaItems', 'handleToggleAreaItems'],
  ['setAreaItems', 'handleToggleAllAreaItemLevels'],
  ['toggleCharacterBonuses', 'handleToggleCharacterBonuses'],
  ['setCharacterBonuses', 'handleToggleAllCharacterBonuses'],
  ['clearLocalCache', 'handleClearLocalCache'],
  ['copyResult', 'handleCopyResult'],
  ['exportDiagnostics', 'handleExportDiagnostics'],
];

export function createAppLifecycle({
  state,
  elements,
  createRuntime,
  installRecoveringDatalistInput,
  ensureCore,
  ensurePlayerProfiles,
  readPlayer,
  writePlayer,
  renderConfigForms,
  configureRuntimeControls,
  activatePage,
  preloadReferenceData,
  warmupCardSearchIndex,
  handlers,
  appendLog,
  setStatus,
  setError,
}) {
  const VALIDATION_RULES = [
    ['eventSearch', validateEventSearch],
    ['eventCombinedPercent', validateNonNegativeNumber('综合力')],
    ['eventCharacterParamPerformance', validateNonNegativeNumber('演出')],
    ['eventCharacterParamTechnique', validateNonNegativeNumber('技巧')],
    ['eventCharacterParamVisual', validateNonNegativeNumber('形象')],
    ['eventId', validateNonNegativeInteger('活动 ID')],
    ['newEventAttributePercent', validateNonNegativeNumber('属性加成')],
    ['newEventCharacterId', validateEntityId('角色')],
    ['newEventCharacterPercent', validateNonNegativeNumber('角色加成')],
    ['newEventMemberCardId', validateEntityId('卡牌')],
    ['newEventMemberPercent', validateNonNegativeNumber('卡牌加成')],
  ];

  async function bootstrap() {
    try {
      setStatus('初始化运行时');
      state.runtime = await createRuntime({
        onProgress: ({ type, path }) => appendLog(`${type}: ${path}`),
      });
      bindEvents();
      configureRuntimeControls();
      handlers.configureDownloadControls?.();
      setStatus('加载游戏数据');
      await ensureCore();
      setStatus('加载用户配置');
      const player = await state.runtime.loadPlayerConfig();
      writePlayer(player, { autosave: false });
      await ensurePlayerProfiles(player);
      renderConfigForms(readPlayer());
      setStatus('就绪');
      warmupCardSearchIndex?.();
    } catch (error) {
      setError(error);
    }
  }

  function bindEvents() {
    requiredElement('form').addEventListener('submit', requiredHandler('handleCalculate'));
    bindInputValidation();
    for (const tab of elements.pageTabs) {
      tab.addEventListener('click', () => activatePage(tab.dataset.page));
    }

    for (const elementKey of DATALIST_INPUT_KEYS) {
      installRecoveringDatalistInput(requiredElement(elementKey));
    }

    bindAll('change', CHANGE_BINDINGS);
    bindAll('click', CLICK_BINDINGS);
    bindTopbarToggle();

    requiredElement('eventSearch').addEventListener('focus', preloadReferenceData);
    requiredElement('eventSearch').addEventListener('pointerdown', preloadReferenceData);
    requiredElement('clearLog').addEventListener('click', () => {
      requiredElement('log').textContent = '';
    });
  }

  function bindTopbarToggle() {
    const topbar = elements.appTopbar;
    const button = elements.toggleTopbar;
    if (!topbar || !button) {
      return;
    }
    const label = button.querySelector('.button-label');
    button.addEventListener('click', () => {
      const collapsed = !topbar.classList.contains('is-collapsed');
      topbar.classList.toggle('is-collapsed', collapsed);
      button.classList.toggle('is-collapsed', collapsed);
      button.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
      if (label) {
        label.textContent = collapsed ? '展开顶部栏' : '收起顶部栏';
      }
    });
  }

  function bindAll(eventName, bindings) {
    for (const [elementKey, handlerKey] of bindings) {
      requiredElement(elementKey).addEventListener(eventName, requiredHandler(handlerKey));
    }
  }

  function bindInputValidation() {
    for (const [elementKey, validator] of VALIDATION_RULES) {
      const element = elements[elementKey];
      if (!element) {
        continue;
      }
      const message = createValidationMessage(element);
      const validate = () => {
        const error = validator(element.value);
        applyValidationState(element, message, error);
      };
      element.addEventListener('input', validate);
      element.addEventListener('change', validate);
      validate();
    }
  }

  function createValidationMessage(element) {
    const message = document.createElement('div');
    message.className = 'input-validation-message';
    message.textContent = '';
    message.hidden = true;
    message.id = `${element.id}-validation`;
    element.insertAdjacentElement('afterend', message);
    return message;
  }

  function applyValidationState(element, messageElement, message) {
    const invalid = typeof message === 'string' && message.length > 0;
    element.setCustomValidity(invalid ? message : '');
    element.classList.toggle('is-invalid', invalid);
    element.setAttribute('aria-invalid', invalid ? 'true' : 'false');
    if (invalid) {
      element.setAttribute('aria-describedby', messageElement.id);
      messageElement.textContent = message;
      messageElement.hidden = false;
    } else {
      element.removeAttribute('aria-describedby');
      messageElement.textContent = '';
      messageElement.hidden = true;
    }
  }

  function validateNonNegativeInteger(label, { required = false } = {}) {
    return (value) => {
      const trimmed = String(value ?? '').trim();
      if (!trimmed) {
        return required ? `${label}不能为空` : '';
      }
      if (!/^\d+$/.test(trimmed)) {
        return `${label}需为非负整数`;
      }
      const number = Number.parseInt(trimmed, 10);
      if (!Number.isInteger(number) || number < 0) {
        return `${label}需为非负整数`;
      }
      return '';
    };
  }

  function validateEntityId(label, { required = false } = {}) {
    return (value) => {
      const trimmed = String(value ?? '').trim();
      if (!trimmed) {
        return required ? `${label}不能为空` : '';
      }
      try {
        parseEntityId(trimmed, label);
      } catch {
        return `${label}需为正整数`;
      }
      return '';
    };
  }

  function validateNonNegativeNumber(label) {
    return (value) => {
      const trimmed = String(value ?? '').trim();
      if (!trimmed) {
        return '';
      }
      const number = Number(trimmed);
      if (!Number.isFinite(number)) {
        return `${label}需为数字`;
      }
      if (number < 0) {
        return `${label}不能小于 0`;
      }
      return '';
    };
  }

  function validateEventSearch(value) {
    const trimmed = String(value ?? '').trim();
    if (!trimmed) {
      return '';
    }
    const match = trimmed.match(/^(\d+)(?:\b|\s|$|[·-])/);
    if (!match) {
      return '需包含活动 ID（如 287）';
    }
    const number = Number.parseInt(match[1], 10);
    if (!Number.isSafeInteger(number) || number < 0) {
      return '活动 ID 无效';
    }
    return '';
  }

  function requiredElement(elementKey) {
    const element = elements[elementKey];
    if (!element) {
      throw new Error(`missing element binding: ${elementKey}`);
    }
    return element;
  }

  function requiredHandler(handlerKey) {
    const handler = handlers[handlerKey];
    if (typeof handler !== 'function') {
      throw new Error(`missing handler binding: ${handlerKey}`);
    }
    return handler;
  }

  return {
    bootstrap,
  };
}
