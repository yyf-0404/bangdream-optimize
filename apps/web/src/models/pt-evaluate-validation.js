export function validatePtEvaluateTeamSelection(player, request, cardCharacterId) {
  const teams = Array.isArray(request?.teams) ? request.teams : [];
  const expectedTeamCount = request?.liveVariant === 'medley' ? 3 : 1;
  if (teams.length !== expectedTeamCount) {
    throw new Error(`需要配置 ${expectedTeamCount} 支完整队伍`);
  }

  const medleyCardIds = new Set();
  for (const [teamIndex, team] of teams.entries()) {
    const teamLabel = `队伍 ${teamIndex + 1}`;
    const cardIds = Array.isArray(team?.cardIds) ? team.cardIds : [];
    if (cardIds.length !== 5 || cardIds.some((cardId) => !Number.isInteger(cardId) || cardId <= 0)) {
      throw new Error(`${teamLabel}必须完整配置五个卡位`);
    }
    if (team.captainCardId !== cardIds[2]) {
      throw new Error(`${teamLabel}的队长必须是第三个卡位`);
    }
    if (new Set(cardIds).size !== 5) {
      throw new Error(`${teamLabel}不能重复使用同一张卡牌`);
    }

    const characterIds = new Set();
    for (const cardId of cardIds) {
      if (!Object.hasOwn(player?.cardList ?? {}, String(cardId))) {
        throw new Error(`${teamLabel}中的卡牌 ${cardId} 不在当前配置中`);
      }
      const characterId = cardCharacterId(cardId);
      if (!Number.isInteger(characterId) || characterId <= 0) {
        throw new Error(`${teamLabel}中的卡牌 ${cardId} 不存在于当前游戏数据`);
      }
      characterIds.add(characterId);
      if (request.liveVariant === 'medley') {
        if (medleyCardIds.has(cardId)) {
          throw new Error(`巡回演出的三支队伍不能重复使用卡牌 ${cardId}`);
        }
        medleyCardIds.add(cardId);
      }
    }
    if (characterIds.size !== 5) {
      throw new Error(`${teamLabel}必须由五个不同角色组成`);
    }
  }
}
