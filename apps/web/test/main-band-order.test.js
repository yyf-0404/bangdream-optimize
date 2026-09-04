import assert from 'node:assert/strict';
import test from 'node:test';

import { mainBandCardIds } from '../src/data/bestdori.js';

function profileWithMainBand() {
  return {
    mainUserDeck: {
      leader: 30,
      member1: 10,
      member2: 20,
      member3: 3,
      member4: 40,
    },
    mainDeckUserSituations: {
      entries: [3, 10, 20, 30, 40].map((situationId) => ({ situationId })),
    },
  };
}

test('main band maps Bestdori fields to the fixed five display slots', () => {
  assert.deepEqual(mainBandCardIds(profileWithMainBand()), [3, 10, 30, 20, 40]);
});

test('main band requires explicit deck fields and matching card details', () => {
  const missingField = profileWithMainBand();
  delete missingField.mainUserDeck.leader;
  assert.throws(() => mainBandCardIds(missingField), /leader/);

  const missingDetail = profileWithMainBand();
  missingDetail.mainDeckUserSituations.entries =
    missingDetail.mainDeckUserSituations.entries.filter((entry) => entry.situationId !== 30);
  assert.throws(() => mainBandCardIds(missingDetail), /卡牌 30 的详细配置/);
});
