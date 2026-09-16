// Display slots and random skill activations are different orders.
export function teamOrderPresentation(song, isMaxScore) {
  const ids = song.teamCardIds ?? [];
  const recommendation = song.teamOrder;
  const proposed = recommendation?.recommendedTeamCardIds ?? song.recommendedTeamCardIds;
  const valid = Array.isArray(proposed) && proposed.length === 5
    && new Set(proposed).size === 5 && proposed.every(id => ids.includes(id))
    && proposed[2] === song.captainCardId;
  if (valid) return {
    ids: proposed,
    title: '推荐编队顺序',
    note: isMaxScore ? '从左到右 · 队长居中 · 优先提高最高分出现概率' : '从左到右 · 队长居中 · 按加权平均 PT 推荐',
    activation: isMaxScore ? recommendation : null,
    legacyOrder: false,
  };
  if (!isMaxScore && !song.detailedScore && song.liveVariant === 'cooperative' && ids.length === 5) {
    const display = ids.filter(id => id !== song.captainCardId);
    display.splice(2, 0, song.captainCardId);
    return { ids: display, title: '推荐编队顺序', note: '队长居中 · 协力中其余卡位等效', legacyOrder: false };
  }
  return {
    ids: isMaxScore ? (song.skillOrderCardIds ?? song.skillOrderCardIdsBySong ?? song.skillOrder ?? ids) : ids,
    title: isMaxScore ? '最优技能顺序' : '队伍配置',
    note: song.detailedScore ? '按指定卡位计算 · 队长居中'
      : '旧结果未包含推荐站位，请重新计算',
    legacyOrder: isMaxScore,
  };
}

export function formatOrderProbability(count, total) {
  if (!Number.isSafeInteger(count) || !Number.isSafeInteger(total)
    || total <= 0 || count < 0 || count > total) return '—';
  let a = count, b = total;
  while (b !== 0) [a, b] = [b, a % b];
  return `${count / a}/${total / a}`;
}

export function renderActivationOrder(order, captainId, deps) {
  const make = (tag, className, text) => {
    const node = document.createElement(tag);
    node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const section = make('section', 'activation-order');
  section.setAttribute('aria-label', '最优技能触发顺序与概率');
  const heading = make('div', 'activation-heading');
  heading.append(make('strong', '', '最优技能触发顺序'));
  const probability = make('span', 'activation-probability', '最高分概率 ');
  probability.append(make('b', '', formatOrderProbability(order.maxScorePathCount, order.totalPathCount)));
  probability.title = `所有可达最高分顺序的合计概率：${order.maxScorePathCount} / ${order.totalPathCount}`;
  heading.append(probability);
  section.append(heading);
  const sequence = make('ol', 'activation-sequence');
  sequence.setAttribute('aria-label', '按上方编队卡位编号依次触发');
  const cardIds = [...order.skillOrderCardIds, captainId];
  cardIds.forEach((id, index) => {
    const slot = order.recommendedTeamCardIds.indexOf(id) + 1;
    const item = make('li', index === 5 ? 'activation-repeat' : '');
    const name = deps.cardName?.(id) ?? deps.cardLabel(id);
    item.append(make('b', 'activation-slot', String(slot)));
    if (index === 5) item.append(make('small', '', '队长'));
    item.setAttribute('aria-label', `第 ${index + 1} 次触发：卡位 ${slot}，${name}${index === 5 ? '，队长' : ''}`);
    item.title = `第 ${index + 1} 次：${name} · 卡位 ${slot} · #${id}`;
    sequence.append(item);
  });
  section.append(sequence, make('p', 'activation-slot-key', '数字对应上方编队从左到右的 1–5 号位'));
  if (order.optimalOrderCount > 1) {
    section.append(make('p', 'activation-total', `共 ${order.optimalOrderCount} 种触发顺序可达到最高分，上方展示其中一种。`));
  }
  return section;
}

export function renderOrderReference() {
  const note = document.createElement('p');
  note.className = 'order-reference';
  const link = document.createElement('a');
  link.href = 'https://hhwx.org/bandori/events/321?type=event&tier=top10&server=cn&page=1&comment=8f2253be-a539-44bc-96d3-e7b86c2ea721';
  link.textContent = '顺序与概率说明';
  link.target = '_blank';
  link.rel = 'noopener noreferrer';
  link.setAttribute('aria-label', '顺序与概率说明（在新标签页打开）');
  note.append(link);
  return note;
}
