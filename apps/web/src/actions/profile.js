import { confirmDialog } from '../ui/confirm.js?v=1';
import { createCompactProfileCodec } from '../data/compact-profile.js?v=1';
import { copyTextToClipboard } from '../ui/clipboard.js?v=1';
import { setFieldValidationMessage, clearFieldValidationMessage } from '../ui/validation.js';

export function createProfileActions({
  state,
  elements,
  normalizedPlayer,
  normalizedServer,
  parseEntityId,
  parseNonNegativeInteger,
  parseBestdoriProfileExport,
  ensureCore,
  readPlayer,
  writePlayer,
  savePlayerNow,
  refreshPlayerProfiles,
  renderPlayerProfileControls,
  renderConfigForms,
  nextProfileName,
  activeProfileName,
  bestdoriProfileToPlayerConfig,
  playerToBestdoriProfileExport,
  importMainBandCards,
  importMainBandCharacterBonuses,
  importEnabledAreaItems,
  activatePage,
  setStatus,
  setError,
}) {
  let copyToastTimer;
  let copyToastElement;
  let isImportingMainBand = false;
  const importMainBandLabel = elements.importMainBand?.textContent?.trim() || '导入主乐队配置';
  const {
    buildCompactProfilePayload,
    compactProfileToPlayer,
    compressProfilePayload,
    parseCompactProfileExport: parseCompactExport,
  } = createCompactProfileCodec({ normalizedPlayer });

  function setImportMainBandState(isBusy) {
    const button = elements.importMainBand;
    if (!button) {
      return;
    }
    button.disabled = isBusy;
    button.classList.toggle('is-loading', isBusy);
    button.setAttribute('aria-busy', isBusy ? 'true' : 'false');
    button.textContent = isBusy ? '导入中' : importMainBandLabel;
  }

  function showCopyToast(message, { duration = 1400 } = {}) {
    if (!message) {
      return;
    }

    if (!copyToastElement) {
      copyToastElement = document.createElement('div');
      copyToastElement.className = 'copy-success-toast';
      copyToastElement.setAttribute('aria-live', 'polite');
      copyToastElement.setAttribute('role', 'status');
      document.body.appendChild(copyToastElement);
    }

    copyToastElement.textContent = message;
    if (copyToastTimer) {
      clearTimeout(copyToastTimer);
    }

    copyToastElement.classList.remove('is-hidden');
    copyToastElement.classList.add('is-visible');

    copyToastTimer = window.setTimeout(() => {
      copyToastElement.classList.remove('is-visible');
      copyToastElement.classList.add('is-hidden');
      copyToastTimer = undefined;
    }, duration);
  }

  function handlePlayerJsonChange() {
    try {
      const player = readPlayer();
      writePlayer(player);
      renderConfigForms(player);
    } catch (error) {
      setError(error);
    }
  }

  async function handlePlayerProfileChange() {
    try {
      const configId = elements.playerProfile.value;
      if (!configId || configId === state.activePlayerProfileId) {
        return;
      }
      await savePlayerNow();
      const player = await state.runtime.selectPlayerConfig(configId);
      writePlayer(player, { autosave: false });
      await refreshPlayerProfiles();
      renderConfigForms(player);
      setStatus('已切换配置');
    } catch (error) {
      setError(error);
      renderPlayerProfileControls(readPlayer());
    }
  }

  async function handlePlayerProfileNameChange() {
    try {
      if (!state.activePlayerProfileId) {
        return;
      }
      const profile = await state.runtime.renamePlayerConfig(
        state.activePlayerProfileId,
        elements.playerProfileName.value,
      );
      await refreshPlayerProfiles();
      elements.playerProfileName.value = profile.name;
      setStatus('配置名已更新');
    } catch (error) {
      setError(error);
      renderPlayerProfileControls(readPlayer());
    }
  }

  function handlePlayerIdChange() {
    try {
      const player = readPlayer();
      const value = elements.playerId.value.trim();
      const nextPlayerId = /^\d+$/.test(value) ? Number.parseInt(value, 10) : player.playerId;
      if (!value && nextPlayerId === 0) {
        if (player.playerId === 0) {
          return;
        }
      } else if (nextPlayerId === player.playerId) {
        return;
      }
      player.playerId = nextPlayerId;
      writePlayer(player);
      renderConfigForms(player);
      setStatus('玩家 ID 已更新');
    } catch (error) {
      setError(error);
      renderConfigForms(readPlayer());
    }
  }

  function handlePlayerServerChange() {
    try {
      const player = readPlayer();
      player.server = normalizedServer(elements.playerServer.value);
      writePlayer(player);
      renderConfigForms(player);
      setStatus('服务器已更新');
    } catch (error) {
      setError(error);
      renderConfigForms(readPlayer());
    }
  }

  async function handleNewPlayerProfile() {
    try {
      await savePlayerNow();
      const name = nextProfileName('新配置');
      await state.runtime.createPlayerConfig({
        name,
        player: state.runtime.samplePlayerConfig(),
      });
      const player = await state.runtime.loadPlayerConfig();
      writePlayer(player, { autosave: false });
      await refreshPlayerProfiles();
      renderConfigForms(player);
      setStatus('已新建配置');
    } catch (error) {
      setError(error);
    }
  }

  async function handleCopyPlayerProfile() {
    try {
      await savePlayerNow();
      const player = readPlayer();
      await state.runtime.duplicatePlayerConfig({
        name: nextProfileName(`${activeProfileName()} 副本`),
        player,
      });
      const copied = await state.runtime.loadPlayerConfig();
      writePlayer(copied, { autosave: false });
      await refreshPlayerProfiles();
      renderConfigForms(copied);
      setStatus('已复制配置');
    } catch (error) {
      setError(error);
    }
  }

  async function handleDeletePlayerProfile() {
    try {
      if (!state.activePlayerProfileId) {
        return;
      }
      if (state.playerProfiles.length <= 1) {
        throw new Error('至少保留一份配置');
      }
      const confirmed = await confirmDialog({
        title: '删除配置',
        lines: [`将删除配置“${activeProfileName()}”。`],
        confirmText: '确认删除',
        danger: true,
      });
      if (!confirmed) {
        return;
      }
      const player = await state.runtime.deletePlayerConfig(state.activePlayerProfileId);
      writePlayer(player, { autosave: false });
      await refreshPlayerProfiles();
      renderConfigForms(player);
      setStatus('已删除配置');
    } catch (error) {
      setError(error);
    }
  }

  async function handleImportMainBand() {
    if (isImportingMainBand) {
      return;
    }
    const playerIdInput = elements.playerId;
    clearFieldValidationMessage(playerIdInput);
    try {
      isImportingMainBand = true;
      setImportMainBandState(true);
      await ensureCore();
      const playerId = parseEntityId(playerIdInput.value, '玩家 ID');
      const server = normalizedServer(elements.playerServer.value);
      setStatus('导入主乐队配置');
      const profile = await fetchBestdoriPlayerProfile(playerId, server);
      const player = normalizedPlayer(readPlayer());
      player.playerId = playerId;
      player.server = server;
      importMainBandCards(player, profile);
      importMainBandCharacterBonuses(player, profile);
      importEnabledAreaItems(player, profile);
      writePlayer(player);
      renderConfigForms(player);
      setStatus('主乐队配置已导入');
    } catch (error) {
      if (error instanceof Error && /玩家 ID/.test(error.message)) {
        setFieldValidationMessage(playerIdInput, error);
        playerIdInput.focus();
        return;
      }
      setError(error);
    } finally {
      isImportingMainBand = false;
      setImportMainBandState(false);
    }
  }

  function handleOpenBestdoriProfileDialog() {
    if (!elements.bestdoriProfileDialog?.showModal) {
      setError('当前浏览器不支持弹窗，无法使用粘贴导入');
      return;
    }
    elements.bestdoriProfileJson.value = '';
    elements.bestdoriProfileDialog.showModal();
    elements.bestdoriProfileJson.focus();
  }

  async function handleImportBestdoriProfile() {
    try {
      await ensureCore();
      const text = elements.bestdoriProfileJson.value.trim();
      if (!text) {
        throw new Error('请先粘贴 Bestdori Profile 配置');
      }

      const bestdoriProfile = parseBestdoriProfileExport(text);
      const player = normalizedPlayer(readPlayer());
      const imported = bestdoriProfileToPlayerConfig(bestdoriProfile, player);
      writePlayer(imported);
      activatePage('activity', { render: false });
      renderConfigForms(imported, { page: 'activity' });
      elements.bestdoriProfileJson.value = '';
      closeBestdoriProfileDialog();
      setStatus(
        `Bestdori Profile 已导入：${Object.keys(imported.cardList).length} 张卡牌，`
        + `${Object.keys(imported.areaItem).length} 个区域道具`,
      );
    } catch (error) {
      setError(error);
    }
  }

  async function handleImportCompactProfile() {
    try {
      const text = elements.bestdoriProfileJson.value.trim();
      if (!text) {
        throw new Error('请先粘贴 Base64 配置');
      }

      const compact = await parseCompactExport(text);
      const imported = compactProfileToPlayer(compact, normalizedPlayer(readPlayer()));
      writePlayer(imported);
      activatePage('activity', { render: false });
      renderConfigForms(imported, { page: 'activity' });
      elements.bestdoriProfileJson.value = '';
      closeBestdoriProfileDialog();
      setStatus(
        `配置已导入：${Object.keys(imported.cardList).length} 张卡牌，`
        + `${Object.keys(imported.areaItem).length} 个区域道具，`
        + `${Object.keys(imported.characterBouns).length} 个角色加成`,
      );
    } catch (error) {
      setError(error);
    }
  }

  async function handleExportCompactProfile() {
    if (!elements.exportProfileDialog?.showModal) {
      setError('当前浏览器不支持弹窗，无法展示导出内容');
      return;
    }
    if (elements.exportProfilePayload) {
      elements.exportProfilePayload.value = '';
    }
    elements.exportProfileDialog.showModal();
    elements.exportProfilePayload?.focus();
  }

  function handleCloseExportProfileDialog() {
    closeExportProfileDialog();
  }

  function closeExportProfileDialog() {
    if (elements.exportProfileDialog?.open) {
      elements.exportProfileDialog.close();
    }
  }

  async function handleExportCompactProfileAsBase64() {
    const button = elements.exportProfileBase64;
    if (!elements.exportProfilePayload) {
      setError('导出文本框不存在');
      return;
    }
    try {
      button.disabled = true;
      setStatus('正在导出配置');
      const player = readPlayer();
      const payload = buildCompactProfilePayload(player);
      const compressed = await compressProfilePayload(payload);
      const exportPayload = {
        v: compressed.version ?? 1,
        t: compressed.type,
        d: compressed.data,
      };
      elements.exportProfilePayload.value = JSON.stringify(exportPayload);
      elements.exportProfilePayload.focus();
      elements.exportProfilePayload.select();
      await copyExportProfilePayloadToClipboard(elements.exportProfilePayload.value, {
        statusMessage: '配置已生成并复制',
      });
    } catch (error) {
      setError(error);
    } finally {
      button.disabled = false;
    }
  }

  async function copyExportProfilePayloadToClipboard(text, { statusMessage = '导出文本已复制' } = {}) {
    if (!elements.exportProfilePayload) {
      throw new Error('浏览器不支持自动复制');
    }

    await copyTextToClipboard(text, { fallbackInput: elements.exportProfilePayload });
    setStatus(statusMessage);
    showCopyToast(statusMessage, { duration: 1300 });
  }

  async function handleExportCompactProfileBestdori() {
    const button = elements.exportProfileBestdori;
    if (!elements.exportProfilePayload) {
      setError('导出文本框不存在');
      return;
    }
    if (typeof playerToBestdoriProfileExport !== 'function') {
      setError('未注入 Bestdori 导出能力');
      return;
    }
    try {
      if (button) {
        button.disabled = true;
      }
      setStatus('正在导出 Bestdori 配置');
      const player = readPlayer();
      const profileName = activeProfileName?.();
      const exportPayload = {
        ...(profileName ? { name: profileName } : {}),
        ...playerToBestdoriProfileExport(player),
      };
      elements.exportProfilePayload.value = JSON.stringify(exportPayload);
      elements.exportProfilePayload.focus();
      elements.exportProfilePayload.select();
      await copyExportProfilePayloadToClipboard(
        elements.exportProfilePayload.value,
        {
          statusMessage: 'Bestdori 配置已生成并复制',
        },
      );
    } catch (error) {
      setError(error);
    } finally {
      if (button) {
        button.disabled = false;
      }
    }
  }

  function handleCloseBestdoriProfileDialog() {
    closeBestdoriProfileDialog();
  }

  function closeBestdoriProfileDialog() {
    if (elements.bestdoriProfileDialog?.open) {
      elements.bestdoriProfileDialog.close();
    }
  }

  async function fetchBestdoriPlayerProfile(playerId, server) {
    if (typeof state.runtime?.importBestdoriPlayerProfile === 'function') {
      return state.runtime.importBestdoriPlayerProfile({ playerId, server, mode: 3 });
    }
    throw new Error('当前运行时不支持导入 Bestdori 玩家资料');
  }

  return {
    handleCopyPlayerProfile,
    handleDeletePlayerProfile,
    handleOpenBestdoriProfileDialog,
    handleImportBestdoriProfile,
    handleImportCompactProfile,
    handleCloseBestdoriProfileDialog,
    handleImportMainBand,
    handleExportCompactProfile,
    handleExportCompactProfileAsBase64,
    handleExportCompactProfileBestdori,
    handleCloseExportProfileDialog,
    handleNewPlayerProfile,
    handlePlayerIdChange,
    handlePlayerJsonChange,
    handlePlayerProfileChange,
    handlePlayerProfileNameChange,
    handlePlayerServerChange,
  };
}
