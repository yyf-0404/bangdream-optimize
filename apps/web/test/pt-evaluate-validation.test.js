import assert from 'node:assert/strict';
import test from 'node:test';

import { validatePtEvaluateTeamSelection } from '../src/models/pt-evaluate-validation.js';

const characters = new Map([
  [1, 101],
  [2, 102],
  [3, 103],
  [4, 104],
  [5, 105],
  [6, 106],
  [7, 107],
  [8, 108],
  [9, 109],
  [10, 110],
  [11, 111],
  [12, 112],
  [13, 113],
  [14, 114],
  [15, 115],
  [16, 101],
]);
const cardCharacterId = (cardId) => characters.get(cardId);
const player = {
  cardList: Object.fromEntries(Array.from(characters.keys(), (cardId) => [cardId, {}])),
};

function team(cardIds) {
  return { cardIds, captainCardId: cardIds[2] };
}

test('specified-team preflight accepts a legal solo team', () => {
  assert.doesNotThrow(() => validatePtEvaluateTeamSelection(
    player,
    { liveVariant: 'solo', teams: [team([1, 2, 3, 4, 5])] },
    cardCharacterId,
  ));
});

test('specified-team preflight rejects duplicate cards and characters', () => {
  assert.throws(
    () => validatePtEvaluateTeamSelection(
      player,
      { liveVariant: 'solo', teams: [team([1, 1, 3, 4, 5])] },
      cardCharacterId,
    ),
    /不能重复使用同一张卡牌/,
  );
  assert.throws(
    () => validatePtEvaluateTeamSelection(
      player,
      { liveVariant: 'solo', teams: [team([1, 2, 3, 4, 16])] },
      cardCharacterId,
    ),
    /必须由五个不同角色组成/,
  );
});

test('specified-team preflight rejects missing cards and medley reuse', () => {
  assert.throws(
    () => validatePtEvaluateTeamSelection(
      player,
      { liveVariant: 'solo', teams: [team([1, 2, 3, 4, 99])] },
      cardCharacterId,
    ),
    /不在当前配置中/,
  );
  assert.throws(
    () => validatePtEvaluateTeamSelection(
      player,
      {
        liveVariant: 'medley',
        teams: [
          team([1, 2, 3, 4, 5]),
          team([6, 7, 8, 9, 10]),
          team([11, 12, 13, 14, 1]),
        ],
      },
      cardCharacterId,
    ),
    /不能重复使用卡牌 1/,
  );
});
