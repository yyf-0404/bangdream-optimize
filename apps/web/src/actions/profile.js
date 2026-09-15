import { mergeCnAccountImport } from '../data/cn-account.js';
import { reviewImport } from '../ui/import-review.js?v=3';
import {openImportSource,openExportFlow} from '../ui/archive-flows.js?v=3';
import { requestProfileDetails } from '../ui/profile-dialog.js';
import { confirmDialog } from '../ui/confirm.js?v=3';
import { createCompactProfileCodec } from '../data/compact-profile.js?v=3';
import { copyTextToClipboard } from '../ui/clipboard.js?v=3';
import { setFieldValidationMessage, clearFieldValidationMessage } from '../ui/validation.js';

export function createProfileActions({
  state,
  elements,
  normalizedPlayer,
  initializePlayerDefaults = player => ({player}),
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
  const selectedTarget=()=>state.viewedPlayerProfileId||state.activePlayerProfileId;
  const targetName=id=>state.playerProfiles.find(p=>p.id===id)?.name||'所选档案';
  const readTarget=async id=>id===state.activePlayerProfileId?readPlayer():state.runtime.loadPlayerConfig(id);
  let importTarget,exportTarget;
  async function commitTarget(id,player){
    if(!state.playerProfiles.some(p=>p.id===id))throw new Error('目标档案已不存在，请重新选择');
    if(id===state.activePlayerProfileId)writePlayer(player);else await state.runtime.savePlayerConfig(player,id);
    await refreshPlayerProfiles();renderConfigForms(readPlayer());
    document.dispatchEvent(new CustomEvent('profile-content-changed',{detail:{id}}));
  }
  let copyToastTimer;
  let copyToastElement;
  let isImportingMainBand = false;
  const importMainBandLabel = elements.importMainBand?.textContent?.trim() || '导入主乐队';
  const importMainBandIcon = elements.importMainBand
    ?.querySelector('.button-icon')
    ?.cloneNode(true);
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
    setButtonIconText(button, importMainBandIcon, isBusy ? '导入中' : importMainBandLabel);
  }

  function setButtonIconText(button, icon, text) {
    if (!icon) {
      button.textContent = text;
      return;
    }

    button.replaceChildren(icon.cloneNode(true), document.createTextNode(text));
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
      renderConfigForms(readPlayer());
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
      const options=await requestProfileDetails({name:nextProfileName('新档案')});if(!options)return;
      const previous=state.activePlayerProfileId;
      await savePlayerNow();
      await state.runtime.createPlayerConfig({
        name:options.name,
        player: {...initializePlayerDefaults(state.runtime.samplePlayerConfig()).player,server:options.server},
      });
      if(!options.activate)await state.runtime.selectPlayerConfig(previous);
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
      const target=selectedTarget(),player = await readTarget(target);
      const options=await requestProfileDetails({name:nextProfileName(`${targetName(target)} 副本`),server:player.server,copy:true});if(!options)return;
      const previous=state.activePlayerProfileId;await savePlayerNow();
      const latest={...await readTarget(target),server:options.server};
      await state.runtime.duplicatePlayerConfig({
        name: options.name,
        player:latest,
      });
      if(!options.activate)await state.runtime.selectPlayerConfig(previous);
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
    const target=selectedTarget();
    try {
      if (!target) {
        return;
      }
      if (state.playerProfiles.length <= 1) {
        throw new Error('至少保留一份配置');
      }
      const confirmed = await confirmDialog({
        title: '删除配置',
        lines: [`将删除配置“${targetName(target)}”。`],
        confirmText: '确认删除',
        danger: true,
      });
      if (!confirmed) {
        return;
      }
      const player = await state.runtime.deletePlayerConfig(target);
      writePlayer(player, { autosave: false });
      await refreshPlayerProfiles();
      renderConfigForms(player);
      setStatus('已删除配置');
    } catch (error) {
      setError(error);
    }
  }

  async function handleImportMainBand() {
    const importProfileId = selectedTarget();
    if (isImportingMainBand) {
      return;
    }
    const playerIdInput = elements.playerId;
    clearFieldValidationMessage(playerIdInput);
    try {
      isImportingMainBand = true;
      setImportMainBandState(true);
      await ensureCore();
      const source=await readTarget(importProfileId);
      const playerId = parseEntityId(String(source.playerId), '玩家 ID');
      const server = normalizedServer(source.server);
      setStatus('导入主乐队配置');
      const profile = await fetchBestdoriPlayerProfile(playerId, server);
      const player = normalizedPlayer(await readTarget(importProfileId));
      player.playerId = playerId;
      player.server = server;
      importMainBandCards(player, profile);
      importMainBandCharacterBonuses(player, profile);
      importEnabledAreaItems(player, profile);
      if (!state.playerProfiles.some(p=>p.id===importProfileId)) throw new Error('目标档案已不存在，请重新导入');
      if (!await reviewImport(await readTarget(importProfileId),player)) return;
      await commitTarget(importProfileId,player);
      renderConfigForms(readPlayer());
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

  async function handleOpenBestdoriProfileDialog() {
    return openImportForTarget(selectedTarget());
  }

  async function handleQuickImport() {
    return openImportForTarget(state.activePlayerProfileId);
  }

  async function openImportForTarget(target) {
    try{
      const original=await readTarget(target),name=targetName(target);
      openImportSource({profile:{...original,name},onRead:async (source,{signal})=>{
        await ensureCore();
        signal.throwIfAborted();
        let before=normalizedPlayer(await readTarget(target)),imported,identity;
        if(source.source==='paste')imported=source.format==='base64'?compactProfileToPlayer(await parseCompactExport(source.text),before):bestdoriProfileToPlayerConfig(parseBestdoriProfileExport(source.text),before);
        else if(source.server==='cn'&&source.method==='credentials'){
          let data;
          try{data=await state.runtime.importCnAccount({account:source.account,password:source.password,channel:source.channel},{signal});}
          finally{delete source.password;}
          signal.throwIfAborted();before=normalizedPlayer(await readTarget(target));
          imported=mergeCnAccountImport(before,data);identity={name:data.name,gameUid:data.gameUid,rank:data.rank,channel:data.channel};
        }
        else{
          const id=parseEntityId(String(source.playerId),'玩家 ID');imported=structuredClone(before);imported.server=normalizedServer(source.server);imported.playerId=id;
          const data=await fetchBestdoriPlayerProfile(id,imported.server);importMainBandCards(imported,data);importMainBandCharacterBonuses(imported,data);importEnabledAreaItems(imported,data);
        }
        if(!state.playerProfiles.some(p=>p.id===target))throw new Error('目标档案已不存在，请重新导入');
        signal.throwIfAborted();
        const review=await reviewImport(before,imported,{source:source.source,format:source.format,name,identity,signal,allowNew:true});if(!review)return false;
        signal.throwIfAborted();
        if(review.destination==='new'){await savePlayerNow();await state.runtime.createPlayerConfig({name:review.name,player:imported});writePlayer(await state.runtime.loadPlayerConfig(),{autosave:false});await refreshPlayerProfiles();renderConfigForms(readPlayer());}
        else await commitTarget(target,imported);
        setStatus('配置已导入');return {name:review.destination==='new'?review.name:name,customCount:Object.keys(imported.customCards||{}).length,cards:Object.keys(imported.cardList||{}).length,items:Object.keys(imported.areaItem||{}).length,characters:Object.keys(imported.characterBouns||{}).length};
      }});
    }catch(error){setError(error);}
  }

  async function handleImportBestdoriProfile() {
    const importProfileId = (importTarget||selectedTarget());
    try {
      await ensureCore();
      const text = elements.bestdoriProfileJson.value.trim();
      if (!text) {
        throw new Error('请先粘贴 Bestdori Profile 配置');
      }

      const bestdoriProfile = parseBestdoriProfileExport(text);
      const player = normalizedPlayer(await readTarget(importProfileId));
      const imported = bestdoriProfileToPlayerConfig(bestdoriProfile, player);
      if (!state.playerProfiles.some(p=>p.id===importProfileId)) throw new Error('目标档案已不存在，请重新导入');
      if (!await reviewImport(player,imported)) return;
      await commitTarget(importProfileId,imported);

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
    const importProfileId = (importTarget||selectedTarget());
    try {
      const text = elements.bestdoriProfileJson.value.trim();
      if (!text) {
        throw new Error('请先粘贴 Base64 配置');
      }

      const compact = await parseCompactExport(text);
      const imported = compactProfileToPlayer(compact, normalizedPlayer(await readTarget(importProfileId)));
      if (!state.playerProfiles.some(p=>p.id===importProfileId)) throw new Error('目标档案已不存在，请重新导入');
      if (!await reviewImport(await readTarget(importProfileId),imported)) return;
      await commitTarget(importProfileId,imported);

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
    const target=selectedTarget();
    try{const player=await readTarget(target),name=targetName(target);
      openExportFlow({profile:{...player,name,customCount:Object.keys(player.customCards||{}).length,cards:Object.keys(player.cardList||{}).length,items:Object.keys(player.areaItem||{}).length},getPayload:async format=>{
        if(format==='bestdori')return JSON.stringify({name,...playerToBestdoriProfileExport(player)});
        const data=await compressProfilePayload(buildCompactProfilePayload(player));return JSON.stringify({v:data.version??1,t:data.type,d:data.data});
      },save:payload=>state.runtime.saveJsonFile(payload),copy:async payload=>{await copyTextToClipboard(payload);setStatus('配置已生成并复制');}});
    }catch(error){setError(error);}
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
      const player = await readTarget(exportTarget||selectedTarget());
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
      const player = await readTarget(exportTarget||selectedTarget());
      const profileName = targetName(exportTarget||selectedTarget());
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
    handleQuickImport,
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
