import {
  finiteNumberOrZero,
  integerOrZero,
  numericStringSort,
  positiveIntegerOrDefault,
  positiveIntegerOrUndefined,
} from '../utils.js?v=1';

const SERVER_BY_BESTDORI_INDEX = ['jp', 'en', 'tw', 'cn', 'kr'];
const BESTDORI_SERVER_INDEX_BY_SERVER = {
  jp: 0,
  en: 1,
  tw: 2,
  cn: 3,
  kr: 4,
};
const MIN_SKILL_LEVEL = 1;
const MAX_SKILL_LEVEL = 5;
const BESTDORI_PROFILE_AREA_ITEM_IDS = {
  PoppinParty: [1, 6, 11, 16, 21, 26, 31],
  Afterglow: [2, 7, 12, 17, 22, 27, 32],
  HelloHappyWorld: [5, 10, 15, 20, 25, 30, 35],
  PastelPalettes: [3, 8, 13, 18, 23, 28, 33],
  Roselia: [4, 9, 14, 19, 24, 29, 34],
  RaiseASuilen: [90, 91, 92, 93, 94, 95, 96],
  Morfonica: [83, 84, 85, 86, 87, 88, 89],
  MyGO: [97, 98, 99, 100, 101, 102, 103],
  Everyone: [73, 74, 75, 76, 77, 78, 79],
  Magazine: [80, 81, 82],
  Plaza: [56, 60, 57, 58],
  Menu: [70, 69, 66, 67],
};

