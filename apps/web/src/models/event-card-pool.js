import {validationError} from './validation-error.js';
import {serverEventTimestamp} from './event.js?v=3';

const serverIndexes = {jp: 0, en: 1, tw: 2, cn: 3, kr: 4};
export const supportsEventCardPool = mode => ['maximize', 'ptMaximize'].includes(mode);

function timestamp(value) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : null;
}

export function eventCardPoolInfo(player, event, cards = {}) {
  const index = serverIndexes[player.server];
  const endAt = Number(player.currentEvent) === 0 || index === undefined
    ? null : serverEventTimestamp(event?.endAt, index) ?? null;
  const ownedCardIds = Object.keys(player.cardList ?? {});
  const cardIds = endAt === null ? ownedCardIds : ownedCardIds.filter(id => {
    const releasedAt = timestamp(cards[id]?.releasedAt?.[index]);
    return releasedAt !== null && releasedAt < endAt;
  });
  return {server: player.server, eventId: player.currentEvent, endAt, cardIds,
    ...(endAt === null ? {fallback: 'missing-event-end'} : {})};
}

// Both browser WASM and desktop receive the same reduced calculation snapshot.
// Inventory and enabled flags in the saved profile are never changed here.
export function restrictEventCardPool(player, event, cards) {
  if (!supportsEventCardPool(player.calculationMode) || !player.eventAvailableCardsOnly) {
    return {player};
  }
  const restriction = eventCardPoolInfo(player, event, cards);
  const target = '#event-available-cards-only';
  if (restriction.endAt === null) {
    // Keep the fallback in diagnostics/cache checks so an earlier restricted
    // result cannot be reused after the event loses its local end timestamp.
    return {player, restriction};
  }
  if (!restriction.cardIds.length) {
    throw validationError('没有在当前服务器、活动结束前发布的已持有卡牌。请调整档案或关闭限制。', target);
  }
  return {
    restriction,
    player: {
      ...player,
      cardList: Object.fromEntries(restriction.cardIds.map(id => [id, player.cardList[id]])),
      // Custom cards have no official server release date.
      customCards: Object.fromEntries(Object.entries(player.customCards ?? {}).map(([id, card]) =>
        [id, {...card, enabled: false}])),
    },
  };
}
