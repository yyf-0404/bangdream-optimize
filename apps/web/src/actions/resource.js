import { confirmDialog } from '../ui/confirm.js?v=3';

export function createResourceActions({
  state,
  elements,
  cancelPendingSave,
  readPlayer,
  writePlayer,
  refreshPlayerProfiles,
  initializePlayerDefaults,
  renderConfigForms,
  renderReferenceOptions,
  renderResultSummary,
  renderMetrics,
  renderResultCache,
  clearPersistedResultCache,
  invalidateResultCache,
  hasActiveCalculation = () => false,
  ensureCore,
  setStatus,
  setError,
}) {
  function configureRuntimeControls() {
    elements.clearLocalCache.hidden = !(
      typeof state.runtime?.clearLocalCache === 'function'
    );
    elements.syncAllGameData.hidden = typeof state.runtime?.syncAllGameData !== 'function';
    elements.refreshCoreGameData.hidden = typeof state.runtime?.refreshCoreGameData !== 'function';
  }

  async function handleClearGameCache() {
    try {
      await invalidateResultCache();
      await state.runtime.clearGameCache();
      state.core = null;
      await ensureCore({ refreshManifest: true });
      renderReferenceOptions();
      renderConfigForms(readPlayer());
      elements.log.textContent = '';
      setStatus('游戏缓存已清空，玩家档案与计算历史已保留');
    } catch (error) {
      setError(error);
    }
  }

  async function handleSyncAllGameData() {
    try {
      const confirmed = await confirmDialog({
        title: '拉取全量资源',
        lines: [
          '将从 Bestdori 拉取全量游戏资源。',
          '这个过程可能耗时较长。计算历史会保留，新计算使用更新后的游戏数据。',
        ],
        confirmText: '开始拉取',
      });
      if (!confirmed) {
        return;
      }
      setStatus('拉取全量资源');
      await invalidateResultCache();
      await state.runtime.syncAllGameData();
      state.core = null;
      await ensureCore({ refreshManifest: true });
      renderReferenceOptions();
      renderConfigForms(readPlayer());
      setStatus('全量资源已拉取，计算历史已保留');
    } catch (error) {
      setError(error);
    }
  }

  async function handleRefreshCoreGameData() {
    try {
      setStatus('刷新核心资源');
      await invalidateResultCache();
      await state.runtime.refreshCoreGameData();
      state.core = null;
      await ensureCore({ refreshManifest: true });
      renderReferenceOptions();
      renderConfigForms(readPlayer());
      setStatus('核心资源已刷新，计算历史已保留');
    } catch (error) {
      setError(error);
    }
  }

  async function handleClearLocalCache() {
    try {
      if (hasActiveCalculation()) throw new Error('计算仍在进行，请等待完成后再清空玩家档案缓存。');
      const confirmed = await confirmDialog({
        title: '清空玩家档案与计算历史',
        lines: [
          '将删除本地全部玩家档案和计算历史。',
          '请确认当前所有用户配置已导出备份。',
          '档案导出文件不包含计算历史，需要保留的结果请先保存图片或导出诊断。',
          '当前页面会重新加载默认配置和配置列表。',
        ],
        confirmText: '确认清空',
        danger: true,
      });
      if (!confirmed) {
        return;
      }
      if (hasActiveCalculation()) throw new Error('计算仍在进行，请等待完成后再清空玩家档案缓存。');
      await cancelPendingSave();
      await state.runtime.clearLocalCache();
      await clearPersistedCache();
      state.resultCache = [];
      state.profileDiagnostics = {};
      state.activeResultCacheKey = null;
      state.lastDiagnostic = null;
      if (elements.result) elements.result.textContent = '';
      renderResultCache?.([], {activeKey: null});
      renderResultSummary(null);
      renderMetrics(null);
      const loadedPlayer = await state.runtime.loadPlayerConfig();
      const {
        player,
        changed: initializedDefaults,
      } = initializePlayerDefaults(loadedPlayer);
      writePlayer(player, { autosave: false });
      await refreshPlayerProfiles({ defaultPlayer: player });
      if (initializedDefaults) {
        await state.runtime.savePlayerConfig(readPlayer());
      }
      renderConfigForms(player);
      setStatus('玩家档案与计算历史已清空');
    } catch (error) {
      setError(error);
    }
  }

  async function clearPersistedCache() {
    if (typeof clearPersistedResultCache === 'function') {
      await clearPersistedResultCache();
    }
  }

  return {
    configureRuntimeControls,
    handleClearGameCache,
    handleClearLocalCache,
    handleRefreshCoreGameData,
    handleSyncAllGameData,
  };
}
