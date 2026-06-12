import { confirmDialog } from '../ui/confirm.js?v=1';

export function createConfigActions({
  readPlayer,
  writePlayer,
  normalizedCardConfig,
  normalizedCharacterBonus,
  maxAreaItemLevel,
  maxCharacterBonusForPlayer,
  allAreaItemsAreMaxed,
  allCharacterBonusesAreMaxed,
  buildCharacterBonusWithRates,
  cardCharacterId,
  getAreaItemRecords,
  getCharacterRecords,
  renderCards,
  renderAreaItems,
  renderCharacterBonuses,
}) {
  function ensureOwnedCardCharacterBonuses(player) {
    for (const cardId of Object.keys(player.cardList)) {
      const characterId = cardCharacterId(cardId);
      if (characterId != null) {
        player.characterBouns[String(characterId)] ??= normalizedCharacterBonus();
      }
    }
  }

  function updateCard(cardId, patch) {
    const player = readPlayer();
    player.cardList[cardId] = normalizedCardConfig(cardId, {
      ...normalizedCardConfig(cardId, player.cardList[cardId]),
      ...patch,
    });
    writePlayer(player);
    renderCards(player);
  }

  function updateCardEpisode(cardId, index, checked) {
    const player = readPlayer();
    const config = normalizedCardConfig(cardId, player.cardList[cardId]);
    config.episodes[index] = checked;
    player.cardList[cardId] = config;
    writePlayer(player);
    renderCards(player);
  }

  function deleteCard(cardId) {
    const player = readPlayer();
    delete player.cardList[cardId];
    writePlayer(player);
    renderCards(player);
  }

  async function clearCards() {
    const player = readPlayer();
    const cardCount = Object.keys(player.cardList ?? {}).length;
    if (cardCount === 0) {
      return;
    }
    const confirmed = await confirmDialog({
      title: '清空卡牌列表',
      lines: [
        `将删除当前配置中的 ${cardCount} 张卡牌。`,
        '区域道具和角色加成不会被清空。',
      ],
      confirmText: '确认清空',
      danger: true,
    });
    if (!confirmed) {
      return;
    }
    player.cardList = {};
    writePlayer(player);
    renderCards(player);
  }

  function updateAreaItem(areaItemId, patch) {
    const player = readPlayer();
    player.areaItem[areaItemId] = {
      ...player.areaItem[areaItemId],
      ...patch,
    };
    writePlayer(player);
    renderAreaItems(player);
  }

  function updateCharacterBonus(characterId, group, field, value) {
    const player = readPlayer();
    player.characterBouns[characterId] = normalizedCharacterBonus(
      player.characterBouns[characterId],
    );
    player.characterBouns[characterId][group][field] = value;
    writePlayer(player);
    renderCharacterBonuses(player);
  }

  async function handleToggleAllAreaItemLevels() {
    const player = readPlayer();
    const clear = allAreaItemsAreMaxed(player);
    const confirmed = await confirmDialog({
      title: clear ? '清零区域道具' : '区域道具设为满级',
      lines: [
        clear ? '将全部区域道具等级设为 0。' : '将全部区域道具等级设为各自最大等级。',
      ],
      confirmText: clear ? '确认清零' : '确认满级',
      danger: clear,
    });
    if (!confirmed) {
      return;
    }
    for (const areaItemId of Object.keys(getAreaItemRecords() ?? {})) {
      player.areaItem[String(areaItemId)] = {
        level: clear ? 0 : maxAreaItemLevel(areaItemId),
      };
    }
    writePlayer(player);
    renderAreaItems(player);
  }

  async function handleToggleAllCharacterBonuses() {
    const player = readPlayer();
    const clear = allCharacterBonusesAreMaxed(player);
    const confirmed = await confirmDialog({
      title: clear ? '清零角色加成' : '角色加成设为满级',
      lines: [
        clear ? '将全部角色潜能和任务加成设为 0。' : '将全部角色潜能和任务加成设为当前可用上限。',
      ],
      confirmText: clear ? '确认清零' : '确认满级',
      danger: clear,
    });
    if (!confirmed) {
      return;
    }
    const max = maxCharacterBonusForPlayer(player);
    for (const characterId of Object.keys(getCharacterRecords() ?? {})) {
      player.characterBouns[String(characterId)] = clear
        ? buildCharacterBonusWithRates(0, 0, normalizedCharacterBonus)
        : buildCharacterBonusWithRates(max.potential, max.characterTask, normalizedCharacterBonus);
    }
    writePlayer(player);
    renderCharacterBonuses(player);
  }

  return {
    clearCards,
    deleteCard,
    ensureOwnedCardCharacterBonuses,
    handleToggleAllAreaItemLevels,
    handleToggleAllCharacterBonuses,
    updateAreaItem,
    updateCard,
    updateCardEpisode,
    updateCharacterBonus,
  };
}
