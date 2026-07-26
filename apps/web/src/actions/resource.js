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
      await state.runtime.clearGameCache();
      await clearPersistedCache();
      state.core = null;
      state.resultCache = [];
      state.activeResultCacheKey = null;
      state.lastDiagnostic = null;
      if (typeof renderResultCache === 'function') {
        renderResultCache(state.resultCache, { activeKey: state.activeResultCacheKey });
      }
      renderResultSummary(null);
      renderMetrics(null);
      renderReferenceOptions();
      renderConfigForms(readPlayer());
      elements.log.textContent = '';
      setStatus('游戏缓存已清空');
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
          '这个过程可能耗时较长，并会清空当前结果缓存。',
        ],
        confirmText: '开始拉取',
      });
      if (!confirmed) {
        return;
      }
      setStatus('拉取全量资源');
      await state.runtime.syncAllGameData();
      await clearPersistedCache();
      state.core = null;
      state.resultCache = [];
      state.activeResultCacheKey = null;
      state.lastDiagnostic = null;
      if (typeof renderResultCache === 'function') {
        renderResultCache(state.resultCache, { activeKey: state.activeResultCacheKey });
      }
      renderResultSummary(null);
      renderMetrics(null);
      await ensureCore({ refreshManifest: true });
      setStatus('全量资源已拉取');
    } catch (error) {
      setError(error);
    }
  }

  async function handleRefreshCoreGameData() {
    try {
      setStatus('刷新核心资源');
      await state.runtime.refreshCoreGameData();
      await clearPersistedCache();
      state.core = null;
      state.resultCache = [];
      state.activeResultCacheKey = null;
      state.lastDiagnostic = null;
      if (typeof renderResultCache === 'function') {
        renderResultCache(state.resultCache, { activeKey: state.activeResultCacheKey });
      }
      renderResultSummary(null);
      renderMetrics(null);
      await ensureCore({ refreshManifest: true });
      setStatus('核心资源已刷新');
    } catch (error) {
      setError(error);
    }
  }

  async function handleClearLocalCache() {
    try {
      const confirmed = await confirmDialog({
        title: '清空本地缓存',
        lines: [
          '将清空本地用户配置缓存。',
          '请确认当前所有用户配置已导出备份。',
          '当前页面会重新加载默认配置和配置列表。',
        ],
        confirmText: '确认清空',
        danger: true,
      });
      if (!confirmed) {
        return;
      }
      await cancelPendingSave();
      await state.runtime.clearLocalCache();
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
      setStatus('本地缓存已清空');
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
