import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { GameDataClient } from '../src/data/game-sync.js';

test('product payload builders use repaired levels and fetch other missing details', async () => {
  const cardsFix = JSON.parse(await readFile(
    new URL('../../../var/game-data/cardsCNfix.json', import.meta.url), 'utf8',
  ));
  const original = { stat: { 1: { performance: 1, technique: 2, visual: 3 } } };
  const core = {
    cards: { 10000: original, 10005: original, 10011: original, 10012: original,
      10013: original, 10029: original, 7: original }, cardsFix,
    characters: {}, skills: {}, areaItems: {}, songs: {},
  };
  const player = {
    currentEvent: 0, eventOverrides: { 0: { eventType: 'medley' } }, eventSongs: { 0: [] },
    cardList: { 10000: { level: 7 }, 10005: { level: 15 }, 10011: { level: 15 },
      10012: { level: 23 }, 10013: { level: 29 }, 10029: { level: 37 }, 7: { level: 2 } },
  };
  for (const method of ['buildCalculationPayload', 'buildScoreRangePayload',
    'buildPtMaximizePayload', 'buildPtEvaluatePayload']) {
    const client = Object.create(GameDataClient.prototype);
    const fetched = [];
    client.syncCardDetail = async (id) => {
      fetched.push(id);
      return { stat: { 2: { performance: 4, technique: 5, visual: 6 } } };
    };
    client.syncScoreRangeChartMeta = async () => ({});
    const result = await client[method]({
      player, server: 3, eventId: 0, core, options: {}, request: { songs: [] },
    });
    assert.deepEqual(fetched, ['7'], method);
    assert.equal(result.cardsFix, cardsFix, method);
    assert.equal(result.cardsFix['10005'].stat['15'].performance, 2935);
    assert.equal(result.cardsFix['10029'].stat['37'].performance, 5359);
    for (const id of ['10000', '10011', '10012', '10013']) {
      const level = String(player.cardList[id].level);
      assert.ok(result.cardsFix[id].stat[level], `${method}: repaired card ${id} level ${level}`);
    }
    assert.equal(result.cards['7'].stat['2'].performance, 4);
    assert.equal(core.cards['7'], original);
    assert.equal(original.stat['2'], undefined);
  }
  const client = Object.create(GameDataClient.prototype);
  const fetched = [];
  client.syncCardDetail = async (id) => {
    fetched.push(id);
    return { stat: { 15: { performance: 4, technique: 5, visual: 6 } } };
  };
  await client.cardsWithRequestedDetails(core.cards, { cardList: { 10005: { level: 15 } } });
  assert.deepEqual(fetched, ['10005'], 'no-repair requests retain their existing fetch behavior');
});
