import { emptyMessage } from '../ui/dom.js?v=2';
import {
  compactJoin,
  formatDateTime,
  formatInteger,
} from '../utils.js?v=2';

export function createResultCacheView({ elements }) {
  function renderResultCache(entries, { activeKey } = {}) {
    const cacheList = elements.resultCacheList;
    cacheList.textContent = '';
    const normalized = Array.isArray(entries)
      ? [...entries].sort((a, b) => (Number(b?.createdAt) || 0) - (Number(a?.createdAt) || 0))
      : [];

    if (normalized.length === 0) {
      cacheList.append(emptyMessage('暂无结果缓存', 'result-cache-empty', 'li'));
      return;
    }

    for (const entry of normalized) {
      const item = document.createElement('li');
      item.className = compactJoin([
        'result-cache-item',
        entry.key === activeKey && 'is-active',
      ]);

      const content = document.createElement('div');
      content.className = 'result-cache-content';

      const title = document.createElement('p');
      title.className = 'result-cache-title';
      title.textContent = formatEventName(entry);

      const stats = document.createElement('p');
      stats.className = 'result-cache-stats';
      stats.textContent = compactJoin([
        `模式 ${entry.activityMode === 'medley' ? '组曲' : '单曲'}`,
        entry.server ? `服务器 ${entry.server}` : '',
        entry.totalScore == null ? '' : `总分 ${formatInteger(entry.totalScore)}`,
        entry.totalStat == null ? '' : `综合力 ${formatInteger(entry.totalStat)}`,
        entry.songCount == null ? '' : `${entry.songCount} 首`,
      ], ' · ');

      const time = document.createElement('p');
      time.className = 'result-cache-time';
      time.textContent = formatDateTime(entry.createdAt);

      const actions = document.createElement('div');
      actions.className = 'result-cache-actions';
      const restoreButton = document.createElement('button');
      restoreButton.type = 'button';
      restoreButton.textContent = '恢复';
      restoreButton.dataset.resultCacheAction = 'restore';
      restoreButton.dataset.resultCacheKey = String(entry.key);
      restoreButton.className = 'compact-button';
      actions.append(restoreButton);

      content.append(title, stats, time);
      item.append(content, actions);
      cacheList.append(item);
    }
  }

  return {
    renderResultCache,
  };
}

function formatEventName(entry) {
  if (entry.eventLabel) {
    return entry.eventLabel;
  }
  if (entry.eventId == null) {
    return '自定义活动';
  }
  return `活动 ${entry.eventId}`;
}
