import { emptyMessage } from '../ui/dom.js?v=3';
import { buttonIcon } from '../ui/icons.js?v=3';
import {
  compactJoin,
  formatDateTime,
  formatInteger,
} from '../utils.js?v=3';

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
      const scoreRange = entry.calculationMode === 'scoreRange';
      const ptMaximize = entry.calculationMode === 'ptMaximize';
      const ptEvaluate = entry.calculationMode === 'ptEvaluate';
      stats.textContent = compactJoin(scoreRange ? [
        '目标 PT',
        entry.server ? `服务器 ${entry.server}` : '',
        entry.targetDeltaPt == null ? '' : `增量 ${formatInteger(entry.targetDeltaPt)}`,
        entry.playCount == null ? '' : `${entry.playCount} 局`,
        entry.totalFireCost == null ? '' : `火耗 ${entry.totalFireCost}`,
      ] : ptMaximize || ptEvaluate ? [
        ptEvaluate ? '指定队伍' : '最大PT（平均）',
        entry.server ? `服务器 ${entry.server}` : '',
        entry.averagePt == null ? '' : `平均 PT ${formatInteger(Math.round(entry.averagePt))}`,
        entry.averageScore == null
          ? ''
          : `平均分数 ${formatInteger(Math.round(entry.averageScore))}`,
        entry.totalStat == null ? '' : `综合力 ${formatInteger(entry.totalStat)}`,
      ] : [
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

      const deleteButton = document.createElement('button');
      deleteButton.type = 'button';
      deleteButton.dataset.resultCacheAction = 'delete';
      deleteButton.dataset.resultCacheKey = String(entry.key);
      deleteButton.className = 'compact-button result-cache-delete';
      deleteButton.setAttribute('aria-label', '删除此结果缓存');
      deleteButton.title = '删除此结果缓存';
      deleteButton.append(buttonIcon('trash'));
      actions.append(restoreButton, deleteButton);

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
