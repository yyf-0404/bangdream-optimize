import {attributeLabel} from '../../utils.js?v=3';

export const bandOrder = [1, 2, 4, 5, 3, 21, 18, 45,50];
export const bandNames = {1:"Poppin'Party", 2:'Afterglow', 3:'Hello, Happy World!', 4:'Pastel＊Palettes', 5:'Roselia', 18:'RAISE A SUILEN', 21:'Morfonica', 45:'MyGO!!!!!',50:'Ave Mujica'};
// Attribute names are shared game terms and stay English in every UI language.
export const attributeNames = Object.freeze(Object.fromEntries(['powerful','cool','happy','pure'].map(value=>[value,attributeLabel(value)])));
export const serverNames = {jp:'日服', cn:'国服', en:'国际服', tw:'台服', kr:'韩服'};
export const datePriority = ['jp', 'cn', 'en', 'tw', 'kr'];
export const normalize = value => String(value).toLowerCase().replace(/\s/g, '');

export function defaultFilters(characters, server) {
  return {
    character: new Set(characters.map(c => String(c.id))),
    attribute: new Set(Object.keys(attributeNames)),
    rarity: new Set(['5', '4', '3', '2', '1']),
    ownership: new Set(['owned']),
    server: new Set([server])
  };
}

export function bandSelection(selected, members) {
  const count = members.filter(c => selected.has(String(c.id))).length;
  return count === 0 ? 'false' : count === members.length ? 'true' : 'mixed';
}

export function toggleBand(selected, members) {
  const next = new Set(selected);
  const all = bandSelection(selected, members) === 'true';
  members.forEach(c => all ? next.delete(String(c.id)) : next.add(String(c.id)));
  return next;
}

export function filterAllSelected(selected, values) {
  return values.length > 0 && values.every(value => selected.has(value));
}

export function toggleFilterSelection(selected, values) {
  return new Set(filterAllSelected(selected, values) ? [] : values);
}

export function releaseOf(card) {
  for (const server of datePriority) {
    const timestamp = Number(card.releaseDates?.[server]);
    if (Number.isFinite(timestamp) && timestamp > 0) return {timestamp, server};
  }
  return {timestamp:null, server:null};
}

export function compareRelease(a, b, ascending = false) {
  const x = releaseOf(a).timestamp, y = releaseOf(b).timestamp;
  // A missing date stays last in both directions.
  if (x === null || y === null) return x === y ? 0 : x === null ? 1 : -1;
  return ascending ? x - y : y - x;
}

// Header selection follows the archive, independently of the visible filters.
export function isOwnedCard(card) {
  return !!card?.owned;
}

export function latestOwnedCard(cards) {
  let latest = null;
  for (const card of cards) {
    if (Number(card.rarity) === 2 || !isOwnedCard(card)) continue;
    if (!latest || compareRelease(card, latest) < 0 || (compareRelease(card, latest) === 0 && card.id > latest.id)) latest = card;
  }
  return latest;
}

export function resolveCardCover(cards, _server, preference = {}) {
  const manual = preference.mode === 'manual' ? cards.find(c => c.id === Number(preference.cardId) && isOwnedCard(c)) : null;
  return {card:manual || latestOwnedCard(cards), mode:manual ? 'manual' : 'auto', variant:preference.variant === 'normal' ? 'normal' : 'after_training'};
}

export function normalizeCardSort(sort = 'owned-release', direction) {
  const legacy = /^(release|rarity|id)-(asc|desc)$/.exec(sort);
  const rule = legacy?.[1] ?? sort;
  return {
    sort: ['owned-release','release','rarity','id'].includes(rule) ? rule : 'owned-release',
    sortDirection: ['asc','desc'].includes(direction) ? direction : legacy?.[2] ?? 'desc',
  };
}

export function comparator(sort = 'owned-release', direction) {
  const order = normalizeCardSort(sort, direction), ascending = order.sortDirection === 'asc';
  const numeric = (a, b) => ascending ? a - b : b - a;
  return (a, b) => {
    let result = 0;
    if (order.sort === 'owned-release') result = numeric(Number(a.owned), Number(b.owned)) || compareRelease(a, b, ascending);
    else if (order.sort === 'release') result = compareRelease(a, b, ascending);
    else if (order.sort === 'rarity') result = numeric(a.rarity, b.rarity) || compareRelease(a, b, ascending);
    else if (order.sort === 'id') result = numeric(a.id, b.id);
    // Equal keys retain the existing stable ID tie-break in either direction.
    return result || b.id - a.id;
  };
}

export function cardMatchesReleaseFilter(card,server,now=Date.now()){
 const released=Number(card.releaseDates?.[server]);
 return Number.isFinite(released)&&released>0&&released<=now+7*24*60*60*1000;
}

export function filterCards(cards, filters, search = '') {
  const term = normalize(search), now = Date.now();
  return cards.filter(c => filters.character.has(String(c.characterId))
    && filters.attribute.has(c.attribute)
    && filters.rarity.has(String(c.rarity))
    && filters.ownership.has(c.owned ? 'owned' : 'missing')
    && (filters.server===null || [...filters.server].some(s => cardMatchesReleaseFilter(c,s,now)))
    && (!term || normalize(c.id+' '+c.name+' '+c.title+' '+(c.searchText||'')).includes(term)));
}

export function groupCards(cards, group = 'rarity', sort = 'owned-release', characters = [], direction) {
  const map = new Map();
  for (const card of cards) {
    const key = String(group === 'none' ? 'all' : group === 'character' ? card.characterId : card[group]);
    if (!map.has(key)) map.set(key, []);
    map.get(key).push(card);
  }
  const charOrder = [...characters].sort((a,b) => bandOrder.indexOf(a.band)-bandOrder.indexOf(b.band) || a.id-b.id).map(c => String(c.id));
  const keys = [...map.keys()].sort((a,b) => group === 'rarity' ? Number(b)-Number(a)
    : group === 'attribute' ? Object.keys(attributeNames).indexOf(a)-Object.keys(attributeNames).indexOf(b)
    : group === 'character' ? charOrder.indexOf(a)-charOrder.indexOf(b) : 0);
  return keys.map(key => ({key, cards:map.get(key).sort(comparator(sort, direction))}));
}
