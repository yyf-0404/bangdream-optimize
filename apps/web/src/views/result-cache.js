import { emptyMessage } from '../ui/dom.js?v=3';
import {serverNames} from '../ui/cards/rules.js';
import {
  compactJoin,
  formatDateTime,
  formatInteger,
} from '../utils.js?v=3';

export function createResultCacheView({ elements, eventLabel }) {
  function renderResultCache(entries, { activeKey } = {}) {
    const cacheList = elements.resultCacheList;
    cacheList.textContent = '';
    const normalized = Array.isArray(entries)
      ? [...entries].sort((a, b) => (Number(b?.createdAt) || 0) - (Number(a?.createdAt) || 0))
      : [];

    const count = document.querySelector?.('#result-history-count');if(count)count.textContent=normalized.length;
    if (normalized.length === 0) {
      cacheList.append(emptyMessage('暂无历史结果', 'result-cache-empty', 'li'));
      elements.clearResultCache.disabled=true;
      return;
    }
    elements.clearResultCache.disabled=false;

    for (const entry of normalized) {
      const item = document.createElement('li');
      item.className = compactJoin([
        'result-cache-item',
        entry.key === activeKey && 'is-active',
      ]);

      const content = document.createElement('div');
      content.className = 'result-cache-content';

      const title = document.createElement('h3');
      title.className = 'result-cache-title';
      title.textContent = {maximize:'最高得分',ptMaximize:'最大平均 PT',ptEvaluate:'指定队伍',scoreRange:'控分'}[entry.calculationMode]||'最高得分';
      const context=document.createElement('p');context.className='result-cache-context';
      context.textContent=compactJoin([eventLabel?.(entry.eventId, entry.diagnostic?.player)||formatEventName(entry),entry.activityMode==='medley'?'巡回演出':'自由演出',serverNames[entry.server]||entry.server],' · ');

      const stats = document.createElement('p');
      stats.className = 'result-cache-stats';
      const scoreRange = entry.calculationMode === 'scoreRange';
      const ptMaximize = entry.calculationMode === 'ptMaximize';
      const ptEvaluate = entry.calculationMode === 'ptEvaluate';
      stats.textContent = compactJoin(scoreRange ? [
        '目标 PT',
        '',
        entry.targetDeltaPt == null ? '' : `增量 ${formatInteger(entry.targetDeltaPt)}`,
        entry.playCount == null ? '' : `${entry.playCount} 局`,
        entry.totalFireCost == null ? '' : `火耗 ${entry.totalFireCost}`,
      ] : ptMaximize || ptEvaluate ? [
        '',
        '',
        entry.averagePt == null ? '' : `平均 PT ${formatInteger(Math.round(entry.averagePt*(entry.result?.liveVariant==='medley'?3:1)))}（${entry.result?.liveVariant==='challenge_cp'?'200 CP':'0 火'}）`,
        entry.averageScore == null
          ? ''
          : `平均分数 ${formatInteger(Math.round(entry.averageScore))}`,
        entry.totalStat == null ? '' : `综合力 ${formatInteger(entry.totalStat)}`,
      ] : [
        '',
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
      restoreButton.textContent = '查看结果';
      restoreButton.dataset.resultCacheAction = 'restore';
      restoreButton.dataset.resultCacheKey = String(entry.key);
      restoreButton.className = 'compact-button';

      const deleteButton = document.createElement('button');
      deleteButton.type = 'button';
      deleteButton.dataset.resultCacheAction = 'delete';
      deleteButton.dataset.resultCacheKey = String(entry.key);
      deleteButton.className = 'compact-button result-cache-delete';
      deleteButton.setAttribute('aria-label', '删除此历史结果');
      deleteButton.title = '删除此历史结果';
      deleteButton.textContent='删除';
      actions.append(restoreButton, deleteButton);

      content.append(title, context, stats, time, actions);
      item.append(content);
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
