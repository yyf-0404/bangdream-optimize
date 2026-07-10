import { clearFieldValidationMessage, setFieldValidationMessage } from '../ui/validation.js';
import { confirmDialog } from '../ui/confirm.js?v=3';

export function createFormActions({
  elements,
  ensureCore,
  parseEntityId,
  normalizedPlayer,
  normalizedCharacterBonus,
  readPlayer,
  writePlayer,
  renderConfigForms,
  maxCardLevel,
  cardCharacterId,
  expandCardGroupForCard,
  matchingCardPreviewIds,
  setStatus,
  setError,
}) {
  async function handleAddCard() {
    try {
      await ensureCore();
    } catch (error) {
      setError(error);
      return;
    }

    const cardInput = elements.newCardId;
    const selectedCardIds = selectedPreviewCardIds(elements.cardAddPreview);
    if (selectedCardIds.length === 0) {
      setFieldValidationMessage(cardInput, new Error('请选择预览中的卡牌'));
      cardInput.focus();
      return;
    }
    clearFieldValidationMessage(cardInput);

    if (!await confirmCardOverwrite(selectedCardIds, '选中的')) {
      return;
    }
    addCards(selectedCardIds);
  }

  async function handleAddAllCards() {
    try {
      await ensureCore();
    } catch (error) {
      setError(error);
      return;
    }

    const cardInput = elements.newCardId;
    const cardIds = matchingCardPreviewIds?.() ?? [];
    if (cardIds.length === 0) {
      setFieldValidationMessage(cardInput, new Error('当前条件下没有卡牌'));
      cardInput.focus();
      return;
    }
    clearFieldValidationMessage(cardInput);

    if (!await confirmCardOverwrite(cardIds, '当前筛选条件下的全部', { force: true })) {
      return;
    }

    addCards(cardIds);
  }

  async function confirmCardOverwrite(cardIds, scope, { force = false } = {}) {
    const current = normalizedPlayer(readPlayer());
    const ownedCount = cardIds.filter((cardId) => current.cardList[String(cardId)] != null).length;
    if (!force && ownedCount === 0) {
      return true;
    }
    const missingCount = cardIds.length - ownedCount;
    return confirmDialog({
      title: '确认添加卡牌',
      lines: [
        `将添加${scope} ${cardIds.length} 张卡牌。`,
        `已拥有：${ownedCount} 张；未拥有：${missingCount} 张。`,
        '继续后，已拥有卡牌的现有配置也会被默认配置覆盖。',
      ],
      confirmText: '确认覆盖',
      danger: ownedCount > 0,
    });
  }

  function addCards(cardIds) {
    const player = normalizedPlayer(readPlayer());
    for (const cardId of cardIds) {
      player.cardList[String(cardId)] = defaultCardConfig(cardId);
      const characterId = cardCharacterId(cardId);
      if (characterId != null) {
        player.characterBouns[String(characterId)] ??= normalizedCharacterBonus();
      }
      expandCardGroupForCard(cardId);
    }
    elements.newCardId.value = '';
    elements.cardAddPreview?.dispatchEvent(new Event('card-preview-clear'));
    writePlayer(player);
    renderConfigForms(player);
    setStatus(`已添加/覆盖 ${cardIds.length} 张卡牌`);
  }

  function defaultCardConfig(cardId) {
    const maxLevel = maxCardLevel(cardId);
    return {
      level: readDefaultInteger(elements.defaultCardLevel, maxLevel, 1, maxLevel),
      training: elements.defaultCardTraining?.checked ?? true,
      illustTrainingStatus: elements.defaultCardIllust?.checked ?? true,
      episodes: [
        elements.defaultCardEpisode1?.checked ?? true,
        elements.defaultCardEpisode2?.checked ?? true,
      ],
      limitBreakRank: readDefaultInteger(elements.defaultCardLimitBreak, 4, 0, 4),
      skillLevel: readDefaultInteger(elements.defaultCardSkillLevel, 5, 1, 5),
    };
  }

  return {
    handleAddCard,
    handleAddAllCards,
  };
}

function selectedPreviewCardIds(preview) {
  return [...preview?.querySelectorAll('.card-preview-item.is-selected') ?? []]
    .map((item) => Number.parseInt(item.dataset.cardId, 10))
    .filter((cardId) => Number.isInteger(cardId) && cardId > 0);
}

function readDefaultInteger(input, fallback, min, max) {
  const value = Number.parseInt(input?.value, 10);
  if (!Number.isInteger(value)) {
    return fallback;
  }
  return Math.max(min, Math.min(max, value));
}