export function createBestdoriProfileImporter({
  normalizedPlayer,
  normalizedServer,
  normalizedCharacterBonus,
  normalizedStatRate,
  recordWithFix,
  maxCardLevel,
  maxAreaItemLevel,
  cardCharacterId,
  getCharacterRecords,
  bestdoriCharacterBonusFromPoints,
}) {
  function importMainBandCards(player, profile) {
    const entries = mainDeckEntries(profile);
    if (entries.length === 0) {
      throw new Error('玩家主乐队没有卡牌数据');
    }
    for (const entry of entries) {
      const cardId = positiveIntegerOrUndefined(entry?.situationId);
      if (cardId == null) {
        continue;
      }
      player.cardList[String(cardId)] = {
        level: positiveIntegerOrDefault(entry.level, maxCardLevel(cardId)),
        training: entry.trainingStatus === 'done',
        illustTrainingStatus: entry.illust === 'after_training',
        episodes: [true, true],
        limitBreakRank: integerOrZero(entry.limitBreakRank),
        skillLevel: skillLevelFromBestdori(entry.skillLevel),
      };
    }
  }

  function importMainBandCharacterBonuses(player, profile) {
    for (const entry of mainDeckEntries(profile)) {
      const cardId = positiveIntegerOrUndefined(entry?.situationId);
      if (cardId == null) {
        continue;
      }
      const characterId = cardCharacterId(cardId);
      if (characterId == null) {
        continue;
      }
      player.characterBouns[String(characterId)] = characterBonusFromBestdoriCard(entry, cardId);
    }
  }

  function importEnabledAreaItems(player, profile) {
    const entries = profile?.enabledUserAreaItems?.entries;
    if (!Array.isArray(entries)) {
      return;
    }
    for (const item of entries) {
      const areaItemId = positiveIntegerOrUndefined(item?.areaItemCategory);
      if (areaItemId == null) {
        continue;
      }
      player.areaItem[String(areaItemId)] = {
        level: integerOrZero(item.level),
      };
    }
  }

  function bestdoriProfileToPlayerConfig(profile, basePlayer) {
    const player = normalizedPlayer(basePlayer);
    player.server = bestdoriProfileServer(profile.server, player.server);
    player.cardList = bestdoriProfileCards(profile.cards);
    player.areaItem = {
      ...player.areaItem,
      ...bestdoriProfileAreaItems(profile.items),
    };
    player.characterBouns = bestdoriProfileCharacterBonuses(
      profile.items?.potentials,
      player.server,
    );
    return normalizedPlayer(player);
  }

  function playerToBestdoriProfileExport(player = {}) {
    const source = normalizedPlayer(player);
    const cards = compressBestdoriCards(source.cardList);
    return {
      server: bestdoriProfileServerIndex(source.server),
      compression: '2',
      data: {
        cards,
        items: {
          ...compressBestdoriAreaItems(source.areaItem),
          potentials: encodeBestdoriRunLength(collectCharacterPotentialPoints(
            source.characterBouns,
            source.server,
          )),
        },
      },
    };
  }

  function characterBonusFromBestdoriCard(entry, cardId) {
    const append = entry?.userAppendParameter ?? {};
    const base = cardBaseStatWithAppend(cardId, append);
    return normalizedCharacterBonus({
      potential: {
        performance: appendRate(append.characterPotentialPerformance, base.performance),
        technique: appendRate(append.characterPotentialTechnique, base.technique),
        visual: appendRate(append.characterPotentialVisual, base.visual),
      },
      characterTask: {
        performance: appendRate(append.characterBonusPerformance, base.performance),
        technique: appendRate(append.characterBonusTechnique, base.technique),
        visual: appendRate(append.characterBonusVisual, base.visual),
      },
    });
  }

  function cardBaseStatWithAppend(cardId, append) {
    const stat = maxCardStat(cardId);
    return {
      performance: finiteNumberOrZero(append.performance) + stat.performance,
      technique: finiteNumberOrZero(append.technique) + stat.technique,
      visual: finiteNumberOrZero(append.visual) + stat.visual,
    };
  }

  function maxCardStat(cardId) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    const level = maxCardLevel(cardId);
    return normalizedStatRate(card?.stat?.[String(level)]);
  }

  function bestdoriProfileServer(value, fallback) {
    const server = SERVER_BY_BESTDORI_INDEX[Number(value)];
    return server ?? normalizedServer(value ?? fallback);
  }

  function bestdoriProfileServerIndex(value) {
    return BESTDORI_SERVER_INDEX_BY_SERVER[normalizedServer(value)] ?? 3;
  }

  function compressBestdoriCards(cardList = {}) {
    const entries = [];
    for (const [cardId, config] of Object.entries(cardList)) {
      const id = Number(cardId);
      if (!Number.isInteger(id) || id <= 0 || !config || typeof config !== 'object') {
        continue;
      }
      const episodes = Array.isArray(config.episodes) ? config.episodes : [];
      const hasEp1 = episodes[0] === true;
      const hasEp2 = episodes[1] === true;
      entries.push([
        id,
        {
          level: integerOrZero(config.level),
          master: integerOrZero(config.limitBreakRank),
          skill: skillLevelToBestdori(config.skillLevel),
          episodes: !hasEp1 ? 0 : hasEp2 ? 2 : 1,
          training: config.training ? 1 : 0,
          art: config.illustTrainingStatus ? 1 : 0,
          exclude: !!config.exclude,
        },
      ]);
    }
    entries.sort((left, right) => left[0] - right[0]);
    return {
      ids: encodeBestdoriCardIds(entries.map(([cardId]) => cardId)),
      levels: encodeBestdoriRunLength(entries.map(([, card]) => card.level)),
      masters: encodeBestdoriRunLength(entries.map(([, card]) => card.master)),
      skills: encodeBestdoriRunLength(entries.map(([, card]) => card.skill)),
      eps: encodeBestdoriRunLength(entries.map(([, card]) => card.episodes)),
      trains: encodeBestdoriRunLength(entries.map(([, card]) => card.training)),
      arts: encodeBestdoriRunLength(entries.map(([, card]) => card.art)),
      excludes: encodeBestdoriRunLength(entries.map(([, card]) => card.exclude)),
    };
  }

  function compressBestdoriAreaItems(areaItem = {}) {
    const items = {};
    for (const [name, areaItemIds] of Object.entries(BESTDORI_PROFILE_AREA_ITEM_IDS)) {
      const levels = [];
      areaItemIds.forEach((areaItemId) => {
        if (!recordWithFix('areaItems', 'areaItemsFix', areaItemId)) {
          levels.push(null);
          return;
        }
        const raw = integerOrZero(areaItem?.[String(areaItemId)]?.level);
        const capped = Math.min(raw, maxAreaItemLevel(areaItemId));
        levels.push(Math.max(0, capped - 1));
      });
      items[name] = encodeBestdoriRunLength(levels);
    }
    return items;
  }

  function collectCharacterPotentialPoints(characterBouns = {}, server) {
    const characterIds = Object.keys(getCharacterRecords() ?? {}).sort(numericStringSort);
    const maxPotential = bestdoriCharacterBonusPotentialMax(server);
    return characterIds.map((characterId) => {
      const bonus = normalizedCharacterBonus(characterBouns?.[characterId] ?? {});
      const potentialRate = Math.max(
        finiteNumberOrZero(bonus?.potential?.performance),
        finiteNumberOrZero(bonus?.potential?.technique),
        finiteNumberOrZero(bonus?.potential?.visual),
      );
      const taskRate = Math.max(
        finiteNumberOrZero(bonus?.characterTask?.performance),
        finiteNumberOrZero(bonus?.characterTask?.technique),
        finiteNumberOrZero(bonus?.characterTask?.visual),
      );
      const potentialPoints = Math.max(0, bestdoriRateToPoints(potentialRate));
      const taskPoints = Math.max(0, bestdoriRateToPoints(taskRate));
      const potential = Math.min(
        Math.max(potentialPoints, potentialPoints > 0 ? 2 : 0),
        maxPotential,
      );
      const task = Math.min(taskPoints, 60);
      return potential > 0 || task > 0 ? potential + task : 0;
    });
  }

  function bestdoriCharacterBonusPotentialMax(server) {
    return normalizedServer(server) === 'jp' ? 55 : 50;
  }

  function bestdoriRateToPoints(rate) {
    const number = finiteNumberOrZero(rate);
    return Math.round(number * 1000);
  }

  function bestdoriProfileCards(cards) {
    const cardList = {};
    for (const card of cards) {
      if (truthyBestdoriValue(card?.exclude)) {
        continue;
      }
      const cardId = positiveIntegerOrUndefined(card?.id);
      if (cardId == null) {
        continue;
      }
      cardList[String(cardId)] = {
        level: positiveIntegerOrDefault(card.level, maxCardLevel(cardId)),
        training: truthyBestdoriValue(card.train),
        illustTrainingStatus: truthyBestdoriValue(card.art),
        episodes: [
          Number(card.ep) >= 1,
          Number(card.ep) >= 2,
        ],
        limitBreakRank: integerOrZero(card.master),
        skillLevel: skillLevelFromBestdori(card.skill),
      };
    }
    return cardList;
  }

  function bestdoriProfileAreaItems(items = {}) {
    const areaItem = {};
    for (const [name, areaItemIds] of Object.entries(BESTDORI_PROFILE_AREA_ITEM_IDS)) {
      const levels = Array.isArray(items[name]) ? items[name] : [];
      areaItemIds.forEach((areaItemId, index) => {
        const rawLevel = Number(levels[index]);
        if (!Number.isFinite(rawLevel) || rawLevel < 0 || !recordWithFix('areaItems', 'areaItemsFix', areaItemId)) {
          return;
        }
        areaItem[String(areaItemId)] = {
          level: Math.min(Math.floor(rawLevel) + 1, maxAreaItemLevel(areaItemId)),
        };
      });
    }
    return areaItem;
  }

  function bestdoriProfileCharacterBonuses(potentials = [], server) {
    const characterBouns = {};
    const characterIds = Object.keys(getCharacterRecords() ?? {}).sort(numericStringSort);
    characterIds.forEach((characterId, index) => {
      const bonus = bestdoriCharacterBonusFromPoints(potentials[index], server);
      if (bonus.potential <= 0 && bonus.characterTask <= 0) {
        return;
      }
      characterBouns[characterId] = normalizedCharacterBonus({
        potential: {
          performance: bonus.potential,
          technique: bonus.potential,
          visual: bonus.potential,
        },
        characterTask: {
          performance: bonus.characterTask,
          technique: bonus.characterTask,
          visual: bonus.characterTask,
        },
      });
    });
    return characterBouns;
  }

  return {
    bestdoriProfileToPlayerConfig,
    playerToBestdoriProfileExport,
    importEnabledAreaItems,
    importMainBandCards,
    importMainBandCharacterBonuses,
  };
}

