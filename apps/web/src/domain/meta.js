import {
  cardIconUrls as buildCardIconUrls,
  cardTrainingStatusList as cardTrainingStatusListForCard,
  normalizeTrainingStatus,
  songCoverUrls as buildSongCoverUrls,
} from '../assets/index.js?v=1';
import {
  compactJoin,
  hasText,
  normalizedAttribute,
  positiveIntegerOrUndefined,
} from '../utils.js?v=1';

export function createGameMeta({
  getCore,
  serverIndex,
}) {
  function cardLabel(cardId) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    if (!card) {
      return `卡牌 ${cardId}`;
    }
    return compactJoin([
      localizedText(card.prefix, '未命名卡牌'),
      characterLabel(card.characterId),
      card.rarity == null ? undefined : `${card.rarity}星`,
      card.attribute,
    ]);
  }

  function cardName(cardId) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    return localizedText(card?.prefix, `卡牌 ${cardId}`);
  }

  function cardRarity(cardId) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    const rarity = Number(card?.rarity);
    return Number.isInteger(rarity) && rarity > 0 ? rarity : 0;
  }

  function songLabel(songId) {
    const song = getCore()?.songs?.[String(songId)];
    return localizedText(song?.musicTitle, `歌曲 ${songId}`);
  }

  function eventLabel(eventId, player) {
    if (Number(eventId) === 0) {
      return '自定义活动';
    }
    const event = player?.eventPresets?.[String(eventId)]
      ?? getCore()?.events?.[String(eventId)];
    const name = localizedText(event?.eventName, `活动 ${eventId}`);
    return compactJoin([name]);
  }

  function areaItemLabel(areaItemId) {
    const areaItem = recordWithFix('areaItems', 'areaItemsFix', areaItemId);
    return localizedText(areaItem?.areaItemName, `区域道具 ${areaItemId}`);
  }

  function characterLabel(characterId) {
    const character = getCore()?.characters?.[String(characterId)];
    if (!character) {
      return `角色 ${characterId}`;
    }

    const name = localizedText(character.characterName, '');
    const nickname = localizedText(character.nickname, '');
    if (nickname && name && nickname !== name) {
      return `${nickname} (${name})`;
    }
    return name || nickname || `角色 ${characterId}`;
  }

  function cardCharacterId(cardId) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    const characterId = Number(card?.characterId);
    return Number.isInteger(characterId) && characterId > 0 ? characterId : undefined;
  }

  function cardAttribute(cardId) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    return normalizedAttribute(card?.attribute);
  }

  function cardEpisodeAvailable(cardId, index) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    const episodes = card?.stat?.episodes;
    const episode = Array.isArray(episodes) ? episodes[index] : undefined;
    return episode != null;
  }

  function cardEpisodeAlwaysRead(cardId, index) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    const episodes = card?.stat?.episodes;
    const episode = Array.isArray(episodes) ? episodes[index] : undefined;
    if (episode == null) {
      return false;
    }
    return Number(episode?.performance) === 0
      && Number(episode?.technique) === 0
      && Number(episode?.visual) === 0;
  }

  function cardIconUrls(cardId, { illustTrainingStatus = false } = {}) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    return buildCardIconUrls({ cardId, card, illustTrainingStatus });
  }

  function songCoverUrls(songId) {
    const song = getCore()?.songs?.[String(songId)];
    return buildSongCoverUrls({ songId, song });
  }

  function selectedBandId(value) {
    return positiveIntegerOrUndefined(value) ?? {
      PoppinParty: 1,
      Afterglow: 2,
      HelloHappyWorld: 3,
      PastelPalettes: 4,
      Roselia: 5,
      RaiseASuilen: 18,
      Morfonica: 21,
      MyGO: 45,
      AveMujica: 50,
      Everyone: 1000,
    }[value];
  }

  function maxAreaItemLevel(areaItemId) {
    const areaItem = recordWithFix('areaItems', 'areaItemsFix', areaItemId);
    const levels = Object.keys(areaItem?.performance ?? {})
      .map(Number)
      .filter(Number.isInteger);
    return levels.length > 0 ? Math.max(...levels) : 255;
  }

  function maxCardLevel(cardId) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    const levels = Object.keys(card?.stat ?? {})
      .map(Number)
      .filter(Number.isInteger);
    return levels.length > 0 ? Math.max(...levels) : 60;
  }

  function cardTrainingStatusList(cardId) {
    const card = recordWithFix('cards', 'cardsFix', cardId);
    return cardTrainingStatusListForCard(card);
  }

  function normalizeCardTrainingStatus(cardId, value) {
    return normalizeTrainingStatus(cardTrainingStatusList(cardId), value);
  }

  function recordWithFix(key, fixKey, id) {
    const stringId = String(id);
    const core = getCore();
    return core?.[key]?.[stringId] ?? core?.[fixKey]?.[stringId];
  }

  function localizedText(value, fallback) {
    if (Array.isArray(value)) {
      const preferred = value[serverIndex()];
      if (hasText(preferred)) {
        return String(preferred);
      }
      const first = value.find(hasText);
      return first == null ? fallback : String(first);
    }
    if (hasText(value)) {
      return String(value);
    }
    return fallback;
  }

  function serverScopedValue(value) {
    if (!Array.isArray(value)) {
      return value;
    }
    const preferred = value[serverIndex()];
    if (preferred != null) {
      return preferred;
    }
    return value.find((item) => item != null);
  }

  function eventDateRange(event) {
    const start = parseEventDate(serverScopedValue(event?.startAt));
    const end = parseEventDate(serverScopedValue(event?.endAt));
    if (!start && !end) {
      return undefined;
    }
    return compactJoin([start, end], ' - ');
  }

  return {
    areaItemLabel,
    cardAttribute,
    cardEpisodeAlwaysRead,
    cardEpisodeAvailable,
    cardCharacterId,
    cardIconUrls,
    cardLabel,
    cardName,
    cardRarity,
    cardTrainingStatusList,
    characterLabel,
    eventDateRange,
    eventLabel,
    localizedText,
    maxAreaItemLevel,
    maxCardLevel,
    normalizeCardTrainingStatus,
    recordWithFix,
    selectedBandId,
    serverScopedValue,
    songCoverUrls,
    songLabel,
  };
}

function parseEventDate(value) {
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) {
    return undefined;
  }
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).format(new Date(number));
}
