import {cardModel} from '../components/cards/model.js';
import {imageSources} from '../components/cards/images.js';

export function createViewAdapters({
  elements,
  numericStringSort,
  renderMetricsView,
  renderResultSummaryView,
  selectedBandId,
  areaItemGroups,
  areaItemLabel,
  formatAreaItemRate,
  songCoverUrls,
  songLabel,
  getSongRecord,
  getCore,
  cardLabel,
  cardName,
  cardRarity,
  cardCharacterId,
  normalizedCardConfig,
  readPlayer,
  cardIconUrls,
  cardAttribute,
  attributeFallback,
  entityCell,
  characterIconUrls,
  characterLabel,
}) {
  function renderMetrics(metrics) {
    renderMetricsView(elements.metrics, metrics);
  }

  function renderResultSummary(result, options) {
    const player = options?.diagnostic?.player ?? readPlayer();
    const customModels = new Map();
    const customCard = cardId => {
      if (!player.customCards?.[cardId]) return null;
      if (!customModels.has(cardId)) customModels.set(cardId, cardModel(getCore(), player, cardId));
      return customModels.get(cardId);
    };
    renderResultSummaryView(elements.resultSummary, result, {
      selectedBandId,
      areaItemGroups,
      areaItemLabel,
      formatAreaItemRate,
      player,
      songCoverUrls,
      songLabel,
      getSongRecord,
      cardLabel: id => {const card=customCard(id);return card?`${card.title} · ${card.displayId}`:cardLabel(id);},
      cardName: id => customCard(id)?.title ?? cardName(id),
      cardRarity: id => customCard(id)?.rarity ?? cardRarity(id),
      cardCharacterId: id => customCard(id)?.characterId ?? cardCharacterId(id),
      characterLabel,
      characterIconUrls,
      cardConfig: id => player.customCards?.[id]?.growth ?? normalizedCardConfig(id,player.cardList?.[id]),
      cardIconUrls: (id,config) => {const card=customCard(id);return card?imageSources(card):cardIconUrls(id,config);},
      cardAttribute: id => customCard(id)?.attribute ?? cardAttribute(id),
      attributeFallback,
    }, options);
  }

  function mergedEntityIds(records = {}, selected = {}) {
    return Object.keys({
      ...(records ?? {}),
      ...(selected ?? {}),
    }).sort(numericStringSort);
  }

  function cardEntityCell(cardId, config) {
    return entityCell(cardId, cardLabel(cardId), {
      imageUrls: cardIconUrls(cardId, config),
    });
  }

  function characterEntityCell(characterId) {
    return entityCell(characterId, characterLabel(characterId), {
      imageUrls: characterIconUrls(characterId),
    });
  }

  return {
    cardEntityCell,
    characterEntityCell,
    mergedEntityIds,
    renderMetrics,
    renderResultSummary,
  };
}