export function parseBestdoriProfileExport(text) {
  let profile;
  try {
    profile = JSON.parse(text);
  } catch (error) {
    throw new Error(`Bestdori Profile JSON 解析失败：${error.message}`);
  }
  if (!profile || typeof profile !== 'object' || Array.isArray(profile)) {
    throw new Error('Bestdori Profile 必须是 JSON 对象');
  }

  if (profile.compression) {
    profile = {
      ...profile,
      ...decompressBestdoriProfile(profile.compression, profile.data),
    };
    delete profile.compression;
    delete profile.data;
  }

  if (!Array.isArray(profile.cards)) {
    throw new Error('Bestdori Profile 缺少 cards 数组');
  }
  profile.items ??= {};
  return profile;
}

function decompressBestdoriProfile(compression, data) {
  switch (String(compression)) {
    case '2':
      return decompressBestdoriProfileV2(data);
    case '1':
      throw new Error('暂不支持 Bestdori compression 1 旧格式');
    default:
      throw new Error(`不支持的 Bestdori Profile compression：${compression}`);
  }
}

function decompressBestdoriProfileV2(data) {
  const cards = data?.cards;
  const items = data?.items ?? {};
  if (!cards?.ids) {
    throw new Error('Bestdori Profile 压缩数据缺少卡牌 ID');
  }

  const ids = decodeBestdoriCardIds(cards.ids);
  const levels = decodeBestdoriRunLength(cards.levels);
  const masters = decodeBestdoriRunLength(cards.masters);
  const skills = decodeBestdoriRunLength(cards.skills);
  const eps = decodeBestdoriRunLength(cards.eps);
  const trains = decodeBestdoriRunLength(cards.trains);
  const arts = decodeBestdoriRunLength(cards.arts);
  const excludes = decodeBestdoriRunLength(cards.excludes);
  const decodedCards = ids.map((id, index) => ({
    id,
    level: levels[index],
    master: masters[index],
    skill: skills[index],
    ep: eps[index],
    train: trains[index],
    art: arts[index],
    exclude: excludes[index],
  }));

  for (const [name, values] of Object.entries({
    levels,
    masters,
    skills,
    eps,
    trains,
    arts,
    excludes,
  })) {
    if (values.length !== ids.length) {
      throw new Error(`Bestdori Profile 卡牌 ${name} 数量不匹配`);
    }
  }

  return {
    cards: decodedCards,
    items: {
      PoppinParty: decodeBestdoriRunLength(items.PoppinParty),
      Afterglow: decodeBestdoriRunLength(items.Afterglow),
      HelloHappyWorld: decodeBestdoriRunLength(items.HelloHappyWorld),
      PastelPalettes: decodeBestdoriRunLength(items.PastelPalettes),
      Roselia: decodeBestdoriRunLength(items.Roselia),
      RaiseASuilen: decodeBestdoriRunLength(items.RaiseASuilen),
      Morfonica: decodeBestdoriRunLength(items.Morfonica),
      MyGO: decodeBestdoriRunLength(items.MyGO),
      Everyone: decodeBestdoriRunLength(items.Everyone),
      Magazine: decodeBestdoriRunLength(items.Magazine),
      Plaza: decodeBestdoriRunLength(items.Plaza),
      Menu: decodeBestdoriRunLength(items.Menu),
      potentials: decodeBestdoriRunLength(items.potentials),
    },
  };
}

