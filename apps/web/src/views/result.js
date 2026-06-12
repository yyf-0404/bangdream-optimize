import {
  attributeLabel,
  bandLabel,
  compactJoin,
  formatInteger,
  formatMs,
  magazineLabel,
  selectedBandLabel,
} from '../utils.js?v=1';
import {
  assetImage,
  attributeIconUrls,
  bandIconUrls,
} from '../assets/index.js?v=1';
import { cardPreviewItem } from '../ui/card-preview.js?v=1';
import { emptyMessage } from '../ui/dom.js?v=1';
import { renderDifficultyList } from './song.js?v=1';

export function renderMetrics(metricsElement, metrics) {
  metricsElement.textContent = '';
  metricsElement.hidden = !metrics;
  if (!metrics) {
    return;
  }

  const rows = [
    ['总耗时', formatMs(metrics.totalElapsedMs)],
  ];

  for (const [label, value] of rows) {
    const term = document.createElement('dt');
    term.textContent = label;
    const detail = document.createElement('dd');
    detail.textContent = String(value ?? '-');
    metricsElement.append(term, detail);
  }
}

export function renderResultSummary(resultElement, result, deps) {
  resultElement.textContent = '';
  resultElement.hidden = !result;
  if (!result) {
    return;
  }

  const songs = Array.isArray(result.songs) ? result.songs : [];
  const overview = document.createElement('section');
  overview.className = 'result-overview';
  overview.append(
    resultStat('总分', formatInteger(result.totalScore), 'strong'),
    resultStat('综合力', formatInteger(result.totalStat)),
    resultStat('活动', result.eventId == null ? '-' : `ID ${result.eventId}`),
  );
  resultElement.append(overview);

  if (result.items) {
    resultElement.append(renderSelectedItems(result.items, deps));
  }

  const songSection = document.createElement('section');
  songSection.className = 'result-section';
  const title = document.createElement('h3');
  title.textContent = '歌曲与队伍';
  const list = document.createElement('div');
  list.className = 'result-song-list';
  const maxScore = songs.reduce((max, song) => Math.max(max, Number(song.score) || 0), 0);
  for (const [index, song] of songs.entries()) {
    list.append(renderSongResult(song, index, maxScore, deps));
  }
  if (songs.length === 0) {
    list.append(emptyMessage('没有歌曲结果', 'result-empty'));
  }
  songSection.append(title, list);
  resultElement.append(songSection);
}

function resultStat(label, value, tone) {
  const item = document.createElement('div');
  item.className = compactJoin(['result-stat', tone && `result-stat-${tone}`]);
  const labelNode = document.createElement('span');
  labelNode.textContent = label;
  const valueNode = document.createElement('strong');
  valueNode.textContent = String(value ?? '-');
  item.append(labelNode, valueNode);
  return item;
}

function renderSelectedItems(items, deps) {
  const bandId = deps.selectedBandId(items.band);
  const section = document.createElement('section');
  section.className = 'result-items';
  section.append(
    resultItem('乐队道具', bandId == null ? selectedBandLabel(items.band) : bandLabel(bandId), {
      imageUrls: bandIconUrls(bandId),
    }),
    resultItem('属性道具', attributeLabel(items.attribute), {
      imageUrls: attributeIconUrls(items.attribute),
    }),
    resultItem('杂志道具', magazineLabel(items.magazine)),
  );
  return section;
}

function resultItem(label, value, { imageUrls } = {}) {
  const item = document.createElement('div');
  const icon = assetImage(imageUrls, 'result-item-icon', value);
  item.className = compactJoin(['result-item', icon && 'has-icon'], ' ');
  const labelNode = document.createElement('span');
  labelNode.textContent = label;
  const valueNode = document.createElement('strong');
  valueNode.textContent = String(value ?? '-');
  if (icon) {
    item.append(icon);
  }
  item.append(labelNode, valueNode);
  return item;
}

function renderSongResult(song, index, maxScore, deps) {
  const card = document.createElement('article');
  card.className = 'result-song';

  const header = document.createElement('div');
  header.className = 'result-song-header';
  const cover = assetImage(deps.songCoverUrls(song.songId), 'song-cover', deps.songLabel(song.songId));
  const content = document.createElement('div');
  content.className = 'result-song-content';
  const title = document.createElement('div');
  title.className = 'result-song-title';
  const songName = document.createElement('strong');
  songName.textContent = deps.songLabel(song.songId);
  const meta = document.createElement('span');
  meta.className = 'result-song-meta';
  meta.textContent = compactJoin([
    `#${index + 1}`,
    `ID ${song.songId}`,
  ], ' · ');
  const difficultyList = renderDifficultyList(deps.getSongRecord?.(song.songId), song.difficulty);
  difficultyList.classList.add('result-song-difficulty-list');
  title.append(songName, meta);

  const score = document.createElement('div');
  score.className = 'result-song-score';
  score.textContent = formatInteger(song.score);
  content.append(title, difficultyList);
  if (cover) {
    header.append(cover);
  } else {
    header.classList.add('no-cover');
  }
  header.append(content, score);

  const bar = document.createElement('div');
  bar.className = 'result-score-bar';
  const fill = document.createElement('span');
  const ratio = maxScore > 0 ? Math.max(0, Math.min(1, Number(song.score) / maxScore)) : 0;
  fill.style.width = `${Math.max(4, ratio * 100)}%`;
  bar.append(fill);

  const details = document.createElement('div');
  details.className = 'result-song-details';
  details.append(
    resultItem('综合力', formatInteger(song.stat)),
  );

  card.append(header, bar, details, renderSkillOrder(song, deps));
  return card;
}

function renderSkillOrder(song, deps) {
  const section = document.createElement('section');
  section.className = 'result-skill-order';
  const title = document.createElement('div');
  title.className = 'result-item result-skill-order-title';
  const titleText = document.createElement('span');
  titleText.textContent = '最优技能顺序';
  title.append(titleText);
  const preview = document.createElement('div');
  preview.className = 'result-skill-preview';
  const cardIds = skillOrderCardIds(song);
  if (cardIds.length === 0) {
    preview.append(emptyMessage('没有队伍卡片', 'card-preview-empty'));
  } else {
    for (const [index, cardId] of cardIds.entries()) {
      preview.append(resultSkillCard(cardId, {
        isCaptain: cardId === song.captainCardId,
        orderIndex: index,
      }, deps));
    }
  }
  section.append(title, preview);
  return section;
}

function skillOrderCardIds(song) {
  const explicitOrder = song.skillOrderCardIds ?? song.skillOrderCardIdsBySong ?? song.skillOrder;
  const cardIds = Array.isArray(explicitOrder) ? explicitOrder : song.teamCardIds;
  return Array.isArray(cardIds) ? cardIds : [];
}

function resultSkillCard(cardId, { isCaptain, orderIndex }, deps) {
  const orderBadge = document.createElement('span');
  orderBadge.className = 'result-skill-order-badge';
  orderBadge.textContent = String(orderIndex + 1);
  const playerCard = deps.cardConfig(cardId);
  const nameText = deps.cardName?.(cardId) ?? deps.cardLabel(cardId);
  return cardPreviewItem({
    id: cardId,
    name: nameText,
    rarity: deps.cardRarity?.(cardId),
    attribute: deps.cardAttribute(cardId),
    imageUrls: deps.cardIconUrls(cardId, playerCard),
    className: compactJoin(['result-skill-card', isCaptain && 'captain']),
    title: deps.cardLabel(cardId),
    leading: orderBadge,
  });
}
