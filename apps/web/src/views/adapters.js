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
  cardLabel,
  cardName,
  cardRarity,
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
    renderResultSummaryView(elements.resultSummary, result, {
      selectedBandId,
      areaItemGroups,
      areaItemLabel,
      formatAreaItemRate,
      player: options?.diagnostic?.player ?? readPlayer(),
      songCoverUrls,
      songLabel,
      getSongRecord,
      cardLabel,
      cardName,
      cardRarity,
      cardConfig: (cardId) => {const player=options?.diagnostic?.player??readPlayer();return player.customCards?.[cardId]?.growth??normalizedCardConfig(cardId,player.cardList?.[cardId]);},
      cardIconUrls,
      cardAttribute,
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