function decodeBestdoriCardIds(value) {
  const binary = atob(String(value));
  const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
  if (bytes.byteLength % 2 !== 0) {
    throw new Error('Bestdori Profile 卡牌 ID 数据长度无效');
  }

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const ids = [];
  for (let offset = 0; offset < bytes.byteLength; offset += 2) {
    const id = view.getUint16(offset, true);
    ids.push(id > 24464 ? id + 65536 : id);
  }
  return ids;
}

function decodeBestdoriRunLength(values) {
  const encoded = Array.isArray(values) ? values : [];
  if (encoded.length % 2 !== 0) {
    throw new Error('Bestdori Profile 游程编码长度无效');
  }

  const decoded = [];
  for (let index = 0; index < encoded.length; index += 2) {
    const count = Number(encoded[index]);
    const value = encoded[index + 1];
    if (!Number.isInteger(count) || count < 0) {
      throw new Error(`Bestdori Profile 游程编码次数无效：${encoded[index]}`);
    }
    for (let repeated = 0; repeated < count; repeated += 1) {
      decoded.push(value);
    }
  }
  return decoded;
}

function encodeBestdoriRunLength(values) {
  const source = Array.isArray(values) ? values : [];
  if (source.length === 0) {
    return [];
  }

  const encoded = [];
  let current = normalizeBestdoriRunLengthValue(source[0]);
  let count = 1;
  for (let index = 1; index < source.length; index += 1) {
    const value = normalizeBestdoriRunLengthValue(source[index]);
    if (Object.is(value, current)) {
      count += 1;
      continue;
    }
    encoded.push(count, current);
    current = value;
    count = 1;
  }
  encoded.push(count, current);
  return encoded;
}

