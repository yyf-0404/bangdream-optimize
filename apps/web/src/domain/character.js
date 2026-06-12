const BESTDORI_JP_POTENTIAL_DEFAULT_MAX = 55;
const BESTDORI_OTHER_POTENTIAL_DEFAULT_MAX = 50;
const BESTDORI_CHARACTER_TASK_DEFAULT_MAX = 60;

export function createCharacterBonusHelpers({
  getCharacterRecords,
  normalizedServer,
  normalizedCharacterBonus,
  cardCharacterId,
}) {
  function selectedCardCharacterIds(player) {
    return uniqueSortedNumbers(
      Object.keys(player.cardList)
        .map(cardCharacterId)
        .filter((characterId) => characterId != null),
    ).map(String);
  }

  function allCharacterBonusesAreMaxed(player) {
    const characterIds = Object.keys(getCharacterRecords() ?? {});
    const max = maxCharacterBonusForPlayer(player);
    return characterIds.length > 0
      && characterIds.every((characterId) => {
        const bonus = normalizedCharacterBonus(player.characterBouns?.[characterId]);
        return bonus.potential.performance === max.potential
          && bonus.potential.technique === max.potential
          && bonus.potential.visual === max.potential
          && bonus.characterTask.performance === max.characterTask
          && bonus.characterTask.technique === max.characterTask
          && bonus.characterTask.visual === max.characterTask;
      });
  }

  function maxCharacterBonusForPlayer(player) {
    return {
      potential: bestdoriBonusRate(bestdoriPotentialDefaultMaxForServer(player.server)),
      characterTask: bestdoriBonusRate(BESTDORI_CHARACTER_TASK_DEFAULT_MAX),
    };
  }

  function bestdoriCharacterBonusFromPoints(value, server) {
    const rawPoints = Math.max(0, Math.floor(Number(value) || 0));
    const points = rawPoints <= 1 ? 0 : rawPoints;
    const potentialPoints = Math.min(points, bestdoriPotentialDefaultMaxForServer(server));
    const taskPoints = Math.min(
      Math.max(0, points - potentialPoints),
      BESTDORI_CHARACTER_TASK_DEFAULT_MAX,
    );
    return {
      potential: bestdoriBonusRate(potentialPoints),
      characterTask: bestdoriBonusRate(taskPoints),
    };
  }

  function bestdoriPotentialDefaultMaxForServer(server) {
    if (normalizedServer(server) === 'jp') {
      return BESTDORI_JP_POTENTIAL_DEFAULT_MAX;
    }
    return BESTDORI_OTHER_POTENTIAL_DEFAULT_MAX;
  }

  return {
    allCharacterBonusesAreMaxed,
    bestdoriCharacterBonusFromPoints,
    maxCharacterBonusForPlayer,
    selectedCardCharacterIds,
  };
}

export function bestdoriBonusRate(points) {
  return points / 1000;
}

export function characterBonusWithRates(potential, characterTask, normalizedCharacterBonus) {
  return normalizedCharacterBonus({
    potential: {
      performance: potential,
      technique: potential,
      visual: potential,
    },
    characterTask: {
      performance: characterTask,
      technique: characterTask,
      visual: characterTask,
    },
  });
}

function uniqueSortedNumbers(values) {
  return [...new Set(values.map(Number).filter(Number.isInteger))]
    .sort((left, right) => left - right);
}
