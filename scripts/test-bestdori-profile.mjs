import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

import {
  createBestdoriProfileImporter,
  parseBestdoriProfileExport,
} from '../apps/web/src/data/bestdori.js';

function normalizedPlayer(player = {}) {
  return {
    server: player.server ?? 'cn',
    cardList: player.cardList ?? {},
    characterBouns: player.characterBouns ?? {},
    areaItem: player.areaItem ?? {},
  };
}

function normalizedCharacterBonus(bonus = {}) {
  return {
    potential: bonus.potential ?? {},
    characterTask: bonus.characterTask ?? {},
  };
}

function normalizedStatRate(rate = {}) {
  return {
    performance: Number(rate.performance) || 0,
    technique: Number(rate.technique) || 0,
    visual: Number(rate.visual) || 0,
  };
}

const importer = createBestdoriProfileImporter({
  normalizedPlayer,
  normalizedServer: (server) => server ?? 'cn',
  normalizedCharacterBonus,
  normalizedStatRate,
  recordWithFix: () => ({}),
  maxCardLevel: () => 60,
  maxAreaItemLevel: () => 6,
  cardCharacterId: () => 1,
  getCharacterRecords: () => ({}),
  bestdoriCharacterBonusFromPoints: () => ({ potential: 0, characterTask: 0 }),
});

const episodeImporter = createBestdoriProfileImporter({
  normalizedPlayer,
  normalizedServer: (server) => server ?? 'jp',
  normalizedCharacterBonus,
  normalizedStatRate,
  recordWithFix: (_type, _fixType, cardId) => ({
    rarity: Number(cardId) === 202 ? 5 : 4,
    stat: {
      training: { performance: 400, technique: 400, visual: 400 },
      episodes: [
        { performance: 250, technique: 250, visual: 250 },
        { performance: 600, technique: 600, visual: 600 },
      ],
    },
  }),
  maxCardLevel: () => 60,
  maxAreaItemLevel: () => 8,
  cardCharacterId: () => 1,
  getCharacterRecords: () => ({}),
  bestdoriCharacterBonusFromPoints: () => ({ potential: 0, characterTask: 0 }),
});

{
  const player = importer.bestdoriProfileToPlayerConfig({
    server: 3,
    cards: [
      { id: 101, level: 60, skill: 4, master: 0, ep: 2, train: 1, art: 1 },
      { id: 102, level: 60, skill: 0, master: 0, ep: 2, train: 1, art: 1 },
    ],
    items: {},
  });

  assert.equal(player.cardList['101'].skillLevel, 5);
  assert.equal(player.cardList['102'].skillLevel, 1);
}

{
  const player = normalizedPlayer();
  importer.importMainBandCards(player, {
    mainDeckUserSituations: {
      entries: [
        {
          situationId: 201,
          level: 60,
          skillLevel: 4,
          trainingStatus: 'done',
          illust: 'after_training',
          limitBreakRank: 0,
        },
      ],
    },
  });

  assert.equal(player.cardList['201'].skillLevel, 5);
}

{
  const player = normalizedPlayer();
  episodeImporter.importMainBandCards(player, {
    mainDeckUserSituations: {
      entries: [
        {
          situationId: 201,
          trainingStatus: 'done',
          limitBreakRank: 0,
          userAppendParameter: { performance: 400, technique: 400, visual: 400 },
        },
        {
          situationId: 202,
          trainingStatus: 'done',
          limitBreakRank: 1,
          userAppendParameter: { performance: 900, technique: 900, visual: 900 },
        },
        {
          situationId: 203,
          trainingStatus: 'done',
          limitBreakRank: 0,
          userAppendParameter: { performance: 1250, technique: 1250, visual: 1250 },
        },
      ],
    },
  });

  assert.deepEqual(player.cardList['201'].episodes, [false, false]);
  assert.deepEqual(player.cardList['202'].episodes, [true, false]);
  assert.deepEqual(player.cardList['203'].episodes, [true, true]);
}

{
  const exported = importer.playerToBestdoriProfileExport({
    server: 'cn',
    cardList: {
      301: {
        level: 60,
        training: true,
        illustTrainingStatus: true,
        episodes: [true, true],
        limitBreakRank: 0,
        skillLevel: 5,
      },
    },
  });
  const parsed = parseBestdoriProfileExport(JSON.stringify(exported));

  assert.equal(parsed.cards[0].skill, 4);
}

const heavyFixtureUrl = new URL('./fixtures/bestdori-profile-323-heavy.json', import.meta.url);

if (existsSync(heavyFixtureUrl)) {
  const profileText = readFileSync(heavyFixtureUrl, 'utf8');
  const rawProfile = JSON.parse(profileText);
  const bestdoriProfile = parseBestdoriProfileExport(profileText);
  const player = importer.bestdoriProfileToPlayerConfig(bestdoriProfile);
  const lastCard = bestdoriProfile.cards[bestdoriProfile.cards.length - 1];

  assert.equal(rawProfile.name, '新配置');
  assert.equal(rawProfile.server, 3);
  assert.equal(bestdoriProfile.cards.length, 1396);
  assert.equal(bestdoriProfile.cards[0].id, 1);
  assert.equal(bestdoriProfile.cards[0].skill, 4);
  assert.equal(lastCard.id, 10045);

  assert.equal(player.server, 'cn');
  assert.equal(Object.keys(player.cardList).length, 1396);
  assert.equal(Object.keys(player.areaItem).length, 74);
  assert.equal(player.areaItem['59'], undefined);
  assert.equal(player.areaItem['68'], undefined);
  assert.equal(player.areaItem['72'], undefined);
  assert.equal(player.cardList['1'].skillLevel, 5);
  assert.equal(player.cardList['1396'].skillLevel, 2);

  const playerWithBase = importer.bestdoriProfileToPlayerConfig(bestdoriProfile, {
    areaItem: {
      59: { level: 0 },
      68: { level: 0 },
      72: { level: 0 },
    },
  });

  assert.deepEqual(playerWithBase.areaItem['59'], { level: 0 });
  assert.deepEqual(playerWithBase.areaItem['68'], { level: 0 });
  assert.deepEqual(playerWithBase.areaItem['72'], { level: 0 });
}