function normalizeBestdoriRunLengthValue(value) {
  if (value === null || value === undefined) {
    return null;
  }
  if (typeof value === 'boolean') {
    return value;
  }
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return 0;
  }
  if (Number.isInteger(number)) {
    return number;
  }
  return Math.trunc(number);
}

function encodeBestdoriCardIds(cardIds) {
  const source = Array.isArray(cardIds) ? cardIds : [];
  const bytes = new Uint8Array(source.length * 2);
  const view = new DataView(bytes.buffer);
  for (let index = 0; index < source.length; index += 1) {
    const cardId = Number(source[index]);
    const encoded = Number.isInteger(cardId) && cardId > 24464 ? cardId - 65536 : cardId;
    view.setUint16(index * 2, encoded, true);
  }
  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}

function appendRate(value, base) {
  const number = finiteNumberOrZero(value);
  const denominator = finiteNumberOrZero(base);
  return denominator > 0 ? Math.ceil((1000 * number) / denominator) / 1000 : 0;
}

function truthyBestdoriValue(value) {
  return value === true || value === 1 || value === '1';
}

function skillLevelFromBestdori(value) {
  const raw = Number(value);
  if (!Number.isFinite(raw)) {
    return MIN_SKILL_LEVEL;
  }
  return Math.min(MAX_SKILL_LEVEL, Math.max(MIN_SKILL_LEVEL, Math.trunc(raw) + 1));
}

function skillLevelToBestdori(value) {
  const level = positiveIntegerOrDefault(value, MIN_SKILL_LEVEL);
  return Math.max(0, Math.min(MAX_SKILL_LEVEL, level) - 1);
}

function mainDeckEntries(profile) {
  const entries = profile?.mainDeckUserSituations?.entries;
  return Array.isArray(entries) ? entries : [];
}
